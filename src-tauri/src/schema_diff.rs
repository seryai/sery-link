//! Pure schema-diff logic.
//!
//! Given two parsed schemas (old and new) for the same dataset, compute
//! what changed: which columns were added, which were removed, and which
//! had their type altered. Type-change detection is the most interesting
//! case — a common real-world source of downstream breakage is a column
//! silently morphing from INTEGER to VARCHAR when a CSV picks up a
//! non-numeric row.
//!
//! This module is deliberately I/O-free: no DuckDB, no file reads, no
//! serialization. The caller supplies already-parsed `Vec<Column>` values
//! and gets back a `SchemaDiff`. That keeps the logic fast, testable,
//! and reusable across the scanner, the cache upsert path, and any
//! future "did anything change" health check.
//!
//! Consumed by: metadata_cache::upsert_dataset (computes diff at upsert
//! time so the UI can surface unread schema changes in the Fleet view).

use serde::{Deserialize, Serialize};

/// One parsed column from a dataset's schema.
///
/// Intentionally not the same struct as scanner::ColumnSchema —
/// this module only needs name + type, and decoupling it keeps
/// schema_diff consumable from tests without pulling DuckDB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub column_type: String,
}

/// What happened to a single column between `old` and `new`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ColumnChange {
    /// Column exists in `new` but not in `old`.
    Added { name: String, column_type: String },
    /// Column exists in `old` but not in `new`.
    Removed { name: String, column_type: String },
    /// Column exists in both but the type differs. Renames look like a
    /// removed + added pair rather than a type change; we can't tell
    /// a rename from a drop-and-replace without more context.
    TypeChanged {
        name: String,
        old_type: String,
        new_type: String,
    },
}

/// The full set of changes between two schemas.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaDiff {
    pub changes: Vec<ColumnChange>,
}

impl SchemaDiff {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn added(&self) -> usize {
        self.changes
            .iter()
            .filter(|c| matches!(c, ColumnChange::Added { .. }))
            .count()
    }

    pub fn removed(&self) -> usize {
        self.changes
            .iter()
            .filter(|c| matches!(c, ColumnChange::Removed { .. }))
            .count()
    }

    pub fn type_changed(&self) -> usize {
        self.changes
            .iter()
            .filter(|c| matches!(c, ColumnChange::TypeChanged { .. }))
            .count()
    }
}

/// Compute the diff between two schemas.
///
/// Column identity is by name — reorderings alone produce no diff, which
/// is the right behavior because column order is cosmetic in DuckDB/SQL
/// for every relevant read path. Name comparison is case-sensitive to
/// match DuckDB's default (quote-aware identifiers).
///
/// Output ordering: Added first, then Removed, then TypeChanged, with
/// alphabetical order within each group. Stable ordering keeps diffs
/// readable when displayed in the UI.
pub fn diff_schemas(old: &[Column], new: &[Column]) -> SchemaDiff {
    use std::collections::HashMap;

    let old_by_name: HashMap<&str, &str> = old
        .iter()
        .map(|c| (c.name.as_str(), c.column_type.as_str()))
        .collect();
    let new_by_name: HashMap<&str, &str> = new
        .iter()
        .map(|c| (c.name.as_str(), c.column_type.as_str()))
        .collect();

    let mut added: Vec<ColumnChange> = Vec::new();
    let mut removed: Vec<ColumnChange> = Vec::new();
    let mut type_changed: Vec<ColumnChange> = Vec::new();

    for col in new {
        match old_by_name.get(col.name.as_str()) {
            None => added.push(ColumnChange::Added {
                name: col.name.clone(),
                column_type: col.column_type.clone(),
            }),
            Some(old_type) if *old_type != col.column_type => {
                type_changed.push(ColumnChange::TypeChanged {
                    name: col.name.clone(),
                    old_type: (*old_type).to_string(),
                    new_type: col.column_type.clone(),
                });
            }
            Some(_) => {}
        }
    }

    for col in old {
        if !new_by_name.contains_key(col.name.as_str()) {
            removed.push(ColumnChange::Removed {
                name: col.name.clone(),
                column_type: col.column_type.clone(),
            });
        }
    }

    added.sort_by(|a, b| change_name(a).cmp(change_name(b)));
    removed.sort_by(|a, b| change_name(a).cmp(change_name(b)));
    type_changed.sort_by(|a, b| change_name(a).cmp(change_name(b)));

    let mut changes = Vec::with_capacity(added.len() + removed.len() + type_changed.len());
    changes.extend(added);
    changes.extend(removed);
    changes.extend(type_changed);

    SchemaDiff { changes }
}

fn change_name(c: &ColumnChange) -> &str {
    match c {
        ColumnChange::Added { name, .. }
        | ColumnChange::Removed { name, .. }
        | ColumnChange::TypeChanged { name, .. } => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: &str) -> Column {
        Column {
            name: name.into(),
            column_type: ty.into(),
        }
    }

    #[test]
    fn identical_schemas_produce_empty_diff() {
        let schema = vec![col("id", "INTEGER"), col("name", "VARCHAR")];
        let d = diff_schemas(&schema, &schema);
        assert!(d.is_empty());
        assert_eq!(d.added(), 0);
        assert_eq!(d.removed(), 0);
        assert_eq!(d.type_changed(), 0);
    }

    #[test]
    fn reordering_alone_is_not_a_change() {
        let old = vec![col("id", "INTEGER"), col("name", "VARCHAR")];
        let new = vec![col("name", "VARCHAR"), col("id", "INTEGER")];
        assert!(diff_schemas(&old, &new).is_empty());
    }

    #[test]
    fn detects_added_column() {
        let old = vec![col("id", "INTEGER")];
        let new = vec![col("id", "INTEGER"), col("email", "VARCHAR")];
        let d = diff_schemas(&old, &new);
        assert_eq!(d.added(), 1);
        assert_eq!(d.removed(), 0);
        assert_eq!(d.type_changed(), 0);
        assert!(matches!(
            &d.changes[0],
            ColumnChange::Added { name, .. } if name == "email"
        ));
    }

    #[test]
    fn detects_removed_column() {
        let old = vec![col("id", "INTEGER"), col("legacy_field", "BOOLEAN")];
        let new = vec![col("id", "INTEGER")];
        let d = diff_schemas(&old, &new);
        assert_eq!(d.removed(), 1);
        assert!(matches!(
            &d.changes[0],
            ColumnChange::Removed { name, .. } if name == "legacy_field"
        ));
    }

    #[test]
    fn detects_type_change() {
        let old = vec![col("amount", "INTEGER")];
        let new = vec![col("amount", "VARCHAR")];
        let d = diff_schemas(&old, &new);
        assert_eq!(d.type_changed(), 1);
        assert_eq!(d.added(), 0);
        assert_eq!(d.removed(), 0);
        match &d.changes[0] {
            ColumnChange::TypeChanged {
                name,
                old_type,
                new_type,
            } => {
                assert_eq!(name, "amount");
                assert_eq!(old_type, "INTEGER");
                assert_eq!(new_type, "VARCHAR");
            }
            _ => panic!("expected TypeChanged"),
        }
    }

    #[test]
    fn combined_add_remove_and_type_change() {
        let old = vec![
            col("id", "INTEGER"),
            col("name", "VARCHAR"),
            col("amount", "INTEGER"),
            col("legacy_flag", "BOOLEAN"),
        ];
        let new = vec![
            col("id", "INTEGER"),
            col("name", "VARCHAR"),
            col("amount", "VARCHAR"),   // type changed
            col("email", "VARCHAR"),    // added
                                        // legacy_flag removed
        ];
        let d = diff_schemas(&old, &new);
        assert_eq!(d.added(), 1);
        assert_eq!(d.removed(), 1);
        assert_eq!(d.type_changed(), 1);
    }

    #[test]
    fn rename_shows_as_remove_plus_add() {
        // We can't distinguish a rename from a drop+replace from the
        // schema alone. The test pins this intentional behavior.
        let old = vec![col("user_name", "VARCHAR")];
        let new = vec![col("username", "VARCHAR")];
        let d = diff_schemas(&old, &new);
        assert_eq!(d.added(), 1);
        assert_eq!(d.removed(), 1);
    }

    #[test]
    fn changes_sort_alphabetically_within_their_group() {
        // Multiple adds: should come back in alphabetical order.
        let old = vec![col("id", "INTEGER")];
        let new = vec![
            col("id", "INTEGER"),
            col("zulu", "VARCHAR"),
            col("alpha", "VARCHAR"),
            col("mike", "VARCHAR"),
        ];
        let d = diff_schemas(&old, &new);
        let names: Vec<&str> = d
            .changes
            .iter()
            .filter_map(|c| match c {
                ColumnChange::Added { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["alpha", "mike", "zulu"]);
    }

    #[test]
    fn name_comparison_is_case_sensitive() {
        let old = vec![col("ID", "INTEGER")];
        let new = vec![col("id", "INTEGER")];
        let d = diff_schemas(&old, &new);
        // Different cases = different columns = one remove + one add.
        assert_eq!(d.added(), 1);
        assert_eq!(d.removed(), 1);
    }

    #[test]
    fn empty_to_populated_is_all_adds() {
        let old: Vec<Column> = vec![];
        let new = vec![col("id", "INTEGER"), col("name", "VARCHAR")];
        let d = diff_schemas(&old, &new);
        assert_eq!(d.added(), 2);
        assert_eq!(d.removed(), 0);
    }

    #[test]
    fn populated_to_empty_is_all_removes() {
        let old = vec![col("id", "INTEGER"), col("name", "VARCHAR")];
        let new: Vec<Column> = vec![];
        let d = diff_schemas(&old, &new);
        assert_eq!(d.added(), 0);
        assert_eq!(d.removed(), 2);
    }
}

//! Dataset relationship detection — analyzes schema patterns and query history
//! to discover foreign key relationships between datasets.
//!
//! Two detection strategies:
//! 1. **Schema-based**: Detects FK patterns (columns ending in _id, id columns)
//! 2. **Query-based**: Extracts JOIN patterns from query history
//!
//! Relationships are scored by confidence (0-100):
//! - 100: Found in both schema and query history
//! - 80: Found in query history (user explicitly joined these)
//! - 60: Strong schema pattern (id → other_table_id)
//! - 40: Weak schema pattern (name similarity)

use crate::error::Result;
use crate::history::{self, QueryHistoryEntry};
use crate::metadata_cache::{MetadataCache, CachedDataset};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetRelationship {
    /// Source dataset ID
    pub source_id: String,
    /// Target dataset ID
    pub target_id: String,
    /// Source dataset name (for display)
    pub source_name: String,
    /// Target dataset name (for display)
    pub target_name: String,
    /// Column in source dataset
    pub source_column: String,
    /// Column in target dataset
    pub target_column: String,
    /// Confidence score (0-100)
    pub confidence: u8,
    /// How this relationship was detected
    pub detection_method: String,
}

#[derive(Debug, Deserialize)]
struct SchemaColumn {
    name: String,
    #[serde(rename = "type")]
    column_type: Option<String>,
}

/// Main entry point: detect all relationships for a workspace
pub fn detect_relationships(workspace_id: &str) -> Result<Vec<DatasetRelationship>> {
    let cache = MetadataCache::new()?;
    let datasets = cache.get_all(workspace_id)?;

    if datasets.is_empty() {
        return Ok(Vec::new());
    }

    // Parse all schemas into a usable format
    let mut parsed_schemas: HashMap<String, Vec<SchemaColumn>> = HashMap::new();
    for dataset in &datasets {
        if let Some(schema_json) = &dataset.schema_json {
            if let Ok(columns) = serde_json::from_str::<Vec<SchemaColumn>>(schema_json) {
                parsed_schemas.insert(dataset.id.clone(), columns);
            }
        }
    }

    let mut relationships = Vec::new();

    // Strategy 1: Schema-based detection
    relationships.extend(detect_schema_relationships(&datasets, &parsed_schemas));

    // Strategy 2: Query history-based detection
    if let Ok(history) = history::load_history(500) {
        relationships.extend(detect_query_relationships(&datasets, &history, &parsed_schemas));
    }

    // Merge duplicates and boost confidence for relationships found by multiple methods
    let merged = merge_relationships(relationships);

    Ok(merged)
}

/// Detect relationships by analyzing schema patterns
fn detect_schema_relationships(
    datasets: &[CachedDataset],
    parsed_schemas: &HashMap<String, Vec<SchemaColumn>>,
) -> Vec<DatasetRelationship> {
    let mut relationships = Vec::new();

    // Build a lookup map: table_name -> dataset
    let mut name_to_dataset: HashMap<String, &CachedDataset> = HashMap::new();
    for dataset in datasets {
        // Extract table name from filename (remove extension)
        let table_name = dataset.name
            .trim_end_matches(".parquet")
            .trim_end_matches(".csv")
            .trim_end_matches(".xlsx")
            .to_lowercase();
        name_to_dataset.insert(table_name, dataset);
    }

    // For each dataset, look for FK patterns in its columns
    for dataset in datasets {
        if let Some(columns) = parsed_schemas.get(&dataset.id) {
            for column in columns {
                let col_name = column.name.to_lowercase();

                // Pattern 1: column ends with _id (e.g., user_id, order_id)
                if col_name.ends_with("_id") && col_name.len() > 3 {
                    let prefix = &col_name[..col_name.len() - 3]; // Remove "_id"

                    // Look for a dataset with matching name (users, orders, etc.)
                    let potential_targets = vec![
                        prefix.to_string(),
                        format!("{}s", prefix), // singular → plural
                        prefix.trim_end_matches('s').to_string(), // plural → singular
                    ];

                    for target_name in potential_targets {
                        if let Some(target_dataset) = name_to_dataset.get(&target_name) {
                            // Found a match! Look for 'id' column in target
                            if let Some(target_cols) = parsed_schemas.get(&target_dataset.id) {
                                if target_cols.iter().any(|c| c.name.to_lowercase() == "id") {
                                    relationships.push(DatasetRelationship {
                                        source_id: dataset.id.clone(),
                                        target_id: target_dataset.id.to_string(),
                                        source_name: dataset.name.clone(),
                                        target_name: target_dataset.name.clone(),
                                        source_column: column.name.clone(),
                                        target_column: "id".to_string(),
                                        confidence: 60,
                                        detection_method: "schema_fk_pattern".to_string(),
                                    });
                                    break; // Only create one relationship per column
                                }
                            }
                        }
                    }
                }

                // Pattern 2: column is exactly "id" — might be referenced by others
                // (handled implicitly by the _id pattern above)
            }
        }
    }

    relationships
}

/// Detect relationships by analyzing JOIN patterns in query history
fn detect_query_relationships(
    datasets: &[CachedDataset],
    history: &[QueryHistoryEntry],
    _parsed_schemas: &HashMap<String, Vec<SchemaColumn>>,
) -> Vec<DatasetRelationship> {
    let mut relationships = Vec::new();

    // Build a reverse lookup: file_path -> dataset_id
    let mut path_to_dataset: HashMap<String, String> = HashMap::new();
    for dataset in datasets {
        path_to_dataset.insert(dataset.path.clone(), dataset.id.clone());
    }

    // Build a name lookup for matching table aliases in SQL
    let mut name_to_id: HashMap<String, String> = HashMap::new();
    for dataset in datasets {
        let table_name = dataset.name
            .trim_end_matches(".parquet")
            .trim_end_matches(".csv")
            .trim_end_matches(".xlsx")
            .to_lowercase();
        name_to_id.insert(table_name, dataset.id.clone());
    }

    // Parse JOIN patterns from SQL
    for entry in history {
        if entry.status != "success" {
            continue; // Only analyze successful queries
        }

        let sql_lower = entry.sql.to_lowercase();

        // Look for JOIN keywords
        if !sql_lower.contains("join") {
            continue;
        }

        // Simple heuristic: extract table references
        // This is a simplified parser — a full SQL parser would be better but overkill
        let joins = extract_join_patterns(&sql_lower);

        for (left_table, right_table, left_col, right_col) in joins {
            // Match table names to dataset IDs
            if let (Some(left_id), Some(right_id)) = (
                name_to_id.get(&left_table),
                name_to_id.get(&right_table),
            ) {
                // Find the dataset objects
                if let (Some(left_ds), Some(right_ds)) = (
                    datasets.iter().find(|d| &d.id == left_id),
                    datasets.iter().find(|d| &d.id == right_id),
                ) {
                    relationships.push(DatasetRelationship {
                        source_id: left_id.clone(),
                        target_id: right_id.clone(),
                        source_name: left_ds.name.clone(),
                        target_name: right_ds.name.clone(),
                        source_column: left_col,
                        target_column: right_col,
                        confidence: 80,
                        detection_method: "query_history_join".to_string(),
                    });
                }
            }
        }
    }

    relationships
}

/// Extract JOIN patterns from SQL (simplified parser)
/// Returns: Vec<(left_table, right_table, left_column, right_column)>
fn extract_join_patterns(sql: &str) -> Vec<(String, String, String, String)> {
    let mut joins = Vec::new();

    // Very simple pattern matching for "table1.col1 = table2.col2"
    // This won't catch all SQL variants but covers common patterns
    let re = regex::Regex::new(
        r"(?i)join\s+(\w+)\s+(?:as\s+)?(\w+)?\s+on\s+(\w+)\.(\w+)\s*=\s*(\w+)\.(\w+)"
    ).unwrap();

    for cap in re.captures_iter(sql) {
        let table1 = cap.get(1).map_or("", |m| m.as_str()).to_lowercase();
        let alias1 = cap.get(2).map_or("", |m| m.as_str()).to_lowercase();
        let left_alias = cap.get(3).map_or("", |m| m.as_str()).to_lowercase();
        let left_col = cap.get(4).map_or("", |m| m.as_str()).to_string();
        let right_alias = cap.get(5).map_or("", |m| m.as_str()).to_lowercase();
        let right_col = cap.get(6).map_or("", |m| m.as_str()).to_string();

        // Use table name as alias if no alias provided
        let final_alias1 = if alias1.is_empty() { table1.clone() } else { alias1 };

        // Map aliases back to table names
        let left_table = if left_alias == final_alias1 { table1.clone() } else { left_alias };
        let right_table = if right_alias == final_alias1 { table1.clone() } else { right_alias };

        if !left_table.is_empty() && !right_table.is_empty() {
            joins.push((left_table, right_table, left_col, right_col));
        }
    }

    joins
}

/// Merge duplicate relationships and boost confidence
fn merge_relationships(relationships: Vec<DatasetRelationship>) -> Vec<DatasetRelationship> {
    let mut map: HashMap<String, DatasetRelationship> = HashMap::new();

    for rel in relationships {
        // Create a key for deduplication: source_id|target_id|source_col|target_col
        let key = format!(
            "{}|{}|{}|{}",
            rel.source_id, rel.target_id, rel.source_column, rel.target_column
        );

        map.entry(key)
            .and_modify(|existing| {
                // Boost confidence if found by multiple methods
                existing.confidence = existing.confidence.saturating_add(20).min(100);
                // Prefer query-based detection method in the display
                if rel.detection_method == "query_history_join" {
                    existing.detection_method = "query_history_join+schema".to_string();
                }
            })
            .or_insert(rel);
    }

    let mut merged: Vec<DatasetRelationship> = map.into_values().collect();

    // Sort by confidence (highest first)
    merged.sort_by(|a, b| b.confidence.cmp(&a.confidence));

    merged
}

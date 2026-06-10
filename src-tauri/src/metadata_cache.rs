// Local metadata cache — persistent DuckDB storage for offline dataset search
//
// Enables instant fuzzy search over all dataset metadata without hitting
// the backend. Syncs from backend when online, works offline when not.
//
// Architecture:
// - Single DuckDB file: ~/.sery/metadata_cache.db
// - Table: datasets (id, name, path, format, size, schema_json, tags, description, last_synced)
// - Fuzzy search using LIKE with multiple columns
// - Full-text search capability for future enhancement
//
// Connection lifetime:
// The DuckDB connection is kept open for the entire process lifetime via a
// process-global singleton (METADATA_CONN). This prevents the ART index
// checkpoint-on-close crash (DuckDB 1.1 assertion in TransformToDeprecated)
// that fires every time a MetadataCache was dropped. With a singleton, Drop
// is only called at process exit, where an abort is irrelevant.

use duckdb::{Connection, params};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::sync::{Mutex, OnceLock};
use once_cell::sync::OnceCell;
use crate::error::{AgentError, Result};
use crate::schema_diff::{self, SchemaDiff};

/// Process-wide DuckDB connection for the metadata cache.
///
/// Initialized once on the first `MetadataCache::new()` call.
/// Never dropped during app lifetime — this is intentional (see module doc).
static METADATA_CONN: OnceCell<Mutex<Connection>> = OnceCell::new();

/// Lazily open and initialize the singleton DB connection.
/// Returns the static reference on success; propagates IO/SQL errors.
fn get_or_init_conn() -> Result<&'static Mutex<Connection>> {
    METADATA_CONN.get_or_try_init(|| {
        let cache_dir = resolve_cache_dir()?;

        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| AgentError::Config(format!("Failed to create cache directory: {}", e)))?;

        let db_path = cache_dir.join("metadata_cache.db");

        let conn = Connection::open(&db_path)
            .map_err(|e| AgentError::Database(format!("Failed to open metadata cache: {}", e)))?;

        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS datasets (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                file_format TEXT NOT NULL,
                size_bytes BIGINT NOT NULL,
                schema_json TEXT,
                tags TEXT,
                description TEXT,
                last_synced TIMESTAMP NOT NULL,
                UNIQUE(workspace_id, path)
            );
            CREATE INDEX IF NOT EXISTS idx_name      ON datasets(name);
            CREATE INDEX IF NOT EXISTS idx_workspace ON datasets(workspace_id);
            CREATE INDEX IF NOT EXISTS idx_path      ON datasets(path);
            "#,
        )
        .map_err(|e| AgentError::Database(format!("Failed to create schema: {}", e)))?;

        Ok(Mutex::new(conn))
    })
}

/// Where the metadata cache DB lives.
///
/// In test builds this is a per-process temp dir: tests must never open
/// the real user cache — a running Sery Link app holds a DuckDB lock on
/// it, and a test bug could corrupt real user data.
#[cfg(test)]
fn resolve_cache_dir() -> Result<std::path::PathBuf> {
    static TEST_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
    Ok(TEST_DIR
        .get_or_init(|| {
            std::env::temp_dir().join(format!("sery-test-cache-{}", std::process::id()))
        })
        .clone())
}

#[cfg(not(test))]
fn resolve_cache_dir() -> Result<std::path::PathBuf> {
    Ok(dirs::data_local_dir()
        .ok_or_else(|| AgentError::Config("Could not determine local data directory".to_string()))?
        .join("sery"))
}

/// Process-wide write lock for the metadata cache.
///
/// Every command that writes (`upsert_dataset`, `upsert_many`, etc.)
/// acquires this mutex before starting its DuckDB transaction.
/// Reads don't need it — DuckDB's MVCC handles read-write concurrency.
///
/// Why this is needed:
///
/// Two concurrent `rescan_folder` calls each do DELETE-by-key +
/// INSERT-with-key in separate transactions. At the second commit,
/// DuckDB sees the first commit's row and raises a duplicate-key
/// FatalException during index validation, crashing the process.
/// Serialising writes through this mutex means the second transaction
/// waits for the first to commit, then cleanly deletes + re-inserts.
fn upsert_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedDataset {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub path: String,
    pub file_format: String,
    pub size_bytes: i64,
    pub schema_json: Option<String>,
    pub tags: Vec<String>,
    pub description: Option<String>,
    pub last_synced: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub dataset: CachedDataset,
    pub score: f64, // Relevance score (higher is better)
}

/// Handle to the metadata cache.
///
/// Cheap to create — just ensures the singleton connection is alive.
/// The connection itself is never dropped while the process runs.
pub struct MetadataCache;

impl MetadataCache {
    pub fn new() -> Result<Self> {
        get_or_init_conn()?;
        Ok(Self)
    }

    /// Upsert a single dataset into the cache.
    ///
    /// The schema has TWO unique constraints that BOTH fire on the
    /// same logical row when the id is deterministic:
    ///
    ///   id TEXT PRIMARY KEY                     (PK on id)
    ///   UNIQUE(workspace_id, path)              (composite unique)
    ///
    /// Callers compute id as `format!("{}::{}", workspace_id, path)`
    /// — see commands::rescan_folder at the upsert site. So the PK
    /// conflict and the UNIQUE conflict ALWAYS fire together on a
    /// re-scan.
    ///
    /// Bulletproof fix: explicit DELETE + INSERT inside a single
    /// transaction. The DELETE targets BOTH possible conflict
    /// rows (id match OR (workspace_id, path) match), so the INSERT
    /// is guaranteed clean.
    pub fn upsert_dataset(&mut self, dataset: &CachedDataset) -> Result<()> {
        let _guard = upsert_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let tags_json = serde_json::to_string(&dataset.tags)
            .map_err(|e| AgentError::Serialization(format!("Failed to serialize tags: {}", e)))?;

        let mut conn = get_or_init_conn()?
            .lock()
            .unwrap_or_else(|p| p.into_inner());

        let tx = conn.transaction()
            .map_err(|e| AgentError::Database(format!("Failed to start txn: {}", e)))?;

        tx.execute(
            r#"
            DELETE FROM datasets
            WHERE id = ?
               OR (workspace_id = ? AND path = ?)
            "#,
            params![&dataset.id, &dataset.workspace_id, &dataset.path],
        ).map_err(|e| AgentError::Database(format!("Failed to clear conflicts: {}", e)))?;

        tx.execute(
            r#"
            INSERT INTO datasets (
                id, workspace_id, name, path, file_format,
                size_bytes, schema_json, tags, description, last_synced
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            params![
                &dataset.id,
                &dataset.workspace_id,
                &dataset.name,
                &dataset.path,
                &dataset.file_format,
                dataset.size_bytes,
                &dataset.schema_json,
                &tags_json,
                &dataset.description,
                dataset.last_synced.to_rfc3339(),
            ],
        ).map_err(|e| AgentError::Database(format!("Failed to insert dataset: {}", e)))?;

        tx.commit()
            .map_err(|e| AgentError::Database(format!("Failed to commit upsert: {}", e)))?;

        Ok(())
    }

    /// Bulk upsert datasets (for full sync from backend)
    pub fn upsert_many(&mut self, datasets: &[CachedDataset]) -> Result<()> {
        if datasets.is_empty() {
            return Ok(());
        }

        let _guard = upsert_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let mut conn = get_or_init_conn()?
            .lock()
            .unwrap_or_else(|p| p.into_inner());

        let tx = conn.transaction()
            .map_err(|e| AgentError::Database(format!("Failed to start batch txn: {}", e)))?;

        for dataset in datasets {
            let tags_json = serde_json::to_string(&dataset.tags)
                .map_err(|e| AgentError::Serialization(format!("Failed to serialize tags: {}", e)))?;

            tx.execute(
                r#"
                DELETE FROM datasets
                WHERE id = ?
                   OR (workspace_id = ? AND path = ?)
                "#,
                params![&dataset.id, &dataset.workspace_id, &dataset.path],
            ).map_err(|e| AgentError::Database(format!("Failed to clear batch conflict: {}", e)))?;

            tx.execute(
                r#"
                INSERT INTO datasets (
                    id, workspace_id, name, path, file_format,
                    size_bytes, schema_json, tags, description, last_synced
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
                params![
                    &dataset.id,
                    &dataset.workspace_id,
                    &dataset.name,
                    &dataset.path,
                    &dataset.file_format,
                    dataset.size_bytes,
                    &dataset.schema_json,
                    &tags_json,
                    &dataset.description,
                    dataset.last_synced.to_rfc3339(),
                ],
            ).map_err(|e| AgentError::Database(format!("Failed to insert batch dataset: {}", e)))?;
        }

        tx.commit()
            .map_err(|e| AgentError::Database(format!("Failed to commit batch upsert: {}", e)))?;

        Ok(())
    }

    /// Fuzzy search datasets by query string.
    /// Searches across name, path, description, and tags.
    pub fn search(&self, workspace_id: &str, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let query_lower = query.to_lowercase();
        let pattern = format!("%{}%", query_lower);

        let conn = get_or_init_conn()?
            .lock()
            .unwrap_or_else(|p| p.into_inner());

        let mut stmt = conn.prepare(
            r#"
            SELECT
                id, workspace_id, name, path, file_format,
                size_bytes, schema_json, tags, description,
                strftime(last_synced, '%Y-%m-%dT%H:%M:%SZ') AS last_synced_str,
                CASE
                    WHEN LOWER(name) = ? THEN 100
                    WHEN LOWER(name) LIKE ? THEN 90
                    WHEN LOWER(name) LIKE ? THEN 70
                    WHEN LOWER(path) LIKE ? THEN 60
                    WHEN LOWER(description) LIKE ? THEN 50
                    WHEN LOWER(tags) LIKE ? THEN 40
                    ELSE 0
                END as score
            FROM datasets
            WHERE workspace_id = ?
            AND (
                LOWER(name) LIKE ?
                OR LOWER(path) LIKE ?
                OR LOWER(description) LIKE ?
                OR LOWER(tags) LIKE ?
            )
            ORDER BY score DESC, name ASC
            LIMIT ?
            "#,
        ).map_err(|e| AgentError::Database(format!("Failed to prepare search query: {}", e)))?;

        let prefix_pattern = format!("{}%", query_lower);

        let rows = stmt.query_map(
            params![
                &query_lower,
                &prefix_pattern,
                &pattern,
                &pattern,
                &pattern,
                &pattern,
                workspace_id,
                &pattern,
                &pattern,
                &pattern,
                &pattern,
                limit as i32,
            ],
            |row| {
                let tags_json: Option<String> = row.get(7)?;
                let tags: Vec<String> = tags_json
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default();

                let last_synced_str: String = row.get(9)?;
                let last_synced = DateTime::parse_from_rfc3339(&last_synced_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                Ok(SearchResult {
                    dataset: CachedDataset {
                        id: row.get(0)?,
                        workspace_id: row.get(1)?,
                        name: row.get(2)?,
                        path: row.get(3)?,
                        file_format: row.get(4)?,
                        size_bytes: row.get(5)?,
                        schema_json: row.get(6)?,
                        tags,
                        description: row.get(8)?,
                        last_synced,
                    },
                    score: row.get(10)?,
                })
            },
        ).map_err(|e| AgentError::Database(format!("Search query failed: {}", e)))?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| AgentError::Database(format!("Failed to collect search results: {}", e)))
    }

    /// Get all datasets for a workspace (no filtering)
    pub fn get_all(&self, workspace_id: &str) -> Result<Vec<CachedDataset>> {
        let conn = get_or_init_conn()?
            .lock()
            .unwrap_or_else(|p| p.into_inner());

        let mut stmt = conn.prepare(
            r#"
            SELECT
                id, workspace_id, name, path, file_format,
                size_bytes, schema_json, tags, description,
                strftime(last_synced, '%Y-%m-%dT%H:%M:%SZ') AS last_synced
            FROM datasets
            WHERE workspace_id = ?
            ORDER BY name ASC
            "#,
        ).map_err(|e| AgentError::Database(format!("Failed to prepare query: {}", e)))?;

        let rows = stmt.query_map(
            params![workspace_id],
            |row| {
                let tags_json: Option<String> = row.get(7)?;
                let tags: Vec<String> = tags_json
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default();

                let last_synced_str: String = row.get(9)?;
                let last_synced = DateTime::parse_from_rfc3339(&last_synced_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                Ok(CachedDataset {
                    id: row.get(0)?,
                    workspace_id: row.get(1)?,
                    name: row.get(2)?,
                    path: row.get(3)?,
                    file_format: row.get(4)?,
                    size_bytes: row.get(5)?,
                    schema_json: row.get(6)?,
                    tags,
                    description: row.get(8)?,
                    last_synced,
                })
            },
        ).map_err(|e| AgentError::Database(format!("Query failed: {}", e)))?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| AgentError::Database(format!("Failed to collect results: {}", e)))
    }

    /// Get a single dataset by ID
    pub fn get_by_id(&self, id: &str) -> Result<Option<CachedDataset>> {
        let conn = get_or_init_conn()?
            .lock()
            .unwrap_or_else(|p| p.into_inner());

        let mut stmt = conn.prepare(
            r#"
            SELECT
                id, workspace_id, name, path, file_format,
                size_bytes, schema_json, tags, description,
                strftime(last_synced, '%Y-%m-%dT%H:%M:%SZ') AS last_synced
            FROM datasets
            WHERE id = ?
            "#,
        ).map_err(|e| AgentError::Database(format!("Failed to prepare query: {}", e)))?;

        let mut rows = stmt.query_map(
            params![id],
            |row| {
                let tags_json: Option<String> = row.get(7)?;
                let tags: Vec<String> = tags_json
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default();

                let last_synced_str: String = row.get(9)?;
                let last_synced = DateTime::parse_from_rfc3339(&last_synced_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                Ok(CachedDataset {
                    id: row.get(0)?,
                    workspace_id: row.get(1)?,
                    name: row.get(2)?,
                    path: row.get(3)?,
                    file_format: row.get(4)?,
                    size_bytes: row.get(5)?,
                    schema_json: row.get(6)?,
                    tags,
                    description: row.get(8)?,
                    last_synced,
                })
            },
        ).map_err(|e| AgentError::Database(format!("Query failed: {}", e)))?;

        Ok(rows.next().transpose().map_err(|e| AgentError::Database(format!("Failed to fetch dataset: {}", e)))?)
    }

    /// Compute the schema diff between what's cached for this (workspace, path)
    /// and a newly-scanned schema. Returns an empty diff if the path isn't yet
    /// cached (first-sync is not a schema change).
    pub fn compute_schema_diff(
        &self,
        workspace_id: &str,
        path: &str,
        new_schema_json: Option<&str>,
    ) -> Result<SchemaDiff> {
        let conn = get_or_init_conn()?
            .lock()
            .unwrap_or_else(|p| p.into_inner());

        let mut stmt = conn
            .prepare("SELECT schema_json FROM datasets WHERE workspace_id = ? AND path = ?")
            .map_err(|e| AgentError::Database(format!("prepare schema lookup: {}", e)))?;
        let mut rows = stmt
            .query_map(params![workspace_id, path], |row| row.get::<_, Option<String>>(0))
            .map_err(|e| AgentError::Database(format!("query schema lookup: {}", e)))?;

        let old_json: Option<String> = match rows.next() {
            Some(Ok(s)) => s,
            Some(Err(e)) => {
                return Err(AgentError::Database(format!("fetch schema row: {}", e)));
            }
            None => return Ok(SchemaDiff::default()),
        };

        let old = schema_diff::parse_schema_json(old_json.as_deref()).unwrap_or_default();
        let new = schema_diff::parse_schema_json(new_schema_json).unwrap_or_default();
        Ok(schema_diff::diff_schemas(&old, &new))
    }

    /// Delete all cached datasets whose path starts with `path_prefix`
    /// for a given workspace. Called when a source is removed so the local
    /// cache doesn't retain stale entries that are no longer being watched.
    pub fn delete_by_path_prefix(&mut self, workspace_id: &str, path_prefix: &str) -> Result<usize> {
        let conn = get_or_init_conn()?
            .lock()
            .unwrap_or_else(|p| p.into_inner());

        let prefix_like = format!("{}%", path_prefix);
        let n = conn.execute(
            "DELETE FROM datasets WHERE workspace_id = ? AND (path = ? OR path LIKE ?)",
            params![workspace_id, path_prefix, &prefix_like],
        ).map_err(|e| AgentError::Database(format!("Failed to delete by path prefix: {}", e)))?;

        Ok(n)
    }

    /// Clear all cached datasets for a workspace
    pub fn clear_workspace(&mut self, workspace_id: &str) -> Result<()> {
        let conn = get_or_init_conn()?
            .lock()
            .unwrap_or_else(|p| p.into_inner());

        conn.execute(
            "DELETE FROM datasets WHERE workspace_id = ?",
            params![workspace_id],
        ).map_err(|e| AgentError::Database(format!("Failed to clear workspace cache: {}", e)))?;

        Ok(())
    }

    /// Get cache statistics
    pub fn get_stats(&self) -> Result<CacheStats> {
        let conn = get_or_init_conn()?
            .lock()
            .unwrap_or_else(|p| p.into_inner());

        let mut stmt = conn.prepare(
            "SELECT COUNT(*), SUM(size_bytes) FROM datasets"
        ).map_err(|e| AgentError::Database(format!("Failed to prepare stats query: {}", e)))?;

        let (count, total_bytes): (i64, Option<i64>) = stmt.query_row(
            [],
            |row| Ok((row.get(0)?, row.get(1)?))
        ).map_err(|e| AgentError::Database(format!("Stats query failed: {}", e)))?;

        Ok(CacheStats {
            dataset_count: count as usize,
            total_size_bytes: total_bytes.unwrap_or(0) as u64,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CacheStats {
    pub dataset_count: usize,
    pub total_size_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dataset(id: &str, name: &str) -> CachedDataset {
        CachedDataset {
            id: id.to_string(),
            workspace_id: "test-workspace".to_string(),
            name: name.to_string(),
            path: format!("/data/{}.parquet", name),
            file_format: "parquet".to_string(),
            size_bytes: 1024,
            schema_json: Some(r#"[{"name":"id","type":"INT64"}]"#.to_string()),
            tags: vec!["test".to_string()],
            description: Some(format!("Test dataset: {}", name)),
            last_synced: Utc::now(),
        }
    }

    #[test]
    fn test_cache_lifecycle() {
        let mut cache = MetadataCache::new().unwrap();

        let ds1 = test_dataset("1", "customers");
        let ds2 = test_dataset("2", "orders");
        cache.upsert_dataset(&ds1).unwrap();
        cache.upsert_dataset(&ds2).unwrap();

        let results = cache.search("test-workspace", "cust", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].dataset.name, "customers");

        let all = cache.get_all("test-workspace").unwrap();
        assert_eq!(all.len(), 2);

        cache.clear_workspace("test-workspace").unwrap();
        let empty = cache.get_all("test-workspace").unwrap();
        assert_eq!(empty.len(), 0);
    }

    #[test]
    fn compute_schema_diff_returns_empty_for_unknown_path() {
        let cache = MetadataCache::new().unwrap();
        let diff = cache
            .compute_schema_diff(
                "never-seen-workspace",
                "/nothing/here.parquet",
                Some(r#"[{"name":"id","type":"INTEGER","nullable":false}]"#),
            )
            .unwrap();
        assert!(diff.is_empty(), "first-sync must not produce a change diff");
    }

    #[test]
    fn compute_schema_diff_surfaces_real_change() {
        let mut cache = MetadataCache::new().unwrap();
        let ws = "schema-diff-change-test-ws";

        let mut ds = test_dataset("sd-1", "orders");
        ds.workspace_id = ws.to_string();
        ds.path = "/data/orders.parquet".into();
        ds.schema_json = Some(
            r#"[{"name":"id","type":"INTEGER","nullable":false},{"name":"amount","type":"INTEGER","nullable":false}]"#.into()
        );
        cache.upsert_dataset(&ds).unwrap();

        let new_schema = r#"[
            {"name":"id","type":"INTEGER","nullable":false},
            {"name":"amount","type":"VARCHAR","nullable":false},
            {"name":"currency","type":"VARCHAR","nullable":true}
        ]"#;
        let diff = cache
            .compute_schema_diff(ws, "/data/orders.parquet", Some(new_schema))
            .unwrap();
        assert_eq!(diff.added(), 1, "currency is new");
        assert_eq!(diff.type_changed(), 1, "amount type flipped");
        assert_eq!(diff.removed(), 0);

        cache.clear_workspace(ws).unwrap();
    }

    #[test]
    fn compute_schema_diff_noop_when_unchanged() {
        let mut cache = MetadataCache::new().unwrap();
        let ws = "schema-diff-noop-test-ws";

        let mut ds = test_dataset("sd-2", "customers");
        ds.workspace_id = ws.to_string();
        ds.path = "/data/customers.parquet".into();
        let schema = r#"[{"name":"id","type":"INTEGER","nullable":false}]"#;
        ds.schema_json = Some(schema.into());
        cache.upsert_dataset(&ds).unwrap();

        let diff = cache
            .compute_schema_diff(ws, "/data/customers.parquet", Some(schema))
            .unwrap();
        assert!(
            diff.is_empty(),
            "same-schema re-scan must not fire a change notification"
        );

        cache.clear_workspace(ws).unwrap();
    }
}

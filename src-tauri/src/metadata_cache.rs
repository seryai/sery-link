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

use duckdb::{Connection, params};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use crate::error::{AgentError, Result};

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

pub struct MetadataCache {
    conn: Connection,
}

impl MetadataCache {
    /// Initialize or open the metadata cache database
    pub fn new() -> Result<Self> {
        let cache_dir = dirs::data_local_dir()
            .ok_or_else(|| AgentError::Config("Could not determine local data directory".to_string()))?
            .join("sery");

        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| AgentError::Config(format!("Failed to create cache directory: {}", e)))?;

        let db_path = cache_dir.join("metadata_cache.db");

        let conn = Connection::open(&db_path)
            .map_err(|e| AgentError::Database(format!("Failed to open metadata cache: {}", e)))?;

        let mut cache = Self { conn };
        cache.init_schema()?;

        Ok(cache)
    }

    /// Create the datasets table if it doesn't exist
    fn init_schema(&mut self) -> Result<()> {
        self.conn.execute(
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
            "#,
            [],
        ).map_err(|e| AgentError::Database(format!("Failed to create schema: {}", e)))?;

        // Create indexes for fast search
        self.conn.execute(
            r#"
            CREATE INDEX IF NOT EXISTS idx_name ON datasets(name);
            CREATE INDEX IF NOT EXISTS idx_workspace ON datasets(workspace_id);
            CREATE INDEX IF NOT EXISTS idx_path ON datasets(path);
            "#,
            [],
        ).map_err(|e| AgentError::Database(format!("Failed to create indexes: {}", e)))?;

        Ok(())
    }

    /// Upsert a single dataset into the cache
    pub fn upsert_dataset(&mut self, dataset: &CachedDataset) -> Result<()> {
        let tags_json = serde_json::to_string(&dataset.tags)
            .map_err(|e| AgentError::Serialization(format!("Failed to serialize tags: {}", e)))?;

        self.conn.execute(
            r#"
            INSERT INTO datasets (
                id, workspace_id, name, path, file_format,
                size_bytes, schema_json, tags, description, last_synced
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(workspace_id, path) DO UPDATE SET
                id = excluded.id,
                name = excluded.name,
                file_format = excluded.file_format,
                size_bytes = excluded.size_bytes,
                schema_json = excluded.schema_json,
                tags = excluded.tags,
                description = excluded.description,
                last_synced = excluded.last_synced
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
        ).map_err(|e| AgentError::Database(format!("Failed to upsert dataset: {}", e)))?;

        Ok(())
    }

    /// Bulk upsert datasets (for full sync from backend)
    pub fn upsert_many(&mut self, datasets: &[CachedDataset]) -> Result<()> {
        for dataset in datasets {
            self.upsert_dataset(dataset)?;
        }
        Ok(())
    }

    /// Fuzzy search datasets by query string
    /// Searches across name, path, description, and tags
    pub fn search(&self, workspace_id: &str, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let query_lower = query.to_lowercase();
        let pattern = format!("%{}%", query_lower);

        let mut stmt = self.conn.prepare(
            r#"
            SELECT
                id, workspace_id, name, path, file_format,
                size_bytes, schema_json, tags, description,
                strftime(last_synced, '%Y-%m-%dT%H:%M:%SZ') AS last_synced_str,
                -- Simple scoring: exact match > prefix match > contains
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
                &query_lower,      // exact match
                &prefix_pattern,   // prefix match
                &pattern,          // contains
                &pattern,          // path
                &pattern,          // description
                &pattern,          // tags
                workspace_id,
                &pattern,          // name LIKE
                &pattern,          // path LIKE
                &pattern,          // description LIKE
                &pattern,          // tags LIKE
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
        let mut stmt = self.conn.prepare(
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
        let mut stmt = self.conn.prepare(
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

    /// Clear all cached datasets for a workspace
    pub fn clear_workspace(&mut self, workspace_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM datasets WHERE workspace_id = ?",
            params![workspace_id],
        ).map_err(|e| AgentError::Database(format!("Failed to clear workspace cache: {}", e)))?;

        Ok(())
    }

    /// Get cache statistics
    pub fn get_stats(&self) -> Result<CacheStats> {
        let mut stmt = self.conn.prepare(
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

        // Insert datasets
        let ds1 = test_dataset("1", "customers");
        let ds2 = test_dataset("2", "orders");
        cache.upsert_dataset(&ds1).unwrap();
        cache.upsert_dataset(&ds2).unwrap();

        // Search
        let results = cache.search("test-workspace", "cust", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].dataset.name, "customers");

        // Get all
        let all = cache.get_all("test-workspace").unwrap();
        assert_eq!(all.len(), 2);

        // Clear
        cache.clear_workspace("test-workspace").unwrap();
        let empty = cache.get_all("test-workspace").unwrap();
        assert_eq!(empty.len(), 0);
    }
}

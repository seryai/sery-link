//! Persistent preview cache — short-circuits remote DuckDB queries
//! for `read_dataset_rows` (row preview) and `profile_dataset` (column
//! profiles) when the file's freshness key (mtime, size) hasn't moved.
//!
//! Why this exists: opening a file's detail page on a remote source
//! (S3, HTTPS) re-runs DuckDB-httpfs against the URL on every visit
//! — multi-MB column-chunk fetches + a network round-trip every time.
//! With this cache, a re-open of a file the user already previewed
//! is instant + zero S3 egress.
//!
//! Freshness model: the (mtime, size) freshness key is borrowed from
//! `scan_cache::get_freshness(folder, relative)` — that's the source
//! of truth for "current file state". When the scanner re-scans and
//! detects a change, scan_cache.put writes new mtime/size; this
//! cache's old entry is left as garbage but no longer matches reads.
//! Net: no extra HEAD probe at preview time, freshness tracks
//! whatever scan_cache says.
//!
//! Two payload shapes share one DB:
//!   - `row_preview`    → `DatasetRows` blob (read_dataset_rows)
//!   - `column_profile` → `Vec<ColumnProfile>` blob (profile_dataset)
//!
//! Failure mode: every cache operation is best-effort. A DB error
//! falls through to a fresh DuckDB query, so a corrupted or missing
//! cache never blocks previews — it just costs the unoptimised path.

use chrono::Utc;
use duckdb::{params, Connection};
use once_cell::sync::Lazy;
use std::fs;
use std::sync::Mutex;

use crate::commands::{ColumnProfile, DatasetRows};
use crate::error::{AgentError, Result};

static GLOBAL_CACHE: Lazy<Mutex<Option<PreviewCache>>> = Lazy::new(|| Mutex::new(None));

/// Run `f` with the shared preview cache. Returns `None` if the
/// cache can't be opened or the mutex is poisoned. Mirror of
/// `scan_cache::with_cache`.
pub fn with_cache<F, T>(f: F) -> Option<T>
where
    F: FnOnce(&PreviewCache) -> T,
{
    let mut guard = match GLOBAL_CACHE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    if guard.is_none() {
        match PreviewCache::new() {
            Ok(c) => *guard = Some(c),
            Err(e) => {
                eprintln!(
                    "[preview_cache] failed to open DB: {} (cache disabled this session)",
                    e
                );
            }
        }
    }
    guard.as_ref().map(f)
}

pub struct PreviewCache {
    conn: Connection,
}

impl PreviewCache {
    pub fn new() -> Result<Self> {
        let cache_dir = dirs::data_local_dir()
            .ok_or_else(|| AgentError::Config("no local data dir".to_string()))?
            .join("sery");
        fs::create_dir_all(&cache_dir)
            .map_err(|e| AgentError::Config(format!("create cache dir: {}", e)))?;

        let db_path = cache_dir.join("preview_cache.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| AgentError::Database(format!("open preview cache: {}", e)))?;

        // Two tables, same key shape. Splitting them keeps the
        // serialized payload small (a row preview can be hundreds of
        // KB, a profile is small) and lets one populate independently
        // of the other.
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS row_preview (
                folder_path TEXT NOT NULL,
                relative_path TEXT NOT NULL,
                mtime_secs BIGINT NOT NULL,
                size_bytes BIGINT NOT NULL,
                payload_json TEXT NOT NULL,
                cached_at TIMESTAMP NOT NULL,
                PRIMARY KEY (folder_path, relative_path)
            );
            CREATE TABLE IF NOT EXISTS column_profile (
                folder_path TEXT NOT NULL,
                relative_path TEXT NOT NULL,
                mtime_secs BIGINT NOT NULL,
                size_bytes BIGINT NOT NULL,
                payload_json TEXT NOT NULL,
                cached_at TIMESTAMP NOT NULL,
                PRIMARY KEY (folder_path, relative_path)
            );
            "#,
        )
        .map_err(|e| {
            AgentError::Database(format!("init preview cache schema: {}", e))
        })?;
        Ok(Self { conn })
    }

    pub fn get_rows(
        &self,
        folder_path: &str,
        relative_path: &str,
        current_mtime_secs: i64,
        current_size_bytes: i64,
    ) -> Option<DatasetRows> {
        self.get_payload(
            "row_preview",
            folder_path,
            relative_path,
            current_mtime_secs,
            current_size_bytes,
        )
    }

    pub fn put_rows(
        &self,
        folder_path: &str,
        relative_path: &str,
        mtime_secs: i64,
        size_bytes: i64,
        rows: &DatasetRows,
    ) -> Result<()> {
        self.put_payload(
            "row_preview",
            folder_path,
            relative_path,
            mtime_secs,
            size_bytes,
            rows,
        )
    }

    pub fn get_profile(
        &self,
        folder_path: &str,
        relative_path: &str,
        current_mtime_secs: i64,
        current_size_bytes: i64,
    ) -> Option<Vec<ColumnProfile>> {
        self.get_payload(
            "column_profile",
            folder_path,
            relative_path,
            current_mtime_secs,
            current_size_bytes,
        )
    }

    pub fn put_profile(
        &self,
        folder_path: &str,
        relative_path: &str,
        mtime_secs: i64,
        size_bytes: i64,
        profile: &[ColumnProfile],
    ) -> Result<()> {
        self.put_payload(
            "column_profile",
            folder_path,
            relative_path,
            mtime_secs,
            size_bytes,
            &profile.to_vec(),
        )
    }

    /// Drop every cached preview / profile for a folder. Called from
    /// remove_source so an old source's previews don't linger after
    /// the user disconnects it.
    pub fn invalidate_folder(&self, folder_path: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM row_preview WHERE folder_path = ?",
                params![folder_path],
            )
            .map_err(|e| {
                AgentError::Database(format!("invalidate row_preview: {}", e))
            })?;
        self.conn
            .execute(
                "DELETE FROM column_profile WHERE folder_path = ?",
                params![folder_path],
            )
            .map_err(|e| {
                AgentError::Database(format!("invalidate column_profile: {}", e))
            })?;
        Ok(())
    }

    fn get_payload<T: serde::de::DeserializeOwned>(
        &self,
        table: &str,
        folder_path: &str,
        relative_path: &str,
        current_mtime_secs: i64,
        current_size_bytes: i64,
    ) -> Option<T> {
        let sql = format!(
            "SELECT mtime_secs, size_bytes, payload_json
             FROM {} WHERE folder_path = ? AND relative_path = ?",
            table
        );
        let mut stmt = self.conn.prepare(&sql).ok()?;
        let (cached_mtime, cached_size, payload_json): (i64, i64, String) = stmt
            .query_row(params![folder_path, relative_path], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .ok()?;
        if cached_mtime != current_mtime_secs || cached_size != current_size_bytes {
            return None;
        }
        // Schema-mismatch fall-through: deserialize failure → miss.
        // Same policy as scan_cache.
        serde_json::from_str(&payload_json).ok()
    }

    fn put_payload<T: serde::Serialize>(
        &self,
        table: &str,
        folder_path: &str,
        relative_path: &str,
        mtime_secs: i64,
        size_bytes: i64,
        payload: &T,
    ) -> Result<()> {
        let json = serde_json::to_string(payload).map_err(|e| {
            AgentError::Serialization(format!("serialize {} payload: {}", table, e))
        })?;
        let sql = format!(
            r#"
            INSERT INTO {table}
                (folder_path, relative_path, mtime_secs, size_bytes, payload_json, cached_at)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(folder_path, relative_path) DO UPDATE SET
                mtime_secs = excluded.mtime_secs,
                size_bytes = excluded.size_bytes,
                payload_json = excluded.payload_json,
                cached_at = excluded.cached_at
            "#
        );
        self.conn
            .execute(
                &sql,
                params![
                    folder_path,
                    relative_path,
                    mtime_secs,
                    size_bytes,
                    json,
                    Utc::now().to_rfc3339(),
                ],
            )
            .map_err(|e| {
                AgentError::Database(format!("upsert {}: {}", table, e))
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fresh_cache() -> PreviewCache {
        // Use an in-memory DuckDB so tests don't touch the user's
        // real ~/.local/share/sery cache. The schema and operations
        // are otherwise identical.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE row_preview (
                folder_path TEXT NOT NULL,
                relative_path TEXT NOT NULL,
                mtime_secs BIGINT NOT NULL,
                size_bytes BIGINT NOT NULL,
                payload_json TEXT NOT NULL,
                cached_at TIMESTAMP NOT NULL,
                PRIMARY KEY (folder_path, relative_path)
            );
            CREATE TABLE column_profile (
                folder_path TEXT NOT NULL,
                relative_path TEXT NOT NULL,
                mtime_secs BIGINT NOT NULL,
                size_bytes BIGINT NOT NULL,
                payload_json TEXT NOT NULL,
                cached_at TIMESTAMP NOT NULL,
                PRIMARY KEY (folder_path, relative_path)
            );
            "#,
        )
        .unwrap();
        PreviewCache { conn }
    }

    fn sample_rows() -> DatasetRows {
        DatasetRows {
            columns: vec!["id".into(), "name".into()],
            rows: vec![vec!["1".into(), "alice".into()]],
            total_rows: 42,
            truncated: false,
        }
    }

    #[test]
    fn put_then_get_returns_exact_rows() {
        let c = fresh_cache();
        let rows = sample_rows();
        c.put_rows("s3://bucket/", "data.parquet", 100, 1024, &rows)
            .unwrap();
        let hit = c.get_rows("s3://bucket/", "data.parquet", 100, 1024).unwrap();
        assert_eq!(hit.columns, rows.columns);
        assert_eq!(hit.rows, rows.rows);
        assert_eq!(hit.total_rows, 42);
        assert!(!hit.truncated);
    }

    #[test]
    fn freshness_mismatch_returns_none() {
        // mtime change → miss; size change → miss.
        let c = fresh_cache();
        c.put_rows("s3://bucket/", "data.parquet", 100, 1024, &sample_rows())
            .unwrap();
        assert!(c.get_rows("s3://bucket/", "data.parquet", 101, 1024).is_none());
        assert!(c.get_rows("s3://bucket/", "data.parquet", 100, 2048).is_none());
        assert!(c.get_rows("s3://bucket/", "data.parquet", 100, 1024).is_some());
    }

    #[test]
    fn put_overwrites_previous_entry() {
        let c = fresh_cache();
        let v1 = DatasetRows {
            columns: vec!["a".into()],
            rows: vec![vec!["1".into()]],
            total_rows: 1,
            truncated: false,
        };
        let v2 = DatasetRows {
            columns: vec!["a".into(), "b".into()],
            rows: vec![vec!["1".into(), "2".into()]],
            total_rows: 2,
            truncated: false,
        };
        c.put_rows("f", "r", 1, 1, &v1).unwrap();
        c.put_rows("f", "r", 2, 2, &v2).unwrap();
        let hit = c.get_rows("f", "r", 2, 2).unwrap();
        assert_eq!(hit.columns.len(), 2);
        assert_eq!(hit.total_rows, 2);
        // Old (mtime=1, size=1) key no longer matches.
        assert!(c.get_rows("f", "r", 1, 1).is_none());
    }

    #[test]
    fn profile_round_trips_independently_of_rows() {
        let c = fresh_cache();
        let profile = vec![ColumnProfile {
            column_name: "id".into(),
            column_type: "BIGINT".into(),
            count: Some(100),
            null_percentage: Some(0.0),
            approx_unique: Some(100),
            min: Some("1".into()),
            max: Some("100".into()),
            avg: Some("50.5".into()),
            std: None,
        }];
        c.put_profile("f", "r", 10, 20, &profile).unwrap();
        let hit = c.get_profile("f", "r", 10, 20).unwrap();
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].column_name, "id");
        // Storing a profile doesn't poison the rows table.
        assert!(c.get_rows("f", "r", 10, 20).is_none());
    }

    #[test]
    fn invalidate_folder_drops_both_tables() {
        let c = fresh_cache();
        c.put_rows("listing/", "a.parquet", 1, 1, &sample_rows())
            .unwrap();
        c.put_rows("listing/", "b.parquet", 1, 1, &sample_rows())
            .unwrap();
        c.put_rows("OTHER/", "c.parquet", 1, 1, &sample_rows())
            .unwrap();
        c.invalidate_folder("listing/").unwrap();
        assert!(c.get_rows("listing/", "a.parquet", 1, 1).is_none());
        assert!(c.get_rows("listing/", "b.parquet", 1, 1).is_none());
        // Other folder untouched.
        assert!(c.get_rows("OTHER/", "c.parquet", 1, 1).is_some());
    }

    #[test]
    fn corrupt_payload_returns_none_instead_of_panicking() {
        // The "schema evolved + we can't deserialize the old shape"
        // safety: a serde_json::from_str failure in get_payload should
        // act as a cache miss.
        let c = fresh_cache();
        c.conn
            .execute(
                r#"INSERT INTO row_preview VALUES (?, ?, ?, ?, ?, ?)"#,
                params![
                    "f",
                    "r",
                    1i64,
                    1i64,
                    json!({"this": "is not a DatasetRows"}).to_string(),
                    "2026-01-01T00:00:00Z",
                ],
            )
            .unwrap();
        assert!(c.get_rows("f", "r", 1, 1).is_none());
    }
}

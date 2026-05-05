//! Persistent scan cache — short-circuits `DatasetMetadata` extraction for
//! files whose (mtime, size) match the last scan.
//!
//! Backed by a DuckDB database at `~/.sery/scan_cache.db`. Keyed on
//! `(folder_path, relative_path)`. A cached entry is fresh when the
//! current file's mtime + size match the stored values — a change in
//! either invalidates the entry and forces a fresh extract.
//!
//! Why this exists: `FolderDetail` calls `scan_folder` on every visit,
//! which otherwise walks every file and runs DuckDB `DESCRIBE` + sample
//! queries on each one. For folders with hundreds of files that's a
//! noticeable pause every time. With the cache, a re-open of a folder
//! the user already scanned returns instantly — the on-disk walk is still
//! cheap, but the expensive per-file DuckDB work is skipped.
//!
//! Failure mode: every cache operation is best-effort. A DB error falls
//! through to full extraction, so a corrupted or missing cache never
//! blocks scans — it just costs the unoptimised path.

use chrono::Utc;
use duckdb::{params, Connection};
use once_cell::sync::Lazy;
use std::fs;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use crate::error::{AgentError, Result};
use crate::scanner::DatasetMetadata;

/// Process-wide singleton holding the one DuckDB connection to
/// `scan_cache.db`. Every caller — the scanner, `get_cached_folder_metadata`,
/// `remove_watched_folder` — goes through [`with_cache`] so DuckDB never
/// sees multiple concurrent connections to the same file. Opening the
/// file twice in quick succession (e.g. the scanner holding one while the
/// UI opened another for an instant-paint read) was throwing C++
/// exceptions from DuckDB's internal locking that aborted the process.
static GLOBAL_CACHE: Lazy<Mutex<Option<ScanCache>>> = Lazy::new(|| Mutex::new(None));

/// Run `f` with the shared cache. Returns `None` if the cache can't be
/// opened (first-time open failed AND every caller since) or if the
/// mutex is poisoned. The `f` closure runs inside the mutex so callers
/// MUST keep it short — one get/put, no long work.
pub fn with_cache<F, T>(f: F) -> Option<T>
where
    F: FnOnce(&ScanCache) -> T,
{
    let mut guard = match GLOBAL_CACHE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    if guard.is_none() {
        match ScanCache::new() {
            Ok(c) => *guard = Some(c),
            Err(e) => {
                // Without this log, a busted DB silently turns
                // every put into a no-op — the caller can't tell
                // their writes are being dropped, and the user
                // sees a folder that re-scans on every visit.
                eprintln!("[scan_cache] failed to open DB: {} (cache disabled this session)", e);
            }
        }
    }
    guard.as_ref().map(f)
}

pub struct ScanCache {
    conn: Connection,
}

/// One cached row: a single file's metadata plus the folder it lives in.
/// Returned by [`ScanCache::get_all_entries`] for global search; the
/// caller (`search_all_folders`) ranks matches by filename / column /
/// content against this flat list.
#[derive(Debug, Clone)]
pub struct CachedEntry {
    pub folder_path: String,
    pub relative_path: String,
    pub metadata: DatasetMetadata,
}

impl ScanCache {
    /// Open or create the persistent scan cache DB.
    pub fn new() -> Result<Self> {
        let cache_dir = dirs::data_local_dir()
            .ok_or_else(|| AgentError::Config("no local data dir".to_string()))?
            .join("sery");
        fs::create_dir_all(&cache_dir)
            .map_err(|e| AgentError::Config(format!("create cache dir: {}", e)))?;

        let db_path = cache_dir.join("scan_cache.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| AgentError::Database(format!("open scan cache: {}", e)))?;

        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS scan_cache (
                folder_path TEXT NOT NULL,
                relative_path TEXT NOT NULL,
                mtime_secs BIGINT NOT NULL,
                size_bytes BIGINT NOT NULL,
                metadata_json TEXT NOT NULL,
                cached_at TIMESTAMP NOT NULL,
                PRIMARY KEY (folder_path, relative_path)
            );
            "#,
            [],
        )
        .map_err(|e| AgentError::Database(format!("init scan cache schema: {}", e)))?;

        Ok(Self { conn })
    }

    /// Look up the freshness key (mtime, size) currently stored for a
    /// (folder, relative) entry, without comparing it to anything.
    /// Used by `preview_cache` to derive its own cache key from
    /// scan_cache's source-of-truth — keeps the two caches consistent
    /// without re-issuing a HEAD probe at preview time.
    pub fn get_freshness(
        &self,
        folder_path: &str,
        relative_path: &str,
    ) -> Option<(i64, i64)> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT mtime_secs, size_bytes FROM scan_cache
                 WHERE folder_path = ? AND relative_path = ?",
            )
            .ok()?;
        stmt.query_row(params![folder_path, relative_path], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })
        .ok()
    }

    /// Fetch a cached metadata row if (mtime, size) match. Returns `None`
    /// on miss, stale, or any DB/parse error — callers should fall back
    /// to full extraction.
    pub fn get(
        &self,
        folder_path: &str,
        relative_path: &str,
        current_mtime_secs: i64,
        current_size_bytes: i64,
    ) -> Option<DatasetMetadata> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT mtime_secs, size_bytes, metadata_json
                 FROM scan_cache
                 WHERE folder_path = ? AND relative_path = ?",
            )
            .ok()?;

        let (cached_mtime, cached_size, metadata_json): (i64, i64, String) = stmt
            .query_row(params![folder_path, relative_path], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .ok()?;

        if cached_mtime != current_mtime_secs || cached_size != current_size_bytes {
            return None;
        }

        // If the schema evolves in an incompatible way, deserialization
        // silently fails and we fall through to a fresh extract — same
        // as a cache miss. Prefer correctness over preserving a stale
        // row we can no longer parse.
        serde_json::from_str(&metadata_json).ok()
    }

    /// Upsert freshly-extracted metadata.
    pub fn put(
        &self,
        folder_path: &str,
        relative_path: &str,
        mtime_secs: i64,
        size_bytes: i64,
        metadata: &DatasetMetadata,
    ) -> Result<()> {
        let json = serde_json::to_string(metadata)
            .map_err(|e| AgentError::Serialization(format!("serialize metadata: {}", e)))?;

        self.conn
            .execute(
                r#"
                INSERT INTO scan_cache
                    (folder_path, relative_path, mtime_secs, size_bytes, metadata_json, cached_at)
                VALUES (?, ?, ?, ?, ?, ?)
                ON CONFLICT(folder_path, relative_path) DO UPDATE SET
                    mtime_secs = excluded.mtime_secs,
                    size_bytes = excluded.size_bytes,
                    metadata_json = excluded.metadata_json,
                    cached_at = excluded.cached_at
                "#,
                params![
                    folder_path,
                    relative_path,
                    mtime_secs,
                    size_bytes,
                    json,
                    Utc::now().to_rfc3339(),
                ],
            )
            .map_err(|e| AgentError::Database(format!("upsert scan cache: {}", e)))?;

        Ok(())
    }

    /// Return every cached metadata row for a folder, in deterministic
    /// order (by relative_path). Used by `FolderDetail` to paint
    /// instantly from the cache before the background rescan finishes.
    ///
    /// Entries whose stored JSON can't be parsed (schema evolution,
    /// corruption) are silently dropped rather than erroring out — the
    /// background rescan will re-populate them.
    pub fn get_all_for_folder(&self, folder_path: &str) -> Result<Vec<DatasetMetadata>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT metadata_json FROM scan_cache
                 WHERE folder_path = ?
                 ORDER BY relative_path",
            )
            .map_err(|e| AgentError::Database(format!("prepare folder query: {}", e)))?;

        let rows = stmt
            .query_map(params![folder_path], |row| row.get::<_, String>(0))
            .map_err(|e| AgentError::Database(format!("execute folder query: {}", e)))?;

        let mut out = Vec::new();
        for row in rows {
            match row {
                Ok(json) => {
                    if let Ok(meta) = serde_json::from_str::<DatasetMetadata>(&json) {
                        out.push(meta);
                    }
                }
                Err(_) => continue,
            }
        }
        Ok(out)
    }

    /// Return every cached row across every folder. Used by the global
    /// search page to score datasets by filename / column / content
    /// match without reopening files on disk. JSON rows that fail to
    /// parse are silently dropped — the scanner will rewrite them on
    /// the next visit.
    pub fn get_all_entries(&self) -> Result<Vec<CachedEntry>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT folder_path, relative_path, metadata_json
                 FROM scan_cache",
            )
            .map_err(|e| AgentError::Database(format!("prepare all-entries query: {}", e)))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| AgentError::Database(format!("execute all-entries query: {}", e)))?;

        let mut out = Vec::new();
        for row in rows {
            if let Ok((folder_path, relative_path, metadata_json)) = row {
                if let Ok(metadata) = serde_json::from_str::<DatasetMetadata>(&metadata_json) {
                    out.push(CachedEntry {
                        folder_path,
                        relative_path,
                        metadata,
                    });
                }
            }
        }
        Ok(out)
    }

    /// Drop every cached entry for a folder. Call this when the user
    /// removes a watched folder so the cache doesn't keep stale rows.
    pub fn invalidate_folder(&self, folder_path: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM scan_cache WHERE folder_path = ?",
                params![folder_path],
            )
            .map_err(|e| AgentError::Database(format!("invalidate folder: {}", e)))?;
        Ok(())
    }
}

/// Convenience struct: the three fields the scanner needs to use the cache.
/// Built once per file from `fs::Metadata` so we don't reach into the file
/// system twice.
#[derive(Debug, Clone)]
pub struct CacheKey {
    pub relative_path: String,
    pub mtime_secs: i64,
    pub size_bytes: i64,
}

impl CacheKey {
    pub fn from_metadata(
        path: &std::path::Path,
        folder_path: &str,
        meta: &fs::Metadata,
    ) -> Option<Self> {
        let relative_path = path
            .strip_prefix(folder_path)
            .ok()?
            .to_string_lossy()
            .to_string();
        let mtime_secs = meta
            .modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_secs() as i64;
        Some(Self {
            relative_path,
            mtime_secs,
            size_bytes: meta.len() as i64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::{ColumnSchema, DatasetMetadata};

    fn sample_metadata() -> DatasetMetadata {
        DatasetMetadata {
            relative_path: "orders.parquet".to_string(),
            file_format: "parquet".to_string(),
            size_bytes: 1024,
            row_count_estimate: Some(42),
            schema: vec![ColumnSchema {
                name: "id".to_string(),
                col_type: "BIGINT".to_string(),
                nullable: false,
            }],
            last_modified: "2026-01-01T00:00:00Z".to_string(),
            document_markdown: None,
            sample_rows: None,
            samples_redacted: false,
        }
    }

    // Tests share the real on-disk cache because ScanCache::new() resolves
    // its path from dirs::data_local_dir(). Each test uses a unique
    // folder_path so the rows never collide. Cleanup runs at the end.

    #[test]
    fn put_then_get_returns_exact_payload() {
        let cache = ScanCache::new().expect("open cache");
        let folder = "/fake/folder/scan_cache_roundtrip";
        let rel = "a.parquet";

        cache.put(folder, rel, 100, 1024, &sample_metadata()).unwrap();
        let hit = cache.get(folder, rel, 100, 1024);
        assert!(hit.is_some());
        let m = hit.unwrap();
        assert_eq!(m.relative_path, "orders.parquet");
        assert_eq!(m.row_count_estimate, Some(42));
        assert_eq!(m.schema.len(), 1);

        cache.invalidate_folder(folder).unwrap();
    }

    #[test]
    fn mtime_change_invalidates() {
        let cache = ScanCache::new().expect("open cache");
        let folder = "/fake/folder/scan_cache_mtime";
        let rel = "b.parquet";

        cache.put(folder, rel, 100, 1024, &sample_metadata()).unwrap();
        // Same size, different mtime → miss.
        assert!(cache.get(folder, rel, 200, 1024).is_none());

        cache.invalidate_folder(folder).unwrap();
    }

    #[test]
    fn size_change_invalidates() {
        let cache = ScanCache::new().expect("open cache");
        let folder = "/fake/folder/scan_cache_size";
        let rel = "c.parquet";

        cache.put(folder, rel, 100, 1024, &sample_metadata()).unwrap();
        // Same mtime, different size → miss. Guards against the
        // sub-second-mtime case where a file is rewritten in the same
        // second as the previous scan.
        assert!(cache.get(folder, rel, 100, 2048).is_none());

        cache.invalidate_folder(folder).unwrap();
    }

    #[test]
    fn put_overwrites_previous_entry() {
        let cache = ScanCache::new().expect("open cache");
        let folder = "/fake/folder/scan_cache_overwrite";
        let rel = "d.parquet";

        cache.put(folder, rel, 100, 1024, &sample_metadata()).unwrap();

        let mut updated = sample_metadata();
        updated.row_count_estimate = Some(999);
        cache.put(folder, rel, 200, 2048, &updated).unwrap();

        // Old key no longer matches.
        assert!(cache.get(folder, rel, 100, 1024).is_none());
        // New key returns the updated row count.
        let hit = cache.get(folder, rel, 200, 2048).unwrap();
        assert_eq!(hit.row_count_estimate, Some(999));

        cache.invalidate_folder(folder).unwrap();
    }

    #[test]
    fn invalidate_folder_drops_all_rows_for_that_folder() {
        let cache = ScanCache::new().expect("open cache");
        let folder = "/fake/folder/scan_cache_invalidate";

        cache.put(folder, "one.parquet", 1, 1, &sample_metadata()).unwrap();
        cache.put(folder, "two.parquet", 2, 2, &sample_metadata()).unwrap();

        assert!(cache.get(folder, "one.parquet", 1, 1).is_some());
        assert!(cache.get(folder, "two.parquet", 2, 2).is_some());

        cache.invalidate_folder(folder).unwrap();
        assert!(cache.get(folder, "one.parquet", 1, 1).is_none());
        assert!(cache.get(folder, "two.parquet", 2, 2).is_none());
    }
}

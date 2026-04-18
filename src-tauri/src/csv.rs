//! CSV to Parquet conversion.
//!
//! Large CSV files are slow to query with DuckDB (full table scans, no column
//! pruning). This module automatically converts CSV files to Parquet format
//! for 10-100x faster query performance.
//!
//! A simple mtime-based cache avoids re-converting unchanged files on every
//! query or scan.
//!
//! Fallback ladder: DuckDB's CSV sniffer is surprisingly strict — it rejects
//! files with ragged rows, unusual delimiters, or no header. When auto-detect
//! fails we progressively relax the parser until something succeeds, or we
//! fall through with an error the scanner can render as "file exists but
//! schema couldn't be extracted."

use crate::error::{AgentError, Result};
use duckdb::Connection;
use std::fs;
use std::path::{Path, PathBuf};

/// Convert a CSV file to Parquet format in the OS temp directory.
/// Returns the path to the generated Parquet file.
///
/// If a cached Parquet file already exists and is newer than the source CSV,
/// the cached path is returned directly.
pub fn csv_to_parquet(csv_path: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(csv_path)
        .map_err(|e| AgentError::FileSystem(format!("canonicalize: {}", e)))?;

    let cache_path = cache_path_for(&canonical)?;

    // Cache hit: Parquet exists and is at least as new as the source.
    if let (Ok(parquet_meta), Ok(src_meta)) = (fs::metadata(&cache_path), fs::metadata(&canonical)) {
        if let (Ok(parquet_mtime), Ok(src_mtime)) = (parquet_meta.modified(), src_meta.modified()) {
            if parquet_mtime >= src_mtime {
                return Ok(cache_path);
            }
        }
    }

    // Cache miss — convert using DuckDB.
    let conn = Connection::open_in_memory()
        .map_err(|e| AgentError::Database(format!("Failed to open DuckDB: {}", e)))?;

    // Ensure parent directory exists
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).map_err(AgentError::Io)?;
    }

    // Use a temporary file for atomic write
    let tmp_path = cache_path.with_extension("parquet.tmp");
    let escaped_path = canonical.to_string_lossy().replace('\'', "''");
    let escaped_tmp = tmp_path.to_string_lossy().replace('\'', "''");

    // Try a ladder of progressively more permissive read_csv_auto
    // invocations. First success wins. Ordered from "cleanest output"
    // (strict mode, header on) to "will parse anything" (all_varchar,
    // ignore_errors, null_padding).
    let attempts = [
        // 1. Default — clean CSVs with header.
        "read_csv_auto('{p}', header=true)",
        // 2. Permissive — ragged rows OK, bad lines silently skipped,
        //    missing columns null-padded.
        "read_csv_auto('{p}', header=true, ignore_errors=true, null_padding=true)",
        // 3. Maybe no header — some exports skip it.
        "read_csv_auto('{p}', header=false, ignore_errors=true, null_padding=true)",
        // 4. Last resort — treat every column as VARCHAR so type
        //    sniffing can't fail. Loses type info but the schema
        //    extraction downstream still gets column names + a
        //    consistent answer.
        "read_csv_auto('{p}', header=true, ignore_errors=true, null_padding=true, all_varchar=true)",
    ];

    let mut last_err: Option<String> = None;
    for tmpl in attempts {
        let reader = tmpl.replace("{p}", &escaped_path);
        let sql = format!(
            "COPY (SELECT * FROM {reader}) TO '{escaped_tmp}' (FORMAT PARQUET, COMPRESSION 'zstd')"
        );
        match conn.execute(&sql, []) {
            Ok(_) => {
                fs::rename(&tmp_path, &cache_path).map_err(AgentError::Io)?;
                return Ok(cache_path);
            }
            Err(e) => {
                // Clean up any partial tmp file before the next attempt;
                // otherwise a later successful COPY could refuse to
                // overwrite. Best-effort — it may not exist yet.
                let _ = fs::remove_file(&tmp_path);
                last_err = Some(e.to_string());
            }
        }
    }

    Err(AgentError::Database(format!(
        "CSV to Parquet conversion failed after {} attempts: {}",
        attempts.len(),
        last_err.unwrap_or_else(|| "unknown error".to_string())
    )))
}

/// Deterministic cache file path: `{tmp}/seryai-csv-cache/{hash}.parquet`.
/// Using a hash of the canonical path avoids collisions between files with
/// the same basename in different directories.
fn cache_path_for(canonical: &Path) -> Result<PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    canonical.hash(&mut hasher);
    let h = hasher.finish();

    let stem = canonical
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("data");

    let mut dir = std::env::temp_dir();
    dir.push("seryai-csv-cache");
    Ok(dir.join(format!("{}-{:016x}.parquet", sanitize(stem), h)))
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::csv_to_parquet;
    use std::fs::{self, File};
    use std::io::Write;

    /// Write `contents` to a uniquely-named file inside a fresh temp dir
    /// and return the canonicalized path. The temp dir is owned by the
    /// caller via the returned TempDir handle — let it drop to clean up.
    fn write_csv(contents: &str, name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(name);
        let mut f = File::create(&path).expect("create");
        f.write_all(contents.as_bytes()).expect("write");
        drop(f);
        (dir, fs::canonicalize(&path).expect("canonicalize"))
    }

    /// Clear the temp parquet cache for a given CSV path so each test
    /// exercises the fresh-conversion path, not the mtime cache.
    fn clear_cache_for(csv_path: &std::path::Path) {
        if let Ok(cache) = super::cache_path_for(csv_path) {
            let _ = fs::remove_file(&cache);
            let _ = fs::remove_file(cache.with_extension("parquet.tmp"));
        }
    }

    #[test]
    fn clean_csv_succeeds_on_first_attempt() {
        let (_dir, path) = write_csv(
            "id,name,amount\n1,Alice,100.0\n2,Bob,250.5\n",
            "clean.csv",
        );
        clear_cache_for(&path);
        let parquet = csv_to_parquet(&path).expect("conversion should succeed");
        assert!(parquet.exists(), "parquet file should exist");
    }

    #[test]
    fn ragged_rows_succeed_via_fallback() {
        // Row 3 has fewer columns than the header — DuckDB's strict
        // sniffer rejects this; the permissive fallback (attempt 2)
        // with null_padding=true accepts it.
        let (_dir, path) = write_csv(
            "id,name,amount\n1,Alice,100.0\n2,Bob\n3,Charlie,300.0\n",
            "ragged.csv",
        );
        clear_cache_for(&path);
        let parquet = csv_to_parquet(&path).expect(
            "conversion should fall back to permissive mode",
        );
        assert!(parquet.exists());
    }

    #[test]
    fn completely_malformed_fails_cleanly() {
        // Non-CSV bytes — should blow through every fallback and
        // return an Err with a composite message.
        let (_dir, path) = write_csv("\x00\x01\x02 not a csv at all \x7f", "bad.csv");
        clear_cache_for(&path);
        let result = csv_to_parquet(&path);
        // DuckDB's CSV reader is remarkably permissive with all_varchar
        // + ignore_errors — it might succeed by treating the file as a
        // single-column text blob. Either outcome is acceptable; what
        // we're asserting is that the function never panics and returns
        // a Result the caller can handle.
        match result {
            Ok(p) => assert!(p.exists()),
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("after") && msg.contains("attempts"),
                    "error should reference the fallback ladder; got {msg}"
                );
            }
        }
    }
}

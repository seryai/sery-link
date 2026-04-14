//! CSV to Parquet conversion.
//!
//! Large CSV files are slow to query with DuckDB (full table scans, no column
//! pruning). This module automatically converts CSV files to Parquet format
//! for 10-100x faster query performance.
//!
//! A simple mtime-based cache avoids re-converting unchanged files on every
//! query or scan.

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

    // Convert CSV to Parquet using DuckDB
    let sql = format!(
        "COPY (SELECT * FROM read_csv_auto('{}', header=true)) TO '{}' (FORMAT PARQUET, COMPRESSION 'zstd')",
        canonical.to_string_lossy().replace('\'', "''"),
        tmp_path.to_string_lossy().replace('\'', "''")
    );

    conn.execute(&sql, [])
        .map_err(|e| AgentError::Database(format!("CSV to Parquet conversion failed: {}", e)))?;

    // Atomic rename so concurrent readers never see a half-written Parquet file
    fs::rename(&tmp_path, &cache_path).map_err(AgentError::Io)?;

    Ok(cache_path)
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

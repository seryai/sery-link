//! Excel (.xlsx / .xls) support.
//!
//! DuckDB's bundled build does not ship with the `excel` community extension,
//! and downloading extensions at runtime is unreliable inside a sandboxed app.
//! Instead we use the `calamine` crate to read the first worksheet in a
//! workbook and materialize it as a CSV file in the OS temp directory. The
//! resulting CSV path is then handed to DuckDB's `read_csv_auto`, so the rest
//! of the query / scanner pipeline is unchanged.
//!
//! A simple mtime-based cache avoids re-converting unchanged files on every
//! query or scan.

use crate::error::{AgentError, Result};
use calamine::{open_workbook_auto, Data, Reader};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Convert the first sheet of an xlsx/xls workbook to a CSV file in the OS
/// temp directory. Returns the path to the generated CSV.
///
/// If a cached CSV already exists and is newer than the source workbook, the
/// cached path is returned directly.
pub fn xlsx_to_csv(xlsx_path: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(xlsx_path)
        .map_err(|e| AgentError::FileSystem(format!("canonicalize: {}", e)))?;

    let cache_path = cache_path_for(&canonical)?;

    // Cache hit: CSV exists and is at least as new as the source.
    if let (Ok(csv_meta), Ok(src_meta)) = (fs::metadata(&cache_path), fs::metadata(&canonical)) {
        if let (Ok(csv_mtime), Ok(src_mtime)) = (csv_meta.modified(), src_meta.modified()) {
            if csv_mtime >= src_mtime {
                return Ok(cache_path);
            }
        }
    }

    // Cache miss — convert.
    let mut workbook = open_workbook_auto(&canonical)
        .map_err(|e| AgentError::Database(format!("open xlsx: {}", e)))?;

    let sheet_name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| AgentError::Database("xlsx has no sheets".to_string()))?;

    let range = workbook
        .worksheet_range(&sheet_name)
        .map_err(|e| AgentError::Database(format!("read sheet: {}", e)))?;

    // Ensure parent exists (tempdir always exists, but be safe).
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).map_err(AgentError::Io)?;
    }

    let tmp_path = cache_path.with_extension("csv.tmp");
    {
        let mut out = fs::File::create(&tmp_path).map_err(AgentError::Io)?;

        for row in range.rows() {
            let cells: Vec<String> = row.iter().map(cell_to_csv).collect();
            writeln!(out, "{}", cells.join(",")).map_err(AgentError::Io)?;
        }
    }

    // Atomic rename so concurrent readers never see a half-written CSV.
    fs::rename(&tmp_path, &cache_path).map_err(AgentError::Io)?;

    Ok(cache_path)
}

/// Convert a calamine `Data` cell to a CSV-safe string. Handles quoting of
/// fields containing commas, quotes, or newlines per RFC 4180.
fn cell_to_csv(cell: &Data) -> String {
    let raw = match cell {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) => format_float(*f),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Data::DateTime(dt) => format!("{}", dt),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("#ERR:{:?}", e),
    };

    if raw.contains(',') || raw.contains('"') || raw.contains('\n') || raw.contains('\r') {
        let escaped = raw.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        raw
    }
}

fn format_float(f: f64) -> String {
    // Avoid `1.0` becoming `1` — keep deterministic output.
    if f.fract() == 0.0 && f.abs() < 1e16 {
        format!("{:.0}", f)
    } else {
        f.to_string()
    }
}

/// Deterministic cache file path: `{tmp}/seryai-xlsx-cache/{hash}.csv`.
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
        .unwrap_or("sheet");

    let mut dir = std::env::temp_dir();
    dir.push("seryai-xlsx-cache");
    Ok(dir.join(format!("{}-{:016x}.csv", sanitize(stem), h)))
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

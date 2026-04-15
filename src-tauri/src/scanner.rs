//! Folder scanner — walks a directory, filters noise, and extracts schema +
//! row counts from each data file.
//!
//! Improvements over the original MVP:
//!   * Exclude patterns (globs) so `.DS_Store`, `node_modules`, temporary
//!     lock files, etc. never get touched.
//!   * `max_file_size_mb` cap so a rogue 50 GB dump doesn't take the agent
//!     offline — oversized files are skipped with a logged warning instead
//!     of blocking the scan.
//!   * Optional progress callback so callers can surface per-file scan
//!     progress to the UI without extra plumbing.

use crate::config::{Config, WatchedFolder};
use crate::error::{AgentError, Result};
use crate::excel;
use duckdb::Connection;
use glob::Pattern;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::io::Write;
use walkdir::WalkDir;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetMetadata {
    pub relative_path: String,
    pub file_format: String,
    pub size_bytes: u64,
    pub row_count_estimate: Option<i64>,
    pub schema: Vec<ColumnSchema>,
    pub last_modified: String,
    /// Extracted markdown text for document files (DOCX, PPTX, HTML, etc.).
    /// `None` for tabular files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_markdown: Option<String>,
}

/// File extensions classified as document (non-tabular) types.
const DOCUMENT_EXTENSIONS: &[&str] = &["docx", "pptx", "html", "htm", "ipynb"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnSchema {
    pub name: String,
    #[serde(rename = "type")]
    pub col_type: String,
    pub nullable: bool,
}

/// Callback invoked once per file as the scan progresses. Matches the shape
/// of the `ScanProgress` event so the watcher can adapt it trivially.
pub type ProgressCb = Box<dyn Fn(usize, usize, &str) + Send + Sync>;

// ---------------------------------------------------------------------------
// Public scan entry points
// ---------------------------------------------------------------------------

/// Simple scan — no progress callback, resolves the watched-folder settings
/// from config so exclude patterns and file size limits are honoured.
pub async fn scan_folder(folder_path: &str) -> Result<Vec<DatasetMetadata>> {
    let owned = folder_path.to_string();
    tokio::task::spawn_blocking(move || {
        let settings = load_folder_settings(&owned);
        scan_folder_blocking(&owned, &settings, None)
    })
    .await
    .map_err(|e| AgentError::FileSystem(format!("Scan task failed: {}", e)))?
}

/// Scan with a progress callback. Used by the rescan command + watcher when
/// the UI wants to show a live progress bar.
pub async fn scan_folder_with_progress(
    folder_path: &str,
    progress: ProgressCb,
) -> Result<Vec<DatasetMetadata>> {
    let owned = folder_path.to_string();
    tokio::task::spawn_blocking(move || {
        let settings = load_folder_settings(&owned);
        scan_folder_blocking(&owned, &settings, Some(progress))
    })
    .await
    .map_err(|e| AgentError::FileSystem(format!("Scan task failed: {}", e)))?
}

// ---------------------------------------------------------------------------
// Blocking implementation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct FolderSettings {
    exclude_patterns: Vec<Pattern>,
    max_file_size_bytes: u64,
}

fn load_folder_settings(folder_path: &str) -> FolderSettings {
    // Pull the matching WatchedFolder from config; fall back to sane defaults
    // when the folder isn't registered (e.g. a one-shot CLI scan).
    let config = Config::load().ok();
    let folder: Option<WatchedFolder> = config.and_then(|c| {
        c.watched_folders
            .into_iter()
            .find(|f| f.path == folder_path)
    });

    match folder {
        Some(f) => FolderSettings {
            exclude_patterns: compile_patterns(&f.exclude_patterns),
            max_file_size_bytes: f.max_file_size_mb.saturating_mul(1024 * 1024),
        },
        None => FolderSettings {
            exclude_patterns: compile_patterns(&[
                ".DS_Store".to_string(),
                "__MACOSX".to_string(),
                ".git".to_string(),
                "node_modules".to_string(),
                "~$*".to_string(),
            ]),
            max_file_size_bytes: 1024 * 1024 * 1024, // 1 GB default
        },
    }
}

fn compile_patterns(globs: &[String]) -> Vec<Pattern> {
    globs
        .iter()
        .filter_map(|g| Pattern::new(g).ok())
        .collect()
}

fn scan_folder_blocking(
    folder_path: &str,
    settings: &FolderSettings,
    progress: Option<ProgressCb>,
) -> Result<Vec<DatasetMetadata>> {
    // First pass — enumerate candidate files so the progress callback can
    // report "current/total" counts. This costs a second walk but keeps the
    // UX responsive, and walking is cheap relative to schema extraction.
    let candidates: Vec<_> = WalkDir::new(folder_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| is_supported(e.path()))
        .filter(|e| !is_excluded(e.path(), folder_path, &settings.exclude_patterns))
        .collect();

    let total = candidates.len();
    let mut datasets = Vec::with_capacity(total);

    for (idx, entry) in candidates.into_iter().enumerate() {
        let path = entry.path();

        if let Some(cb) = &progress {
            cb(idx + 1, total, &path.to_string_lossy());
        }

        // Enforce the file size cap — oversized files are logged and skipped
        // so one bad file doesn't take the whole scan down.
        if let Ok(meta) = fs::metadata(path) {
            if meta.len() > settings.max_file_size_bytes {
                eprintln!(
                    "[scanner] skipping {} ({} bytes, exceeds {} MB cap)",
                    path.display(),
                    meta.len(),
                    settings.max_file_size_bytes / (1024 * 1024)
                );
                continue;
            }
        }

        match extract_metadata(path, folder_path) {
            Ok(metadata) => datasets.push(metadata),
            Err(e) => eprintln!("[scanner] failed to extract metadata from {:?}: {}", path, e),
        }
    }

    Ok(datasets)
}

fn is_supported(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()).unwrap_or(""),
        "parquet" | "csv" | "xlsx" | "xls"
        | "docx" | "pptx" | "html" | "htm" | "ipynb"
    )
}

fn is_document_ext(ext: &str) -> bool {
    DOCUMENT_EXTENSIONS.contains(&ext)
}

fn is_excluded(path: &Path, base: &str, patterns: &[Pattern]) -> bool {
    // Match both full path, file name, and each intermediate component so a
    // single `node_modules` pattern covers every nested folder.
    let rel = path.strip_prefix(base).unwrap_or(path);
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let rel_str = rel.to_string_lossy();

    for p in patterns {
        if p.matches(file_name) || p.matches(&rel_str) {
            return true;
        }
        for component in rel.components() {
            if let Some(s) = component.as_os_str().to_str() {
                if p.matches(s) {
                    return true;
                }
            }
        }
    }
    false
}

fn extract_metadata(file_path: &Path, base_path: &str) -> Result<DatasetMetadata> {
    let file_metadata = fs::metadata(file_path)
        .map_err(|e| AgentError::FileSystem(format!("Failed to read file metadata: {}", e)))?;

    let relative_path = file_path
        .strip_prefix(base_path)
        .map_err(|e| AgentError::FileSystem(format!("Failed to get relative path: {}", e)))?
        .to_string_lossy()
        .to_string();

    let ext = file_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    let last_modified = file_metadata
        .modified()
        .ok()
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    if is_document_ext(ext) {
        // Document files — extract markdown via anytomd
        let document_markdown = extract_document_markdown(file_path, ext);
        Ok(DatasetMetadata {
            relative_path,
            file_format: ext.to_string(),
            size_bytes: file_metadata.len(),
            row_count_estimate: None,
            schema: vec![],
            last_modified,
            document_markdown,
        })
    } else {
        // Tabular files — DuckDB schema extraction
        let (schema, row_count) = extract_schema(file_path, ext, &file_metadata)?;
        Ok(DatasetMetadata {
            relative_path,
            file_format: ext.to_string(),
            size_bytes: file_metadata.len(),
            row_count_estimate: Some(row_count),
            schema,
            last_modified,
            document_markdown: None,
        })
    }
}

/// Convert a document file to markdown using the MarkItDown sidecar.
/// Falls back to anytomd if the sidecar fails.
/// Returns `Some(markdown)` on success, `None` on error (logged and skipped).
fn extract_document_markdown(file_path: &Path, ext: &str) -> Option<String> {
    // Cap at 50 MB
    let bytes = match fs::read(file_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!(
                "[scanner] failed to read document {:?}: {}",
                file_path, e
            );
            return None;
        }
    };

    if bytes.len() > 50 * 1024 * 1024 {
        eprintln!(
            "[scanner] document {:?} exceeds 50 MB, skipping conversion",
            file_path
        );
        return None;
    }

    // Try sidecar first (MarkItDown)
    if let Some(markdown) = try_sidecar_conversion(file_path) {
        eprintln!("[scanner] ✅ MarkItDown sidecar converted {:?}", file_path);
        return Some(markdown);
    }

    // Fallback to anytomd (Rust-native, faster but less capable)
    eprintln!("[scanner] ⚠️ Sidecar failed for {:?}, trying anytomd fallback", file_path);
    match anytomd::convert_bytes(&bytes, ext, &anytomd::ConversionOptions::default()) {
        Ok(result) => {
            eprintln!("[scanner] ✅ anytomd converted {:?}", file_path);
            Some(result.markdown)
        },
        Err(e) => {
            eprintln!(
                "[scanner] ❌ Both sidecar and anytomd failed for {:?}: {}",
                file_path, e
            );
            None
        }
    }
}

/// Call the MarkItDown sidecar binary to convert a document.
/// Returns `Some(markdown)` on success, `None` on failure.
fn try_sidecar_conversion(file_path: &Path) -> Option<String> {
    // Construct sidecar binary path (bundled with the app)
    let sidecar_path = if cfg!(target_os = "macos") {
        // macOS: sidecar is in .app/Contents/MacOS/
        std::env::current_exe()
            .ok()?
            .parent()?
            .join("markitdown-sidecar")
    } else if cfg!(target_os = "windows") {
        std::env::current_exe()
            .ok()?
            .parent()?
            .join("markitdown-sidecar.exe")
    } else {
        // Linux
        std::env::current_exe()
            .ok()?
            .parent()?
            .join("markitdown-sidecar")
    };

    if !sidecar_path.exists() {
        eprintln!("[scanner] sidecar not found at {:?}", sidecar_path);
        return None;
    }

    // Spawn the sidecar process
    let mut child = match Command::new(&sidecar_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[scanner] failed to spawn sidecar: {}", e);
            return None;
        }
    };

    // Write the file path to stdin
    if let Some(mut stdin) = child.stdin.take() {
        let path_str = file_path.to_string_lossy();
        if let Err(e) = stdin.write_all(path_str.as_bytes()) {
            eprintln!("[scanner] failed to write to sidecar stdin: {}", e);
            return None;
        }
        drop(stdin); // Close stdin to signal EOF
    }

    // Read the JSON response from stdout
    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[scanner] failed to read sidecar output: {}", e);
            return None;
        }
    };

    if !output.status.success() {
        eprintln!(
            "[scanner] sidecar exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        return None;
    }

    // Parse JSON response
    #[derive(Deserialize)]
    struct SidecarResponse {
        success: bool,
        markdown: Option<String>,
        error: Option<String>,
    }

    match serde_json::from_slice::<SidecarResponse>(&output.stdout) {
        Ok(response) => {
            if response.success {
                response.markdown
            } else {
                eprintln!(
                    "[scanner] sidecar conversion failed: {}",
                    response.error.unwrap_or_else(|| "unknown error".to_string())
                );
                None
            }
        }
        Err(e) => {
            eprintln!("[scanner] failed to parse sidecar JSON: {}", e);
            eprintln!("[scanner] stdout was: {}", String::from_utf8_lossy(&output.stdout));
            None
        }
    }
}

fn extract_schema(
    file_path: &Path,
    ext: &str,
    _file_metadata: &fs::Metadata,
) -> Result<(Vec<ColumnSchema>, i64)> {
    let conn = Connection::open_in_memory()
        .map_err(|e| AgentError::Database(format!("Failed to open DuckDB: {}", e)))?;

    // xlsx is transparently converted to cached CSV, then to Parquet.
    // csv is transparently converted to cached Parquet for 10-100x faster queries.
    // This keeps the read_func pipeline uniform downstream (always Parquet).
    let (effective_path, effective_ext): (Cow<Path>, &str) = match ext {
        "xlsx" | "xls" => {
            let csv = excel::xlsx_to_csv(file_path)?;
            let parquet = crate::csv::csv_to_parquet(&csv)?;
            (Cow::Owned(parquet), "parquet")
        },
        "csv" => {
            let parquet = crate::csv::csv_to_parquet(file_path)?;
            (Cow::Owned(parquet), "parquet")
        },
        _ => (Cow::Borrowed(file_path), ext)
    };
    let path_str = effective_path.to_string_lossy();

    let (read_func, count_sql) = match effective_ext {
        "parquet" => (
            "read_parquet",
            format!("SELECT COUNT(*) FROM read_parquet('{}')", path_str),
        ),
        "csv" => (
            "read_csv_auto",
            format!("SELECT COUNT(*) FROM read_csv_auto('{}')", path_str),
        ),
        _ => {
            return Err(AgentError::Database(format!("Unsupported format: {}", ext)))
        }
    };

    let schema_sql = format!("DESCRIBE SELECT * FROM {}('{}')", read_func, path_str);

    let mut columns = Vec::new();
    let mut stmt = conn
        .prepare(&schema_sql)
        .map_err(|e| AgentError::Database(format!("Failed to read schema: {}", e)))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,  // column_name
                row.get::<_, String>(1)?,  // column_type
            ))
        })
        .map_err(|e| AgentError::Database(format!("Failed to read schema: {}", e)))?;

    for row in rows {
        if let Ok((name, col_type)) = row {
            columns.push(ColumnSchema {
                name,
                col_type,
                nullable: true,
            });
        }
    }

    let row_count: i64 = match conn.query_row(&count_sql, [], |row| row.get(0)) {
        Ok(count) => count,
        Err(_) => {
            // Fallback to a byte-based estimate when counting fails (e.g. the
            // file is partially written or too large for a quick scan).
            let file_size = fs::metadata(file_path).map(|m| m.len()).unwrap_or(0);
            (file_size / 100) as i64
        }
    };

    Ok((columns, row_count))
}

// ---------------------------------------------------------------------------
// Sync to cloud
// ---------------------------------------------------------------------------

pub async fn sync_metadata_to_cloud(
    api_url: &str,
    token: &str,
    datasets: Vec<DatasetMetadata>,
) -> Result<serde_json::Value> {
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/v1/agent/sync-metadata", api_url))
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({ "datasets": datasets }))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(AgentError::Network(format!(
            "Metadata sync failed ({}): {}",
            status, error_text
        )));
    }

    let result: serde_json::Value = response.json().await?;
    Ok(result)
}

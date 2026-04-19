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

    /// Up to 5 sample rows from the file, PII-scrubbed. Used by the
    /// cloud LLM for better SQL grounding. See SPEC_BACKEND_UNBLOCK.md
    /// §Metadata enrichment. `None` for documents + files that fail
    /// sampling. Serialised only when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rows: Option<Vec<serde_json::Map<String, serde_json::Value>>>,

    /// `true` if the sampler substituted redacted placeholders for any
    /// PII-looking column values. Always sent so the backend knows to
    /// caveat LLM answers that rely on sampled values.
    #[serde(default)]
    pub samples_redacted: bool,
}

/// File extensions classified as document (non-tabular) types.
const DOCUMENT_EXTENSIONS: &[&str] = &["docx", "pptx", "html", "htm", "ipynb"];

/// How much work the scanner should do for a given file.
///
/// - `Full`: extract schema, row count, and sample rows. Only makes sense for
///   tabular formats where every column matters to downstream query.
/// - `Content`: extract markdown text but skip schema/samples. For docs where
///   searchable content is the whole point.
/// - `Shallow`: record file-system facts only (path, size, mtime). Used for
///   formats where content extraction is expensive per-file (sidecar spawn,
///   full-document parse) and the marginal signal isn't worth the wall-time —
///   the user can still find the file by name and locate it in Finder.
///
/// Defaults per-extension live in [`default_tier_for`]; users override via
/// `config.sync.scan_tier_overrides`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanTier {
    Full,
    Content,
    Shallow,
}

/// Map an extension to the default tier. Tuned for the common case: tabular
/// files are cheap + valuable so go Full; DOCX/PPTX are slow per-file but
/// document content is usually the reason someone's scanning them, so Content;
/// HTML and IPYNB are often dumped in bulk (saved pages, notebook exports)
/// and each one pays the sidecar spawn cost for questionable value, so
/// Shallow. An unrecognised extension falls through to Shallow — we'll still
/// index the filename but won't waste time trying to parse it.
fn default_tier_for(ext: &str) -> ScanTier {
    match ext {
        "parquet" | "csv" | "xlsx" | "xls" => ScanTier::Full,
        "docx" | "pptx" => ScanTier::Content,
        "html" | "htm" | "ipynb" => ScanTier::Shallow,
        _ => ScanTier::Shallow,
    }
}

/// Resolve the tier for a given extension, honouring user overrides from
/// config. Override values are matched case-insensitively against the tier
/// names; anything unrecognised is ignored.
fn tier_for(ext: &str, overrides: &std::collections::HashMap<String, String>) -> ScanTier {
    if let Some(raw) = overrides.get(ext) {
        match raw.to_ascii_lowercase().as_str() {
            "full" => return ScanTier::Full,
            "content" => return ScanTier::Content,
            "shallow" => return ScanTier::Shallow,
            _ => {} // unknown → fall through to default
        }
    }
    default_tier_for(ext)
}

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

/// Callback invoked once per file immediately after its metadata has been
/// extracted (cache hit or miss). The callback receives the 1-based index,
/// total file count, and the dataset itself — giving progressive UIs
/// everything they need to render rows as they land.
pub type DatasetCb = Box<dyn Fn(usize, usize, &DatasetMetadata) + Send + Sync>;

// ---------------------------------------------------------------------------
// Public scan entry points
// ---------------------------------------------------------------------------

/// Simple scan — no progress callback, resolves the watched-folder settings
/// from config so exclude patterns and file size limits are honoured.
pub async fn scan_folder(folder_path: &str) -> Result<Vec<DatasetMetadata>> {
    if crate::url::is_remote_url(folder_path) {
        return scan_remote_folder(folder_path, None, None).await;
    }
    let owned = folder_path.to_string();
    tokio::task::spawn_blocking(move || {
        let settings = load_folder_settings(&owned);
        scan_folder_blocking(&owned, &settings, None, None)
    })
    .await
    .map_err(|e| AgentError::FileSystem(format!("Scan task failed: {}", e)))?
}

/// Scan with both a per-file progress callback AND a per-dataset callback
/// that fires after each file's metadata is extracted. Used by
/// `rescan_folder` so FolderDetail can stream rows in as they land instead
/// of waiting for the whole folder to finish.
pub async fn scan_folder_with_events(
    folder_path: &str,
    progress: Option<ProgressCb>,
    on_dataset: Option<DatasetCb>,
) -> Result<Vec<DatasetMetadata>> {
    if crate::url::is_remote_url(folder_path) {
        return scan_remote_folder(folder_path, progress, on_dataset).await;
    }
    let owned = folder_path.to_string();
    tokio::task::spawn_blocking(move || {
        let settings = load_folder_settings(&owned);
        scan_folder_blocking(&owned, &settings, progress, on_dataset)
    })
    .await
    .map_err(|e| AgentError::FileSystem(format!("Scan task failed: {}", e)))?
}

/// URL-based "folder" scan. A remote source is always one file (Phase A),
/// so we skip the rayon fan-out and run one HEAD probe + one DuckDB
/// DESCRIBE query. Produces exactly one `DatasetMetadata` that ends up
/// in the scan cache keyed on (url, "", last_modified_secs,
/// content_length) — same shape as a local file, just with synthetic
/// cache values.
async fn scan_remote_folder(
    url: &str,
    progress: Option<ProgressCb>,
    on_dataset: Option<DatasetCb>,
) -> Result<Vec<DatasetMetadata>> {
    if let Some(cb) = &progress {
        cb(1, 1, url);
    }

    let head = match crate::remote::head_probe(url).await {
        Ok(h) => h,
        Err(e) => {
            // A HEAD failure is not fatal by itself — the server may
            // just not support HEAD. Fall back to empty freshness hints
            // and let the DuckDB query decide whether the URL is
            // actually reachable.
            eprintln!("[scanner] HEAD probe failed for {}: {} — continuing", url, e);
            crate::remote::RemoteHeadInfo::default()
        }
    };

    let url_owned = url.to_string();
    let head_owned = head.clone();
    let metadata = tokio::task::spawn_blocking(move || {
        // Serve from cache first when the (url, mtime, size) key matches —
        // avoids a second DuckDB hit on subsequent FolderDetail visits.
        let cache_hit = crate::scan_cache::with_cache(|c| {
            c.get(
                &url_owned,
                "",
                head_owned.last_modified_secs.unwrap_or(0),
                head_owned.content_length.unwrap_or(0),
            )
        })
        .flatten();
        if let Some(hit) = cache_hit {
            return Ok(hit);
        }

        let meta = crate::remote::scan_remote_blocking(&url_owned, &head_owned)?;

        // Persist so next time we short-circuit — same freshness key as
        // the cache hit path above.
        let _ = crate::scan_cache::with_cache(|c| {
            c.put(
                &url_owned,
                "",
                head_owned.last_modified_secs.unwrap_or(0),
                head_owned.content_length.unwrap_or(0),
                &meta,
            )
        });
        Ok::<_, AgentError>(meta)
    })
    .await
    .map_err(|e| AgentError::FileSystem(format!("Remote scan task failed: {}", e)))??;

    if let Some(cb) = &on_dataset {
        cb(1, 1, &metadata);
    }
    Ok(vec![metadata])
}

// ---------------------------------------------------------------------------
// Blocking implementation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct FolderSettings {
    exclude_patterns: Vec<Pattern>,
    max_file_size_bytes: u64,
    /// Extension → tier overrides, inherited from `config.sync.scan_tier_overrides`.
    /// Empty map means "use the default tier for every extension".
    tier_overrides: std::collections::HashMap<String, String>,
}

fn load_folder_settings(folder_path: &str) -> FolderSettings {
    // Pull the matching WatchedFolder from config; fall back to sane defaults
    // when the folder isn't registered (e.g. a one-shot CLI scan).
    let config = Config::load().ok();
    let tier_overrides = config
        .as_ref()
        .map(|c| c.sync.scan_tier_overrides.clone())
        .unwrap_or_default();
    let folder: Option<WatchedFolder> = config.and_then(|c| {
        c.watched_folders
            .into_iter()
            .find(|f| f.path == folder_path)
    });

    match folder {
        Some(f) => FolderSettings {
            exclude_patterns: compile_patterns(&f.exclude_patterns),
            max_file_size_bytes: f.max_file_size_mb.saturating_mul(1024 * 1024),
            tier_overrides,
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
            tier_overrides,
        },
    }
}

fn compile_patterns(globs: &[String]) -> Vec<Pattern> {
    globs
        .iter()
        .filter_map(|g| Pattern::new(g).ok())
        .collect()
}

/// Hard cap on concurrent scanner workers. Each worker opens a DuckDB
/// in-memory database (schema + sample extraction) and, for docx/pptx,
/// can fork a MarkItDown Python sidecar.
///
/// Set to 1 after repeated crashes on macOS with the message
/// "Rust cannot catch foreign exceptions, aborting" during parallel
/// scans. Multiple rayon workers concurrently calling
/// `Connection::open_in_memory()` appear to trip a DuckDB internal
/// race that throws a C++ exception — once that exception unwinds
/// through a Rust frame, the runtime aborts the whole process.
/// Serial DuckDB access eliminates the race. The rayon/parallel
/// plumbing is kept in place so we can re-enable once we've isolated
/// a safe-to-parallelise call site.
const MAX_SCAN_WORKERS: usize = 1;

/// Global serialisation point for MarkItDown sidecar spawns. We saw the
/// crash specifically when many workers forked Python processes in
/// parallel — each one pulls in ~100 MB of interpreter + dependencies.
/// A single mutex means at most one sidecar runs at a time; the per-doc
/// wall-time cost is linear in file count but the process stays alive.
/// Tabular extraction and the cheaper anytomd fallback stay parallel.
static SIDECAR_GUARD: once_cell::sync::Lazy<std::sync::Mutex<()>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(()));

pub(crate) fn lock_sidecar() -> std::sync::MutexGuard<'static, ()> {
    // Poisoned lock is fine — the inner `()` has no invariants to
    // break. Recover and move on.
    SIDECAR_GUARD.lock().unwrap_or_else(|e| e.into_inner())
}

/// Extract metadata for a single file. Split out of the parallel loop
/// body so we can wrap the call site in `std::panic::catch_unwind` —
/// the closure syntax made that awkward inline. Returns `None` if the
/// file should be skipped from the final list (size cap, missing fs
/// metadata, or total failure).
fn scan_one(
    entry: &walkdir::DirEntry,
    folder_path: &str,
    settings: &FolderSettings,
    done: &std::sync::atomic::AtomicUsize,
    total: usize,
    progress: Option<&ProgressCb>,
    on_dataset: Option<&DatasetCb>,
) -> Option<DatasetMetadata> {
    use std::sync::atomic::Ordering;

    let path = entry.path();
    let file_metadata = fs::metadata(path).ok()?;

    // Enforce the file size cap — oversized files are logged and
    // skipped so one bad file doesn't take the whole scan down.
    if file_metadata.len() > settings.max_file_size_bytes {
        eprintln!(
            "[scanner] skipping {} ({} bytes, exceeds {} MB cap)",
            path.display(),
            file_metadata.len(),
            settings.max_file_size_bytes / (1024 * 1024)
        );
        return None;
    }

    // Cache fast path: if (mtime, size) match what we stored, reuse
    // the previously-extracted metadata and skip DuckDB entirely. Runs
    // through the process-wide `with_cache` so there's only ever one
    // DuckDB connection to scan_cache.db open at a time.
    let cache_key = crate::scan_cache::CacheKey::from_metadata(path, folder_path, &file_metadata);
    if let Some(key) = &cache_key {
        let hit = crate::scan_cache::with_cache(|c| {
            c.get(
                folder_path,
                &key.relative_path,
                key.mtime_secs,
                key.size_bytes,
            )
        })
        .flatten();
        if let Some(hit) = hit {
            let idx = done.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(cb) = progress {
                cb(idx, total, &path.to_string_lossy());
            }
            if let Some(cb) = on_dataset {
                cb(idx, total, &hit);
            }
            return Some(hit);
        }
    }

    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let tier = tier_for(ext, &settings.tier_overrides);

    // Trace log so if the process aborts mid-scan the last line tells us
    // exactly which file + tier was being processed. This is essential
    // for diagnosing foreign (C++ / Obj-C) exceptions that catch_unwind
    // can't intercept — we get a breadcrumb even though we can't recover.
    eprintln!(
        "[scanner] ▶ {:?} tier={:?} ext={}",
        path, tier, ext
    );
    let _ = std::io::Write::flush(&mut std::io::stderr());

    let metadata = match extract_metadata_at_tier(path, folder_path, tier) {
        Ok(m) => m,
        Err(e) => {
            // Full extraction failed — most commonly because DuckDB's
            // CSV sniffer rejected a file it couldn't parse, or an Excel
            // file was corrupted. We still want the user to see the
            // file exists in the folder detail view, just without
            // queryable schema. Build a minimal DatasetMetadata from
            // the filesystem-level info we DO have.
            eprintln!(
                "[scanner] schema extraction failed for {:?} — degrading to file-only entry: {}",
                path, e
            );
            extract_minimal_metadata(path, folder_path).ok()?
        }
    };

    eprintln!("[scanner] ✓ {:?}", path);

    // Persist freshly-extracted metadata so the next scan for this
    // file short-circuits. Goes through the shared singleton so we
    // never open a second DuckDB connection to scan_cache.db.
    if let Some(key) = &cache_key {
        let _ = crate::scan_cache::with_cache(|c| {
            c.put(
                folder_path,
                &key.relative_path,
                key.mtime_secs,
                key.size_bytes,
                &metadata,
            )
        });
    }

    let idx = done.fetch_add(1, Ordering::Relaxed) + 1;
    if let Some(cb) = progress {
        cb(idx, total, &path.to_string_lossy());
    }
    if let Some(cb) = on_dataset {
        cb(idx, total, &metadata);
    }
    Some(metadata)
}

fn scan_folder_blocking(
    folder_path: &str,
    settings: &FolderSettings,
    progress: Option<ProgressCb>,
    on_dataset: Option<DatasetCb>,
) -> Result<Vec<DatasetMetadata>> {
    use rayon::prelude::*;
    use std::sync::atomic::AtomicUsize;

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

    // No local ScanCache instance — we route every get/put through the
    // process-wide `scan_cache::with_cache` singleton so there's only
    // ever one DuckDB connection to scan_cache.db, regardless of how
    // many scans / cached-read calls run concurrently. Multiple opens
    // on the same file tripped DuckDB's internal locking and aborted
    // the process with a foreign C++ exception.
    eprintln!("[scanner] using shared scan cache");

    // Atomic so parallel workers agree on progress numbering without
    // stepping on each other. We report finishes (not starts) — with
    // rayon a "start" order isn't meaningful.
    let done = AtomicUsize::new(0);

    // Fan the per-file work out over a DEDICATED rayon pool capped at
    // MAX_SCAN_WORKERS. We avoid the global pool on purpose — on high-core
    // machines it defaults to num_cpus, which produced enough concurrent
    // DuckDB connections + sidecar forks to trip the macOS per-process
    // VM region cap and abort(). The callbacks already require Send + Sync
    // so they're safe to call from any worker thread; Tauri's event emit
    // is internally synchronised.
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(MAX_SCAN_WORKERS.min(num_cpus::get().max(1)))
        .thread_name(|i| format!("scanner-{}", i))
        .build()
        .map_err(|e| AgentError::FileSystem(format!("scanner pool init: {}", e)))?;

    let datasets: Vec<DatasetMetadata> = pool.install(|| {
        candidates
            .into_par_iter()
            .filter_map(|entry| {
                // Catch Rust panics from per-file extraction so one bad
                // file just gets logged and skipped instead of poisoning
                // the whole scan. Note: this does NOT catch foreign (C++/
                // Obj-C) exceptions — those still abort the process. For
                // those we rely on MAX_SCAN_WORKERS=1 (eliminates DuckDB
                // concurrency races) and the `extract_metadata_at_tier`
                // error path (which already routes DuckDB `Err` results
                // into `extract_minimal_metadata`).
                let path_for_log = entry.path().to_path_buf();
                let result = std::panic::catch_unwind(
                    std::panic::AssertUnwindSafe(|| {
                        scan_one(
                            &entry,
                            folder_path,
                            settings,
                            &done,
                            total,
                            progress.as_ref(),
                            on_dataset.as_ref(),
                        )
                    }),
                );
                match result {
                    Ok(metadata) => metadata,
                    Err(_payload) => {
                        eprintln!(
                            "[scanner] panic while scanning {:?} — skipping",
                            path_for_log
                        );
                        None
                    }
                }
            })
            .collect()
    });

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

/// Last-resort metadata builder for files whose schema extraction blew up
/// (malformed CSV, corrupted Excel, permission denied mid-read). Returns
/// just the file-system facts so the file still surfaces in the Folder
/// detail view — user can see it exists, decide whether to investigate.
///
/// Errors only when the filesystem itself can't tell us about the file.
fn extract_minimal_metadata(file_path: &Path, base_path: &str) -> Result<DatasetMetadata> {
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
        .unwrap_or("")
        .to_string();
    let last_modified = file_metadata
        .modified()
        .ok()
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    Ok(DatasetMetadata {
        relative_path,
        file_format: ext,
        size_bytes: file_metadata.len(),
        row_count_estimate: None,
        schema: vec![],
        last_modified,
        document_markdown: None,
        sample_rows: None,
        samples_redacted: false,
    })
}

/// Tier-aware dispatcher. Callers pass in the resolved tier (from config or
/// defaults); we route to the appropriate extraction path. A Shallow tier
/// returns filename-only metadata without ever opening the file, which is
/// the big win for folders full of HTML/IPYNB that would otherwise each
/// pay a sidecar spawn.
fn extract_metadata_at_tier(
    file_path: &Path,
    base_path: &str,
    tier: ScanTier,
) -> Result<DatasetMetadata> {
    match tier {
        ScanTier::Shallow => extract_minimal_metadata(file_path, base_path),
        ScanTier::Content | ScanTier::Full => extract_metadata(file_path, base_path, tier),
    }
}

fn extract_metadata(file_path: &Path, base_path: &str, tier: ScanTier) -> Result<DatasetMetadata> {
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
            sample_rows: None,
            samples_redacted: false,
        })
    } else {
        // Tabular files — DuckDB schema extraction + optional sampling.
        let (schema, row_count) = extract_schema(file_path, ext, &file_metadata)?;
        // Sample-row collection runs best-effort — a failure here MUST NOT
        // block the sync. At Content tier we skip it entirely: saves one
        // DuckDB query per file and most users never expand the preview
        // anyway. Full tier still collects samples so the cloud agent has
        // concrete values to ground LLM answers.
        let (sample_rows, samples_redacted) = if matches!(tier, ScanTier::Full) {
            extract_sample_rows(file_path, ext, &schema).unwrap_or((None, false))
        } else {
            (None, false)
        };
        Ok(DatasetMetadata {
            relative_path,
            file_format: ext.to_string(),
            size_bytes: file_metadata.len(),
            row_count_estimate: Some(row_count),
            schema,
            last_modified,
            document_markdown: None,
            sample_rows,
            samples_redacted,
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
///
/// Serialised globally via `SIDECAR_GUARD` so a parallel scan can't fork
/// a dozen Python processes at once — each one is ~100 MB and the fleet
/// of them is what tripped `mach_vm_allocate_kernel` in the scanner crash
/// reports. One-at-a-time is plenty given how few DOCX/PPTX files most
/// folders contain.
fn try_sidecar_conversion(file_path: &Path) -> Option<String> {
    let _guard = lock_sidecar();

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
    eprintln!("[extract_schema] open_in_memory {:?}", file_path);
    let conn = Connection::open_in_memory()
        .map_err(|e| AgentError::Database(format!("Failed to open DuckDB: {}", e)))?;

    // xlsx is transparently converted to cached CSV, then to Parquet.
    // csv is transparently converted to cached Parquet for 10-100x faster queries.
    // This keeps the read_func pipeline uniform downstream (always Parquet).
    let (effective_path, effective_ext): (Cow<Path>, &str) = match ext {
        "xlsx" | "xls" => {
            eprintln!("[extract_schema] xlsx→csv {:?}", file_path);
            let csv = excel::xlsx_to_csv(file_path)?;
            eprintln!("[extract_schema] csv→parquet {:?}", csv);
            let parquet = crate::csv::csv_to_parquet(&csv)?;
            (Cow::Owned(parquet), "parquet")
        },
        "csv" => {
            eprintln!("[extract_schema] csv→parquet {:?}", file_path);
            let parquet = crate::csv::csv_to_parquet(file_path)?;
            (Cow::Owned(parquet), "parquet")
        },
        _ => (Cow::Borrowed(file_path), ext)
    };
    let path_str = effective_path.to_string_lossy();
    eprintln!("[extract_schema] DESCRIBE {}", path_str);

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
// Sample-row collection (LLM grounding; PII-scrubbed)
// ---------------------------------------------------------------------------

/// Up to this many sample rows per file. Mirrors the server-side cap in
/// api/app/api/v1/agent_metadata.py `_MAX_SAMPLE_ROWS`.
const SAMPLE_ROW_LIMIT: usize = 5;

/// Crate-visible accessor for the sample-row cap. Remote sources
/// (`remote::scan_remote_blocking`) reuse this so local and URL-based
/// files cap at the same N.
pub(crate) fn sample_row_limit() -> usize {
    SAMPLE_ROW_LIMIT
}

/// Crate-visible wrapper so the remote scanner can reuse the same PII
/// redaction heuristic without duplicating the signal list.
pub(crate) fn is_pii_column_name(name: &str) -> bool {
    is_pii_column(name)
}

/// Column names whose values get replaced with `<redacted>` in samples.
/// Case-insensitive substring match — conservative by design.
const PII_COLUMN_SIGNALS: &[&str] = &[
    "email",
    "ssn",
    "credit",
    "card",
    "cvv",
    "phone",
    "tel",
    "password",
    "passwd",
    "token",
    "secret",
    "api_key",
    "apikey",
    "auth",
];

fn is_pii_column(col_name: &str) -> bool {
    let lower = col_name.to_ascii_lowercase();
    PII_COLUMN_SIGNALS.iter().any(|p| lower.contains(p))
}

#[cfg(test)]
mod pii_tests {
    use super::is_pii_column;

    #[test]
    fn matches_common_pii_columns() {
        assert!(is_pii_column("email"));
        assert!(is_pii_column("Email"));
        assert!(is_pii_column("user_email_address"));
        assert!(is_pii_column("phone_number"));
        assert!(is_pii_column("ssn"));
        assert!(is_pii_column("credit_card"));
        assert!(is_pii_column("cvv"));
        assert!(is_pii_column("password_hash"));
        assert!(is_pii_column("api_key"));
        assert!(is_pii_column("ApiKey"));
        assert!(is_pii_column("auth_token"));
    }

    #[test]
    fn leaves_benign_columns_alone() {
        assert!(!is_pii_column("date"));
        assert!(!is_pii_column("amount"));
        assert!(!is_pii_column("product_name"));
        assert!(!is_pii_column("order_id"));
        assert!(!is_pii_column("country"));
        assert!(!is_pii_column("status"));
    }
}

#[cfg(test)]
mod tier_tests {
    use super::{default_tier_for, tier_for, ScanTier};
    use std::collections::HashMap;

    #[test]
    fn tabular_formats_default_to_full() {
        for ext in ["parquet", "csv", "xlsx", "xls"] {
            assert_eq!(default_tier_for(ext), ScanTier::Full, "{ext} should be Full");
        }
    }

    #[test]
    fn docx_and_pptx_default_to_content() {
        for ext in ["docx", "pptx"] {
            assert_eq!(
                default_tier_for(ext),
                ScanTier::Content,
                "{ext} should be Content"
            );
        }
    }

    #[test]
    fn html_and_ipynb_default_to_shallow() {
        // The motivating case — these were the main perf culprit because
        // every one spawned the MarkItDown sidecar. Shallow by default.
        for ext in ["html", "htm", "ipynb"] {
            assert_eq!(
                default_tier_for(ext),
                ScanTier::Shallow,
                "{ext} should be Shallow"
            );
        }
    }

    #[test]
    fn unknown_extensions_default_to_shallow() {
        // Anything we haven't explicitly classified falls to Shallow so
        // the scanner never burns cycles on something it can't parse.
        assert_eq!(default_tier_for("xyz"), ScanTier::Shallow);
        assert_eq!(default_tier_for(""), ScanTier::Shallow);
    }

    #[test]
    fn override_promotes_html_to_content() {
        let mut overrides = HashMap::new();
        overrides.insert("html".to_string(), "content".to_string());
        assert_eq!(tier_for("html", &overrides), ScanTier::Content);
    }

    #[test]
    fn override_demotes_csv_to_shallow() {
        let mut overrides = HashMap::new();
        overrides.insert("csv".to_string(), "shallow".to_string());
        assert_eq!(tier_for("csv", &overrides), ScanTier::Shallow);
    }

    #[test]
    fn override_is_case_insensitive_on_value() {
        let mut overrides = HashMap::new();
        overrides.insert("html".to_string(), "CONTENT".to_string());
        assert_eq!(tier_for("html", &overrides), ScanTier::Content);
    }

    #[test]
    fn unknown_override_value_falls_through_to_default() {
        // A typo in the config ("fuull" instead of "full") must not wipe
        // out the sensible default — we silently fall through.
        let mut overrides = HashMap::new();
        overrides.insert("csv".to_string(), "fuull".to_string());
        assert_eq!(tier_for("csv", &overrides), ScanTier::Full);
    }
}

#[cfg(test)]
mod filter_tests {
    use super::{compile_patterns, is_document_ext, is_excluded, is_supported};
    use std::path::{Path, PathBuf};

    #[test]
    fn is_supported_covers_all_indexable_formats() {
        for ext in [
            "parquet", "csv", "xlsx", "xls", "docx", "pptx", "html", "htm", "ipynb",
        ] {
            let p = PathBuf::from(format!("/tmp/file.{ext}"));
            assert!(is_supported(&p), "{ext} should be supported");
        }
    }

    #[test]
    fn is_supported_rejects_nonindexable() {
        for bad in ["exe", "bin", "mp4", "zip", "tar", ""] {
            let p = PathBuf::from(format!("/tmp/file.{bad}"));
            assert!(!is_supported(&p), "{bad} should NOT be supported");
        }
        // No extension — must also be rejected.
        assert!(!is_supported(Path::new("/tmp/some_binary")));
    }

    #[test]
    fn is_document_ext_separates_docs_from_tabular() {
        assert!(is_document_ext("docx"));
        assert!(is_document_ext("pptx"));
        assert!(is_document_ext("html"));
        assert!(is_document_ext("ipynb"));

        assert!(!is_document_ext("parquet"));
        assert!(!is_document_ext("csv"));
        assert!(!is_document_ext("xlsx"));
    }

    #[test]
    fn compile_patterns_drops_invalid_globs_silently() {
        // One valid, one invalid. compile_patterns is best-effort: a bad
        // pattern in user settings must not break the scan.
        let patterns = compile_patterns(&["node_modules".into(), "[".into()]);
        assert_eq!(patterns.len(), 1);
        assert!(patterns[0].matches("node_modules"));
    }

    #[test]
    fn is_excluded_matches_directory_component() {
        // "node_modules" as a single pattern should exclude nested folders.
        let base = "/home/user/repo";
        let patterns = compile_patterns(&["node_modules".into()]);
        let nested = PathBuf::from("/home/user/repo/app/node_modules/foo.js");
        assert!(is_excluded(&nested, base, &patterns));
    }

    #[test]
    fn is_excluded_matches_filename_pattern() {
        let base = "/home/user/repo";
        let patterns = compile_patterns(&["*.log".into()]);
        assert!(is_excluded(
            &PathBuf::from("/home/user/repo/app.log"),
            base,
            &patterns,
        ));
        assert!(!is_excluded(
            &PathBuf::from("/home/user/repo/data.csv"),
            base,
            &patterns,
        ));
    }

    #[test]
    fn is_excluded_false_when_no_patterns() {
        let patterns = compile_patterns(&[]);
        assert!(!is_excluded(
            &PathBuf::from("/tmp/anything.csv"),
            "/tmp",
            &patterns,
        ));
    }
}

/// Best-effort sample-row extraction for tabular files.
///
/// Opens a fresh in-memory DuckDB, runs `SELECT * LIMIT 5`, converts the
/// rows to JSON values, and substitutes `<redacted>` for PII-looking
/// columns.
///
/// Returns:
///   - `Ok((Some(samples), redacted_flag))` on success
///   - `Ok((None, false))` if the file yields no rows (empty CSV etc.)
///   - `Err(...)` on database errors (caller should .unwrap_or fall back)
fn extract_sample_rows(
    file_path: &Path,
    ext: &str,
    schema: &[ColumnSchema],
) -> Result<(Option<Vec<serde_json::Map<String, serde_json::Value>>>, bool)> {
    if schema.is_empty() {
        return Ok((None, false));
    }

    let conn = Connection::open_in_memory()
        .map_err(|e| AgentError::Database(format!("Failed to open DuckDB: {}", e)))?;

    // Reuse the xlsx/csv → parquet conversion cache, same as extract_schema.
    let (effective_path, effective_ext): (Cow<Path>, &str) = match ext {
        "xlsx" | "xls" => {
            let csv = excel::xlsx_to_csv(file_path)?;
            let parquet = crate::csv::csv_to_parquet(&csv)?;
            (Cow::Owned(parquet), "parquet")
        }
        "csv" => {
            let parquet = crate::csv::csv_to_parquet(file_path)?;
            (Cow::Owned(parquet), "parquet")
        }
        _ => (Cow::Borrowed(file_path), ext),
    };
    let path_str = effective_path.to_string_lossy();

    let read_func = match effective_ext {
        "parquet" => "read_parquet",
        "csv" => "read_csv_auto",
        other => {
            return Err(AgentError::Database(format!(
                "extract_sample_rows: unsupported format {}",
                other
            )));
        }
    };

    let sample_sql = format!(
        "SELECT * FROM {}('{}') LIMIT {}",
        read_func, path_str, SAMPLE_ROW_LIMIT
    );

    let mut stmt = conn
        .prepare(&sample_sql)
        .map_err(|e| AgentError::Database(format!("Failed to prepare sample SQL: {}", e)))?;

    // Pre-compute which column indices need redaction.
    let redacted_indices: Vec<usize> = schema
        .iter()
        .enumerate()
        .filter_map(|(i, c)| if is_pii_column(&c.name) { Some(i) } else { None })
        .collect();

    let rows = stmt
        .query_map([], |row| {
            let mut obj = serde_json::Map::with_capacity(schema.len());
            for (i, col) in schema.iter().enumerate() {
                let value = if redacted_indices.contains(&i) {
                    serde_json::Value::String("<redacted>".to_string())
                } else {
                    duckdb_cell_to_json(row, i)
                };
                obj.insert(col.name.clone(), value);
            }
            Ok(obj)
        })
        .map_err(|e| AgentError::Database(format!("Failed to query samples: {}", e)))?;

    let mut samples: Vec<serde_json::Map<String, serde_json::Value>> = Vec::new();
    for row in rows {
        if let Ok(obj) = row {
            samples.push(obj);
        }
        if samples.len() >= SAMPLE_ROW_LIMIT {
            break;
        }
    }

    if samples.is_empty() {
        return Ok((None, false));
    }

    Ok((Some(samples), !redacted_indices.is_empty()))
}

/// Convert a single DuckDB cell at `idx` to a JSON value, trying a few
/// common types. Anything we can't map cleanly becomes a string via
/// Debug formatting so the LLM still gets *some* signal.
pub(crate) fn duckdb_cell_to_json(row: &duckdb::Row<'_>, idx: usize) -> serde_json::Value {
    // Try in order of most specific → most lenient. The duckdb-rs crate
    // returns an error when a type doesn't match, so we cascade.
    if let Ok(v) = row.get::<_, Option<i64>>(idx) {
        return v.map(serde_json::Value::from).unwrap_or(serde_json::Value::Null);
    }
    if let Ok(v) = row.get::<_, Option<f64>>(idx) {
        return v
            .and_then(|f| serde_json::Number::from_f64(f).map(serde_json::Value::Number))
            .unwrap_or(serde_json::Value::Null);
    }
    if let Ok(v) = row.get::<_, Option<bool>>(idx) {
        return v.map(serde_json::Value::Bool).unwrap_or(serde_json::Value::Null);
    }
    if let Ok(v) = row.get::<_, Option<String>>(idx) {
        return v.map(serde_json::Value::String).unwrap_or(serde_json::Value::Null);
    }
    serde_json::Value::Null
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

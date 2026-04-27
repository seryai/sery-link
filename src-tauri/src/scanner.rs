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
const DOCUMENT_EXTENSIONS: &[&str] = &["docx", "pptx", "html", "htm", "ipynb", "pdf"];

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
        "docx" | "pptx" | "pdf" => ScanTier::Content,
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

/// URL-based "folder" scan. Dispatches on URL shape:
///   * Single-object URL → one HEAD probe + one DuckDB DESCRIBE.
///   * S3 listing URL (prefix or glob) → enumerate via `glob()` and
///     fan out a DESCRIBE per matching object.
///
/// Each object ends up in the scan cache keyed on (folder_path,
/// relative_path, mtime, size) — same shape as local files. For
/// listings the `folder_path` is the user-facing bucket/prefix URL
/// and `relative_path` is the S3 key below it, so the cache and
/// FolderDetail / search paths all work unchanged.
async fn scan_remote_folder(
    url: &str,
    progress: Option<ProgressCb>,
    on_dataset: Option<DatasetCb>,
) -> Result<Vec<DatasetMetadata>> {
    if crate::url::is_s3_listing(url) {
        scan_s3_listing(url, progress, on_dataset).await
    } else {
        scan_remote_single(url, progress, on_dataset).await
    }
}

/// Single-URL remote scan — Phase A + B1 path.
async fn scan_remote_single(
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

/// S3 bucket listing scan — enumerates objects under a prefix or glob
/// pattern, then scans each one. Emits `dataset_scanned` events as
/// each object completes so FolderDetail streams rows in.
///
/// Creds are keyed on the LISTING URL (not each enumerated object),
/// so `scan_remote_blocking_with_creds` receives the listing URL as
/// the `creds_source`.
async fn scan_s3_listing(
    listing_url: &str,
    progress: Option<ProgressCb>,
    on_dataset: Option<DatasetCb>,
) -> Result<Vec<DatasetMetadata>> {
    // Step 1: enumerate objects (blocking — DuckDB is sync).
    let listing_owned = listing_url.to_string();
    let objects = tokio::task::spawn_blocking(move || {
        crate::remote::list_s3_blocking(&listing_owned)
    })
    .await
    .map_err(|e| AgentError::FileSystem(format!("S3 list task failed: {}", e)))??;

    let total = objects.len();
    if total == 0 {
        return Ok(Vec::new());
    }

    // Step 2: scan each object sequentially. Parallel fan-out via
    // rayon is tempting but DuckDB file-cache contention + S3 rate
    // limits make serial safer for B2. The dataset_scanned events
    // stream rows to the UI so users see progress even without
    // parallelism.
    let mut datasets: Vec<DatasetMetadata> = Vec::with_capacity(total);
    for (idx, obj) in objects.into_iter().enumerate() {
        if let Some(cb) = &progress {
            cb(idx + 1, total, &obj.url);
        }

        let obj_url = obj.url.clone();
        let listing_key = listing_url.to_string();
        let head = crate::remote::RemoteHeadInfo {
            last_modified_secs: obj.last_modified_secs,
            content_length: obj.size_bytes,
        };

        // Per-file scan. Cache key uses the LISTING URL as folder and
        // the object URL's filename portion as the relative path —
        // matches the local-folder convention where folder_path is
        // the root and relative_path is below it.
        let listing_for_cache = listing_key.clone();
        let result = tokio::task::spawn_blocking(move || {
            let relative = crate::url::infer_filename_from_url(&obj_url);

            // Cache fast path.
            let hit = crate::scan_cache::with_cache(|c| {
                c.get(
                    &listing_for_cache,
                    &relative,
                    head.last_modified_secs.unwrap_or(0),
                    head.content_length.unwrap_or(0),
                )
            })
            .flatten();
            if let Some(meta) = hit {
                return Ok::<_, AgentError>(meta);
            }

            let mut meta = crate::remote::scan_remote_blocking_with_creds(
                &obj_url,
                &head,
                &listing_key,
            )?;
            // Use the relative-key form so FolderDetail's row label
            // shows the object's key below the prefix rather than the
            // filename alone (e.g. `2024/sales.parquet`, not just
            // `sales.parquet`). Re-derive from the URL+listing pair.
            meta.relative_path =
                relative_key(&obj_url, &listing_for_cache).unwrap_or(relative);

            let _ = crate::scan_cache::with_cache(|c| {
                c.put(
                    &listing_for_cache,
                    &meta.relative_path,
                    head.last_modified_secs.unwrap_or(0),
                    head.content_length.unwrap_or(0),
                    &meta,
                )
            });
            Ok(meta)
        })
        .await
        .map_err(|e| {
            AgentError::FileSystem(format!("S3 object scan task failed: {}", e))
        })?;

        match result {
            Ok(meta) => {
                if let Some(cb) = &on_dataset {
                    cb(idx + 1, total, &meta);
                }
                datasets.push(meta);
            }
            Err(e) => {
                // One bad object shouldn't kill the whole listing —
                // most commonly it's an Access Denied on a specific
                // prefix. Log and continue.
                eprintln!("[scanner] scan failed for {}: {} — skipping", listing_url, e);
            }
        }
    }

    Ok(datasets)
}

/// Derive the S3 "relative path" of an object URL under its listing
/// URL. Strips scheme+bucket+common-prefix so FolderDetail shows the
/// part that varies between objects.
fn relative_key(object_url: &str, listing_url: &str) -> Option<String> {
    // Strip the leading glob pattern so `s3://bucket/prefix/*.parquet`
    // acts like `s3://bucket/prefix/` for comparison purposes.
    let base = if listing_url.contains('*') {
        match listing_url.rsplit_once('/') {
            Some((root, _)) => format!("{}/", root),
            None => listing_url.to_string(),
        }
    } else if listing_url.ends_with('/') {
        listing_url.to_string()
    } else {
        format!("{}/", listing_url)
    };
    object_url.strip_prefix(&base).map(|s| s.to_string())
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

/// Best-effort discovery of the directory carrying app-bundled
/// runtime libraries (e.g. `libpdfium.dylib`).
///
/// We avoid plumbing Tauri's `AppHandle` into this module because
/// `MDKIT_ENGINE` is a process-wide `Lazy` initialised on first use
/// from any thread, and the scanner is sometimes invoked from places
/// that don't have a handy `AppHandle` reference (rayon workers,
/// background indexers). Inferring from `current_exe` is brittle but
/// matches Tauri's bundle layout convention:
///
/// - **macOS production**: binary at `<App>.app/Contents/MacOS/<bin>`,
///   resources at `<App>.app/Contents/Resources/`.
/// - **Linux / Windows production**: binary at `<dir>/<bin>`,
///   resources at `<dir>/resources/` (Tauri's bundler copies
///   `tauri.conf.json` `bundle.resources` paths there).
/// - **Debug builds (cargo run / `pnpm tauri dev`)**: nothing is
///   bundled — the dev pseudo-resources live at
///   `<src-tauri>/resources/`. We reach them via
///   `CARGO_MANIFEST_DIR` (set at compile time), gated to
///   `debug_assertions` so production builds never trust an
///   embedded build-time path that won't exist on the user's
///   machine.
///
/// Returns `None` if `current_exe` fails or no convention matches —
/// callers should treat that as "no bundle, fall through to system
/// search."
fn bundled_resource_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let parent = exe.parent()?;

    if cfg!(target_os = "macos") {
        let resources = parent.parent()?.join("Resources");
        if resources.is_dir() {
            return Some(resources);
        }
    }

    let next_to_binary = parent.join("resources");
    if next_to_binary.is_dir() {
        return Some(next_to_binary);
    }

    // Debug-only dev fallback — the path embedded by `env!` is the
    // build host's `src-tauri/`, which only exists on developers'
    // machines. `cfg(debug_assertions)` keeps it out of release
    // binaries shipped to users.
    #[cfg(debug_assertions)]
    {
        let dev = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        if dev.is_dir() {
            return Some(dev);
        }
    }

    None
}

/// Process-wide mdkit engine. Replaces the v0.x MarkItDown Python
/// sidecar — see `extract_document_markdown` for the dispatch surface.
///
/// `mdkit` is in-process Rust; no fork-bomb risk, no ~100 MB Python
/// interpreter per worker, no global mutex needed. The previous
/// `SIDECAR_GUARD` mutex existed because parallel rayon workers were
/// each forking Python and tripping `mach_vm_allocate_kernel`; that
/// constraint is gone with mdkit and document extraction parallelises
/// naturally with the rest of the scanner.
///
/// **Bundle path:** when the Tauri-bundled `Resources/libpdfium/`
/// directory exists, we prefer it over system-wide library search.
/// This lets shipped builds work on consumer machines without
/// requiring the user to install libpdfium via Homebrew / apt /
/// downloading from `bblanchon/pdfium-binaries`. On dev machines
/// without the bundle, we fall through to
/// `Pdfium::bind_to_system_library()` which checks
/// `DYLD_LIBRARY_PATH` + `/usr/lib` + the system dyld cache.
///
/// `with_defaults_diagnostic` returns the backends that failed to
/// register (e.g. libpdfium not on the library path, pandoc not on
/// PATH); we log them so missing runtime deps are debuggable without
/// reading mdkit source.
static MDKIT_ENGINE: once_cell::sync::Lazy<mdkit::Engine> = once_cell::sync::Lazy::new(|| {
    // Start from `with_defaults_diagnostic` (system search for every
    // backend), then patch in bundled overrides for the backends
    // that need them. Cheaper than reconstructing the whole engine
    // by hand and keeps mdkit's default registration order intact.
    let (mut engine, errors) = mdkit::Engine::with_defaults_diagnostic();
    let pdf_failed = errors.iter().any(|(name, _)| *name == "pdf");
    let pandoc_failed = errors.iter().any(|(name, _)| *name == "pandoc");

    for (backend, err) in &errors {
        eprintln!(
            "[scanner] mdkit: backend `{backend}` failed system search: {err}"
        );
    }

    let bundled = bundled_resource_dir();

    // Bundled libpdfium override. Only attempted when the system
    // search didn't already find it, AND the resource dir + the
    // `libpdfium` subdirectory inside it both exist.
    if pdf_failed {
        if let Some(resource_dir) = bundled.as_ref() {
            let pdfium_dir = resource_dir.join("libpdfium");
            if pdfium_dir.is_dir() {
                let dir_str = pdfium_dir.to_string_lossy();
                match mdkit::pdf::PdfiumExtractor::with_library_path(&dir_str) {
                    Ok(ext) => {
                        engine.register(Box::new(ext));
                        eprintln!(
                            "[scanner] mdkit: backend `pdf` registered from bundled \
                             libpdfium at {dir_str}"
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "[scanner] mdkit: bundled libpdfium at {dir_str} failed to load: {e}"
                        );
                    }
                }
            }
        }
    }

    // Bundled pandoc override. Same shape as libpdfium: when system
    // PATH discovery fails AND the bundled `pandoc` binary exists,
    // construct a PandocExtractor with the explicit path. Closes the
    // "consumer-bound app, no Homebrew" gap; without it DOCX / PPTX
    // / EPUB / RTF / ODT / LaTeX silently fall through to the
    // anytomd safety net (lower fidelity than Pandoc).
    if pandoc_failed {
        if let Some(resource_dir) = bundled.as_ref() {
            let bin_name = if cfg!(target_os = "windows") {
                "pandoc.exe"
            } else {
                "pandoc"
            };
            let pandoc_bin = resource_dir.join("pandoc").join(bin_name);
            if pandoc_bin.is_file() {
                let bin_str = pandoc_bin.to_string_lossy();
                match mdkit::pandoc::PandocExtractor::with_binary(pandoc_bin.clone()) {
                    Ok(ext) => {
                        engine.register(Box::new(ext));
                        eprintln!(
                            "[scanner] mdkit: backend `pandoc` registered from bundled \
                             binary at {bin_str}"
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "[scanner] mdkit: bundled pandoc at {bin_str} failed to verify: {e}"
                        );
                    }
                }
            }
        }
    }

    if errors.is_empty() {
        eprintln!("[scanner] mdkit: all backends registered cleanly");
    }
    engine
});

/// Extract metadata for a single file. Split out of the parallel loop
/// body so we can wrap the call site in `std::panic::catch_unwind` —
/// the closure syntax made that awkward inline. Returns `None` if the
/// file should be skipped from the final list (size cap, missing fs
/// metadata, or total failure).
fn scan_one(
    path: &Path,
    folder_path: &str,
    settings: &FolderSettings,
    done: &std::sync::atomic::AtomicUsize,
    total: usize,
    progress: Option<&ProgressCb>,
    on_dataset: Option<&DatasetCb>,
) -> Option<DatasetMetadata> {
    use std::sync::atomic::Ordering;

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
    //
    // `scankit::Scanner` handles the walkdir + size-cap glue. The
    // sery-link-specific filters (is_supported by extension list,
    // is_excluded matching against full path / file name / path
    // components) layer on top — scankit's own globset excludes are
    // a narrower API than what FolderSettings.exclude_patterns can
    // express, so we keep the existing pattern logic post-walk for now.
    let scanner = scankit::Scanner::new(
        scankit::ScanConfig::default()
            .max_file_size_bytes(settings.max_file_size_bytes)
            .follow_symlinks(false),
    )
    .map_err(|e| AgentError::FileSystem(format!("scankit init: {e}")))?;
    let candidates: Vec<std::path::PathBuf> = scanner
        .walk(folder_path)
        // The local `Result` alias is `Result<T, AgentError>`, not
        // std::result::Result, so the bare `.filter_map(Result::ok)`
        // shorthand doesn't compile here. Closure form makes the
        // method-resolution unambiguous.
        .filter_map(|r| r.ok())
        .filter(|e| is_supported(&e.path))
        .filter(|e| !is_excluded(&e.path, folder_path, &settings.exclude_patterns))
        .map(|e| e.path)
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
            .filter_map(|entry: std::path::PathBuf| {
                // Catch Rust panics from per-file extraction so one bad
                // file just gets logged and skipped instead of poisoning
                // the whole scan. Note: this does NOT catch foreign (C++/
                // Obj-C) exceptions — those still abort the process. For
                // those we rely on MAX_SCAN_WORKERS=1 (eliminates DuckDB
                // concurrency races) and the `extract_metadata_at_tier`
                // error path (which already routes DuckDB `Err` results
                // into `extract_minimal_metadata`).
                let path_for_log = entry.clone();
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
        | "docx" | "pptx" | "html" | "htm" | "ipynb" | "pdf"
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

/// Convert a document file to markdown using the in-process `mdkit`
/// engine. Falls back to `anytomd` (also in-process Rust) if mdkit
/// returns no markdown — the fallback exists because mdkit's
/// pandoc-backed extractors return `MissingDependency` when the
/// `pandoc` binary isn't on PATH, and we'd rather hand the user a
/// degraded extraction than nothing at all.
///
/// Returns `Some(markdown)` on success, `None` on error (logged and
/// skipped — the file still gets indexed by name + size, just without
/// extracted content).
fn extract_document_markdown(file_path: &Path, ext: &str) -> Option<String> {
    // 50 MB cap. Anything bigger is almost certainly an LLM-spam
    // artefact (a 200 MB DOCX is rarely useful for grounding) and
    // pdfium / pandoc / Apple Vision all start to trip on memory
    // limits past the 50 MB mark anyway.
    let bytes = match fs::read(file_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[scanner] failed to read document {file_path:?}: {e}");
            return None;
        }
    };
    if bytes.len() > 50 * 1024 * 1024 {
        eprintln!("[scanner] document {file_path:?} exceeds 50 MB, skipping conversion");
        return None;
    }

    // Primary: mdkit. In-process, no fork, no mutex, parallel-safe.
    match MDKIT_ENGINE.extract(file_path) {
        Ok(doc) if !doc.markdown.trim().is_empty() => {
            eprintln!("[scanner] ✅ mdkit converted {file_path:?}");
            return Some(doc.markdown);
        }
        Ok(_) => {
            eprintln!(
                "[scanner] ⚠️ mdkit returned empty markdown for {file_path:?}, trying anytomd fallback"
            );
        }
        Err(e) => {
            eprintln!(
                "[scanner] ⚠️ mdkit failed for {file_path:?} ({e}), trying anytomd fallback"
            );
        }
    }

    // Fallback: anytomd. Lower quality (no Pandoc-class DOCX
    // fidelity, no OCR) but pure-Rust with zero runtime deps, so
    // it's the right safety net when mdkit's Pandoc / libpdfium
    // dependencies aren't available on the user's system.
    match anytomd::convert_bytes(&bytes, ext, &anytomd::ConversionOptions::default()) {
        Ok(result) => {
            eprintln!("[scanner] ✅ anytomd converted {file_path:?}");
            Some(result.markdown)
        }
        Err(e) => {
            eprintln!("[scanner] ❌ Both mdkit and anytomd failed for {file_path:?}: {e}");
            None
        }
    }
}

fn extract_schema(
    file_path: &Path,
    ext: &str,
    _file_metadata: &fs::Metadata,
) -> Result<(Vec<ColumnSchema>, i64)> {
    // Fast path: tabkit handles XLSX / XLS / XLSB / XLSM / ODS /
    // CSV / TSV / Parquet in one in-process pass — no DuckDB
    // connection, no XLSX → CSV → Parquet conversion. The DuckDB
    // pipeline below stays as the fallback for any tabular format
    // tabkit doesn't claim (currently empty — the matchset above
    // is exhaustive — but the structure stays for forward-compat).
    if let Some((columns, row_count, _samples)) = tabkit_extract(file_path, ext)? {
        eprintln!(
            "[extract_schema] tabkit handled {} ({} columns, {} rows)",
            file_path.display(),
            columns.len(),
            row_count,
        );
        return Ok((columns, row_count));
    }

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
// tabkit-backed extraction (schema + samples in one in-process pass)
// ---------------------------------------------------------------------------

/// Process-wide tabkit engine. Cheap to construct, `Send + Sync`,
/// shared across all parallel scanners. Default features cover
/// XLSX/XLS/XLSB/XLSM/ODS + CSV/TSV; the `parquet` feature is
/// enabled in Cargo.toml so we also handle .parquet without
/// going through DuckDB.
static TABKIT_ENGINE: once_cell::sync::Lazy<tabkit::Engine> =
    once_cell::sync::Lazy::new(tabkit::Engine::with_defaults);

/// Best-effort schema + samples + row count extraction via tabkit.
/// Returns `Ok(Some(_))` when tabkit handled the format; `Ok(None)`
/// when the format isn't tabular and the caller should fall through
/// to a different path; `Err(_)` when tabkit recognised the format
/// but extraction failed (corrupt file, etc.) and the caller should
/// surface the error.
///
/// Replaces the v0.x DuckDB pipeline for `extract_schema` +
/// `extract_sample_rows` for tabular files. Wins over the old
/// pipeline:
///
/// - One file read instead of three (DESCRIBE + COUNT + sample SELECT,
///   each potentially through XLSX → CSV → Parquet conversion).
/// - No DuckDB connection per file.
/// - No parquet cache writes per scan.
/// - Type inference uniform across XLSX / CSV / Parquet via the
///   tabkit `infer_column_type` rules.
///
/// The DuckDB pipeline stays for the agent's separate query path
/// (`agent_metadata` API + executor), where SQL queryability is
/// the actual requirement.
fn tabkit_extract(
    file_path: &Path,
    ext: &str,
) -> Result<
    Option<(
        Vec<ColumnSchema>,
        i64,
        Vec<serde_json::Map<String, serde_json::Value>>,
    )>,
> {
    // Match tabkit's covered extensions exactly. Anything else
    // returns `Ok(None)` so the caller falls through.
    if !matches!(
        ext,
        "xlsx" | "xls" | "xlsb" | "xlsm" | "ods" | "csv" | "tsv" | "parquet"
    ) {
        return Ok(None);
    }

    let options = tabkit::ReadOptions::default().max_sample_rows(SAMPLE_ROW_LIMIT);
    let table = match TABKIT_ENGINE.read(file_path, &options) {
        Ok(t) => t,
        Err(e) => {
            return Err(AgentError::FileSystem(format!(
                "tabkit failed to read {}: {e}",
                file_path.display()
            )));
        }
    };

    let columns: Vec<ColumnSchema> = table
        .columns
        .iter()
        .map(|c| ColumnSchema {
            name: c.name.clone(),
            col_type: tabkit_type_to_duckdb_string(c.data_type),
            nullable: c.nullable,
        })
        .collect();

    let row_count: i64 = table
        .row_count
        .and_then(|n| i64::try_from(n).ok())
        .unwrap_or(0);

    // Convert tabkit sample rows → serde_json::Map with PII
    // redaction. Re-uses the same `is_pii_column` heuristic the old
    // DuckDB-backed `extract_sample_rows` used.
    let redacted_indices: Vec<usize> = columns
        .iter()
        .enumerate()
        .filter_map(|(i, c)| if is_pii_column(&c.name) { Some(i) } else { None })
        .collect();

    let samples: Vec<serde_json::Map<String, serde_json::Value>> = table
        .sample_rows
        .iter()
        .map(|row| {
            let mut obj = serde_json::Map::with_capacity(columns.len());
            for (i, col) in columns.iter().enumerate() {
                let value = if redacted_indices.contains(&i) {
                    serde_json::Value::String("<redacted>".to_string())
                } else {
                    tabkit_value_to_json(row.get(i).unwrap_or(&tabkit::Value::Null))
                };
                obj.insert(col.name.clone(), value);
            }
            obj
        })
        .collect();

    Ok(Some((columns, row_count, samples)))
}

/// Map tabkit's coarse `DataType` enum to DuckDB-style type
/// strings. `ColumnSchema.col_type` is consumed downstream by the
/// agent (which uses DuckDB type names for SQL generation) and the
/// UI (which renders typed badges); keeping the strings
/// DuckDB-shaped means callers don't need to learn a new type
/// vocabulary just because the producer changed.
fn tabkit_type_to_duckdb_string(t: tabkit::DataType) -> String {
    match t {
        tabkit::DataType::Bool => "BOOLEAN",
        tabkit::DataType::Integer => "BIGINT",
        tabkit::DataType::Float => "DOUBLE",
        tabkit::DataType::Date => "DATE",
        tabkit::DataType::DateTime => "TIMESTAMP",
        // Text + Unknown both map to VARCHAR — DuckDB's natural
        // "we don't know more than 'string'" type. The downstream
        // agent treats VARCHAR as opaque, which matches what we
        // want for these cases.
        tabkit::DataType::Text | tabkit::DataType::Unknown => "VARCHAR",
        // Wildcard for forward-compat — tabkit's DataType is
        // #[non_exhaustive].
        _ => "VARCHAR",
    }
    .to_string()
}

/// tabkit `Value` → `serde_json::Value`. Date / DateTime payloads
/// stay as strings (matches the JSON-IPC contract tabkit advertises).
fn tabkit_value_to_json(v: &tabkit::Value) -> serde_json::Value {
    match v {
        tabkit::Value::Null => serde_json::Value::Null,
        tabkit::Value::Bool(b) => serde_json::Value::Bool(*b),
        tabkit::Value::Integer(i) => serde_json::Value::Number((*i).into()),
        tabkit::Value::Float(f) => serde_json::Number::from_f64(*f)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        tabkit::Value::Date(s) | tabkit::Value::DateTime(s) | tabkit::Value::Text(s) => {
            serde_json::Value::String(s.clone())
        }
        // Wildcard for forward-compat — tabkit's Value is
        // #[non_exhaustive].
        _ => serde_json::Value::Null,
    }
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
mod relative_key_tests {
    use super::relative_key;

    #[test]
    fn strips_bucket_prefix_from_object_url() {
        assert_eq!(
            relative_key(
                "s3://my-bucket/sales/2024/jan.parquet",
                "s3://my-bucket/sales/",
            ),
            Some("2024/jan.parquet".to_string())
        );
    }

    #[test]
    fn adds_trailing_slash_when_listing_url_lacks_one() {
        // User pasted `s3://bucket/prefix` (no trailing /) — we still
        // want the derived relative path to drop the prefix correctly.
        assert_eq!(
            relative_key("s3://my-bucket/sales/jan.parquet", "s3://my-bucket/sales"),
            Some("jan.parquet".to_string())
        );
    }

    #[test]
    fn strips_glob_tail_from_listing_url() {
        // For `s3://bucket/path/*.parquet`, everything up to the last
        // `/` is the effective folder; the relative path is the object
        // name below that root.
        assert_eq!(
            relative_key(
                "s3://my-bucket/sales/jan.parquet",
                "s3://my-bucket/sales/*.parquet",
            ),
            Some("jan.parquet".to_string())
        );
    }

    #[test]
    fn returns_none_when_object_url_is_outside_listing() {
        // Defensive — if DuckDB's glob ever returns something that
        // isn't actually under the listing prefix, we'd rather get
        // None than misattribute it.
        assert_eq!(
            relative_key("s3://other-bucket/file.parquet", "s3://my-bucket/"),
            None
        );
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
            "parquet", "csv", "xlsx", "xls", "docx", "pptx", "html", "htm", "ipynb", "pdf",
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
        assert!(is_document_ext("pdf"));

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

    // Fast path: tabkit. Same in-process read used by
    // `extract_schema`'s fast path; here we discard the schema +
    // row count it returns and keep the sample rows. The
    // PII-redaction logic lives inside `tabkit_extract` so the
    // produced rows already have the right cells substituted.
    //
    // Return-shape contract preserved from the v0.x DuckDB path:
    //
    // - `(None, false)` when no samples were produced (empty
    //   sheet, etc.).
    // - `(Some(samples), redacted)` otherwise, where `redacted`
    //   is true iff ANY column matched the PII heuristic. Re-
    //   compute from `schema` rather than threading the flag out
    //   of `tabkit_extract` to keep that helper's signature lean.
    if let Some((_cols, _row_count, samples)) = tabkit_extract(file_path, ext)? {
        if samples.is_empty() {
            return Ok((None, false));
        }
        let redacted = schema.iter().any(|c| is_pii_column(&c.name));
        return Ok((Some(samples), redacted));
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
    mut datasets: Vec<DatasetMetadata>,
) -> Result<serde_json::Value> {
    // ROADMAP F2 — opt-in for uploading extracted document text. The
    // toggle defaults to OFF, which means we strip `document_markdown`
    // from every dataset before serializing the sync payload. The cloud
    // never sees document content under default settings, matching
    // ROADMAP F2's "never includes file contents" promise.
    //
    // Per DECISIONS.md 2026-04-25 (F2 Option 3 resolution): users who
    // want cross-machine document search opt in via Settings → Sync →
    // "Include document text in workspace catalog". When that toggle
    // is true the markdown rides along; when false (the default) it
    // gets nulled here even though the scanner extracted it locally
    // (the local cache + per-machine document search still work, only
    // the cloud upload is suppressed).
    let include_document_text = Config::load()
        .map(|c| c.sync.include_document_text)
        .unwrap_or(false);
    if !include_document_text {
        for d in datasets.iter_mut() {
            d.document_markdown = None;
        }
    }

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

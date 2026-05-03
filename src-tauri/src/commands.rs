//! Tauri commands — the RPC surface exposed to the frontend via `invoke()`.
//!
//! Conventions:
//!   * All commands return `Result<T, String>` so errors serialize cleanly to
//!     JavaScript. Rich `AgentError` variants are flattened with `.to_string()`.
//!   * Long-running work (scans, zip exports, HTTP calls) is kept off the UI
//!     thread via `tokio::task::spawn_blocking` or `async` + reqwest.
//!   * Commands that mutate config always go through `Config::load → mutate →
//!     save` so every change is durable.

use crate::audit;
use crate::auth::{self, AgentToken};
use crate::config::{AuthMode, Config};
use crate::events;
use crate::history::{self, QueryHistoryEntry};
use crate::keyring_store;
use crate::metadata_cache::{CachedDataset, SearchResult, CacheStats, MetadataCache};
use crate::machines::{self, MachinesResponse};
use crate::scanner::{self, DatasetMetadata};
use crate::stats::{self, Stats};
use crate::watcher::{self, WatcherHandle};
use crate::websocket::WebSocketClient;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::RwLock;

// ---------------------------------------------------------------------------
// Global handles
// ---------------------------------------------------------------------------

static WS_CLIENT: once_cell::sync::Lazy<Arc<RwLock<Option<WebSocketClient>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(None)));

static WATCHER: once_cell::sync::Lazy<Arc<RwLock<Option<WatcherHandle>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(None)));

/// Process-wide "cloud sync is currently unreachable" flag. Once the first
/// sync attempt of a session fails (network error, 500, 401, etc.) we
/// stop attempting further syncs for the rest of the process lifetime —
/// every subsequent scan would otherwise repeat the same failing POST
/// and pollute logs + the sync_failed UI toast. Cleared on app restart,
/// or explicitly by `clear_cloud_offline` (exposed for future
/// "retry now" UI actions).
static CLOUD_OFFLINE: once_cell::sync::Lazy<std::sync::atomic::AtomicBool> =
    once_cell::sync::Lazy::new(|| std::sync::atomic::AtomicBool::new(false));

fn cloud_offline() -> bool {
    CLOUD_OFFLINE.load(std::sync::atomic::Ordering::Relaxed)
}

fn mark_cloud_offline() {
    CLOUD_OFFLINE.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Returns true only when the user has explicitly set up a workspace
/// connection AND a token is present in the keyring AND we haven't
/// already seen the cloud misbehave this session. LocalOnly / BYOK
/// users (and legacy users with a stale bootstrap token they never
/// re-authenticated) never attempt metadata sync.
pub(crate) fn cloud_sync_enabled() -> bool {
    if cloud_offline() {
        return false;
    }
    let cfg = match Config::load() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let explicit_workspace =
        matches!(cfg.app.selected_auth_mode, Some(AuthMode::WorkspaceKey { .. }));
    explicit_workspace && keyring_store::has_token()
}

// ---------------------------------------------------------------------------
// Auth + config
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn start_auth_flow(agent_name: String, platform: String) -> Result<AgentToken, String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    auth::start_oauth_flow(agent_name, platform, config.cloud.api_url)
        .await
        .map_err(|e| e.to_string())
}

/// Persist workspace_id + agent_id to disk so offline-capable paths
/// (scanner cache, schema-diff) don't have to round-trip /v1/agent/info
/// on every call. Non-fatal on save failure — token persistence already
/// happened in the keyring, so auth is still complete.
fn persist_identity(workspace_id: &str, agent_id: &str) {
    if let Ok(mut config) = Config::load() {
        config.agent.workspace_id = Some(workspace_id.to_string());
        config.agent.agent_id = Some(agent_id.to_string());
        let _ = config.save();
    }
}

#[tauri::command]
pub async fn bootstrap_workspace(display_name: String) -> Result<AgentToken, String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    let token = auth::bootstrap_workspace(display_name, config.cloud.api_url)
        .await
        .map_err(|e| e.to_string())?;
    persist_identity(&token.workspace_id, &token.agent_id);
    Ok(token)
}

#[tauri::command]
pub async fn auth_with_key(key: String, display_name: String) -> Result<AgentToken, String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    let token = auth::auth_with_workspace_key(key, display_name, config.cloud.api_url)
        .await
        .map_err(|e| e.to_string())?;
    persist_identity(&token.workspace_id, &token.agent_id);
    Ok(token)
}

// ---------------------------------------------------------------------------
// Machines view — list every device connected to this workspace
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_machines() -> Result<MachinesResponse, String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    machines::list_machines(&config.cloud.api_url)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_config() -> Result<Config, String> {
    Config::load().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_config(config: Config) -> Result<(), String> {
    config.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_watched_folder(path: String) -> Result<(), String> {
    let mut config = Config::load().map_err(|e| e.to_string())?;
    config.add_watched_folder(path, true);
    config.save().map_err(|e| e.to_string())?;
    let _ = restart_file_watcher().await;
    Ok(())
}

/// Register a remote data source. Accepts `http(s)://` (Phase A) or
/// `s3://` (Phase B) URLs. The URL is stored in `watched_folders`
/// alongside local folder paths; `is_remote_url()` / `is_s3_url()`
/// discriminate downstream.
///
/// For s3:// URLs the caller must also pass `credentials` — we
/// validate + persist them to the macOS Keychain via
/// `remote_creds::save` before the URL is added, so the initial scan
/// has something to sign with.
///
/// The initial scan happens on the frontend via `rescan_folder` after
/// this returns, so users see progress/errors in the same UI that
/// handles local folders.
#[tauri::command]
pub async fn add_remote_source(
    url: String,
    credentials: Option<crate::remote_creds::S3Credentials>,
) -> Result<String, String> {
    let validation = crate::url::validate_url(&url);
    let normalised = match validation {
        crate::url::UrlValidation::Ok { normalised, .. } => normalised,
        crate::url::UrlValidation::Invalid { reason } => return Err(reason),
    };

    // S3 URLs require credentials. Gate before touching config so a
    // bad-creds input doesn't leave an orphan entry behind.
    if crate::url::is_s3_url(&normalised) {
        let creds = credentials.ok_or_else(|| {
            "S3 sources need credentials — provide AWS access key, secret, and region"
                .to_string()
        })?;
        crate::remote_creds::save(&normalised, &creds).map_err(|e| e.to_string())?;
    } else if credentials.is_some() {
        // Public HTTP(S) URLs don't take credentials in Phase B; if the
        // UI ever sends some, clearly flag rather than silently drop.
        return Err(
            "Credentials are only used for s3:// URLs in this build.".to_string(),
        );
    }

    let mut config = Config::load().map_err(|e| e.to_string())?;
    config.add_watched_folder(normalised.clone(), false);
    config.save().map_err(|e| e.to_string())?;
    // No file watcher restart — URLs aren't on the filesystem.
    Ok(normalised)
}

// ─── Google Drive OAuth (Phase 3b) ────────────────────────────────
//
// `start_gdrive_oauth` is the entry point the frontend calls when the
// user clicks "Connect Google Drive". Returns immediately after
// opening the browser; the actual OAuth completion fires the
// `gdrive-oauth-complete` Tauri event from deep_link.rs once Google
// redirects back through `seryai://oauth/gdrive/callback`.
//
// `gdrive_status` is a synchronous lookup of whether tokens are
// already stored — used by the frontend to render "Connect" vs
// "Connected as <email>" state.

#[tauri::command]
pub async fn start_gdrive_oauth(app: tauri::AppHandle) -> Result<(), String> {
    let auth_url = crate::gdrive_oauth::start_flow(app)
        .await
        .map_err(|e| e.to_string())?;
    // tauri-plugin-opener is used elsewhere via the `open` crate;
    // matching the pattern keeps platform handling consistent.
    open::that(&auth_url).map_err(|e| format!("could not open browser: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn gdrive_status() -> Result<bool, String> {
    // True if we have stored tokens for the default account, false
    // otherwise. Doesn't validate token freshness — the scan/query
    // path will refresh as needed.
    match crate::gdrive_creds::load("default").map_err(|e| e.to_string())? {
        Some(_) => Ok(true),
        None => Ok(false),
    }
}

#[tauri::command]
pub async fn disconnect_gdrive() -> Result<(), String> {
    let account = "default";
    crate::gdrive_creds::delete(account).map_err(|e| e.to_string())?;
    // Wipe the local cache so "I disconnected Drive" matches the
    // user's mental model — no Drive bytes left on disk after.
    crate::gdrive_cache::forget_account(account).map_err(|e| e.to_string())?;

    let mut config = Config::load().map_err(|e| e.to_string())?;
    config
        .gdrive_watched_folders
        .retain(|f| f.account_id != account);
    if let Ok(cache_dir) = crate::gdrive_cache::account_dir(account) {
        config.remove_watched_folder(&cache_dir.to_string_lossy());
    }
    config.save().map_err(|e| e.to_string())?;
    Ok(())
}

/// List the user's top-level Drive folders. Used by the Phase 3c-2
/// folder picker UI. Auto-refreshes the access token if it's near
/// expiry — caller doesn't need to think about token lifecycle.
#[tauri::command]
pub async fn gdrive_list_root_folders(
) -> Result<Vec<crate::gdrive_api::DriveFile>, String> {
    crate::gdrive_api::list_root_folders("default")
        .await
        .map_err(|e| e.to_string())
}

/// List the children of a chosen Drive folder. `include_folders=true`
/// returns subfolders (so the picker can drill down); the scan
/// walker (Phase 3c-3) will set false to get just data files.
#[tauri::command]
pub async fn gdrive_list_folder(
    folder_id: String,
    include_folders: bool,
) -> Result<Vec<crate::gdrive_api::DriveFile>, String> {
    crate::gdrive_api::list_folder("default", &folder_id, include_folders)
        .await
        .map_err(|e| e.to_string())
}

/// Watch a Drive folder: walk it, download every eligible file into
/// the local cache, register the folder in config, and trigger a
/// rescan so the indexer picks up the cached files. Emits
/// `gdrive-watch-progress` events with `phase` ∈ {walking,
/// downloading, scanning, done} so the UI can render a progress bar.
/// Process-wide single-flight guard for Drive watches. Concurrent
/// invocations (auto-watch on connect + the user manually clicking
/// Watch on something else, or the hourly refresh racing with a
/// fresh OAuth) would otherwise stack downloads in memory and load
/// multiple Drive walks against the same OAuth tokens. Serialising
/// at the command boundary is the simplest fix: the second caller
/// blocks on the mutex until the first finishes, then proceeds.
fn gdrive_watch_lock() -> &'static tokio::sync::Mutex<()> {
    use std::sync::OnceLock;
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[tauri::command]
pub async fn gdrive_watch_folder<R: Runtime>(
    app: AppHandle<R>,
    folder_id: String,
    folder_name: String,
) -> Result<Value, String> {
    use tauri::Emitter;
    let account = "default".to_string();

    // Hold the watch mutex for the entire flow. Doesn't block other
    // commands (just other watches), and the wait shows up to the
    // user as a delayed "walking" event — acceptable; better than
    // OOM-ing the process from two parallel walks.
    let _guard = gdrive_watch_lock().lock().await;

    // Pre-flight free-disk check. Refuse to start a watch when the
    // user is already low on disk — better than discovering it 30
    // minutes into a download. The check looks at the volume
    // holding the Sery data dir (~/.seryai), since that's where
    // gdrive-cache lives.
    if let Ok(data_dir) = Config::data_dir() {
        match crate::disk_space::available_bytes(&data_dir) {
            Ok(free) if free < crate::disk_space::MIN_FREE_BYTES_FOR_WATCH => {
                let needed_gb =
                    crate::disk_space::MIN_FREE_BYTES_FOR_WATCH / (1024 * 1024 * 1024);
                let free_mb = free / (1024 * 1024);
                return Err(format!(
                    "Free disk too low to safely start a Drive watch. \
                     Need at least {} GB free; only {} MB available on \
                     the Sery data volume. Free up some space and try \
                     again.",
                    needed_gb, free_mb,
                ));
            }
            Ok(_) => {}
            Err(e) => {
                // Don't block the watch on a failed query — log and
                // proceed. Better to attempt the watch than to lock
                // the user out due to a syscall hiccup.
                eprintln!("[gdrive-watch] free-space query failed: {}", e);
            }
        }
    }

    let _ = app.emit(
        "gdrive-watch-progress",
        serde_json::json!({"folder_id": folder_id, "phase": "walking"}),
    );

    let walked = crate::gdrive_walker::walk_folder(&account, &folder_id)
        .await
        .map_err(|e| e.to_string())?;

    // Audit the walker's skip decisions before we touch the
    // network. The user can later inspect these in Settings →
    // Storage. Native + unsupported are catalogued here; too-large
    // gets logged inside the download loop because Drive's
    // reported size lies for native exports.
    let now_iso = chrono::Utc::now().to_rfc3339();
    for f in &walked.skipped_native {
        crate::gdrive_skipped::record(&crate::gdrive_skipped::SkippedEntry {
            account_id: account.clone(),
            watch_folder_id: folder_id.clone(),
            file_id: f.id.clone(),
            name: f.name.clone(),
            mime_type: f.mime_type.clone(),
            size_bytes: f.size,
            reason: crate::gdrive_skipped::SkipReason::NativeUnexportable,
            skipped_at: now_iso.clone(),
            detail: None,
        });
    }
    for f in &walked.skipped_unsupported {
        crate::gdrive_skipped::record(&crate::gdrive_skipped::SkippedEntry {
            account_id: account.clone(),
            watch_folder_id: folder_id.clone(),
            file_id: f.id.clone(),
            name: f.name.clone(),
            mime_type: f.mime_type.clone(),
            size_bytes: f.size,
            reason: crate::gdrive_skipped::SkipReason::UnsupportedExtension,
            skipped_at: now_iso.clone(),
            detail: None,
        });
    }

    let total = walked.files.len();
    let mut file_ids: Vec<String> = Vec::with_capacity(total);
    let mut skipped_too_large = 0usize;
    // Captured before the move into GdriveWatchedFolder below; we
    // use it for the user-facing "indexed N files" toast and the
    // Tauri command return value.
    let cached_count: usize;

    for (i, file) in walked.files.iter().enumerate() {
        let _ = app.emit(
            "gdrive-watch-progress",
            serde_json::json!({
                "folder_id": folder_id,
                "phase": "downloading",
                "current": i + 1,
                "total": total,
                "file_name": file.name,
            }),
        );
        // Per-file isolation: an oversized file or transient network
        // hiccup on one item must not abort the whole watch. Log,
        // count, and continue. The user sees the skipped count on
        // the `done` event.
        match crate::gdrive_cache::download_if_stale(&account, file).await {
            Ok(_) => file_ids.push(file.id.clone()),
            Err(e) => {
                let detail = e.to_string();
                eprintln!("[gdrive-watch] skipped {:?}: {}", file.name, detail);
                // Heuristic: differentiate too-large from generic
                // failures so the Storage page can surface the
                // right reason. The cache layer's error message
                // for the size cap mentions "1 GiB cap" / "byte cap".
                let reason = if detail.contains("cap") {
                    crate::gdrive_skipped::SkipReason::TooLarge
                } else {
                    crate::gdrive_skipped::SkipReason::DownloadFailed
                };
                crate::gdrive_skipped::record(&crate::gdrive_skipped::SkippedEntry {
                    account_id: account.clone(),
                    watch_folder_id: folder_id.clone(),
                    file_id: file.id.clone(),
                    name: file.name.clone(),
                    mime_type: file.mime_type.clone(),
                    size_bytes: file.size,
                    reason,
                    skipped_at: now_iso.clone(),
                    detail: Some(detail),
                });
                skipped_too_large += 1;
            }
        }
    }

    // Persist the watched-folder entry, plus add the cache root to
    // watched_folders so the existing scanner picks up the files.
    // Idempotent: re-watching the same folder updates its file_ids.
    let cache_dir = crate::gdrive_cache::account_dir(&account)
        .map_err(|e| e.to_string())?;
    let cache_path_str = cache_dir.to_string_lossy().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    cached_count = file_ids.len();
    {
        let mut config = Config::load().map_err(|e| e.to_string())?;
        config.add_gdrive_watched_folder(crate::config::GdriveWatchedFolder {
            account_id: account.clone(),
            folder_id: folder_id.clone(),
            name: folder_name.clone(),
            last_walk_at: Some(now.clone()),
            file_ids,
        });
        config.add_watched_folder(cache_path_str.clone(), true);
        config.save().map_err(|e| e.to_string())?;
    }

    // Hand off to the existing rescan flow — it owns tray state,
    // schema diff, audit, and the standard scan event stream that
    // FolderDetail already knows how to render. The Drive UI just
    // additionally listens for our gdrive-watch-progress events.
    let _ = app.emit(
        "gdrive-watch-progress",
        serde_json::json!({"folder_id": folder_id, "phase": "scanning"}),
    );
    let rescan_result = rescan_folder(app.clone(), cache_path_str).await?;

    let _ = app.emit(
        "gdrive-watch-progress",
        serde_json::json!({
            "folder_id": folder_id,
            "phase": "done",
            "total_files": cached_count,
            "skipped_native": walked.skipped_native.len(),
            "skipped_unsupported": walked.skipped_unsupported.len(),
            "skipped_too_large": skipped_too_large,
        }),
    );

    Ok(serde_json::json!({
        "folder_id": folder_id,
        "files_cached": cached_count,
        "skipped_native": walked.skipped_native.len(),
        "skipped_unsupported": walked.skipped_unsupported.len(),
        "skipped_too_large": skipped_too_large,
        "folders_walked": walked.folder_count,
        "rescan": rescan_result,
    }))
}

/// Stop watching a Drive folder. Removes its entry from config and
/// purges any cached files that aren't shared with another active
/// watch. Idempotent: unwatching a folder that's already gone is OK.
#[tauri::command]
pub async fn gdrive_unwatch_folder(folder_id: String) -> Result<(), String> {
    let account = "default";
    let mut config = Config::load().map_err(|e| e.to_string())?;

    let entry = config
        .gdrive_watched_folders
        .iter()
        .find(|f| f.account_id == account && f.folder_id == folder_id)
        .cloned();

    let entry = match entry {
        Some(e) => e,
        None => return Ok(()),
    };

    // Files shared with another active watch (Drive folders can
    // overlap, especially via shared folders) must NOT be deleted.
    let shared: std::collections::HashSet<String> = config
        .gdrive_watched_folders
        .iter()
        .filter(|f| !(f.account_id == account && f.folder_id == folder_id))
        .flat_map(|f| f.file_ids.iter().cloned())
        .collect();

    for fid in &entry.file_ids {
        if !shared.contains(fid) {
            crate::gdrive_cache::forget_file(account, fid)
                .map_err(|e| e.to_string())?;
        }
    }

    config.remove_gdrive_watched_folder(account, &folder_id);

    // No more Drive watches → drop the cache root from the local
    // watched_folders list so the user doesn't see a phantom
    // "Google Drive" entry pointing at an empty cache dir.
    if config.gdrive_watched_folders.is_empty() {
        if let Ok(cache_dir) = crate::gdrive_cache::account_dir(account) {
            config.remove_watched_folder(&cache_dir.to_string_lossy());
        }
    }

    config.save().map_err(|e| e.to_string())?;
    Ok(())
}

/// List all Drive folders the user is currently watching, for the
/// UI's "Google Drive" section.
#[tauri::command]
pub async fn gdrive_list_watched_folders(
) -> Result<Vec<crate::config::GdriveWatchedFolder>, String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    Ok(config.gdrive_watched_folders.clone())
}

/// Snapshot of disk usage for the Sery data dir + free space on
/// the host volume, for the Settings → Storage page. All sizes
/// are bytes; the UI is responsible for human formatting.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StorageInfo {
    /// Total bytes Sery occupies under ~/.seryai (excluding stuff
    /// that lives elsewhere — duckdb caches in
    /// ~/Library/Application Support/sery on macOS — those add up
    /// to a few MB at most so we report them separately if at all).
    pub data_dir_bytes: u64,
    /// Bytes consumed by ~/.seryai/gdrive-cache. Surfaced
    /// separately because this is the dominant variable cost and
    /// the user has a "Clear Drive cache" recovery action for it.
    pub gdrive_cache_bytes: u64,
    /// Bytes available on the volume holding ~/.seryai. Drives
    /// the "X GB free" UI plus the pre-flight check that refuses
    /// new watches when this drops too low.
    pub free_bytes: u64,
}

#[tauri::command]
pub async fn get_storage_info() -> Result<StorageInfo, String> {
    use crate::disk_space::available_bytes;

    let data_dir = Config::data_dir().map_err(|e| e.to_string())?;
    let gdrive_cache_dir = data_dir.join("gdrive-cache");

    // Run the directory walks in spawn_blocking — for very-large
    // caches (10s of GB across 100k files) the recursive stat can
    // take a couple of seconds, which would otherwise block the
    // Tauri runtime.
    let dd = data_dir.clone();
    let gd = gdrive_cache_dir.clone();
    let (data_dir_bytes, gdrive_cache_bytes) = tokio::task::spawn_blocking(move || {
        (recursive_size(&dd), recursive_size(&gd))
    })
    .await
    .map_err(|e| format!("storage walk task: {}", e))?;

    let free_bytes = available_bytes(&data_dir).map_err(|e| e.to_string())?;
    Ok(StorageInfo {
        data_dir_bytes,
        gdrive_cache_bytes,
        free_bytes,
    })
}

/// Sum the byte size of every regular file under `root`. Returns 0
/// if `root` doesn't exist (first-run state, or post-clear). Errors
/// during the walk are swallowed silently — better to under-report
/// than to surface a syscall failure as a UI-blocker.
fn recursive_size(root: &std::path::Path) -> u64 {
    if !root.exists() {
        return 0;
    }
    let mut total: u64 = 0;
    let mut stack: Vec<std::path::PathBuf> = vec![root.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if meta.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&p) {
                for e in entries.flatten() {
                    stack.push(e.path());
                }
            }
        } else if meta.is_file() {
            total = total.saturating_add(meta.len());
        }
    }
    total
}

/// Wipe the entire Drive cache. Combines with disconnect_gdrive's
/// existing per-account wipe, but exposed separately so the user
/// can free disk WITHOUT losing their OAuth grant — useful when
/// they want to re-watch a smaller subset later.
#[tauri::command]
pub async fn clear_gdrive_cache() -> Result<(), String> {
    let data_dir = Config::data_dir().map_err(|e| e.to_string())?;
    let cache_dir = data_dir.join("gdrive-cache");
    if cache_dir.exists() {
        std::fs::remove_dir_all(&cache_dir)
            .map_err(|e| format!("remove gdrive cache: {}", e))?;
    }

    // Also drop the gdrive_watched_folders entries + the cache dir
    // from watched_folders so the next refresh / scan doesn't trip
    // over a missing directory. Tokens stay in keychain; the user
    // can re-watch without re-authenticating.
    let mut config = Config::load().map_err(|e| e.to_string())?;
    config.gdrive_watched_folders.clear();
    config.remove_watched_folder(&cache_dir.to_string_lossy());
    config.save().map_err(|e| e.to_string())?;

    // The skipped log is keyed to specific watches; once they're
    // gone the entries are stale anecdotes. Drop them too.
    crate::gdrive_skipped::clear().map_err(|e| e.to_string())?;

    Ok(())
}

/// Surface the skipped log for Settings → Storage. Returns up to
/// `limit` most-recent entries (newest first) plus a count-by-
/// reason summary so the UI can render both the totals and an
/// inspect-the-list view.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GdriveSkippedSummary {
    pub recent: Vec<crate::gdrive_skipped::SkippedEntry>,
    pub by_reason: std::collections::HashMap<String, usize>,
    pub total: usize,
}

#[tauri::command]
pub async fn get_gdrive_skipped(limit: Option<usize>) -> Result<GdriveSkippedSummary, String> {
    let limit = limit.unwrap_or(100);
    let recent = crate::gdrive_skipped::recent(limit).map_err(|e| e.to_string())?;
    let counts = crate::gdrive_skipped::count_by_reason().map_err(|e| e.to_string())?;
    let total = counts.values().sum();
    // Convert the enum keys to strings via serde so the UI can
    // index the map without re-implementing the enum on the TS
    // side (it's already serialised as snake_case by SkipReason's
    // rename_all).
    let by_reason = counts
        .into_iter()
        .map(|(k, v)| {
            let key = serde_json::to_string(&k)
                .map(|s| s.trim_matches('"').to_string())
                .unwrap_or_else(|_| "unknown".to_string());
            (key, v)
        })
        .collect();
    Ok(GdriveSkippedSummary {
        recent,
        by_reason,
        total,
    })
}

#[tauri::command]
pub async fn remove_watched_folder(path: String) -> Result<(), String> {
    let mut config = Config::load().map_err(|e| e.to_string())?;
    config.remove_watched_folder(&path);
    config.save().map_err(|e| e.to_string())?;
    // Clear any S3 credentials we stored for this URL — otherwise a
    // keyring entry lingers after the source is gone. Failure here is
    // non-fatal: the user can remove the entry manually from Keychain
    // Access if needed.
    if crate::url::is_s3_url(&path) {
        let _ = crate::remote_creds::delete(&path);
    }
    // Drop any cached scan results for this folder — otherwise re-adding
    // the same path later would surface rows for files that may have
    // moved or been deleted in the meantime. Goes through the shared
    // cache singleton.
    let _ = crate::scan_cache::with_cache(|c| c.invalidate_folder(&path));
    let _ = restart_file_watcher().await;
    Ok(())
}

// ─── MCP integration ───────────────────────────────────────────────────────

/// Toggle whether a watched folder is exposed via the MCP stdio mode.
/// Just flips the persisted flag — the MCP server is started by the
/// LLM client (Claude Desktop / Cursor / …) when the user adds the
/// corresponding `mcp.json` block. We track the state so the Settings
/// UI can show / hide the snippet generator accordingly.
#[tauri::command]
pub async fn set_folder_mcp_enabled(path: String, enabled: bool) -> Result<(), String> {
    let mut config = Config::load().map_err(|e| e.to_string())?;
    let folder = config
        .watched_folders
        .iter_mut()
        .find(|f| f.path == path)
        .ok_or_else(|| format!("watched folder not found: {path}"))?;
    folder.mcp_enabled = enabled;
    config.save().map_err(|e| e.to_string())?;
    Ok(())
}

/// One ready-to-paste snippet for a specific MCP client + folder.
/// `language` is `"json"` for Claude Desktop / Cursor / Continue
/// (they all use JSON `mcp.json` shapes) or `"toml"` for Zed (settings
/// file). Frontends render with a copy-to-clipboard button.
#[derive(Debug, serde::Serialize)]
pub struct McpSnippet {
    pub client: String,
    pub label: String,
    pub language: String,
    pub config: String,
    /// User-facing path the snippet should be written to (informational).
    pub config_path_hint: String,
}

/// Return ready-to-paste config snippets for each known LLM client,
/// for a given watched folder. The snippet embeds the absolute path
/// to the currently-running sery-link binary so users can paste it
/// straight into their LLM client config.
///
/// Frontend renders these in Settings → MCP. The user clicks a copy
/// button and pastes into their Claude Desktop / Cursor / … config.
/// We deliberately don't auto-write into those configs — JSON files
/// elsewhere on disk are easy to corrupt, and the failure mode of a
/// borked Claude Desktop config is dire. Better to show the snippet,
/// let the user own the paste.
#[tauri::command]
pub async fn get_mcp_snippets(folder_path: String) -> Result<Vec<McpSnippet>, String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    if !config.watched_folders.iter().any(|f| f.path == folder_path) {
        return Err(format!("not a watched folder: {folder_path}"));
    }

    // Resolve the path to *this* sery-link binary so the snippet
    // points at the user's actual install. On macOS that's
    //   /Applications/Sery Link.app/Contents/MacOS/SeryLink
    // On Windows + Linux it's just the binary alongside the
    // installer's chosen install dir.
    let exe = std::env::current_exe().map_err(|e| format!("cannot resolve exe path: {e}"))?;
    let exe_str = exe
        .to_str()
        .ok_or_else(|| "exe path is not valid UTF-8 (rare)".to_string())?
        .to_string();

    let server_name = "sery-link"; // What the LLM client labels the server.

    Ok(vec![
        // Claude Desktop — JSON, ~/Library/Application Support/Claude/claude_desktop_config.json
        McpSnippet {
            client: "claude-desktop".to_string(),
            label: "Claude Desktop".to_string(),
            language: "json".to_string(),
            config: claude_desktop_snippet(server_name, &exe_str, &folder_path),
            config_path_hint: claude_desktop_config_path_hint(),
        },
        // Cursor — JSON, ~/.cursor/mcp.json (or .cursor/mcp.json per-project)
        McpSnippet {
            client: "cursor".to_string(),
            label: "Cursor".to_string(),
            language: "json".to_string(),
            config: cursor_snippet(server_name, &exe_str, &folder_path),
            config_path_hint: "~/.cursor/mcp.json (global) — or .cursor/mcp.json in your project root for project-scoped MCP".to_string(),
        },
        // Continue — JSON, ~/.continue/config.json
        McpSnippet {
            client: "continue".to_string(),
            label: "Continue".to_string(),
            language: "json".to_string(),
            config: continue_snippet(server_name, &exe_str, &folder_path),
            config_path_hint: "~/.continue/config.json under the experimental.modelContextProtocolServers key"
                .to_string(),
        },
    ])
}

fn claude_desktop_snippet(server: &str, exe: &str, root: &str) -> String {
    // Claude Desktop's mcp.json shape — top-level "mcpServers"
    // object keyed by server name. The user adds this entry; if
    // they already have other MCP servers, they merge.
    serde_json::to_string_pretty(&serde_json::json!({
        "mcpServers": {
            server: {
                "command": exe,
                "args": ["--mcp-stdio", "--root", root]
            }
        }
    }))
    .unwrap_or_else(|_| String::new())
}

fn cursor_snippet(server: &str, exe: &str, root: &str) -> String {
    // Cursor's mcp.json uses the same `mcpServers` shape Claude
    // Desktop pioneered, so the snippet body is identical.
    claude_desktop_snippet(server, exe, root)
}

fn continue_snippet(_server: &str, exe: &str, root: &str) -> String {
    // Continue's config.json embeds MCP servers under a
    // experimental.modelContextProtocolServers array. Slightly
    // different shape from the other two; we match the format
    // documented at https://docs.continue.dev.
    serde_json::to_string_pretty(&serde_json::json!({
        "experimental": {
            "modelContextProtocolServers": [
                {
                    "transport": {
                        "type": "stdio",
                        "command": exe,
                        "args": ["--mcp-stdio", "--root", root]
                    }
                }
            ]
        }
    }))
    .unwrap_or_else(|_| String::new())
}

fn claude_desktop_config_path_hint() -> String {
    // Platform-specific paths so the snippet card can show the user
    // exactly where to paste.
    if cfg!(target_os = "macos") {
        "~/Library/Application Support/Claude/claude_desktop_config.json".to_string()
    } else if cfg!(target_os = "windows") {
        "%APPDATA%\\Claude\\claude_desktop_config.json".to_string()
    } else {
        "~/.config/Claude/claude_desktop_config.json".to_string()
    }
}

// ---------------------------------------------------------------------------
// Global search — the v1 hero feature. Ranks cached datasets by filename,
// column name, and document content against a user query. See
// `docs/local-first positioning` memory note for why this is the core wedge.
// ---------------------------------------------------------------------------

/// Why a particular dataset matched a search query. Multiple reasons can
/// apply to the same file (e.g. filename + column name both contain the
/// query) — the UI surfaces each as a badge so users understand why a
/// result is ranked where it is.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SearchMatchReason {
    /// The query matched somewhere in the relative path (filename or
    /// any parent segment).
    Filename,
    /// The query matched a column name in a tabular file's schema.
    Column {
        name: String,
        col_type: String,
    },
    /// The query matched document-extracted text. Snippet is a ±40-char
    /// window around the first match, so the UI can show context.
    Content {
        snippet: String,
    },
    /// The match comes from the gdrive-skipped log — Sery knows this
    /// Drive file exists but didn't cache its contents (too big,
    /// non-indexable type, Google Doc, etc.). The UI badges the row
    /// distinctly and disables navigation since there's no parsed
    /// content to inspect.
    SkippedDrive {
        reason: crate::gdrive_skipped::SkipReason,
    },
}

/// One search result: a single dataset plus the reasons it matched and a
/// relevance score (higher = better).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchMatch {
    pub folder_path: String,
    pub relative_path: String,
    pub file_format: String,
    pub size_bytes: u64,
    pub last_modified: String,
    pub row_count_estimate: Option<i64>,
    pub column_count: usize,
    pub match_reasons: Vec<SearchMatchReason>,
    pub score: i32,
}

const SEARCH_RESULT_LIMIT: usize = 200;

/// Global column-aware search. Reads every cached dataset via the shared
/// `scan_cache` singleton and scores each by filename / column-name /
/// content match against the query. Case-insensitive. Empty query
/// returns `[]`.
///
/// Runs entirely on the scan cache — no disk re-read — so results are
/// instant for cache-hit datasets. Files that haven't been scanned yet
/// won't appear; the user needs to open their folder(s) first to
/// populate the cache.
#[tauri::command]
pub async fn search_all_folders(query: String) -> Result<Vec<SearchMatch>, String> {
    let trimmed = query.trim().to_string();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    tokio::task::spawn_blocking(move || {
        let entries = crate::scan_cache::with_cache(|c| c.get_all_entries())
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or_default();

        let mut matches = rank_matches(&entries, &trimmed);

        // Append filename-only matches from the gdrive-skipped log.
        // These represent Drive files Sery knows about but didn't
        // cache — without this the user gets silent zeros when
        // searching for them, and Drive becomes a black box. The
        // log is bounded at 10k entries by rotation, so loading it
        // all is cheap.
        if let Ok(skipped) = crate::gdrive_skipped::recent(usize::MAX) {
            matches.extend(rank_skipped(&skipped, &trimmed));
        }

        // Stable re-sort so skipped results land where their score
        // earns them — usually below content-indexed hits since
        // their score comes from filename only.
        matches.sort_by(|a, b| b.score.cmp(&a.score));
        matches.truncate(SEARCH_RESULT_LIMIT);
        Ok(matches)
    })
    .await
    .map_err(|e| format!("search task failed: {}", e))?
}

/// Score skipped-log entries against the same query. Filename only
/// — there's no schema or content to look at, by definition.
fn rank_skipped(
    entries: &[crate::gdrive_skipped::SkippedEntry],
    query: &str,
) -> Vec<SearchMatch> {
    let q = query.to_lowercase();
    let mut out: Vec<SearchMatch> = Vec::new();
    for entry in entries {
        let name_lower = entry.name.to_lowercase();
        let score = if name_lower == q {
            // Match the rank_matches scoring band so an exact
            // skipped-file name doesn't outrank an indexed file
            // with the same name + content match. We dock 20
            // points across the board since "filename only" is
            // strictly weaker context.
            100
        } else if name_lower.starts_with(&q) {
            75
        } else if name_lower.contains(&q) {
            55
        } else {
            continue;
        };

        // file_format is the extension lowercased; empty string for
        // the no-extension edge case is fine — the UI badges it as
        // "filename only" anyway.
        let file_format = std::path::Path::new(&entry.name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        out.push(SearchMatch {
            // Intentionally empty: the row is non-navigable because
            // there's no cached content to drill into. Frontend
            // checks for a SkippedDrive reason and disables the
            // click handler.
            folder_path: String::new(),
            relative_path: entry.name.clone(),
            file_format,
            size_bytes: entry.size_bytes.unwrap_or(0),
            last_modified: entry.skipped_at.clone(),
            row_count_estimate: None,
            column_count: 0,
            match_reasons: vec![
                SearchMatchReason::Filename,
                SearchMatchReason::SkippedDrive { reason: entry.reason },
            ],
            score,
        });
    }
    out
}

/// Pure scoring function — split out for testing. Caller guarantees
/// `query` is non-empty (the command short-circuits empty queries so
/// we never pay the iteration cost).
fn rank_matches(
    entries: &[crate::scan_cache::CachedEntry],
    query: &str,
) -> Vec<SearchMatch> {
    let q = query.to_lowercase();
    let mut matches: Vec<SearchMatch> = Vec::new();

    for entry in entries {
        let mut reasons: Vec<SearchMatchReason> = Vec::new();
        let mut score: i32 = 0;

        // --- Filename ---
        // We match against the relative path (so "2024/report" hits)
        // and separately against the basename (so "report.csv" scores
        // higher than buried-in-a-subfolder hits).
        let rel_lower = entry.relative_path.to_lowercase();
        let basename_lower: String = std::path::Path::new(&entry.relative_path)
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_else(|| rel_lower.clone());

        if basename_lower == q {
            score += 120;
            reasons.push(SearchMatchReason::Filename);
        } else if basename_lower.starts_with(&q) {
            score += 95;
            reasons.push(SearchMatchReason::Filename);
        } else if basename_lower.contains(&q) {
            score += 75;
            reasons.push(SearchMatchReason::Filename);
        } else if rel_lower.contains(&q) {
            // Matched a parent directory segment — weaker signal.
            score += 40;
            reasons.push(SearchMatchReason::Filename);
        }

        // --- Column name (tabular files only) ---
        // Stop after the strongest single column match. Showing five
        // near-duplicate column reasons for one file is noise.
        let mut best_col_score = 0i32;
        let mut best_col: Option<(String, String)> = None;
        for col in &entry.metadata.schema {
            let col_lower = col.name.to_lowercase();
            let col_score = if col_lower == q {
                95
            } else if col_lower.starts_with(&q) {
                75
            } else if col_lower.contains(&q) {
                55
            } else {
                0
            };
            if col_score > best_col_score {
                best_col_score = col_score;
                best_col = Some((col.name.clone(), col.col_type.clone()));
            }
        }
        if let Some((name, col_type)) = best_col {
            score += best_col_score;
            reasons.push(SearchMatchReason::Column { name, col_type });
        }

        // --- Document content (docs only) ---
        if let Some(md) = &entry.metadata.document_markdown {
            let md_lower = md.to_lowercase();
            if let Some(pos) = md_lower.find(&q) {
                let start = pos.saturating_sub(40);
                let end = (pos + q.len() + 40).min(md.len());
                // Snap to valid char boundaries so we don't slice in the
                // middle of a UTF-8 codepoint.
                let start = find_char_boundary(md, start, false);
                let end = find_char_boundary(md, end, true);
                let snippet = md[start..end].replace('\n', " ").trim().to_string();
                score += 50;
                reasons.push(SearchMatchReason::Content { snippet });
            }
        }

        if !reasons.is_empty() {
            matches.push(SearchMatch {
                folder_path: entry.folder_path.clone(),
                relative_path: entry.relative_path.clone(),
                file_format: entry.metadata.file_format.clone(),
                size_bytes: entry.metadata.size_bytes,
                last_modified: entry.metadata.last_modified.clone(),
                row_count_estimate: entry.metadata.row_count_estimate,
                column_count: entry.metadata.schema.len(),
                match_reasons: reasons,
                score,
            });
        }
    }

    matches.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });
    matches.truncate(SEARCH_RESULT_LIMIT);
    matches
}

/// Walk `idx` up/down until it lands on a UTF-8 char boundary. Used to
/// safely slice document snippets; without this a multibyte char at the
/// boundary would panic `str::slice_index`.
fn find_char_boundary(s: &str, mut idx: usize, forward: bool) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    while idx > 0 && !s.is_char_boundary(idx) {
        if forward {
            idx += 1;
            if idx >= s.len() {
                return s.len();
            }
        } else {
            idx -= 1;
        }
    }
    idx
}

// ---------------------------------------------------------------------------
// Scan + sync
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn scan_folder(folder_path: String) -> Result<Vec<DatasetMetadata>, String> {
    scanner::scan_folder(&folder_path)
        .await
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Per-file profiling — used by the FileDetail page's "Profile this file"
// action to show per-column stats (null %, unique count, min/max/avg) without
// writing or seeing any SQL. Thin wrapper around DuckDB's SUMMARIZE.
// ---------------------------------------------------------------------------

/// One row of DuckDB's SUMMARIZE output, lightly renamed for the UI. Values
/// are strings (DuckDB emits min/max as VARCHAR so all column types are
/// representable) so the frontend doesn't need to worry about numeric
/// precision or timestamp formatting.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ColumnProfile {
    pub column_name: String,
    pub column_type: String,
    pub count: Option<i64>,
    pub null_percentage: Option<f64>,
    pub approx_unique: Option<i64>,
    pub min: Option<String>,
    pub max: Option<String>,
    pub avg: Option<String>,
    pub std: Option<String>,
}

/// Compute per-column stats for a file under a watched folder. Runs
/// locally (no cloud). Tabular formats only — docx/pptx/html have no
/// columnar structure to profile, so the command errors.
///
/// Reuses the existing Parquet cache: CSV/XLSX files go through the
/// same `csv_to_parquet` pipeline we already use for schema extraction,
/// so a second SUMMARIZE pass on the same file is cheap.
#[tauri::command]
pub async fn profile_dataset(
    folder_path: String,
    relative_path: String,
) -> Result<Vec<ColumnProfile>, String> {
    tokio::task::spawn_blocking(move || profile_blocking(&folder_path, &relative_path))
        .await
        .map_err(|e| format!("profile task failed: {}", e))?
}

fn profile_blocking(folder_path: &str, relative_path: &str) -> Result<Vec<ColumnProfile>, String> {
    use std::borrow::Cow;
    use std::path::{Path, PathBuf};

    // Remote source branch — the folder_path IS the URL and
    // relative_path is just a display filename we synthesized at scan
    // time. We hand the URL straight to DuckDB after loading httpfs;
    // Parquet and CSV are the two formats Phase A supports.
    if crate::url::is_remote_url(folder_path) {
        return profile_remote(folder_path);
    }

    // Compose the absolute file path. `relative_path` comes from the scan
    // cache (which the frontend populated via scan_folder) so it's
    // trusted input — we still guard against path traversal by requiring
    // the final path to start with the folder path.
    let full_path: PathBuf = Path::new(folder_path).join(relative_path);
    if !full_path.starts_with(folder_path) {
        return Err("invalid relative path".to_string());
    }
    if !full_path.exists() {
        return Err(format!("file not found: {}", full_path.display()));
    }

    let ext = full_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Route CSV / XLSX through the Parquet cache so SUMMARIZE runs on
    // a columnar format — orders of magnitude faster, and the cache
    // was already built during the scan anyway.
    let (effective_path, effective_ext): (Cow<Path>, &str) = match ext.as_str() {
        "xlsx" | "xls" => {
            let csv = crate::excel::xlsx_to_csv(&full_path).map_err(|e| e.to_string())?;
            let parquet = crate::csv::csv_to_parquet(&csv).map_err(|e| e.to_string())?;
            (Cow::Owned(parquet), "parquet")
        }
        "csv" => {
            let parquet = crate::csv::csv_to_parquet(&full_path).map_err(|e| e.to_string())?;
            (Cow::Owned(parquet), "parquet")
        }
        "parquet" => (Cow::Borrowed(full_path.as_path()), "parquet"),
        other => {
            return Err(format!(
                "can't profile {} files — stats are only available for tabular data",
                other
            ));
        }
    };

    let read_func = match effective_ext {
        "parquet" => "read_parquet",
        _ => {
            return Err(format!(
                "unsupported format after conversion: {}",
                effective_ext
            ))
        }
    };

    let path_str = effective_path.to_string_lossy().replace('\'', "''");

    // Run the DuckDB work inside catch_unwind. duckdb-rs occasionally
    // panics inside its C bindings (e.g. when Statement::column_names()
    // is called before the query has been executed — the original
    // version of this function hit exactly that panic). catch_unwind
    // catches Rust panics (not foreign exceptions) and converts them
    // into a normal error so the frontend shows a friendly message
    // instead of the whole spawn_blocking task aborting.
    let read_func_owned = read_func.to_string();
    let path_owned = path_str;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        run_summarize(&read_func_owned, &path_owned)
    }));

    match result {
        Ok(r) => r,
        Err(_payload) => Err(
            "profile panicked inside DuckDB — this file may be malformed or use \
             an unsupported type. Try re-scanning the folder."
                .to_string(),
        ),
    }
}

/// Profile a remote URL. Loads httpfs + (for s3://) credentials into
/// the session and runs the same SUMMARIZE pattern used for local
/// files. Wrapped in catch_unwind to match the local path's panic
/// safety.
fn profile_remote(url: &str) -> Result<Vec<ColumnProfile>, String> {
    let ext = crate::url::extension_from_url(url);
    let read_func = match ext.as_str() {
        "parquet" => "read_parquet",
        "csv" | "tsv" => "read_csv_auto",
        other => {
            return Err(format!(
                "can't profile remote {} files — Phase A/B support csv / parquet URLs",
                if other.is_empty() { "unknown" } else { other }
            ));
        }
    };
    let escaped = url.replace('\'', "''");
    let url_owned = url.to_string();
    let read_func_owned = read_func.to_string();
    let escaped_owned = escaped;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        run_remote_summarize(&url_owned, &read_func_owned, &escaped_owned)
    }));
    match result {
        Ok(r) => r,
        Err(_) => Err(
            "profile panicked inside DuckDB — the URL may be unreachable \
             or serving an unexpected format."
                .to_string(),
        ),
    }
}

fn run_remote_summarize(
    url: &str,
    read_func: &str,
    escaped_url: &str,
) -> Result<Vec<ColumnProfile>, String> {
    use duckdb::Connection;
    let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
    // Install + load httpfs — we're on a fresh connection so the
    // extension isn't loaded by default.
    conn.execute_batch("INSTALL httpfs; LOAD httpfs;")
        .map_err(|e| format!("load httpfs: {}", e))?;
    // S3 URLs need credentials in the session before we can run any
    // query against them. The scanner already persisted them to the
    // keyring when the source was added.
    if crate::url::is_s3_url(url) {
        crate::remote::apply_s3_credentials(&conn, url).map_err(|e| e.to_string())?;
    }
    let sql = format!(
        "SELECT * FROM (SUMMARIZE SELECT * FROM {}('{}'))",
        read_func, escaped_url
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let str_by_name = |name: &str| -> Option<String> {
                row.get::<_, Option<String>>(name).ok().flatten()
            };
            let i64_by_name = |name: &str| -> Option<i64> {
                row.get::<_, Option<i64>>(name).ok().flatten()
            };
            let f64_by_name = |name: &str| -> Option<f64> {
                row.get::<_, Option<f64>>(name).ok().flatten()
            };
            Ok(ColumnProfile {
                column_name: str_by_name("column_name").unwrap_or_default(),
                column_type: str_by_name("column_type").unwrap_or_default(),
                count: i64_by_name("count"),
                null_percentage: f64_by_name("null_percentage").or_else(|| {
                    str_by_name("null_percentage").and_then(|s| s.parse::<f64>().ok())
                }),
                approx_unique: i64_by_name("approx_unique"),
                min: str_by_name("min"),
                max: str_by_name("max"),
                avg: str_by_name("avg"),
                std: str_by_name("std"),
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        if let Ok(p) = row {
            out.push(p);
        }
    }
    Ok(out)
}

/// Execute SUMMARIZE and materialize the result into `ColumnProfile` rows.
/// Split out of `profile_blocking` so the whole DuckDB interaction sits
/// inside a single `catch_unwind` boundary.
///
/// Columns are accessed by NAME inside the per-row closure rather than
/// via `Statement::column_names()` — the latter panicked in duckdb-rs
/// when called before the statement had been stepped.
fn run_summarize(read_func: &str, path_str: &str) -> Result<Vec<ColumnProfile>, String> {
    use duckdb::Connection;

    let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
    // Wrap SUMMARIZE in a regular SELECT. Running `SUMMARIZE <query>`
    // as a top-level statement goes through a code path in duckdb-rs
    // that panicked inside `raw_statement.rs` at line 239
    // (`Option::unwrap()` on None) because the column metadata wasn't
    // populated the same way as a normal result set. Wrapping it as a
    // subquery makes DuckDB treat it as an ordinary SELECT and the
    // column info arrives before we start iterating rows.
    let sql = format!(
        "SELECT * FROM (SUMMARIZE SELECT * FROM {}('{}'))",
        read_func, path_str
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            // Helpers that return None on any lookup/conversion error
            // (wrong-type column, absent column, null). This way a
            // version bump that drops or renames one SUMMARIZE column
            // just leaves that field blank instead of breaking the
            // whole profile.
            let str_by_name = |name: &str| -> Option<String> {
                row.get::<_, Option<String>>(name).ok().flatten()
            };
            let i64_by_name = |name: &str| -> Option<i64> {
                row.get::<_, Option<i64>>(name).ok().flatten()
            };
            let f64_by_name = |name: &str| -> Option<f64> {
                row.get::<_, Option<f64>>(name).ok().flatten()
            };

            Ok(ColumnProfile {
                column_name: str_by_name("column_name").unwrap_or_default(),
                column_type: str_by_name("column_type").unwrap_or_default(),
                count: i64_by_name("count"),
                // null_percentage is DECIMAL in DuckDB; the rust binding
                // may not deserialise that as f64 directly, so we also
                // try reading it as a string and parsing.
                null_percentage: f64_by_name("null_percentage").or_else(|| {
                    str_by_name("null_percentage").and_then(|s| s.parse::<f64>().ok())
                }),
                approx_unique: i64_by_name("approx_unique"),
                min: str_by_name("min"),
                max: str_by_name("max"),
                avg: str_by_name("avg"),
                std: str_by_name("std"),
            })
        })
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for row in rows {
        if let Ok(p) = row {
            out.push(p);
        }
    }
    Ok(out)
}

/// Read every cached dataset for a folder without touching disk beyond
/// the scan cache. Used by `FolderDetail` to paint instantly — rows will
/// be reconciled against fresh data via `dataset_scanned` events once the
/// background rescan kicks in. Returns an empty list (not an error) if
/// the cache hasn't seen this folder yet.
#[tauri::command]
pub async fn get_cached_folder_metadata(folder_path: String) -> Result<Vec<DatasetMetadata>, String> {
    use std::io::Write;
    eprintln!("[get_cached] ▶ {}", folder_path);
    let _ = std::io::stderr().flush();

    let result: Result<Vec<DatasetMetadata>, String> = tokio::task::spawn_blocking(move || {
        // Goes through the process-wide scan cache singleton; no extra
        // DuckDB connections are opened here. Returns empty list if the
        // singleton failed to initialise (the scanner will still work).
        let rows = crate::scan_cache::with_cache(|c| c.get_all_for_folder(&folder_path))
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        eprintln!("[get_cached] got {} rows", rows.len());
        let _ = std::io::stderr().flush();
        Ok(rows)
    })
    .await
    .map_err(|e| format!("cache read task failed: {}", e))?;

    result
}

// ─── Format conversion (FileDetail action) ──────────────────────────────────
//
// User clicks "Convert to Parquet" on a CSV / XLSX file → we write the
// parquet next to the source so it lands in the same watched folder + the
// existing file watcher picks it up on the next scan tick.
//
// Why surface this: parquet is 5-50x smaller AND queries 10-100x faster than
// equivalent CSVs. Users with raw CSV exports benefit a lot, and the
// conversion is one-line for someone with DuckDB CLI but invisible
// otherwise. Sery already has the engine wired in; exposing it as a button
// turns latent capability into product surface.

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConvertResult {
    /// Absolute path to the new parquet file. Sits next to the source
    /// in the same folder, so the existing file watcher will pick it
    /// up on its next debounced sync.
    pub output_path: String,
    /// Relative path within the watched folder — what the UI will
    /// navigate to after the convert succeeds.
    pub relative_path: String,
    /// Bytes written. Useful for the "shrunk from N to M" copy.
    pub size_bytes: u64,
    /// Source size for the same comparison.
    pub source_size_bytes: u64,
}

/// Convert a CSV / XLSX dataset to Parquet, writing the result next
/// to the source. Returns the new file's path so the UI can navigate
/// to it. If a `<basename>.parquet` already exists at the destination,
/// we append `-converted-1`, `-2`, etc. until we find a free slot —
/// won't clobber the user's existing files.
#[tauri::command]
pub async fn convert_to_parquet(
    folder_path: String,
    relative_path: String,
) -> Result<ConvertResult, String> {
    tokio::task::spawn_blocking(move || convert_to_parquet_blocking(&folder_path, &relative_path))
        .await
        .map_err(|e| format!("convert task failed: {}", e))?
}

fn convert_to_parquet_blocking(
    folder_path: &str,
    relative_path: &str,
) -> Result<ConvertResult, String> {
    use duckdb::Connection;
    use std::path::{Path, PathBuf};

    if crate::url::is_remote_url(folder_path) {
        return Err(
            "Conversion is local-only — fetch the file first or use a local watched \
             folder.".to_string(),
        );
    }

    let full_path: PathBuf = Path::new(folder_path).join(relative_path);
    if !full_path.starts_with(folder_path) {
        return Err("invalid relative path".to_string());
    }
    if !full_path.exists() {
        return Err(format!("file not found: {}", full_path.display()));
    }

    let ext = full_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    if ext == "parquet" {
        return Err("File is already Parquet.".to_string());
    }
    if !matches!(ext.as_str(), "csv" | "tsv" | "xlsx" | "xls") {
        return Err(format!(
            "Convert only supports csv / tsv / xlsx / xls right now (got: {}).",
            ext
        ));
    }

    let source_size_bytes = std::fs::metadata(&full_path)
        .map(|m| m.len())
        .unwrap_or(0);

    // Excel: lean on the existing xlsx_to_csv pipeline to get a
    // CSV intermediate, then read that into parquet. xlsx_to_csv
    // writes to a temp dir; we don't pollute the user's folder.
    let csv_for_read = if matches!(ext.as_str(), "xlsx" | "xls") {
        Some(crate::excel::xlsx_to_csv(&full_path).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let read_path = csv_for_read.as_deref().unwrap_or(full_path.as_path());
    let escaped_read = read_path.to_string_lossy().replace('\'', "''");

    // Pick a non-colliding destination next to the source.
    let dest = pick_convert_destination(&full_path)?;
    let escaped_dest = dest.to_string_lossy().replace('\'', "''");

    let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
    // Same fallback ladder csv.rs uses internally — header on, then
    // permissive, then no-header, then all_varchar. We want this to
    // succeed on weird CSVs the user might have.
    let attempts = [
        "read_csv_auto('{p}', header=true)",
        "read_csv_auto('{p}', header=true, ignore_errors=true, null_padding=true)",
        "read_csv_auto('{p}', header=false, ignore_errors=true, null_padding=true)",
        "read_csv_auto('{p}', header=true, ignore_errors=true, null_padding=true, all_varchar=true)",
    ];
    let mut last_err: Option<String> = None;
    for tmpl in attempts {
        let reader = tmpl.replace("{p}", &escaped_read);
        let sql = format!(
            "COPY (SELECT * FROM {reader}) TO '{escaped_dest}' (FORMAT PARQUET, COMPRESSION 'zstd')"
        );
        match conn.execute(&sql, []) {
            Ok(_) => {
                let size_bytes = std::fs::metadata(&dest)
                    .map(|m| m.len())
                    .unwrap_or(0);
                let new_relative = dest
                    .strip_prefix(folder_path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| dest.file_name().map_or_else(
                        || dest.to_string_lossy().to_string(),
                        |s| s.to_string_lossy().to_string(),
                    ));
                return Ok(ConvertResult {
                    output_path: dest.to_string_lossy().to_string(),
                    relative_path: new_relative,
                    size_bytes,
                    source_size_bytes,
                });
            }
            Err(e) => {
                // Clean up partial file before the next attempt — DuckDB
                // refuses to overwrite by default and the next COPY
                // would fail for the wrong reason.
                let _ = std::fs::remove_file(&dest);
                last_err = Some(e.to_string());
            }
        }
    }
    Err(format!(
        "DuckDB couldn't convert {}: {}",
        full_path.display(),
        last_err.unwrap_or_else(|| "unknown error".to_string())
    ))
}

/// Build a non-colliding destination path next to the source. First
/// preference is `<basename>.parquet`; if that exists we walk
/// `-converted-1.parquet`, `-converted-2.parquet`, … until we find a
/// free slot. Caps at -999 so a runaway loop can't churn forever
/// (very unlikely but cheap insurance).
fn pick_convert_destination(source: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let parent = source.parent().ok_or_else(|| "no parent dir".to_string())?;
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "no file stem".to_string())?;
    let first = parent.join(format!("{}.parquet", stem));
    if !first.exists() {
        return Ok(first);
    }
    for i in 1..1000 {
        let candidate = parent.join(format!("{}-converted-{}.parquet", stem, i));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("Couldn't find a free destination filename — clean up old conversions first.".to_string())
}

// ─── Inline data preview (FileDetail "Browse" section) ──────────────────────
//
// User-facing surface: open a tabular file in FileDetail, see its rows in a
// virtualized table — not just the 5 sample rows the cache holds. This is
// the file-manager differentiator that justifies the v0.5.3 → pivot story
// ("a great file manager for data files").
//
// Data path: read via DuckDB the same way `profile_dataset` does — CSV /
// XLSX go through the existing `csv_to_parquet` cache so a second read is
// cheap, parquet reads directly. Cap at MAX_PREVIEW_ROWS to keep memory
// + render bounded; the UI surfaces "showing N of M" so the user knows
// when more rows exist.

const MAX_PREVIEW_ROWS: usize = 5_000;

#[derive(Debug, Clone, serde::Serialize)]
pub struct DatasetRows {
    pub columns: Vec<String>,
    /// Each row's cells stringified for stable JSON shape. The frontend
    /// renders them as text; numeric / date typing comes from `columns`
    /// metadata if needed downstream.
    pub rows: Vec<Vec<String>>,
    /// True row count when known (cheap COUNT(*) on parquet); -1 when
    /// the count would require reading the whole file (CSV with no
    /// stats). The UI shows "N+ rows" in that case.
    pub total_rows: i64,
    /// True if the read hit MAX_PREVIEW_ROWS. UI shows
    /// "showing first {len} of {total} rows" + a hint to use the
    /// cloud Ask page for full analysis.
    pub truncated: bool,
}

#[tauri::command]
pub async fn read_dataset_rows(
    folder_path: String,
    relative_path: String,
) -> Result<DatasetRows, String> {
    tokio::task::spawn_blocking(move || read_rows_blocking(&folder_path, &relative_path))
        .await
        .map_err(|e| format!("rows task failed: {}", e))?
}

fn read_rows_blocking(folder_path: &str, relative_path: &str) -> Result<DatasetRows, String> {
    use std::borrow::Cow;
    use std::path::{Path, PathBuf};

    if crate::url::is_remote_url(folder_path) {
        return read_rows_remote(folder_path);
    }

    let full_path: PathBuf = Path::new(folder_path).join(relative_path);
    if !full_path.starts_with(folder_path) {
        return Err("invalid relative path".to_string());
    }
    if !full_path.exists() {
        return Err(format!("file not found: {}", full_path.display()));
    }

    let ext = full_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Same cache path as profile_dataset — CSV / XLSX → Parquet so
    // repeat reads on the same file are essentially free.
    let (effective_path, effective_ext): (Cow<Path>, &str) = match ext.as_str() {
        "xlsx" | "xls" => {
            let csv = crate::excel::xlsx_to_csv(&full_path).map_err(|e| e.to_string())?;
            let parquet = crate::csv::csv_to_parquet(&csv).map_err(|e| e.to_string())?;
            (Cow::Owned(parquet), "parquet")
        }
        "csv" => {
            let parquet = crate::csv::csv_to_parquet(&full_path).map_err(|e| e.to_string())?;
            (Cow::Owned(parquet), "parquet")
        }
        "parquet" => (Cow::Borrowed(full_path.as_path()), "parquet"),
        other => {
            return Err(format!(
                "can't preview rows for {} files — only tabular formats supported",
                other
            ));
        }
    };

    let read_func = match effective_ext {
        "parquet" => "read_parquet",
        _ => return Err(format!("unsupported format: {}", effective_ext)),
    };

    let path_str = effective_path.to_string_lossy().replace('\'', "''");

    let read_func_owned = read_func.to_string();
    let path_owned = path_str;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        run_read_rows(&read_func_owned, &path_owned)
    }))
    .unwrap_or_else(|_| {
        Err(
            "row read panicked inside DuckDB — file may be malformed. \
             Try re-scanning the folder."
                .to_string(),
        )
    })
}

fn read_rows_remote(url: &str) -> Result<DatasetRows, String> {
    let ext = crate::url::extension_from_url(url);
    let read_func = match ext.as_str() {
        "parquet" => "read_parquet",
        "csv" | "tsv" => "read_csv_auto",
        other => {
            return Err(format!(
                "can't preview rows for {} URLs — only csv / parquet supported",
                other
            ));
        }
    };
    let escaped = url.replace('\'', "''");
    use duckdb::Connection;
    let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
    conn.execute_batch("INSTALL httpfs; LOAD httpfs;")
        .map_err(|e| format!("load httpfs: {}", e))?;
    if crate::url::is_s3_url(url) {
        crate::remote::apply_s3_credentials(&conn, url).map_err(|e| e.to_string())?;
    }
    run_read_rows_with_conn(&conn, read_func, &escaped)
}

fn run_read_rows(read_func: &str, path_str: &str) -> Result<DatasetRows, String> {
    use duckdb::Connection;
    let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
    run_read_rows_with_conn(&conn, read_func, path_str)
}

fn run_read_rows_with_conn(
    conn: &duckdb::Connection,
    read_func: &str,
    path_str: &str,
) -> Result<DatasetRows, String> {
    // Total count first — separate query so the row read can use a
    // simple LIMIT without us needing to count after-the-fact. Cheap
    // on parquet (header metadata); for read_csv_auto over remote
    // URLs it's a full scan, so we use try_to_compute_count which
    // returns -1 instead of waiting forever.
    let total_rows = compute_total_rows(conn, read_func, path_str).unwrap_or(-1);

    // Same DESCRIBE-then-execute pattern text_to_sql uses to avoid
    // duckdb-rs panicking when column metadata is read pre-execute.
    let select_sql = format!(
        "SELECT * FROM {}('{}') LIMIT {}",
        read_func, path_str, MAX_PREVIEW_ROWS
    );
    let describe_sql = format!("DESCRIBE ({})", select_sql);
    let columns: Vec<String> = match conn.prepare(&describe_sql) {
        Ok(mut s) => match s.query_map([], |row| row.get::<_, String>(0)) {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(e) => return Err(format!("describe failed: {}", e)),
        },
        Err(e) => return Err(format!("describe prepare failed: {}", e)),
    };
    let column_count = columns.len();

    let mut stmt = conn
        .prepare(&select_sql)
        .map_err(|e| format!("select prepare failed: {}", e))?;

    let rows_iter = stmt
        .query_map([], |row| {
            let mut out: Vec<String> = Vec::with_capacity(column_count);
            for i in 0..column_count {
                let v: duckdb::types::Value = row
                    .get::<_, duckdb::types::Value>(i)
                    .unwrap_or(duckdb::types::Value::Null);
                out.push(stringify_cell(&v));
            }
            Ok(out)
        })
        .map_err(|e| format!("select execute failed: {}", e))?;

    let mut rows: Vec<Vec<String>> = Vec::new();
    for r in rows_iter {
        match r {
            Ok(row) => rows.push(row),
            Err(e) => return Err(format!("row read failed: {}", e)),
        }
        if rows.len() >= MAX_PREVIEW_ROWS {
            break;
        }
    }

    // Truncated = "we stopped at the limit AND there are more rows
    // available." When total_rows is unknown (-1) we still flag
    // truncated if we hit the cap, since we can't prove the file
    // is shorter than that.
    let truncated = rows.len() >= MAX_PREVIEW_ROWS
        && (total_rows < 0 || total_rows > rows.len() as i64);

    Ok(DatasetRows {
        columns,
        rows,
        total_rows,
        truncated,
    })
}

fn compute_total_rows(
    conn: &duckdb::Connection,
    read_func: &str,
    path_str: &str,
) -> Option<i64> {
    let sql = format!("SELECT COUNT(*) FROM {}('{}')", read_func, path_str);
    let mut stmt = conn.prepare(&sql).ok()?;
    let row: i64 = stmt.query_row([], |r| r.get(0)).ok()?;
    Some(row)
}

/// Stable string repr for one DuckDB cell. Strings come out unquoted
/// (the UI cell renders them as-is); NULL becomes "NULL" so missing
/// values are visible.
fn stringify_cell(v: &duckdb::types::Value) -> String {
    use duckdb::types::Value;
    match v {
        Value::Null => "NULL".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::TinyInt(n) => n.to_string(),
        Value::SmallInt(n) => n.to_string(),
        Value::Int(n) => n.to_string(),
        Value::BigInt(n) => n.to_string(),
        Value::HugeInt(n) => n.to_string(),
        Value::UTinyInt(n) => n.to_string(),
        Value::USmallInt(n) => n.to_string(),
        Value::UInt(n) => n.to_string(),
        Value::UBigInt(n) => n.to_string(),
        Value::Float(n) => n.to_string(),
        Value::Double(n) => n.to_string(),
        Value::Decimal(d) => d.to_string(),
        Value::Text(s) => s.clone(),
        Value::Blob(b) => format!("<{} bytes>", b.len()),
        Value::Date32(d) => d.to_string(),
        Value::Time64(_, t) => t.to_string(),
        Value::Timestamp(_, t) => t.to_string(),
        other => format!("{:?}", other),
    }
}

/// Rescan a folder and sync its metadata to the cloud in one shot. Emits
/// scan_progress / scan_complete events and records an audit entry so the
/// Privacy tab can show what was uploaded.
#[tauri::command]
pub async fn rescan_folder<R: Runtime>(app: AppHandle<R>, folder_path: String) -> Result<Value, String> {
    let started = std::time::Instant::now();

    // Flip tray to "syncing" so users see the work kick off.
    crate::tray::set_state(&app, "syncing");

    // 1. Scan the folder locally with progress + per-dataset events. Both
    // callbacks run on the blocking scan thread so the UI stays smooth
    // even for huge folders. The dataset callback lets FolderDetail stream
    // rows into view incrementally instead of waiting for the whole folder
    // to finish — important for first-time scans of large folders where
    // the wait would otherwise be minutes of empty screen.
    let folder_for_walk = folder_path.clone();
    let app_for_walk = app.clone();
    let walk_progress_cb: scanner::WalkProgressCb = Box::new(move |discovered| {
        events::emit_scan_walk_progress(
            &app_for_walk,
            events::ScanWalkProgress {
                folder: folder_for_walk.clone(),
                discovered,
            },
        );
    });

    let folder_for_progress = folder_path.clone();
    let app_for_progress = app.clone();
    let progress_cb: scanner::ProgressCb = Box::new(move |current, total, current_file| {
        events::emit_scan_progress(
            &app_for_progress,
            events::ScanProgress {
                folder: folder_for_progress.clone(),
                current,
                total,
                current_file: current_file.to_string(),
            },
        );
    });

    let folder_for_dataset = folder_path.clone();
    let app_for_dataset = app.clone();
    let dataset_cb: scanner::DatasetCb = Box::new(move |index, total, dataset, phase| {
        events::emit_dataset_scanned(
            &app_for_dataset,
            events::DatasetScanned {
                folder: folder_for_dataset.clone(),
                index,
                total,
                dataset: dataset.clone(),
                phase,
            },
        );
    });

    let datasets = scanner::scan_folder_with_events(
        &folder_path,
        Some(walk_progress_cb),
        Some(progress_cb),
        Some(dataset_cb),
    )
    .await
        .map_err(|e| {
            audit::record(&folder_path, 0, 0, 0, Some(e.to_string()));
            events::emit_sync_failed(&app, &folder_path, &e.to_string());
            crate::tray::set_state(&app, "online");
            e.to_string()
        })?;

    let column_count: u64 = datasets.iter().map(|d| d.schema.len() as u64).sum();
    let total_bytes: u64 = datasets.iter().map(|d| d.size_bytes).sum();
    let dataset_count = datasets.len() as u64;
    let duration_ms = started.elapsed().as_millis() as u64;

    // 2a. Compute schema diffs against the local cache and emit one
    // schema_changed event per dataset whose schema shifted. Best-effort:
    // diff failures must never block the sync. Requires a known
    // workspace_id (persisted at auth time) — if we don't have one yet
    // (never signed in), we skip the diff silently.
    if let Ok(config_for_diff) = Config::load() {
        if let Some(workspace_id) = config_for_diff.agent.workspace_id.as_deref() {
            if let Ok(cache) = MetadataCache::new() {
                for ds in &datasets {
                    let schema_json = serde_json::to_string(&ds.schema).ok();
                    if let Ok(diff) = cache.compute_schema_diff(
                        workspace_id,
                        &ds.relative_path,
                        schema_json.as_deref(),
                    ) {
                        if !diff.is_empty() {
                            let dataset_name = std::path::Path::new(&ds.relative_path)
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or(&ds.relative_path)
                                .to_string();
                            // Persist first so the id the UI sees is the
                            // same one stored on disk — this lets mark-read
                            // work offline and across app restarts.
                            let origin = config_for_diff.agent.agent_id.clone();
                            let stored = crate::schema_notifications::record(
                                workspace_id,
                                &ds.relative_path,
                                &dataset_name,
                                diff.added() as u64,
                                diff.removed() as u64,
                                diff.type_changed() as u64,
                                diff.clone(),
                                origin.clone(),
                            );
                            if let Ok(stored) = stored {
                                events::emit_schema_changed(
                                    &app,
                                    events::SchemaChanged {
                                        id: stored.id,
                                        received_at: stored.received_at,
                                        workspace_id: workspace_id.to_string(),
                                        dataset_path: ds.relative_path.clone(),
                                        dataset_name,
                                        added: diff.added() as u64,
                                        removed: diff.removed() as u64,
                                        type_changed: diff.type_changed() as u64,
                                        diff,
                                        origin_agent_id: origin,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // 2b. Populate the local cache so the next scan's diff has a baseline.
    // Runs regardless of cloud sync, because the cache is a local-only
    // offline-search surface. Best-effort: cache failures must never
    // block the scan.
    if let Ok(config_for_cache) = Config::load() {
        if let Some(workspace_id) = config_for_cache.agent.workspace_id.as_deref() {
            if let Ok(mut cache) = MetadataCache::new() {
                let now = chrono::Utc::now();
                for ds in &datasets {
                    let schema_json = serde_json::to_string(&ds.schema).ok();
                    let name = std::path::Path::new(&ds.relative_path)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&ds.relative_path)
                        .to_string();
                    // Deterministic local-only id: UNIQUE(workspace_id, path)
                    // is the real key; this id is a stable display handle.
                    let id = format!("{}::{}", workspace_id, ds.relative_path);
                    let cached = crate::metadata_cache::CachedDataset {
                        id,
                        workspace_id: workspace_id.to_string(),
                        name,
                        path: ds.relative_path.clone(),
                        file_format: ds.file_format.clone(),
                        size_bytes: ds.size_bytes as i64,
                        schema_json,
                        tags: Vec::new(),
                        description: None,
                        last_synced: now,
                    };
                    let _ = cache.upsert_dataset(&cached);
                }
            }
        }
    }

    // 3. Persist scan stats on the folder. The LOCAL scan succeeded at
    // this point, regardless of any cloud round-trip — local-first means
    // we commit the result without waiting on the server.
    if let Ok(mut config) = Config::load() {
        config.update_folder_scan_stats(
            &folder_path,
            crate::config::ScanStats {
                datasets: dataset_count,
                columns: column_count,
                errors: 0,
                total_bytes,
                duration_ms,
            },
            chrono::Utc::now().to_rfc3339(),
        );
        let _ = config.save();
    }

    // 4. Audit + local-scan completion events. These fire on every
    // successful scan so FolderDetail reconciles its row list from the
    // cache.
    audit::record(&folder_path, dataset_count, column_count, total_bytes, None);
    events::emit_scan_complete(
        &app,
        events::ScanComplete {
            folder: folder_path.clone(),
            datasets: dataset_count,
            columns: column_count,
            errors: 0,
            total_bytes,
            duration_ms,
        },
    );

    // 5. Best-effort cloud sync. A failure here (network down, 500 from
    // the backend, expired token) does NOT fail the whole command — the
    // local scan is already persisted and the user can still use the UI.
    //
    // The gate `cloud_sync_enabled()` ensures we only attempt sync when
    // the user has explicitly connected via WorkspaceKey AND the first
    // failure of this session hasn't already flipped the process to
    // cloud-offline mode. LocalOnly users, users with stale tokens from
    // an old bootstrap attempt, and sessions that have already hit an
    // error all short-circuit here without another wasted POST.
    let cloud_resp = if cloud_sync_enabled() {
        match (Config::load(), keyring_store::get_token()) {
            (Ok(config), Ok(token)) => {
                match scanner::sync_metadata_to_cloud(&config.cloud.api_url, &token, datasets.clone())
                    .await
                {
                    Ok(r) => {
                        events::emit_sync_completed(&app, &folder_path, dataset_count);
                        r
                    }
                    Err(e) => {
                        // First failure wins: mark the cloud offline so
                        // we don't retry every scan until the user
                        // restarts. If the error looks like an auth
                        // problem, also clear the stale token so the
                        // next launch starts fresh in local-only mode.
                        eprintln!("[rescan] cloud sync failed for {folder_path}: {e}");
                        mark_cloud_offline();
                        let msg = e.to_string();
                        if msg.contains("401") || msg.contains("403") || msg.contains("Unauthorized") {
                            eprintln!("[rescan] auth-looking failure — clearing token");
                            let _ = keyring_store::delete_token();
                        }
                        events::emit_sync_failed(&app, &folder_path, &msg);
                        serde_json::json!({
                            "synced": false,
                            "reason": format!("cloud_sync_failed: {msg}"),
                        })
                    }
                }
            }
            _ => serde_json::json!({ "synced": false, "reason": "config_or_token_unavailable" }),
        }
    } else {
        // LocalOnly (or cloud-offline for the session): don't even
        // attempt to sync. Emit sync_completed so UI surfaces that track
        // sync state flip back to their idle state instead of hanging
        // in "syncing…".
        events::emit_sync_completed(&app, &folder_path, dataset_count);
        let reason = if cloud_offline() { "cloud_offline" } else { "local_only" };
        serde_json::json!({ "synced": false, "reason": reason })
    };

    crate::tray::set_state(&app, "online");
    Ok(cloud_resp)
}

#[tauri::command]
pub async fn sync_metadata(datasets: Vec<DatasetMetadata>) -> Result<Value, String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    let token = keyring_store::get_token().map_err(|e| e.to_string())?;
    scanner::sync_metadata_to_cloud(&config.cloud.api_url, &token, datasets)
        .await
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Token / session
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn has_token() -> Result<bool, String> {
    Ok(keyring_store::has_token())
}

#[tauri::command]
pub async fn get_agent_info() -> Result<Option<AgentToken>, String> {
    if !keyring_store::has_token() {
        return Ok(None);
    }

    let token = keyring_store::get_token().map_err(|e| e.to_string())?;
    let config = Config::load().map_err(|e| e.to_string())?;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("{}/v1/agent/info", config.cloud.api_url))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if response.status().is_success() {
        let agent_token: AgentToken = response.json().await.map_err(|e| e.to_string())?;
        Ok(Some(agent_token))
    } else {
        Ok(None)
    }
}

// ─── Workspace recipes (ROADMAP F11 — cross-machine sync) ───────────────
//
// Read-only fetch of the workspace recipe library — the saved questions
// the user (or any workspace member) created from app-dashboard chat.
// Each recipe is a question they want to re-run; "running" from sery-link
// opens the user's browser to app.sery.ai/chat with the question
// pre-filled (sery-link doesn't currently have its own chat surface).
//
// This is the network-effect half of recipes: save once on machine A,
// see it on machine B. Authenticates with the existing agent token
// against /v1/agent/workspace-recipes (parallel to the user-authed
// endpoint at /v1/workspace-recipes).

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceRecipe {
    pub id: String,
    pub workspace_id: String,
    pub created_by: Option<String>,
    pub name: String,
    pub question: String,
    pub source_message_id: Option<String>,
    pub created_at: String,
    pub last_run_at: Option<String>,
    pub run_count: i64,
}

#[derive(Debug, serde::Deserialize)]
struct WorkspaceRecipeListResponse {
    recipes: Vec<WorkspaceRecipe>,
    #[allow(dead_code)]
    total: i64,
}

#[tauri::command]
pub async fn fetch_workspace_recipes() -> Result<Vec<WorkspaceRecipe>, String> {
    if !keyring_store::has_token() {
        // No workspace token = LocalOnly mode = no recipes to sync.
        // Return an empty list rather than an error so the UI can show
        // a clean "connect to sync recipes" empty state.
        return Ok(Vec::new());
    }
    let token = keyring_store::get_token().map_err(|e| e.to_string())?;
    let config = Config::load().map_err(|e| e.to_string())?;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("{}/v1/agent/workspace-recipes", config.cloud.api_url))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Failed to fetch recipes ({}): {}", status, text));
    }

    let parsed: WorkspaceRecipeListResponse =
        response.json().await.map_err(|e| e.to_string())?;
    Ok(parsed.recipes)
}

#[tauri::command]
pub async fn open_recipe_in_browser(question: String) -> Result<(), String> {
    // The user's browser is the canonical "ask" surface today —
    // sery-link doesn't have a chat UI yet. Use the configured
    // web_url so dev / staging / prod all route correctly.
    let config = Config::load().map_err(|e| e.to_string())?;
    let base = config.cloud.web_url.clone();
    let url = format!(
        "{}/chat?question={}",
        base,
        urlencoding::encode(&question)
    );
    open::that(&url).map_err(|e| format!("Failed to open browser: {}", e))?;
    Ok(())
}

/// Notify the API that a recipe was just run from this machine. Bumps
/// run_count + last_run_at and emits a F14 audit event tagged with this
/// agent. Best-effort: failures are non-fatal because the user's actual
/// goal — opening the question in the browser — already succeeded.
#[tauri::command]
pub async fn mark_recipe_run(recipe_id: String) -> Result<(), String> {
    if !keyring_store::has_token() {
        // No token = LocalOnly mode; nothing to mark on the server.
        return Ok(());
    }
    let token = keyring_store::get_token().map_err(|e| e.to_string())?;
    let config = Config::load().map_err(|e| e.to_string())?;
    let client = reqwest::Client::new();

    let response = client
        .post(format!(
            "{}/v1/agent/workspace-recipes/{}/run",
            config.cloud.api_url, recipe_id
        ))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("Failed to mark recipe run ({}): {}", status, text));
    }
    Ok(())
}

#[tauri::command]
pub async fn logout() -> Result<(), String> {
    // Close websocket + watcher before clearing credentials so we don't leave
    // a connected client trying to authenticate with a dead token.
    let mut ws_guard = WS_CLIENT.write().await;
    *ws_guard = None;
    drop(ws_guard);

    let mut watcher_guard = WATCHER.write().await;
    *watcher_guard = None;
    drop(watcher_guard);

    keyring_store::delete_token().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_current_auth_mode() -> Result<AuthMode, String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    Ok(auth::get_auth_mode(&config))
}

#[tauri::command]
pub async fn check_feature_available(feature: String) -> Result<bool, String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    let mode = auth::get_auth_mode(&config);
    Ok(auth::feature_available(&mode, &feature))
}

#[tauri::command]
pub async fn set_auth_mode(mode: AuthMode) -> Result<(), String> {
    let mut config = Config::load().map_err(|e| e.to_string())?;
    config.app.selected_auth_mode = Some(mode);
    config.save().map_err(|e| e.to_string())
}

/// Toggle Local-Only network mode. When enabled, disconnects the WebSocket
/// tunnel and pins `selected_auth_mode` to `LocalOnly`, so cloud-dependent
/// feature gates (`ai_queries`, `cloud_sync`, `team_sharing`) all return
/// false until the user toggles it back. The keyring token is **left
/// intact** — toggling back restores whatever auth mode the keyring/env-vars
/// imply (WorkspaceKey or BYOK), and the WebSocket reconnects.
///
/// This is the implementation of ROADMAP F6: the "we're a network, not a
/// store" promise is structural only if the user can verify "if I turn the
/// network off, the app still does its core job." Local features (column
/// search, profiles, recipes, the watcher) keep running regardless.
#[tauri::command]
pub async fn set_local_only_mode<R: Runtime>(
    enabled: bool,
    app: AppHandle<R>,
) -> Result<(), String> {
    let mut config = Config::load().map_err(|e| e.to_string())?;

    if enabled {
        // 1. Pin auth mode so feature gates flip cloud features off.
        config.app.selected_auth_mode = Some(AuthMode::LocalOnly);
        config.save().map_err(|e| e.to_string())?;

        // 2. Drop the live WebSocket. The connection closes; auto-reconnect
        //    won't happen because the global is now None.
        let mut ws_guard = WS_CLIENT.write().await;
        *ws_guard = None;
        drop(ws_guard);

        // 3. Note: the keyring token and watcher are deliberately NOT
        //    touched. Local file watching keeps working; the user's
        //    workspace credentials survive the disconnect so re-enabling
        //    is one click rather than re-pairing.
    } else {
        // 1. Clear the pinned mode so auth detection auto-picks the right
        //    state from the keyring / env vars (WorkspaceKey, BYOK, or
        //    LocalOnly fallback if no creds exist).
        config.app.selected_auth_mode = None;
        config.save().map_err(|e| e.to_string())?;

        // 2. If we have a workspace token, restart the WebSocket. If not
        //    (the user was in pure local mode anyway), this is a no-op.
        if keyring_store::has_token() {
            if let Ok(token) = keyring_store::get_token() {
                let new_config = Config::load().map_err(|e| e.to_string())?;
                let client = WebSocketClient::new(new_config);
                client.start_with_app(token, app).await;

                let mut ws_guard = WS_CLIENT.write().await;
                *ws_guard = Some(client);
            }
        }
    }

    Ok(())
}

/// Lightweight read of the user's intent (vs `get_current_auth_mode`,
/// which returns the *resolved* mode). Returns `true` only if the user
/// has explicitly pinned `LocalOnly` via `set_local_only_mode(true)`.
/// Used by the Settings UI to reflect the toggle state without overlap
/// with users who just happen to have no workspace credentials.
#[tauri::command]
pub async fn is_local_only_mode_enabled() -> Result<bool, String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    Ok(matches!(
        config.app.selected_auth_mode,
        Some(AuthMode::LocalOnly)
    ))
}

// ---------------------------------------------------------------------------
// File watcher
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn start_file_watcher() -> Result<(), String> {
    // Idempotent install. Holding the write lock across the full body
    // serialises concurrent callers (React 19 strict mode fires
    // useEffect twice in dev, which was creating overlapping notify /
    // FSEvents streams on the same paths — the old handle's Drop then
    // threw a foreign exception on macOS that aborted the process).
    //
    // Semantics:
    //   * If a watcher is already running, return Ok without touching it.
    //     Callers who need to apply a new folder config should call
    //     `restart_file_watcher` explicitly.
    //   * If watched_folders is empty or auto-sync is off, leave the
    //     (absent) watcher alone and return Ok.
    let mut guard = WATCHER.write().await;
    if guard.is_some() {
        return Ok(());
    }

    let config = Config::load().map_err(|e| e.to_string())?;
    if config.watched_folders.is_empty() || !config.sync.auto_sync_on_change {
        return Ok(());
    }

    // Only LOCAL folders go through notify — URLs are Phase-A remote
    // sources and don't live on the filesystem. notify::Watcher::watch
    // would error on a non-filesystem path.
    let folder_paths: Vec<String> = config
        .watched_folders
        .iter()
        .filter(|f| !crate::url::is_remote_url(&f.path))
        .map(|f| f.path.clone())
        .collect();
    if folder_paths.is_empty() {
        return Ok(());
    }

    let handle = watcher::start_watcher(folder_paths)
        .await
        .map_err(|e| e.to_string())?;

    *guard = Some(handle);
    Ok(())
}

#[tauri::command]
pub async fn stop_file_watcher() -> Result<(), String> {
    let mut guard = WATCHER.write().await;
    *guard = None;
    Ok(())
}

#[tauri::command]
pub async fn restart_file_watcher() -> Result<(), String> {
    // Atomic drop-then-install under a single write lock — a previous
    // split-phase stop+start let a concurrent caller slip in between,
    // producing overlapping FSEvents streams that tripped the
    // foreign-exception crash on macOS.
    let mut guard = WATCHER.write().await;
    *guard = None;

    let config = Config::load().map_err(|e| e.to_string())?;
    if config.watched_folders.is_empty() || !config.sync.auto_sync_on_change {
        return Ok(());
    }

    // Only LOCAL folders go through notify — URLs are Phase-A remote
    // sources and don't live on the filesystem. notify::Watcher::watch
    // would error on a non-filesystem path.
    let folder_paths: Vec<String> = config
        .watched_folders
        .iter()
        .filter(|f| !crate::url::is_remote_url(&f.path))
        .map(|f| f.path.clone())
        .collect();
    if folder_paths.is_empty() {
        return Ok(());
    }

    let handle = watcher::start_watcher(folder_paths)
        .await
        .map_err(|e| e.to_string())?;

    *guard = Some(handle);
    Ok(())
}

// ---------------------------------------------------------------------------
// Query history + stats
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_query_history(limit: Option<usize>) -> Result<Vec<QueryHistoryEntry>, String> {
    history::load_history(limit.unwrap_or(100)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_query_history() -> Result<(), String> {
    history::clear_history().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_stats() -> Result<Stats, String> {
    stats::load().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Privacy / audit
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_sync_audit() -> Result<Vec<audit::AuditEntry>, String> {
    audit::latest_by_folder().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_sync_audit() -> Result<(), String> {
    audit::clear().map_err(|e| e.to_string())
}

/// Delete metadata for this agent from the cloud. Keeps local files untouched.
///
/// The backend exposes per-dataset DELETE only (`/v1/agent/datasets/{id}`),
/// scoped server-side to the bearer token's agent_id, so this command lists
/// then iterates. "Clear all" is rarely clicked, so an N+1 call pattern is
/// acceptable and avoids adding a bulk endpoint.
#[tauri::command]
pub async fn clear_cloud_metadata() -> Result<Value, String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    let token = keyring_store::get_token().map_err(|e| e.to_string())?;
    let client = reqwest::Client::new();
    let base = &config.cloud.api_url;

    // 1. List every dataset this agent has synced.
    let list_resp = client
        .get(format!("{}/v1/agent/datasets", base))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !list_resp.status().is_success() {
        let body = list_resp.text().await.unwrap_or_default();
        return Err(format!("List datasets failed: {}", body));
    }

    let payload: Value = list_resp.json().await.map_err(|e| e.to_string())?;
    let datasets = payload["datasets"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let total = datasets.len();

    // 2. Delete each by id. We tolerate individual failures and report the
    //    deleted count so the UI can surface partial success.
    let mut deleted = 0usize;
    let mut last_error: Option<String> = None;
    for ds in datasets {
        let Some(id) = ds["id"].as_str() else { continue };
        match client
            .delete(format!("{}/v1/agent/datasets/{}", base, id))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => deleted += 1,
            Ok(r) => {
                last_error = Some(format!(
                    "delete {} returned {}",
                    id,
                    r.status().as_u16()
                ));
            }
            Err(e) => last_error = Some(e.to_string()),
        }
    }

    // If we couldn't delete anything but there was something to delete, bubble
    // the last error up so the user sees a real failure in the toast.
    if deleted == 0 && total > 0 {
        return Err(last_error.unwrap_or_else(|| "No datasets were deleted".into()));
    }

    // Wipe the local audit log so the Privacy tab reflects the new state.
    let _ = audit::clear();
    Ok(serde_json::json!({
        "cleared": true,
        "deleted": deleted,
        "total": total,
    }))
}

// ---------------------------------------------------------------------------
// WebSocket tunnel
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn start_websocket_tunnel<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    let token = keyring_store::get_token().map_err(|e| e.to_string())?;

    let client = WebSocketClient::new(config);
    client.start_with_app(token, app).await;

    let mut ws_guard = WS_CLIENT.write().await;
    *ws_guard = Some(client);
    Ok(())
}

#[tauri::command]
pub async fn get_websocket_status() -> Result<String, String> {
    let ws_guard = WS_CLIENT.read().await;
    if let Some(client) = ws_guard.as_ref() {
        Ok(format!("{:?}", client.get_status().await))
    } else {
        Ok("Offline".to_string())
    }
}

// ---------------------------------------------------------------------------
// Diagnostics bundle
// ---------------------------------------------------------------------------

/// Zip up a redacted diagnostic bundle: config (with token fields removed),
/// recent history, stats, audit log, and the agent version. Dropped on the
/// desktop so users can attach it to support tickets.
#[tauri::command]
pub async fn export_diagnostic_bundle() -> Result<String, String> {
    tokio::task::spawn_blocking(build_diagnostic_bundle)
        .await
        .map_err(|e| format!("diagnostic task failed: {}", e))?
}

fn build_diagnostic_bundle() -> Result<String, String> {
    use zip::write::FileOptions;

    let desktop = dirs::desktop_dir()
        .or_else(dirs::home_dir)
        .ok_or_else(|| "no desktop dir".to_string())?;
    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let out_path = desktop.join(format!("seryai-agent-diagnostic-{}.zip", timestamp));

    let file = fs::File::create(&out_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    // --- config.json (redacted) ---
    if let Ok(config) = Config::load() {
        let redacted = serde_json::to_string_pretty(&config).unwrap_or_default();
        zip.start_file("config.json", opts).map_err(|e| e.to_string())?;
        zip.write_all(redacted.as_bytes()).map_err(|e| e.to_string())?;
    }

    // --- stats.json ---
    if let Ok(s) = stats::load() {
        let body = serde_json::to_string_pretty(&s).unwrap_or_default();
        zip.start_file("stats.json", opts).map_err(|e| e.to_string())?;
        zip.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    }

    // --- query_history.jsonl (last 500 entries) ---
    if let Ok(entries) = history::load_history(500) {
        let body = serde_json::to_string_pretty(&entries).unwrap_or_default();
        zip.start_file("query_history.json", opts).map_err(|e| e.to_string())?;
        zip.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    }

    // --- sync_audit.json ---
    if let Ok(entries) = audit::load(usize::MAX) {
        let body = serde_json::to_string_pretty(&entries).unwrap_or_default();
        zip.start_file("sync_audit.json", opts).map_err(|e| e.to_string())?;
        zip.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    }

    // --- meta.json ---
    let meta = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    });
    zip.start_file("meta.json", opts).map_err(|e| e.to_string())?;
    zip.write_all(serde_json::to_string_pretty(&meta).unwrap_or_default().as_bytes())
        .map_err(|e| e.to_string())?;

    zip.finish().map_err(|e| e.to_string())?;
    Ok(out_path.to_string_lossy().to_string())
}

// ---------------------------------------------------------------------------
// Deep links + window management
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn open_in_sery_cloud() -> Result<(), String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    let url = if let Some(agent_id) = &config.agent.agent_id {
        format!("{}/agents/{}", config.cloud.web_url, agent_id)
    } else {
        config.cloud.web_url.clone()
    };
    open::that(url).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn complete_first_run() -> Result<(), String> {
    let mut config = Config::load().map_err(|e| e.to_string())?;
    config.app.first_run_completed = true;
    config.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn reveal_in_finder(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    // Open the containing directory so the file is highlighted (OS-native
    // behaviour). If the path is already a directory, just open it.
    let target = if p.is_file() {
        p.parent().map(|x| x.to_path_buf()).unwrap_or(p)
    } else {
        p
    };
    open::that(target).map_err(|e| e.to_string())
}

/// Return the absolute on-disk path of the outbound audit log + reveal
/// it in the OS file manager. The path is also returned so the Privacy
/// view can show it next to the Reveal button — users see exactly
/// where the file is, can `tail -f` it from a terminal, or paste the
/// path into a privacy-conscious customer's audit request.
#[tauri::command]
pub async fn reveal_audit_file_in_finder() -> Result<String, String> {
    let p = audit::audit_file_path().map_err(|e| e.to_string())?;
    if let Some(parent) = p.parent() {
        // Make sure the directory exists so the OS doesn't error on a
        // missing path. If the file itself doesn't exist yet (no syncs
        // and no BYOK calls), opening the parent still gives the user a
        // useful destination — they see ~/.seryai/ with whatever else
        // is there.
        let _ = std::fs::create_dir_all(parent);
    }
    let target: PathBuf = if p.is_file() {
        p.parent().map(|x| x.to_path_buf()).unwrap_or(p.clone())
    } else {
        p.parent()
            .map(|x| x.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("/"))
    };
    open::that(&target).map_err(|e| e.to_string())?;
    Ok(p.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn show_main_window<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    #[cfg(target_os = "macos")]
    {
        use tauri::ActivationPolicy;
        let _ = app.set_activation_policy(ActivationPolicy::Regular);
    }
    Ok(())
}

#[tauri::command]
pub async fn set_launch_at_login<R: Runtime>(app: AppHandle<R>, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;

    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|e| e.to_string())?;
    } else {
        manager.disable().map_err(|e| e.to_string())?;
    }

    // Persist in config so next launch reflects the choice.
    let mut config = Config::load().map_err(|e| e.to_string())?;
    config.app.launch_at_login = enabled;
    config.save().map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Local metadata cache (offline dataset search)
// ---------------------------------------------------------------------------
// NOTE: We create a new MetadataCache instance for each command because DuckDB's
// Connection uses RefCell internally and cannot be safely shared across threads.
// This is fine - DuckDB is file-based and handles concurrent access natively.

#[tauri::command]
pub async fn search_cached_datasets(
    workspace_id: String,
    query: String,
    limit: usize,
) -> Result<Vec<SearchResult>, String> {
    let cache = MetadataCache::new().map_err(|e| e.to_string())?;
    cache.search(&workspace_id, &query, limit).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_all_cached_datasets(workspace_id: String) -> Result<Vec<CachedDataset>, String> {
    let cache = MetadataCache::new().map_err(|e| e.to_string())?;
    cache.get_all(&workspace_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_cached_dataset(id: String) -> Result<Option<CachedDataset>, String> {
    let cache = MetadataCache::new().map_err(|e| e.to_string())?;
    cache.get_by_id(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn upsert_cached_dataset(dataset: CachedDataset) -> Result<(), String> {
    let mut cache = MetadataCache::new().map_err(|e| e.to_string())?;
    cache.upsert_dataset(&dataset).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn upsert_cached_datasets(datasets: Vec<CachedDataset>) -> Result<(), String> {
    let mut cache = MetadataCache::new().map_err(|e| e.to_string())?;
    cache.upsert_many(&datasets).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_cached_workspace(workspace_id: String) -> Result<(), String> {
    let mut cache = MetadataCache::new().map_err(|e| e.to_string())?;
    cache.clear_workspace(&workspace_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_cache_stats() -> Result<CacheStats, String> {
    let cache = MetadataCache::new().map_err(|e| e.to_string())?;
    cache.get_stats().map_err(|e| e.to_string())
}

/// Compute what changed between the currently-cached schema for a dataset
/// and a newly-scanned one. Returns an empty diff when the dataset isn't
/// yet cached (first-sync is not a change). Callers typically invoke this
/// *before* upserting so they can capture the notification.
#[tauri::command]
pub async fn compute_schema_diff(
    workspace_id: String,
    path: String,
    new_schema_json: Option<String>,
) -> Result<crate::schema_diff::SchemaDiff, String> {
    let cache = MetadataCache::new().map_err(|e| e.to_string())?;
    cache
        .compute_schema_diff(&workspace_id, &path, new_schema_json.as_deref())
        .map_err(|e| e.to_string())
}

// ─── Schema-change notifications (persistent) ─────────────────────────────

#[tauri::command]
pub async fn get_schema_notifications(
    limit: Option<usize>,
) -> Result<Vec<crate::schema_notifications::StoredNotification>, String> {
    crate::schema_notifications::load(limit.unwrap_or(200))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mark_schema_notification_read(id: String) -> Result<(), String> {
    crate::schema_notifications::mark_read(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mark_all_schema_notifications_read() -> Result<(), String> {
    crate::schema_notifications::mark_all_read().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_schema_notifications() -> Result<(), String> {
    crate::schema_notifications::clear().map_err(|e| e.to_string())
}

// ─── Dataset Relationships ────────────────────────────────────────────────

#[tauri::command]
pub async fn detect_dataset_relationships(
    workspace_id: String,
) -> Result<Vec<crate::relationship_detector::DatasetRelationship>, String> {
    crate::relationship_detector::detect_relationships(&workspace_id)
        .map_err(|e| e.to_string())
}

// ─── Export/Import ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn export_configuration(workspace_id: String) -> Result<String, String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    crate::export_import::export_to_json(&workspace_id, &config)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_configuration(
    json: String,
    workspace_id: String,
    strategy: crate::export_import::ImportStrategy,
) -> Result<crate::export_import::ImportResult, String> {
    let mut config = Config::load().map_err(|e| e.to_string())?;

    let (new_folders, result) = crate::export_import::import_from_json(
        &json,
        &workspace_id,
        &config.watched_folders,
        strategy,
    )
    .map_err(|e| e.to_string())?;

    // Save the updated config
    config.watched_folders = new_folders;
    config.save().map_err(|e| e.to_string())?;

    Ok(result)
}

#[tauri::command]
pub async fn validate_import_file(json: String) -> Result<crate::export_import::ExportData, String> {
    crate::export_import::validate_export(&json).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn read_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read file {}: {}", path, e))
}

// SQL Recipe Executor was removed in the v0.5.0 pivot. The module,
// example recipe JSONs, and frontend surfaces have all been deleted.
#[cfg(test)]
mod search_tests {
    use super::{rank_matches, SearchMatchReason};
    use crate::scan_cache::CachedEntry;
    use crate::scanner::{ColumnSchema, DatasetMetadata};

    fn tabular(folder: &str, rel: &str, cols: &[(&str, &str)]) -> CachedEntry {
        CachedEntry {
            folder_path: folder.to_string(),
            relative_path: rel.to_string(),
            metadata: DatasetMetadata {
                relative_path: rel.to_string(),
                file_format: "csv".to_string(),
                size_bytes: 1024,
                row_count_estimate: Some(100),
                schema: cols
                    .iter()
                    .map(|(n, t)| ColumnSchema {
                        name: n.to_string(),
                        col_type: t.to_string(),
                        nullable: true,
                    })
                    .collect(),
                last_modified: "2026-01-01T00:00:00Z".to_string(),
                document_markdown: None,
                sample_rows: None,
                samples_redacted: false,
            },
        }
    }

    fn doc(folder: &str, rel: &str, markdown: &str) -> CachedEntry {
        CachedEntry {
            folder_path: folder.to_string(),
            relative_path: rel.to_string(),
            metadata: DatasetMetadata {
                relative_path: rel.to_string(),
                file_format: "docx".to_string(),
                size_bytes: 2048,
                row_count_estimate: None,
                schema: Vec::new(),
                last_modified: "2026-01-01T00:00:00Z".to_string(),
                document_markdown: Some(markdown.to_string()),
                sample_rows: None,
                samples_redacted: false,
            },
        }
    }

    #[test]
    fn exact_filename_ranks_highest() {
        // Three files — the one whose basename IS the query should beat
        // a file that merely contains the query substring.
        let entries = vec![
            tabular("/a", "orders_2024.csv", &[]),
            tabular("/a", "orders.csv", &[]),
            tabular("/a", "my_orders_final.csv", &[]),
        ];
        let matches = rank_matches(&entries, "orders.csv");
        assert!(matches.len() >= 1);
        assert_eq!(matches[0].relative_path, "orders.csv");
        assert!(matches[0].score >= 120);
    }

    #[test]
    fn column_match_finds_files_without_filename_hint() {
        // This is the killer feature — user types "price" and we surface
        // any tabular file that has a `price` column, even if the file
        // is named something unrelated.
        let entries = vec![
            tabular("/a", "random_name.csv", &[("id", "INT"), ("price", "DOUBLE")]),
            tabular("/a", "other.csv", &[("id", "INT"), ("amount", "DOUBLE")]),
        ];
        let matches = rank_matches(&entries, "price");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].relative_path, "random_name.csv");
        // And the reason must include the column match so the UI can
        // show "column: price" as the explanation.
        assert!(matches[0].match_reasons.iter().any(|r| matches!(
            r,
            SearchMatchReason::Column { name, .. } if name == "price"
        )));
    }

    #[test]
    fn filename_and_column_combine_score() {
        // A file that matches on BOTH filename AND column should rank
        // above a file that matches only one.
        let only_column =
            tabular("/a", "random.csv", &[("price", "DOUBLE")]);
        let both = tabular("/a", "price_list.csv", &[("price", "DOUBLE")]);
        let entries = vec![only_column.clone(), both.clone()];
        let matches = rank_matches(&entries, "price");
        assert_eq!(matches[0].relative_path, "price_list.csv");
        assert!(matches[0].score > matches[1].score);
    }

    #[test]
    fn content_match_returns_snippet_with_context() {
        let entries = vec![doc(
            "/a",
            "resume.docx",
            "Experienced engineer with a focus on Anthropic APIs and distributed systems.",
        )];
        let matches = rank_matches(&entries, "Anthropic");
        assert_eq!(matches.len(), 1);
        let snippet = matches[0]
            .match_reasons
            .iter()
            .find_map(|r| match r {
                SearchMatchReason::Content { snippet } => Some(snippet.clone()),
                _ => None,
            })
            .expect("content reason should be present");
        // Snippet should include the query word and some surrounding text.
        assert!(snippet.to_lowercase().contains("anthropic"));
        assert!(snippet.len() > "Anthropic".len());
    }

    #[test]
    fn case_insensitive_match() {
        let entries = vec![tabular("/a", "Orders.CSV", &[("Price", "DOUBLE")])];
        // Upper case query, lower case in entry, and vice-versa.
        let lower = rank_matches(&entries, "price");
        let upper = rank_matches(&entries, "ORDERS");
        assert_eq!(lower.len(), 1);
        assert_eq!(upper.len(), 1);
    }

    #[test]
    fn empty_entries_or_no_match_returns_empty() {
        assert!(rank_matches(&[], "price").is_empty());
        let entries = vec![tabular("/a", "other.csv", &[("id", "INT")])];
        assert!(rank_matches(&entries, "nonexistent").is_empty());
    }

    #[test]
    fn multibyte_snippet_does_not_panic() {
        // Regression guard: str slicing at a non-boundary panics. Snippets
        // around multi-byte characters must snap to valid boundaries.
        let entries = vec![doc("/a", "doc.docx", "résumé for my application — focus on engineering")];
        let matches = rank_matches(&entries, "application");
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn only_one_column_reason_per_file() {
        // If many columns match, we should only surface the strongest —
        // otherwise the UI drowns in "column: price, column: price_usd,
        // column: price_cents" noise.
        let entries = vec![tabular(
            "/a",
            "prices.csv",
            &[("price", "DOUBLE"), ("price_usd", "DOUBLE"), ("price_cents", "INT")],
        )];
        let matches = rank_matches(&entries, "price");
        assert_eq!(matches.len(), 1);
        let column_reason_count = matches[0]
            .match_reasons
            .iter()
            .filter(|r| matches!(r, SearchMatchReason::Column { .. }))
            .count();
        assert_eq!(column_reason_count, 1, "expected exactly one column reason");
    }
}

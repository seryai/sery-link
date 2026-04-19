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
use crate::fleet::{self, FleetResponse};
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

static PLUGIN_RUNTIME: once_cell::sync::Lazy<Arc<RwLock<crate::plugin_runtime::PluginRuntime>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(crate::plugin_runtime::PluginRuntime::new())));

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
// Fleet view (SPEC_FIRST_INSTALL.md §Screen 6)
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_fleet() -> Result<FleetResponse, String> {
    let config = Config::load().map_err(|e| e.to_string())?;
    fleet::list_fleet(&config.cloud.api_url)
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

#[tauri::command]
pub async fn remove_watched_folder(path: String) -> Result<(), String> {
    let mut config = Config::load().map_err(|e| e.to_string())?;
    config.remove_watched_folder(&path);
    config.save().map_err(|e| e.to_string())?;
    // Drop any cached scan results for this folder — otherwise re-adding
    // the same path later would surface rows for files that may have
    // moved or been deleted in the meantime. Goes through the shared
    // cache singleton.
    let _ = crate::scan_cache::with_cache(|c| c.invalidate_folder(&path));
    let _ = restart_file_watcher().await;
    Ok(())
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
    let dataset_cb: scanner::DatasetCb = Box::new(move |index, total, dataset| {
        events::emit_dataset_scanned(
            &app_for_dataset,
            events::DatasetScanned {
                folder: folder_for_dataset.clone(),
                index,
                total,
                dataset: dataset.clone(),
            },
        );
    });

    let datasets =
        scanner::scan_folder_with_events(&folder_path, Some(progress_cb), Some(dataset_cb))
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

    let folder_paths: Vec<String> = config
        .watched_folders
        .iter()
        .map(|f| f.path.clone())
        .collect();

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

    let folder_paths: Vec<String> = config
        .watched_folders
        .iter()
        .map(|f| f.path.clone())
        .collect();

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

// ─── Plugin Management ─────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_plugins() -> Result<Vec<(crate::plugin::PluginManifest, bool)>, String> {
    let manager = crate::plugin::PluginManager::new().map_err(|e| e.to_string())?;
    manager.list_plugins().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn enable_plugin(plugin_id: String) -> Result<(), String> {
    let mut manager = crate::plugin::PluginManager::new().map_err(|e| e.to_string())?;
    manager.enable_plugin(&plugin_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn disable_plugin(plugin_id: String) -> Result<(), String> {
    let mut manager = crate::plugin::PluginManager::new().map_err(|e| e.to_string())?;
    manager.disable_plugin(&plugin_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn uninstall_plugin(plugin_id: String) -> Result<(), String> {
    let mut manager = crate::plugin::PluginManager::new().map_err(|e| e.to_string())?;
    manager.uninstall_plugin(&plugin_id).map_err(|e| e.to_string())
}

// ─── Plugin Runtime (WebAssembly execution) ─────────────────────────────────

#[tauri::command]
pub async fn load_plugin_into_runtime(plugin_id: String) -> Result<(), String> {
    let manager = crate::plugin::PluginManager::new().map_err(|e| e.to_string())?;
    let plugins = manager.list_plugins().map_err(|e| e.to_string())?;

    let (manifest, enabled) = plugins
        .iter()
        .find(|(m, _)| m.id == plugin_id)
        .ok_or_else(|| format!("Plugin not found: {}", plugin_id))?;

    if !enabled {
        return Err(format!("Plugin is disabled: {}", plugin_id));
    }

    let plugin_dir = dirs::home_dir()
        .ok_or_else(|| "Could not find home directory".to_string())?
        .join(".sery")
        .join("plugins")
        .join(&plugin_id);

    let mut runtime = PLUGIN_RUNTIME.write().await;
    runtime.load_plugin(&plugin_dir, manifest.clone())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn unload_plugin_from_runtime(plugin_id: String) -> Result<(), String> {
    let mut runtime = PLUGIN_RUNTIME.write().await;
    runtime.unload_plugin(&plugin_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn is_plugin_loaded(plugin_id: String) -> Result<bool, String> {
    let runtime = PLUGIN_RUNTIME.read().await;
    Ok(runtime.is_loaded(&plugin_id))
}

#[tauri::command]
pub async fn get_loaded_plugins() -> Result<Vec<String>, String> {
    let runtime = PLUGIN_RUNTIME.read().await;
    Ok(runtime.loaded_plugins())
}

#[tauri::command]
pub async fn execute_plugin_with_file(
    plugin_id: String,
    file_path: String,
    function_name: String,
) -> Result<String, String> {
    let mut runtime = PLUGIN_RUNTIME.write().await;

    // Read the file with sandboxing
    let file_bytes = runtime
        .read_file_for_plugin(&plugin_id, &file_path)
        .map_err(|e| e.to_string())?;

    // Convert bytes to string (for text files like CSV)
    let file_str = String::from_utf8_lossy(&file_bytes);

    // Write file contents to plugin memory
    let data_ptr = runtime
        .write_string_to_memory(&plugin_id, &file_str)
        .map_err(|e| e.to_string())?;

    // Call the plugin function with pointer and length
    let data_len = file_str.len() as i32;
    let result = runtime
        .execute(&plugin_id, &function_name, vec![
            wasmer::Value::I32(data_ptr),
            wasmer::Value::I32(data_len),
        ])
        .map_err(|e| e.to_string())?;

    // Extract the result (assuming i32 return)
    if let Some(wasmer::Value::I32(value)) = result.first() {
        Ok(format!(
            "{{\"plugin\":\"{}\",\"file\":\"{}\",\"size\":{},\"function\":\"{}\",\"result\":{}}}",
            plugin_id,
            file_path,
            file_bytes.len(),
            function_name,
            value
        ))
    } else {
        Err("Plugin function did not return i32".to_string())
    }
}


// ─── Plugin Marketplace ─────────────────────────────────────────────────────

use crate::plugin_marketplace::{MarketplaceEntry, MarketplaceRegistry, PluginInstaller};
use once_cell::sync::Lazy;

static MARKETPLACE: Lazy<Arc<RwLock<Option<MarketplaceRegistry>>>> = 
    Lazy::new(|| Arc::new(RwLock::new(None)));

#[tauri::command]
pub async fn load_marketplace() -> Result<MarketplaceRegistry, String> {
    let marketplace_path = dirs::home_dir()
        .ok_or_else(|| "Could not find home directory".to_string())?
        .join(".sery")
        .join("marketplace.json");

    if marketplace_path.exists() {
        let registry = MarketplaceRegistry::load(&marketplace_path)
            .map_err(|e| e.to_string())?;
        *MARKETPLACE.write().await = Some(registry.clone());
        Ok(registry)
    } else {
        // Return empty marketplace if file doesn't exist
        let registry = MarketplaceRegistry::default();
        Ok(registry)
    }
}

#[tauri::command]
pub async fn search_marketplace(query: String) -> Result<Vec<MarketplaceEntry>, String> {
    let marketplace = MARKETPLACE.read().await;
    
    if let Some(ref registry) = *marketplace {
        let results = registry.search(&query);
        Ok(results.into_iter().cloned().collect())
    } else {
        Err("Marketplace not loaded".to_string())
    }
}

#[tauri::command]
pub async fn get_featured_plugins() -> Result<Vec<MarketplaceEntry>, String> {
    let marketplace = MARKETPLACE.read().await;
    
    if let Some(ref registry) = *marketplace {
        let results = registry.featured();
        Ok(results.into_iter().cloned().collect())
    } else {
        Err("Marketplace not loaded".to_string())
    }
}

#[tauri::command]
pub async fn get_popular_plugins(limit: usize) -> Result<Vec<MarketplaceEntry>, String> {
    let marketplace = MARKETPLACE.read().await;
    
    if let Some(ref registry) = *marketplace {
        let results = registry.popular(limit);
        Ok(results.into_iter().cloned().collect())
    } else {
        Err("Marketplace not loaded".to_string())
    }
}

#[tauri::command]
pub async fn get_marketplace_plugin(plugin_id: String) -> Result<Option<MarketplaceEntry>, String> {
    let marketplace = MARKETPLACE.read().await;
    
    if let Some(ref registry) = *marketplace {
        Ok(registry.get(&plugin_id).cloned())
    } else {
        Err("Marketplace not loaded".to_string())
    }
}

#[tauri::command]
pub async fn install_marketplace_plugin(plugin_id: String) -> Result<(), String> {
    let marketplace = MARKETPLACE.read().await;
    let entry = if let Some(ref registry) = *marketplace {
        registry.get(&plugin_id)
            .ok_or_else(|| format!("Plugin not found in marketplace: {}", plugin_id))?
            .clone()
    } else {
        return Err("Marketplace not loaded".to_string());
    };

    let plugins_dir = dirs::home_dir()
        .ok_or_else(|| "Could not find home directory".to_string())?
        .join(".sery")
        .join("plugins");

    let installer = PluginInstaller::new(plugins_dir);
    installer.install(&entry).await
        .map_err(|e| e.to_string())
}

// ─── SQL Recipe Executor ───────────────────────────────────────────────────

use crate::recipe_executor::{Recipe, RecipeExecutor, RecipeTier};
use std::collections::HashMap;

static RECIPE_EXECUTOR: Lazy<Arc<RwLock<RecipeExecutor>>> =
    Lazy::new(|| Arc::new(RwLock::new(RecipeExecutor::new())));

#[tauri::command]
pub async fn load_recipes_from_dir(dir_path: String) -> Result<Vec<Recipe>, String> {
    let path = PathBuf::from(dir_path);
    let mut executor = RECIPE_EXECUTOR.write().await;

    executor.load_recipes_from_dir(&path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn load_recipe(file_path: String) -> Result<Recipe, String> {
    let path = PathBuf::from(file_path);
    let mut executor = RECIPE_EXECUTOR.write().await;

    executor.load_recipe(&path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_recipes(query: String) -> Result<Vec<Recipe>, String> {
    let executor = RECIPE_EXECUTOR.read().await;
    let results = executor.search_recipes(&query);
    Ok(results.into_iter().cloned().collect())
}

#[tauri::command]
pub async fn get_recipe(recipe_id: String) -> Result<Option<Recipe>, String> {
    let executor = RECIPE_EXECUTOR.read().await;
    Ok(executor.get_recipe(&recipe_id).cloned())
}

#[tauri::command]
pub async fn list_recipes() -> Result<Vec<Recipe>, String> {
    let executor = RECIPE_EXECUTOR.read().await;
    Ok(executor.list_recipes().into_iter().cloned().collect())
}

#[tauri::command]
pub async fn filter_recipes_by_data_source(data_source: String) -> Result<Vec<Recipe>, String> {
    let executor = RECIPE_EXECUTOR.read().await;
    Ok(executor.filter_by_data_source(&data_source).into_iter().cloned().collect())
}

#[tauri::command]
pub async fn render_recipe_sql(
    recipe_id: String,
    params: HashMap<String, serde_json::Value>
) -> Result<String, String> {
    let executor = RECIPE_EXECUTOR.read().await;

    let recipe = executor.get_recipe(&recipe_id)
        .ok_or_else(|| format!("Recipe not found: {}", recipe_id))?;

    executor.render_sql(recipe, &params)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn validate_recipe_tables(
    recipe_id: String,
    available_tables: HashMap<String, Vec<String>>
) -> Result<(), String> {
    let executor = RECIPE_EXECUTOR.read().await;

    let recipe = executor.get_recipe(&recipe_id)
        .ok_or_else(|| format!("Recipe not found: {}", recipe_id))?;

    executor.validate_tables(recipe, &available_tables)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn execute_recipe(
    recipe_id: String,
    params: HashMap<String, serde_json::Value>,
) -> Result<String, String> {
    let executor = RECIPE_EXECUTOR.read().await;
    let recipe = executor.get_recipe(&recipe_id)
        .ok_or_else(|| format!("Recipe not found: {}", recipe_id))?;

    // Tier authorization check
    let config = Config::load().map_err(|e| e.to_string())?;
    let auth_mode = auth::get_auth_mode(&config);

    let allowed = match (&recipe.tier, &auth_mode) {
        (RecipeTier::Free, _) => true,
        (RecipeTier::Pro, AuthMode::LocalOnly) => false,
        (RecipeTier::Pro, _) => true,
        (RecipeTier::Team, AuthMode::WorkspaceKey { .. }) => true,
        (RecipeTier::Team, _) => false,
    };

    if !allowed {
        let tier_name = match recipe.tier {
            RecipeTier::Free => "FREE",
            RecipeTier::Pro => "PRO",
            RecipeTier::Team => "TEAM",
        };
        return Err(format!(
            "Recipe '{}' requires {} tier. Connect your workspace or use your own API key to access.",
            recipe.name,
            tier_name
        ));
    }

    // Render the SQL with parameters
    executor.render_sql(recipe, &params)
        .map_err(|e| e.to_string())
}

//! Typed event emission.
//!
//! The agent pushes a handful of strongly-typed events to the frontend so
//! the UI can stay reactive without polling. All payloads are serde-friendly
//! structs; the canonical event name constants live here as well so both
//! sides of the bridge stay in sync.

use once_cell::sync::OnceCell;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime, Wry};
use tauri_plugin_notification::NotificationExt;

// ---------------------------------------------------------------------------
// Global handle — set once during `lib.rs` setup so background tasks
// (watcher, periodic rescan, etc.) can emit events without threading an
// `AppHandle` through every layer. Concrete `Wry` because the global can't
// be runtime-generic.
// ---------------------------------------------------------------------------

static APP_HANDLE: OnceCell<AppHandle<Wry>> = OnceCell::new();

pub fn set_app_handle(app: AppHandle<Wry>) {
    let _ = APP_HANDLE.set(app);
}

pub fn app_handle() -> Option<&'static AppHandle<Wry>> {
    APP_HANDLE.get()
}

// ---------------------------------------------------------------------------
// Event name constants — must match the frontend listener keys.
// ---------------------------------------------------------------------------

pub const EVT_SCAN_PROGRESS: &str = "scan_progress";
pub const EVT_SCAN_COMPLETE: &str = "scan_complete";
pub const EVT_DATASET_SCANNED: &str = "dataset_scanned";
pub const EVT_SCHEMA_CHANGED: &str = "schema_changed";
pub const EVT_WS_STATUS: &str = "ws_status";
pub const EVT_QUERY_STARTED: &str = "query_started";
pub const EVT_QUERY_COMPLETED: &str = "query_completed";
pub const EVT_HISTORY_UPDATED: &str = "history_updated";
pub const EVT_AUTH_EXPIRED: &str = "auth_expired";
pub const EVT_SYNC_COMPLETED: &str = "sync_completed";
pub const EVT_SYNC_FAILED: &str = "sync_failed";
pub const EVT_STATS_UPDATED: &str = "stats_updated";

// ---------------------------------------------------------------------------
// Payload types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ScanProgress {
    pub folder: String,
    pub current: usize,
    pub total: usize,
    pub current_file: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanComplete {
    pub folder: String,
    pub datasets: u64,
    pub columns: u64,
    pub errors: u64,
    pub total_bytes: u64,
    pub duration_ms: u64,
}

/// Emitted once per file as soon as the scanner finishes extracting its
/// metadata — lets FolderDetail stream rows into view instead of waiting
/// for the whole folder to finish scanning. `index` is 1-based to match
/// `ScanProgress.current`.
#[derive(Debug, Clone, Serialize)]
pub struct DatasetScanned {
    pub folder: String,
    pub index: usize,
    pub total: usize,
    pub dataset: crate::scanner::DatasetMetadata,
}

/// Fired once per dataset whose cached schema differs from a freshly-
/// scanned one. Emitted BEFORE the upsert so the UI can react to the
/// change (toast, notifications tab) while the cache is still in its
/// "old" state — not strictly required, but makes reasoning simpler.
///
/// `id` and `received_at` are assigned by `schema_notifications::record`
/// before emission so the frontend store and the on-disk log agree on
/// the same id (mark-read operations need that).
#[derive(Debug, Clone, Serialize)]
pub struct SchemaChanged {
    pub id: String,
    pub received_at: String, // RFC 3339
    pub workspace_id: String,
    pub dataset_path: String,
    pub dataset_name: String,
    pub added: u64,
    pub removed: u64,
    pub type_changed: u64,
    pub diff: crate::schema_diff::SchemaDiff,
    // Which machine in the workspace observed the change. Populated by local-scan
    // callers from config.agent.agent_id; populated by cross-machine
    // broadcast consumers from the WS message's origin_agent_id.
    pub origin_agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WsStatus {
    pub status: String, // "online" | "connecting" | "offline" | "error"
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueryStarted {
    pub query_id: String,
    pub file_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueryCompleted {
    pub query_id: String,
    pub file_path: String,
    pub status: String, // "success" | "error"
    pub row_count: Option<usize>,
    pub duration_ms: u64,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Emission helpers
// ---------------------------------------------------------------------------

pub fn emit_scan_progress<R: Runtime>(app: &AppHandle<R>, payload: ScanProgress) {
    let _ = app.emit(EVT_SCAN_PROGRESS, payload);
}

pub fn emit_scan_complete<R: Runtime>(app: &AppHandle<R>, payload: ScanComplete) {
    let _ = app.emit(EVT_SCAN_COMPLETE, payload);
}

pub fn emit_dataset_scanned<R: Runtime>(app: &AppHandle<R>, payload: DatasetScanned) {
    let _ = app.emit(EVT_DATASET_SCANNED, payload);
}

pub fn emit_schema_changed<R: Runtime>(app: &AppHandle<R>, payload: SchemaChanged) {
    let _ = app.emit(EVT_SCHEMA_CHANGED, payload);
}

pub fn emit_ws_status<R: Runtime>(app: &AppHandle<R>, status: &str, detail: Option<String>) {
    let _ = app.emit(
        EVT_WS_STATUS,
        WsStatus {
            status: status.to_string(),
            detail,
        },
    );
    // Also refresh the tray icon to reflect the current connection state.
    crate::tray::set_state(app, status);
}

pub fn emit_query_started<R: Runtime>(app: &AppHandle<R>, payload: QueryStarted) {
    let _ = app.emit(EVT_QUERY_STARTED, payload);
}

pub fn emit_query_completed<R: Runtime>(app: &AppHandle<R>, payload: QueryCompleted) {
    let _ = app.emit(EVT_QUERY_COMPLETED, payload);
}

pub fn emit_history_updated<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.emit(EVT_HISTORY_UPDATED, ());
}

pub fn emit_auth_expired<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.emit(EVT_AUTH_EXPIRED, ());
    notify(
        app,
        "Sery Link",
        "Your session has expired. Please reconnect.",
    );
    crate::tray::set_state(app, "error");
}

pub fn emit_sync_completed<R: Runtime>(app: &AppHandle<R>, folder: &str, datasets: u64) {
    let _ = app.emit(
        EVT_SYNC_COMPLETED,
        serde_json::json!({ "folder": folder, "datasets": datasets }),
    );
}

pub fn emit_sync_failed<R: Runtime>(app: &AppHandle<R>, folder: &str, error: &str) {
    // Intentionally does NOT call `notify()` on this path. The in-app
    // banner + sync_failed event already surface the failure, and going
    // through Tauri's notification plugin here crashed the process in
    // dev mode when notification permission hadn't been granted — Obj-C
    // `UNUserNotificationCenter` throws a foreign exception that unwinds
    // straight past Rust's panic machinery and aborts. Keep OS-level
    // notifications reserved for rare, genuinely user-actionable events.
    let _ = app.emit(
        EVT_SYNC_FAILED,
        serde_json::json!({ "folder": folder, "error": error }),
    );
}

pub fn emit_stats_updated<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.emit(EVT_STATS_UPDATED, ());
}

// ---------------------------------------------------------------------------
// Desktop notifications
// ---------------------------------------------------------------------------

/// Show an OS notification if notifications are enabled in config and the
/// OS has granted permission.
///
/// CAUTION: on macOS, calling `.show()` when permission hasn't been
/// determined can throw an Obj-C exception from `UNUserNotificationCenter`
/// which unwinds past Rust's panic machinery and aborts the process.
/// We gate on `permission_state()` to avoid the untrusted path, but even
/// so: use sparingly, only for events that genuinely warrant an OS-level
/// nudge (auth expired, first-time background minimisation). Routine
/// errors (sync failures, scan errors) should surface in the in-app UI,
/// not via notify().
pub fn notify<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    // Respect the user's notification preference first.
    if let Ok(config) = crate::config::Config::load() {
        if !config.app.notifications_enabled {
            return;
        }
    }

    // Only call .show() when permission is explicitly granted. When it's
    // Unknown or Denied we bail out — .show() in those states can throw
    // on macOS. Any error from the permission check itself also bails.
    let notif = app.notification();
    match notif.permission_state() {
        Ok(tauri_plugin_notification::PermissionState::Granted) => {}
        _ => return,
    }

    let _ = notif
        .builder()
        .title(title)
        .body(body)
        .show();
}

/// One-shot notification used the first time the user closes the window so
/// they understand the agent is still running in the background.
pub fn notify_window_hidden<R: Runtime>(app: &AppHandle<R>) {
    notify(
        app,
        "Sery Link is still running",
        "Click the tray icon to reopen the window. Sery will keep answering queries in the background.",
    );
}

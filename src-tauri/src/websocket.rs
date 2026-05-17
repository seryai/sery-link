#![allow(dead_code)]
//! WebSocket tunnel to Sery.ai Cloud.
//!
//! Responsibilities beyond "move bytes":
//!   * Emit `ws_status` events so the frontend status bar and tray stay in sync
//!     with the actual connection state.
//!   * Detect authentication failures (401/403) and surface `auth_expired` so
//!     the UI can prompt for re-auth instead of retrying forever.
//!   * Record query success/failure into `stats` and emit `query_started` /
//!     `query_completed` events for the live history view.
//!
//! The older `start(token)` entry point is preserved for headless / test use
//! where an `AppHandle` isn't available.

use crate::auth;
use crate::config::Config;
use crate::duckdb_engine;
use crate::error::{AgentError, Result};
use crate::events;
use crate::history;
use crate::keyring_store;
use crate::stats;
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Runtime};
use tokio::sync::RwLock;
use tokio_tungstenite::{
    connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream,
};
use url::Url;

const HEARTBEAT_INTERVAL_SECS: u64 = 30;
const RECONNECT_DELAY_SECS: u64 = 5;
const MAX_RECONNECT_DELAY_SECS: u64 = 60;

type WsWriter = Arc<
    tokio::sync::Mutex<
        futures::stream::SplitSink<
            WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
            Message,
        >,
    >,
>;

// Global handle on the *current* connection's write half. Used by
// async, non-WS code (the scanner pipeline) that wants to send
// best-effort status messages back to the cloud without owning a
// reference to WebSocketClient. None when the tunnel is offline —
// callers must treat send failures as non-fatal because scanner
// state will resync on the next message.
static OUTBOUND_WRITER: once_cell::sync::Lazy<RwLock<Option<WsWriter>>> =
    once_cell::sync::Lazy::new(|| RwLock::new(None));

// Last scan_status snapshot the scanner pipeline published. Read
// by the keepalive loop in commands::rescan_folder (so a stuck
// extraction doesn't let the cloud's 60s TTL expire) AND by the
// websocket reconnect path (so a tunnel blip mid-scan resyncs
// the dashboard pill within milliseconds instead of waiting 30s
// for the next keepalive tick). Cleared at scan end (idle/error)
// so a stale "scanning" state can't get replayed forever.
//
// Plain std::sync::Mutex (not tokio::sync::RwLock) so the sync
// scanner progress callbacks can write to it without awaiting.
// Writes hold the lock for microseconds; contention is a non-issue.
pub static LAST_SCAN_STATUS: once_cell::sync::Lazy<std::sync::Mutex<Option<Value>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(None));

/// Send a JSON Text frame on the active tunnel. Returns `true` if
/// the frame was queued, `false` if no connection was up. Best-
/// effort by design — the caller is expected to retry semantically
/// (e.g. emit the next progress tick) rather than buffer here.
pub async fn send_outbound_json(value: &Value) -> bool {
    let writer = {
        let guard = OUTBOUND_WRITER.read().await;
        match guard.as_ref() {
            Some(w) => Arc::clone(w),
            None => return false,
        }
    };
    let mut w = writer.lock().await;
    w.send(Message::Text(value.to_string())).await.is_ok()
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum ConnectionStatus {
    Online,
    Offline,
    Connecting,
    Error(String),
    AuthExpired,
}

impl ConnectionStatus {
    /// Friendly string used by the frontend + tray. Kept in one place so the
    /// match arms stay consistent.
    pub fn label(&self) -> &'static str {
        match self {
            ConnectionStatus::Online => "online",
            ConnectionStatus::Connecting => "connecting",
            ConnectionStatus::Offline => "offline",
            ConnectionStatus::Error(_) => "error",
            ConnectionStatus::AuthExpired => "error",
        }
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct WebSocketClient {
    config: Arc<RwLock<Config>>,
    status: Arc<RwLock<ConnectionStatus>>,
}

impl WebSocketClient {
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            status: Arc::new(RwLock::new(ConnectionStatus::Offline)),
        }
    }

    pub async fn get_status(&self) -> ConnectionStatus {
        self.status.read().await.clone()
    }

    /// Legacy entry point for headless / tests — runs without emitting UI
    /// events. Real app flow uses `start_with_app`.
    pub async fn start(&self, token: String) {
        let config = self.config.clone();
        let status = self.status.clone();

        tokio::spawn(async move {
            Self::maintain_connection::<tauri::Wry>(token, config, status, None).await;
        });
    }

    /// Main entry point — spawns the reconnect loop and wires up event
    /// emission so the frontend reacts instantly to state changes.
    /// Returns the JoinHandle so the caller can abort the task on disconnect.
    pub async fn start_with_app<R: Runtime>(
        &self,
        token: String,
        app: AppHandle<R>,
    ) -> tokio::task::JoinHandle<()> {
        let config = self.config.clone();
        let status = self.status.clone();

        tokio::spawn(async move {
            Self::maintain_connection(token, config, status, Some(app)).await;
        })
    }

    async fn maintain_connection<R: Runtime>(
        mut token: String,
        config: Arc<RwLock<Config>>,
        status: Arc<RwLock<ConnectionStatus>>,
        app: Option<AppHandle<R>>,
    ) {
        let mut reconnect_delay = Duration::from_secs(RECONNECT_DELAY_SECS);
        let max_delay = Duration::from_secs(MAX_RECONNECT_DELAY_SECS);
        // Consecutive auth errors before surfacing "session expired". A
        // single 401 during the handshake can be a transient startup issue
        // (API restarting, load-balancer draining), not a real token problem.
        // Three consecutive failures almost certainly mean the token is gone.
        let mut consecutive_auth_errors: u32 = 0;
        const AUTH_ERROR_THRESHOLD: u32 = 3;

        loop {
            *status.write().await = ConnectionStatus::Connecting;
            emit_status(&app, "connecting", None);

            let cfg = config.read().await.clone();
            let ws_url = format!("{}/v1/agent/tunnel", cfg.cloud.websocket_url);

            match Self::connect_and_run(&ws_url, &token, config.clone(), status.clone(), app.clone()).await {
                Ok(_) => {
                    eprintln!("WebSocket disconnected gracefully");
                    consecutive_auth_errors = 0;
                    *status.write().await = ConnectionStatus::Offline;
                    emit_status(&app, "offline", None);
                    reconnect_delay = Duration::from_secs(RECONNECT_DELAY_SECS);
                }
                Err(AgentError::WebSocket(ref e)) if is_auth_error(e) => {
                    consecutive_auth_errors += 1;
                    eprintln!(
                        "Auth error on WebSocket ({}/{}): {}",
                        consecutive_auth_errors, AUTH_ERROR_THRESHOLD, e
                    );

                    if consecutive_auth_errors < AUTH_ERROR_THRESHOLD {
                        // Might be a transient API restart — back off and retry
                        // before concluding the token is actually invalid.
                        *status.write().await = ConnectionStatus::Error(e.to_string());
                        emit_status(&app, "error", Some(e.to_string()));
                        tokio::time::sleep(reconnect_delay).await;
                        reconnect_delay = std::cmp::min(reconnect_delay * 2, max_delay);
                        continue;
                    }

                    // Workspace-key users: silently exchange the saved key for
                    // a fresh agent token instead of surfacing the re-auth modal.
                    if let Ok(saved_key) = keyring_store::get_workspace_key() {
                        eprintln!("Attempting silent re-auth with saved workspace key");
                        let api_url = cfg.cloud.api_url.clone();
                        let display_name = cfg.agent.name.clone();
                        let machine_id = cfg.agent.machine_id.clone();
                        match auth::auth_with_workspace_key(saved_key, display_name, machine_id, api_url).await {
                            Ok(new_token) => {
                                eprintln!("Silent re-auth succeeded, resuming tunnel");
                                token = new_token.access_token;
                                consecutive_auth_errors = 0;
                                reconnect_delay = Duration::from_secs(RECONNECT_DELAY_SECS);
                                continue;
                            }
                            Err(e) => {
                                eprintln!("Silent re-auth failed: {}", e);
                            }
                        }
                    }

                    // No saved key or re-auth failed — prompt the user.
                    *status.write().await = ConnectionStatus::AuthExpired;
                    if let Some(app) = &app {
                        events::emit_auth_expired(app);
                    }
                    return;
                }
                Err(e) => {
                    consecutive_auth_errors = 0;
                    eprintln!(
                        "WebSocket error: {}, reconnecting in {:?}",
                        e, reconnect_delay
                    );
                    *status.write().await = ConnectionStatus::Error(e.to_string());
                    emit_status(&app, "error", Some(e.to_string()));
                    tokio::time::sleep(reconnect_delay).await;
                    reconnect_delay = std::cmp::min(reconnect_delay * 2, max_delay);
                }
            }
        }
    }

    async fn connect_and_run<R: Runtime>(
        ws_url: &str,
        token: &str,
        config: Arc<RwLock<Config>>,
        status: Arc<RwLock<ConnectionStatus>>,
        app: Option<AppHandle<R>>,
    ) -> Result<()> {
        let url = Url::parse(ws_url)
            .map_err(|e| AgentError::WebSocket(format!("Invalid WebSocket URL: {}", e)))?;

        let request = tokio_tungstenite::tungstenite::http::Request::builder()
            .uri(ws_url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Host", url.host_str().unwrap_or("localhost"))
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tokio_tungstenite::tungstenite::handshake::client::generate_key(),
            )
            .body(())
            .map_err(|e| AgentError::WebSocket(format!("Failed to build request: {}", e)))?;

        let (ws_stream, _) = connect_async(request)
            .await
            .map_err(|e| AgentError::WebSocket(format!("Connection failed: {}", e)))?;

        eprintln!("WebSocket connected");
        *status.write().await = ConnectionStatus::Online;
        emit_status(&app, "online", None);

        let (write, mut read) = ws_stream.split();
        let write = Arc::new(tokio::sync::Mutex::new(write));

        // Publish the writer so non-WS code (scanner pipeline) can send
        // best-effort status messages on this connection. Cleared in the
        // RAII guard below so a dropped/errored connection can't leave a
        // stale Arc around — the next reconnect will publish a fresh one.
        *OUTBOUND_WRITER.write().await = Some(Arc::clone(&write));
        struct OutboundGuard;
        impl Drop for OutboundGuard {
            fn drop(&mut self) {
                // Synchronous drop: schedule the clear on a background task
                // so we don't block the runtime in Drop.
                tokio::spawn(async {
                    *OUTBOUND_WRITER.write().await = None;
                });
            }
        }
        let _outbound_guard = OutboundGuard;

        // Reconcile cloud dataset records against current source_roots.
        // Deletes stale datasets for sources removed while offline, failed
        // deletes, etc. Spawned detached so it never blocks the tunnel loop.
        {
            let cfg = config.read().await.clone();
            let api_url = cfg.cloud.api_url.clone();
            let token_clone = token.to_string();
            let source_roots: Vec<String> = cfg
                .sources
                .iter()
                .filter_map(|s| {
                    use crate::sources::SourceKind;
                    match &s.kind {
                        SourceKind::Local { path, .. } => {
                            Some(path.to_string_lossy().to_string())
                        }
                        SourceKind::S3 { url } | SourceKind::Https { url } => {
                            Some(url.clone())
                        }
                        _ => None,
                    }
                })
                .collect();
            tokio::spawn(async move {
                crate::scanner::reconcile_with_cloud(&api_url, &token_clone, source_roots).await;
            });
        }

        // Replay the last scan_status snapshot if we reconnected
        // mid-scan. Without this, a tunnel blip leaves the cloud
        // dashboard pill blank until the next 30s keepalive tick or
        // the next progress callback fires — for a slow-file scan
        // both could be far away. Best-effort: a send failure here
        // just means the keepalive will catch up shortly.
        let snapshot = LAST_SCAN_STATUS.lock().unwrap().clone();
        if let Some(payload) = snapshot {
            let mut w = write.lock().await;
            let _ = w.send(Message::Text(payload.to_string())).await;
        }

        // Heartbeat task — pings every N seconds. Dies when the connection
        // closes (send error → break).
        let heartbeat_handle = tokio::spawn({
            let write = Arc::clone(&write);
            async move {
                let mut interval =
                    tokio::time::interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
                loop {
                    interval.tick().await;
                    let ping_msg = serde_json::json!({
                        "type": "ping",
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    });
                    let mut write_guard = write.lock().await;
                    if write_guard
                        .send(Message::Text(ping_msg.to_string()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        });

        // Message handler — runs until the socket closes or errors.
        while let Some(msg_result) = read.next().await {
            match msg_result {
                Ok(Message::Text(text)) => {
                    if let Ok(message) = serde_json::from_str::<Value>(&text) {
                        if let Err(e) = Self::handle_message(
                            &message,
                            config.clone(),
                            Arc::clone(&write),
                            app.clone(),
                        )
                        .await
                        {
                            eprintln!("Message handler error: {}", e);
                        }
                    }
                }
                Ok(Message::Close(frame)) => {
                    eprintln!("WebSocket closed by server: {:?}", frame);
                    // If the server sent an auth-related close code, surface it
                    // so the reconnect loop can halt.
                    if let Some(f) = frame {
                        let code: u16 = f.code.into();
                        if code == 4001 || code == 4003 {
                            heartbeat_handle.abort();
                            return Err(AgentError::WebSocket(format!("unauthorized close: {}", code)));
                        }
                    }
                    break;
                }
                Err(e) => {
                    // tungstenite surfaces HTTP errors here during the handshake
                    // phase. Look for 401/403 in the error text.
                    let es = e.to_string();
                    if is_auth_error(&es) {
                        heartbeat_handle.abort();
                        return Err(AgentError::WebSocket(es));
                    }
                    eprintln!("WebSocket read error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        heartbeat_handle.abort();
        Ok(())
    }

    async fn handle_message<R: Runtime>(
        message: &Value,
        config: Arc<RwLock<Config>>,
        write: WsWriter,
        app: Option<AppHandle<R>>,
    ) -> Result<()> {
        let msg_type = message["type"]
            .as_str()
            .ok_or_else(|| AgentError::WebSocket("Missing message type".to_string()))?;

        match msg_type {
            "ping" => {
                let pong_msg = serde_json::json!({
                    "type": "pong",
                    "timestamp": message["timestamp"]
                });
                let mut write_guard = write.lock().await;
                write_guard.send(Message::Text(pong_msg.to_string())).await.ok();
            }
            "run_sql" => {
                Self::handle_run_sql(message, config, write, app).await?;
            }
            "schema_change" => {
                Self::handle_remote_schema_change(message, app);
            }
            "config_update" => {
                if let Some(config_val) = message.get("config") {
                    if let Ok(remote) = serde_json::from_value::<crate::config::RemoteAgentConfig>(config_val.clone()) {
                        if let Ok(mut c) = crate::config::Config::load() {
                            c.apply_remote_config(&remote);
                            let _ = c.save();
                        }
                    }
                }
            }
            _ => {
                eprintln!("Unknown message type: {}", msg_type);
            }
        }

        Ok(())
    }

    /// Handle a cloud-originated schema_change broadcast. Another machine
    /// in the workspace detected a schema drift and the backend fanned it
    /// out — we persist it locally (with a new local id) and emit a
    /// schema_changed app event so the Notifications tab + toast fire
    /// just like a local scan would.
    ///
    /// Best-effort: any persistence failure is swallowed. Showing a
    /// warning toast on cross-machine messages is more disruptive than
    /// silently missing one if the log is unwritable.
    fn handle_remote_schema_change<R: Runtime>(
        message: &Value,
        app: Option<AppHandle<R>>,
    ) {
        let Some(app) = app else { return };

        let workspace_id = message["workspace_id"].as_str().unwrap_or("").to_string();
        let dataset_path = message["dataset_path"].as_str().unwrap_or("").to_string();
        let dataset_name_raw = message["dataset_name"].as_str();
        let dataset_name = dataset_name_raw
            .map(str::to_string)
            .unwrap_or_else(|| {
                std::path::Path::new(&dataset_path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&dataset_path)
                    .to_string()
            });
        let added = message["added"].as_u64().unwrap_or(0);
        let removed = message["removed"].as_u64().unwrap_or(0);
        let type_changed = message["type_changed"].as_u64().unwrap_or(0);
        let origin_agent = message["origin_agent_name"].as_str().unwrap_or("another machine");
        let origin_agent_id = message["origin_agent_id"].as_str().map(str::to_string);

        // The cloud sends a {added, removed, changed} name-list drift
        // shape; our local SchemaDiff wants full ColumnChange entries.
        // For cross-machine events we synthesize best-effort entries —
        // column types aren't in the drift payload so we mark "unknown".
        let mut changes: Vec<crate::schema_diff::ColumnChange> = Vec::new();
        if let Some(drift) = message["drift"].as_object() {
            for name in drift
                .get("added")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|n| n.as_str()).collect::<Vec<_>>())
                .unwrap_or_default()
            {
                changes.push(crate::schema_diff::ColumnChange::Added {
                    name: name.to_string(),
                    column_type: "unknown".to_string(),
                });
            }
            for name in drift
                .get("removed")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|n| n.as_str()).collect::<Vec<_>>())
                .unwrap_or_default()
            {
                changes.push(crate::schema_diff::ColumnChange::Removed {
                    name: name.to_string(),
                    column_type: "unknown".to_string(),
                });
            }
            for name in drift
                .get("changed")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|n| n.as_str()).collect::<Vec<_>>())
                .unwrap_or_default()
            {
                changes.push(crate::schema_diff::ColumnChange::TypeChanged {
                    name: name.to_string(),
                    old_type: "unknown".to_string(),
                    new_type: "unknown".to_string(),
                });
            }
        }
        let diff = crate::schema_diff::SchemaDiff { changes };

        // Tag the display name with the origin machine so users can tell
        // "this change happened on my laptop" vs "on my desktop."
        let tagged_name = format!("{} (from {})", dataset_name, origin_agent);

        if let Ok(stored) = crate::schema_notifications::record(
            &workspace_id,
            &dataset_path,
            &tagged_name,
            added,
            removed,
            type_changed,
            diff.clone(),
            origin_agent_id.clone(),
        ) {
            crate::events::emit_schema_changed(
                &app,
                crate::events::SchemaChanged {
                    id: stored.id,
                    received_at: stored.received_at,
                    workspace_id,
                    dataset_path,
                    dataset_name: tagged_name,
                    added,
                    removed,
                    type_changed,
                    diff,
                    origin_agent_id,
                },
            );
        }
    }

    async fn handle_run_sql<R: Runtime>(
        message: &Value,
        config: Arc<RwLock<Config>>,
        write: WsWriter,
        app: Option<AppHandle<R>>,
    ) -> Result<()> {
        let query_id = message["query_id"]
            .as_str()
            .ok_or_else(|| AgentError::WebSocket("Missing query_id".to_string()))?;

        let sql = message["sql"]
            .as_str()
            .ok_or_else(|| AgentError::WebSocket("Missing sql".to_string()))?;

        // The cloud sends this field as `database_path` (see api/.../tunnel.py
        // ConnectionManager.send_query). Locally the value is just an absolute
        // file path on disk, so we keep the Rust binding named `file_path`.
        let file_path = message["database_path"]
            .as_str()
            .or_else(|| message["file_path"].as_str())
            .ok_or_else(|| AgentError::WebSocket("Missing database_path".to_string()))?;

        eprintln!("Executing query {}: {}", query_id, sql);

        // Tell the frontend a new query is starting so it can spin up a row in
        // the live history table before the result lands.
        if let Some(app) = &app {
            events::emit_query_started(
                app,
                events::QueryStarted {
                    query_id: query_id.to_string(),
                    file_path: file_path.to_string(),
                },
            );
        }

        let cfg = config.read().await.clone();

        match duckdb_engine::execute_query(sql, file_path, &cfg).await {
            Ok(result) => {
                eprintln!(
                    "Query {} completed: {} rows in {}ms",
                    query_id, result.row_count, result.duration_ms
                );

                history::record(
                    Some(query_id.to_string()),
                    file_path,
                    sql,
                    "success",
                    Some(result.row_count),
                    result.duration_ms,
                    None,
                );
                let _ = stats::record_query_success(result.duration_ms, Some(result.row_count));

                if let Some(app) = &app {
                    events::emit_query_completed(
                        app,
                        events::QueryCompleted {
                            query_id: query_id.to_string(),
                            file_path: file_path.to_string(),
                            status: "success".to_string(),
                            row_count: Some(result.row_count),
                            duration_ms: result.duration_ms,
                            error: None,
                        },
                    );
                    events::emit_history_updated(app);
                    events::emit_stats_updated(app);
                }

                let response = serde_json::json!({
                    "type": "query_result",
                    "query_id": query_id,
                    "columns": result.columns,
                    "rows": result.rows,
                    "row_count": result.row_count,
                    "execution_ms": result.duration_ms
                });

                let mut write_guard = write.lock().await;
                if let Err(e) = write_guard.send(Message::Text(response.to_string())).await {
                    eprintln!("Failed to send query result: {}", e);
                }
            }
            Err(error) => {
                eprintln!("Query {} failed: {}", query_id, error);

                history::record(
                    Some(query_id.to_string()),
                    file_path,
                    sql,
                    "error",
                    None,
                    0,
                    Some(error.to_string()),
                );
                let _ = stats::record_query_failure();

                if let Some(app) = &app {
                    events::emit_query_completed(
                        app,
                        events::QueryCompleted {
                            query_id: query_id.to_string(),
                            file_path: file_path.to_string(),
                            status: "error".to_string(),
                            row_count: None,
                            duration_ms: 0,
                            error: Some(error.to_string()),
                        },
                    );
                    events::emit_history_updated(app);
                    events::emit_stats_updated(app);
                }

                let error_response = serde_json::json!({
                    "type": "query_error",
                    "query_id": query_id,
                    "error": error.to_string(),
                    "suggestion": "Check if the file path is accessible and the SQL is valid"
                });

                let mut write_guard = write.lock().await;
                if let Err(e) = write_guard.send(Message::Text(error_response.to_string())).await {
                    eprintln!("Failed to send query error: {}", e);
                }
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_auth_error(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("401")
        || lower.contains("403")
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
}

fn emit_status<R: Runtime>(app: &Option<AppHandle<R>>, status: &str, detail: Option<String>) {
    if let Some(app) = app {
        events::emit_ws_status(app, status, detail);
    }
}

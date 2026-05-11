//! Local-first product analytics queue + flusher.
//!
//! Records user-visible events (scan_started, scan_completed,
//! machine_paired, query_executed, …) and ships them in batches to
//! `analytics.sery.ai`. The design intentionally mirrors how serious
//! telemetry SDKs work (PostHog, Sentry, Amplitude all do this):
//!
//!   1. `record_event(name, props)` appends a JSON line to
//!      `~/.seryai/events.jsonl`. Fast — one `open(append)` + write.
//!   2. A background task wakes every 60s, AND any call that pushes
//!      the queue past the 50-event threshold pings the flusher
//!      eagerly via a tokio Notify. Whichever fires first.
//!   3. The flusher reads the file, sends a single POST to
//!      `/v1/events`, and on 200 truncates the file. On non-200 it
//!      leaves the file alone — next flush retries with the same
//!      lines plus any new ones added in the meantime.
//!   4. Shutdown drains the queue best-effort (see
//!      `analytics::drain_on_shutdown`).
//!
//! Why JSONL instead of SQLite: append is one syscall, no schema
//! migration ever, no dependency on duckdb/keyring being healthy when
//! the user just wants telemetry to not break their main flow. The
//! tradeoff is no random-access updates, which we never need (events
//! are write-once, read-once on flush, then truncated).
//!
//! Privacy boundary: this file is the entire telemetry surface area.
//! Audit it once → know everything that leaves the machine in the
//! analytics channel. No call site should send file contents, query
//! text, paths the user typed, or anything that could ID a real
//! person. `props` are event-shape metadata only (counts, durations,
//! formats, success/failure).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

use crate::config::Config;
use crate::keyring_store;

/// Flush every 60 seconds OR whenever the queue grows past 50 events,
/// whichever comes first. Tunable but the defaults match PostHog's
/// SDK constants and seem to work well in practice.
const FLUSH_INTERVAL: Duration = Duration::from_secs(60);
const FLUSH_THRESHOLD_EVENTS: usize = 50;

/// Soft cap on the on-disk queue. If the user has been offline for
/// days AND we hit this, we drop the oldest events on next append.
/// Lossy, but the alternative (filling the disk with telemetry the
/// server will never see) is worse.
const MAX_QUEUED_EVENTS: usize = 10_000;

/// Wire payload for one event. Matches `app/schemas/analytics.py`
/// 1:1 — change both together.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventLine {
    pub event_id: Uuid,
    pub event_name: String,
    pub occurred_at: String, // RFC 3339
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub props: Value,
}

/// Coordination handle. `Mutex` serializes file access so concurrent
/// `record_event` calls + the flusher don't interleave writes.
/// `Notify` lets `record_event` wake the flusher early when the queue
/// crosses the threshold.
struct AnalyticsState {
    file_path: PathBuf,
    file_lock: Mutex<()>,
    flush_signal: Notify,
}

static STATE: OnceCell<Arc<AnalyticsState>> = OnceCell::new();

fn events_path() -> PathBuf {
    // Live next to config.json under ~/.seryai. Falls back to /tmp
    // if dirs::home_dir is None (CI containers without a home).
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".seryai")
        .join("events.jsonl")
}

fn state() -> Arc<AnalyticsState> {
    STATE
        .get_or_init(|| {
            Arc::new(AnalyticsState {
                file_path: events_path(),
                file_lock: Mutex::new(()),
                flush_signal: Notify::new(),
            })
        })
        .clone()
}

/// Record one event. Cheap — appends a single JSON line to disk and
/// returns. The flusher does the network work asynchronously.
///
/// `props` should be a plain `serde_json::json!({...})` value — keep
/// it shallow (≤4 KB serialized, server caps the rest). No file
/// contents, no SQL text, no user-typed paths — see the module
/// docstring.
///
/// Drops events silently when telemetry is disabled in settings; the
/// caller should not condition on the return value.
pub async fn record_event(name: &str, props: Value) {
    // Cheap guard before touching disk — config load is in-process
    // RAM after first read.
    let config = match Config::load() {
        Ok(c) => c,
        Err(_) => return,
    };
    if !config.app.telemetry_enabled {
        return;
    }

    let line = EventLine {
        event_id: Uuid::new_v4(),
        event_name: name.to_string(),
        occurred_at: Utc::now().to_rfc3339(),
        workspace_id: config.agent.workspace_id.clone(),
        agent_id: config.agent.agent_id.clone(),
        user_id: None,
        props,
    };

    let serialized = match serde_json::to_string(&line) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[analytics] serialize failed for {name}: {e}");
            return;
        }
    };

    let st = state();
    let crossed_threshold = {
        let _guard = st.file_lock.lock().await;
        if let Some(parent) = st.file_path.parent() {
            let _ = fs::create_dir_all(parent).await;
        }
        if let Err(e) = append_line(&st.file_path, &serialized).await {
            eprintln!("[analytics] append failed: {e}");
            return;
        }
        // Cheap line count — only used to decide whether to wake the
        // flusher. We don't need exact precision; an over- or under-
        // count by one is fine.
        line_count(&st.file_path).await.unwrap_or(0) >= FLUSH_THRESHOLD_EVENTS
    };
    if crossed_threshold {
        st.flush_signal.notify_one();
    }
}

async fn append_line(path: &PathBuf, line: &str) -> std::io::Result<()> {
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    f.write_all(line.as_bytes()).await?;
    f.write_all(b"\n").await?;
    Ok(())
}

async fn line_count(path: &PathBuf) -> std::io::Result<usize> {
    match fs::read_to_string(path).await {
        Ok(s) => Ok(s.lines().count()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(e),
    }
}

/// Read the queue file, drop the oldest events if we're over
/// MAX_QUEUED_EVENTS, return the lines we'll attempt to send +
/// keep the remainder on disk (in case of overflow trim).
async fn snapshot_queue(path: &PathBuf) -> std::io::Result<Vec<EventLine>> {
    let contents = match fs::read_to_string(path).await {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut events: Vec<EventLine> = contents
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    if events.len() > MAX_QUEUED_EVENTS {
        let drop_from_front = events.len() - MAX_QUEUED_EVENTS;
        eprintln!(
            "[analytics] queue overflow, dropping {drop_from_front} oldest events"
        );
        events.drain(0..drop_from_front);
    }
    Ok(events)
}

#[derive(Serialize)]
struct EventBatchOut<'a> {
    source: &'static str,
    events: &'a [EventLine],
    client_version: &'a str,
}

/// Try to ship every queued event in one batch. On success, truncate
/// the file. On failure (network / 5xx / 401), leave it for retry.
async fn flush_once() {
    let st = state();
    // Take the lock for the whole flush. Means new record_event calls
    // wait, but those are cheap and the flush is at most one POST.
    let guard = st.file_lock.lock().await;

    let events = match snapshot_queue(&st.file_path).await {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[analytics] flush: snapshot failed: {e}");
            drop(guard);
            return;
        }
    };
    if events.is_empty() {
        drop(guard);
        return;
    }

    let config = match Config::load() {
        Ok(c) => c,
        Err(_) => {
            drop(guard);
            return;
        }
    };
    let token = match keyring_store::get_token() {
        Ok(t) => t,
        Err(_) => {
            // Not paired yet — keep events on disk. Once the user
            // pairs, the next flush sends them under the new identity
            // (workspace_id will be filled in from config at that
            // point on each new record_event call, so older lines
            // with workspace_id=None still upload successfully — the
            // server fills missing workspace_id from the auth token).
            drop(guard);
            return;
        }
    };

    let batch = EventBatchOut {
        source: "desktop",
        events: &events,
        client_version: concat!("sery-link/", env!("CARGO_PKG_VERSION")),
    };

    let url = format!("{}/v1/events", config.cloud.analytics_url);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let mut req = client
        .post(&url)
        .bearer_auth(&token)
        .json(&batch);
    if let Some(agent_id) = config.agent.agent_id.as_deref() {
        req = req.header("X-Sery-Agent-Id", agent_id);
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[analytics] flush: network error: {e}");
            drop(guard);
            return;
        }
    };
    if !resp.status().is_success() {
        eprintln!(
            "[analytics] flush: non-2xx ({}) — leaving queue for retry",
            resp.status()
        );
        drop(guard);
        return;
    }

    // 200 — truncate the file. Use create(true) + write empty so a
    // concurrent read during truncate sees either the old contents
    // or empty, never half-truncated garbage.
    if let Err(e) = fs::write(&st.file_path, b"").await {
        eprintln!("[analytics] flush: truncate failed: {e}");
    }
    drop(guard);
}

/// Background task entry point. Spawn once at app startup. The task
/// runs until the process exits; lifetime mirrors the Tauri app.
pub fn spawn_flusher() {
    tokio::spawn(async {
        let st = state();
        loop {
            tokio::select! {
                _ = tokio::time::sleep(FLUSH_INTERVAL) => {
                    flush_once().await;
                }
                _ = st.flush_signal.notified() => {
                    // Eagerly woken by record_event when the queue
                    // crosses the threshold. Same flush; we just
                    // didn't wait the full 60s.
                    flush_once().await;
                }
            }
        }
    });
}

/// Best-effort drain on app shutdown. Bounded by a short timeout so
/// a wedged network can't keep the app open indefinitely.
///
/// Not currently wired into the Tauri exit handler — the existing
/// `.run(tauri::generate_context!())` call shape doesn't surface
/// RunEvent::ExitRequested. Wiring this means refactoring to
/// `.build(ctx).run(|h, e| match e ...)`, deferred to a follow-up
/// commit. For now, the 60-second flusher interval means we lose
/// at most ~60s of events on quit, which is acceptable for v1.
#[allow(dead_code)]
pub async fn drain_on_shutdown() {
    let _ = tokio::time::timeout(Duration::from_secs(5), flush_once()).await;
}

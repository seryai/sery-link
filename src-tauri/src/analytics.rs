//! Minimal anonymous analytics — install count + DAU only.
//!
//! What this file does:
//!
//!   1. On every app launch, records one `daily_ping` event keyed by
//!      a stable per-install random UUID (`app.install_id` in config,
//!      generated on first run).
//!   2. While the app is running, fires another `daily_ping` every
//!      12 hours. This catches users who keep Sery Link running for
//!      days at a time — without the periodic tick they'd never show
//!      up as DAU on days they don't relaunch.
//!   3. Buffers pings to `~/.seryai/events.jsonl` and flushes in
//!      batches to `analytics.sery.ai/v1/pings` (anonymous POST, no
//!      bearer token). On success, truncates the file. On failure
//!      (offline, 5xx), leaves it for the next flush.
//!
//! What this file deliberately does NOT do:
//!
//!   - No `workspace_id`, no `user_id`, no `agent_id`. Those would
//!     let us reconstruct per-user activity timelines — out of scope
//!     for "install + DAU." Server side can only derive
//!     `count(distinct install_id) per day`, nothing else.
//!   - No `scan_started`, `scan_completed`, `machine_paired`, or any
//!     event tied to product activity. The brand promise is "your
//!     files never leave your machine," and the cheapest way to
//!     honor it is to never put product-action events on the wire.
//!   - No file paths, file names, SQL text, IP addresses, hostnames,
//!     or anything else that could identify a person.
//!
//! The entire surface area of telemetry is this one file. Audit it
//! once → know everything that leaves the machine in the analytics
//! channel. See TELEMETRY.md for the user-facing policy.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

use crate::config::Config;

/// Consecutive flush failures. Used to throttle stderr noise when
/// the analytics endpoint is unreachable (DNS not configured,
/// service not deployed, user offline for an extended period).
/// We log the first failure of a streak normally, then every 10th
/// retry — so a permanently unreachable endpoint produces one line
/// per ~10 minutes instead of one per minute. Resets on success.
static FLUSH_CONSECUTIVE_FAILURES: AtomicU32 = AtomicU32::new(0);

/// Decide whether to print a flush-failure log line. Always logs the
/// first failure of a streak (count was 0 before this call), then
/// every 10th. Increments the counter as a side effect.
fn should_log_flush_failure() -> bool {
    let prev = FLUSH_CONSECUTIVE_FAILURES.fetch_add(1, Ordering::Relaxed);
    prev == 0 || prev % 10 == 0
}

fn reset_flush_failures() {
    FLUSH_CONSECUTIVE_FAILURES.store(0, Ordering::Relaxed);
}

/// Flush every 60 seconds OR whenever the queue grows past 10 events,
/// whichever comes first. The threshold is low because we only emit
/// ~2 events per day per install (one on launch + one per 12h tick).
/// In practice the timer fires, not the threshold.
const FLUSH_INTERVAL: Duration = Duration::from_secs(60);
const FLUSH_THRESHOLD_EVENTS: usize = 10;

/// Heartbeat cadence — fires a `daily_ping` every 12 hours while the
/// app is running. 12h covers UTC day boundaries: a user launching at
/// 23:55 UTC and never restarting still emits at least one ping for
/// the following UTC day. 24h would miss them.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(12 * 60 * 60);

/// Soft cap on the on-disk queue. If the user has been offline for
/// weeks AND we hit this (unlikely at ~2 events/day = 30 days of
/// offline coverage at MAX_QUEUED_EVENTS=60), we drop the oldest on
/// next append. Lossy but bounded.
const MAX_QUEUED_EVENTS: usize = 60;

/// Wire payload for one ping. Matches `app/schemas/analytics.py`
/// 1:1 — change both together.
///
/// Note what's NOT here: workspace_id, user_id, agent_id, hostname,
/// IP, anything user-typed. Just an opaque random install_id + the
/// app build's version + the OS string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventLine {
    pub event_id: Uuid,
    pub event_name: String,
    pub occurred_at: String, // RFC 3339
    pub install_id: String,
    pub props: Value,
}

/// Coordination handle. `Mutex` serializes file access so concurrent
/// record + flusher don't interleave writes. `Notify` lets record
/// wake the flusher eagerly when the queue crosses the threshold.
struct AnalyticsState {
    file_path: PathBuf,
    file_lock: Mutex<()>,
    flush_signal: Notify,
}

static STATE: OnceCell<Arc<AnalyticsState>> = OnceCell::new();

fn events_path() -> PathBuf {
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
/// Today the only event recorded is `daily_ping` (see `record_ping`),
/// but this function is left generic in case we ever add a second
/// genuinely-anonymous event. It refuses to attach any identifier
/// other than the install_id — there's no parameter for workspace /
/// user / agent ids by design.
///
/// Drops events silently when telemetry is disabled in settings or
/// when install_id hasn't been minted yet (shouldn't happen — config
/// loader mints on first read).
async fn record_event(name: &str, props: Value) {
    let config = match Config::load() {
        Ok(c) => c,
        Err(_) => return,
    };
    if !config.app.telemetry_enabled {
        return;
    }
    let install_id = match config.app.install_id.as_deref() {
        Some(id) => id.to_string(),
        None => return, // Config::load mints on first read; if missing, something's wrong — skip.
    };

    let line = EventLine {
        event_id: Uuid::new_v4(),
        event_name: name.to_string(),
        occurred_at: Utc::now().to_rfc3339(),
        install_id,
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
        line_count(&st.file_path).await.unwrap_or(0) >= FLUSH_THRESHOLD_EVENTS
    };
    if crossed_threshold {
        st.flush_signal.notify_one();
    }
}

/// The one and only event recorded by this build of Sery Link. Fires
/// on launch and every 12h while running. Server uses
/// `COUNT(DISTINCT install_id)` to derive installs + DAU.
pub async fn record_ping() {
    record_event(
        "daily_ping",
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "platform": std::env::consts::OS,
        }),
    )
    .await;
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
/// MAX_QUEUED_EVENTS, return the lines we'll attempt to send.
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
            "[analytics] queue overflow, dropping {drop_from_front} oldest pings"
        );
        events.drain(0..drop_from_front);
    }
    Ok(events)
}

#[derive(Serialize)]
struct PingBatchOut<'a> {
    source: &'static str,
    pings: &'a [EventLine],
    client_version: &'a str,
}

/// Try to ship every queued ping in one batch. On success, truncate
/// the file. On failure (network / 5xx), leave it for retry.
///
/// No bearer token — the analytics endpoint is anonymous. Auth would
/// require us to know the user, which defeats the whole point of
/// keying on install_id.
async fn flush_once() {
    let st = state();
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

    let batch = PingBatchOut {
        source: "desktop",
        pings: &events,
        client_version: concat!("sery-link/", env!("CARGO_PKG_VERSION")),
    };

    // If analytics_url is the prod default but api_url is local (dev mode),
    // fall back to the local api_url so pings don't hit the prod endpoint.
    let analytics_base = if config.cloud.analytics_url.contains("analytics.sery.ai")
        && (config.cloud.api_url.contains("localhost") || config.cloud.api_url.contains("127.0.0.1"))
    {
        config.cloud.api_url.trim_end_matches('/').to_string()
    } else {
        config.cloud.analytics_url.trim_end_matches('/').to_string()
    };
    let url = format!("{}/v1/pings", analytics_base);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let resp = match client.post(&url).json(&batch).send().await {
        Ok(r) => r,
        Err(e) => {
            if should_log_flush_failure() {
                eprintln!(
                    "[analytics] flush: network error: {e} (further failures suppressed)"
                );
            }
            drop(guard);
            return;
        }
    };
    if !resp.status().is_success() {
        if should_log_flush_failure() {
            eprintln!(
                "[analytics] flush: non-2xx ({}) — leaving queue for retry (further failures suppressed)",
                resp.status()
            );
        }
        drop(guard);
        return;
    }

    if let Err(e) = fs::write(&st.file_path, b"").await {
        eprintln!("[analytics] flush: truncate failed: {e}");
    }
    // Got a 200 — clear the failure counter so the next streak gets
    // its own first-line log.
    reset_flush_failures();
    drop(guard);
}

/// Background task entry point. Spawn once at app startup from the
/// Tauri `setup` callback. Two concurrent loops:
///   - flusher: drains the on-disk queue to the server (60s timer
///     OR when threshold is crossed by record_event)
///   - heartbeat: fires a `daily_ping` every 12 hours while running
///
/// Uses `tauri::async_runtime::spawn` (not `tokio::spawn`) because
/// the setup callback runs on the main thread BEFORE a tokio
/// runtime is in scope on that thread — `tokio::spawn` panics with
/// "there is no reactor running" in that context. The async_runtime
/// wrapper grabs Tauri's runtime regardless of which thread we're
/// on. Same pattern as gdrive_refresh.rs / tray.rs.
pub fn spawn_flusher() {
    tauri::async_runtime::spawn(async {
        let st = state();
        loop {
            tokio::select! {
                _ = tokio::time::sleep(FLUSH_INTERVAL) => {
                    flush_once().await;
                }
                _ = st.flush_signal.notified() => {
                    flush_once().await;
                }
            }
        }
    });
    tauri::async_runtime::spawn(async {
        // Heartbeat: while the app is running, ensure we emit at
        // least one daily_ping per UTC day even if the user never
        // restarts.
        loop {
            tokio::time::sleep(HEARTBEAT_INTERVAL).await;
            record_ping().await;
        }
    });
}

/// Best-effort drain on app shutdown. Bounded by a short timeout so
/// a wedged network can't keep the app open indefinitely.
///
/// Not currently wired into the Tauri exit handler — the existing
/// `.run(tauri::generate_context!())` call shape doesn't surface
/// RunEvent::ExitRequested. With the 60s flusher interval + 12h
/// heartbeat interval, losing at most one ping per quit is fine.
#[allow(dead_code)]
pub async fn drain_on_shutdown() {
    let _ = tokio::time::timeout(Duration::from_secs(5), flush_once()).await;
}

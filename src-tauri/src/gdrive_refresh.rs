//! Periodic refresh of every watched Google Drive folder.
//!
//! Slice 4 of v0.6 / Phase 3c-3. A long-lived tokio task wakes up
//! once an hour, walks every entry in `Config::gdrive_watched_folders`,
//! and reconciles the local cache against Drive's current state:
//!
//!   - **New / modified files** → downloaded via
//!     `gdrive_cache::download_if_stale`.
//!   - **Files Drive deleted** → removed from the cache via
//!     `gdrive_cache::forget_file`, but only if no OTHER active watch
//!     still references the same file_id (Drive folders can overlap
//!     via shared folders, so a file can belong to several watches).
//!   - **`last_walk_at`** → stamped so the UI's "last refreshed N
//!     min ago" label stays accurate.
//!
//! We do NOT call `rescan_folder` from here. The local file watcher
//! already monitors the cache root (it's a regular `watched_folders`
//! entry once the user has watched ≥1 Drive folder). Files appearing
//! or disappearing in the cache fire normal filesystem events that
//! drive the standard scan path. Calling rescan from here would
//! duplicate that work and surface scan-progress UI for a background
//! task that should be silent.
//!
//! Errors per-folder are isolated: a failure walking folder A doesn't
//! abort the refresh of folder B. Each error is logged to stderr
//! (visible in `tauri dev`); we don't escalate to the user via toasts
//! because background work that the user didn't initiate shouldn't
//! steal their attention.

use crate::config::Config;
use crate::error::Result;
use crate::{gdrive_cache, gdrive_creds, gdrive_walker};
use chrono::Utc;
use std::collections::HashSet;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Runtime};

/// How long between refresh ticks. One hour is the default elsewhere
/// in Sery Link's sync infrastructure (see `default_fallback_scan`)
/// and matches user mental model of "background, not real-time."
const REFRESH_INTERVAL: Duration = Duration::from_secs(3600);

/// Delay before the FIRST tick after app launch. Gives initial-scan
/// + auth bootstrap a chance to settle so we don't pile a Drive walk
/// on top of a cold-start workload.
const STARTUP_DELAY: Duration = Duration::from_secs(60);

/// Fire-and-forget loop kicked off from `lib.rs::run`. Runs forever
/// or until the app exits — there's no shutdown handshake because
/// Tauri terminates the runtime on quit and the loop's only side
/// effects are HTTP + filesystem writes that are safe to be cut off.
pub fn start_refresh_loop<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        loop {
            if let Err(e) = refresh_all(&app).await {
                eprintln!("[gdrive-refresh] tick failed: {}", e);
            }
            tokio::time::sleep(REFRESH_INTERVAL).await;
        }
    });
}

/// Walk every watched folder, reconcile, save. Returns Ok even if
/// individual folders failed — only Config-load / token-load errors
/// bubble up and abort the whole tick.
async fn refresh_all<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let config = Config::load()?;
    if config.gdrive_watched_folders.is_empty() {
        return Ok(());
    }

    // If the user disconnected and tokens are gone, skip silently.
    // disconnect_gdrive should have cleared gdrive_watched_folders,
    // but defense in depth — racing a disconnect with a refresh
    // tick should not cause every tick to bubble a token error.
    if gdrive_creds::load("default")?.is_none() {
        return Ok(());
    }

    let entries = config.gdrive_watched_folders.clone();
    for entry in entries {
        let folder_id = entry.folder_id.clone();
        let folder_name = entry.name.clone();
        if let Err(e) = refresh_one(app, &entry).await {
            eprintln!(
                "[gdrive-refresh] folder {} ({}) failed: {}",
                folder_name, folder_id, e
            );
        }
    }

    Ok(())
}

/// Refresh a single watched folder. Walks Drive, downloads new /
/// changed files, removes cache entries for files Drive deleted
/// (when not shared with another watch), updates config.
async fn refresh_one<R: Runtime>(
    app: &AppHandle<R>,
    entry: &crate::config::GdriveWatchedFolder,
) -> Result<()> {
    let walked = gdrive_walker::walk_folder(&entry.account_id, &entry.folder_id).await?;

    let new_ids: HashSet<String> = walked.files.iter().map(|f| f.id.clone()).collect();
    let old_ids: HashSet<String> = entry.file_ids.iter().cloned().collect();

    // Download every walked file. download_if_stale is a no-op when
    // the cached modifiedTime matches Drive's, so the cost is one
    // sidecar read per file when nothing changed.
    let mut downloaded = 0usize;
    for f in &walked.files {
        let stale = gdrive_cache::is_stale(&entry.account_id, f).unwrap_or(true);
        if stale {
            gdrive_cache::download_if_stale(&entry.account_id, f).await?;
            downloaded += 1;
        }
    }

    let deleted: Vec<String> = old_ids.difference(&new_ids).cloned().collect();
    if !deleted.is_empty() {
        // Re-load config inside the deletion block — between the walk
        // and now, the user may have added another watch that shares
        // some of these ids, and we must not nuke files still in use.
        let cfg = Config::load()?;
        let still_referenced: HashSet<String> = cfg
            .gdrive_watched_folders
            .iter()
            .filter(|f| !(f.account_id == entry.account_id && f.folder_id == entry.folder_id))
            .flat_map(|f| f.file_ids.iter().cloned())
            .collect();
        for id in &deleted {
            if !still_referenced.contains(id) {
                let _ = gdrive_cache::forget_file(&entry.account_id, id);
            }
        }
    }

    // Persist the new file_ids + last_walk_at. We re-load config so
    // we don't clobber any unrelated edits the user made between
    // the start of refresh_all and now (e.g. unwatching a different
    // folder via the UI).
    let mut cfg = Config::load()?;
    let new_id_vec: Vec<String> = walked.files.iter().map(|f| f.id.clone()).collect();
    cfg.update_gdrive_walk_state(
        &entry.account_id,
        &entry.folder_id,
        new_id_vec,
        Utc::now().to_rfc3339(),
    );
    cfg.save()?;

    // Tell the UI that this folder's "last refreshed" label needs to
    // re-tick. Frontend calls gdrive_list_watched_folders on receipt.
    let _ = app.emit(
        "gdrive-refresh",
        serde_json::json!({
            "folder_id": entry.folder_id,
            "downloaded": downloaded,
            "deleted": deleted.len(),
        }),
    );

    Ok(())
}

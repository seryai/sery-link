//! Background auto-scan loop. Reads `sync.auto_scan_interval_minutes` from
//! config on every tick, rescans watched folders that have file-system changes
//! since their last scan. Only one loop runs at a time (guard via
//! `AUTOSCAN_RUNNING`). Starting a second loop is a no-op.

use std::sync::atomic::{AtomicBool, Ordering};

static AUTOSCAN_RUNNING: AtomicBool = AtomicBool::new(false);

/// Start the background auto-scan loop. Call once from app setup.
/// Subsequent calls are silently ignored (only one loop per process).
pub fn start_autoscan_loop<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    // Only one loop running at a time.
    if AUTOSCAN_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        loop {
            // Read the interval from config on every tick so live changes
            // (e.g. from a config_update WebSocket message) take effect
            // within one cycle.
            let interval_minutes = crate::config::Config::load()
                .ok()
                .and_then(|c| c.sync.auto_scan_interval_minutes)
                .unwrap_or(0);

            if interval_minutes == 0 {
                // No interval set — sleep 60s and check again.
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                continue;
            }

            tokio::time::sleep(std::time::Duration::from_secs(
                interval_minutes as u64 * 60,
            ))
            .await;

            // Re-check after sleep in case the interval was disabled while we
            // were sleeping.
            let still_enabled = crate::config::Config::load()
                .ok()
                .and_then(|c| c.sync.auto_scan_interval_minutes)
                .is_some();
            if !still_enabled {
                continue;
            }

            // Collect watched-folder paths. Load fresh so newly-added folders
            // are picked up without restarting the loop.
            let folders = crate::config::Config::load()
                .map(|c| {
                    c.watched_folders
                        .iter()
                        .map(|f| f.path.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            for folder in folders {
                if folder_has_changes(&folder) {
                    let _ = crate::commands::rescan_folder(app.clone(), folder).await;
                }
            }
        }
    });
}

/// Returns `true` if any file inside `folder_path` has been modified since the
/// folder's `last_scan_at` timestamp. Returns `true` when the folder has never
/// been scanned (so we always scan it on first tick).
pub fn folder_has_changes(folder_path: &str) -> bool {
    let last_scan = crate::config::Config::load().ok().and_then(|c| {
        c.watched_folders
            .iter()
            .find(|f| f.path == folder_path)
            .and_then(|f| f.last_scan_at.as_deref())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc))
    });

    let cutoff = match last_scan {
        Some(t) => t,
        // Never scanned — always scan.
        None => return true,
    };

    // Walk the folder and stop as soon as one changed file is found.
    walk_has_newer(std::path::Path::new(folder_path), cutoff)
}

fn walk_has_newer(
    dir: &std::path::Path,
    cutoff: chrono::DateTime<chrono::Utc>,
) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if let Ok(meta) = entry.metadata() {
            if let Ok(mtime) = meta.modified() {
                let mtime_utc: chrono::DateTime<chrono::Utc> = mtime.into();
                if mtime_utc > cutoff {
                    return true;
                }
            }
            if meta.is_dir() && walk_has_newer(&path, cutoff) {
                return true;
            }
        }
    }
    false
}

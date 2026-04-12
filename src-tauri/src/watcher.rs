//! Live file watcher + periodic fallback rescan.
//!
//! notify does a great job on local disks, but cloud-synced folders (iCloud,
//! Dropbox, SMB, VMware shares) silently drop events. As a safety net we also
//! run a full rescan on every folder on a slow timer so nothing gets missed
//! forever.
//!
//! All sync activity is funnelled through a single `sync_folder` helper which:
//!   * Scans via `scanner::scan_folder`
//!   * Posts metadata to the cloud
//!   * Records a row in the audit log
//!   * Emits scan_complete / sync_completed / sync_failed events
//!   * Updates the tray state ("syncing" → "online") via `tray::set_state`

use crate::audit;
use crate::config::{Config, ScanStats};
use crate::error::{AgentError, Result};
use crate::events;
use crate::keyring_store;
use crate::scanner;
use crate::tray;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;

const DEBOUNCE_SECS: u64 = 2;

/// Handle returned by `start_watcher`. Dropping it stops the underlying
/// notify watcher, the debounce consumer, and the periodic rescan task.
pub struct WatcherHandle {
    shutdown_tx: mpsc::Sender<()>,
    _watcher: RecommendedWatcher,
    _fallback_handle: tokio::task::JoinHandle<()>,
}

impl Drop for WatcherHandle {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.try_send(());
        self._fallback_handle.abort();
    }
}

/// Start watching the given folders. Events are debounced and then dispatched
/// to `sync_folder`. A background task also rescans each folder on a slow
/// interval (`config.sync.fallback_scan_interval_seconds`) as an event-loss
/// safety net.
pub async fn start_watcher(folders: Vec<String>) -> Result<WatcherHandle> {
    if folders.is_empty() {
        return Err(AgentError::FileSystem("No folders to watch".to_string()));
    }

    // Pre-load exclude patterns so the notify callback can reject noise events
    // without even queueing them for the consumer task.
    let config = Config::load().unwrap_or_default();
    let excludes_by_folder: Vec<(String, Vec<glob::Pattern>)> = config
        .watched_folders
        .iter()
        .filter(|f| folders.contains(&f.path))
        .map(|f| {
            (
                f.path.clone(),
                f.exclude_patterns
                    .iter()
                    .filter_map(|g| glob::Pattern::new(g).ok())
                    .collect(),
            )
        })
        .collect();

    let (path_tx, mut path_rx) = mpsc::unbounded_channel::<PathBuf>();
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    let mut watcher: RecommendedWatcher = Watcher::new(
        {
            let path_tx = path_tx.clone();
            let excludes = excludes_by_folder.clone();
            move |res: notify::Result<Event>| {
                if let Ok(event) = res {
                    if matches!(
                        event.kind,
                        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                    ) {
                        for p in event.paths {
                            if !is_data_file(&p) {
                                continue;
                            }
                            // Respect exclude patterns so a busy `.git` folder
                            // never triggers a rescan storm.
                            if is_path_excluded(&p, &excludes) {
                                continue;
                            }
                            let _ = path_tx.send(p);
                        }
                    }
                }
            }
        },
        notify::Config::default(),
    )
    .map_err(|e| AgentError::FileSystem(format!("Failed to create watcher: {}", e)))?;

    for folder in &folders {
        watcher
            .watch(Path::new(folder), RecursiveMode::Recursive)
            .map_err(|e| {
                AgentError::FileSystem(format!("Failed to watch folder {}: {}", folder, e))
            })?;
        eprintln!("[watcher] watching {}", folder);
    }

    drop(path_tx);

    // Consumer task: debounce + dispatch to sync_folder.
    tokio::spawn(async move {
        let mut pending: HashSet<PathBuf> = HashSet::new();
        let debounce = Duration::from_secs(DEBOUNCE_SECS);

        loop {
            tokio::select! {
                biased;

                _ = shutdown_rx.recv() => {
                    eprintln!("[watcher] shutdown received, exiting consumer task");
                    break;
                }

                maybe_path = path_rx.recv() => {
                    match maybe_path {
                        Some(p) => { pending.insert(p); }
                        None => {
                            eprintln!("[watcher] event channel closed, exiting consumer task");
                            break;
                        }
                    }
                }

                _ = tokio::time::sleep(debounce), if !pending.is_empty() => {
                    let changed: Vec<PathBuf> = pending.drain().collect();
                    eprintln!("[watcher] flushing {} change(s)", changed.len());

                    if let Err(e) = handle_changes(changed).await {
                        eprintln!("[watcher] sync error: {}", e);
                    }
                }
            }
        }
    });

    // Periodic fallback rescan — catches events dropped by cloud-synced
    // folders. Interval comes from config so users can tune it.
    let fallback_interval = Duration::from_secs(
        config
            .sync
            .fallback_scan_interval_seconds
            .max(60), // floor at 1 minute to prevent accidental DoS
    );
    let fallback_folders = folders.clone();
    let fallback_handle = tokio::spawn(async move {
        // Wait one full interval before the first rescan so we don't double-
        // scan immediately after startup.
        tokio::time::sleep(fallback_interval).await;

        loop {
            for folder in &fallback_folders {
                eprintln!("[watcher] fallback rescan of {}", folder);
                if let Err(e) = sync_folder(folder).await {
                    eprintln!("[watcher] fallback sync error for {}: {}", folder, e);
                }
            }
            tokio::time::sleep(fallback_interval).await;
        }
    });

    Ok(WatcherHandle {
        shutdown_tx,
        _watcher: watcher,
        _fallback_handle: fallback_handle,
    })
}

/// Fan out a batch of changed paths to their parent watched folders and
/// rescan each one exactly once.
async fn handle_changes(paths: Vec<PathBuf>) -> Result<()> {
    let config = Config::load()?;

    if keyring_store::get_token().is_err() {
        eprintln!("[watcher] no token, skipping sync");
        return Ok(());
    }

    let mut affected_folders: HashSet<String> = HashSet::new();
    for path in &paths {
        for folder in &config.watched_folders {
            if path.starts_with(&folder.path) {
                affected_folders.insert(folder.path.clone());
                break;
            }
        }
    }

    for folder_path in affected_folders {
        if let Err(e) = sync_folder(&folder_path).await {
            eprintln!("[watcher] sync failed for {}: {}", folder_path, e);
        }
    }

    Ok(())
}

/// Scan + upload + audit + emit events. The single source of truth for
/// "something in this folder changed and we want the cloud to know".
async fn sync_folder(folder_path: &str) -> Result<()> {
    // Flip the tray to "syncing" so users see activity.
    if let Some(app) = events::app_handle() {
        tray::set_state(app, "syncing");
    }

    let started = std::time::Instant::now();

    let datasets = match scanner::scan_folder(folder_path).await {
        Ok(d) => d,
        Err(e) => {
            audit::record(folder_path, 0, 0, 0, Some(e.to_string()));
            if let Some(app) = events::app_handle() {
                events::emit_sync_failed(app, folder_path, &e.to_string());
                tray::set_state(app, "online");
            }
            return Err(e);
        }
    };

    let dataset_count = datasets.len() as u64;
    let column_count: u64 = datasets.iter().map(|d| d.schema.len() as u64).sum();
    let total_bytes: u64 = datasets.iter().map(|d| d.size_bytes).sum();

    let config = Config::load()?;
    let token = keyring_store::get_token()
        .map_err(|e| AgentError::Config(format!("missing token: {}", e)))?;

    let sync_result =
        scanner::sync_metadata_to_cloud(&config.cloud.api_url, &token, datasets).await;

    let duration_ms = started.elapsed().as_millis() as u64;

    match sync_result {
        Ok(_) => {
            // Persist scan stats on the folder so the folder card can show
            // them next render.
            if let Ok(mut c) = Config::load() {
                c.update_folder_scan_stats(
                    folder_path,
                    ScanStats {
                        datasets: dataset_count,
                        columns: column_count,
                        errors: 0,
                        total_bytes,
                        duration_ms,
                    },
                    chrono::Utc::now().to_rfc3339(),
                );
                let _ = c.save();
            }

            audit::record(folder_path, dataset_count, column_count, total_bytes, None);

            if let Some(app) = events::app_handle() {
                events::emit_scan_complete(
                    app,
                    events::ScanComplete {
                        folder: folder_path.to_string(),
                        datasets: dataset_count,
                        columns: column_count,
                        errors: 0,
                        total_bytes,
                        duration_ms,
                    },
                );
                events::emit_sync_completed(app, folder_path, dataset_count);
                tray::set_state(app, "online");
            }
            Ok(())
        }
        Err(e) => {
            audit::record(folder_path, 0, 0, 0, Some(e.to_string()));
            if let Some(app) = events::app_handle() {
                events::emit_sync_failed(app, folder_path, &e.to_string());
                tray::set_state(app, "online");
            }
            Err(e)
        }
    }
}

fn is_data_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).unwrap_or(""),
        "parquet" | "csv" | "xlsx" | "xls"
    )
}

fn is_path_excluded(path: &Path, excludes_by_folder: &[(String, Vec<glob::Pattern>)]) -> bool {
    for (base, patterns) in excludes_by_folder {
        if !path.starts_with(base) {
            continue;
        }
        let rel = path.strip_prefix(base).unwrap_or(path);
        let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or_default();
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
    }
    false
}

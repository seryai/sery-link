use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use crate::error::{AgentError, Result};
use crate::sources::{migrate_watched_folder_to_source, DataSource, SourceKind};

// ---------------------------------------------------------------------------
// Auth modes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum AuthMode {
    LocalOnly,
    BYOK {
        provider: String,
        #[serde(skip_serializing)]
        api_key: String,
    },
    WorkspaceKey {
        #[serde(skip_serializing)]
        key: String,
    },
}

// ---------------------------------------------------------------------------
// Top-level config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub agent: AgentConfig,
    pub watched_folders: Vec<WatchedFolder>,
    pub cloud: CloudConfig,
    pub sync: SyncConfig,
    #[serde(default)]
    pub app: AppConfig,
    /// Google Drive folders the user has chosen to watch. Distinct
    /// from `watched_folders` because they need a Drive API walk
    /// before the cache directory becomes scannable. The cache dir
    /// itself (`~/.seryai/gdrive-cache/<account>/`) IS added to
    /// `watched_folders` once at least one Drive folder is being
    /// watched, so the existing scanner picks up the downloaded
    /// files without any code in scanner.rs that knows about Drive.
    #[serde(default)]
    pub gdrive_watched_folders: Vec<GdriveWatchedFolder>,
    /// F42: unified sources sidebar. Populated by migration from
    /// `watched_folders` on first load after upgrade; new sources go
    /// here directly. `watched_folders` is kept written for one
    /// release (v0.7.0) for rollback safety, then read-only in v0.7.1,
    /// then removed in v0.8.0. See SPEC_F42_SOURCES_SIDEBAR.md §2.3.
    #[serde(default)]
    pub sources: Vec<DataSource>,
}

/// One Drive folder the user has elected to watch. The `name` is
/// shown in the UI; `folder_id` is the Drive id we re-walk on
/// refresh; `account_id` is keyed for multi-account future support
/// (always `"default"` in v0.6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GdriveWatchedFolder {
    pub account_id: String,
    pub folder_id: String,
    pub name: String,
    /// RFC 3339 timestamp of the last successful walk + download.
    /// Used by the UI ("last refreshed 2 min ago") and the
    /// background refresh job (slice 4) to space out re-walks.
    #[serde(default)]
    pub last_walk_at: Option<String>,
    /// Drive file ids we've cached as part of this watch. Lets us
    /// surgically uncache only this folder's files when the user
    /// unwatches it without nuking files that another watch shares.
    /// (Multi-watch + shared files is rare but possible — Drive
    /// folders can overlap.)
    #[serde(default)]
    pub file_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub platform: String,
    pub hostname: String,
    pub agent_id: Option<String>,
    // Persisted so the local cache + schema-diff path can key off a known
    // workspace_id without a round-trip to /v1/agent/info on every scan.
    // Populated by bootstrap_workspace, auth_with_key, and pair_complete
    // when they return a fresh AgentToken.
    #[serde(default)]
    pub workspace_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchedFolder {
    pub path: String,
    pub recursive: bool,
    #[serde(default = "default_exclude_patterns")]
    pub exclude_patterns: Vec<String>,
    #[serde(default = "default_max_file_size_mb")]
    pub max_file_size_mb: u64,
    #[serde(default)]
    pub last_scan_at: Option<String>,
    #[serde(default)]
    pub last_scan_stats: Option<ScanStats>,
    /// Whether this folder is exposed via the MCP stdio mode
    /// (`sery-link --mcp-stdio --root <this folder>`). Off by default;
    /// users opt-in per folder from Settings → MCP. The flag itself
    /// doesn't run anything — it's the LLM client (Claude Desktop /
    /// Cursor / Zed / …) that spawns the process when the user adds
    /// the corresponding `mcp.json` block. We track the state here so
    /// the Settings UI can show / hide the snippet generator and
    /// remind users which folders they've already exposed.
    #[serde(default)]
    pub mcp_enabled: bool,
}

fn default_exclude_patterns() -> Vec<String> {
    vec![
        ".DS_Store".to_string(),
        "__MACOSX".to_string(),
        ".git".to_string(),
        "node_modules".to_string(),
        ".venv".to_string(),
        "venv".to_string(),
        "target".to_string(),
        ".cache".to_string(),
        "~$*".to_string(),
        ".~lock*".to_string(),
    ]
}

fn default_max_file_size_mb() -> u64 {
    1024 // 1 GB
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanStats {
    pub datasets: u64,
    pub columns: u64,
    pub errors: u64,
    pub total_bytes: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudConfig {
    pub api_url: String,
    pub websocket_url: String,
    #[serde(default = "default_web_url")]
    pub web_url: String,
}

// Cloud endpoint defaults bake into the binary at compile time.
//
// Production builds get the real sery.ai URLs out of the box. Developers
// override per-environment by setting SERY_API_URL / SERY_WEBSOCKET_URL /
// SERY_WEB_URL when running `cargo build` or `pnpm tauri dev` — e.g.:
//
//     SERY_API_URL=http://localhost:8000 \
//     SERY_WEBSOCKET_URL=ws://localhost:8000 \
//     SERY_WEB_URL=http://localhost:3000 \
//     pnpm tauri dev
//
// Once the user has connected to a workspace, the resulting `~/.seryai/
// config.json` overrides these compile-time defaults at runtime, so a
// rebuild isn't required to point an installed app at a different cloud.
fn default_api_url() -> String {
    option_env!("SERY_API_URL")
        .unwrap_or("https://api.sery.ai")
        .to_string()
}

fn default_websocket_url() -> String {
    option_env!("SERY_WEBSOCKET_URL")
        .unwrap_or("wss://api.sery.ai")
        .to_string()
}

fn default_web_url() -> String {
    option_env!("SERY_WEB_URL")
        .unwrap_or("https://app.sery.ai")
        .to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    pub interval_seconds: u64,
    pub auto_sync_on_change: bool,
    #[serde(default = "default_fallback_scan")]
    pub fallback_scan_interval_seconds: u64,
    /// Per-extension scan-depth overrides. Keys are bare extensions (no dot,
    /// lowercase e.g. `"html"`); values are `"full" | "content" | "shallow"`.
    /// Empty by default — the scanner uses its built-in defaults
    /// (see `scanner::default_tier_for`). Users bump `html` to Content if
    /// they care about saved-page text, or demote `csv` to Shallow for
    /// folders of throwaway exports.
    #[serde(default)]
    pub scan_tier_overrides: std::collections::HashMap<String, String>,
    /// ROADMAP F2 — opt-in for uploading extracted document text
    /// (the markdown extracted from DOCX/PPTX/HTML/IPYNB) to the
    /// Sery cloud catalog. Default OFF: the catalog is content-blind
    /// for documents, matching ROADMAP F2's "never includes file
    /// contents" promise. Turning it on lets cross-machine document
    /// search match against extracted text — useful for "find every
    /// note that mentions Acme" across machines, but means the text
    /// crosses the network.
    ///
    /// When false, scanner short-circuits the document-markdown
    /// extraction and the sync payload omits `document_markdown`,
    /// so the cloud `Dataset.document_text` column stays NULL.
    ///
    /// Resolution of the F2 open question (DECISIONS.md 2026-04-25)
    /// per Option 3 (user opt-in, default off).
    #[serde(default)]
    pub include_document_text: bool,
}

fn default_fallback_scan() -> u64 {
    3600 // 1 hour
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_theme")]
    pub theme: String, // "light" | "dark" | "system"
    #[serde(default)]
    pub launch_at_login: bool,
    #[serde(default = "default_true")]
    pub auto_update: bool,
    #[serde(default = "default_true")]
    pub notifications_enabled: bool,
    #[serde(default)]
    pub first_run_completed: bool,
    #[serde(default)]
    pub window_hide_notified: bool,
    #[serde(default)]
    pub selected_auth_mode: Option<AuthMode>,
    // Surface a toast when a scan detects a schema change. The Notifications
    // tab + Machines badge still update regardless — this only silences the
    // transient popup, which scan-heavy users can find noisy.
    #[serde(default = "default_true")]
    pub schema_change_toasts_enabled: bool,
    /// DEPRECATED in v0.5.3 → file-manager pivot. Held the active
    /// BYOK provider name back when text-to-SQL ran on the desktop.
    /// Kept on the struct so existing user configs deserialize
    /// without losing their other fields; the value is no longer
    /// read by anything. Will be removed in v0.7.0.
    #[serde(default)]
    pub selected_byok_provider: Option<String>,
    /// DEPRECATED in v0.5.3 → file-manager pivot. Per-provider
    /// model overrides for the now-removed local BYOK Ask page.
    /// Same back-compat reason as `selected_byok_provider`. Will
    /// be removed in v0.7.0.
    #[serde(default)]
    pub byok_models: std::collections::HashMap<String, String>,
}

fn default_theme() -> String {
    "system".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            launch_at_login: true,
            auto_update: true,
            notifications_enabled: true,
            first_run_completed: false,
            window_hide_notified: false,
            selected_auth_mode: None,
            schema_change_toasts_enabled: true,
            selected_byok_provider: None,
            byok_models: std::collections::HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Default + I/O
// ---------------------------------------------------------------------------

impl Default for Config {
    fn default() -> Self {
        let hostname = hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "unknown".to_string());

        Self {
            agent: AgentConfig {
                name: hostname.clone(),
                platform: std::env::consts::OS.to_string(),
                hostname,
                agent_id: None,
                workspace_id: None,
            },
            watched_folders: Vec::new(),
            gdrive_watched_folders: Vec::new(),
            sources: Vec::new(),
            cloud: CloudConfig {
                api_url: default_api_url(),
                websocket_url: default_websocket_url(),
                web_url: default_web_url(),
            },
            sync: SyncConfig {
                interval_seconds: 300,
                auto_sync_on_change: true,
                fallback_scan_interval_seconds: default_fallback_scan(),
                scan_tier_overrides: std::collections::HashMap::new(),
                include_document_text: false,
            },
            app: AppConfig::default(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;

        let mut config: Self = if config_path.exists() {
            let contents = fs::read_to_string(&config_path)
                .map_err(|e| AgentError::Config(format!("Failed to read config: {}", e)))?;

            serde_json::from_str(&contents)
                .map_err(|e| AgentError::Config(format!("Failed to parse config: {}", e)))?
        } else {
            Self::default()
        };

        config.migrate_sources_if_needed();
        Ok(config)
    }

    /// F42 Day 2 wire-up + incremental migration: populate `sources`
    /// from legacy `watched_folders`. Runs on every Config::load.
    ///
    /// Behavior:
    /// - First load after upgrade: bulk-migrate every watched_folder.
    /// - Subsequent loads: pick up any watched_folder that doesn't yet
    ///   have a corresponding source (matched by path/url). New
    ///   entries get appended to the tail of `sources`, preserving
    ///   existing source IDs.
    ///
    /// The incremental path matters because the existing
    /// add_watched_folder / add_remote_source commands still write to
    /// watched_folders only — until those are replaced by the
    /// kind-specific add_*_source commands, this bridge keeps the
    /// Sources sidebar in sync without requiring callers to dual-write.
    ///
    /// Drive accounts (`gdrive_watched_folders`) are NOT migrated
    /// here — they rewire through this abstraction when the gdrive
    /// adapter itself is refactored to use `DataSource`.
    fn migrate_sources_if_needed(&mut self) {
        if self.watched_folders.is_empty() {
            return;
        }

        // Build a set of paths/urls already represented in sources so
        // we can skip already-migrated entries. Match by the path
        // (Local) or url (Https/S3) field — same key the legacy
        // watched_folders entry uses.
        let known: std::collections::HashSet<String> = self
            .sources
            .iter()
            .filter_map(|s| match &s.kind {
                SourceKind::Local { path, .. } => {
                    Some(path.to_string_lossy().to_string())
                }
                SourceKind::Https { url } | SourceKind::S3 { url } => {
                    Some(url.clone())
                }
                SourceKind::GoogleDrive { .. } => None,
            })
            .collect();

        let to_migrate: Vec<&WatchedFolder> = self
            .watched_folders
            .iter()
            .filter(|wf| !known.contains(&wf.path))
            .collect();

        if to_migrate.is_empty() {
            return;
        }

        // Append at the tail, preserving existing sort_order values.
        let mut next_order = self
            .sources
            .iter()
            .map(|s| s.sort_order)
            .max()
            .map(|m| m + 1)
            .unwrap_or(0);
        for wf in to_migrate {
            let mut new_source = migrate_watched_folder_to_source(wf);
            new_source.sort_order = next_order;
            next_order += 1;
            self.sources.push(new_source);
        }
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path()?;

        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| AgentError::Config(format!("Failed to create config dir: {}", e)))?;
        }

        let contents = serde_json::to_string_pretty(self)
            .map_err(|e| AgentError::Config(format!("Failed to serialize config: {}", e)))?;

        fs::write(&config_path, contents)
            .map_err(|e| AgentError::Config(format!("Failed to write config: {}", e)))?;

        Ok(())
    }

    pub fn config_path() -> Result<PathBuf> {
        dirs::home_dir()
            .ok_or_else(|| AgentError::Config("Could not find home directory".to_string()))
            .map(|home| home.join(".seryai").join("config.json"))
    }

    pub fn data_dir() -> Result<PathBuf> {
        dirs::home_dir()
            .ok_or_else(|| AgentError::Config("Could not find home directory".to_string()))
            .map(|home| home.join(".seryai"))
    }

    /// Add a folder to the watch list, idempotent on path.
    ///
    /// If `path` is already watched, only the `recursive` flag is updated —
    /// exclude_patterns / max_file_size_mb / scan history stay intact. This
    /// prevents the onboarding-double-click bug (user picks the same folder
    /// twice) from producing duplicate entries that cause double-scan +
    /// double-upload.
    pub fn add_watched_folder(&mut self, path: String, recursive: bool) {
        if let Some(existing) = self.watched_folders.iter_mut().find(|f| f.path == path) {
            existing.recursive = recursive;
            return;
        }
        self.watched_folders.push(WatchedFolder {
            path,
            recursive,
            exclude_patterns: default_exclude_patterns(),
            max_file_size_mb: default_max_file_size_mb(),
            last_scan_at: None,
            last_scan_stats: None,
            mcp_enabled: false,
        });
    }

    pub fn remove_watched_folder(&mut self, path: &str) {
        self.watched_folders.retain(|f| f.path != path);
    }

    /// Idempotent on (account_id, folder_id). Re-watching an existing
    /// Drive folder updates the display name + replaces the file_ids
    /// (so a folder that's had files added since last walk reflects
    /// the new set), but keeps last_walk_at unset until the caller
    /// stamps it after a successful walk.
    pub fn add_gdrive_watched_folder(&mut self, entry: GdriveWatchedFolder) {
        if let Some(existing) = self
            .gdrive_watched_folders
            .iter_mut()
            .find(|f| f.account_id == entry.account_id && f.folder_id == entry.folder_id)
        {
            existing.name = entry.name;
            existing.file_ids = entry.file_ids;
            return;
        }
        self.gdrive_watched_folders.push(entry);
    }

    pub fn remove_gdrive_watched_folder(&mut self, account_id: &str, folder_id: &str) {
        self.gdrive_watched_folders
            .retain(|f| !(f.account_id == account_id && f.folder_id == folder_id));
    }

    /// Replace the file_ids + stamp last_walk_at on a Drive watched
    /// folder after a successful refresh. Idempotent; no-op if the
    /// entry has been unwatched between the walk start and the
    /// config update (the refresh loop tolerates that race).
    pub fn update_gdrive_walk_state(
        &mut self,
        account_id: &str,
        folder_id: &str,
        file_ids: Vec<String>,
        when: String,
    ) {
        if let Some(entry) = self
            .gdrive_watched_folders
            .iter_mut()
            .find(|f| f.account_id == account_id && f.folder_id == folder_id)
        {
            entry.file_ids = file_ids;
            entry.last_walk_at = Some(when);
        }
    }

    pub fn update_folder_scan_stats(&mut self, path: &str, stats: ScanStats, when: String) {
        if let Some(folder) = self.watched_folders.iter_mut().find(|f| f.path == path) {
            folder.last_scan_stats = Some(stats);
            folder.last_scan_at = Some(when);
        }
    }

    // ─── F42 source mutations (Day 4) ──────────────────────────────
    //
    // The frontend mutates sources through these helpers via the
    // commands::{rename_source, set_source_group, remove_source,
    // reorder_sources} Tauri commands. Each helper is pure (no FS,
    // no async) so the Config tests can exercise them directly; the
    // Tauri command wrappers handle load/save around each call.

    /// Rename a source by id. Returns NotFound if no source matches.
    pub fn rename_source(&mut self, id: &str, new_name: String) -> Result<()> {
        match self.sources.iter_mut().find(|s| s.id == id) {
            Some(source) => {
                source.name = new_name;
                Ok(())
            }
            None => Err(AgentError::NotFound(format!(
                "No source with id {id:?}"
            ))),
        }
    }

    /// Set or clear a source's group. `None` moves it to the top-level
    /// (ungrouped). Returns NotFound if no source matches.
    pub fn set_source_group(
        &mut self,
        id: &str,
        group: Option<String>,
    ) -> Result<()> {
        match self.sources.iter_mut().find(|s| s.id == id) {
            Some(source) => {
                source.group = group;
                Ok(())
            }
            None => Err(AgentError::NotFound(format!(
                "No source with id {id:?}"
            ))),
        }
    }

    /// Drop a source by id. Returns NotFound if no source matches.
    /// Does NOT remove the corresponding entry in `watched_folders` —
    /// that legacy field is read-only post-v0.7.0 and kept for one
    /// release for rollback safety per spec §2.3.
    pub fn remove_source(&mut self, id: &str) -> Result<()> {
        let before = self.sources.len();
        self.sources.retain(|s| s.id != id);
        if self.sources.len() == before {
            return Err(AgentError::NotFound(format!(
                "No source with id {id:?}"
            )));
        }
        Ok(())
    }

    /// Rewrite each source's `sort_order` based on the input id list.
    /// IDs missing from `ordered_ids` keep their existing order
    /// appended after the explicitly ordered ones — defensive against
    /// the frontend sending a partial list. Returns NotFound if any id
    /// in `ordered_ids` doesn't match a source (the user can't reorder
    /// a phantom).
    pub fn reorder_sources(&mut self, ordered_ids: &[String]) -> Result<()> {
        for id in ordered_ids {
            if !self.sources.iter().any(|s| &s.id == id) {
                return Err(AgentError::NotFound(format!(
                    "No source with id {id:?}"
                )));
            }
        }
        for (i, id) in ordered_ids.iter().enumerate() {
            if let Some(s) = self.sources.iter_mut().find(|s| &s.id == id) {
                s.sort_order = i as i32;
            }
        }
        // Sources not mentioned: shift to the tail, preserving their
        // existing relative order.
        let tail_start = ordered_ids.len() as i32;
        let mut tail_idx = tail_start;
        let mut unmentioned: Vec<&mut DataSource> = self
            .sources
            .iter_mut()
            .filter(|s| !ordered_ids.contains(&s.id))
            .collect();
        unmentioned.sort_by_key(|s| s.sort_order);
        for s in unmentioned {
            s.sort_order = tail_idx;
            tail_idx += 1;
        }
        Ok(())
    }

    /// Migrate existing users to the new auth mode system.
    /// Checks keyring for existing token and sets appropriate auth mode.
    pub fn migrate_if_needed(&mut self) -> Result<()> {
        use crate::keyring_store;

        if self.app.selected_auth_mode.is_none() {
            // Check if user has existing workspace token
            if keyring_store::has_token() {
                // User was already authenticated with workspace key
                self.app.selected_auth_mode = Some(AuthMode::WorkspaceKey {
                    key: "<from_keyring>".to_string(),
                });
            } else {
                // New user or no previous auth - default to local-only
                self.app.selected_auth_mode = Some(AuthMode::LocalOnly);
            }
            self.save()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();

        // Verify default values
        assert!(!config.app.first_run_completed);
        assert!(config.app.auto_update);
        assert!(config.app.notifications_enabled);
        assert_eq!(config.app.theme, "system");
        assert!(config.app.selected_auth_mode.is_none());
        assert!(config.watched_folders.is_empty());
    }

    #[test]
    fn test_app_config_defaults() {
        let app_config = AppConfig::default();

        assert_eq!(app_config.theme, "system");
        assert!(app_config.launch_at_login);
        assert!(app_config.auto_update);
        assert!(app_config.notifications_enabled);
        assert!(!app_config.first_run_completed);
        assert!(!app_config.window_hide_notified);
        assert!(app_config.selected_auth_mode.is_none());
    }

    #[test]
    fn test_add_watched_folder() {
        let mut config = Config::default();

        assert_eq!(config.watched_folders.len(), 0);

        config.add_watched_folder("/path/to/folder".to_string(), true);

        assert_eq!(config.watched_folders.len(), 1);
        assert_eq!(config.watched_folders[0].path, "/path/to/folder");
        assert!(config.watched_folders[0].recursive);
        assert!(config.watched_folders[0].exclude_patterns.contains(&".git".to_string()));
    }

    #[test]
    fn add_watched_folder_is_idempotent_on_path() {
        let mut config = Config::default();
        config.add_watched_folder("/data".to_string(), true);
        // Mutate the existing entry — this simulates the scanner having
        // populated it — and then re-add. The mutation must be preserved
        // instead of being reset by the idempotent add.
        config.watched_folders[0].last_scan_at = Some("2026-04-17T12:00:00Z".to_string());

        config.add_watched_folder("/data".to_string(), true);

        assert_eq!(config.watched_folders.len(), 1, "no duplicate entries");
        assert_eq!(
            config.watched_folders[0].last_scan_at.as_deref(),
            Some("2026-04-17T12:00:00Z"),
            "existing scan history must not be reset"
        );
    }

    #[test]
    fn add_watched_folder_updates_recursive_flag_on_re_add() {
        let mut config = Config::default();
        config.add_watched_folder("/data".to_string(), false);
        assert!(!config.watched_folders[0].recursive);

        // User toggled recursive in settings — re-adding should update.
        config.add_watched_folder("/data".to_string(), true);
        assert_eq!(config.watched_folders.len(), 1);
        assert!(config.watched_folders[0].recursive);
    }

    #[test]
    fn test_remove_watched_folder() {
        let mut config = Config::default();

        config.add_watched_folder("/folder1".to_string(), true);
        config.add_watched_folder("/folder2".to_string(), true);
        config.add_watched_folder("/folder3".to_string(), true);

        assert_eq!(config.watched_folders.len(), 3);

        config.remove_watched_folder("/folder2");

        assert_eq!(config.watched_folders.len(), 2);
        assert_eq!(config.watched_folders[0].path, "/folder1");
        assert_eq!(config.watched_folders[1].path, "/folder3");
    }

    #[test]
    fn test_update_folder_scan_stats() {
        let mut config = Config::default();
        config.add_watched_folder("/test/folder".to_string(), true);

        let stats = ScanStats {
            datasets: 10,
            columns: 50,
            errors: 0,
            total_bytes: 1024000,
            duration_ms: 500,
        };

        let timestamp = "2026-04-15T12:00:00Z".to_string();

        config.update_folder_scan_stats("/test/folder", stats.clone(), timestamp.clone());

        let folder = &config.watched_folders[0];
        assert!(folder.last_scan_stats.is_some());
        assert_eq!(folder.last_scan_stats.as_ref().unwrap().datasets, 10);
        assert_eq!(folder.last_scan_at.as_ref().unwrap(), &timestamp);
    }

    #[test]
    fn test_exclude_patterns_default() {
        let patterns = default_exclude_patterns();

        assert!(patterns.contains(&".DS_Store".to_string()));
        assert!(patterns.contains(&".git".to_string()));
        assert!(patterns.contains(&"node_modules".to_string()));
        assert!(patterns.contains(&".venv".to_string()));
        assert!(patterns.contains(&"target".to_string()));
    }

    #[test]
    fn test_max_file_size_default() {
        let max_size = default_max_file_size_mb();
        assert_eq!(max_size, 1024); // 1 GB
    }

    #[test]
    fn test_auth_mode_serialization_in_config() {
        let mut config = Config::default();

        // Test LocalOnly mode
        config.app.selected_auth_mode = Some(AuthMode::LocalOnly);
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("LocalOnly"));

        // Test BYOK mode (api_key should not be serialized)
        config.app.selected_auth_mode = Some(AuthMode::BYOK {
            provider: "anthropic".to_string(),
            api_key: "secret-key".to_string(),
        });
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("BYOK"));
        assert!(!json.contains("secret-key"));

        // Test WorkspaceKey mode (key should not be serialized)
        config.app.selected_auth_mode = Some(AuthMode::WorkspaceKey {
            key: "secret-workspace".to_string(),
        });
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("WorkspaceKey"));
        assert!(!json.contains("secret-workspace"));
    }

    #[test]
    fn test_config_deserialization_with_missing_auth_mode() {
        // Simulate old config file without selected_auth_mode
        let json = r#"{
            "agent": {
                "name": "test",
                "platform": "macos",
                "hostname": "test-host",
                "agent_id": null
            },
            "watched_folders": [],
            "cloud": {
                "api_url": "http://localhost:8000",
                "websocket_url": "ws://localhost:8000",
                "web_url": "http://localhost:3000"
            },
            "sync": {
                "interval_seconds": 300,
                "auto_sync_on_change": true,
                "fallback_scan_interval_seconds": 3600
            },
            "app": {
                "theme": "system",
                "launch_at_login": true,
                "auto_update": true,
                "notifications_enabled": true,
                "first_run_completed": false,
                "window_hide_notified": false
            }
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();

        // selected_auth_mode should default to None
        assert!(config.app.selected_auth_mode.is_none());
    }

    #[test]
    fn test_scan_stats_serialization() {
        let stats = ScanStats {
            datasets: 100,
            columns: 500,
            errors: 5,
            total_bytes: 1048576,
            duration_ms: 1500,
        };

        let json = serde_json::to_string(&stats).unwrap();
        let deserialized: ScanStats = serde_json::from_str(&json).unwrap();

        assert_eq!(stats.datasets, deserialized.datasets);
        assert_eq!(stats.columns, deserialized.columns);
        assert_eq!(stats.errors, deserialized.errors);
        assert_eq!(stats.total_bytes, deserialized.total_bytes);
        assert_eq!(stats.duration_ms, deserialized.duration_ms);
    }

    #[test]
    fn test_watched_folder_defaults() {
        let mut config = Config::default();
        config.add_watched_folder("/test".to_string(), true);

        let folder = &config.watched_folders[0];

        assert_eq!(folder.max_file_size_mb, 1024);
        assert!(folder.exclude_patterns.len() > 0);
        assert!(folder.last_scan_at.is_none());
        assert!(folder.last_scan_stats.is_none());
    }

    // ─── F42: Sources migration wiring ─────────────────────────────
    //
    // Per SPEC_F42_SOURCES_SIDEBAR.md §2.3 + §5: after upgrade,
    // load() must transparently populate `sources` from legacy
    // `watched_folders`, idempotent on subsequent loads, and preserve
    // both fields on save (rollback safety for one release).

    use crate::sources::SourceKind;

    #[test]
    fn migrate_sources_populates_from_watched_folders_on_first_load() {
        let mut config = Config::default();
        config.add_watched_folder("/Users/me/Documents".to_string(), true);
        config.add_watched_folder("s3://my-bucket/data/".to_string(), true);

        // Pre-state: legacy populated, sources empty
        assert!(config.sources.is_empty());
        assert_eq!(config.watched_folders.len(), 2);

        config.migrate_sources_if_needed();

        // Post-state: sources populated, legacy untouched
        assert_eq!(config.sources.len(), 2);
        assert_eq!(config.watched_folders.len(), 2);
        assert!(matches!(config.sources[0].kind, SourceKind::Local { .. }));
        assert!(matches!(config.sources[1].kind, SourceKind::S3 { .. }));
    }

    #[test]
    fn migrate_sources_preserves_existing_ids_across_reload() {
        // User already on v0.7.x — re-running migration must NOT
        // duplicate entries OR generate fresh IDs for already-known
        // paths. Source IDs are load-bearing (keychain key, cache
        // prefix, deep links) — they have to survive re-load.
        let mut config = Config::default();
        config.add_watched_folder("/Users/me/Documents".to_string(), true);
        config.migrate_sources_if_needed();
        assert_eq!(config.sources.len(), 1);
        let original_id = config.sources[0].id.clone();

        // Re-run migration with no new watched_folders — must be a no-op.
        config.migrate_sources_if_needed();
        assert_eq!(
            config.sources.len(),
            1,
            "migration with no new watched_folders must be a no-op"
        );
        assert_eq!(
            config.sources[0].id, original_id,
            "existing source IDs must survive re-load"
        );
    }

    #[test]
    fn migrate_sources_picks_up_new_watched_folders_incrementally() {
        // The legacy add_watched_folder / add_remote_source commands
        // still write only to watched_folders. The incremental path
        // closes that gap so the Sources sidebar reflects the new
        // entry on next Config::load — without dual-writing.
        let mut config = Config::default();
        config.add_watched_folder("/Users/me/Documents".to_string(), true);
        config.migrate_sources_if_needed();
        assert_eq!(config.sources.len(), 1);
        let original_id = config.sources[0].id.clone();
        let original_order = config.sources[0].sort_order;

        // Simulate a subsequent add via the legacy command path.
        config.add_watched_folder("s3://my-bucket/data/".to_string(), true);
        config.migrate_sources_if_needed();

        assert_eq!(config.sources.len(), 2, "new source must be picked up");
        // First source preserved
        assert_eq!(config.sources[0].id, original_id);
        assert_eq!(config.sources[0].sort_order, original_order);
        // New source appended at the tail
        assert!(matches!(config.sources[1].kind, SourceKind::S3 { .. }));
        assert_eq!(
            config.sources[1].sort_order,
            original_order + 1,
            "new source must take the next sort_order slot"
        );
    }

    #[test]
    fn migrate_sources_skips_paths_already_in_sources() {
        // Mixed state: sources has one entry, watched_folders has the
        // same one (legacy still written) plus a new one. Migration
        // must add only the new one — not duplicate the matching path.
        let mut config = Config::default();
        config.add_watched_folder("/a".to_string(), true);
        config.add_watched_folder("/b".to_string(), true);
        config.migrate_sources_if_needed();
        assert_eq!(config.sources.len(), 2);

        // Re-call: the two existing paths are already known; nothing
        // to add.
        config.migrate_sources_if_needed();
        assert_eq!(config.sources.len(), 2);

        // Add a third watched_folder; migration adds exactly one source.
        config.add_watched_folder("/c".to_string(), true);
        config.migrate_sources_if_needed();
        assert_eq!(config.sources.len(), 3);
    }

    #[test]
    fn migrate_sources_no_op_on_empty_legacy() {
        // Fresh install — no watched_folders, no sources. Migration is
        // a clean no-op; sources stays empty.
        let mut config = Config::default();
        config.migrate_sources_if_needed();
        assert!(config.sources.is_empty());
        assert!(config.watched_folders.is_empty());
    }

    #[test]
    fn legacy_v06_config_json_loads_and_migrates() {
        // Real-shape v0.6.x config (no `sources` field at all).
        // serde_json::from_str + manual migrate proves the load path.
        let json = r#"{
            "agent": {
                "name": "test",
                "platform": "macos",
                "hostname": "test-host",
                "agent_id": null
            },
            "watched_folders": [
                {
                    "path": "/Users/me/Documents",
                    "recursive": true,
                    "exclude_patterns": [".git"],
                    "max_file_size_mb": 1024,
                    "last_scan_at": "2026-04-15T10:00:00Z",
                    "last_scan_stats": null,
                    "mcp_enabled": true
                }
            ],
            "cloud": {
                "api_url": "http://localhost:8000",
                "websocket_url": "ws://localhost:8000",
                "web_url": "http://localhost:3000"
            },
            "sync": {
                "interval_seconds": 300,
                "auto_sync_on_change": true,
                "fallback_scan_interval_seconds": 3600,
                "scan_tier_overrides": {},
                "include_document_text": false
            },
            "app": {
                "theme": "system",
                "launch_at_login": true,
                "auto_update": true,
                "notifications_enabled": true,
                "first_run_completed": false,
                "window_hide_notified": false
            }
        }"#;

        let mut config: Config = serde_json::from_str(json).expect("parse legacy config");

        // Field defaulted on missing
        assert!(config.sources.is_empty());

        config.migrate_sources_if_needed();

        assert_eq!(config.sources.len(), 1);
        let src = &config.sources[0];
        match &src.kind {
            SourceKind::Local { path, recursive, .. } => {
                assert_eq!(path, &PathBuf::from("/Users/me/Documents"));
                assert!(*recursive);
            }
            other => panic!("expected Local, got {:?}", other),
        }
        // mcp_enabled + last_scan_at carry through
        assert!(src.mcp_enabled);
        assert_eq!(src.last_scan_at.as_deref(), Some("2026-04-15T10:00:00Z"));
    }

    // ─── F42 source mutations (Day 4) ──────────────────────────────

    fn config_with_three_sources() -> Config {
        let mut config = Config::default();
        config.add_watched_folder("/a".to_string(), true);
        config.add_watched_folder("/b".to_string(), true);
        config.add_watched_folder("/c".to_string(), true);
        config.migrate_sources_if_needed();
        // sort_order set by migration to 0/1/2
        config
    }

    #[test]
    fn rename_source_changes_name_in_place() {
        let mut config = config_with_three_sources();
        let id = config.sources[1].id.clone();
        config
            .rename_source(&id, "Renamed".to_string())
            .expect("rename should succeed");
        assert_eq!(config.sources[1].name, "Renamed");
    }

    #[test]
    fn rename_source_returns_not_found_for_unknown_id() {
        let mut config = config_with_three_sources();
        let result = config.rename_source("does-not-exist", "X".to_string());
        assert!(matches!(result, Err(AgentError::NotFound(_))));
    }

    #[test]
    fn set_source_group_assigns_and_clears_group() {
        let mut config = config_with_three_sources();
        let id = config.sources[0].id.clone();
        config
            .set_source_group(&id, Some("Work".to_string()))
            .expect("set_group should succeed");
        assert_eq!(config.sources[0].group.as_deref(), Some("Work"));

        // None clears the group
        config
            .set_source_group(&id, None)
            .expect("clear_group should succeed");
        assert!(config.sources[0].group.is_none());
    }

    #[test]
    fn remove_source_drops_one_entry() {
        let mut config = config_with_three_sources();
        let id = config.sources[1].id.clone();
        config.remove_source(&id).expect("remove should succeed");
        assert_eq!(config.sources.len(), 2);
        assert!(!config.sources.iter().any(|s| s.id == id));
    }

    #[test]
    fn remove_source_returns_not_found_for_unknown_id() {
        let mut config = config_with_three_sources();
        let result = config.remove_source("phantom");
        assert!(matches!(result, Err(AgentError::NotFound(_))));
        assert_eq!(config.sources.len(), 3, "no entries dropped on miss");
    }

    #[test]
    fn reorder_sources_rewrites_sort_order_per_input_list() {
        let mut config = config_with_three_sources();
        let ids: Vec<String> = config.sources.iter().map(|s| s.id.clone()).collect();
        // Reverse
        let reversed = vec![ids[2].clone(), ids[1].clone(), ids[0].clone()];
        config
            .reorder_sources(&reversed)
            .expect("reorder should succeed");
        // Now sort_order matches the new positions
        let by_id: std::collections::HashMap<&String, i32> =
            config.sources.iter().map(|s| (&s.id, s.sort_order)).collect();
        assert_eq!(by_id[&ids[2]], 0);
        assert_eq!(by_id[&ids[1]], 1);
        assert_eq!(by_id[&ids[0]], 2);
    }

    #[test]
    fn reorder_sources_appends_unmentioned_to_tail() {
        let mut config = config_with_three_sources();
        let ids: Vec<String> = config.sources.iter().map(|s| s.id.clone()).collect();
        // Only mention the middle one; the other two should land after it
        let partial = vec![ids[1].clone()];
        config
            .reorder_sources(&partial)
            .expect("partial reorder should succeed");
        let by_id: std::collections::HashMap<&String, i32> =
            config.sources.iter().map(|s| (&s.id, s.sort_order)).collect();
        assert_eq!(by_id[&ids[1]], 0);
        // Unmentioned entries get tail positions, preserving their
        // pre-call relative order (ids[0] was sort_order 0 < ids[2]
        // sort_order 2 → ids[0] before ids[2] in tail)
        assert_eq!(by_id[&ids[0]], 1);
        assert_eq!(by_id[&ids[2]], 2);
    }

    #[test]
    fn reorder_sources_returns_not_found_for_phantom_id() {
        let mut config = config_with_three_sources();
        let real_id = config.sources[0].id.clone();
        let result = config.reorder_sources(&[real_id, "phantom".to_string()]);
        assert!(matches!(result, Err(AgentError::NotFound(_))));
    }

    #[test]
    fn save_roundtrip_preserves_both_legacy_and_sources_fields() {
        // SPEC §2.3: keep writing both for one release for rollback
        // safety. Confirm the JSON serialization includes both arrays.
        let mut config = Config::default();
        config.add_watched_folder("/Users/me/Documents".to_string(), true);
        config.migrate_sources_if_needed();

        let json = serde_json::to_string(&config).expect("serialize");
        // Both arrays must appear in the on-disk payload
        assert!(
            json.contains("\"watched_folders\""),
            "watched_folders must still serialize for rollback safety"
        );
        assert!(
            json.contains("\"sources\""),
            "sources must serialize so the next load reads it as authoritative"
        );

        // Round-trip the JSON: re-parse, run migration (idempotent),
        // assert sources stayed at exactly one entry.
        let mut reloaded: Config = serde_json::from_str(&json).expect("re-parse");
        reloaded.migrate_sources_if_needed();
        assert_eq!(reloaded.sources.len(), 1);
        assert_eq!(reloaded.watched_folders.len(), 1);
    }
}

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use crate::error::{AgentError, Result};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

fn default_web_url() -> String {
    "http://localhost:3000".to_string()
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
    // tab + Fleet badge still update regardless — this only silences the
    // transient popup, which scan-heavy users can find noisy.
    #[serde(default = "default_true")]
    pub schema_change_toasts_enabled: bool,
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
            cloud: CloudConfig {
                api_url: "http://localhost:8000".to_string(),
                websocket_url: "ws://localhost:8000".to_string(),
                web_url: default_web_url(),
            },
            sync: SyncConfig {
                interval_seconds: 300,
                auto_sync_on_change: true,
                fallback_scan_interval_seconds: default_fallback_scan(),
                scan_tier_overrides: std::collections::HashMap::new(),
            },
            app: AppConfig::default(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;

        if config_path.exists() {
            let contents = fs::read_to_string(&config_path)
                .map_err(|e| AgentError::Config(format!("Failed to read config: {}", e)))?;

            serde_json::from_str(&contents)
                .map_err(|e| AgentError::Config(format!("Failed to parse config: {}", e)))
        } else {
            Ok(Self::default())
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
        });
    }

    pub fn remove_watched_folder(&mut self, path: &str) {
        self.watched_folders.retain(|f| f.path != path);
    }

    pub fn update_folder_scan_stats(&mut self, path: &str, stats: ScanStats, when: String) {
        if let Some(folder) = self.watched_folders.iter_mut().find(|f| f.path == path) {
            folder.last_scan_stats = Some(stats);
            folder.last_scan_at = Some(when);
        }
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
}

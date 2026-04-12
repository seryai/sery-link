use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use crate::error::{AgentError, Result};

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
            launch_at_login: false,
            auto_update: true,
            notifications_enabled: true,
            first_run_completed: false,
            window_hide_notified: false,
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

    pub fn add_watched_folder(&mut self, path: String, recursive: bool) {
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
}

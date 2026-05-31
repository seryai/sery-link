use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::db::agent_driver::{AgentDriverClient, AgentMethod};

pub const DEFAULT_JRE_KEY: &str = "21";

fn default_jre_key() -> String {
    DEFAULT_JRE_KEY.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRegistry {
    #[serde(default)]
    pub jre: Option<JreInfo>,
    #[serde(default)]
    pub jres: std::collections::HashMap<String, JreInfo>,
    pub drivers: std::collections::HashMap<String, DriverInfo>,
}

impl AgentRegistry {
    pub fn resolve_jre(&self, key: &str) -> Option<&JreInfo> {
        if !self.jres.is_empty() {
            return self.jres.get(key);
        }
        if key == DEFAULT_JRE_KEY {
            self.jre.as_ref()
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JreInfo {
    pub version: String,
    pub platforms: std::collections::HashMap<String, ArtifactInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverInfo {
    pub version: String,
    pub label: String,
    pub min_app_version: String,
    pub jar: ArtifactInfo,
    #[serde(default = "default_jre_key")]
    pub jre: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactInfo {
    pub url: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentState {
    #[serde(default)]
    pub jre_version: Option<String>,
    #[serde(default)]
    pub jre_versions: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub installed_drivers: std::collections::HashMap<String, InstalledDriver>,
    #[serde(default)]
    pub java_runtime: JavaRuntimeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledDriver {
    pub version: String,
    pub installed_at: String,
    #[serde(default = "default_jre_key")]
    pub jre: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum JavaRuntimeMode {
    #[default]
    Managed,
    System,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JavaRuntimeConfig {
    #[serde(default)]
    pub mode: JavaRuntimeMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_java_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverStatus {
    pub db_type: String,
    pub label: String,
    pub version: String,
    pub size: u64,
    pub installed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    pub update_available: bool,
    pub jre: String,
    pub jre_installed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverStoreUsageItem {
    pub id: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverStoreUsage {
    pub total_bytes: u64,
    pub jre_bytes: u64,
    pub driver_bytes: u64,
    pub jres: Vec<DriverStoreUsageItem>,
    pub drivers: Vec<DriverStoreUsageItem>,
}

pub struct AgentManager {
    base_dir: PathBuf,
    app_version: String,
    pub(crate) daemons: Mutex<std::collections::HashMap<String, AgentDriverClient>>,
}

impl Default for AgentManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentManager {
    /// Creates a new `AgentManager` rooted at `~/.seryai/drivers/`.
    pub fn new() -> Self {
        let home =
            std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_else(|_| ".".to_string());
        Self::new_with_base_dir(PathBuf::from(home).join(".seryai").join("drivers"))
    }

    pub fn new_with_base_dir(base_dir: PathBuf) -> Self {
        Self::new_with_base_dir_and_app_version(base_dir, env!("CARGO_PKG_VERSION"))
    }

    pub fn new_with_base_dir_and_app_version(base_dir: PathBuf, app_version: impl Into<String>) -> Self {
        let mgr = Self {
            base_dir,
            app_version: app_version.into(),
            daemons: Mutex::new(std::collections::HashMap::new()),
        };
        mgr.migrate_legacy_jre();
        mgr
    }

    /// Migrate `jre/` → `jre-21/` if we encounter the old single-JRE layout.
    fn migrate_legacy_jre(&self) {
        let legacy = self.base_dir.join("jre");
        let versioned = self.jre_dir(DEFAULT_JRE_KEY);
        if legacy.exists() && !versioned.exists() {
            let _ = std::fs::rename(&legacy, &versioned);
        }
    }

    pub fn base_dir(&self) -> &PathBuf {
        &self.base_dir
    }

    pub fn agent_app_version(&self) -> &str {
        &self.app_version
    }

    /// `~/.seryai/drivers/jre-{jre_key}/`
    pub fn jre_dir(&self, jre_key: &str) -> PathBuf {
        self.base_dir.join(format!("jre-{jre_key}"))
    }

    pub fn jre_java_path(&self, jre_key: &str) -> PathBuf {
        let dir = self.jre_dir(jre_key);
        let java_name = if cfg!(windows) { "java.exe" } else { "java" };
        let flat = dir.join("bin").join(java_name);
        if flat.exists() {
            return flat;
        }
        // macOS Adoptium JRE may use Contents/Home/ layout
        let macos = dir.join("Contents").join("Home").join("bin").join(java_name);
        if macos.exists() {
            return macos;
        }
        flat
    }

    /// `~/.seryai/drivers/drivers/{db_type}/agent.jar`
    pub fn driver_jar_path(&self, db_type: &str) -> PathBuf {
        self.base_dir.join("drivers").join(db_type).join("agent.jar")
    }

    fn state_path(&self) -> PathBuf {
        self.base_dir.join("state.json")
    }

    pub fn load_state(&self) -> AgentState {
        std::fs::read_to_string(self.state_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save_state(&self, state: &AgentState) -> Result<(), String> {
        let dir = self.base_dir.clone();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
        std::fs::write(self.state_path(), json).map_err(|e| e.to_string())
    }

    pub fn is_jre_installed(&self, jre_key: &str) -> bool {
        self.jre_java_path(jre_key).exists()
    }

    pub fn is_driver_installed(&self, db_type: &str) -> bool {
        self.driver_jar_path(db_type).exists()
    }

    pub fn collect_driver_store_usage(&self) -> DriverStoreUsage {
        let mut jres = Vec::new();
        let mut jre_bytes = 0u64;
        if let Ok(entries) = std::fs::read_dir(&self.base_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
                    continue;
                };
                if !name.starts_with("jre-") {
                    continue;
                }
                let key = name.trim_start_matches("jre-").to_string();
                let bytes = path_size_bytes(&path);
                jre_bytes = jre_bytes.saturating_add(bytes);
                jres.push(DriverStoreUsageItem { id: key, bytes });
            }
        }
        jres.sort_by(|l, r| l.id.cmp(&r.id));

        let mut driver_items = Vec::new();
        let mut driver_bytes = 0u64;
        let drivers_root = self.base_dir.join("drivers");
        if let Ok(entries) = std::fs::read_dir(&drivers_root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let Some(id) = path.file_name().and_then(|v| v.to_str()) else {
                    continue;
                };
                let bytes = path_size_bytes(&path);
                driver_bytes = driver_bytes.saturating_add(bytes);
                driver_items.push(DriverStoreUsageItem { id: id.to_string(), bytes });
            }
        }
        driver_items.sort_by(|l, r| l.id.cmp(&r.id));

        DriverStoreUsage {
            total_bytes: jre_bytes.saturating_add(driver_bytes),
            jre_bytes,
            driver_bytes,
            jres,
            drivers: driver_items,
        }
    }

    pub fn resolve_java_runtime(&self, state: &AgentState, jre_key: &str) -> Result<PathBuf, String> {
        match state.java_runtime.mode {
            JavaRuntimeMode::Managed => {
                if !self.is_jre_installed(jre_key) {
                    return Err(format!(
                        "JRE {jre_key} runtime is not installed. Please install it from the Driver Manager."
                    ));
                }
                Ok(self.jre_java_path(jre_key))
            }
            JavaRuntimeMode::System => resolve_system_java_path(None).ok_or_else(|| {
                "System Java runtime was not found on PATH. Please install Java or choose a custom Java executable."
                    .to_string()
            }),
            JavaRuntimeMode::Custom => {
                let path = state
                    .java_runtime
                    .custom_java_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|path| !path.is_empty())
                    .ok_or_else(|| {
                        "Custom Java runtime path is empty. Please choose a Java executable.".to_string()
                    })?;
                resolve_custom_java_path(path)
            }
        }
    }

    pub async fn stop_daemons(&self) {
        crate::agent_runtime::stop_daemons(self).await;
    }

    pub async fn stop_daemon_by_key(&self, agent_key: &str) {
        crate::agent_runtime::stop_daemon_by_key(self, agent_key).await;
    }

    pub async fn spawn_agent(
        &self,
        driver_key: &str,
    ) -> Result<AgentDriverClient, String> {
        crate::agent_runtime::spawn_client_for_key(self, driver_key).await
    }

    pub async fn call_daemon<T: serde::de::DeserializeOwned + Send + 'static>(
        &self,
        driver_key: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<T, String> {
        crate::agent_runtime::call_daemon(self, driver_key, method, params).await
    }

    pub async fn call_daemon_method<T: serde::de::DeserializeOwned + Send + 'static>(
        &self,
        driver_key: &str,
        method: AgentMethod,
        params: serde_json::Value,
    ) -> Result<T, String> {
        crate::agent_runtime::call_daemon_method(self, driver_key, method, params).await
    }

    pub fn current_platform() -> &'static str {
        if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
            "macos-aarch64"
        } else if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
            "macos-x64"
        } else if cfg!(target_os = "linux") && cfg!(target_arch = "aarch64") {
            "linux-aarch64"
        } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
            "linux-x64"
        } else if cfg!(target_os = "windows") && cfg!(target_arch = "aarch64") {
            "windows-aarch64"
        } else if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
            "windows-x64"
        } else {
            "unknown"
        }
    }
}

fn path_size_bytes(path: &Path) -> u64 {
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.is_file() {
            return meta.len();
        }
        if !meta.is_dir() {
            return 0;
        }
    } else {
        return 0;
    }

    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            total = total.saturating_add(path_size_bytes(&entry.path()));
        }
    }
    total
}

fn java_executable_name() -> &'static str {
    if cfg!(windows) { "java.exe" } else { "java" }
}

fn resolve_custom_java_path(path: &str) -> Result<PathBuf, String> {
    let raw = PathBuf::from(path);
    if is_executable_file(&raw) {
        return Ok(raw);
    }

    let flat = raw.join("bin").join(java_executable_name());
    if is_executable_file(&flat) {
        return Ok(flat);
    }

    let macos = raw.join("Contents").join("Home").join("bin").join(java_executable_name());
    if is_executable_file(&macos) {
        return Ok(macos);
    }

    Err(format!("Custom Java runtime does not exist or is not a Java executable: {}", raw.display()))
}

fn resolve_system_java_path(path_var: Option<&OsStr>) -> Option<PathBuf> {
    let path_var = path_var.map(|p| p.to_owned()).or_else(|| std::env::var_os("PATH"))?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(java_executable_name()))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata().map(|meta| meta.permissions().mode() & 0o111 != 0).unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_manager(name: &str) -> AgentManager {
        let dir = std::env::temp_dir()
            .join(format!("sery-agent-manager-{name}-{}", uuid::Uuid::new_v4()));
        AgentManager::new_with_base_dir(dir)
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).unwrap();
        }
    }

    #[test]
    fn resolves_managed_java_runtime_by_default() {
        let manager = test_manager("managed");
        let java = manager.jre_java_path(DEFAULT_JRE_KEY);
        touch(&java);

        let state = AgentState::default();
        assert_eq!(manager.resolve_java_runtime(&state, DEFAULT_JRE_KEY).unwrap(), java);
    }

    #[test]
    fn resolves_custom_java_runtime_when_configured() {
        let manager = test_manager("custom");
        let custom_java = manager.base_dir().join("custom").join("bin").join("java");
        touch(&custom_java);
        let state = AgentState {
            java_runtime: JavaRuntimeConfig {
                mode: JavaRuntimeMode::Custom,
                custom_java_path: Some(custom_java.to_string_lossy().to_string()),
            },
            ..AgentState::default()
        };
        assert_eq!(manager.resolve_java_runtime(&state, DEFAULT_JRE_KEY).unwrap(), custom_java);
    }

    #[test]
    fn rejects_missing_custom_java_runtime() {
        let manager = test_manager("missing-custom");
        let state = AgentState {
            java_runtime: JavaRuntimeConfig {
                mode: JavaRuntimeMode::Custom,
                custom_java_path: Some(
                    manager.base_dir().join("missing-java").to_string_lossy().to_string(),
                ),
            },
            ..AgentState::default()
        };
        let err = manager.resolve_java_runtime(&state, DEFAULT_JRE_KEY).unwrap_err();
        assert!(err.contains("Custom Java runtime does not exist"));
    }

    #[test]
    fn stores_configured_app_version_for_agent_handshake() {
        let dir = std::env::temp_dir()
            .join(format!("sery-agent-manager-version-{}", uuid::Uuid::new_v4()));
        let manager = AgentManager::new_with_base_dir_and_app_version(dir, "0.12.0");
        assert_eq!(manager.agent_app_version(), "0.12.0");
    }

    #[tokio::test]
    async fn runtime_gateway_returns_existing_missing_driver_error() {
        let manager = test_manager("missing-driver");
        let err = match manager.spawn_agent("snowflake").await {
            Ok(_) => panic!("missing driver should fail"),
            Err(err) => err,
        };
        assert_eq!(err, "snowflake driver is not installed. Please install it from the Driver Manager.");
    }

    #[tokio::test]
    async fn runtime_gateway_returns_existing_missing_java_error() {
        let manager = test_manager("missing-java");
        let jar = manager.driver_jar_path("snowflake");
        touch(&jar);

        let err = match manager.spawn_agent("snowflake").await {
            Ok(_) => panic!("missing Java runtime should fail"),
            Err(err) => err,
        };
        assert_eq!(err, "JRE 21 runtime is not installed. Please install it from the Driver Manager.");
    }
}

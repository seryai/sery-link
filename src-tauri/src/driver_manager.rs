//! Tauri commands for the Driver Store — install/uninstall Java-based JDBC
//! drivers (Oracle, Snowflake, DB2, SAP HANA, …) on demand, without bloating
//! the main binary.
//!
//! Drivers are stored at `~/.seryai/drivers/` (JREs + JARs, ~180 MB per JRE).
//! The registry is fetched from:
//!   https://github.com/seryai/sery-drivers/releases/latest/download/driver-registry.json

use db_core::agent_manager::{AgentManager, DriverStatus, DriverStoreUsage, JavaRuntimeConfig, JavaRuntimeMode, DEFAULT_JRE_KEY};
use db_core::agent_service::{
    build_driver_list, fetch_registry, import_driver_jar, install_driver, invalidate_registry_cache,
    uninstall_driver,
};
use once_cell::sync::Lazy;
use std::path::Path;
use std::sync::Arc;
use tauri::Emitter;

// ──────────── Global AgentManager instance ────────────

static AGENT_MANAGER: Lazy<Arc<AgentManager>> =
    Lazy::new(|| Arc::new(AgentManager::new()));

fn am() -> Arc<AgentManager> {
    AGENT_MANAGER.clone()
}

// ──────────── Commands ────────────

/// List all available drivers with install status (local only, no network call).
#[tauri::command]
pub async fn list_drivers_local() -> Result<Vec<DriverStatus>, String> {
    Ok(build_driver_list(&am(), None))
}

/// List all available drivers with install status (fetches registry for version/size info).
#[tauri::command]
pub async fn list_drivers(_app: tauri::AppHandle) -> Result<Vec<DriverStatus>, String> {
    let registry = fetch_registry().await.ok();
    Ok(build_driver_list(&am(), registry.as_ref()))
}

/// Install a driver (and the required JRE if missing). Emits
/// `driver-install-progress` events during download.
#[tauri::command]
pub async fn install_driver_cmd(app: tauri::AppHandle, db_type: String) -> Result<(), String> {
    let app_handle = app.clone();
    let db_type_clone = db_type.clone();
    install_driver(&am(), &db_type, move |event| {
        let payload = DriverInstallProgressPayload {
            db_type: db_type_clone.clone(),
            step: event.step.clone(),
            downloaded: event.downloaded,
            total: event.total,
        };
        let _ = app_handle.emit("driver-install-progress", payload);
    })
    .await
}

/// Uninstall a driver (removes JAR and state.json entry).
#[tauri::command]
pub async fn uninstall_driver_cmd(db_type: String) -> Result<(), String> {
    uninstall_driver(&am(), &db_type).await
}

/// Returns disk usage breakdown for `~/.seryai/drivers/`.
#[tauri::command]
pub async fn get_driver_store_usage() -> Result<DriverStoreUsage, String> {
    Ok(am().collect_driver_store_usage())
}

/// Returns the current Java runtime configuration.
#[tauri::command]
pub async fn get_java_runtime_config() -> Result<JavaRuntimeConfig, String> {
    Ok(am().load_state().java_runtime)
}

/// Saves the Java runtime configuration (validates custom path before saving).
#[tauri::command]
pub async fn set_java_runtime_config(mut config: JavaRuntimeConfig) -> Result<JavaRuntimeConfig, String> {
    let mgr = am();
    if config.mode == JavaRuntimeMode::Custom || config.mode == JavaRuntimeMode::System {
        let candidate_state =
            db_core::agent_manager::AgentState { java_runtime: config.clone(), ..mgr.load_state() };
        let resolved = mgr.resolve_java_runtime(&candidate_state, DEFAULT_JRE_KEY)?;
        if config.mode == JavaRuntimeMode::Custom {
            config.custom_java_path = Some(resolved.to_string_lossy().to_string());
        }
    }
    if config.mode != JavaRuntimeMode::Custom {
        config.custom_java_path = None;
    }
    let mut local_state = mgr.load_state();
    local_state.java_runtime = config.clone();
    mgr.save_state(&local_state)?;
    mgr.stop_daemons().await;
    Ok(config)
}

/// Invalidates the in-memory registry cache so the next list_drivers call
/// fetches fresh data from GitHub.
#[tauri::command]
pub async fn invalidate_driver_registry_cache() -> Result<(), String> {
    invalidate_registry_cache().await;
    Ok(())
}

/// Import a driver JAR from the local filesystem (offline install).
#[tauri::command]
pub async fn import_driver_jar_cmd(db_type: String, path: String) -> Result<(), String> {
    import_driver_jar(&am(), &db_type, Path::new(&path))
}

// ──────────── Event payload types ────────────

/// Progress payload emitted as `driver-install-progress` during install.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DriverInstallProgressPayload {
    pub db_type: String,
    pub step: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloaded: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

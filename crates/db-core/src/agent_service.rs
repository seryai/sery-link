use std::path::{Path, PathBuf};

use crate::agent_catalog;
use crate::agent_manager::{
    AgentManager, AgentRegistry, DriverStatus, InstalledDriver, JavaRuntimeMode, DEFAULT_JRE_KEY,
};

/// Primary registry URL — points at the sery-drivers release.
const REGISTRY_URL: &str =
    "https://github.com/seryai/sery-drivers/releases/latest/download/driver-registry.json";

static REGISTRY_CACHE: std::sync::LazyLock<tokio::sync::Mutex<Option<(std::time::Instant, AgentRegistry)>>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(None));

// ──────────── Progress events ────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct DriverProgressEvent {
    pub step: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloaded: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_type: Option<String>,
}

impl DriverProgressEvent {
    pub fn step(step: impl Into<String>) -> Self {
        Self { step: step.into(), downloaded: None, total: None, db_type: None }
    }

    pub fn transfer(step: impl Into<String>, downloaded: u64, total: u64) -> Self {
        Self { downloaded: Some(downloaded), total: Some(total), ..Self::step(step) }
    }

    pub fn with_db_type(mut self, db_type: Option<&str>) -> Self {
        self.db_type = db_type.map(ToString::to_string);
        self
    }
}

// ──────────── Registry helpers ────────────

pub async fn fetch_registry() -> Result<AgentRegistry, String> {
    {
        let cache = REGISTRY_CACHE.lock().await;
        if let Some((ts, registry)) = cache.as_ref() {
            if ts.elapsed() < std::time::Duration::from_secs(300) {
                return Ok(registry.clone());
            }
        }
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|err| format!("Failed to create HTTP client: {err}"))?;
    let resp = client
        .get(REGISTRY_URL)
        .header(reqwest::header::USER_AGENT, "sery-link-driver-manager")
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|err| format!("Failed to fetch driver registry: {err}"))?;
    let registry: AgentRegistry =
        resp.json().await.map_err(|err| format!("Failed to parse driver registry: {err}"))?;
    *REGISTRY_CACHE.lock().await = Some((std::time::Instant::now(), registry.clone()));
    Ok(registry)
}

pub async fn invalidate_registry_cache() {
    *REGISTRY_CACHE.lock().await = None;
}

// ──────────── Driver list builder ────────────

pub fn build_driver_list(am: &AgentManager, registry: Option<&AgentRegistry>) -> Vec<DriverStatus> {
    let local_state = am.load_state();
    agent_catalog::driver_store_entries()
        .map(|(key, label)| {
            let installed = am.is_driver_installed(key);
            let local = local_state.installed_drivers.get(key);
            let remote = registry.and_then(|r| r.drivers.get(key));
            let jre_key = remote
                .map(|r| r.jre.clone())
                .or_else(|| local.map(|l| l.jre.clone()))
                .unwrap_or_else(|| DEFAULT_JRE_KEY.to_string());
            let remote_jre_version = registry.and_then(|r| r.resolve_jre(&jre_key)).map(|j| &j.version);
            let local_jre_version = local_state.jre_versions.get(&jre_key);
            let jre_update_available = installed
                && (!am.is_jre_installed(&jre_key)
                    || remote_jre_version
                        .is_some_and(|version| local_jre_version != Some(version)));
            DriverStatus {
                db_type: key.to_string(),
                label: label.to_string(),
                version: remote.map(|r| r.version.clone()).unwrap_or_default(),
                size: remote.map(|r| r.jar.size).unwrap_or(0),
                installed,
                installed_version: local.map(|l| l.version.clone()),
                update_available: match (local, remote) {
                    (Some(l), Some(r)) => l.version != r.version || jre_update_available,
                    _ => false,
                },
                jre: jre_key.clone(),
                jre_installed: am.is_jre_installed(&jre_key),
            }
        })
        .collect()
}

pub fn jre_needs_install(am: &AgentManager, registry: &AgentRegistry, jre_key: &str) -> bool {
    let state = am.load_state();
    if state.java_runtime.mode != JavaRuntimeMode::Managed {
        return false;
    }
    if !am.is_jre_installed(jre_key) {
        return true;
    }
    registry.resolve_jre(jre_key).is_some_and(|jre| state.jre_versions.get(jre_key) != Some(&jre.version))
}

// ──────────── Install / uninstall ────────────

pub async fn install_driver(
    am: &AgentManager,
    db_type: &str,
    progress: impl Fn(DriverProgressEvent),
) -> Result<(), String> {
    match fetch_registry().await {
        Ok(registry) => install_driver_from_registry(am, &registry, db_type, &progress).await,
        Err(registry_err) => Err(registry_err),
    }
}

async fn install_driver_from_registry(
    am: &AgentManager,
    registry: &AgentRegistry,
    db_type: &str,
    progress: &impl Fn(DriverProgressEvent),
) -> Result<(), String> {
    let Some(driver) = registry.drivers.get(db_type) else {
        return Err(format!("Unknown driver type: {db_type}"));
    };
    let jre_key = &driver.jre;
    let needs_jre = jre_needs_install(am, registry, jre_key);

    if needs_jre {
        let jre_info =
            registry.resolve_jre(jre_key).ok_or_else(|| format!("No JRE definition for version: {jre_key}"))?;
        let platform = AgentManager::current_platform();
        let platform_jre = jre_info
            .platforms
            .get(platform)
            .ok_or_else(|| format!("No JRE {jre_key} available for platform: {platform}"))?;
        let jre_archive = am.base_dir().join("jre-download.tar.gz");
        progress(DriverProgressEvent::transfer("jre", 0, platform_jre.size).with_db_type(Some(db_type)));
        download_with_progress(
            progress,
            "jre",
            &platform_jre.url,
            &jre_archive,
            platform_jre.size,
            Some(db_type),
        )
        .await?;
        progress(DriverProgressEvent::transfer("jre-extract", 0, 0).with_db_type(Some(db_type)));
        let jre_dir = am.jre_dir(jre_key);
        if jre_dir.exists() {
            std::fs::remove_dir_all(&jre_dir).map_err(|err| format!("Failed to remove old JRE: {err}"))?;
        }
        extract_tar_gz(&jre_archive, &jre_dir)?;
        std::fs::remove_file(&jre_archive).ok();
    }

    let jar_path = am.driver_jar_path(db_type);
    progress(DriverProgressEvent::transfer("driver", 0, driver.jar.size).with_db_type(Some(db_type)));
    download_with_progress(progress, "driver", &driver.jar.url, &jar_path, driver.jar.size, Some(db_type)).await?;

    let mut local_state = am.load_state();
    if let Some(jre_info) = registry.resolve_jre(jre_key) {
        local_state.jre_versions.insert(jre_key.clone(), jre_info.version.clone());
    }
    local_state.installed_drivers.insert(
        db_type.to_string(),
        InstalledDriver {
            version: driver.version.clone(),
            installed_at: chrono::Utc::now().to_rfc3339(),
            jre: jre_key.clone(),
        },
    );
    am.save_state(&local_state)?;
    am.stop_daemon_by_key(db_type).await;
    progress(DriverProgressEvent::step("done"));
    Ok(())
}

pub async fn uninstall_driver(am: &AgentManager, db_type: &str) -> Result<(), String> {
    let jar_path = am.driver_jar_path(db_type);
    if jar_path.exists() {
        std::fs::remove_file(&jar_path).map_err(|err| err.to_string())?;
    }
    if let Some(driver_dir) = jar_path.parent() {
        if driver_dir.exists() {
            std::fs::remove_dir_all(driver_dir).map_err(|err| err.to_string())?;
        }
    }
    let mut local_state = am.load_state();
    local_state.installed_drivers.remove(db_type);
    am.save_state(&local_state)?;
    am.stop_daemon_by_key(db_type).await;
    Ok(())
}

pub fn install_local_driver(am: &AgentManager, db_type: &str, source: PathBuf) -> Result<(), String> {
    let jar_path = am.driver_jar_path(db_type);
    let parent = jar_path.parent().ok_or_else(|| format!("Invalid driver path: {}", jar_path.display()))?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    std::fs::copy(&source, &jar_path).map_err(|e| format!("Failed to copy local driver jar: {e}"))?;

    let mut local_state = am.load_state();
    local_state.installed_drivers.insert(
        db_type.to_string(),
        InstalledDriver {
            version: "0.1.0-local".to_string(),
            installed_at: chrono::Utc::now().to_rfc3339(),
            jre: DEFAULT_JRE_KEY.to_string(),
        },
    );
    am.save_state(&local_state)
}

pub fn import_driver_jar(am: &AgentManager, db_type: &str, jar_path: &Path) -> Result<(), String> {
    if !jar_path.exists() {
        return Err(format!("File not found: {}", jar_path.display()));
    }
    install_local_driver(am, db_type, jar_path.to_path_buf())
}

// ──────────── Download helpers ────────────

async fn download_with_progress(
    progress: &impl Fn(DriverProgressEvent),
    step: &str,
    url: &str,
    dest: &Path,
    total_size: u64,
    db_type: Option<&str>,
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let tmp = dest.with_extension("download");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|err| format!("Failed to create HTTP client: {err}"))?;
    let mut resp = client
        .get(url)
        .header(reqwest::header::USER_AGENT, "sery-link-driver-manager")
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|err| format!("Failed to download {url}: {err}"))?;
    let content_length = resp.content_length().unwrap_or(total_size);
    let mut file = std::fs::File::create(&tmp).map_err(|err| format!("Failed to create temp file: {err}"))?;
    let mut downloaded = 0u64;
    while let Some(chunk) =
        resp.chunk().await.map_err(|err| format!("Download stream error: {err}"))?
    {
        std::io::Write::write_all(&mut file, &chunk)
            .map_err(|err| format!("Failed to write chunk: {err}"))?;
        downloaded += chunk.len() as u64;
        progress(
            DriverProgressEvent::transfer(step, downloaded, content_length).with_db_type(db_type),
        );
    }
    std::io::Write::flush(&mut file).map_err(|err| format!("Failed to flush temp file: {err}"))?;
    drop(file);
    replace_download(&tmp, dest)
}

fn replace_download(tmp: &Path, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if dest.exists() {
        let backup = dest.with_extension("backup");
        std::fs::rename(dest, &backup).map_err(|e| format!("Failed to back up existing file: {e}"))?;
        match std::fs::rename(tmp, dest) {
            Ok(()) => {
                std::fs::remove_file(&backup).ok();
                Ok(())
            }
            Err(err) => {
                let _ = std::fs::rename(&backup, dest);
                Err(format!("Failed to replace downloaded file: {err}"))
            }
        }
    } else {
        std::fs::rename(tmp, dest)
            .map_err(|e| format!("Failed to move downloaded file into place: {e}"))
    }
}

fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let status = std::process::Command::new("tar")
        .args(["xzf", &archive.to_string_lossy(), "-C", &dest.to_string_lossy(), "--strip-components=1"])
        .status()
        .map_err(|e| format!("Failed to extract archive: {e}"))?;
    if !status.success() {
        return Err("Failed to extract JRE archive".to_string());
    }
    Ok(())
}

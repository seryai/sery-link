//! Credential storage — ~/.seryai/.credentials.json
//!
//! Stores access_token, workspace_key, and machine_id as a plain JSON
//! file owned by the user. Same security model as ~/.aws/credentials
//! or ~/.netrc: readable only by the user, no OS prompts, works on
//! headless/dev builds without a signed binary.
//!
//! Replaces the previous OS keychain backend which triggered a macOS
//! authorization prompt for each keychain entry on ad-hoc-signed builds.

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::error::{AgentError, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Credentials {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workspace_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    machine_id: Option<String>,
}

/// In-process cache so callers don't hit the filesystem on every read.
static CACHE: Lazy<Mutex<Option<Credentials>>> = Lazy::new(|| Mutex::new(None));

fn creds_path() -> Result<PathBuf> {
    dirs::home_dir()
        .map(|h| h.join(".seryai").join(".credentials.json"))
        .ok_or_else(|| AgentError::Config("no home directory".to_string()))
}

fn load() -> Credentials {
    let mut guard = CACHE.lock().expect("CACHE poisoned");
    if let Some(ref c) = *guard {
        return c.clone();
    }
    let creds = match creds_path().and_then(|p| {
        if p.exists() {
            let txt = fs::read_to_string(&p)
                .map_err(|e| AgentError::Config(format!("read credentials: {e}")))?;
            serde_json::from_str(&txt)
                .map_err(|e| AgentError::Config(format!("parse credentials: {e}")))
        } else {
            Ok(Credentials::default())
        }
    }) {
        Ok(c) => c,
        Err(_) => Credentials::default(),
    };
    *guard = Some(creds.clone());
    creds
}

fn save_creds(creds: &Credentials) -> Result<()> {
    let path = creds_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| AgentError::Config(format!("create .seryai dir: {e}")))?;
    }
    let json = serde_json::to_string_pretty(creds)
        .map_err(|e| AgentError::Config(format!("serialize credentials: {e}")))?;
    fs::write(&path, json)
        .map_err(|e| AgentError::Config(format!("write credentials: {e}")))?;
    // Set permissions to 0600 (owner read/write only).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    *CACHE.lock().expect("CACHE poisoned") = Some(creds.clone());
    Ok(())
}

fn mutate<F: FnOnce(&mut Credentials)>(f: F) -> Result<()> {
    let mut creds = load();
    f(&mut creds);
    save_creds(&creds)
}

// ── Public API (same signatures as the old keychain version) ──────────────

pub fn save_token(token: &str) -> Result<()> {
    mutate(|c| c.access_token = Some(token.to_string()))
}

pub fn get_token() -> Result<String> {
    load()
        .access_token
        .ok_or_else(|| AgentError::Keyring("No access token stored".to_string()))
}

pub fn delete_token() -> Result<()> {
    mutate(|c| c.access_token = None)
}

pub fn has_token() -> bool {
    load().access_token.is_some()
}

pub fn save_workspace_key(key: &str) -> Result<()> {
    mutate(|c| c.workspace_key = Some(key.to_string()))
}

pub fn get_workspace_key() -> Result<String> {
    load()
        .workspace_key
        .ok_or_else(|| AgentError::Keyring("No workspace key stored".to_string()))
}

/// Returns the stable machine identity UUID. Creates one on first run.
/// Migrates from the config-file fallback if the credentials file is new.
pub fn get_or_create_machine_id(config_fallback: &str) -> String {
    let creds = load();
    if let Some(id) = creds.machine_id.filter(|s| !s.is_empty()) {
        return id;
    }
    // Not in credentials file — use config fallback or generate fresh.
    let id = if !config_fallback.is_empty() {
        config_fallback.to_string()
    } else {
        uuid::Uuid::new_v4().to_string()
    };
    let _ = mutate(|c| c.machine_id = Some(id.clone()));
    id
}

//! F49 — OneDrive credentials in the OS keychain. Mirrors the
//! other cache-and-scan creds modules. The payload here is more
//! than a static token: access_token + refresh_token + expires_at,
//! refreshed in place by `onedrive::refresh_access_token`.

use crate::error::{AgentError, Result};
use crate::onedrive::OneDriveCredentials;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

const SERVICE: &str = "sery-link-onedrive";

static CRED_CACHE: Lazy<Mutex<HashMap<String, OneDriveCredentials>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn save(source_id: &str, creds: &OneDriveCredentials) -> Result<()> {
    if !creds.is_valid() {
        return Err(AgentError::Config(
            "OneDrive credentials need access + refresh tokens".to_string(),
        ));
    }
    let entry = keyring::Entry::new(SERVICE, source_id)
        .map_err(|e| AgentError::Config(format!("keyring entry: {e}")))?;
    let json = serde_json::to_string(creds)
        .map_err(|e| AgentError::Serialization(format!("serialize creds: {e}")))?;
    entry
        .set_password(&json)
        .map_err(|e| AgentError::Config(format!("keyring write: {e}")))?;
    CRED_CACHE
        .lock()
        .expect("CRED_CACHE poisoned")
        .insert(source_id.to_string(), creds.clone());
    Ok(())
}

pub fn load(source_id: &str) -> Result<Option<OneDriveCredentials>> {
    if let Some(cached) = CRED_CACHE
        .lock()
        .expect("CRED_CACHE poisoned")
        .get(source_id)
    {
        return Ok(Some(cached.clone()));
    }
    let entry = match keyring::Entry::new(SERVICE, source_id) {
        Ok(e) => e,
        Err(e) => return Err(AgentError::Config(format!("keyring entry: {e}"))),
    };
    match entry.get_password() {
        Ok(json) => {
            let creds: OneDriveCredentials =
                serde_json::from_str(&json).map_err(|e| {
                    AgentError::Serialization(format!("parse creds: {e}"))
                })?;
            CRED_CACHE
                .lock()
                .expect("CRED_CACHE poisoned")
                .insert(source_id.to_string(), creds.clone());
            Ok(Some(creds))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AgentError::Config(format!("keyring read: {e}"))),
    }
}

pub fn delete(source_id: &str) -> Result<()> {
    let entry = match keyring::Entry::new(SERVICE, source_id) {
        Ok(e) => e,
        Err(e) => return Err(AgentError::Config(format!("keyring entry: {e}"))),
    };
    let result = match entry.delete_password() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AgentError::Config(format!("keyring delete: {e}"))),
    };
    CRED_CACHE
        .lock()
        .expect("CRED_CACHE poisoned")
        .remove(source_id);
    result
}

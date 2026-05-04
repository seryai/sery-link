//! F44 — WebDAV credentials in the OS keychain. Keyed on source_id
//! from the start (post-F42). Mirrors `sftp_creds` shape.

use crate::error::{AgentError, Result};
use crate::webdav::WebDavCredentials;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

const SERVICE: &str = "sery-link-webdav";

static CRED_CACHE: Lazy<Mutex<HashMap<String, WebDavCredentials>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn save(source_id: &str, creds: &WebDavCredentials) -> Result<()> {
    if !creds.is_valid() {
        return Err(AgentError::Config(
            "WebDAV credentials need server URL and an auth payload".to_string(),
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

pub fn load(source_id: &str) -> Result<Option<WebDavCredentials>> {
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
            let creds: WebDavCredentials =
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

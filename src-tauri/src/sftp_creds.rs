//! F43 — SFTP credentials in the OS keychain.
//!
//! Mirrors `remote_creds` (S3) but keys on `source_id` from the
//! start, not URL. F43 lands after the F42 sources sidebar so we
//! get to use the new keying scheme without a migration. The
//! payload is a serialized `SftpCredentials` JSON in the keyring
//! Service `sery-link-sftp`, account `<source_id>`.
//!
//! The same process-wide cache pattern remote_creds uses to
//! prevent prompt-storm scans applies here.

use crate::error::{AgentError, Result};
use crate::sftp::SftpCredentials;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

const SERVICE: &str = "sery-link-sftp";

// Process-wide cache: source_id → creds.
static CRED_CACHE: Lazy<Mutex<HashMap<String, SftpCredentials>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Save SFTP credentials keyed on the source's UUID. Overwrites any
/// existing entry for the same source_id (used by Edit credentials).
pub fn save(source_id: &str, creds: &SftpCredentials) -> Result<()> {
    if !creds.is_valid() {
        return Err(AgentError::Config(
            "SFTP credentials need host, username, and an auth payload"
                .to_string(),
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

/// Load SFTP credentials for a source_id. Returns Ok(None) when no
/// entry exists.
pub fn load(source_id: &str) -> Result<Option<SftpCredentials>> {
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
            let creds: SftpCredentials =
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

/// Delete credentials for a source_id. Idempotent on missing.
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

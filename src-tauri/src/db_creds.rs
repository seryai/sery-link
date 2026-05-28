//! F52 — Database source credentials in the OS keychain.
//!
//! Stores only the password (a plain string) for each DB source,
//! keyed on source_id. Connection metadata (host, port, username,
//! database) lives in SourceKind — never the password.
//!
//! Pattern mirrors sftp_creds.rs.

use crate::error::{AgentError, Result};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

const SERVICE: &str = "sery-link-db";

static CRED_CACHE: Lazy<Mutex<HashMap<String, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Save the DB password keyed on source_id. Overwrites on re-connect.
pub fn save(source_id: &str, password: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, source_id)
        .map_err(|e| AgentError::Config(format!("keyring entry: {e}")))?;
    entry
        .set_password(password)
        .map_err(|e| AgentError::Config(format!("keyring write: {e}")))?;
    CRED_CACHE
        .lock()
        .expect("CRED_CACHE poisoned")
        .insert(source_id.to_string(), password.to_string());
    Ok(())
}

/// Load the DB password for a source_id. Returns Ok(None) when absent.
pub fn load(source_id: &str) -> Result<Option<String>> {
    if let Some(cached) = CRED_CACHE
        .lock()
        .expect("CRED_CACHE poisoned")
        .get(source_id)
    {
        return Ok(Some(cached.clone()));
    }
    let entry = keyring::Entry::new(SERVICE, source_id)
        .map_err(|e| AgentError::Config(format!("keyring entry: {e}")))?;
    match entry.get_password() {
        Ok(pw) => {
            CRED_CACHE
                .lock()
                .expect("CRED_CACHE poisoned")
                .insert(source_id.to_string(), pw.clone());
            Ok(Some(pw))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AgentError::Config(format!("keyring read: {e}"))),
    }
}

/// Delete credentials for a source_id. Idempotent on missing.
pub fn delete(source_id: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, source_id)
        .map_err(|e| AgentError::Config(format!("keyring entry: {e}")))?;
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

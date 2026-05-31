//! F52 — Database source credentials in the OS keychain.
//!
//! All DB passwords are stored in ONE keychain entry (service =
//! "sery-link-db", account = "db-passwords") as a JSON object keyed
//! by source_id. Using a single entry means macOS only ever asks the
//! user to authorize access once — not once per database source.
//!
//! The in-process `CRED_CACHE` mirrors the full map so individual
//! load/save calls don't round-trip the keychain after the first read.

use crate::error::{AgentError, Result};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

const SERVICE: &str = "sery-link-db";
const ACCOUNT: &str = "db-passwords";

/// In-process copy of the full source_id → password map. Populated on
/// first keychain read; updated on every save/delete without re-reading.
static CRED_CACHE: Lazy<Mutex<Option<HashMap<String, String>>>> =
    Lazy::new(|| Mutex::new(None));

/// Read the full map from the keychain. Returns an empty map when no
/// entry exists yet. Propagates real keychain errors.
fn read_map_from_keychain() -> Result<HashMap<String, String>> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|e| AgentError::Config(format!("keyring entry: {e}")))?;
    match entry.get_password() {
        Ok(json) => serde_json::from_str(&json)
            .map_err(|e| AgentError::Config(format!("db_creds parse: {e}"))),
        Err(keyring::Error::NoEntry) => Ok(HashMap::new()),
        Err(e) => Err(AgentError::Config(format!("keyring read: {e}"))),
    }
}

/// Write the full map back to the single keychain entry.
fn write_map_to_keychain(map: &HashMap<String, String>) -> Result<()> {
    let json = serde_json::to_string(map)
        .map_err(|e| AgentError::Config(format!("db_creds serialize: {e}")))?;
    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|e| AgentError::Config(format!("keyring entry: {e}")))?;
    entry
        .set_password(&json)
        .map_err(|e| AgentError::Config(format!("keyring write: {e}")))
}

/// Ensure `CRED_CACHE` is populated. This triggers at most one keychain
/// read per process lifetime — the single macOS authorization prompt.
fn ensure_cache(guard: &mut Option<HashMap<String, String>>) -> Result<()> {
    if guard.is_none() {
        *guard = Some(read_map_from_keychain()?);
    }
    Ok(())
}

/// Save the DB password keyed on source_id.
pub fn save(source_id: &str, password: &str) -> Result<()> {
    let mut guard = CRED_CACHE.lock().expect("CRED_CACHE poisoned");
    ensure_cache(&mut guard)?;
    let map = guard.as_mut().expect("cache populated above");
    map.insert(source_id.to_string(), password.to_string());
    write_map_to_keychain(map)
}

/// Load the DB password for a source_id. Returns Ok(None) when absent.
pub fn load(source_id: &str) -> Result<Option<String>> {
    let mut guard = CRED_CACHE.lock().expect("CRED_CACHE poisoned");
    ensure_cache(&mut guard)?;
    Ok(guard
        .as_ref()
        .expect("cache populated above")
        .get(source_id)
        .cloned())
}

/// Delete credentials for a source_id. Idempotent on missing.
pub fn delete(source_id: &str) -> Result<()> {
    let mut guard = CRED_CACHE.lock().expect("CRED_CACHE poisoned");
    ensure_cache(&mut guard)?;
    let map = guard.as_mut().expect("cache populated above");
    if map.remove(source_id).is_none() {
        return Ok(());
    }
    if map.is_empty() {
        // Remove the keychain entry entirely when no passwords remain.
        let entry = keyring::Entry::new(SERVICE, ACCOUNT)
            .map_err(|e| AgentError::Config(format!("keyring entry: {e}")))?;
        match entry.delete_password() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AgentError::Config(format!("keyring delete: {e}"))),
        }
    } else {
        write_map_to_keychain(map)
    }
}

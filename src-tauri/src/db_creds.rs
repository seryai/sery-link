//! F52 — Database source credentials in ~/.seryai/.vault.json.
//!
//! All DB passwords are stored in ONE vault entry (service =
//! "sery-link-db", account = "db-passwords") as a JSON object keyed
//! by source_id. One entry means no per-source authorization prompts.

use crate::error::{AgentError, Result};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

const SERVICE: &str = "sery-link-db";
const ACCOUNT: &str = "db-passwords";

static CRED_CACHE: Lazy<Mutex<Option<HashMap<String, String>>>> =
    Lazy::new(|| Mutex::new(None));

fn read_map() -> Result<HashMap<String, String>> {
    let entry = crate::cred_store::Entry::new(SERVICE, ACCOUNT)
        .map_err(|e| AgentError::Config(format!("cred_store entry: {e}")))?;
    match entry.get_password() {
        Ok(json) => serde_json::from_str(&json)
            .map_err(|e| AgentError::Config(format!("db_creds parse: {e}"))),
        Err(crate::cred_store::Error::NoEntry) => Ok(HashMap::new()),
        Err(e) => Err(AgentError::Config(format!("cred_store read: {e}"))),
    }
}

fn write_map(map: &HashMap<String, String>) -> Result<()> {
    let json = serde_json::to_string(map)
        .map_err(|e| AgentError::Config(format!("db_creds serialize: {e}")))?;
    let entry = crate::cred_store::Entry::new(SERVICE, ACCOUNT)
        .map_err(|e| AgentError::Config(format!("cred_store entry: {e}")))?;
    entry
        .set_password(&json)
        .map_err(|e| AgentError::Config(format!("cred_store write: {e}")))
}

fn ensure_cache(guard: &mut Option<HashMap<String, String>>) -> Result<()> {
    if guard.is_none() {
        *guard = Some(read_map()?);
    }
    Ok(())
}

pub fn save(source_id: &str, password: &str) -> Result<()> {
    let mut guard = CRED_CACHE.lock().expect("CRED_CACHE poisoned");
    ensure_cache(&mut guard)?;
    let map = guard.as_mut().expect("cache populated above");
    map.insert(source_id.to_string(), password.to_string());
    write_map(map)
}

pub fn load(source_id: &str) -> Result<Option<String>> {
    let mut guard = CRED_CACHE.lock().expect("CRED_CACHE poisoned");
    ensure_cache(&mut guard)?;
    Ok(guard
        .as_ref()
        .expect("cache populated above")
        .get(source_id)
        .cloned())
}

pub fn delete(source_id: &str) -> Result<()> {
    let mut guard = CRED_CACHE.lock().expect("CRED_CACHE poisoned");
    ensure_cache(&mut guard)?;
    let map = guard.as_mut().expect("cache populated above");
    if map.remove(source_id).is_none() {
        return Ok(());
    }
    if map.is_empty() {
        let entry = crate::cred_store::Entry::new(SERVICE, ACCOUNT)
            .map_err(|e| AgentError::Config(format!("cred_store entry: {e}")))?;
        match entry.delete_password() {
            Ok(()) | Err(crate::cred_store::Error::NoEntry) => Ok(()),
            Err(e) => Err(AgentError::Config(format!("cred_store delete: {e}"))),
        }
    } else {
        write_map(map)
    }
}

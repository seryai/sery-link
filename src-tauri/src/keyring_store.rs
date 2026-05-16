use keyring::Entry;
use once_cell::sync::Lazy;
use std::sync::Mutex;
use crate::error::{AgentError, Result};

const SERVICE_NAME: &str = "seryai-agent";
const ACCESS_TOKEN_KEY: &str = "access_token";
const WORKSPACE_KEY_KEY: &str = "workspace_key";
const MACHINE_ID_KEY: &str = "machine_id";

// Process-wide token cache. macOS prompts the user *every* keychain
// read on ad-hoc-signed builds (no stable code signature for the OS
// to bind "Always Allow" against), so the same launch hitting
// `get_token` twice prompts twice. Cache the value once we've decrypted
// it so the rest of the session is silent. Cleared on save/delete so
// the cache can never go stale relative to the keychain.
static TOKEN_CACHE: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

pub fn save_token(token: &str) -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, ACCESS_TOKEN_KEY)
        .map_err(|e| AgentError::Keyring(format!("Failed to create keyring entry: {}", e)))?;

    entry
        .set_password(token)
        .map_err(|e| AgentError::Keyring(format!("Failed to save token: {}", e)))?;

    *TOKEN_CACHE.lock().expect("TOKEN_CACHE poisoned") = Some(token.to_string());
    Ok(())
}

pub fn get_token() -> Result<String> {
    if let Some(cached) = TOKEN_CACHE.lock().expect("TOKEN_CACHE poisoned").as_ref() {
        return Ok(cached.clone());
    }

    let entry = Entry::new(SERVICE_NAME, ACCESS_TOKEN_KEY)
        .map_err(|e| AgentError::Keyring(format!("Failed to create keyring entry: {}", e)))?;

    let token = entry
        .get_password()
        .map_err(|e| AgentError::Keyring(format!("Failed to retrieve token: {}", e)))?;

    *TOKEN_CACHE.lock().expect("TOKEN_CACHE poisoned") = Some(token.clone());
    Ok(token)
}

pub fn delete_token() -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, ACCESS_TOKEN_KEY)
        .map_err(|e| AgentError::Keyring(format!("Failed to create keyring entry: {}", e)))?;

    entry
        .delete_password()
        .map_err(|e| AgentError::Keyring(format!("Failed to delete token: {}", e)))?;

    *TOKEN_CACHE.lock().expect("TOKEN_CACHE poisoned") = None;
    Ok(())
}

pub fn has_token() -> bool {
    get_token().is_ok()
}

pub fn save_workspace_key(key: &str) -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, WORKSPACE_KEY_KEY)
        .map_err(|e| AgentError::Keyring(format!("Failed to create keyring entry: {}", e)))?;
    entry
        .set_password(key)
        .map_err(|e| AgentError::Keyring(format!("Failed to save workspace key: {}", e)))?;
    Ok(())
}

pub fn get_workspace_key() -> Result<String> {
    let entry = Entry::new(SERVICE_NAME, WORKSPACE_KEY_KEY)
        .map_err(|e| AgentError::Keyring(format!("Failed to create keyring entry: {}", e)))?;
    entry
        .get_password()
        .map_err(|e| AgentError::Keyring(format!("Failed to retrieve workspace key: {}", e)))
}

/// Returns the stable machine identity UUID, creating and persisting one if
/// this is the first run (or the keyring was wiped). The `config_fallback`
/// is the value already stored in the config file — used to migrate existing
/// installs that have a machine_id in config but not yet in the keyring.
pub fn get_or_create_machine_id(config_fallback: &str) -> String {
    let entry = match Entry::new(SERVICE_NAME, MACHINE_ID_KEY) {
        Ok(e) => e,
        Err(_) => return config_fallback.to_string(),
    };

    // Keyring has it — return it.
    if let Ok(id) = entry.get_password() {
        if !id.is_empty() {
            return id;
        }
    }

    // Not in keyring. Prefer the value already in config (migration path for
    // users upgrading from a build that stored machine_id only in config).
    let id = if !config_fallback.is_empty() {
        config_fallback.to_string()
    } else {
        uuid::Uuid::new_v4().to_string()
    };

    let _ = entry.set_password(&id); // best-effort; config fallback still works
    id
}


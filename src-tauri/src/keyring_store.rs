use keyring::Entry;
use once_cell::sync::Lazy;
use std::sync::Mutex;
use crate::error::{AgentError, Result};

const SERVICE_NAME: &str = "seryai-agent";
const ACCESS_TOKEN_KEY: &str = "access_token";

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

// BYOK keyring helpers were removed in the v0.5.3 → file-manager
// pivot. AI now happens cloud-side via the dashboard, so the
// desktop no longer holds per-provider API keys. Existing keychain
// entries (`byok_<provider>`) are left in place — they're harmless
// and the user can clear them via Keychain Access if they want.

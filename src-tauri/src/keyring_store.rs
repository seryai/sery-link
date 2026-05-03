use keyring::Entry;
use crate::error::{AgentError, Result};

const SERVICE_NAME: &str = "seryai-agent";
const ACCESS_TOKEN_KEY: &str = "access_token";

pub fn save_token(token: &str) -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, ACCESS_TOKEN_KEY)
        .map_err(|e| AgentError::Keyring(format!("Failed to create keyring entry: {}", e)))?;

    entry
        .set_password(token)
        .map_err(|e| AgentError::Keyring(format!("Failed to save token: {}", e)))?;

    Ok(())
}

pub fn get_token() -> Result<String> {
    let entry = Entry::new(SERVICE_NAME, ACCESS_TOKEN_KEY)
        .map_err(|e| AgentError::Keyring(format!("Failed to create keyring entry: {}", e)))?;

    entry
        .get_password()
        .map_err(|e| AgentError::Keyring(format!("Failed to retrieve token: {}", e)))
}

pub fn delete_token() -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, ACCESS_TOKEN_KEY)
        .map_err(|e| AgentError::Keyring(format!("Failed to create keyring entry: {}", e)))?;

    entry
        .delete_password()
        .map_err(|e| AgentError::Keyring(format!("Failed to delete token: {}", e)))?;

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

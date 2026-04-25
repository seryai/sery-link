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

// ---------------------------------------------------------------------------
// BYOK (Bring Your Own Key) — separate keyring entry per provider so the
// workspace token and BYOK keys can coexist. Per-provider keying so a user
// can have both Anthropic and (future) OpenAI keys saved at once.
// ---------------------------------------------------------------------------

fn byok_account(provider: &str) -> String {
    format!("byok_{}", provider.to_lowercase())
}

pub fn save_byok_key(provider: &str, key: &str) -> Result<()> {
    let account = byok_account(provider);
    let entry = Entry::new(SERVICE_NAME, &account)
        .map_err(|e| AgentError::Keyring(format!("Failed to create BYOK entry: {}", e)))?;

    entry
        .set_password(key)
        .map_err(|e| AgentError::Keyring(format!("Failed to save BYOK key: {}", e)))?;

    Ok(())
}

pub fn get_byok_key(provider: &str) -> Result<String> {
    let account = byok_account(provider);
    let entry = Entry::new(SERVICE_NAME, &account)
        .map_err(|e| AgentError::Keyring(format!("Failed to create BYOK entry: {}", e)))?;

    entry
        .get_password()
        .map_err(|e| AgentError::Keyring(format!("Failed to retrieve BYOK key: {}", e)))
}

pub fn delete_byok_key(provider: &str) -> Result<()> {
    let account = byok_account(provider);
    let entry = Entry::new(SERVICE_NAME, &account)
        .map_err(|e| AgentError::Keyring(format!("Failed to create BYOK entry: {}", e)))?;

    entry
        .delete_password()
        .map_err(|e| AgentError::Keyring(format!("Failed to delete BYOK key: {}", e)))?;

    Ok(())
}

pub fn has_byok_key(provider: &str) -> bool {
    get_byok_key(provider).is_ok()
}

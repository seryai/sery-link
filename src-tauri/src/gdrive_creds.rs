//! Google OAuth tokens for the Drive connector. Stored in the OS
//! keychain via the `keyring` crate, mirroring the pattern in
//! `remote_creds.rs` but with a different SERVICE name and payload.
//!
//! Per Phase 3b of the cloud-connectors migration: tokens stay on the
//! user's machine, never reach Sery's api server. The api never asks
//! for them and has no endpoint that accepts them.
//!
//! Entry naming: `sery-link-gdrive` / `<account_id>`.
//!
//! For v1, `account_id` is the literal string `"default"` — one Drive
//! account per Sery Link install. Multi-account support (Phase 3c+)
//! will key on the Google user's email, fetched via a `userinfo` call
//! after the token exchange completes.

use crate::error::{AgentError, Result};
use chrono::{DateTime, Duration, Utc};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

const SERVICE: &str = "sery-link-gdrive";

// Process-wide cache: account_id → tokens. macOS prompts on every
// keychain read against an ad-hoc-signed binary, so navigating around
// the app would re-prompt for the same Drive account. Cache survives
// the session; save/delete invalidate the relevant entry.
static TOKEN_CACHE: Lazy<Mutex<HashMap<String, StoredTokens>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Tokens persisted to the keychain. Distinct from `TokenResponse` in
/// `gdrive_oauth.rs` because that's the wire shape (`expires_in` is
/// relative seconds); we store an absolute `DateTime<Utc>` so refresh
/// decisions don't depend on when we deserialised.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_at: DateTime<Utc>,
    pub scope: String,
}

impl StoredTokens {
    /// Build from Google's wire format. `refresh_token` may be missing
    /// in some edge cases — Google sometimes omits it on re-consent if
    /// the same Google account already has an active grant for this
    /// OAuth client (despite our `prompt=consent`). Returns Err in that
    /// case so the user gets an actionable message instead of a silent
    /// panic in the spawned OAuth task.
    pub fn from_token_response(
        resp: &crate::gdrive_oauth::TokenResponse,
    ) -> std::result::Result<Self, String> {
        let refresh = resp.refresh_token.clone().ok_or_else(|| {
            "Google didn't issue a refresh_token. This usually means the \
             account already has an active grant for Sery Link. Visit \
             https://myaccount.google.com/permissions, remove Sery Link's \
             access, then try Connect Google Drive again."
                .to_string()
        })?;
        Ok(Self {
            access_token: resp.access_token.clone(),
            refresh_token: refresh,
            access_expires_at: Utc::now() + Duration::seconds(resp.expires_in as i64),
            scope: resp.scope.clone(),
        })
    }

    /// Apply a refresh-flow response to existing stored tokens.
    /// Google returns a new access_token + scope + expires_in every
    /// time, but only sometimes returns a new refresh_token. When it
    /// does, replace the stored one; when it doesn't, keep the old.
    pub fn merge_refresh_response(
        &mut self,
        resp: &crate::gdrive_oauth::TokenResponse,
    ) {
        self.access_token = resp.access_token.clone();
        if let Some(new_refresh) = &resp.refresh_token {
            self.refresh_token = new_refresh.clone();
        }
        self.access_expires_at = Utc::now() + Duration::seconds(resp.expires_in as i64);
        self.scope = resp.scope.clone();
    }

    /// Will the access token still be valid in 60 seconds? Used by
    /// callers that want to refresh proactively rather than at the
    /// moment of failure (smoother UX during scans).
    pub fn is_fresh(&self) -> bool {
        Utc::now() + Duration::seconds(60) < self.access_expires_at
    }
}

pub fn save(account_id: &str, tokens: &StoredTokens) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, account_id)
        .map_err(|e| AgentError::Config(format!("keyring entry: {}", e)))?;
    let json = serde_json::to_string(tokens)
        .map_err(|e| AgentError::Serialization(format!("serialize tokens: {}", e)))?;
    entry
        .set_password(&json)
        .map_err(|e| AgentError::Config(format!("keyring write: {}", e)))?;
    TOKEN_CACHE
        .lock()
        .expect("TOKEN_CACHE poisoned")
        .insert(account_id.to_string(), tokens.clone());
    Ok(())
}

pub fn load(account_id: &str) -> Result<Option<StoredTokens>> {
    if let Some(cached) = TOKEN_CACHE
        .lock()
        .expect("TOKEN_CACHE poisoned")
        .get(account_id)
    {
        return Ok(Some(cached.clone()));
    }
    let entry = match keyring::Entry::new(SERVICE, account_id) {
        Ok(e) => e,
        Err(e) => return Err(AgentError::Config(format!("keyring entry: {}", e))),
    };
    match entry.get_password() {
        Ok(json) => {
            let tokens: StoredTokens = serde_json::from_str(&json).map_err(|e| {
                AgentError::Serialization(format!("parse tokens: {}", e))
            })?;
            TOKEN_CACHE
                .lock()
                .expect("TOKEN_CACHE poisoned")
                .insert(account_id.to_string(), tokens.clone());
            Ok(Some(tokens))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AgentError::Config(format!("keyring read: {}", e))),
    }
}

pub fn delete(account_id: &str) -> Result<()> {
    let entry = match keyring::Entry::new(SERVICE, account_id) {
        Ok(e) => e,
        Err(e) => return Err(AgentError::Config(format!("keyring entry: {}", e))),
    };
    let result = match entry.delete_password() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AgentError::Config(format!("keyring delete: {}", e))),
    };
    TOKEN_CACHE
        .lock()
        .expect("TOKEN_CACHE poisoned")
        .remove(account_id);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_response(refresh: Option<&str>) -> crate::gdrive_oauth::TokenResponse {
        crate::gdrive_oauth::TokenResponse {
            access_token: "ya29.fake".to_string(),
            refresh_token: refresh.map(|s| s.to_string()),
            expires_in: 3600,
            scope: "drive.readonly".to_string(),
            token_type: "Bearer".to_string(),
        }
    }

    #[test]
    fn from_response_captures_expiry() {
        let resp = fake_response(Some("rt-1"));
        let stored = StoredTokens::from_token_response(&resp).expect("ok");
        assert_eq!(stored.access_token, "ya29.fake");
        assert_eq!(stored.refresh_token, "rt-1");
        let delta = (stored.access_expires_at - Utc::now()).num_seconds();
        assert!((3590..=3600).contains(&delta), "delta was {}", delta);
    }

    #[test]
    fn from_response_errors_without_refresh_token() {
        let resp = fake_response(None);
        let err = StoredTokens::from_token_response(&resp).unwrap_err();
        assert!(
            err.contains("refresh_token") || err.contains("permissions"),
            "error message should mention the underlying issue: {}",
            err
        );
    }

    #[test]
    fn merge_keeps_old_refresh_when_new_is_none() {
        let mut stored =
            StoredTokens::from_token_response(&fake_response(Some("original-rt"))).expect("ok");
        let refresh_response = fake_response(None);
        stored.merge_refresh_response(&refresh_response);
        assert_eq!(stored.refresh_token, "original-rt");
    }

    #[test]
    fn merge_replaces_refresh_when_present() {
        let mut stored =
            StoredTokens::from_token_response(&fake_response(Some("original-rt"))).expect("ok");
        let refresh_response = fake_response(Some("rotated-rt"));
        stored.merge_refresh_response(&refresh_response);
        assert_eq!(stored.refresh_token, "rotated-rt");
    }

    #[test]
    fn is_fresh_respects_buffer() {
        let mut stored =
            StoredTokens::from_token_response(&fake_response(Some("rt"))).expect("ok");
        assert!(stored.is_fresh());
        stored.access_expires_at = Utc::now() + Duration::seconds(30);
        assert!(!stored.is_fresh());
        stored.access_expires_at = Utc::now() + Duration::seconds(120);
        assert!(stored.is_fresh());
    }
}

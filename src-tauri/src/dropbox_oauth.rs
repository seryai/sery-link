//! F48 — Dropbox OAuth (PKCE no-redirect flow).
//!
//! v0.7.0 ships PAT auth as the primary Dropbox path; this module
//! adds OAuth as the friendlier consumer alternative. PAT vs OAuth
//! trade-off:
//!   - PAT: user generates a token in dropbox.com/developers/apps;
//!     no token expiry; "developer" mental model is friction for
//!     non-power-users.
//!   - OAuth: user clicks "Sign in with Dropbox", clicks Allow,
//!     pastes a code Dropbox shows on its own page. Cleaner UX
//!     but needs a registered Dropbox app + token refresh logic.
//!
//! Why "no-redirect" (not localhost callback): Dropbox supports
//! "authorization code with no redirect URI" where, after the user
//! clicks Allow, Dropbox displays a code on its page that the user
//! copies back to the app. This avoids the deep-link / localhost
//! plumbing while still using the standard OAuth code grant.
//!
//! PKCE adds a code_verifier / code_challenge pair to the flow —
//! makes the Dropbox app's client_id safe to embed in the binary
//! (a stolen client_id alone can't redeem tokens without the
//! verifier from the user's session).
//!
//! Build-time gating: `DROPBOX_APP_KEY` env var must be set at
//! `cargo build` time (option_env!). Builds without it leave OAuth
//! disabled — start_oauth_flow surfaces a "not configured" error.
//! Same pattern as gdrive_oauth's GOOGLE_OAUTH_CLIENT_ID. See
//! datalake/SETUP_DROPBOX_OAUTH.md for app registration steps.

use crate::error::{AgentError, Result};
use chrono::{Duration as ChronoDuration, Utc};
use sha2::{Digest, Sha256};
use base64::Engine as _;
use serde::{Deserialize, Serialize};

/// Dropbox app key from the developer dashboard, embedded at build
/// time via the `DROPBOX_APP_KEY` env var. Public identifier — safe
/// to embed (Dropbox docs explicitly support this for PKCE-grant
/// apps that don't store a client_secret).
///
/// Builds without the env var leave this as `None`; callers must
/// surface a "not configured" error to the UI. Same pattern as
/// `gdrive_oauth::client_id()` for Google Drive.
///
/// To register: https://www.dropbox.com/developers/apps → Create
/// app → Scoped access → Full Dropbox. Required scopes:
/// `files.content.read`, `files.metadata.read`. Allow "PKCE
/// (recommended for mobile and desktop apps)" in OAuth 2 settings.
/// See `datalake/SETUP_DROPBOX_OAUTH.md` for the full runbook.
pub fn app_key() -> Option<&'static str> {
    option_env!("DROPBOX_APP_KEY")
        .filter(|s| !s.is_empty() && *s != "REPLACE_WITH_REAL_DROPBOX_APP_KEY")
}

const AUTHORIZE_URL: &str = "https://www.dropbox.com/oauth2/authorize";
const TOKEN_URL: &str = "https://api.dropboxapi.com/oauth2/token";

/// Stored OAuth tokens — return shape for `complete_oauth_flow`
/// and `refresh_access_token`. Callers convert into the persisted
/// `DropboxCredentials` shape (which carries the same fields plus
/// PAT compatibility); the expiry check lives there too.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropboxOAuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// RFC 3339 timestamp.
    pub expires_at: String,
}

/// Public state returned to the frontend when starting auth — the
/// browser URL to open + the verifier the frontend hands back when
/// completing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropboxAuthStart {
    pub authorize_url: String,
    /// PKCE verifier — frontend stores this in component state and
    /// passes it to complete_dropbox_oauth alongside the user-pasted
    /// code. Don't persist; lifecycle is the OAuth flow only.
    pub code_verifier: String,
}

/// Generate a PKCE code_verifier (43-128 char URL-safe random
/// string per RFC 7636) and the SHA256-derived code_challenge.
fn generate_pkce_pair() -> (String, String) {
    use rand::Rng;
    const CHARSET: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut rng = rand::thread_rng();
    let verifier: String = (0..96)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(hasher.finalize());
    (verifier, challenge)
}

/// Step 1 of the OAuth flow. Builds the authorize URL the frontend
/// opens in the user's browser. Returns the URL + verifier.
pub fn start_oauth_flow() -> Result<DropboxAuthStart> {
    let key = app_key().ok_or_else(|| {
        AgentError::Config(
            "Dropbox OAuth not yet configured — this build was \
             produced without DROPBOX_APP_KEY. Rebuild Sery Link \
             with the env var set. See \
             datalake/SETUP_DROPBOX_OAUTH.md."
                .to_string(),
        )
    })?;
    let (verifier, challenge) = generate_pkce_pair();
    // token_access_type=offline → Dropbox issues a refresh_token
    // alongside the access_token. Without this the access_token
    // expires after 4 hours with no recovery path.
    let url = format!(
        "{AUTHORIZE_URL}?client_id={}&response_type=code&token_access_type=offline&code_challenge={}&code_challenge_method=S256",
        key,
        urlencoding::encode(&challenge)
    );
    Ok(DropboxAuthStart {
        authorize_url: url,
        code_verifier: verifier,
    })
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    expires_in: i64,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("dropbox_oauth reqwest client builder")
}

/// Step 2 of the OAuth flow. Frontend supplies the user-pasted
/// code + the verifier from start_oauth_flow. Returns the tokens
/// to persist in the keychain.
pub async fn complete_oauth_flow(
    code: &str,
    code_verifier: &str,
) -> Result<DropboxOAuthTokens> {
    let key = app_key().ok_or_else(|| {
        AgentError::Config("Dropbox OAuth not configured (DROPBOX_APP_KEY)".to_string())
    })?;
    let resp = http_client()
        .post(TOKEN_URL)
        .form(&[
            ("code", code.trim()),
            ("grant_type", "authorization_code"),
            ("client_id", key),
            ("code_verifier", code_verifier),
        ])
        .send()
        .await
        .map_err(|e| {
            AgentError::Network(format!("Dropbox token exchange: {e}"))
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AgentError::Auth(format!(
            "Dropbox token exchange (HTTP {status}): {}",
            body.chars().take(300).collect::<String>()
        )));
    }
    let token: TokenResponse = resp.json().await.map_err(|e| {
        AgentError::Serialization(format!("parse token resp: {e}"))
    })?;
    if let Some(err) = token.error {
        return Err(AgentError::Auth(format!(
            "Dropbox auth error ({err}): {}",
            token.error_description.unwrap_or_default()
        )));
    }
    let refresh_token = token.refresh_token.unwrap_or_default();
    if refresh_token.is_empty() {
        return Err(AgentError::Auth(
            "Dropbox token response missing refresh_token — was \
             token_access_type=offline included in the authorize URL?"
                .to_string(),
        ));
    }
    let expires_at =
        (Utc::now() + ChronoDuration::seconds(token.expires_in)).to_rfc3339();
    Ok(DropboxOAuthTokens {
        access_token: token.access_token,
        refresh_token,
        expires_at,
    })
}

/// Refresh an expired access_token using the stored refresh_token.
/// Mutates the supplied tokens in place.
pub async fn refresh_access_token(tokens: &mut DropboxOAuthTokens) -> Result<()> {
    let key = app_key().ok_or_else(|| {
        AgentError::Config("Dropbox OAuth not configured (DROPBOX_APP_KEY)".to_string())
    })?;
    let resp = http_client()
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", tokens.refresh_token.as_str()),
            ("client_id", key),
        ])
        .send()
        .await
        .map_err(|e| {
            AgentError::Network(format!("Dropbox refresh: {e}"))
        })?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AgentError::Auth(format!(
            "Dropbox refresh failed (HTTP {status}): {} — re-auth needed",
            body.chars().take(300).collect::<String>()
        )));
    }
    let token: TokenResponse = resp.json().await.map_err(|e| {
        AgentError::Serialization(format!("parse refresh resp: {e}"))
    })?;
    tokens.access_token = token.access_token;
    if let Some(new_refresh) = token.refresh_token {
        tokens.refresh_token = new_refresh;
    }
    tokens.expires_at =
        (Utc::now() + ChronoDuration::seconds(token.expires_in)).to_rfc3339();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_pair_meets_spec() {
        let (v, c) = generate_pkce_pair();
        // Verifier must be 43-128 chars per RFC 7636
        assert!(v.len() >= 43 && v.len() <= 128, "verifier len {}", v.len());
        // Challenge is base64url(sha256(verifier)) — 43 chars when
        // unpadded.
        assert_eq!(c.len(), 43, "challenge len {}", c.len());
        // No padding chars
        assert!(!c.contains('='));
    }

    #[test]
    fn pkce_pair_is_random() {
        let (v1, _) = generate_pkce_pair();
        let (v2, _) = generate_pkce_pair();
        assert_ne!(v1, v2, "verifiers must differ across calls");
    }

    #[test]
    fn start_oauth_returns_error_when_app_key_unconfigured() {
        // Skip in builds that DO have the env var set — the test
        // only meaningfully covers the unconfigured branch. CI runs
        // tests without DROPBOX_APP_KEY so this almost always fires.
        if app_key().is_some() {
            return;
        }
        let r = start_oauth_flow();
        assert!(r.is_err());
        let msg = format!("{:?}", r.unwrap_err());
        assert!(msg.contains("not yet configured") || msg.contains("DROPBOX_APP_KEY"));
    }

    #[test]
    fn app_key_filters_placeholder_value() {
        // Sanity: even if someone leaves the historical placeholder
        // in their env (DROPBOX_APP_KEY=REPLACE_WITH_REAL_DROPBOX_APP_KEY),
        // app_key() rejects it as unconfigured. Verified via the
        // filter logic — we can't actually mutate option_env! at
        // runtime, but we can test the filter's intent.
        fn filter(s: &str) -> Option<&str> {
            Some(s)
                .filter(|s| !s.is_empty() && *s != "REPLACE_WITH_REAL_DROPBOX_APP_KEY")
        }
        assert!(filter("REPLACE_WITH_REAL_DROPBOX_APP_KEY").is_none());
        assert!(filter("").is_none());
        assert!(filter("real-app-key-abc123").is_some());
    }

    // Expiry-detection coverage lives in dropbox::tests now —
    // DropboxCredentials::is_expiring is the canonical implementation
    // for both PAT and OAuth shapes; the bare DropboxOAuthTokens
    // type is just a transport between OAuth helpers and
    // dropbox::ensure_fresh.
}

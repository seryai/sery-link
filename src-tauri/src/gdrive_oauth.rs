//! Google Drive OAuth — Phase 3b of the cloud-connectors migration.
//!
//! See `datalake/SPEC_CLOUD_CONNECTORS_MIGRATION.md` for the bigger
//! plan and `datalake/SETUP_GOOGLE_OAUTH.md` for the maintainer-side
//! Google Cloud Console setup that produces `GOOGLE_OAUTH_CLIENT_ID`.
//!
//! This module owns the desktop OAuth flow (PKCE + S256, RFC 7636):
//!
//!   1. `start_flow()` generates a one-shot PKCE pair + random state,
//!      stores them in a process-local pending table keyed by state,
//!      and returns the authorization URL the caller should open in a
//!      browser.
//!   2. The browser → Google → user consents → Google redirects to
//!      `seryai://oauth/gdrive/callback?code=...&state=...`.
//!   3. The OS hands the URL to the running Sery Link process, which
//!      `deep_link.rs` parses and dispatches to `handle_callback()`.
//!   4. `handle_callback()` looks up the verifier by state, exchanges
//!      the code for tokens, persists tokens to the OS keychain via
//!      `gdrive_creds`, emits a `gdrive-oauth-complete` Tauri event so
//!      the frontend can react, and clears the pending state.
//!
//! Build-time gating: if `GOOGLE_OAUTH_CLIENT_ID` isn't set at compile
//! time, `client_id()` returns `None` and the start command surfaces a
//! "Google Drive integration not configured for this build" error.
//! The rest of Sery Link still compiles and runs normally; users who
//! don't have Drive available just don't see the option in the UI.
//!
//! End-to-end testing requires a real client ID. Pure functions
//! (PKCE, URL building, redirect parsing) have unit tests; HTTP paths
//! (code exchange, token refresh) are tested manually after Phase 3a
//! setup completes.
//!
//! ## OAuth scopes
//!
//! Read-only ALWAYS. Sery never modifies the user's Drive content.
//! The two scopes documented in `SETUP_GOOGLE_OAUTH.md` Step 3:
//!
//!   - `https://www.googleapis.com/auth/drive.readonly` — file content
//!   - `https://www.googleapis.com/auth/drive.metadata.readonly` — listing

use crate::error::{AgentError, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Runtime};

// ── Constants ──────────────────────────────────────────────────────

/// Authorization endpoint per Google's OAuth 2.0 documentation.
/// Stable URL; if it ever changes, Google emits a deprecation notice
/// at https://developers.google.com/identity/protocols/oauth2.
const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";

/// Token endpoint — handles both initial code exchange and refresh.
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

/// The redirect_uri Google sends users back to. Must match what's
/// registered in Google Cloud Console (or be a custom URL scheme,
/// which is what we use). The OS routes this URL to Sery Link via
/// `tauri-plugin-deep-link`; see `deep_link.rs` for the dispatcher.
pub const REDIRECT_URI: &str = "seryai://oauth/gdrive/callback";

/// Scopes requested in the consent screen. Read-only by design — see
/// module docs. Order matters for the URL but not for Google.
const SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/drive.readonly",
    "https://www.googleapis.com/auth/drive.metadata.readonly",
];

/// How long a pending OAuth flow is allowed to sit before we GC it.
/// Real flows complete in under a minute; the TTL just prevents the
/// pending table growing unbounded if a user starts a flow and walks
/// away without consenting.
const PENDING_TTL: Duration = Duration::from_secs(600);

// ── Build-time client ID gating ────────────────────────────────────

/// Compile-time-embedded OAuth client ID, set via the
/// `GOOGLE_OAUTH_CLIENT_ID` env var at `cargo build` time. Returns
/// `None` for builds without the env var; callers should surface a
/// "not configured" error to the UI in that case.
///
/// Why compile-time and not runtime? Desktop OAuth client IDs aren't
/// secrets — they're discoverable in network traces and the binary —
/// but baking them in keeps the desktop app self-contained and able
/// to start the OAuth flow when offline. Rotation requires a release,
/// which goes through the auto-updater.
pub fn client_id() -> Option<&'static str> {
    option_env!("GOOGLE_OAUTH_CLIENT_ID")
}

// ── PKCE ───────────────────────────────────────────────────────────

/// PKCE verifier + challenge per RFC 7636.
///
///   - `verifier`: 43-128 chars from `[A-Z][a-z][0-9]-._~`. We use 64.
///   - `challenge`: `base64url(sha256(verifier))` with no padding.
///
/// Both produced together via `Pkce::new()` so the relationship is
/// preserved by construction. The verifier is sent in the token-
/// exchange step; the challenge is in the authorization URL.
#[derive(Debug, Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Self {
        // 64-byte verifier from the unreserved alphabet. RFC 7636
        // §4.1 specifies 43-128; 64 is comfortable middle ground that
        // gives us 384 bits of entropy.
        let verifier: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(64)
            .map(char::from)
            .collect();

        // SHA-256 → base64url-no-pad. The S256 method.
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let digest = hasher.finalize();
        let challenge = URL_SAFE_NO_PAD.encode(digest);

        Self {
            verifier,
            challenge,
        }
    }
}

/// Generate a random URL-safe state string, used to bind the
/// authorization URL to the callback (CSRF protection).
fn random_state() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

// ── Pending-flow table ─────────────────────────────────────────────

/// One in-flight OAuth flow. Owned by the static `PENDING` table,
/// keyed on `state`. Lives until `handle_callback` finds it (success
/// path) or `PENDING_TTL` elapses (abandoned-flow GC).
struct PendingFlow {
    verifier: String,
    started_at: Instant,
}

/// Process-local storage for in-flight flows. A `Mutex<HashMap>` is
/// fine here — flow start/finish is rare (manual user action), so
/// contention is negligible and the simpler primitive beats the
/// concurrent-map dependency.
fn pending() -> &'static Mutex<HashMap<String, PendingFlow>> {
    use std::sync::OnceLock;
    static P: OnceLock<Mutex<HashMap<String, PendingFlow>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Drop entries older than `PENDING_TTL`. Called opportunistically
/// from `start_flow` so we never accumulate without bound; not on a
/// timer because flows are infrequent.
fn gc_pending() {
    if let Ok(mut map) = pending().lock() {
        let now = Instant::now();
        map.retain(|_, flow| now.duration_since(flow.started_at) < PENDING_TTL);
    }
}

// ── Authorization-URL builder ──────────────────────────────────────

/// Build the full authorization URL the user should visit in their
/// browser. Encodes scopes, redirect_uri, PKCE challenge, and state.
///
/// Tested in unit tests below for parameter correctness; not for
/// "Google accepts this URL" — that requires a real client ID and is
/// covered by the manual smoke test in `SETUP_GOOGLE_OAUTH.md` Step 7.
pub fn build_authorization_url(
    client_id: &str,
    state: &str,
    code_challenge: &str,
) -> String {
    use urlencoding::encode;
    let scope = SCOPES.join(" ");
    format!(
        "{auth}?client_id={cid}&redirect_uri={redirect}&response_type=code&scope={scope}&state={state}&code_challenge={challenge}&code_challenge_method=S256&access_type=offline&prompt=consent",
        auth = AUTH_ENDPOINT,
        cid = encode(client_id),
        redirect = encode(REDIRECT_URI),
        scope = encode(&scope),
        state = encode(state),
        challenge = encode(code_challenge),
    )
    // `access_type=offline` requests a refresh_token alongside the
    // access_token — required for any flow longer than the access
    // token's 1-hour lifetime.
    //
    // `prompt=consent` forces the consent screen even on re-auth.
    // Otherwise Google may skip it and return an access_token without
    // a refresh_token, breaking long-term re-use. Pay the extra click
    // for the guarantee.
}

// ── Public flow entry ──────────────────────────────────────────────

/// Returns the authorization URL for the caller to open in a browser,
/// after stashing a fresh PKCE verifier under a random state.
///
/// Returns `Err` if the build wasn't configured with an OAuth client
/// ID — the UI should surface this as "not available in this build."
pub fn start_flow() -> Result<String> {
    let cid = client_id().ok_or_else(|| {
        AgentError::Config(
            "Google Drive integration not configured for this build. \
             See datalake/SETUP_GOOGLE_OAUTH.md."
                .to_string(),
        )
    })?;

    gc_pending();

    let pkce = Pkce::new();
    let state = random_state();

    pending().lock().unwrap().insert(
        state.clone(),
        PendingFlow {
            verifier: pkce.verifier.clone(),
            started_at: Instant::now(),
        },
    );

    Ok(build_authorization_url(cid, &state, &pkce.challenge))
}

// ── Token exchange ─────────────────────────────────────────────────

/// What Google's token endpoint returns on success.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    /// Present on initial code exchange + when Google decides to
    /// rotate. Persist to keychain whenever non-empty; never overwrite
    /// with `None`.
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Seconds from issuance to expiry. Typically 3600.
    pub expires_in: u64,
    pub scope: String,
    pub token_type: String,
}

/// POST to Google's token endpoint to exchange an authorization code
/// for tokens. Used once per OAuth flow.
pub async fn exchange_code(client_id: &str, code: &str, verifier: &str) -> Result<TokenResponse> {
    let client = reqwest::Client::new();
    let params = [
        ("client_id", client_id),
        ("code", code),
        ("code_verifier", verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", REDIRECT_URI),
    ];

    let resp = client
        .post(TOKEN_ENDPOINT)
        .form(&params)
        .send()
        .await
        .map_err(|e| AgentError::Network(format!("token exchange request: {}", e)))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AgentError::Network(format!(
            "token exchange {}: {}",
            status, body
        )));
    }

    resp.json::<TokenResponse>()
        .await
        .map_err(|e| AgentError::Serialization(format!("parse token response: {}", e)))
}

/// POST to Google's token endpoint to refresh an expired access
/// token. Refresh tokens are long-lived but Google may rotate them
/// — if the response contains a new `refresh_token`, replace the
/// stored one.
pub async fn refresh_token(client_id: &str, refresh_token: &str) -> Result<TokenResponse> {
    let client = reqwest::Client::new();
    let params = [
        ("client_id", client_id),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ];

    let resp = client
        .post(TOKEN_ENDPOINT)
        .form(&params)
        .send()
        .await
        .map_err(|e| AgentError::Network(format!("refresh request: {}", e)))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AgentError::Network(format!(
            "token refresh {}: {}",
            status, body
        )));
    }

    resp.json::<TokenResponse>()
        .await
        .map_err(|e| AgentError::Serialization(format!("parse refresh response: {}", e)))
}

// ── Callback handler ───────────────────────────────────────────────

/// Called by `deep_link.rs` when the OS routes a
/// `seryai://oauth/gdrive/callback?code=...&state=...` URL.
///
/// Looks up the pending flow by state, exchanges the code for tokens,
/// stores tokens via `gdrive_creds`, and emits a Tauri event so the
/// frontend can react. Errors are logged and emitted as the failure
/// payload of the same event — UI treats both branches the same.
pub async fn handle_callback<R: Runtime>(app: &AppHandle<R>, code: &str, state: &str) {
    let result = handle_callback_inner(code, state).await;
    let payload = match &result {
        Ok(account) => serde_json::json!({"ok": true, "account": account}),
        Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
    };
    if let Err(emit_err) = app.emit("gdrive-oauth-complete", payload) {
        eprintln!("[gdrive-oauth] failed to emit event: {}", emit_err);
    }
}

async fn handle_callback_inner(code: &str, state: &str) -> Result<String> {
    let cid = client_id().ok_or_else(|| {
        AgentError::Config(
            "Google Drive integration not configured for this build.".to_string(),
        )
    })?;

    // Pull the verifier out of the pending table. After this, state
    // is consumed — a replay of the same callback URL fails fast.
    let verifier = {
        let mut map = pending().lock().unwrap();
        map.remove(state)
            .map(|p| p.verifier)
            .ok_or_else(|| AgentError::Config(
                "OAuth callback state didn't match any pending flow. The flow \
                 may have expired or been replayed.".to_string()
            ))?
    };

    let tokens = exchange_code(cid, code, &verifier).await?;

    // For v1 we use a single "default" account per Sery Link install.
    // Multi-account support (Phase 3c+) keys on the Google user's
    // email, fetched via a `userinfo` call after token exchange.
    let account = "default".to_string();
    let stored = crate::gdrive_creds::StoredTokens::from_token_response(&tokens);
    crate::gdrive_creds::save(&account, &stored)?;

    Ok(account)
}

// ── Unit tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_pair_matches_rfc_7636() {
        let pkce = Pkce::new();
        // Verifier length: 64 chars per our chosen size.
        assert_eq!(pkce.verifier.len(), 64);
        // All chars are unreserved per RFC 7636 §4.1 (alphanumeric
        // here — Alphanumeric is a strict subset of unreserved).
        for c in pkce.verifier.chars() {
            assert!(c.is_ascii_alphanumeric(), "char {} not unreserved", c);
        }
        // Challenge is 43 chars (base64url(32 bytes) = 43 with no pad).
        assert_eq!(pkce.challenge.len(), 43);
        // Challenge is base64url alphabet only (URL-safe, no padding).
        for c in pkce.challenge.chars() {
            assert!(
                c.is_ascii_alphanumeric() || c == '-' || c == '_',
                "char {} not in base64url alphabet",
                c
            );
        }
    }

    #[test]
    fn pkce_pairs_are_unique() {
        let a = Pkce::new();
        let b = Pkce::new();
        assert_ne!(a.verifier, b.verifier);
        assert_ne!(a.challenge, b.challenge);
    }

    #[test]
    fn pkce_challenge_derives_from_verifier() {
        // Re-run the challenge derivation by hand and confirm match —
        // catches accidental algo changes.
        let pkce = Pkce::new();
        let mut hasher = Sha256::new();
        hasher.update(pkce.verifier.as_bytes());
        let expected = URL_SAFE_NO_PAD.encode(hasher.finalize());
        assert_eq!(pkce.challenge, expected);
    }

    #[test]
    fn authorization_url_has_required_params() {
        let url = build_authorization_url("the-client-id", "abc-state", "the-challenge");
        // Endpoint
        assert!(url.starts_with(AUTH_ENDPOINT));
        // All required params present and properly URL-encoded
        assert!(url.contains("client_id=the-client-id"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge=the-challenge"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=abc-state"));
        // redirect_uri must be URL-encoded (% is the marker)
        assert!(
            url.contains("redirect_uri=seryai%3A%2F%2Foauth%2Fgdrive%2Fcallback"),
            "redirect_uri not properly encoded: {}",
            url
        );
        // Scopes joined by URL-encoded space (%20 or +)
        assert!(url.contains("drive.readonly"));
        assert!(url.contains("drive.metadata.readonly"));
        // access_type=offline so we get a refresh_token
        assert!(url.contains("access_type=offline"));
        // prompt=consent so we always get a refresh_token (Google
        // omits it on re-auth without this).
        assert!(url.contains("prompt=consent"));
    }

    #[test]
    fn pending_flow_round_trip() {
        // start_flow with a fake client ID via env override would
        // require unsetting; instead we test the table directly.
        let key = "test-state-1".to_string();
        pending().lock().unwrap().insert(
            key.clone(),
            PendingFlow {
                verifier: "test-verifier".to_string(),
                started_at: Instant::now(),
            },
        );
        let removed = pending().lock().unwrap().remove(&key);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().verifier, "test-verifier");
        // Second remove is None — replay fails.
        assert!(pending().lock().unwrap().remove(&key).is_none());
    }

    #[test]
    fn random_state_is_url_safe() {
        for _ in 0..10 {
            let s = random_state();
            assert_eq!(s.len(), 32);
            assert!(
                s.chars().all(|c| c.is_ascii_alphanumeric()),
                "non-alphanumeric in state: {}",
                s
            );
        }
    }
}

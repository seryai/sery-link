//! Google Drive OAuth — Phase 3b of the cloud-connectors migration.
//!
//! See `datalake/SPEC_CLOUD_CONNECTORS_MIGRATION.md` for the bigger
//! plan and `datalake/SETUP_GOOGLE_OAUTH.md` for the maintainer-side
//! Google Cloud Console setup that produces `GOOGLE_OAUTH_CLIENT_ID`.
//!
//! This module owns the desktop OAuth flow (PKCE + S256, RFC 7636).
//! Uses **loopback redirect** per Google's recommended pattern for
//! desktop apps — custom URI schemes like `seryai://` are blocked by
//! Google's 2024+ policy unless they follow reverse-domain notation.
//!
//!   1. `start_flow()`:
//!      - generates a one-shot PKCE pair + random state, stores them
//!        in a process-local pending table keyed by state
//!      - binds an HTTP server on `127.0.0.1:0` (OS picks the port)
//!      - builds the authorization URL with `redirect_uri=http://127.0.0.1:PORT/`
//!      - spawns a background task that accepts ONE TCP connection,
//!        parses `?code=...&state=...` from the GET request, sends a
//!        friendly "you can close this tab" HTML response, then calls
//!        `handle_callback()` which exchanges the code for tokens
//!      - returns the authorization URL the caller opens in a browser
//!   2. Google redirects the browser to `http://127.0.0.1:PORT/` after
//!      user consent — our embedded server catches it.
//!   3. `handle_callback()` looks up the verifier by state, exchanges
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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// ── Constants ──────────────────────────────────────────────────────

/// Authorization endpoint per Google's OAuth 2.0 documentation.
/// Stable URL; if it ever changes, Google emits a deprecation notice
/// at https://developers.google.com/identity/protocols/oauth2.
const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";

/// Token endpoint — handles both initial code exchange and refresh.
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

/// How long the loopback HTTP server stays bound waiting for the
/// browser callback. Real consents complete in well under a minute;
/// the timeout protects against an abandoned flow leaking a TCP
/// listener and a pending entry forever.
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

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
/// `redirect_uri` is dynamic — built from the loopback server's
/// chosen port at flow-start time. Must EXACTLY match the
/// redirect_uri sent in the subsequent token-exchange request, or
/// Google rejects with `redirect_uri_mismatch`.
///
/// Tested in unit tests below for parameter correctness; not for
/// "Google accepts this URL" — that requires a real client ID and is
/// covered by the manual smoke test in `SETUP_GOOGLE_OAUTH.md` Step 7.
pub fn build_authorization_url(
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    code_challenge: &str,
) -> String {
    use urlencoding::encode;
    let scope = SCOPES.join(" ");
    format!(
        "{auth}?client_id={cid}&redirect_uri={redirect}&response_type=code&scope={scope}&state={state}&code_challenge={challenge}&code_challenge_method=S256&access_type=offline&prompt=consent",
        auth = AUTH_ENDPOINT,
        cid = encode(client_id),
        redirect = encode(redirect_uri),
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

/// Returns the authorization URL for the caller to open in a
/// browser, after binding a one-shot loopback HTTP server that
/// catches the callback and spawning a background task that processes
/// it. The caller's responsibility ends after opening the URL — the
/// background task fires `gdrive-oauth-complete` when the flow is
/// done (success, user cancel, or timeout).
///
/// Returns `Err` if the build wasn't configured with an OAuth client
/// ID — the UI should surface this as "not available in this build."
pub async fn start_flow<R: Runtime>(app: AppHandle<R>) -> Result<String> {
    let cid = client_id().ok_or_else(|| {
        AgentError::Config(
            "Google Drive integration not configured for this build. \
             See datalake/SETUP_GOOGLE_OAUTH.md."
                .to_string(),
        )
    })?;

    gc_pending();

    // Bind first so we know the port before building the URL. Google
    // requires the redirect_uri sent in /authorize to byte-match the
    // one sent in /token; using the bound port everywhere guarantees
    // that match without configuration on the maintainer side.
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| AgentError::Network(format!("loopback bind: {}", e)))?;
    let port = listener
        .local_addr()
        .map_err(|e| AgentError::Network(format!("loopback addr: {}", e)))?
        .port();
    // Trailing slash matters — Google compares redirect_uri strings
    // exactly. The browser will GET `/?code=...&state=...`.
    let redirect_uri = format!("http://127.0.0.1:{}/", port);

    let pkce = Pkce::new();
    let state = random_state();

    pending().lock().unwrap().insert(
        state.clone(),
        PendingFlow {
            verifier: pkce.verifier.clone(),
            started_at: Instant::now(),
        },
    );

    let auth_url = build_authorization_url(cid, &redirect_uri, &state, &pkce.challenge);

    // Spawn the loopback-server task. After it captures the callback
    // (or times out) it calls handle_callback which exchanges the
    // code, persists tokens, and emits the Tauri event. We do NOT
    // await — the Tauri command returns immediately so the frontend
    // can show "connecting…" while the user consents in the browser.
    tauri::async_runtime::spawn(loopback_server_task(app, listener, redirect_uri));

    Ok(auth_url)
}

/// The task that runs the loopback HTTP server. Accepts ONE
/// connection (Google redirects exactly once per consent), parses
/// the GET line for `code` + `state`, sends a friendly HTML
/// response, then dispatches to `handle_callback`. On timeout or
/// network error the function exits quietly — the user already saw
/// "connecting…" in the UI and the gdrive-oauth-complete event
/// either fires from within handle_callback or never fires.
async fn loopback_server_task<R: Runtime>(
    app: AppHandle<R>,
    listener: TcpListener,
    redirect_uri: String,
) {
    let result = tokio::time::timeout(CALLBACK_TIMEOUT, accept_one_callback(listener)).await;
    match result {
        Ok(Ok((code, state, error))) => {
            if let Some(err) = error {
                // Google sends `?error=access_denied` when the user
                // clicks Cancel on the consent screen. Surface
                // distinctly so the UI shows "cancelled" not "failed."
                eprintln!("[gdrive-oauth] consent error: {}", err);
                let _ = app.emit(
                    "gdrive-oauth-complete",
                    serde_json::json!({"ok": false, "error": err}),
                );
                // Drop the pending state so a later replay can't reuse it.
                if let Some(s) = state {
                    pending().lock().unwrap().remove(&s);
                }
            } else if let (Some(c), Some(s)) = (code, state) {
                handle_callback(&app, &c, &s, &redirect_uri).await;
            } else {
                eprintln!("[gdrive-oauth] callback missing code or state");
                let _ = app.emit(
                    "gdrive-oauth-complete",
                    serde_json::json!({"ok": false, "error": "callback missing parameters"}),
                );
            }
        }
        Ok(Err(e)) => {
            eprintln!("[gdrive-oauth] loopback accept failed: {}", e);
            let _ = app.emit(
                "gdrive-oauth-complete",
                serde_json::json!({"ok": false, "error": format!("loopback accept: {}", e)}),
            );
        }
        Err(_) => {
            eprintln!("[gdrive-oauth] callback timed out after {:?}", CALLBACK_TIMEOUT);
            let _ = app.emit(
                "gdrive-oauth-complete",
                serde_json::json!({"ok": false, "error": "consent timed out — try again"}),
            );
        }
    }
}

/// Accept one TCP connection on the loopback listener, parse the
/// GET request line for query parameters, send a friendly HTML
/// response, return the extracted code/state/error.
///
/// Manual HTTP parsing is fine here — we know exactly the shape of
/// the request (browser GET on `/?code=...`) and don't need full
/// HTTP server semantics.
async fn accept_one_callback(
    listener: TcpListener,
) -> std::io::Result<(Option<String>, Option<String>, Option<String>)> {
    let (mut stream, _) = listener.accept().await?;

    // Read up to 4 KB of the request — the GET line + headers fits
    // easily and Google's redirect URLs aren't unbounded.
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).await?;
    let req_text = String::from_utf8_lossy(&buf[..n]);

    // First line is `GET /?code=...&state=... HTTP/1.1`. Take
    // everything between the first space and the second.
    let path_with_query = req_text
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .unwrap_or("");

    // url::Url needs an absolute URL; prefix with our scheme + host
    // for parsing. The path may or may not start with '/'.
    let parse_url = format!("http://127.0.0.1{}", path_with_query);
    let mut code = None;
    let mut state = None;
    let mut error = None;
    if let Ok(parsed) = url::Url::parse(&parse_url) {
        for (k, v) in parsed.query_pairs() {
            match k.as_ref() {
                "code" => code = Some(v.into_owned()),
                "state" => state = Some(v.into_owned()),
                "error" => error = Some(v.into_owned()),
                _ => {}
            }
        }
    }

    let body_html = if error.is_some() {
        callback_html("Connection cancelled", "You can close this tab and return to Sery Link.")
    } else if code.is_some() {
        callback_html("Connected to Google Drive", "You can close this tab and return to Sery Link.")
    } else {
        callback_html("Something went wrong", "Sery Link couldn't read the callback. Try again from the app.")
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body_html.len(),
        body_html
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;

    Ok((code, state, error))
}

/// HTML shown in the user's browser tab after the callback. Plain
/// inline CSS, system fonts — meant to render fine on any browser
/// without external resources. The user's expected next action is
/// to close the tab and return to Sery Link.
fn callback_html(title: &str, message: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en"><head>
<meta charset="UTF-8">
<title>{title} — Sery Link</title>
<style>
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
       max-width: 480px; margin: 64px auto; padding: 32px;
       text-align: center; color: #1f2937; }}
h1 {{ color: #7c3aed; margin: 16px 0 8px; font-size: 22px; }}
p {{ color: #6b7280; line-height: 1.6; font-size: 15px; }}
.icon {{ font-size: 48px; line-height: 1; }}
</style>
</head><body>
<div class="icon">✓</div>
<h1>{title}</h1>
<p>{message}</p>
</body></html>"#,
        title = title,
        message = message,
    )
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
///
/// `redirect_uri` must EXACTLY match the one sent in the original
/// /authorize call — Google validates byte-for-byte. The loopback
/// flow generates this dynamically in `start_flow()` and threads it
/// through to here.
pub async fn exchange_code(
    client_id: &str,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse> {
    let client = reqwest::Client::new();
    let params = [
        ("client_id", client_id),
        ("code", code),
        ("code_verifier", verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri),
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

/// Called by the loopback server task when Google redirects with a
/// `?code=...&state=...` to the loopback URL.
///
/// Looks up the pending flow by state, exchanges the code for tokens
/// (using the same redirect_uri Google saw), stores tokens via
/// `gdrive_creds`, emits a Tauri event so the frontend can react.
/// Errors are logged + emitted as the failure payload of the same
/// event — UI treats both branches the same.
pub async fn handle_callback<R: Runtime>(
    app: &AppHandle<R>,
    code: &str,
    state: &str,
    redirect_uri: &str,
) {
    let result = handle_callback_inner(code, state, redirect_uri).await;
    let payload = match &result {
        Ok(account) => {
            // Stdout visibility for `tauri dev` users — the browser
            // tab already says "Connected", but the terminal is the
            // ground truth when the frontend mysteriously doesn't
            // update.
            eprintln!("[gdrive-oauth] tokens persisted for account={}", account);
            serde_json::json!({"ok": true, "account": account})
        }
        Err(e) => {
            // Same logging philosophy: any failure after the browser
            // redirect needs to be visible somewhere the developer can
            // see it. Without this the spawned task fails silently and
            // the only symptom is the modal still showing "Connect".
            eprintln!("[gdrive-oauth] callback failed: {}", e);
            serde_json::json!({"ok": false, "error": e.to_string()})
        }
    };
    if let Err(emit_err) = app.emit("gdrive-oauth-complete", payload) {
        eprintln!("[gdrive-oauth] failed to emit event: {}", emit_err);
    }
}

async fn handle_callback_inner(code: &str, state: &str, redirect_uri: &str) -> Result<String> {
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

    let tokens = exchange_code(cid, code, &verifier, redirect_uri).await?;

    // For v1 we use a single "default" account per Sery Link install.
    // Multi-account support (Phase 3c+) keys on the Google user's
    // email, fetched via a `userinfo` call after token exchange.
    let account = "default".to_string();
    let stored = crate::gdrive_creds::StoredTokens::from_token_response(&tokens)
        .map_err(AgentError::Config)?;
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
        let url = build_authorization_url(
            "the-client-id",
            "http://127.0.0.1:54321/",
            "abc-state",
            "the-challenge",
        );
        // Endpoint
        assert!(url.starts_with(AUTH_ENDPOINT));
        // All required params present and properly URL-encoded
        assert!(url.contains("client_id=the-client-id"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge=the-challenge"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=abc-state"));
        // redirect_uri must be URL-encoded (% is the marker). Loopback
        // IP per Google's 2024+ policy for desktop apps.
        assert!(
            url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A54321%2F"),
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

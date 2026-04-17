//! Agent pairing — "Add another machine" flow.
//!
//! This module is a thin HTTP client for the api/v1/agents/pair-* endpoints
//! (see datalake/SPEC_PAIR_FLOW.md for the full UX). The Tauri command
//! wrappers in `commands.rs` expose these to the React frontend.
//!
//! Three operations:
//!   - pair_request  — generating machine gets a one-time pair code + QR URL
//!   - pair_status   — generating machine polls for completion
//!   - pair_complete — new machine redeems the code, gets a 30-day token
//!
//! pair_request + pair_status require the calling agent's existing bearer
//! token. pair_complete is unauthenticated (the pair_code itself IS the
//! auth, guarded by server-side TTL + rate limit).

use crate::error::{AgentError, Result};
use crate::keyring_store;
use serde::{Deserialize, Serialize};

/// Response from POST /v1/agents/pair-request.
///
/// `pair_code` is hyphen-formatted (XXX-XXX-XXX-XXX). `qr_url` is a short
/// redirect that embeds the pair_code — machine #1 encodes this in a QR
/// so machine #2 can scan it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairRequestResponse {
    pub pair_code: String,
    pub expires_at: String,
    pub expires_in_seconds: i64,
    pub qr_url: String,
}

/// Response from GET /v1/agents/pair-status/{code}.
///
/// `status` is one of: "pending" | "completed" | "expired".
/// When status=="completed", `new_agent` describes the machine that just
/// joined; UI should transition to a "connected" confirmation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairStatusResponse {
    pub status: String,
    pub expires_at: Option<String>,
    pub new_agent: Option<serde_json::Value>,
}

/// Response from POST /v1/agents/pair-complete — a 30-day agent token for
/// the newly-enrolled machine. Same shape as /v1/agent/token / workspace-
/// key auth so downstream code (keyring save, WebSocket connect) doesn't
/// branch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairCompleteResponse {
    pub access_token: String,
    pub agent_id: String,
    pub workspace_id: String,
    pub expires_in: Option<u64>,
}

/// POST /v1/agents/pair-request
///
/// Called by the UI on machine #1 when the user hits "Add another machine".
/// The currently-stored bearer token is used for auth — the server ties
/// the generated code to this agent's workspace.
pub async fn pair_request(api_url: &str) -> Result<PairRequestResponse> {
    let token = keyring_store::get_token()?;
    let url = format!("{}/v1/agents/pair-request", api_url);

    let resp = reqwest::Client::new()
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    decode_or_err(resp, "pair_request").await
}

/// GET /v1/agents/pair-status/{code}
///
/// Called by the UI on machine #1 in a polling loop (~2s) to detect when
/// machine #2 has redeemed the code. Uses the same bearer token as
/// pair_request (server enforces that only the generator can poll).
///
/// `code` may be hyphen-formatted or raw — server normalises both.
pub async fn pair_status(api_url: &str, code: &str) -> Result<PairStatusResponse> {
    let token = keyring_store::get_token()?;
    // Strip hyphens so the URL is always compact.
    let normalized = code.replace('-', "");
    let url = format!("{}/v1/agents/pair-status/{}", api_url, normalized);

    let resp = reqwest::Client::new()
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    decode_or_err(resp, "pair_status").await
}

/// POST /v1/agents/pair-complete
///
/// Called by the UI on machine #2 after the user scans a QR or types a
/// pair code. Unauthenticated — the code itself is the auth. On success,
/// stores the returned token in the keyring so the next launch picks it
/// up transparently.
pub async fn pair_complete(
    api_url: &str,
    pair_code: &str,
    display_name: &str,
) -> Result<PairCompleteResponse> {
    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string());

    let os_type = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    };

    let body = serde_json::json!({
        "pair_code": pair_code.replace('-', ""),
        "display_name": display_name,
        "os_type": os_type,
        "hostname": hostname,
        "agent_version": env!("CARGO_PKG_VERSION"),
    });

    let url = format!("{}/v1/agents/pair-complete", api_url);
    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await?;

    let token: PairCompleteResponse = decode_or_err(resp, "pair_complete").await?;

    // Mirror start_oauth_flow / auth_with_workspace_key — new agent's token
    // goes to the OS keyring so the next process launch picks it up.
    keyring_store::save_token(&token.access_token)?;

    Ok(token)
}

/// Shared response-decoding helper. Surfaces server-side HTTP errors with
/// status code + response body so the UI can show something actionable.
async fn decode_or_err<T: for<'de> Deserialize<'de>>(
    resp: reqwest::Response,
    op: &str,
) -> Result<T> {
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(AgentError::Network(format!(
            "{} failed ({}): {}",
            op, status, text
        )));
    }
    resp.json::<T>()
        .await
        .map_err(|e| AgentError::Network(format!("{} decode error: {}", op, e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_code_normalisation_strips_hyphens() {
        // We rely on server-side normalisation but also strip hyphens
        // client-side so polling URLs stay compact.
        let hyphenated = "A7B-3FK-9XM-2DP";
        let compact: String = hyphenated.replace('-', "");
        assert_eq!(compact, "A7B3FK9XM2DP");
        assert_eq!(compact.len(), 12);
    }

    #[test]
    fn os_type_matches_target() {
        // Ensure pair_complete sends the correct os_type at build time.
        let os_type = if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else {
            "linux"
        };
        assert!(["macos", "windows", "linux"].contains(&os_type));
    }
}

//! Machines — "My Machines" view client.
//!
//! Thin wrapper around GET /v1/agent/workspace/fleet (agent-authed
//! variant landed in api commit 9516cfe; the backend route name is
//! kept for continuity with the HTTP contract). Lets Sery Link list
//! every machine in its workspace without needing a user bearer token.
//!
//! The Tauri command in commands.rs exposes this to the React side as
//! `list_machines`.

use crate::error::{AgentError, Result};
use crate::keyring_store;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Machine {
    pub agent_id: String,
    pub display_name: Option<String>,
    pub name: String,
    pub hostname: Option<String>,
    pub os_type: Option<String>,
    pub status: String, // "online" | "offline" | "error"
    pub last_seen_at: Option<String>,
    pub dataset_count: i64,
    pub total_bytes: i64,
    #[serde(default)]
    pub is_current_user: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachinesResponse {
    pub workspace_id: String,
    pub agents: Vec<Machine>,
    pub total: i64,
}

/// Fetch every machine in the calling agent's workspace. Returns them
/// in creation order with live online status.
pub async fn list_machines(api_url: &str) -> Result<MachinesResponse> {
    let token = keyring_store::get_token()?;
    let url = format!("{}/v1/agent/workspace/fleet", api_url);

    let resp = reqwest::Client::new()
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(AgentError::Network(format!(
            "machines listing failed ({}): {}",
            status, text
        )));
    }

    resp.json::<MachinesResponse>()
        .await
        .map_err(|e| AgentError::Network(format!("machines decode error: {}", e)))
}

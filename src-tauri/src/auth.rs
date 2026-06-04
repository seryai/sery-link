use axum::{
    extract::Query,
    response::Html,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::oneshot;
use crate::config::{AuthMode, Config};
use crate::error::{AgentError, Result};
use crate::keyring_store;

const CALLBACK_PORT: u16 = 7777;
const AUTH_TIMEOUT_SECS: u64 = 300; // 5 minutes

#[derive(Debug, Deserialize)]
struct CallbackParams {
    code: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentToken {
    pub access_token: String,
    pub agent_id: String,
    pub workspace_id: String,
    pub expires_in: Option<u64>,
}

pub async fn start_oauth_flow(
    agent_name: String,
    platform: String,
    machine_id: String,
    api_url: String,
) -> Result<AgentToken> {
    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string());

    // Build authorization URL
    let auth_url = format!(
        "{}/v1/agent/authorize?agent_name={}&platform={}&hostname={}&machine_id={}&redirect_uri=http://localhost:{}",
        api_url,
        urlencoding::encode(&agent_name),
        urlencoding::encode(&platform),
        urlencoding::encode(&hostname),
        urlencoding::encode(&machine_id),
        CALLBACK_PORT
    );

    // Open browser
    open::that(&auth_url)
        .map_err(|e| AgentError::Auth(format!("Failed to open browser: {}", e)))?;

    // Start callback server and wait for code
    let code = start_callback_server().await?;

    // Exchange code for token
    let token = exchange_token(code, api_url).await?;

    // Save token to keyring
    keyring_store::save_token(&token.access_token)?;

    Ok(token)
}

async fn start_callback_server() -> Result<String> {
    let (code_tx, code_rx) = oneshot::channel();
    let code_tx = Arc::new(tokio::sync::Mutex::new(Some(code_tx)));

    let app = Router::new().route(
        "/",
        get({
            let code_tx = Arc::clone(&code_tx);
            move |query: Query<CallbackParams>| async move {
                let code = query.0.code.clone();

                // Send code to main task
                if let Some(tx) = code_tx.lock().await.take() {
                    let _ = tx.send(code);
                }

                Html(
                    r#"
                    <!DOCTYPE html>
                    <html>
                        <head>
                            <title>Sery - Authentication Successful</title>
                            <style>
                                body {
                                    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
                                    display: flex;
                                    justify-content: center;
                                    align-items: center;
                                    height: 100vh;
                                    margin: 0;
                                    background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
                                }
                                .container {
                                    background: white;
                                    padding: 3rem;
                                    border-radius: 1rem;
                                    box-shadow: 0 20px 60px rgba(0,0,0,0.3);
                                    text-align: center;
                                }
                                h1 {
                                    color: #667eea;
                                    margin: 0 0 1rem 0;
                                }
                                p {
                                    color: #666;
                                    margin: 0;
                                }
                            </style>
                        </head>
                        <body>
                            <div class="container">
                                <h1>✓ Authentication Successful!</h1>
                                <p>You can close this window and return to Sery Link.</p>
                            </div>
                            <script>
                                setTimeout(() => window.close(), 2000);
                            </script>
                        </body>
                    </html>
                    "#,
                )
            }
        }),
    );

    // Start server
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", CALLBACK_PORT))
        .await
        .map_err(|e| AgentError::Auth(format!("Failed to bind callback server: {}", e)))?;

    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    // Wait for code with timeout
    let code = tokio::time::timeout(
        std::time::Duration::from_secs(AUTH_TIMEOUT_SECS),
        code_rx,
    )
    .await
    .map_err(|_| AgentError::Auth("Authentication timeout".to_string()))?
    .map_err(|_| AgentError::Auth("Failed to receive authorization code".to_string()))?;

    Ok(code)
}

/// Anonymous workspace bootstrap — the zero-friction path for brand-new
/// installs (SPEC_FIRST_INSTALL.md §Config Persisted at First-Install).
///
/// POSTs to /v1/workspace/bootstrap with just a display_name, receives
/// a workspace_id + agent_id + 30-day token in one call. No OAuth, no
/// email, no workspace key prompt. Token is saved to the OS keyring so
/// the next launch picks it up transparently.
///
/// Caller should invoke this only on first run — after the keyring
/// already has a token, use that instead.
pub async fn bootstrap_workspace(
    display_name: String,
    api_url: String,
) -> Result<AgentToken> {
    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string());
    let os_type = std::env::consts::OS;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/workspace/bootstrap", api_url))
        .json(&serde_json::json!({
            "display_name": display_name,
            "os_type": os_type,
            "hostname": hostname,
            "agent_version": env!("CARGO_PKG_VERSION"),
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        let message = serde_json::from_str::<serde_json::Value>(&error_text)
            .ok()
            .and_then(|v| v["detail"].as_str().map(String::from))
            .unwrap_or(error_text);
        return Err(AgentError::Auth(format!("bootstrap failed: {}", message)));
    }

    let token: AgentToken = response.json().await?;
    keyring_store::save_token(&token.access_token)?;
    Ok(token)
}

pub async fn auth_with_workspace_key(
    key: String,
    display_name: String,
    machine_id: String,
    api_url: String,
) -> Result<AgentToken> {
    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string());
    let platform = std::env::consts::OS;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/agent/auth/key", api_url))
        .json(&serde_json::json!({
            "key": key,
            "display_name": display_name,
            "platform": platform,
            "hostname": hostname,
            "machine_id": machine_id,
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        // Parse {"detail": "..."} from FastAPI error responses
        let message = serde_json::from_str::<serde_json::Value>(&error_text)
            .ok()
            .and_then(|v| v["detail"].as_str().map(String::from))
            .unwrap_or(error_text);
        return Err(AgentError::Auth(message));
    }

    let token: AgentToken = response.json().await?;
    keyring_store::save_token(&token.access_token)?;
    keyring_store::save_workspace_key(&key)?;
    Ok(token)
}

/// Redeem a single-use mesh invitation code (the v0.8.10 alternative
/// to long-lived workspace keys). The code is a 10-char Crockford-
/// base32 string generated by the dashboard — see
/// `app/services/mesh/__init__.py`. POSTs to /v1/mesh/invitations/redeem
/// and, on success, gets back the same AgentToken shape as workspace-
/// key auth, with `auth_method="mesh_invitation"` recorded server-side.
///
/// We do NOT call `save_workspace_key` here — invitations are single-
/// use, so persisting the code would only enable a confusing retry on
/// re-launch (the server will reject it the second time). The token
/// alone is enough to keep the agent paired across launches.
pub async fn auth_with_mesh_invitation(
    code: String,
    display_name: String,
    machine_id: String,
    api_url: String,
) -> Result<AgentToken> {
    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string());
    let platform = std::env::consts::OS;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/mesh/invitations/redeem", api_url))
        .json(&serde_json::json!({
            "code": code,
            "display_name": display_name,
            "platform": platform,
            "hostname": hostname,
            "machine_id": machine_id,
            "version": env!("CARGO_PKG_VERSION"),
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        let message = serde_json::from_str::<serde_json::Value>(&error_text)
            .ok()
            .and_then(|v| v["detail"].as_str().map(String::from))
            .unwrap_or(error_text);
        return Err(AgentError::Auth(message));
    }

    let token: AgentToken = response.json().await?;
    keyring_store::save_token(&token.access_token)?;
    Ok(token)
}

async fn exchange_token(code: String, api_url: String) -> Result<AgentToken> {
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/v1/agent/token", api_url))
        .json(&serde_json::json!({ "code": code }))
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        return Err(AgentError::Auth(format!("Token exchange failed: {}", error_text)));
    }

    let token: AgentToken = response.json().await?;

    Ok(token)
}

// ---------------------------------------------------------------------------
// Auth mode detection
// ---------------------------------------------------------------------------

/// Detect the current authentication mode based on stored credentials.
///
/// Detection order:
/// 1. Explicit selection in config
/// 2. Workspace token in keyring → WorkspaceKey
/// 3. Default to LocalOnly
pub fn get_auth_mode(config: &Config) -> AuthMode {
    if let Some(mode) = &config.app.selected_auth_mode {
        return mode.clone();
    }

    // Single keyring read — `has_token` used to be called first as a
    // precheck, but it just calls `get_token` internally, so on
    // ad-hoc-signed builds macOS prompted the user twice for the same
    // item. The token cache in keyring_store collapses this to one
    // prompt per launch.
    if let Ok(token) = keyring_store::get_token() {
        return AuthMode::WorkspaceKey { key: token };
    }

    AuthMode::LocalOnly
}

/// Check if a feature is available in the current auth mode.
///
/// Feature availability:
/// - LocalOnly: free_recipes + local_sql only
/// - WorkspaceKey: all features
pub fn feature_available(mode: &AuthMode, feature: &str) -> bool {
    match (mode, feature) {
        // Free features - available in all modes
        (_, "free_recipes") => true,
        (_, "local_sql") => true,

        // Pro + team features - require WorkspaceKey
        (AuthMode::WorkspaceKey { .. }, "pro_recipes") => true,
        (AuthMode::WorkspaceKey { .. }, "ai_queries") => true,
        (AuthMode::WorkspaceKey { .. }, "cloud_sync") => true,
        (AuthMode::WorkspaceKey { .. }, "team_sharing") => true,
        (AuthMode::WorkspaceKey { .. }, "performance_mode") => true,

        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_only_mode_defaults() {
        let mode = AuthMode::LocalOnly;

        // FREE features should be available
        assert!(feature_available(&mode, "free_recipes"));
        assert!(feature_available(&mode, "local_sql"));

        // PRO features should be blocked
        assert!(!feature_available(&mode, "pro_recipes"));
        assert!(!feature_available(&mode, "ai_queries"));

        // TEAM features should be blocked
        assert!(!feature_available(&mode, "cloud_sync"));
        assert!(!feature_available(&mode, "team_sharing"));
        assert!(!feature_available(&mode, "performance_mode"));
    }

    #[test]
    fn test_workspace_key_mode_features() {
        let mode = AuthMode::WorkspaceKey {
            key: "sery_k_test_workspace_key".to_string(),
        };

        // All features should be available
        assert!(feature_available(&mode, "free_recipes"));
        assert!(feature_available(&mode, "local_sql"));
        assert!(feature_available(&mode, "pro_recipes"));
        assert!(feature_available(&mode, "ai_queries"));
        assert!(feature_available(&mode, "cloud_sync"));
        assert!(feature_available(&mode, "team_sharing"));
        assert!(feature_available(&mode, "performance_mode"));
    }

    #[test]
    fn test_unknown_feature_defaults_to_false() {
        let mode = AuthMode::WorkspaceKey {
            key: "test".to_string(),
        };

        assert!(!feature_available(&mode, "unknown_feature"));
        assert!(!feature_available(&mode, ""));
        assert!(!feature_available(&mode, "future_feature_2025"));
    }

    #[test]
    fn test_auth_mode_equality() {
        assert_eq!(AuthMode::LocalOnly, AuthMode::LocalOnly);
        assert_eq!(
            AuthMode::WorkspaceKey { key: "k".to_string() },
            AuthMode::WorkspaceKey { key: "k".to_string() },
        );
    }

    #[test]
    fn test_auth_mode_serialization() {
        use serde_json;

        let local = AuthMode::LocalOnly;
        let json = serde_json::to_string(&local).unwrap();
        assert!(json.contains("LocalOnly"));

        // WorkspaceKey serialization (key should be skipped)
        let workspace = AuthMode::WorkspaceKey {
            key: "secret-workspace-key".to_string(),
        };
        let json = serde_json::to_string(&workspace).unwrap();
        assert!(json.contains("WorkspaceKey"));
        // key should not be serialized
        assert!(!json.contains("secret-workspace-key"));
    }

    #[test]
    fn test_feature_gating_case_sensitivity() {
        let mode = AuthMode::LocalOnly;

        // Features should be case-sensitive
        assert!(feature_available(&mode, "free_recipes"));
        assert!(!feature_available(&mode, "FREE_RECIPES"));
        assert!(!feature_available(&mode, "Free_Recipes"));
    }

    #[test]
    fn test_tier_progression() {
        let test_features = [
            "free_recipes",
            "local_sql",
            "pro_recipes",
            "ai_queries",
            "cloud_sync",
            "team_sharing",
            "performance_mode",
        ];

        let local_count = test_features
            .iter()
            .filter(|f| feature_available(&AuthMode::LocalOnly, f))
            .count();
        let workspace_count = test_features
            .iter()
            .filter(|f| feature_available(&AuthMode::WorkspaceKey { key: "test".to_string() }, f))
            .count();

        assert!(local_count < workspace_count);
        assert_eq!(local_count, 2);    // free_recipes, local_sql
        assert_eq!(workspace_count, 7); // all features
    }
}

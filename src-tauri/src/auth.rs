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
    api_url: String,
) -> Result<AgentToken> {
    let hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string());

    // Build authorization URL
    let auth_url = format!(
        "{}/v1/agent/authorize?agent_name={}&platform={}&hostname={}&redirect_uri=http://localhost:{}",
        api_url,
        urlencoding::encode(&agent_name),
        urlencoding::encode(&platform),
        urlencoding::encode(&hostname),
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

/// Detect the current authentication mode based on stored credentials and environment.
///
/// Detection order:
/// 1. Check keyring for workspace token (WorkspaceKey mode)
/// 2. Check environment variables for BYOK (BYOK mode)
/// 3. Default to LocalOnly mode
pub fn get_auth_mode(config: &Config) -> AuthMode {
    // Check explicit selection first
    if let Some(mode) = &config.app.selected_auth_mode {
        return mode.clone();
    }

    // Auto-detect based on stored credentials

    // 1. Check keyring for workspace token
    if keyring_store::has_token() {
        if let Ok(token) = keyring_store::get_token() {
            return AuthMode::WorkspaceKey {
                key: token,
            };
        }
    }

    // 2. (Removed in v0.5.3 pivot.) The OS-keychain BYOK lookup
    //    used to live here — desktop no longer holds per-provider
    //    API keys. Existing keychain entries are left alone; AI
    //    happens cloud-side via the dashboard now.

    // 3. Fallback: env var (legacy / power-user path; useful in CI and dev).
    if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
        if !api_key.is_empty() {
            return AuthMode::BYOK {
                provider: "anthropic".to_string(),
                api_key,
            };
        }
    }

    // 4. Default to local-only
    AuthMode::LocalOnly
}

/// Check if a feature is available in the current auth mode.
///
/// Feature availability:
/// - LocalOnly: free_recipes only
/// - BYOK: free_recipes + pro_recipes
/// - WorkspaceKey: free_recipes + pro_recipes + team features
pub fn feature_available(mode: &AuthMode, feature: &str) -> bool {
    match (mode, feature) {
        // Free features - available in all modes
        (_, "free_recipes") => true,
        (_, "local_sql") => true,

        // Pro features - require BYOK or WorkspaceKey
        (AuthMode::BYOK { .. }, "pro_recipes") => true,
        (AuthMode::WorkspaceKey { .. }, "pro_recipes") => true,
        (AuthMode::BYOK { .. }, "ai_queries") => true,
        (AuthMode::WorkspaceKey { .. }, "ai_queries") => true,

        // Team features - require WorkspaceKey only
        (AuthMode::WorkspaceKey { .. }, "cloud_sync") => true,
        (AuthMode::WorkspaceKey { .. }, "team_sharing") => true,
        (AuthMode::WorkspaceKey { .. }, "performance_mode") => true,

        // Default deny
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
    fn test_byok_mode_features() {
        let mode = AuthMode::BYOK {
            provider: "anthropic".to_string(),
            api_key: "sk-ant-test-key".to_string(),
        };

        // FREE features should be available
        assert!(feature_available(&mode, "free_recipes"));
        assert!(feature_available(&mode, "local_sql"));

        // PRO features should be available
        assert!(feature_available(&mode, "pro_recipes"));
        assert!(feature_available(&mode, "ai_queries"));

        // TEAM features should still be blocked
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
        let local1 = AuthMode::LocalOnly;
        let local2 = AuthMode::LocalOnly;
        assert_eq!(local1, local2);

        let byok1 = AuthMode::BYOK {
            provider: "anthropic".to_string(),
            api_key: "key1".to_string(),
        };
        let byok2 = AuthMode::BYOK {
            provider: "anthropic".to_string(),
            api_key: "key1".to_string(),
        };
        assert_eq!(byok1, byok2);

        let workspace1 = AuthMode::WorkspaceKey {
            key: "key1".to_string(),
        };
        let workspace2 = AuthMode::WorkspaceKey {
            key: "key1".to_string(),
        };
        assert_eq!(workspace1, workspace2);
    }

    #[test]
    fn test_auth_mode_serialization() {
        use serde_json;

        // LocalOnly serialization
        let local = AuthMode::LocalOnly;
        let json = serde_json::to_string(&local).unwrap();
        assert!(json.contains("LocalOnly"));

        // BYOK serialization (api_key should be skipped)
        let byok = AuthMode::BYOK {
            provider: "anthropic".to_string(),
            api_key: "secret-key".to_string(),
        };
        let json = serde_json::to_string(&byok).unwrap();
        assert!(json.contains("BYOK"));
        assert!(json.contains("anthropic"));
        // api_key should not be serialized
        assert!(!json.contains("secret-key"));

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
        // Verify that each tier builds on the previous one
        let modes = vec![
            ("LocalOnly", AuthMode::LocalOnly),
            (
                "BYOK",
                AuthMode::BYOK {
                    provider: "test".to_string(),
                    api_key: "test".to_string(),
                },
            ),
            (
                "WorkspaceKey",
                AuthMode::WorkspaceKey {
                    key: "test".to_string(),
                },
            ),
        ];

        // Count available features for each mode
        let test_features = vec![
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
            .filter(|f| feature_available(&modes[0].1, f))
            .count();
        let byok_count = test_features
            .iter()
            .filter(|f| feature_available(&modes[1].1, f))
            .count();
        let workspace_count = test_features
            .iter()
            .filter(|f| feature_available(&modes[2].1, f))
            .count();

        // Each tier should have more features than the previous
        assert!(local_count < byok_count);
        assert!(byok_count < workspace_count);

        // Expected counts
        assert_eq!(local_count, 2); // free_recipes, local_sql
        assert_eq!(byok_count, 4); // + pro_recipes, ai_queries
        assert_eq!(workspace_count, 7); // + cloud_sync, team_sharing, performance_mode
    }
}

use axum::{
    extract::Query,
    response::Html,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::oneshot;
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

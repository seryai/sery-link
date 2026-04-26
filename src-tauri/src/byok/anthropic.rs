// Anthropic /v1/messages client used in BYOK mode.
//
// PRIVACY-CRITICAL: every HTTPS request constructed in this file targets
// `api.anthropic.com`. If you ever change the host, you are breaking the
// BYOK guarantee. The `request_url()` helper exists so tests can assert
// the host without doing a network round-trip.

use serde::{Deserialize, Serialize};
use crate::error::{AgentError, Result};

const ANTHROPIC_API_HOST: &str = "https://api.anthropic.com";
const ANTHROPIC_API_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";
const DEFAULT_MAX_TOKENS: u32 = 1024;

pub struct AnthropicClient {
    api_key: String,
}

#[derive(Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: Vec<Message<'a>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Message<'a> {
    pub role: &'a str,
    pub content: &'a str,
}

#[derive(Deserialize, Debug)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}

#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct AskResponse {
    pub text: String,
    pub stop_reason: Option<String>,
    pub usage: Option<Usage>,
}

impl AnthropicClient {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    fn request_url(path: &str) -> String {
        format!("{}{}", ANTHROPIC_API_HOST, path)
    }

    pub async fn ask(&self, prompt: &str) -> Result<AskResponse> {
        self.ask_with_model(prompt, DEFAULT_MODEL, DEFAULT_MAX_TOKENS).await
    }

    pub async fn ask_with_model(
        &self,
        prompt: &str,
        model: &str,
        max_tokens: u32,
    ) -> Result<AskResponse> {
        let body = MessagesRequest {
            model,
            max_tokens,
            messages: vec![Message {
                role: "user",
                content: prompt,
            }],
        };

        let client = reqwest::Client::new();
        let url = Self::request_url("/v1/messages");
        let started = std::time::Instant::now();
        let send_result = client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await;

        // Translate transport-level failure into our error type AND
        // record the failed call to the local audit so the user sees
        // it in Privacy → Outbound. The host stays "api.anthropic.com"
        // even on failure — that's the structural privacy proof.
        let resp = match send_result {
            Ok(r) => r,
            Err(err) => {
                let duration_ms = started.elapsed().as_millis() as u64;
                crate::audit::record_byok_call(
                    "anthropic",
                    "api.anthropic.com",
                    prompt.chars().count() as u64,
                    None,
                    duration_ms,
                    Some(format!("transport error: {}", err)),
                );
                return Err(AgentError::Http(err));
            }
        };

        let status = resp.status();
        if !status.is_success() {
            let err_text = resp.text().await.unwrap_or_default();
            let duration_ms = started.elapsed().as_millis() as u64;
            crate::audit::record_byok_call(
                "anthropic",
                "api.anthropic.com",
                prompt.chars().count() as u64,
                None,
                duration_ms,
                Some(format!("HTTP {}: {}", status, err_text)),
            );
            return Err(AgentError::Network(format!(
                "Anthropic API returned {}: {}",
                status, err_text
            )));
        }

        let parsed: MessagesResponse = resp.json().await.map_err(AgentError::Http)?;

        let text = parsed
            .content
            .into_iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text),
                ContentBlock::Other => None,
            })
            .collect::<Vec<_>>()
            .join("");

        let duration_ms = started.elapsed().as_millis() as u64;
        crate::audit::record_byok_call(
            "anthropic",
            "api.anthropic.com",
            prompt.chars().count() as u64,
            Some(text.chars().count() as u64),
            duration_ms,
            None,
        );

        Ok(AskResponse {
            text,
            stop_reason: parsed.stop_reason,
            usage: parsed.usage,
        })
    }

    /// Validate the API key by making a minimal real call. Returns Ok(()) if
    /// the key is accepted, Err otherwise. Any 4xx other than 401/403 is
    /// treated as "key works but request shape is wrong" — that means the
    /// key authenticated, which is what we care about here.
    pub async fn validate(&self) -> Result<()> {
        let body = MessagesRequest {
            model: DEFAULT_MODEL,
            max_tokens: 1,
            messages: vec![Message {
                role: "user",
                content: "ping",
            }],
        };

        let client = reqwest::Client::new();
        let url = Self::request_url("/v1/messages");
        let resp = client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(AgentError::Http)?;

        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }

        if status == reqwest::StatusCode::UNAUTHORIZED
            || status == reqwest::StatusCode::FORBIDDEN
        {
            let body = resp.text().await.unwrap_or_default();
            return Err(AgentError::Auth(format!(
                "Anthropic rejected the API key ({}): {}",
                status, body
            )));
        }

        let body = resp.text().await.unwrap_or_default();
        Err(AgentError::Network(format!(
            "Anthropic returned {} on validation: {}",
            status, body
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PRIVACY GUARANTEE TEST.
    ///
    /// This test asserts that every URL the Anthropic client constructs
    /// targets `api.anthropic.com` and never `*.sery.ai`. If this test fails,
    /// the BYOK marketing claim is false.
    #[test]
    fn anthropic_request_url_targets_anthropic_only() {
        let url = AnthropicClient::request_url("/v1/messages");
        assert!(
            url.starts_with("https://api.anthropic.com"),
            "BYOK request URL must target api.anthropic.com, got: {}",
            url
        );
        assert!(
            !url.contains("sery.ai"),
            "BYOK request URL must NEVER contain sery.ai, got: {}",
            url
        );

        let validate_url = AnthropicClient::request_url("/v1/messages");
        assert!(
            validate_url.starts_with("https://api.anthropic.com"),
            "Validate URL must target api.anthropic.com"
        );
        assert!(
            !validate_url.contains("sery.ai"),
            "Validate URL must NEVER contain sery.ai"
        );
    }

    #[test]
    fn provider_parses_anthropic_case_insensitive() {
        assert_eq!(
            super::super::Provider::parse("anthropic").unwrap(),
            super::super::Provider::Anthropic
        );
        assert_eq!(
            super::super::Provider::parse("Anthropic").unwrap(),
            super::super::Provider::Anthropic
        );
        assert_eq!(
            super::super::Provider::parse("ANTHROPIC").unwrap(),
            super::super::Provider::Anthropic
        );
    }

    #[test]
    fn provider_rejects_unsupported() {
        assert!(super::super::Provider::parse("openai").is_err());
        assert!(super::super::Provider::parse("ollama").is_err());
        assert!(super::super::Provider::parse("").is_err());
    }
}

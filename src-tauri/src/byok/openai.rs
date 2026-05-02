// OpenAI /v1/chat/completions client used in BYOK mode.
//
// PRIVACY-CRITICAL: every HTTPS request constructed in this file targets
// `api.openai.com`. If you ever change the host, you are breaking the
// BYOK guarantee. The `request_url()` helper exists so tests can assert
// the host without doing a network round-trip.

use crate::error::{AgentError, Result};
use serde::{Deserialize, Serialize};

const OPENAI_API_HOST: &str = "https://api.openai.com";
/// gpt-4o-mini is the cheapest current chat model with good-enough
/// quality for the Ask page's "single-shot Q&A on local files" use
/// case. Users can change models later via a model picker if we add
/// one; for v0.6 the single default keeps the surface area small.
const DEFAULT_MODEL: &str = "gpt-4o-mini";
const DEFAULT_MAX_TOKENS: u32 = 1024;

pub struct OpenAiClient {
    api_key: String,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
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
struct ChatResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize, Debug)]
struct Choice {
    message: AssistantMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
struct AssistantMessage {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
}

impl OpenAiClient {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    fn request_url(path: &str) -> String {
        format!("{}{}", OPENAI_API_HOST, path)
    }

    pub async fn ask(
        &self,
        prompt: &str,
    ) -> Result<crate::byok::anthropic::AskResponse> {
        self.ask_with_model(prompt, DEFAULT_MODEL, DEFAULT_MAX_TOKENS).await
    }

    pub async fn ask_with_model(
        &self,
        prompt: &str,
        model: &str,
        max_tokens: u32,
    ) -> Result<crate::byok::anthropic::AskResponse> {
        let body = ChatRequest {
            model,
            max_tokens,
            messages: vec![Message {
                role: "user",
                content: prompt,
            }],
        };

        let client = reqwest::Client::new();
        let url = Self::request_url("/v1/chat/completions");
        let started = std::time::Instant::now();
        let send_result = client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await;

        let resp = match send_result {
            Ok(r) => r,
            Err(err) => {
                let duration_ms = started.elapsed().as_millis() as u64;
                crate::audit::record_byok_call(
                    "openai",
                    "api.openai.com",
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
                "openai",
                "api.openai.com",
                prompt.chars().count() as u64,
                None,
                duration_ms,
                Some(format!("HTTP {}: {}", status, err_text)),
            );
            return Err(AgentError::Network(format!(
                "OpenAI API returned {}: {}",
                status, err_text
            )));
        }

        let parsed: ChatResponse = resp.json().await.map_err(AgentError::Http)?;
        let stop_reason = parsed
            .choices
            .first()
            .and_then(|c| c.finish_reason.clone());
        let text = parsed
            .choices
            .into_iter()
            .filter_map(|c| c.message.content)
            .collect::<Vec<_>>()
            .join("");

        let duration_ms = started.elapsed().as_millis() as u64;
        crate::audit::record_byok_call(
            "openai",
            "api.openai.com",
            prompt.chars().count() as u64,
            Some(text.chars().count() as u64),
            duration_ms,
            None,
        );

        // Reuse the AskResponse shape from anthropic.rs so the
        // frontend doesn't have to discriminate on provider when
        // rendering the answer. Token-count fields are renamed
        // (OpenAI says prompt_tokens/completion_tokens; Anthropic
        // says input_tokens/output_tokens) — we forward as
        // input/output to keep the JSON shape stable.
        Ok(crate::byok::anthropic::AskResponse {
            text,
            stop_reason,
            usage: parsed.usage.map(|u| crate::byok::anthropic::Usage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
            }),
        })
    }

    /// Validate the API key by making a minimal real call. Returns
    /// Ok(()) if the key is accepted, Err otherwise. 4xx other than
    /// 401/403 means "key authenticated but request shape was wrong"
    /// — that's still a valid key.
    pub async fn validate(&self) -> Result<()> {
        let body = ChatRequest {
            model: DEFAULT_MODEL,
            max_tokens: 1,
            messages: vec![Message {
                role: "user",
                content: "ping",
            }],
        };

        let client = reqwest::Client::new();
        let url = Self::request_url("/v1/chat/completions");
        let resp = client
            .post(&url)
            .bearer_auth(&self.api_key)
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
                "OpenAI rejected the API key ({}): {}",
                status, body
            )));
        }

        let body = resp.text().await.unwrap_or_default();
        Err(AgentError::Network(format!(
            "OpenAI returned {} on validation: {}",
            status, body
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PRIVACY GUARANTEE TEST.
    /// Every URL the OpenAI client constructs must target
    /// api.openai.com — never *.sery.ai. If this fails, the BYOK
    /// promise breaks for OpenAI users.
    #[test]
    fn openai_request_url_targets_openai_only() {
        let url = OpenAiClient::request_url("/v1/chat/completions");
        assert!(
            url.starts_with("https://api.openai.com"),
            "BYOK request URL must target api.openai.com, got: {}",
            url
        );
        assert!(
            !url.contains("sery.ai"),
            "BYOK request URL must NEVER contain sery.ai, got: {}",
            url
        );
    }
}

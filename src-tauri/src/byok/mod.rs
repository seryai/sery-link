// BYOK (Bring Your Own Key) — direct LLM clients.
//
// The marketing claim is: in BYOK mode, your question goes from your
// machine to the LLM provider directly, never traversing sery.ai.
// This module is the single place that calls live LLM APIs from
// sery-link with a user-provided key. If new BYOK code lives outside
// this module, the privacy guarantee becomes harder to verify.

pub mod anthropic;
pub mod gemini;
pub mod openai;

use crate::error::{AgentError, Result};

/// Supported BYOK providers. The string form is what gets persisted
/// (config + keyring entry name) so DON'T rename — those entries
/// would orphan. New variants append; existing strings are stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
    OpenAi,
    Gemini,
}

impl Provider {
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "anthropic" => Ok(Provider::Anthropic),
            "openai" => Ok(Provider::OpenAi),
            "gemini" | "google" => Ok(Provider::Gemini),
            other => Err(AgentError::Validation(format!(
                "Unsupported BYOK provider: {} (anthropic, openai, gemini)",
                other
            ))),
        }
    }

    /// Stable persisted form of the provider name. Used as both the
    /// keyring `account` and the value stored in
    /// `Config::app::selected_byok_provider`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::OpenAi => "openai",
            Provider::Gemini => "gemini",
        }
    }

    /// Iteration order matters for the UI's "first configured
    /// provider wins" lookup in `get_byok_status`.
    pub fn all() -> &'static [Provider] {
        &[Provider::Anthropic, Provider::OpenAi, Provider::Gemini]
    }
}

/// Validate a key against the provider's API. Each provider's
/// validate() makes a minimal call (1-token completion) — costs
/// fractions of a cent and confirms the key works before we save it
/// to the keyring.
pub async fn validate_key(provider: Provider, key: &str) -> Result<()> {
    match provider {
        Provider::Anthropic => {
            anthropic::AnthropicClient::new(key.to_string())
                .validate()
                .await
        }
        Provider::OpenAi => openai::OpenAiClient::new(key.to_string()).validate().await,
        Provider::Gemini => gemini::GeminiClient::new(key.to_string()).validate().await,
    }
}

/// Dispatch a Q&A call against the configured provider, using the
/// per-provider client. The AskResponse shape lives in
/// `anthropic::AskResponse` for historical reasons but is
/// provider-neutral — the OpenAI and Gemini clients map their
/// native usage fields onto it.
pub async fn ask(
    provider: Provider,
    api_key: &str,
    prompt: &str,
) -> Result<anthropic::AskResponse> {
    match provider {
        Provider::Anthropic => {
            anthropic::AnthropicClient::new(api_key.to_string())
                .ask(prompt)
                .await
        }
        Provider::OpenAi => {
            openai::OpenAiClient::new(api_key.to_string())
                .ask(prompt)
                .await
        }
        Provider::Gemini => {
            gemini::GeminiClient::new(api_key.to_string())
                .ask(prompt)
                .await
        }
    }
}

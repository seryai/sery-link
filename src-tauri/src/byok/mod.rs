// BYOK (Bring Your Own Key) — direct LLM client.
//
// The marketing claim is: in BYOK mode, your question goes from your machine
// to the LLM provider directly, never traversing sery.ai. This module is the
// single place that calls live LLM APIs from sery-link with a user-provided
// key. If new BYOK code lives outside this module, the privacy guarantee
// becomes harder to verify.

pub mod anthropic;

use crate::error::{AgentError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
}

impl Provider {
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "anthropic" => Ok(Provider::Anthropic),
            other => Err(AgentError::Validation(format!(
                "Unsupported BYOK provider: {} (only 'anthropic' supported in v0.5.0)",
                other
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
        }
    }
}

pub async fn validate_key(provider: Provider, key: &str) -> Result<()> {
    match provider {
        Provider::Anthropic => anthropic::AnthropicClient::new(key.to_string()).validate().await,
    }
}

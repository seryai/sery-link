// Google Gemini generateContent client used in BYOK mode.
//
// PRIVACY-CRITICAL: every HTTPS request constructed in this file
// targets `generativelanguage.googleapis.com`. If you ever change
// the host, you are breaking the BYOK guarantee.
//
// Quirk: Google's API takes the API key as a query string param
// (`?key=...`), NOT an Authorization header. We follow Google's
// documented form so existing keys minted in AI Studio "just work".

use crate::error::{AgentError, Result};
use serde::{Deserialize, Serialize};

const GEMINI_API_HOST: &str = "https://generativelanguage.googleapis.com";
/// gemini-2.0-flash is fast, cheap, and on Google's free tier as of
/// the v0.6 ship date. Users can upgrade later via a model picker.
const DEFAULT_MODEL: &str = "gemini-2.0-flash";

pub struct GeminiClient {
    api_key: String,
}

#[derive(Serialize)]
struct GenerateRequest<'a> {
    contents: Vec<Content<'a>>,
}

#[derive(Serialize)]
struct Content<'a> {
    parts: Vec<Part<'a>>,
}

#[derive(Serialize)]
struct Part<'a> {
    text: &'a str,
}

#[derive(Deserialize, Debug)]
struct GenerateResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
    #[serde(default, rename = "usageMetadata")]
    usage_metadata: Option<UsageMetadata>,
}

#[derive(Deserialize, Debug)]
struct Candidate {
    #[serde(default)]
    content: Option<CandidateContent>,
    #[serde(default, rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
struct CandidateContent {
    #[serde(default)]
    parts: Vec<CandidatePart>,
}

#[derive(Deserialize, Debug)]
struct CandidatePart {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize, Debug)]
struct UsageMetadata {
    #[serde(default, rename = "promptTokenCount")]
    prompt_token_count: u32,
    #[serde(default, rename = "candidatesTokenCount")]
    candidates_token_count: u32,
}

impl GeminiClient {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    /// Build the request URL for a given model. The API key goes in
    /// the query string per Google's spec; logging this URL would
    /// leak the key, so callers must NOT log raw URLs from this fn.
    /// Tests use `request_url_redacted` instead.
    fn request_url(&self, model: &str) -> String {
        format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            GEMINI_API_HOST, model, self.api_key
        )
    }

    /// Same shape as `request_url` but without the key — for tests
    /// and any future audit surface that needs to record what host
    /// + path the request hit without exposing the secret. Annotated
    /// `allow(dead_code)` because non-test callers don't exist yet
    /// (audit currently logs only the host string, not the full URL).
    #[allow(dead_code)]
    pub fn request_url_redacted(model: &str) -> String {
        format!(
            "{}/v1beta/models/{}:generateContent?key=REDACTED",
            GEMINI_API_HOST, model
        )
    }

    pub async fn ask(
        &self,
        prompt: &str,
    ) -> Result<crate::byok::anthropic::AskResponse> {
        self.ask_with_model(prompt, DEFAULT_MODEL).await
    }

    pub async fn ask_with_model(
        &self,
        prompt: &str,
        model: &str,
    ) -> Result<crate::byok::anthropic::AskResponse> {
        let body = GenerateRequest {
            contents: vec![Content {
                parts: vec![Part { text: prompt }],
            }],
        };

        let client = reqwest::Client::new();
        let url = self.request_url(model);
        let started = std::time::Instant::now();
        let send_result = client
            .post(&url)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await;

        let resp = match send_result {
            Ok(r) => r,
            Err(err) => {
                let duration_ms = started.elapsed().as_millis() as u64;
                crate::audit::record_byok_call(
                    "gemini",
                    "generativelanguage.googleapis.com",
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
                "gemini",
                "generativelanguage.googleapis.com",
                prompt.chars().count() as u64,
                None,
                duration_ms,
                Some(format!("HTTP {}: {}", status, err_text)),
            );
            return Err(AgentError::Network(format!(
                "Gemini API returned {}: {}",
                status, err_text
            )));
        }

        let parsed: GenerateResponse = resp.json().await.map_err(AgentError::Http)?;
        let stop_reason = parsed
            .candidates
            .first()
            .and_then(|c| c.finish_reason.clone());
        let text = parsed
            .candidates
            .into_iter()
            .filter_map(|c| c.content)
            .flat_map(|c| c.parts.into_iter().filter_map(|p| p.text))
            .collect::<Vec<_>>()
            .join("");

        let duration_ms = started.elapsed().as_millis() as u64;
        crate::audit::record_byok_call(
            "gemini",
            "generativelanguage.googleapis.com",
            prompt.chars().count() as u64,
            Some(text.chars().count() as u64),
            duration_ms,
            None,
        );

        // Reuse the AskResponse shape so the frontend doesn't have
        // to discriminate by provider. Map Gemini's
        // promptTokenCount/candidatesTokenCount → input/output.
        Ok(crate::byok::anthropic::AskResponse {
            text,
            stop_reason,
            usage: parsed
                .usage_metadata
                .map(|u| crate::byok::anthropic::Usage {
                    input_tokens: u.prompt_token_count,
                    output_tokens: u.candidates_token_count,
                }),
        })
    }

    /// Validate the API key by making a minimal real call. The
    /// generateContent endpoint will reject an invalid key with 400
    /// `API_KEY_INVALID`; we treat any 4xx as auth-failure unless
    /// it's clearly a request-shape error.
    pub async fn validate(&self) -> Result<()> {
        let body = GenerateRequest {
            contents: vec![Content {
                parts: vec![Part { text: "ping" }],
            }],
        };

        let client = reqwest::Client::new();
        let url = self.request_url(DEFAULT_MODEL);
        let resp = client
            .post(&url)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(AgentError::Http)?;

        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }

        // Google returns 400 for bad keys (with body containing
        // "API_KEY_INVALID") and 403 for disabled / not-allowed.
        // Both are auth failures from the user's perspective.
        if status == reqwest::StatusCode::BAD_REQUEST
            || status == reqwest::StatusCode::UNAUTHORIZED
            || status == reqwest::StatusCode::FORBIDDEN
        {
            let body = resp.text().await.unwrap_or_default();
            return Err(AgentError::Auth(format!(
                "Google rejected the API key ({}): {}",
                status, body
            )));
        }

        let body = resp.text().await.unwrap_or_default();
        Err(AgentError::Network(format!(
            "Gemini returned {} on validation: {}",
            status, body
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PRIVACY GUARANTEE TEST.
    /// Every URL the Gemini client constructs must target
    /// generativelanguage.googleapis.com — never *.sery.ai.
    #[test]
    fn gemini_request_url_targets_google_only() {
        let url = GeminiClient::request_url_redacted("gemini-2.0-flash");
        assert!(
            url.starts_with("https://generativelanguage.googleapis.com"),
            "BYOK request URL must target Google, got: {}",
            url
        );
        assert!(
            !url.contains("sery.ai"),
            "BYOK request URL must NEVER contain sery.ai, got: {}",
            url
        );
    }

    #[test]
    fn redacted_url_does_not_leak_key() {
        // Sanity check that the helper used in logs/audit doesn't
        // accidentally include the real key. The "real" url is
        // built only in instance methods that aren't reachable
        // from logging code paths.
        let url = GeminiClient::request_url_redacted("gemini-2.0-flash");
        assert!(url.contains("REDACTED"));
        assert!(!url.contains("AIza"), "looks like a Google API key leaked: {}", url);
    }
}

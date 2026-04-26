//! Outbound network audit log — records every cloud call the agent makes.
//!
//! This is the transparency layer for the "your data stays local" promise.
//! Users can open the Privacy tab (or the file directly via the
//! "Reveal in Finder" affordance) and see exactly what crossed the
//! network. Two event kinds are recorded today:
//!
//!   * `sync` — metadata uploaded to Sery.ai Cloud (folder, dataset
//!     count, column count, byte size). Has been in the file since v0.4.
//!   * `byok_call` — BYOK LLM call sent direct to the provider's host
//!     (e.g., `api.anthropic.com`). The whole point of recording these
//!     in the LOCAL file is that they never reach Sery's backend, so
//!     Sery's Privacy Dashboard cannot show them. The local audit file
//!     is the only place the user can verify "yes, this prompt went
//!     directly to Anthropic and didn't traverse sery.ai." (F5 + F7.)
//!
//! Stored as JSONL at `~/.seryai/sync_audit.jsonl` (append-only, newest
//! last; the file name is kept for backwards compat with v0.4 readers).
//! Lazily capped at 10 000 entries.

use crate::config::Config;
use crate::error::{AgentError, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

const MAX_AUDIT_ENTRIES: usize = 10_000;

/// Kind discriminator for the entry. Defaults to `sync` so that v0.4
/// audit files (which don't have this field) still deserialize cleanly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditKind {
    Sync,
    ByokCall,
}

impl Default for AuditKind {
    fn default() -> Self {
        AuditKind::Sync
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: String,
    #[serde(default)]
    pub kind: AuditKind,

    // ─── sync fields ───────────────────────────────────────────────────
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub dataset_count: u64,
    #[serde(default)]
    pub column_count: u64,
    #[serde(default)]
    pub total_bytes: u64,

    // ─── byok_call fields (all optional; only populated for byok_call) ──
    /// Provider name, e.g. "anthropic". Lower-cased canonical form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Host the request actually targeted (e.g. "api.anthropic.com").
    /// This is the load-bearing privacy proof — if this ever shows
    /// "*.sery.ai" for a byok_call, the BYOK guarantee is broken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Length of the prompt text we sent in characters (NOT the prompt
    /// itself — we don't log content, only metadata).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_chars: Option<u64>,
    /// Length of the response text in characters (when applicable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_chars: Option<u64>,
    /// Round-trip duration in milliseconds, for ops/debug visibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,

    // ─── shared status fields ──────────────────────────────────────────
    pub status: String, // "success" | "error"
    pub error: Option<String>,
}

/// Absolute path to the audit log file. Exposed publicly so the
/// `reveal_audit_file_in_finder` Tauri command can hand the user the
/// real on-disk location — this is the load-bearing "verify it
/// yourself" affordance for the privacy story.
pub fn audit_file_path() -> Result<PathBuf> {
    Ok(Config::data_dir()?.join("sync_audit.jsonl"))
}

fn path() -> Result<PathBuf> {
    audit_file_path()
}

pub fn append(entry: &AuditEntry) -> Result<()> {
    let p = path()?;
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(AgentError::Io)?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
        .map_err(AgentError::Io)?;

    let json = serde_json::to_string(entry)
        .map_err(|e| AgentError::Config(format!("serialize audit: {}", e)))?;
    writeln!(file, "{}", json).map_err(AgentError::Io)?;

    rotate_if_needed(&p)?;
    Ok(())
}

fn rotate_if_needed(p: &PathBuf) -> Result<()> {
    let file = fs::File::open(p).map_err(AgentError::Io)?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().map_while(|l| l.ok()).collect();
    if lines.len() > (MAX_AUDIT_ENTRIES + MAX_AUDIT_ENTRIES / 10) {
        let keep: Vec<&String> = lines.iter().skip(lines.len() - MAX_AUDIT_ENTRIES).collect();
        let tmp = p.with_extension("jsonl.tmp");
        {
            let mut out = fs::File::create(&tmp).map_err(AgentError::Io)?;
            for line in keep {
                writeln!(out, "{}", line).map_err(AgentError::Io)?;
            }
        }
        fs::rename(&tmp, p).map_err(AgentError::Io)?;
    }
    Ok(())
}

/// Load audit entries newest-first, up to `limit`.
pub fn load(limit: usize) -> Result<Vec<AuditEntry>> {
    let p = path()?;
    if !p.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(&p).map_err(AgentError::Io)?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for line in reader.lines().map_while(|l| l.ok()) {
        if let Ok(entry) = serde_json::from_str::<AuditEntry>(&line) {
            out.push(entry);
        }
    }
    out.reverse();
    out.truncate(limit);
    Ok(out)
}

/// Aggregate the latest audit state per folder — used by the Privacy tab to
/// show "what's currently synced" without scrolling through history.
pub fn latest_by_folder() -> Result<Vec<AuditEntry>> {
    let all = load(usize::MAX)?;
    let mut seen: std::collections::HashMap<String, AuditEntry> = std::collections::HashMap::new();
    // `load` returns newest-first, so the first time we see a folder that's
    // the latest record; skip subsequent older entries for the same folder.
    for entry in all {
        seen.entry(entry.folder.clone()).or_insert(entry);
    }
    let mut result: Vec<AuditEntry> = seen.into_values().collect();
    result.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(result)
}

pub fn clear() -> Result<()> {
    let p = path()?;
    if p.exists() {
        fs::remove_file(&p).map_err(AgentError::Io)?;
    }
    Ok(())
}

pub fn record(folder: &str, dataset_count: u64, column_count: u64, total_bytes: u64, error: Option<String>) {
    let entry = AuditEntry {
        timestamp: Utc::now().to_rfc3339(),
        kind: AuditKind::Sync,
        folder: folder.to_string(),
        dataset_count,
        column_count,
        total_bytes,
        provider: None,
        host: None,
        prompt_chars: None,
        response_chars: None,
        duration_ms: None,
        status: if error.is_none() {
            "success".to_string()
        } else {
            "error".to_string()
        },
        error,
    };
    let _ = append(&entry);
}

/// Record a single BYOK LLM call. Best-effort: failures are silent.
///
/// PRIVACY-CRITICAL: `host` is what proves the call went direct to the
/// provider rather than via Sery's backend. The byok module only ever
/// passes "api.anthropic.com" (or future providers' canonical hosts);
/// if a different host shows up here, the BYOK guarantee is broken.
pub fn record_byok_call(
    provider: &str,
    host: &str,
    prompt_chars: u64,
    response_chars: Option<u64>,
    duration_ms: u64,
    error: Option<String>,
) {
    let entry = AuditEntry {
        timestamp: Utc::now().to_rfc3339(),
        kind: AuditKind::ByokCall,
        folder: String::new(),
        dataset_count: 0,
        column_count: 0,
        total_bytes: 0,
        provider: Some(provider.to_string()),
        host: Some(host.to_string()),
        prompt_chars: Some(prompt_chars),
        response_chars,
        duration_ms: Some(duration_ms),
        status: if error.is_none() {
            "success".to_string()
        } else {
            "error".to_string()
        },
        error,
    };
    let _ = append(&entry);
}

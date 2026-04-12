//! Sync audit log — records what metadata the agent uploaded to the cloud.
//!
//! This is the transparency layer for the "your data stays local" promise.
//! Users can open the Privacy tab and see exactly which folders, datasets
//! and columns were shipped to Sery.ai Cloud, so nothing is hidden.
//!
//! Stored as JSONL at `~/.seryai/sync_audit.jsonl` (append-only, newest
//! last). Lazily capped at 10 000 entries.

use crate::config::Config;
use crate::error::{AgentError, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

const MAX_AUDIT_ENTRIES: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: String,
    pub folder: String,
    pub dataset_count: u64,
    pub column_count: u64,
    pub total_bytes: u64,
    pub status: String, // "success" | "error"
    pub error: Option<String>,
}

fn path() -> Result<PathBuf> {
    Ok(Config::data_dir()?.join("sync_audit.jsonl"))
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
        folder: folder.to_string(),
        dataset_count,
        column_count,
        total_bytes,
        status: if error.is_none() {
            "success".to_string()
        } else {
            "error".to_string()
        },
        error,
    };
    let _ = append(&entry);
}

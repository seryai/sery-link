//! Append-only log of Drive files we couldn't (or chose not to)
//! cache. Surfaces in Settings → Storage so users can audit what
//! Sery skipped and why, instead of seeing silent zeros at search
//! time.
//!
//! Storage: `~/.seryai/gdrive-skipped.jsonl` — one JSON object per
//! line. Append-only and bounded by `MAX_LINES`; oldest entries
//! are dropped when the cap is hit so the file stays small even on
//! big Drives.
//!
//! Why JSONL and not a DuckDB table? The data is write-mostly,
//! read-rarely (only the Storage page consumes it), and the
//! existing `scan_cache.db` is locked-open by the running app,
//! which complicates writes from other code paths. A flat file
//! has no lock contention and can be read by `tail` for debugging.

use crate::config::Config;
use crate::error::{AgentError, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

/// Hard cap on lines retained. Picked so a user with a million-
/// file Drive doesn't blow up disk just from the skipped log.
/// At ~250 bytes per JSON line, 10k entries ≈ 2.5 MB.
const MAX_LINES: usize = 10_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// Google-native type with no /export mapping (Forms, Sites,
    /// Drawings). Could move to `Skipped` if/when we add export
    /// support.
    NativeUnexportable,
    /// Extension isn't in the scanner's supported list (mp4, zip,
    /// exe, raw photos, ...). The walker filters these out.
    UnsupportedExtension,
    /// Single file exceeded the 1 GiB per-file download cap.
    TooLarge,
    /// Network or write failure during download. Re-tried on the
    /// next refresh tick — listed here so users can see flapping.
    DownloadFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedEntry {
    pub account_id: String,
    /// The Drive folder the watch was scoped to.
    pub watch_folder_id: String,
    pub file_id: String,
    pub name: String,
    pub mime_type: String,
    /// Bytes Drive reported. None for native types (Drive doesn't
    /// report size for Docs/Sheets/etc.).
    pub size_bytes: Option<u64>,
    pub reason: SkipReason,
    /// RFC 3339 timestamp.
    pub skipped_at: String,
    /// Free-form context (e.g. the underlying Network error
    /// message for DownloadFailed). Kept short — full stack traces
    /// stay in stderr.
    #[serde(default)]
    pub detail: Option<String>,
}

fn log_path() -> Result<PathBuf> {
    Ok(Config::data_dir()?.join("gdrive-skipped.jsonl"))
}

/// Append a single entry. Best-effort: failures are logged but
/// not bubbled up — the caller is mid-watch and a busted log
/// shouldn't abort the user's actual work.
pub fn record(entry: &SkippedEntry) {
    if let Err(e) = record_inner(entry) {
        eprintln!("[gdrive-skipped] log write failed: {}", e);
    }
}

fn record_inner(entry: &SkippedEntry) -> Result<()> {
    let path = log_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AgentError::Config(format!("create skipped log dir: {}", e)))?;
    }

    let json = serde_json::to_string(entry)
        .map_err(|e| AgentError::Serialization(format!("serialize skipped: {}", e)))?;

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| AgentError::Config(format!("open skipped log: {}", e)))?;
    writeln!(file, "{}", json)
        .map_err(|e| AgentError::Config(format!("write skipped log: {}", e)))?;

    // Best-effort rotation: only check periodically (every 256
    // writes, hashed off the current entry length) to keep the hot
    // path cheap. The file growing past MAX_LINES temporarily is
    // fine — rotation eventually catches up.
    if json.len() % 256 == 0 {
        let _ = rotate_if_needed(&path);
    }
    Ok(())
}

fn rotate_if_needed(path: &PathBuf) -> Result<()> {
    let lines: Vec<String> = std::fs::File::open(path)
        .ok()
        .map(|f| BufReader::new(f).lines().map_while(|r| r.ok()).collect())
        .unwrap_or_default();
    if lines.len() <= MAX_LINES {
        return Ok(());
    }
    let keep = lines.len() - MAX_LINES;
    let kept = &lines[keep..];
    let tmp = path.with_extension("jsonl.tmp");
    let body = kept.join("\n") + "\n";
    std::fs::write(&tmp, body)
        .map_err(|e| AgentError::Config(format!("write rotated: {}", e)))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| AgentError::Config(format!("rotate: {}", e)))?;
    Ok(())
}

/// Load up to `limit` most-recent entries (newest first). Returns
/// an empty list if the log doesn't exist (clean install / never-
/// watched Drive).
pub fn recent(limit: usize) -> Result<Vec<SkippedEntry>> {
    let path = log_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = std::fs::File::open(&path)
        .map_err(|e| AgentError::Config(format!("open skipped log: {}", e)))?;
    let mut entries: Vec<SkippedEntry> = BufReader::new(file)
        .lines()
        .map_while(|r| r.ok())
        .filter_map(|l| serde_json::from_str(&l).ok())
        .collect();
    entries.reverse();
    entries.truncate(limit);
    Ok(entries)
}

/// Count entries grouped by reason. Used by the Storage page for
/// the "X Docs/Forms · Y too big · Z unsupported" summary line
/// without loading the full list into memory.
pub fn count_by_reason() -> Result<std::collections::HashMap<SkipReason, usize>> {
    let path = log_path()?;
    let mut counts = std::collections::HashMap::new();
    if !path.exists() {
        return Ok(counts);
    }
    let file = std::fs::File::open(&path)
        .map_err(|e| AgentError::Config(format!("open skipped log: {}", e)))?;
    for line in BufReader::new(file).lines().map_while(|r| r.ok()) {
        if let Ok(entry) = serde_json::from_str::<SkippedEntry>(&line) {
            *counts.entry(entry.reason).or_insert(0) += 1;
        }
    }
    Ok(counts)
}

/// Wipe the log. Called by `clear_gdrive_cache` since the cache
/// being gone means the skip context is also stale.
pub fn clear() -> Result<()> {
    let path = log_path()?;
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| AgentError::Config(format!("remove skipped log: {}", e)))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_round_trip() {
        let e = SkippedEntry {
            account_id: "default".into(),
            watch_folder_id: "root".into(),
            file_id: "abc123".into(),
            name: "video.mp4".into(),
            mime_type: "video/mp4".into(),
            size_bytes: Some(5_000_000_000),
            reason: SkipReason::TooLarge,
            skipped_at: "2026-05-02T10:00:00Z".into(),
            detail: Some("over 1 GiB".into()),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: SkippedEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.file_id, "abc123");
        assert_eq!(back.reason, SkipReason::TooLarge);
        assert_eq!(back.size_bytes, Some(5_000_000_000));
    }

    #[test]
    fn reason_serializes_snake_case() {
        let json = serde_json::to_string(&SkipReason::NativeUnexportable).unwrap();
        assert_eq!(json, "\"native_unexportable\"");
        let json = serde_json::to_string(&SkipReason::UnsupportedExtension).unwrap();
        assert_eq!(json, "\"unsupported_extension\"");
    }
}

//! Persistent local query history (JSONL) for the Sery Link agent.
//!
//! Every query executed over the WebSocket tunnel gets appended to
//! `~/.seryai/query_history.jsonl` with one JSON object per line. The file is
//! read back on demand by the frontend via the `get_query_history` command.

use crate::error::{AgentError, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

const MAX_HISTORY_LINES: usize = 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryHistoryEntry {
    /// ISO-8601 timestamp when the query was recorded.
    pub timestamp: String,
    /// Optional query_id from the cloud (maps back to a chat message).
    pub query_id: Option<String>,
    /// Target file path (truncated at display time if needed).
    pub file_path: String,
    /// The SQL that was run (truncated to 2000 chars).
    pub sql: String,
    /// `"success"` or `"error"`.
    pub status: String,
    /// Row count on success, None on error.
    pub row_count: Option<usize>,
    /// Duration of the query in milliseconds.
    pub duration_ms: u64,
    /// Error string on failure, None on success.
    pub error: Option<String>,
}

fn history_path() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| AgentError::Config("Could not find home directory".to_string()))?;
    Ok(home.join(".seryai").join("query_history.jsonl"))
}

/// Append a single entry to the history file. This is called from the
/// WebSocket query handler; failures are logged but do not propagate so
/// history never breaks actual query execution.
pub fn append_entry(entry: &QueryHistoryEntry) {
    if let Err(e) = append_entry_inner(entry) {
        eprintln!("[history] failed to append entry: {}", e);
    }
}

fn append_entry_inner(entry: &QueryHistoryEntry) -> Result<()> {
    let path = history_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(AgentError::Io)?;
    }

    let line = serde_json::to_string(entry)
        .map_err(|e| AgentError::Serialization(format!("history serialize: {}", e)))?;

    {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(AgentError::Io)?;
        writeln!(file, "{}", line).map_err(AgentError::Io)?;
    }

    // Lazy rotation: if the file exceeds the cap by more than 20%, compact it.
    if let Ok(metadata) = fs::metadata(&path) {
        // Heuristic: only check rotation every ~10KB appended
        if metadata.len() > 0 && metadata.len() % 10_240 < 200 {
            let _ = rotate_if_needed(&path);
        }
    }

    Ok(())
}

fn rotate_if_needed(path: &PathBuf) -> Result<()> {
    let file = fs::File::open(path).map_err(AgentError::Io)?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().map_while(|l| l.ok()).collect();

    if lines.len() > (MAX_HISTORY_LINES + MAX_HISTORY_LINES / 5) {
        // Keep only the last MAX_HISTORY_LINES entries
        let keep: Vec<&String> = lines
            .iter()
            .skip(lines.len() - MAX_HISTORY_LINES)
            .collect();
        let tmp = path.with_extension("jsonl.tmp");
        {
            let mut out = fs::File::create(&tmp).map_err(AgentError::Io)?;
            for line in keep {
                writeln!(out, "{}", line).map_err(AgentError::Io)?;
            }
        }
        fs::rename(&tmp, path).map_err(AgentError::Io)?;
    }
    Ok(())
}

/// Load the most recent `limit` history entries, newest first.
pub fn load_history(limit: usize) -> Result<Vec<QueryHistoryEntry>> {
    let path = history_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = fs::File::open(&path).map_err(AgentError::Io)?;
    let reader = BufReader::new(file);

    let mut entries: Vec<QueryHistoryEntry> = Vec::new();
    for line in reader.lines().map_while(|l| l.ok()) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<QueryHistoryEntry>(trimmed) {
            Ok(entry) => entries.push(entry),
            Err(e) => {
                eprintln!("[history] skipping malformed line: {}", e);
            }
        }
    }

    // Newest first
    entries.reverse();
    entries.truncate(limit);
    Ok(entries)
}

/// Clear all history.
pub fn clear_history() -> Result<()> {
    let path = history_path()?;
    if path.exists() {
        fs::remove_file(&path).map_err(AgentError::Io)?;
    }
    Ok(())
}

/// Convenience constructor used by the query executor.
pub fn record(
    query_id: Option<String>,
    file_path: &str,
    sql: &str,
    status: &str,
    row_count: Option<usize>,
    duration_ms: u64,
    error: Option<String>,
) {
    let truncated_sql = if sql.len() > 2000 {
        format!("{}…", &sql[..2000])
    } else {
        sql.to_string()
    };

    let entry = QueryHistoryEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        query_id,
        file_path: file_path.to_string(),
        sql: truncated_sql,
        status: status.to_string(),
        row_count,
        duration_ms,
        error,
    };
    append_entry(&entry);
}

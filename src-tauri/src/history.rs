//! Persistent local query history backed by SQLite.
//!
//! Every query executed over the WebSocket tunnel is appended to the
//! `query_history` table in `~/.seryai/sery.db`. The table is capped at
//! MAX_HISTORY_ROWS rows; older rows are deleted on each insert via a
//! single `DELETE … NOT IN (… ORDER BY id DESC LIMIT N)` statement.
//!
//! On first run the code migrates any existing
//! `~/.seryai/query_history.jsonl` file into SQLite so history is not lost.

use crate::error::{AgentError, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_HISTORY_ROWS: usize = 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryHistoryEntry {
    /// ISO-8601 timestamp when the query was recorded.
    pub timestamp: String,
    /// Optional query_id from the cloud (maps back to a chat message).
    pub query_id: Option<String>,
    /// Natural-language question that triggered this SQL (forwarded by
    /// the cloud API via the WebSocket message when available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
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

fn db_path() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| AgentError::Config("Could not find home directory".to_string()))?;
    Ok(home.join(".seryai").join("sery.db"))
}

fn open_db() -> Result<Connection> {
    let path = db_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(AgentError::Io)?;
    }
    let conn = Connection::open(&path)
        .map_err(|e| AgentError::Config(format!("SQLite open: {e}")))?;
    // WAL mode: readers don't block writers and vice-versa.
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
        .map_err(|e| AgentError::Config(format!("SQLite pragma: {e}")))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS query_history (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp   TEXT    NOT NULL,
            query_id    TEXT,
            question    TEXT,
            file_path   TEXT    NOT NULL,
            sql         TEXT    NOT NULL,
            status      TEXT    NOT NULL,
            row_count   INTEGER,
            duration_ms INTEGER NOT NULL DEFAULT 0,
            error       TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_qh_id ON query_history(id DESC);",
    )
    .map_err(|e| AgentError::Config(format!("SQLite create table: {e}")))?;

    // One-time migration from the legacy JSONL file.
    migrate_from_jsonl(&conn);

    Ok(conn)
}

/// Import `~/.seryai/query_history.jsonl` into SQLite (once).
///
/// Runs only when the table is empty AND the JSONL file exists.
/// After a successful import the JSONL file is renamed to
/// `query_history.jsonl.migrated` so it is never re-imported.
fn migrate_from_jsonl(conn: &Connection) {
    // Skip if the table already has rows — migration already ran.
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM query_history", [], |r| r.get(0))
        .unwrap_or(1);
    if count > 0 {
        return;
    }

    let jsonl_path = match dirs::home_dir() {
        Some(h) => h.join(".seryai").join("query_history.jsonl"),
        None => return,
    };
    if !jsonl_path.exists() {
        return;
    }

    let content = match std::fs::read_to_string(&jsonl_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut imported = 0usize;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(e) = serde_json::from_str::<QueryHistoryEntry>(trimmed) {
            let _ = conn.execute(
                "INSERT INTO query_history
                 (timestamp, query_id, question, file_path, sql, status, row_count, duration_ms, error)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    e.timestamp,
                    e.query_id,
                    e.question,
                    e.file_path,
                    e.sql,
                    e.status,
                    e.row_count.map(|n| n as i64),
                    e.duration_ms as i64,
                    e.error,
                ],
            );
            imported += 1;
        }
    }

    if imported > 0 {
        eprintln!("[history] migrated {imported} entries from JSONL to SQLite");
        let done = jsonl_path.with_extension("jsonl.migrated");
        let _ = std::fs::rename(&jsonl_path, &done);
    }
}

/// Append a single entry. Failures are logged but never propagate so
/// history never breaks actual query execution.
pub fn append_entry(entry: &QueryHistoryEntry) {
    if let Err(e) = append_entry_inner(entry) {
        eprintln!("[history] failed to append entry: {}", e);
    }
}

fn append_entry_inner(entry: &QueryHistoryEntry) -> Result<()> {
    let conn = open_db()?;
    conn.execute(
        "INSERT INTO query_history
         (timestamp, query_id, question, file_path, sql, status, row_count, duration_ms, error)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            entry.timestamp,
            entry.query_id,
            entry.question,
            entry.file_path,
            entry.sql,
            entry.status,
            entry.row_count.map(|n| n as i64),
            entry.duration_ms as i64,
            entry.error,
        ],
    )
    .map_err(|e| AgentError::Config(format!("SQLite insert: {e}")))?;

    // Trim to MAX_HISTORY_ROWS — keep the newest rows.
    conn.execute(
        "DELETE FROM query_history
         WHERE id NOT IN (
             SELECT id FROM query_history ORDER BY id DESC LIMIT ?1
         )",
        params![MAX_HISTORY_ROWS as i64],
    )
    .map_err(|e| AgentError::Config(format!("SQLite trim: {e}")))?;

    Ok(())
}

/// Load the most recent `limit` history entries, newest first.
pub fn load_history(limit: usize) -> Result<Vec<QueryHistoryEntry>> {
    let conn = open_db()?;
    let mut stmt = conn
        .prepare(
            "SELECT timestamp, query_id, question, file_path, sql,
                    status, row_count, duration_ms, error
             FROM query_history
             ORDER BY id DESC
             LIMIT ?1",
        )
        .map_err(|e| AgentError::Config(format!("SQLite prepare: {e}")))?;

    let entries: Vec<QueryHistoryEntry> = stmt
        .query_map(params![limit as i64], |row| {
            Ok(QueryHistoryEntry {
                timestamp: row.get(0)?,
                query_id: row.get(1)?,
                question: row.get(2)?,
                file_path: row.get(3)?,
                sql: row.get(4)?,
                status: row.get(5)?,
                row_count: row.get::<_, Option<i64>>(6)?.map(|n| n as usize),
                duration_ms: row.get::<_, i64>(7)? as u64,
                error: row.get(8)?,
            })
        })
        .map_err(|e| AgentError::Config(format!("SQLite query: {e}")))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(entries)
}

/// Clear all history.
pub fn clear_history() -> Result<()> {
    let conn = open_db()?;
    conn.execute("DELETE FROM query_history", [])
        .map_err(|e| AgentError::Config(format!("SQLite clear: {e}")))?;
    Ok(())
}

/// Convenience constructor used by the query executor.
pub fn record(
    query_id: Option<String>,
    question: Option<String>,
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

    append_entry(&QueryHistoryEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        query_id,
        question,
        file_path: file_path.to_string(),
        sql: truncated_sql,
        status: status.to_string(),
        row_count,
        duration_ms,
        error,
    });
}

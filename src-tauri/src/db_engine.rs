//! F52 — Database query execution and schema introspection.
//!
//! Handles MySQL and PostgreSQL sources via DuckDB's community extensions
//! (mysql_scanner, postgres_scanner). The extension is installed at
//! runtime on first use; subsequent connections reuse the cached install.
//!
//! Security model:
//!   - SELECT only. DDL/DML keywords are rejected before the query runs.
//!   - Credentials are loaded from the OS keychain, never from the query string.
//!   - READ_ONLY is enforced at the ATTACH level.
//!   - 100 000 row cap and 60 s timeout match the file-based engine.

use crate::config::Config;
use crate::duckdb_engine::QueryResult;
use crate::error::{AgentError, Result};
use crate::sources::SourceKind;
use duckdb::Connection;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const MAX_ROWS: usize = 100_000;
const TIMEOUT_SECS: u64 = 60;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TableSchema {
    pub table_name: String,
    pub columns: Vec<ColumnInfo>,
    pub row_count_estimate: Option<i64>,
}

/// Validate that the SQL is SELECT-only. DB sources accept plain SQL
/// (no {{file}} placeholder) but must not mutate data.
pub fn validate_db_sql(sql: &str) -> Result<()> {
    let lower = sql.trim().to_ascii_lowercase();
    const FORBIDDEN: &[&str] = &[
        "insert", "update", "delete", "drop", "create", "alter",
        "truncate", "replace", "merge", "call", "exec",
        "grant", "revoke", "attach", "detach", "install", "load",
        "copy", "export", "import",
    ];
    for kw in FORBIDDEN {
        if contains_word(&lower, kw) {
            return Err(AgentError::Database(format!(
                "DB queries must be SELECT-only. Forbidden keyword: {kw}"
            )));
        }
    }
    Ok(())
}

fn contains_word(text: &str, word: &str) -> bool {
    let bytes = text.as_bytes();
    let wbytes = word.as_bytes();
    let mut i = 0usize;
    while i + wbytes.len() <= bytes.len() {
        if &bytes[i..i + wbytes.len()] == wbytes {
            let before_ok = i == 0 || !is_ident_char(bytes[i - 1]);
            let after_ok = i + wbytes.len() == bytes.len()
                || !is_ident_char(bytes[i + wbytes.len()]);
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Build the DuckDB ATTACH connection string for a source.
fn build_attach_string(
    kind: &SourceKind,
    password: &str,
) -> Option<(&'static str, String)> {
    match kind {
        SourceKind::Mysql {
            host,
            port,
            username,
            database,
        } => {
            let conn_str = format!(
                "host={host} port={port} database={database} user={username} password={password}"
            );
            Some(("MYSQL", conn_str))
        }
        SourceKind::Postgresql {
            host,
            port,
            username,
            database,
        } => {
            let conn_str = format!(
                "host={host} port={port} dbname={database} user={username} password={password}"
            );
            Some(("POSTGRES", conn_str))
        }
        _ => None,
    }
}

/// Return the DuckDB extension name for a SourceKind.
fn extension_name(kind: &SourceKind) -> Option<&'static str> {
    match kind {
        SourceKind::Mysql { .. } => Some("mysql"),
        SourceKind::Postgresql { .. } => Some("postgres"),
        _ => None,
    }
}

/// Default schema name inside the attached DuckDB database alias.
/// MySQL uses 'main'; PostgreSQL uses 'main' too (DuckDB maps pg 'public' to 'main').
fn default_schema(_kind: &SourceKind) -> &'static str {
    "main"
}

/// Lookup a DB source in config and load its password from the keychain.
fn resolve_source<'a>(
    source_id: &str,
    config: &'a Config,
) -> Result<(&'a SourceKind, String)> {
    let source = config
        .sources
        .iter()
        .find(|s| s.id == source_id)
        .ok_or_else(|| AgentError::Database(format!("DB source not found: {source_id}")))?;

    let password = crate::db_creds::load(&source.id)?
        .ok_or_else(|| {
            AgentError::Database(format!(
                "No credentials stored for source {source_id} — reconnect the source."
            ))
        })?;

    Ok((&source.kind, password))
}

/// Execute a SELECT query against a DB source identified by source_id.
///
/// Called from `agent_rpc/commands/sql.rs` when the tunnel delivers
/// `path = "db://<source_id>"`.
pub async fn execute_db_query(
    sql: &str,
    source_id: &str,
    config: &Config,
) -> Result<QueryResult> {
    validate_db_sql(sql)?;

    let (kind, password) = resolve_source(source_id, config)?;
    let (db_type, conn_str) = build_attach_string(kind, &password)
        .ok_or_else(|| AgentError::Database("Source is not a database type".to_string()))?;
    let ext = extension_name(kind).unwrap();
    let schema = default_schema(kind);
    let sql_owned = sql.to_string();

    let task = tokio::task::spawn_blocking(move || {
        run_db_query_blocking(&sql_owned, &conn_str, db_type, ext, schema)
    });

    match tokio::time::timeout(Duration::from_secs(TIMEOUT_SECS), task).await {
        Ok(join) => join.map_err(|e| AgentError::Database(format!("task: {e}")))?,
        Err(_) => Err(AgentError::Database(format!(
            "DB query timed out after {TIMEOUT_SECS}s"
        ))),
    }
}

fn run_db_query_blocking(
    sql: &str,
    conn_str: &str,
    db_type: &str,
    ext: &str,
    schema: &str,
) -> Result<QueryResult> {
    let start = std::time::Instant::now();
    let conn = Connection::open_in_memory()
        .map_err(|e| AgentError::Database(format!("open: {e}")))?;

    conn.execute_batch(&format!("INSTALL {ext}; LOAD {ext};"))
        .map_err(|e| {
            AgentError::Database(format!(
                "Failed to load {ext} extension: {e}. \
                 Sery Link needs internet access on first use to download the extension."
            ))
        })?;

    conn.execute_batch(&format!(
        "ATTACH '{conn_str}' AS _db (TYPE {db_type}, READ_ONLY);"
    ))
    .map_err(|e| AgentError::Database(format!("ATTACH: {e}")))?;

    conn.execute_batch(&format!("USE _db; SET schema='{schema}';"))
        .map_err(|e| AgentError::Database(format!("USE: {e}")))?;

    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| AgentError::Database(format!("prepare: {e}")))?;

    let columns: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();
    let mut truncated = false;
    let mut raw = stmt
        .query([])
        .map_err(|e| AgentError::Database(format!("query: {e}")))?;

    while let Some(row) = raw
        .next()
        .map_err(|e| AgentError::Database(format!("row: {e}")))?
    {
        if rows.len() >= MAX_ROWS {
            truncated = true;
            break;
        }
        let vals: Vec<serde_json::Value> = (0..columns.len())
            .map(|i| row_to_json(row, i))
            .collect();
        rows.push(vals);
    }

    Ok(QueryResult {
        columns,
        row_count: rows.len(),
        rows,
        duration_ms: start.elapsed().as_millis() as u64,
        truncated,
    })
}

fn row_to_json(row: &duckdb::Row, idx: usize) -> serde_json::Value {
    if let Ok(Some(v)) = row.get::<_, Option<i64>>(idx) {
        return serde_json::json!(v);
    }
    if let Ok(Some(v)) = row.get::<_, Option<f64>>(idx) {
        return serde_json::json!(v);
    }
    if let Ok(Some(v)) = row.get::<_, Option<bool>>(idx) {
        return serde_json::json!(v);
    }
    if let Ok(Some(v)) = row.get::<_, Option<String>>(idx) {
        return serde_json::json!(v);
    }
    serde_json::Value::Null
}

/// Introspect the schema of a DB source: list tables + their columns.
///
/// Uses INFORMATION_SCHEMA.COLUMNS for both MySQL and PostgreSQL.
/// Called after adding a source to push schema to the cloud catalog.
pub async fn introspect_schema(
    source_id: &str,
    config: &Config,
) -> Result<Vec<TableSchema>> {
    let (kind, password) = resolve_source(source_id, config)?;
    let (db_type, conn_str) = build_attach_string(kind, &password)
        .ok_or_else(|| AgentError::Database("Source is not a database type".to_string()))?;
    let ext = extension_name(kind).unwrap();
    let schema = default_schema(kind);
    let db_name = db_name_from_kind(kind).to_string();

    let task = tokio::task::spawn_blocking(move || {
        introspect_blocking(&conn_str, db_type, ext, schema, &db_name)
    });

    match tokio::time::timeout(Duration::from_secs(30), task).await {
        Ok(join) => join.map_err(|e| AgentError::Database(format!("task: {e}")))?,
        Err(_) => Err(AgentError::Database("Schema introspection timed out".to_string())),
    }
}

fn db_name_from_kind(kind: &SourceKind) -> &str {
    match kind {
        SourceKind::Mysql { database, .. } => database.as_str(),
        SourceKind::Postgresql { database, .. } => database.as_str(),
        _ => "",
    }
}

fn introspect_blocking(
    conn_str: &str,
    db_type: &str,
    ext: &str,
    schema: &str,
    db_name: &str,
) -> Result<Vec<TableSchema>> {
    let conn = Connection::open_in_memory()
        .map_err(|e| AgentError::Database(format!("open: {e}")))?;

    conn.execute_batch(&format!("INSTALL {ext}; LOAD {ext};"))
        .map_err(|e| AgentError::Database(format!("load ext: {e}")))?;

    conn.execute_batch(&format!(
        "ATTACH '{conn_str}' AS _db (TYPE {db_type}, READ_ONLY);"
    ))
    .map_err(|e| AgentError::Database(format!("ATTACH: {e}")))?;

    conn.execute_batch(&format!("USE _db; SET schema='{schema}';"))
        .map_err(|e| AgentError::Database(format!("USE: {e}")))?;

    // INFORMATION_SCHEMA.COLUMNS is supported by both mysql_scanner and postgres_scanner.
    let col_sql = format!(
        "SELECT table_name, column_name, data_type, is_nullable \
         FROM information_schema.columns \
         WHERE table_schema = '{db_name}' \
         ORDER BY table_name, ordinal_position"
    );

    let mut stmt = conn
        .prepare(&col_sql)
        .map_err(|e| AgentError::Database(format!("prepare schema query: {e}")))?;

    let mut tables: std::collections::BTreeMap<String, Vec<ColumnInfo>> =
        std::collections::BTreeMap::new();
    let mut raw = stmt
        .query([])
        .map_err(|e| AgentError::Database(format!("schema query: {e}")))?;

    while let Some(row) = raw.next().map_err(|e| AgentError::Database(e.to_string()))? {
        let table: String = row.get(0).unwrap_or_default();
        let col_name: String = row.get(1).unwrap_or_default();
        let data_type: String = row.get(2).unwrap_or_default();
        let nullable: String = row.get(3).unwrap_or_else(|_| "YES".to_string());
        tables.entry(table).or_default().push(ColumnInfo {
            name: col_name,
            data_type,
            nullable: nullable.to_ascii_uppercase() == "YES",
        });
    }

    Ok(tables
        .into_iter()
        .map(|(table_name, columns)| TableSchema {
            table_name,
            columns,
            row_count_estimate: None,
        })
        .collect())
}

/// Test a DB connection using provided credentials (before persisting).
/// Runs blocking; call from a spawn_blocking context.
pub fn test_connection_blocking(
    kind: &SourceKind,
    password: &str,
) -> Result<()> {
    let (db_type, conn_str) = build_attach_string(kind, password)
        .ok_or_else(|| AgentError::Database("Not a database source type".to_string()))?;
    let ext = extension_name(kind).unwrap();
    let schema = default_schema(kind);

    let conn = Connection::open_in_memory()
        .map_err(|e| AgentError::Database(format!("open: {e}")))?;

    conn.execute_batch(&format!("INSTALL {ext}; LOAD {ext};"))
        .map_err(|e| AgentError::Database(format!("load ext: {e}")))?;

    conn.execute_batch(&format!(
        "ATTACH '{conn_str}' AS _db (TYPE {db_type}, READ_ONLY);"
    ))
    .map_err(|e| AgentError::Database(format!("connection failed: {e}")))?;

    conn.execute_batch(&format!("USE _db; SET schema='{schema}';"))
        .map_err(|e| AgentError::Database(format!("USE: {e}")))?;

    // Simple ping query
    conn.execute_batch("SELECT 1;")
        .map_err(|e| AgentError::Database(format!("ping: {e}")))?;

    Ok(())
}

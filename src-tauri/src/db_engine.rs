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
/// Returns (db_type_str, conn_str, extension_install_sql).
/// Only for DuckDB-ATTACH sources: MySQL, PostgreSQL, Snowflake, SQLite.
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
        SourceKind::Snowflake {
            account,
            username,
            warehouse,
            database,
            schema,
        } => {
            let conn_str = format!(
                "account={account};user={username};password={password};\
                 warehouse={warehouse};database={database};schema={schema}"
            );
            Some(("SNOWFLAKE", conn_str))
        }
        SourceKind::Sqlite { path } => {
            Some(("SQLITE", path.to_string_lossy().to_string()))
        }
        _ => None,
    }
}

/// Return the DuckDB extension install SQL for a SourceKind.
fn extension_install_sql(kind: &SourceKind) -> Option<&'static str> {
    match kind {
        SourceKind::Mysql { .. } => Some("INSTALL mysql; LOAD mysql;"),
        SourceKind::Postgresql { .. } => Some("INSTALL postgres; LOAD postgres;"),
        SourceKind::Snowflake { .. } => Some("INSTALL snowflake FROM community; LOAD snowflake;"),
        SourceKind::Sqlite { .. } => Some("INSTALL sqlite; LOAD sqlite;"),
        _ => None,
    }
}

/// Return the DuckDB extension name for a SourceKind (used for error messages).
fn extension_name(kind: &SourceKind) -> Option<&'static str> {
    match kind {
        SourceKind::Mysql { .. } => Some("mysql"),
        SourceKind::Postgresql { .. } => Some("postgres"),
        SourceKind::Snowflake { .. } => Some("snowflake"),
        SourceKind::Sqlite { .. } => Some("sqlite"),
        _ => None,
    }
}

/// Default schema name inside the attached DuckDB database alias.
/// MySQL / PostgreSQL / Snowflake use 'main'; SQLite uses 'main' too.
fn default_schema(_kind: &SourceKind) -> Option<&'static str> {
    match _kind {
        SourceKind::Sqlite { .. } => None, // SQLite: no SET schema needed
        _ => Some("main"),
    }
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

    // Dispatch to specialised engines for HTTP / document / key-value sources.
    match kind {
        SourceKind::Clickhouse { host, port, username, database } => {
            let (host, port, username, database) =
                (host.clone(), *port, username.clone(), database.clone());
            let sql_owned = sql.to_string();
            let pw = password.clone();
            let task = tokio::task::spawn_blocking(move || {
                execute_clickhouse_query_blocking(&sql_owned, &host, port, &username, &pw, &database)
            });
            return match tokio::time::timeout(Duration::from_secs(TIMEOUT_SECS), task).await {
                Ok(join) => join.map_err(|e| AgentError::Database(format!("task: {e}")))?,
                Err(_) => Err(AgentError::Database(format!("ClickHouse query timed out after {TIMEOUT_SECS}s"))),
            };
        }
        SourceKind::Mongodb { host, port, username, database, auth_db } => {
            let (host, port, username, database, auth_db) =
                (host.clone(), *port, username.clone(), database.clone(), auth_db.clone());
            let sql_owned = sql.to_string();
            let pw = password.clone();
            return execute_mongodb_query(&sql_owned, &host, port, &username, &pw, &database, &auth_db).await;
        }
        SourceKind::Redis { host, port, db } => {
            let (host, port, db) = (host.clone(), *port, *db);
            let sql_owned = sql.to_string();
            let pw = password.clone();
            return execute_redis_query(&sql_owned, &host, port, db, &pw).await;
        }
        _ => {}
    }

    // DuckDB ATTACH path: MySQL, PostgreSQL, Snowflake, SQLite.
    let (db_type, conn_str) = build_attach_string(kind, &password)
        .ok_or_else(|| AgentError::Database("Source is not a database type".to_string()))?;
    let ext_sql = extension_install_sql(kind)
        .ok_or_else(|| AgentError::Database("Unknown DB extension".to_string()))?
        .to_string();
    let ext_name = extension_name(kind).unwrap_or("unknown").to_string();
    let schema = default_schema(kind).map(|s| s.to_string());
    let sql_owned = sql.to_string();

    let task = tokio::task::spawn_blocking(move || {
        run_db_query_blocking(&sql_owned, &conn_str, db_type, &ext_sql, &ext_name, schema.as_deref())
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
    ext_install_sql: &str,
    ext_name: &str,
    schema: Option<&str>,
) -> Result<QueryResult> {
    let start = std::time::Instant::now();
    let conn = Connection::open_in_memory()
        .map_err(|e| AgentError::Database(format!("open: {e}")))?;

    conn.execute_batch(ext_install_sql)
        .map_err(|e| {
            AgentError::Database(format!(
                "Failed to load {ext_name} extension: {e}. \
                 Sery Link needs internet access on first use to download the extension."
            ))
        })?;

    conn.execute_batch(&format!(
        "ATTACH '{conn_str}' AS _db (TYPE {db_type}, READ_ONLY);"
    ))
    .map_err(|e| AgentError::Database(format!("ATTACH: {e}")))?;

    conn.execute_batch("USE _db;")
        .map_err(|e| AgentError::Database(format!("USE: {e}")))?;

    if let Some(s) = schema {
        conn.execute_batch(&format!("SET schema='{s}';"))
            .map_err(|e| AgentError::Database(format!("SET schema: {e}")))?;
    }

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
/// Uses INFORMATION_SCHEMA.COLUMNS for MySQL / PostgreSQL / Snowflake,
/// sqlite_master for SQLite, system.columns for ClickHouse,
/// collection listing for MongoDB, and a virtual schema for Redis.
pub async fn introspect_schema(
    source_id: &str,
    config: &Config,
) -> Result<Vec<TableSchema>> {
    let (kind, password) = resolve_source(source_id, config)?;

    match kind {
        SourceKind::Clickhouse { host, port, username, database } => {
            let (host, port, username, database) =
                (host.clone(), *port, username.clone(), database.clone());
            let pw = password.clone();
            let task = tokio::task::spawn_blocking(move || {
                introspect_clickhouse_blocking(&host, port, &username, &pw, &database)
            });
            return match tokio::time::timeout(Duration::from_secs(30), task).await {
                Ok(join) => join.map_err(|e| AgentError::Database(format!("task: {e}")))?,
                Err(_) => Err(AgentError::Database("ClickHouse introspect timed out".to_string())),
            };
        }
        SourceKind::Mongodb { host, port, username, database, auth_db } => {
            let (host, port, username, database, auth_db) =
                (host.clone(), *port, username.clone(), database.clone(), auth_db.clone());
            let pw = password.clone();
            return introspect_mongodb(&host, port, &username, &pw, &database, &auth_db).await;
        }
        SourceKind::Redis { .. } => {
            // Redis: single virtual table
            return Ok(vec![TableSchema {
                table_name: "keys".to_string(),
                columns: vec![
                    ColumnInfo { name: "key".to_string(), data_type: "TEXT".to_string(), nullable: false },
                    ColumnInfo { name: "value".to_string(), data_type: "TEXT".to_string(), nullable: true },
                    ColumnInfo { name: "value_type".to_string(), data_type: "TEXT".to_string(), nullable: false },
                    ColumnInfo { name: "ttl".to_string(), data_type: "INTEGER".to_string(), nullable: false },
                ],
                row_count_estimate: None,
            }]);
        }
        _ => {}
    }

    let (db_type, conn_str) = build_attach_string(kind, &password)
        .ok_or_else(|| AgentError::Database("Source is not a database type".to_string()))?;
    let ext_sql = extension_install_sql(kind)
        .ok_or_else(|| AgentError::Database("Unknown extension".to_string()))?
        .to_string();
    let schema = default_schema(kind).map(|s| s.to_string());
    let db_name = db_name_from_kind(kind).to_string();
    let use_sqlite = matches!(kind, SourceKind::Sqlite { .. });

    let task = tokio::task::spawn_blocking(move || {
        introspect_blocking(&conn_str, db_type, &ext_sql, schema.as_deref(), &db_name, use_sqlite)
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
        SourceKind::Snowflake { database, .. } => database.as_str(),
        SourceKind::Clickhouse { database, .. } => database.as_str(),
        SourceKind::Mongodb { database, .. } => database.as_str(),
        _ => "",
    }
}

fn introspect_blocking(
    conn_str: &str,
    db_type: &str,
    ext_install_sql: &str,
    _schema: Option<&str>,
    _db_name: &str,
    _use_sqlite: bool,
) -> Result<Vec<TableSchema>> {
    let conn = Connection::open_in_memory()
        .map_err(|e| AgentError::Database(format!("open: {e}")))?;

    conn.execute_batch(ext_install_sql)
        .map_err(|e| AgentError::Database(format!("load ext: {e}")))?;

    conn.execute_batch(&format!(
        "ATTACH '{conn_str}' AS _db (TYPE {db_type}, READ_ONLY);"
    ))
    .map_err(|e| AgentError::Database(format!("ATTACH: {e}")))?;

    // duckdb_columns() is DuckDB's own catalog function — works for every
    // attached database type without relying on information_schema visibility
    // after USE. Filter by database_name='_db' to only see the attached DB,
    // exclude well-known system schemas, and skip internal columns.
    let col_sql =
        "SELECT table_name, column_name, data_type, \
         CASE WHEN is_nullable THEN 'YES' ELSE 'NO' END \
         FROM duckdb_columns() \
         WHERE database_name = '_db' \
           AND schema_name NOT IN ( \
               'information_schema', 'pg_catalog', 'pg_toast', \
               'pg_internal', 'INFORMATION_SCHEMA', 'pg_toast_temp_1' \
           ) \
           AND NOT internal \
         ORDER BY table_name, column_index";

    let mut stmt = conn
        .prepare(col_sql)
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
/// Handles all 7 DB source kinds. For Clickhouse/MongoDB/Redis, uses
/// their respective blocking test helpers.
pub fn test_connection_blocking(
    kind: &SourceKind,
    password: &str,
) -> Result<()> {
    match kind {
        SourceKind::Clickhouse { host, port, username, database } => {
            return test_clickhouse_connection_blocking(host, *port, username, password, database);
        }
        SourceKind::Mongodb { host, port, username, auth_db, .. } => {
            // MongoDB is async; block on it here via a mini tokio runtime.
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| AgentError::Database(format!("runtime: {e}")))?;
            return rt.block_on(test_mongodb_connection(host, *port, username, password, auth_db));
        }
        SourceKind::Redis { host, port, db } => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| AgentError::Database(format!("runtime: {e}")))?;
            return rt.block_on(test_redis_connection(host, *port, *db, password));
        }
        _ => {}
    }

    let (db_type, conn_str) = build_attach_string(kind, password)
        .ok_or_else(|| AgentError::Database("Not a database source type".to_string()))?;
    let ext_install_sql = extension_install_sql(kind)
        .ok_or_else(|| AgentError::Database("Unknown extension".to_string()))?;
    let ext_name = extension_name(kind).unwrap_or("unknown");
    let schema = default_schema(kind);

    let conn = Connection::open_in_memory()
        .map_err(|e| AgentError::Database(format!("open: {e}")))?;

    conn.execute_batch(ext_install_sql)
        .map_err(|e| AgentError::Database(format!("load {ext_name} ext: {e}")))?;

    conn.execute_batch(&format!(
        "ATTACH '{conn_str}' AS _db (TYPE {db_type}, READ_ONLY);"
    ))
    .map_err(|e| AgentError::Database(format!("connection failed: {e}")))?;

    conn.execute_batch("USE _db;")
        .map_err(|e| AgentError::Database(format!("USE: {e}")))?;

    if let Some(s) = schema {
        conn.execute_batch(&format!("SET schema='{s}';"))
            .map_err(|e| AgentError::Database(format!("SET schema: {e}")))?;
    }

    conn.execute_batch("SELECT 1;")
        .map_err(|e| AgentError::Database(format!("ping: {e}")))?;

    Ok(())
}

// ─── ClickHouse HTTP engine ──────────────────────────────────────────────────

fn execute_clickhouse_query_blocking(
    sql: &str,
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    database: &str,
) -> Result<QueryResult> {
    let start = std::time::Instant::now();
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .build()
        .map_err(|e| AgentError::Database(format!("build client: {e}")))?;

    let url = format!("http://{}:{}/", host, port);
    let resp = client
        .post(&url)
        .query(&[
            ("query", sql),
            ("user", username),
            ("password", password),
            ("database", database),
            ("default_format", "JSONCompact"),
        ])
        .send()
        .map_err(|e| AgentError::Database(format!("ClickHouse HTTP: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(AgentError::Database(format!("ClickHouse error {status}: {body}")));
    }

    let body: serde_json::Value = resp
        .json()
        .map_err(|e| AgentError::Database(format!("parse JSON: {e}")))?;

    let columns: Vec<String> = body["meta"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|m| m["name"].as_str().map(|s| s.to_string()))
        .collect();

    let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();
    let mut truncated = false;

    if let Some(data) = body["data"].as_array() {
        for row_val in data {
            if rows.len() >= MAX_ROWS {
                truncated = true;
                break;
            }
            if let Some(cells) = row_val.as_array() {
                rows.push(cells.clone());
            }
        }
    }

    Ok(QueryResult {
        columns,
        row_count: rows.len(),
        rows,
        duration_ms: start.elapsed().as_millis() as u64,
        truncated,
    })
}

fn introspect_clickhouse_blocking(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    database: &str,
) -> Result<Vec<TableSchema>> {
    let sql = format!(
        "SELECT table_name, name, type, toString(is_in_primary_key) AS is_nullable \
         FROM system.columns \
         WHERE database = '{}' \
         ORDER BY table_name, position",
        database
    );
    let result = execute_clickhouse_query_blocking(&sql, host, port, username, password, database)?;

    let mut tables: std::collections::BTreeMap<String, Vec<ColumnInfo>> =
        std::collections::BTreeMap::new();
    for row in &result.rows {
        if row.len() < 4 { continue; }
        let table = row[0].as_str().unwrap_or("").to_string();
        let col_name = row[1].as_str().unwrap_or("").to_string();
        let data_type = row[2].as_str().unwrap_or("").to_string();
        let is_pk = row[3].as_str().unwrap_or("0");
        tables.entry(table).or_default().push(ColumnInfo {
            name: col_name,
            data_type,
            nullable: is_pk == "0",
        });
    }
    Ok(tables.into_iter().map(|(table_name, columns)| TableSchema {
        table_name, columns, row_count_estimate: None,
    }).collect())
}

fn test_clickhouse_connection_blocking(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    database: &str,
) -> Result<()> {
    execute_clickhouse_query_blocking("SELECT 1", host, port, username, password, database)?;
    Ok(())
}

// ─── MongoDB engine ──────────────────────────────────────────────────────────

fn extract_collection_names(sql: &str) -> Vec<String> {
    let re = regex::Regex::new(r"(?i)\b(?:FROM|JOIN)\s+([a-zA-Z_][a-zA-Z0-9_]*)").unwrap();
    re.captures_iter(sql)
        .filter_map(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect()
}

async fn execute_mongodb_query(
    sql: &str,
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    database: &str,
    auth_db: &str,
) -> Result<QueryResult> {
    use mongodb::{Client, options::ClientOptions, bson::Document};
    use futures::TryStreamExt;

    let start = std::time::Instant::now();
    let uri = if password.is_empty() {
        format!("mongodb://{}:{}/{}", host, port, auth_db)
    } else {
        format!("mongodb://{}:{}@{}:{}/{}", username, password, host, port, auth_db)
    };

    let opts = ClientOptions::parse(&uri)
        .await
        .map_err(|e| AgentError::Database(format!("MongoDB URI: {e}")))?;
    let client = Client::with_options(opts)
        .map_err(|e| AgentError::Database(format!("MongoDB client: {e}")))?;

    let db = client.database(database);
    let collection_names = extract_collection_names(sql);

    let conn = Connection::open_in_memory()
        .map_err(|e| AgentError::Database(format!("DuckDB open: {e}")))?;

    for coll_name in &collection_names {
        let collection = db.collection::<Document>(coll_name);
        let mut cursor = collection
            .find(mongodb::bson::doc! {})
            .limit(MAX_ROWS as i64)
            .await
            .map_err(|e| AgentError::Database(format!("MongoDB find {coll_name}: {e}")))?;

        let mut docs: Vec<serde_json::Value> = Vec::new();
        while let Some(doc) = cursor.try_next().await
            .map_err(|e| AgentError::Database(format!("cursor: {e}")))? {
            if let Ok(json_val) = serde_json::to_value(&doc) {
                docs.push(json_val);
            }
        }

        let json_bytes = serde_json::to_vec(&docs)
            .map_err(|e| AgentError::Database(format!("json: {e}")))?;

        let tmp_path = std::env::temp_dir().join(format!("sery_mongo_{}.json", coll_name));
        std::fs::write(&tmp_path, &json_bytes)
            .map_err(|e| AgentError::Database(format!("write tmp: {e}")))?;

        let path_str = tmp_path.to_string_lossy().to_string();
        // Escape single quotes in path
        let escaped = path_str.replace('\'', "''");
        conn.execute_batch(&format!(
            "CREATE TEMP TABLE \"{coll_name}\" AS SELECT * FROM read_json_auto('{escaped}');"
        ))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            AgentError::Database(format!("load {coll_name}: {e}"))
        })?;
        let _ = std::fs::remove_file(&tmp_path);
    }

    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| AgentError::Database(format!("prepare: {e}")))?;

    let columns: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();
    let mut truncated = false;
    let mut raw = stmt.query([])
        .map_err(|e| AgentError::Database(format!("query: {e}")))?;

    while let Some(row) = raw.next()
        .map_err(|e| AgentError::Database(format!("row: {e}")))? {
        if rows.len() >= MAX_ROWS {
            truncated = true;
            break;
        }
        rows.push((0..columns.len()).map(|i| row_to_json(row, i)).collect());
    }

    Ok(QueryResult {
        columns,
        row_count: rows.len(),
        rows,
        duration_ms: start.elapsed().as_millis() as u64,
        truncated,
    })
}

async fn introspect_mongodb(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    database: &str,
    auth_db: &str,
) -> Result<Vec<TableSchema>> {
    use mongodb::{Client, options::ClientOptions, bson::Document};
    use futures::TryStreamExt;

    let uri = if password.is_empty() {
        format!("mongodb://{}:{}/{}", host, port, auth_db)
    } else {
        format!("mongodb://{}:{}@{}:{}/{}", username, password, host, port, auth_db)
    };

    let opts = ClientOptions::parse(&uri)
        .await
        .map_err(|e| AgentError::Database(format!("MongoDB URI: {e}")))?;
    let client = Client::with_options(opts)
        .map_err(|e| AgentError::Database(format!("MongoDB client: {e}")))?;

    let db = client.database(database);
    let coll_names = db
        .list_collection_names()
        .await
        .map_err(|e| AgentError::Database(format!("list collections: {e}")))?;

    let mut tables: Vec<TableSchema> = Vec::new();
    for coll_name in coll_names {
        let collection = db.collection::<Document>(&coll_name);
        let mut cursor = collection
            .find(mongodb::bson::doc! {})
            .limit(50)
            .await
            .map_err(|e| AgentError::Database(format!("sample {coll_name}: {e}")))?;

        let mut field_map: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        while let Some(doc) = cursor.try_next().await
            .map_err(|e| AgentError::Database(format!("cursor: {e}")))? {
            for (k, v) in doc.iter() {
                field_map.entry(k.clone()).or_insert_with(|| bson_type_str(v).to_string());
            }
        }

        let columns: Vec<ColumnInfo> = field_map
            .into_iter()
            .map(|(name, data_type)| ColumnInfo { name, data_type, nullable: true })
            .collect();

        tables.push(TableSchema { table_name: coll_name, columns, row_count_estimate: None });
    }
    Ok(tables)
}

fn bson_type_str(v: &mongodb::bson::Bson) -> &'static str {
    use mongodb::bson::Bson;
    match v {
        Bson::String(_) => "String",
        Bson::Int32(_) => "Int32",
        Bson::Int64(_) => "Int64",
        Bson::Double(_) => "Double",
        Bson::Boolean(_) => "Boolean",
        Bson::DateTime(_) => "DateTime",
        Bson::Array(_) => "Array",
        Bson::Document(_) => "Document",
        Bson::ObjectId(_) => "ObjectId",
        Bson::Null => "Null",
        _ => "Mixed",
    }
}

async fn test_mongodb_connection(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    auth_db: &str,
) -> Result<()> {
    use mongodb::{Client, options::ClientOptions};

    let uri = if password.is_empty() {
        format!("mongodb://{}:{}/{}", host, port, auth_db)
    } else {
        format!("mongodb://{}:{}@{}:{}/{}", username, password, host, port, auth_db)
    };

    let opts = ClientOptions::parse(&uri)
        .await
        .map_err(|e| AgentError::Database(format!("MongoDB URI: {e}")))?;
    let client = Client::with_options(opts)
        .map_err(|e| AgentError::Database(format!("MongoDB client: {e}")))?;
    client
        .database(auth_db)
        .run_command(mongodb::bson::doc! {"ping": 1})
        .await
        .map_err(|e| AgentError::Database(format!("MongoDB ping: {e}")))?;
    Ok(())
}

// ─── Redis engine ────────────────────────────────────────────────────────────

async fn execute_redis_query(
    sql: &str,
    host: &str,
    port: u16,
    db: u8,
    password: &str,
) -> Result<QueryResult> {

    let start = std::time::Instant::now();
    let url = if password.is_empty() {
        format!("redis://{}:{}/{}", host, port, db)
    } else {
        format!("redis://:{}@{}:{}/{}", password, host, port, db)
    };

    let client = redis::Client::open(url.as_str())
        .map_err(|e| AgentError::Database(format!("Redis client: {e}")))?;
    let mut con = client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| AgentError::Database(format!("Redis connect: {e}")))?;

    // SCAN all keys
    let mut keys: Vec<String> = Vec::new();
    let mut cursor: u64 = 0;
    loop {
        let (next_cursor, batch): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("COUNT")
            .arg(200u64)
            .query_async(&mut con)
            .await
            .map_err(|e| AgentError::Database(format!("SCAN: {e}")))?;
        keys.extend(batch);
        cursor = next_cursor;
        if cursor == 0 || keys.len() >= MAX_ROWS {
            break;
        }
    }
    keys.truncate(MAX_ROWS);

    // Build rows as (key, value, value_type, ttl)
    let mut key_rows: Vec<(String, String, String, i64)> = Vec::new();
    for key in &keys {
        let key_type: String = redis::cmd("TYPE")
            .arg(key)
            .query_async(&mut con)
            .await
            .unwrap_or_else(|_| "unknown".to_string());

        let value: String = match key_type.as_str() {
            "string" => redis::cmd("GET")
                .arg(key)
                .query_async(&mut con)
                .await
                .unwrap_or_default(),
            "list" => {
                let lst: Vec<String> = redis::cmd("LRANGE")
                    .arg(key).arg(0i64).arg(9i64)
                    .query_async(&mut con)
                    .await
                    .unwrap_or_default();
                format!("[{}]", lst.join(", "))
            }
            "hash" => {
                let hm: Vec<(String, String)> = redis::cmd("HGETALL")
                    .arg(key)
                    .query_async(&mut con)
                    .await
                    .unwrap_or_default();
                let pairs: Vec<String> = hm.into_iter().map(|(k, v)| format!("{k}={v}")).collect();
                format!("{{{}}}", pairs.join(", "))
            }
            "set" => {
                let members: Vec<String> = redis::cmd("SMEMBERS")
                    .arg(key)
                    .query_async(&mut con)
                    .await
                    .unwrap_or_default();
                format!("{{{}}}", members.join(", "))
            }
            "zset" => {
                let members: Vec<String> = redis::cmd("ZRANGE")
                    .arg(key).arg(0i64).arg(9i64)
                    .query_async(&mut con)
                    .await
                    .unwrap_or_default();
                format!("[{}]", members.join(", "))
            }
            _ => String::new(),
        };

        let ttl: i64 = redis::cmd("TTL")
            .arg(key)
            .query_async(&mut con)
            .await
            .unwrap_or(-1);

        key_rows.push((key.clone(), value, key_type, ttl));
    }

    // Register as DuckDB temp table and execute SQL
    let duckdb_conn = Connection::open_in_memory()
        .map_err(|e| AgentError::Database(format!("DuckDB open: {e}")))?;

    // Build inline values for DuckDB
    if key_rows.is_empty() {
        duckdb_conn.execute_batch(
            "CREATE TEMP TABLE keys (key TEXT, value TEXT, value_type TEXT, ttl INTEGER);"
        ).map_err(|e| AgentError::Database(format!("create empty keys: {e}")))?;
    } else {
        let rows_json: Vec<serde_json::Value> = key_rows.iter().map(|(k, v, t, ttl)| {
            serde_json::json!({ "key": k, "value": v, "value_type": t, "ttl": ttl })
        }).collect();
        let json_array = serde_json::to_vec(&rows_json)
            .map_err(|e| AgentError::Database(format!("json: {e}")))?;
        let tmp_path = std::env::temp_dir().join("sery_redis_keys.json");
        std::fs::write(&tmp_path, &json_array)
            .map_err(|e| AgentError::Database(format!("write tmp: {e}")))?;
        let path_str = tmp_path.to_string_lossy().to_string().replace('\'', "''");
        duckdb_conn.execute_batch(&format!(
            "CREATE TEMP TABLE keys AS SELECT * FROM read_json_auto('{path_str}');"
        )).map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            AgentError::Database(format!("load keys: {e}"))
        })?;
        let _ = std::fs::remove_file(&tmp_path);
    }

    let mut stmt = duckdb_conn
        .prepare(sql)
        .map_err(|e| AgentError::Database(format!("prepare: {e}")))?;

    let columns: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();
    let mut truncated = false;
    let mut raw = stmt.query([])
        .map_err(|e| AgentError::Database(format!("query: {e}")))?;

    while let Some(row) = raw.next()
        .map_err(|e| AgentError::Database(format!("row: {e}")))? {
        if rows.len() >= MAX_ROWS {
            truncated = true;
            break;
        }
        rows.push((0..columns.len()).map(|i| row_to_json(row, i)).collect());
    }

    Ok(QueryResult {
        columns,
        row_count: rows.len(),
        rows,
        duration_ms: start.elapsed().as_millis() as u64,
        truncated,
    })
}

async fn test_redis_connection(
    host: &str,
    port: u16,
    db: u8,
    password: &str,
) -> Result<()> {
    let url = if password.is_empty() {
        format!("redis://{}:{}/{}", host, port, db)
    } else {
        format!("redis://:{}@{}:{}/{}", password, host, port, db)
    };
    let client = redis::Client::open(url.as_str())
        .map_err(|e| AgentError::Database(format!("Redis client: {e}")))?;
    let mut con = client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| AgentError::Database(format!("Redis connect: {e}")))?;
    let _: String = redis::cmd("PING")
        .query_async(&mut con)
        .await
        .map_err(|e| AgentError::Database(format!("Redis PING: {e}")))?;
    Ok(())
}

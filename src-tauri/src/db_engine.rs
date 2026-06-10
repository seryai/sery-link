//! F52 — Database query execution and schema introspection.
//!
//! MySQL and PostgreSQL use native connection pools (db-core crate).
//! Snowflake and SQLite keep DuckDB ATTACH.
//! ClickHouse uses HTTP. MongoDB and Redis use their dedicated clients.
//!
//! Security model:
//!   - SELECT only. DDL/DML keywords are rejected before the query runs.
//!   - Credentials are loaded from the OS keychain, never from the query string.
//!   - 100 000 row cap and 60 s timeout match the file-based engine.

use crate::config::Config;
use crate::db_creds::{DbConnectionConfig, load_connection};
use crate::duckdb_engine::QueryResult;
use crate::error::{AgentError, Result};
use crate::sources::SourceKind;
use db_core::agent_runtime::{self, spawn_client_for_driver};
use db_core::db::agent_driver::{AgentDriverClient, AgentMethod};
use duckdb::Connection;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

// ─── Agent-based DB types (Oracle, DB2, SAP HANA, Teradata, Vertica, …) ────
//
// These database types require a Java JDBC agent that must be installed
// via Settings → Driver Store before a connection can be opened. Connect
// + execute go through `connect_agent_source` / `execute_agent_query`,
// which spawn the Java agent (one process per `driver_key`) and cache
// the connected client per `source_id` so multiple sources of the same
// kind can coexist without trampling each other's connect state.

/// Per-source agent client cache.
///
/// Key = source_id (NOT driver_key). Each entry represents an agent
/// process that has already received `connect` for that specific source.
/// Sharing one process per driver_key would force every source to share
/// the same JDBC connection state — incorrect when two Oracle sources
/// point at different databases. Keying on source_id keeps them
/// isolated. The process itself is still spawned via
/// `spawn_client_for_driver(driver_key)` so the JAR lookup uses the
/// installed driver's path under `~/.seryai/drivers/<driver_key>/`.
static AGENT_DB_CLIENTS: Lazy<tokio::sync::Mutex<HashMap<String, AgentDriverClient>>> =
    Lazy::new(|| tokio::sync::Mutex::new(HashMap::new()));

/// Build the JDBC connection_string for drivers whose URL shape depends
/// on metadata only the desktop knows (e.g. Oracle service vs SID, SAP
/// HANA databaseName). Returns the explicit `connection_string` arg if
/// the caller already supplied one, otherwise synthesises it. An empty
/// string means "agent: build it yourself from host/port/database".
fn build_agent_connection_string(
    driver_key: &str,
    host: &str,
    port: u16,
    database: Option<&str>,
    oracle_connection_type: Option<&str>,
    connection_string: Option<&str>,
) -> String {
    if let Some(s) = connection_string.filter(|s| !s.is_empty()) {
        return s.to_string();
    }

    if driver_key.starts_with("oracle") {
        let svc = database.unwrap_or("");
        // service_name (default, modern Oracle) vs sid (older 10g).
        let is_sid = oracle_connection_type
            .map(|t| t.eq_ignore_ascii_case("sid"))
            .unwrap_or(false);
        return if is_sid {
            format!("jdbc:oracle:thin:@{host}:{port}:{svc}")
        } else {
            format!("jdbc:oracle:thin:@//{host}:{port}/{svc}")
        };
    }

    if driver_key == "saphana" {
        let db = database.unwrap_or("");
        return if db.is_empty() {
            format!("jdbc:sap://{host}:{port}/")
        } else {
            format!("jdbc:sap://{host}:{port}/?databaseName={db}")
        };
    }

    // For most drivers the agent itself knows how to assemble the URL
    // from host/port/database, so leave it blank.
    String::new()
}

/// Send `connect` to the agent for `source_id`. Spawns the agent
/// process if one isn't running yet for this source, then caches the
/// (post-connect) client under the source_id key. Idempotent — calling
/// twice on the same source_id is a no-op once the first call succeeds.
pub async fn connect_agent_source(
    source_id: &str,
    driver_key: &str,
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    database: Option<&str>,
    oracle_connection_type: Option<&str>,
    connection_string: Option<&str>,
    url_params: Option<&str>,
    _config: &Config,
) -> std::result::Result<(), String> {
    let mut clients = AGENT_DB_CLIENTS.lock().await;
    if clients.contains_key(source_id) {
        return Ok(());
    }

    let mgr = crate::driver_manager::am();
    let mut client = spawn_client_for_driver(&mgr, driver_key).await?;

    let conn_str = build_agent_connection_string(
        driver_key,
        host,
        port,
        database,
        oracle_connection_type,
        connection_string,
    );

    let params = serde_json::json!({
        "host": host,
        "port": port,
        "database": database.unwrap_or(""),
        "username": username,
        "password": password,
        "url_params": url_params.unwrap_or(""),
        "connection_string": conn_str,
    });

    client
        .call_method::<serde_json::Value>(AgentMethod::Connect, params)
        .await?;

    clients.insert(source_id.to_string(), client);
    Ok(())
}

/// Execute `sql` against the agent client for `source_id`. If the
/// client has been evicted (or never opened) we re-load the source
/// metadata + keychain password and reconnect transparently before
/// running the query. This mirrors how the native MySQL / PG pools
/// lazy-reconnect on cache miss.
pub async fn execute_agent_query(
    sql: &str,
    source_id: &str,
    config: &Config,
) -> Result<QueryResult> {
    // Reconnect transparently if the client was never opened (or was
    // dropped due to e.g. a driver reinstall).
    let needs_connect = {
        let clients = AGENT_DB_CLIENTS.lock().await;
        !clients.contains_key(source_id)
    };
    if needs_connect {
        let source = config
            .sources
            .iter()
            .find(|s| s.id == source_id)
            .ok_or_else(|| AgentError::Database(format!(
                "Agent DB source not found: {source_id}"
            )))?;
        let (driver_key, host, port, username, database, oracle_ct, conn_str, url_params) =
            match &source.kind {
                SourceKind::AgentDb {
                    driver_key,
                    host,
                    port,
                    username,
                    database,
                    oracle_connection_type,
                    connection_string,
                    url_params,
                    ..
                } => (
                    driver_key.clone(),
                    host.clone(),
                    *port,
                    username.clone(),
                    database.clone(),
                    oracle_connection_type.clone(),
                    connection_string.clone(),
                    url_params.clone(),
                ),
                _ => return Err(AgentError::Database(format!(
                    "Source {source_id} is not an AgentDb source"
                ))),
            };
        let password = crate::agent_db_creds::load_password(source_id)
            .map_err(AgentError::Database)?;
        connect_agent_source(
            source_id,
            &driver_key,
            &host,
            port,
            &username,
            &password,
            database.as_deref(),
            oracle_ct.as_deref(),
            conn_str.as_deref(),
            url_params.as_deref(),
            config,
        )
        .await
        .map_err(AgentError::Database)?;
    }

    let start = std::time::Instant::now();
    let params = serde_json::json!({ "sql": sql, "params": [] });

    let mut clients = AGENT_DB_CLIENTS.lock().await;
    let client = clients.get_mut(source_id).ok_or_else(|| {
        AgentError::Database(format!("Agent client missing for {source_id}"))
    })?;

    let result: serde_json::Value = client
        .call_method::<serde_json::Value>(AgentMethod::ExecuteQuery, params)
        .await
        .map_err(AgentError::Database)?;

    // Agent returns { "columns": [String], "rows": [[Value, …], …] }
    let columns: Vec<String> = result
        .get("columns")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let rows: Vec<Vec<serde_json::Value>> = result
        .get("rows")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|r| r.as_array().cloned())
                .collect()
        })
        .unwrap_or_default();

    let row_count = rows.len();
    let truncated = row_count >= MAX_ROWS;

    Ok(QueryResult {
        columns,
        row_count,
        rows,
        duration_ms: start.elapsed().as_millis() as u64,
        truncated,
    })
}

/// Introspect an Agent DB source's schema: list tables, then fetch
/// per-table columns + indexes via the agent. Sequential RPC because
/// the agent holds one connection per source; parallel calls would
/// race the JDBC connection state.
pub async fn introspect_agent_schema(
    source_id: &str,
    config: &Config,
) -> Result<Vec<TableSchema>> {
    let source = config
        .sources
        .iter()
        .find(|s| s.id == source_id)
        .ok_or_else(|| AgentError::Database(format!(
            "Agent DB source not found: {source_id}"
        )))?;
    let (driver_key, host, port, username, database, oracle_ct, conn_str, url_params) =
        match &source.kind {
            SourceKind::AgentDb {
                driver_key,
                host,
                port,
                username,
                database,
                oracle_connection_type,
                connection_string,
                url_params,
                ..
            } => (
                driver_key.clone(),
                host.clone(),
                *port,
                username.clone(),
                database.clone(),
                oracle_connection_type.clone(),
                connection_string.clone(),
                url_params.clone(),
            ),
            _ => return Err(AgentError::Database(format!(
                "Source {source_id} is not an AgentDb source"
            ))),
        };

    // Ensure we have a connected client.
    let password = crate::agent_db_creds::load_password(source_id)
        .map_err(AgentError::Database)?;
    connect_agent_source(
        source_id,
        &driver_key,
        &host,
        port,
        &username,
        &password,
        database.as_deref(),
        oracle_ct.as_deref(),
        conn_str.as_deref(),
        url_params.as_deref(),
        config,
    )
    .await
    .map_err(AgentError::Database)?;

    // list_tables: { "database": "...", "schema": "..." }
    // For most JDBC drivers the schema is the username (Oracle) or
    // empty. We pass database + the username as schema and let the
    // agent fall back if either is empty.
    let db_arg = database.clone().unwrap_or_default();
    let schema_arg = username.clone();
    let tables_params = serde_json::json!({
        "database": db_arg,
        "schema": schema_arg,
    });

    let tables_resp: serde_json::Value = {
        let mut clients = AGENT_DB_CLIENTS.lock().await;
        let client = clients.get_mut(source_id).ok_or_else(|| {
            AgentError::Database(format!("Agent client missing for {source_id}"))
        })?;
        client
            .call_method::<serde_json::Value>(AgentMethod::ListTables, tables_params)
            .await
            .map_err(AgentError::Database)?
    };

    let table_names: Vec<String> = match &tables_resp {
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| {
                if let Some(s) = v.as_str() {
                    Some(s.to_string())
                } else if let Some(obj) = v.as_object() {
                    obj.get("name")
                        .or_else(|| obj.get("table_name"))
                        .or_else(|| obj.get("tableName"))
                        .and_then(|n| n.as_str().map(|s| s.to_string()))
                } else {
                    None
                }
            })
            .collect(),
        serde_json::Value::Object(obj) => obj
            .get("tables")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        if let Some(s) = v.as_str() {
                            Some(s.to_string())
                        } else {
                            v.as_object()
                                .and_then(|o| {
                                    o.get("name")
                                        .or_else(|| o.get("table_name"))
                                        .or_else(|| o.get("tableName"))
                                })
                                .and_then(|n| n.as_str().map(|s| s.to_string()))
                        }
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => vec![],
    };

    let mut tables: Vec<TableSchema> = Vec::with_capacity(table_names.len());
    for table_name in table_names {
        // get_columns
        let cols_params = serde_json::json!({
            "database": db_arg,
            "schema": schema_arg,
            "table": table_name,
        });
        let cols_resp: serde_json::Value = {
            let mut clients = AGENT_DB_CLIENTS.lock().await;
            let client = match clients.get_mut(source_id) {
                Some(c) => c,
                None => continue,
            };
            match client
                .call_method::<serde_json::Value>(AgentMethod::GetColumns, cols_params)
                .await
            {
                Ok(v) => v,
                Err(_) => serde_json::Value::Null,
            }
        };
        let columns = parse_agent_columns(&cols_resp);

        // list_indexes (best-effort: many drivers don't implement it)
        let idx_params = serde_json::json!({
            "database": db_arg,
            "schema": schema_arg,
            "table": table_name,
        });
        let idx_resp: serde_json::Value = {
            let mut clients = AGENT_DB_CLIENTS.lock().await;
            let client = match clients.get_mut(source_id) {
                Some(c) => c,
                None => continue,
            };
            match client
                .call_method::<serde_json::Value>(AgentMethod::ListIndexes, idx_params)
                .await
            {
                Ok(v) => v,
                Err(_) => serde_json::Value::Null,
            }
        };
        let indexes = parse_agent_indexes(&idx_resp);

        tables.push(TableSchema {
            table_name,
            columns,
            row_count_estimate: None,
            size_bytes: None,
            indexes,
            foreign_keys: vec![],
        });
    }

    Ok(tables)
}

fn parse_agent_columns(v: &serde_json::Value) -> Vec<ColumnInfo> {
    let arr: &Vec<serde_json::Value> = match v {
        serde_json::Value::Array(a) => a,
        serde_json::Value::Object(o) => match o.get("columns").and_then(|c| c.as_array()) {
            Some(a) => a,
            None => return vec![],
        },
        _ => return vec![],
    };
    arr.iter()
        .filter_map(|c| {
            let o = c.as_object()?;
            let name = o
                .get("name")
                .or_else(|| o.get("column_name"))
                .or_else(|| o.get("columnName"))
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            if name.is_empty() {
                return None;
            }
            let data_type = o
                .get("type")
                .or_else(|| o.get("data_type"))
                .or_else(|| o.get("dataType"))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            let nullable = o
                .get("nullable")
                .and_then(|n| n.as_bool())
                .unwrap_or(true);
            let is_primary_key = o
                .get("is_primary_key")
                .or_else(|| o.get("primaryKey"))
                .or_else(|| o.get("primary_key"))
                .and_then(|p| p.as_bool())
                .unwrap_or(false);
            let default_value = o
                .get("default")
                .or_else(|| o.get("default_value"))
                .or_else(|| o.get("defaultValue"))
                .and_then(|d| d.as_str())
                .map(|s| s.to_string());
            let comment = o
                .get("comment")
                .or_else(|| o.get("remarks"))
                .and_then(|d| d.as_str())
                .map(|s| s.to_string());
            Some(ColumnInfo {
                name,
                data_type,
                nullable,
                is_primary_key,
                default_value,
                comment,
            })
        })
        .collect()
}

fn parse_agent_indexes(v: &serde_json::Value) -> Vec<IndexInfo> {
    let arr: &Vec<serde_json::Value> = match v {
        serde_json::Value::Array(a) => a,
        serde_json::Value::Object(o) => match o.get("indexes").and_then(|c| c.as_array()) {
            Some(a) => a,
            None => return vec![],
        },
        _ => return vec![],
    };
    arr.iter()
        .filter_map(|i| {
            let o = i.as_object()?;
            let name = o
                .get("name")
                .or_else(|| o.get("index_name"))
                .or_else(|| o.get("indexName"))
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            if name.is_empty() {
                return None;
            }
            let columns: Vec<String> = o
                .get("columns")
                .and_then(|c| c.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let unique = o.get("unique").and_then(|u| u.as_bool()).unwrap_or(false);
            let primary = o.get("primary").and_then(|p| p.as_bool()).unwrap_or(false);
            Some(IndexInfo {
                name,
                columns,
                unique,
                primary,
            })
        })
        .collect()
}

/// Disconnect + drop the cached agent client for `source_id`. Called
/// when a source is removed. Best-effort — disconnect errors don't
/// fail the caller; we always evict the client.
#[allow(dead_code)]
pub async fn disconnect_agent_source(source_id: &str) {
    let mut clients = AGENT_DB_CLIENTS.lock().await;
    if let Some(mut client) = clients.remove(source_id) {
        let _ = client
            .call_method::<serde_json::Value>(AgentMethod::Disconnect, serde_json::json!({}))
            .await;
    }
}

/// Legacy entry point. Kept as a thin shim so older call sites that
/// referenced the v0.5 stub keep working — they now route through
/// `execute_agent_query`. New code should call `execute_agent_query`
/// directly.
#[allow(dead_code)]
pub async fn execute_via_agent(
    sql: &str,
    source_id: &str,
    config: &Config,
) -> Result<QueryResult> {
    execute_agent_query(sql, source_id, config).await
}

// Reference unused imports for crates we may need later. agent_runtime
// is re-exported for downstream callers that want call_daemon helpers
// without taking a separate dependency on db_core.
#[allow(dead_code)]
const _AGENT_RUNTIME_USED: fn() = || {
    let _ = agent_runtime::stop_daemons;
};

// ─── native connection pool caches ───────────────────────────────────────────

static MYSQL_POOLS: Lazy<Mutex<HashMap<String, db_core::mysql::MySqlPool>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static PG_POOLS: Lazy<Mutex<HashMap<String, db_core::postgres::PgPool>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
// SSH tunnels must be kept alive as long as the corresponding pool is alive.
// Stored alongside pools, keyed on the same source_id.
static MYSQL_TUNNELS: Lazy<Mutex<HashMap<String, db_core::ssh_tunnel::SshTunnel>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static PG_TUNNELS: Lazy<Mutex<HashMap<String, db_core::ssh_tunnel::SshTunnel>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

async fn get_or_create_mysql_pool(
    source_id: &str,
    cfg: &db_core::mysql::MySqlConfig,
) -> Result<db_core::mysql::MySqlPool> {
    // Check cache first (lock scope must be short — MySqlPool is Clone).
    if let Some(pool) = MYSQL_POOLS.lock().expect("MYSQL_POOLS").get(source_id).cloned() {
        return Ok(pool);
    }
    // If SSH tunnel is configured, open it and redirect host/port.
    let resolved_cfg = if let Some(ref ssh_cfg) = cfg.ssh {
        let tunnel = db_core::ssh_tunnel::SshTunnel::open(ssh_cfg, &cfg.host, cfg.port)
            .await
            .map_err(|e| AgentError::Database(format!("SSH tunnel: {e}")))?;
        let local_port = tunnel.local_port();
        let redirected = db_core::mysql::MySqlConfig {
            host: "127.0.0.1".to_string(),
            port: local_port,
            username: cfg.username.clone(),
            database: cfg.database.clone(),
            password: cfg.password.clone(),
            ssl_mode: cfg.ssl_mode.clone(),
            ssl_ca_cert: cfg.ssl_ca_cert.clone(),
            ssh: None, // already handled
        };
        MYSQL_TUNNELS.lock().expect("MYSQL_TUNNELS").insert(source_id.to_string(), tunnel);
        redirected
    } else {
        db_core::mysql::MySqlConfig {
            host: cfg.host.clone(),
            port: cfg.port,
            username: cfg.username.clone(),
            database: cfg.database.clone(),
            password: cfg.password.clone(),
            ssl_mode: cfg.ssl_mode.clone(),
            ssl_ca_cert: cfg.ssl_ca_cert.clone(),
            ssh: None,
        }
    };
    let pool = db_core::mysql::create_pool(&resolved_cfg)
        .await
        .map_err(|e| AgentError::Database(format!("MySQL connect: {e}")))?;
    MYSQL_POOLS.lock().expect("MYSQL_POOLS").insert(source_id.to_string(), pool.clone());
    Ok(pool)
}

async fn get_or_create_pg_pool(
    source_id: &str,
    cfg: &db_core::postgres::PgConfig,
) -> Result<db_core::postgres::PgPool> {
    if let Some(pool) = PG_POOLS.lock().expect("PG_POOLS").get(source_id).cloned() {
        return Ok(pool);
    }
    // If SSH tunnel is configured, open it and redirect host/port.
    let resolved_cfg = if let Some(ref ssh_cfg) = cfg.ssh {
        let tunnel = db_core::ssh_tunnel::SshTunnel::open(ssh_cfg, &cfg.host, cfg.port)
            .await
            .map_err(|e| AgentError::Database(format!("SSH tunnel: {e}")))?;
        let local_port = tunnel.local_port();
        let redirected = db_core::postgres::PgConfig {
            host: "127.0.0.1".to_string(),
            port: local_port,
            username: cfg.username.clone(),
            database: cfg.database.clone(),
            password: cfg.password.clone(),
            ssl_mode: cfg.ssl_mode.clone(),
            ssl_ca_cert: cfg.ssl_ca_cert.clone(),
            ssh: None, // already handled
        };
        PG_TUNNELS.lock().expect("PG_TUNNELS").insert(source_id.to_string(), tunnel);
        redirected
    } else {
        db_core::postgres::PgConfig {
            host: cfg.host.clone(),
            port: cfg.port,
            username: cfg.username.clone(),
            database: cfg.database.clone(),
            password: cfg.password.clone(),
            ssl_mode: cfg.ssl_mode.clone(),
            ssl_ca_cert: cfg.ssl_ca_cert.clone(),
            ssh: None,
        }
    };
    let pool = db_core::postgres::create_pool(&resolved_cfg)
        .await
        .map_err(|e| AgentError::Database(format!("PostgreSQL connect: {e}")))?;
    PG_POOLS.lock().expect("PG_POOLS").insert(source_id.to_string(), pool.clone());
    Ok(pool)
}

fn ssh_config_to_tunnel_config(
    ssh: &crate::db_creds::SshConfig,
) -> db_core::ssh_tunnel::SshTunnelConfig {
    let auth = match (&ssh.key_path, &ssh.password) {
        (Some(path), _) => db_core::ssh_tunnel::SshAuth::PrivateKey {
            path: path.clone(),
            passphrase: ssh.key_passphrase.clone(),
        },
        (None, Some(pw)) => db_core::ssh_tunnel::SshAuth::Password(pw.clone()),
        (None, None) => db_core::ssh_tunnel::SshAuth::Password(String::new()),
    };
    db_core::ssh_tunnel::SshTunnelConfig {
        host: ssh.host.clone(),
        port: ssh.port,
        username: ssh.username.clone(),
        auth,
    }
}

fn mysql_config_from_db_config(cfg: &DbConnectionConfig) -> Option<db_core::mysql::MySqlConfig> {
    match cfg {
        DbConnectionConfig::Mysql { host, port, username, database, password, ssh } => {
            Some(db_core::mysql::MySqlConfig {
                host: host.clone(),
                port: *port,
                username: username.clone(),
                database: database.clone(),
                password: password.clone(),
                ssl_mode: None,
                ssl_ca_cert: None,
                ssh: ssh.as_ref().map(ssh_config_to_tunnel_config),
            })
        }
        _ => None,
    }
}

fn pg_config_from_db_config(cfg: &DbConnectionConfig) -> Option<db_core::postgres::PgConfig> {
    match cfg {
        DbConnectionConfig::Postgresql { host, port, username, database, password, ssh } => {
            Some(db_core::postgres::PgConfig {
                host: host.clone(),
                port: *port,
                username: username.clone(),
                database: database.clone(),
                password: password.clone(),
                ssl_mode: None,
                ssl_ca_cert: None,
                ssh: ssh.as_ref().map(ssh_config_to_tunnel_config),
            })
        }
        _ => None,
    }
}

fn db_core_result_to_query_result(r: db_core::types::QueryResult) -> QueryResult {
    QueryResult {
        columns: r.columns,
        row_count: r.row_count,
        rows: r.rows,
        duration_ms: r.duration_ms,
        truncated: r.truncated,
    }
}

fn db_core_table_info_to_schema(t: db_core::types::TableInfo) -> TableSchema {
    TableSchema {
        table_name: t.table_name,
        columns: t.columns.into_iter().map(|c| ColumnInfo {
            name: c.name,
            data_type: c.data_type,
            nullable: c.nullable,
            is_primary_key: c.is_primary_key,
            default_value: c.default_value,
            comment: c.comment,
        }).collect(),
        row_count_estimate: t.row_count_estimate.map(|v| v as i64),
        size_bytes: t.size_bytes.map(|v| v as i64),
        indexes: t.indexes.into_iter().map(|i| IndexInfo {
            name: i.name,
            columns: i.columns,
            unique: i.unique,
            primary: i.primary,
        }).collect(),
        foreign_keys: vec![],
    }
}

const MAX_ROWS: usize = 100_000;
const TIMEOUT_SECS: u64 = 60;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub is_primary_key: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IndexInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    pub primary: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ForeignKeyInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TableSchema {
    pub table_name: String,
    pub columns: Vec<ColumnInfo>,
    pub row_count_estimate: Option<i64>,
    /// Approximate on-disk size in bytes (table + indexes). None if unavailable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<IndexInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub foreign_keys: Vec<ForeignKeyInfo>,
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

/// Build the DuckDB ATTACH connection string from a DbConnectionConfig or
/// a SQLite SourceKind (SQLite has no credentials, only a path).
/// Returns (db_type_str, conn_str).
fn build_attach_string_from_config(cfg: &DbConnectionConfig) -> (&'static str, String) {
    match cfg {
        DbConnectionConfig::Mysql { host, port, username, database, password, .. } => {
            let conn_str = format!(
                "host={host} port={port} database={database} user={username} password={password}"
            );
            ("MYSQL", conn_str)
        }
        DbConnectionConfig::Postgresql { host, port, username, database, password, .. } => {
            let conn_str = format!(
                "host={host} port={port} dbname={database} user={username} password={password}"
            );
            ("POSTGRES", conn_str)
        }
        DbConnectionConfig::Snowflake { account, username, warehouse, database, schema, password } => {
            let conn_str = format!(
                "account={account};user={username};password={password};\
                 warehouse={warehouse};database={database};schema={schema}"
            );
            ("SNOWFLAKE", conn_str)
        }
        DbConnectionConfig::Clickhouse { host, port, username, database, .. } => {
            // ClickHouse uses its own HTTP engine, not DuckDB ATTACH.
            // This branch should not be reached via build_attach_string_from_config,
            // but we provide a placeholder to satisfy exhaustiveness.
            let conn_str = format!("host={host} port={port} database={database} user={username}");
            ("CLICKHOUSE", conn_str)
        }
        DbConnectionConfig::Mongodb { host, port, username, database, .. } => {
            let conn_str = format!("host={host} port={port} database={database} user={username}");
            ("MONGODB", conn_str)
        }
        DbConnectionConfig::Redis { host, port, db, .. } => {
            let conn_str = format!("host={host} port={port} db={db}");
            ("REDIS", conn_str)
        }
    }
}

/// Build ATTACH string for a SQLite source (path-only, no DbConnectionConfig).
fn build_sqlite_attach_string(kind: &SourceKind) -> Option<(&'static str, String)> {
    match kind {
        SourceKind::Sqlite { path } => Some(("SQLITE", path.to_string_lossy().to_string())),
        _ => None,
    }
}

/// Return the DuckDB extension install SQL for a DbConnectionConfig.
fn extension_install_sql_for_config(cfg: &DbConnectionConfig) -> &'static str {
    match cfg {
        DbConnectionConfig::Mysql { .. } => "INSTALL mysql; LOAD mysql;",
        DbConnectionConfig::Postgresql { .. } => "INSTALL postgres; LOAD postgres;",
        DbConnectionConfig::Snowflake { .. } => "INSTALL snowflake FROM community; LOAD snowflake;",
        // Clickhouse/Mongodb/Redis use their own engines, not DuckDB ATTACH.
        _ => "",
    }
}

/// Return the DuckDB extension name for error messages.
fn extension_name_for_config(cfg: &DbConnectionConfig) -> &'static str {
    match cfg {
        DbConnectionConfig::Mysql { .. } => "mysql",
        DbConnectionConfig::Postgresql { .. } => "postgres",
        DbConnectionConfig::Snowflake { .. } => "snowflake",
        _ => "unknown",
    }
}

/// Default schema name inside the attached DuckDB database alias.
fn default_schema_for_config(cfg: &DbConnectionConfig) -> Option<&'static str> {
    match cfg {
        DbConnectionConfig::Mysql { .. }
        | DbConnectionConfig::Postgresql { .. }
        | DbConnectionConfig::Snowflake { .. } => Some("main"),
        _ => None,
    }
}

/// Lookup a DB source in config and load its full connection config from the vault.
/// For SQLite sources (no vault entry), returns Err — callers must handle SQLite separately.
fn resolve_source<'a>(
    source_id: &str,
    config: &'a Config,
) -> Result<(&'a SourceKind, Option<DbConnectionConfig>)> {
    let source = config
        .sources
        .iter()
        .find(|s| s.id == source_id)
        .ok_or_else(|| AgentError::Database(format!("DB source not found: {source_id}")))?;

    // SQLite has no vault entry — return None for the config.
    if matches!(source.kind, SourceKind::Sqlite { .. }) {
        return Ok((&source.kind, None));
    }

    // AgentDb: password lives in the dedicated agent keychain, not the
    // db_creds vault. Skip the vault lookup so we don't error out on
    // the missing entry — execute / introspect read the password from
    // `agent_db_creds` directly.
    if matches!(source.kind, SourceKind::AgentDb { .. }) {
        return Ok((&source.kind, None));
    }

    let cfg = load_connection(source_id)?;
    Ok((&source.kind, Some(cfg)))
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

    let (kind, maybe_cfg) = resolve_source(source_id, config)?;

    // SQLite path (no vault credentials).
    if let SourceKind::Sqlite { .. } = kind {
        let (db_type, conn_str) = build_sqlite_attach_string(kind)
            .ok_or_else(|| AgentError::Database("Expected SQLite source".to_string()))?;
        let ext_sql = "INSTALL sqlite; LOAD sqlite;".to_string();
        let sql_owned = sql.to_string();
        let task = tokio::task::spawn_blocking(move || {
            run_db_query_blocking(&sql_owned, &conn_str, db_type, &ext_sql, "sqlite", None)
        });
        return match tokio::time::timeout(Duration::from_secs(TIMEOUT_SECS), task).await {
            Ok(join) => join.map_err(|e| AgentError::Database(format!("task: {e}")))?,
            Err(_) => Err(AgentError::Database(format!("DB query timed out after {TIMEOUT_SECS}s"))),
        };
    }

    // Agent (JDBC) path — Oracle, DB2, SAP HANA, Teradata, etc.
    if matches!(kind, SourceKind::AgentDb { .. }) {
        return tokio::time::timeout(
            Duration::from_secs(TIMEOUT_SECS),
            execute_agent_query(sql, source_id, config),
        )
        .await
        .map_err(|_| {
            AgentError::Database(format!("Agent DB query timed out after {TIMEOUT_SECS}s"))
        })?;
    }

    let cfg = maybe_cfg.ok_or_else(|| {
        AgentError::Database(format!("No credentials for source {source_id}"))
    })?;

    // Native MySQL pool path.
    if let Some(mysql_cfg) = mysql_config_from_db_config(&cfg) {
        let pool = get_or_create_mysql_pool(source_id, &mysql_cfg).await?;
        let result = tokio::time::timeout(
            Duration::from_secs(TIMEOUT_SECS),
            db_core::mysql::execute_query(&pool, sql, MAX_ROWS),
        )
        .await
        .map_err(|_| AgentError::Database(format!("MySQL query timed out after {TIMEOUT_SECS}s")))?
        .map_err(AgentError::Database)?;
        return Ok(db_core_result_to_query_result(result));
    }

    // Native PostgreSQL pool path.
    if let Some(pg_cfg) = pg_config_from_db_config(&cfg) {
        let pool = get_or_create_pg_pool(source_id, &pg_cfg).await?;
        let result = tokio::time::timeout(
            Duration::from_secs(TIMEOUT_SECS),
            db_core::postgres::execute_query(&pool, sql, MAX_ROWS),
        )
        .await
        .map_err(|_| AgentError::Database(format!("PostgreSQL query timed out after {TIMEOUT_SECS}s")))?
        .map_err(AgentError::Database)?;
        return Ok(db_core_result_to_query_result(result));
    }

    // Dispatch to specialised engines for HTTP / document / key-value sources.
    match &cfg {
        DbConnectionConfig::Clickhouse { host, port, username, database, password } => {
            let (host, port, username, database, pw) =
                (host.clone(), *port, username.clone(), database.clone(), password.clone());
            let sql_owned = sql.to_string();
            let task = tokio::task::spawn_blocking(move || {
                execute_clickhouse_query_blocking(&sql_owned, &host, port, &username, &pw, &database)
            });
            return match tokio::time::timeout(Duration::from_secs(TIMEOUT_SECS), task).await {
                Ok(join) => join.map_err(|e| AgentError::Database(format!("task: {e}")))?,
                Err(_) => Err(AgentError::Database(format!("ClickHouse query timed out after {TIMEOUT_SECS}s"))),
            };
        }
        DbConnectionConfig::Mongodb { host, port, username, database, auth_db, password } => {
            let (host, port, username, database, auth_db, pw) =
                (host.clone(), *port, username.clone(), database.clone(), auth_db.clone(), password.clone());
            let sql_owned = sql.to_string();
            return execute_mongodb_query(&sql_owned, &host, port, &username, &pw, &database, &auth_db).await;
        }
        DbConnectionConfig::Redis { host, port, db, password } => {
            let (host, port, db, pw) = (host.clone(), *port, *db, password.clone());
            let sql_owned = sql.to_string();
            return execute_redis_query(&sql_owned, &host, port, db, &pw).await;
        }
        _ => {}
    }

    // DuckDB ATTACH path: Snowflake only (MySQL/PG handled above).
    let (db_type, conn_str) = build_attach_string_from_config(&cfg);
    let ext_sql = extension_install_sql_for_config(&cfg).to_string();
    let ext_name = extension_name_for_config(&cfg).to_string();
    let schema = default_schema_for_config(&cfg).map(|s| s.to_string());
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
    let (kind, maybe_cfg) = resolve_source(source_id, config)?;

    // SQLite path.
    if let SourceKind::Sqlite { .. } = kind {
        let (db_type, conn_str) = build_sqlite_attach_string(kind)
            .ok_or_else(|| AgentError::Database("Expected SQLite source".to_string()))?;
        let ext_sql = "INSTALL sqlite; LOAD sqlite;".to_string();
        let task = tokio::task::spawn_blocking(move || {
            introspect_blocking(&conn_str, db_type, &ext_sql, None, "", true)
        });
        return match tokio::time::timeout(Duration::from_secs(30), task).await {
            Ok(join) => join.map_err(|e| AgentError::Database(format!("task: {e}")))?,
            Err(_) => Err(AgentError::Database("Schema introspection timed out".to_string())),
        };
    }

    // Agent (JDBC) path — Oracle, DB2, SAP HANA, Teradata, etc.
    if matches!(kind, SourceKind::AgentDb { .. }) {
        return tokio::time::timeout(
            Duration::from_secs(120),
            introspect_agent_schema(source_id, config),
        )
        .await
        .map_err(|_| AgentError::Database("Agent schema introspection timed out".to_string()))?;
    }

    let cfg = maybe_cfg.ok_or_else(|| {
        AgentError::Database(format!("No credentials for source {source_id}"))
    })?;

    // Native MySQL introspection.
    if let Some(mysql_cfg) = mysql_config_from_db_config(&cfg) {
        let database = match &cfg {
            DbConnectionConfig::Mysql { database, .. } => database.clone(),
            _ => unreachable!(),
        };
        let pool = get_or_create_mysql_pool(source_id, &mysql_cfg).await?;
        let tables = tokio::time::timeout(
            Duration::from_secs(30),
            db_core::mysql::introspect_schema(&pool, &database),
        )
        .await
        .map_err(|_| AgentError::Database("MySQL introspect timed out".to_string()))?
        .map_err(AgentError::Database)?;
        return Ok(tables.into_iter().map(db_core_table_info_to_schema).collect());
    }

    // Native PostgreSQL introspection.
    if let Some(pg_cfg) = pg_config_from_db_config(&cfg) {
        let pool = get_or_create_pg_pool(source_id, &pg_cfg).await?;
        let tables = tokio::time::timeout(
            Duration::from_secs(30),
            db_core::postgres::introspect_schema(&pool, "public"),
        )
        .await
        .map_err(|_| AgentError::Database("PostgreSQL introspect timed out".to_string()))?
        .map_err(AgentError::Database)?;
        return Ok(tables.into_iter().map(db_core_table_info_to_schema).collect());
    }

    match &cfg {
        DbConnectionConfig::Clickhouse { host, port, username, database, password } => {
            let (host, port, username, database, pw) =
                (host.clone(), *port, username.clone(), database.clone(), password.clone());
            let task = tokio::task::spawn_blocking(move || {
                introspect_clickhouse_blocking(&host, port, &username, &pw, &database)
            });
            return match tokio::time::timeout(Duration::from_secs(30), task).await {
                Ok(join) => join.map_err(|e| AgentError::Database(format!("task: {e}")))?,
                Err(_) => Err(AgentError::Database("ClickHouse introspect timed out".to_string())),
            };
        }
        DbConnectionConfig::Mongodb { host, port, username, database, auth_db, password } => {
            let (host, port, username, database, auth_db, pw) =
                (host.clone(), *port, username.clone(), database.clone(), auth_db.clone(), password.clone());
            return introspect_mongodb(&host, port, &username, &pw, &database, &auth_db).await;
        }
        DbConnectionConfig::Redis { .. } => {
            // Redis: single virtual table
            return Ok(vec![TableSchema {
                table_name: "keys".to_string(),
                columns: vec![
                    ColumnInfo { name: "key".to_string(), data_type: "TEXT".to_string(), nullable: false, is_primary_key: true, default_value: None, comment: None },
                    ColumnInfo { name: "value".to_string(), data_type: "TEXT".to_string(), nullable: true, is_primary_key: false, default_value: None, comment: None },
                    ColumnInfo { name: "value_type".to_string(), data_type: "TEXT".to_string(), nullable: false, is_primary_key: false, default_value: None, comment: None },
                    ColumnInfo { name: "ttl".to_string(), data_type: "INTEGER".to_string(), nullable: false, is_primary_key: false, default_value: None, comment: None },
                ],
                row_count_estimate: None,
                size_bytes: None,
                indexes: vec![],
                foreign_keys: vec![],
            }]);
        }
        _ => {}
    }

    // DuckDB ATTACH path: Snowflake only.
    let (db_type, conn_str) = build_attach_string_from_config(&cfg);
    let ext_sql = extension_install_sql_for_config(&cfg).to_string();
    let schema = default_schema_for_config(&cfg).map(|s| s.to_string());
    let db_name = db_name_from_config(&cfg).to_string();

    let task = tokio::task::spawn_blocking(move || {
        introspect_blocking(&conn_str, db_type, &ext_sql, schema.as_deref(), &db_name, false)
    });

    match tokio::time::timeout(Duration::from_secs(30), task).await {
        Ok(join) => join.map_err(|e| AgentError::Database(format!("task: {e}")))?,
        Err(_) => Err(AgentError::Database("Schema introspection timed out".to_string())),
    }
}

fn db_name_from_config(cfg: &DbConnectionConfig) -> &str {
    match cfg {
        DbConnectionConfig::Mysql { database, .. } => database.as_str(),
        DbConnectionConfig::Postgresql { database, .. } => database.as_str(),
        DbConnectionConfig::Snowflake { database, .. } => database.as_str(),
        DbConnectionConfig::Clickhouse { database, .. } => database.as_str(),
        DbConnectionConfig::Mongodb { database, .. } => database.as_str(),
        DbConnectionConfig::Redis { .. } => "",
    }
}

/// Extract column names from a Postgres `CREATE INDEX ... (col1, col2)` definition.
fn parse_indexdef_columns(indexdef: &str) -> Vec<String> {
    let start = indexdef.rfind('(').map(|i| i + 1).unwrap_or(indexdef.len());
    let end = indexdef.rfind(')').unwrap_or(indexdef.len());
    if start >= end {
        return vec![];
    }
    indexdef[start..end]
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn introspect_blocking(
    conn_str: &str,
    db_type: &str,
    ext_install_sql: &str,
    _schema: Option<&str>,
    db_name: &str,
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

    // Normalize to lowercase so match arms work regardless of how
    // build_attach_string capitalises the type ("POSTGRES" vs "postgres").
    let db_type_lc = db_type.to_ascii_lowercase();
    let db_type = db_type_lc.as_str();

    const SYS_DUCKDB: &str =
        "'information_schema', 'pg_catalog', 'pg_toast', \
         'pg_internal', 'INFORMATION_SCHEMA', 'pg_toast_temp_1'";

    // ── 1. PK column sets — via native catalog (postgres_query / mysql_query)
    // duckdb_constraints() only tracks DuckDB-native constraints, not remote ones.
    // postgres_query / mysql_query push the SQL to the actual server.
    let mut pk_cols: std::collections::HashMap<String, std::collections::HashSet<String>> =
        std::collections::HashMap::new();

    let pk_native_sql: Option<String> = match db_type {
        "postgres" => Some(
            "SELECT * FROM postgres_query('_db', \
             'SELECT kcu.table_name, kcu.column_name \
              FROM information_schema.table_constraints tc \
              JOIN information_schema.key_column_usage kcu \
                ON tc.constraint_name = kcu.constraint_name \
               AND tc.table_schema = kcu.table_schema \
              WHERE tc.constraint_type = ''PRIMARY KEY'' \
                AND tc.table_schema NOT IN \
                    (''information_schema'', ''pg_catalog'', ''pg_toast'')')"
            .to_string(),
        ),
        // Direct attachment access — same connection path as duckdb_columns(),
        // avoids mysql_query() which opens a new TCP connection that drops.
        "mysql" => Some(format!(
            "SELECT TABLE_NAME, COLUMN_NAME \
             FROM _db.information_schema.KEY_COLUMN_USAGE \
             WHERE CONSTRAINT_NAME = 'PRIMARY' \
               AND TABLE_SCHEMA = '{db_name}'",
        )),
        _ => None,
    };
    if let Some(sql) = pk_native_sql {
        match conn.prepare(&sql) {
            Err(e) => eprintln!("[introspect] pk prepare failed: {e}"),
            Ok(mut st) => match st.query([]) {
                Err(e) => eprintln!("[introspect] pk query failed: {e}"),
                Ok(mut rows) => {
                    while let Ok(Some(row)) = rows.next() {
                        let tbl: String = row.get(0).unwrap_or_default();
                        let col: String = row.get(1).unwrap_or_default();
                        pk_cols.entry(tbl).or_default().insert(col);
                    }
                }
            },
        }
    }

    // ── 2. Columns (duckdb_columns — proven reliable for attached DBs) ──────
    let col_sql = format!(
        "SELECT table_name, column_name, data_type, \
         CASE WHEN is_nullable THEN 'YES' ELSE 'NO' END, \
         column_default \
         FROM duckdb_columns() \
         WHERE database_name = '_db' \
           AND schema_name NOT IN ({SYS_DUCKDB}) \
           AND NOT internal \
         ORDER BY table_name, column_index"
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
        let default_value: Option<String> = row.get::<_, Option<String>>(4).unwrap_or(None);
        let is_pk = pk_cols
            .get(&table)
            .map(|s| s.contains(&col_name))
            .unwrap_or(false);
        tables.entry(table).or_default().push(ColumnInfo {
            name: col_name,
            data_type,
            nullable: nullable.eq_ignore_ascii_case("YES"),
            is_primary_key: is_pk,
            default_value,
            comment: None,
        });
    }

    // ── 3. Row counts + sizes via native server catalog ─────────────────────
    // postgres_query / mysql_query push SQL to the actual database server so
    // pg_class.reltuples and information_schema.TABLES are always reachable.
    let mut row_counts: std::collections::HashMap<String, i64> =
        std::collections::HashMap::new();
    let mut table_sizes: std::collections::HashMap<String, i64> =
        std::collections::HashMap::new();

    let stats_sql: Option<String> = match db_type {
        "postgres" => Some(
            "SELECT * FROM postgres_query('_db', \
             'SELECT c.relname::text, \
                     GREATEST(c.reltuples::bigint, 0), \
                     pg_total_relation_size(c.oid)::bigint \
              FROM pg_class c \
              JOIN pg_namespace n ON n.oid = c.relnamespace \
              WHERE c.relkind = ''r'' \
                AND n.nspname NOT IN \
                    (''information_schema'',''pg_catalog'',''pg_toast'',''pg_toast_temp_1'')')"
            .to_string(),
        ),
        "mysql" => Some(format!(
            "SELECT TABLE_NAME, TABLE_ROWS, DATA_LENGTH + INDEX_LENGTH \
             FROM _db.information_schema.TABLES \
             WHERE TABLE_SCHEMA = '{db_name}' AND TABLE_TYPE = 'BASE TABLE'",
        )),
        _ => None,
    };
    if let Some(sql) = stats_sql {
        match conn.prepare(&sql) {
            Err(e) => eprintln!("[introspect] stats prepare failed: {e}"),
            Ok(mut st) => match st.query([]) {
                Err(e) => eprintln!("[introspect] stats query failed: {e}"),
                Ok(mut rows) => {
                    while let Ok(Some(row)) = rows.next() {
                        let tbl: String = row.get(0).unwrap_or_default();
                        let cnt: i64 = row.get::<_, i64>(1)
                            .or_else(|_| row.get::<_, u64>(1).map(|v| v as i64))
                            .unwrap_or(0);
                        let sz: i64 = row.get::<_, i64>(2)
                            .or_else(|_| row.get::<_, u64>(2).map(|v| v as i64))
                            .unwrap_or(0);
                        if cnt >= 0 { row_counts.insert(tbl.clone(), cnt); }
                        if sz > 0  { table_sizes.insert(tbl, sz); }
                    }
                }
            },
        }
    }

    // ── 4. Indexes — native catalog ─────────────────────────────────────────
    // Postgres: use pg_indexes view (returns indexdef string, avoids int2vector
    // casting issues with array_position). Parse column names from indexdef in Rust.
    // MySQL: information_schema.STATISTICS with GROUP_CONCAT.
    let mut table_indexes: std::collections::HashMap<String, Vec<IndexInfo>> =
        std::collections::HashMap::new();

    let idx_native_sql: Option<String> = match db_type {
        "postgres" => Some(
            // pg_indexes already excludes system indexes; filter PKs via pg_constraint.
            // Returns: tablename, indexname, is_unique (t/f), indexdef
            "SELECT * FROM postgres_query('_db', \
             'SELECT pi.tablename::text, pi.indexname::text, \
                     pi.indexdef LIKE ''CREATE UNIQUE%'', \
                     pi.indexdef \
              FROM pg_indexes pi \
              WHERE pi.schemaname NOT IN \
                    (''information_schema'', ''pg_catalog'', ''pg_toast'') \
                AND NOT EXISTS ( \
                    SELECT 1 FROM pg_constraint pc \
                    WHERE pc.conname = pi.indexname \
                      AND pc.contype = ''p'' \
                )')"
            .to_string(),
        ),
        "mysql" => Some(format!(
            "SELECT TABLE_NAME, INDEX_NAME, NON_UNIQUE, COLUMN_NAME \
             FROM _db.information_schema.STATISTICS \
             WHERE TABLE_SCHEMA = '{db_name}' AND INDEX_NAME <> 'PRIMARY' \
             ORDER BY TABLE_NAME, INDEX_NAME, SEQ_IN_INDEX",
        )),
        _ => None,
    };
    if let Some(sql) = idx_native_sql {
        match conn.prepare(&sql) {
            Err(e) => eprintln!("[introspect] index query prepare failed: {e}"),
            Ok(mut st) => match st.query([]) {
                Err(e) => eprintln!("[introspect] index query exec failed: {e}"),
                Ok(mut rows) => {
                    // For MySQL: per-row (no GROUP_CONCAT), group by (tbl, name) here.
                    // For Postgres: each row has the full indexdef; parse columns from it.
                    let mut mysql_idx_acc: std::collections::BTreeMap<
                        (String, String),
                        (bool, Vec<String>),
                    > = std::collections::BTreeMap::new();

                    while let Ok(Some(row)) = rows.next() {
                        let tbl: String = row.get(0).unwrap_or_default();
                        let name: String = row.get(1).unwrap_or_default();
                        if db_type == "postgres" {
                            // col2 = unique bool, col3 = indexdef string
                            let indexdef: String = row.get(3).unwrap_or_default();
                            let columns = parse_indexdef_columns(&indexdef);
                            let unique: bool = row.get::<_, bool>(2).unwrap_or(false);
                            table_indexes.entry(tbl).or_default().push(IndexInfo {
                                name, columns, unique, primary: false,
                            });
                        } else {
                            // col2 = NON_UNIQUE (0 = unique), col3 = COLUMN_NAME
                            let non_unique: i64 = row.get::<_, i64>(2).unwrap_or(1);
                            let col: String = row.get(3).unwrap_or_default();
                            let entry = mysql_idx_acc.entry((tbl, name))
                                .or_insert_with(|| (non_unique == 0, vec![]));
                            if !col.is_empty() { entry.1.push(col); }
                        }
                    }
                    for ((tbl, name), (unique, columns)) in mysql_idx_acc {
                        table_indexes.entry(tbl).or_default().push(IndexInfo {
                            name, columns, unique, primary: false,
                        });
                    }
                }
            },
        }
    }

    // ── 5. Foreign keys — native catalog ────────────────────────────────────
    let mut table_fks: std::collections::HashMap<String, Vec<ForeignKeyInfo>> =
        std::collections::HashMap::new();

    let fk_native_sql: Option<String> = match db_type {
        "postgres" => Some(
            "SELECT * FROM postgres_query('_db', \
             'SELECT tc.constraint_name, tc.table_name, \
                     kcu.column_name, \
                     ccu.table_name AS foreign_table_name, \
                     ccu.column_name AS foreign_column_name \
              FROM information_schema.table_constraints tc \
              JOIN information_schema.key_column_usage kcu \
                ON tc.constraint_name = kcu.constraint_name \
               AND tc.table_schema = kcu.table_schema \
              JOIN information_schema.constraint_column_usage ccu \
                ON ccu.constraint_name = tc.constraint_name \
               AND ccu.table_schema = tc.table_schema \
              WHERE tc.constraint_type = ''FOREIGN KEY'' \
                AND tc.table_schema NOT IN \
                    (''information_schema'', ''pg_catalog'', ''pg_toast'') \
              ORDER BY tc.constraint_name, kcu.ordinal_position')"
            .to_string(),
        ),
        "mysql" => Some(format!(
            "SELECT CONSTRAINT_NAME, TABLE_NAME, COLUMN_NAME, \
                    REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME \
             FROM _db.information_schema.KEY_COLUMN_USAGE \
             WHERE TABLE_SCHEMA = '{db_name}' \
               AND REFERENCED_TABLE_NAME IS NOT NULL \
             ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION",
        )),
        _ => None,
    };
    if let Some(sql) = fk_native_sql {
        let mut fk_map: std::collections::BTreeMap<
            (String, String),
            (Vec<String>, String, Vec<String>),
        > = std::collections::BTreeMap::new();
        match conn.prepare(&sql) {
            Err(e) => eprintln!("[introspect] fk prepare failed: {e}"),
            Ok(mut st) => match st.query([]) {
                Err(e) => eprintln!("[introspect] fk query failed: {e}"),
                Ok(mut rows) => {
                    while let Ok(Some(row)) = rows.next() {
                        let cname: String = row.get(0).unwrap_or_default();
                        let tbl: String = row.get(1).unwrap_or_default();
                        let col: String = row.get(2).unwrap_or_default();
                        let ref_tbl: String = row.get(3).unwrap_or_default();
                        let ref_col: String = row.get(4).unwrap_or_default();
                        let e = fk_map.entry((tbl, cname))
                            .or_insert_with(|| (vec![], ref_tbl, vec![]));
                        e.0.push(col);
                        e.2.push(ref_col);
                    }
                }
            },
        }
        for ((tbl, name), (columns, ref_table, ref_columns)) in fk_map {
            table_fks.entry(tbl).or_default().push(ForeignKeyInfo {
                name, columns, ref_table, ref_columns,
            });
        }
    }

    eprintln!(
        "[introspect] {db_type}: {} tables, {} with row counts, {} with sizes, {} index entries, {} fk entries",
        tables.len(), row_counts.len(), table_sizes.len(), table_indexes.len(), table_fks.len()
    );

    Ok(tables
        .into_iter()
        .map(|(table_name, columns)| {
            let size_bytes = table_sizes.get(&table_name).copied().filter(|&s| s > 0);
            let indexes = table_indexes.remove(&table_name).unwrap_or_default();
            let foreign_keys = table_fks.remove(&table_name).unwrap_or_default();
            let row_count = row_counts.get(&table_name).copied();
            TableSchema {
                table_name,
                columns,
                row_count_estimate: row_count,
                size_bytes,
                indexes,
                foreign_keys,
            }
        })
        .collect())
}

/// Open an SSH tunnel if configured and return a resolved (host, port) pair.
/// The returned `SshTunnel` must be kept alive for the duration of the test.
async fn resolve_ssh_for_test(
    ssh: Option<&crate::db_creds::SshConfig>,
    remote_host: &str,
    remote_port: u16,
) -> Result<(String, u16, Option<db_core::ssh_tunnel::SshTunnel>)> {
    if let Some(ssh_cfg) = ssh {
        let tunnel_cfg = ssh_config_to_tunnel_config(ssh_cfg);
        let tunnel = db_core::ssh_tunnel::SshTunnel::open(&tunnel_cfg, remote_host, remote_port)
            .await
            .map_err(|e| AgentError::Database(format!("SSH tunnel: {e}")))?;
        let port = tunnel.local_port();
        Ok(("127.0.0.1".to_string(), port, Some(tunnel)))
    } else {
        Ok((remote_host.to_string(), remote_port, None))
    }
}

/// Test a DB connection using provided credentials (before persisting).
/// Accepts a DbConnectionConfig (for all non-SQLite types) or a SQLite SourceKind.
pub fn test_connection_blocking(
    cfg: &DbConnectionConfig,
) -> Result<()> {
    // Native MySQL test.
    if let Some(mut mysql_cfg) = mysql_config_from_db_config(cfg) {
        let ssh_opt = match cfg {
            DbConnectionConfig::Mysql { ssh, .. } => ssh.as_ref(),
            _ => None,
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| AgentError::Database(format!("runtime: {e}")))?;
        return rt.block_on(async {
            let (host, port, _tunnel) =
                resolve_ssh_for_test(ssh_opt, &mysql_cfg.host, mysql_cfg.port).await?;
            mysql_cfg.host = host;
            mysql_cfg.port = port;
            mysql_cfg.ssh = None;
            db_core::mysql::test_connection(&mysql_cfg)
                .await
                .map_err(AgentError::Database)
        });
    }

    // Native PostgreSQL test.
    if let Some(mut pg_cfg) = pg_config_from_db_config(cfg) {
        let ssh_opt = match cfg {
            DbConnectionConfig::Postgresql { ssh, .. } => ssh.as_ref(),
            _ => None,
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| AgentError::Database(format!("runtime: {e}")))?;
        return rt.block_on(async {
            let (host, port, _tunnel) =
                resolve_ssh_for_test(ssh_opt, &pg_cfg.host, pg_cfg.port).await?;
            pg_cfg.host = host;
            pg_cfg.port = port;
            pg_cfg.ssh = None;
            db_core::postgres::test_connection(&pg_cfg)
                .await
                .map_err(AgentError::Database)
        });
    }

    match cfg {
        DbConnectionConfig::Clickhouse { host, port, username, database, password } => {
            return test_clickhouse_connection_blocking(host, *port, username, password, database);
        }
        DbConnectionConfig::Mongodb { host, port, username, auth_db, password, .. } => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| AgentError::Database(format!("runtime: {e}")))?;
            return rt.block_on(test_mongodb_connection(host, *port, username, password, auth_db));
        }
        DbConnectionConfig::Redis { host, port, db, password } => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| AgentError::Database(format!("runtime: {e}")))?;
            return rt.block_on(test_redis_connection(host, *port, *db, password));
        }
        _ => {}
    }

    // DuckDB ATTACH path for Snowflake.
    let (db_type, conn_str) = build_attach_string_from_config(cfg);
    let ext_install_sql = extension_install_sql_for_config(cfg);
    let ext_name = extension_name_for_config(cfg);
    let schema = default_schema_for_config(cfg);

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

/// Test a SQLite connection (path only, no DbConnectionConfig needed).
pub fn test_sqlite_connection_blocking(path: &std::path::Path) -> Result<()> {
    let conn_str = path.to_string_lossy().to_string();
    let conn = Connection::open_in_memory()
        .map_err(|e| AgentError::Database(format!("open: {e}")))?;
    conn.execute_batch("INSTALL sqlite; LOAD sqlite;")
        .map_err(|e| AgentError::Database(format!("load sqlite ext: {e}")))?;
    conn.execute_batch(&format!(
        "ATTACH '{conn_str}' AS _db (TYPE SQLITE, READ_ONLY);"
    ))
    .map_err(|e| AgentError::Database(format!("connection failed: {e}")))?;
    conn.execute_batch("USE _db; SELECT 1;")
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
            is_primary_key: is_pk == "1",
            default_value: None,
            comment: None,
        });
    }
    Ok(tables.into_iter().map(|(table_name, columns)| TableSchema {
        table_name, columns, row_count_estimate: None,
        size_bytes: None, indexes: vec![], foreign_keys: vec![],
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
            .map(|(name, data_type)| ColumnInfo { name, data_type, nullable: true, is_primary_key: false, default_value: None, comment: None })
            .collect();

        tables.push(TableSchema { table_name: coll_name, columns, row_count_estimate: None, size_bytes: None, indexes: vec![], foreign_keys: vec![] });
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

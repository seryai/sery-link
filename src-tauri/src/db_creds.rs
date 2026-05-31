//! F52 — Database connection config in ~/.seryai/.vault.json.
//!
//! Full connection config (host, port, username, database, password)
//! is stored per source in ONE vault entry (service = "sery-link-db",
//! account = <source_id>) as a JSON object. One entry per source means
//! one authorization prompt per source on platforms that prompt.
//!
//! An in-process CONN_CACHE avoids re-reading the vault on every query.

use crate::error::{AgentError, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

const SERVICE: &str = "sery-link-db";

/// Full connection configuration for a database source.
/// Stored in .vault.json via cred_store, keyed on source_id.
/// SQLite is excluded — it has no credentials, only a file path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DbConnectionConfig {
    Mysql {
        host: String,
        port: u16,
        username: String,
        database: String,
        password: String,
    },
    Postgresql {
        host: String,
        port: u16,
        username: String,
        database: String,
        password: String,
    },
    Snowflake {
        account: String,
        username: String,
        warehouse: String,
        database: String,
        schema: String,
        password: String,
    },
    Clickhouse {
        host: String,
        port: u16,
        username: String,
        database: String,
        password: String,
    },
    Mongodb {
        host: String,
        port: u16,
        username: String,
        database: String,
        auth_db: String,
        password: String,
    },
    Redis {
        host: String,
        port: u16,
        db: u8,
        password: String,
    },
}

static CONN_CACHE: Lazy<Mutex<HashMap<String, DbConnectionConfig>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn save_connection(source_id: &str, config: &DbConnectionConfig) -> Result<()> {
    let json = serde_json::to_string(config)
        .map_err(|e| AgentError::Config(format!("db_creds serialize: {e}")))?;
    let entry = crate::cred_store::Entry::new(SERVICE, source_id)
        .map_err(|e| AgentError::Config(format!("cred_store entry: {e}")))?;
    entry
        .set_password(&json)
        .map_err(|e| AgentError::Config(format!("cred_store write: {e}")))?;
    // Update in-process cache.
    let mut guard = CONN_CACHE.lock().expect("CONN_CACHE poisoned");
    guard.insert(source_id.to_string(), config.clone());
    Ok(())
}

pub fn load_connection(source_id: &str) -> Result<DbConnectionConfig> {
    // Check cache first.
    {
        let guard = CONN_CACHE.lock().expect("CONN_CACHE poisoned");
        if let Some(cfg) = guard.get(source_id) {
            return Ok(cfg.clone());
        }
    }
    // Cache miss — read from vault.
    let entry = crate::cred_store::Entry::new(SERVICE, source_id)
        .map_err(|e| AgentError::Config(format!("cred_store entry: {e}")))?;
    let json = match entry.get_password() {
        Ok(j) => j,
        Err(crate::cred_store::Error::NoEntry) => {
            return Err(AgentError::Database(format!(
                "No credentials for source {source_id} — please remove and re-add it."
            )));
        }
        Err(e) => return Err(AgentError::Config(format!("cred_store read: {e}"))),
    };
    let cfg: DbConnectionConfig = serde_json::from_str(&json)
        .map_err(|e| AgentError::Config(format!("db_creds parse: {e}")))?;
    // Populate cache.
    let mut guard = CONN_CACHE.lock().expect("CONN_CACHE poisoned");
    guard.insert(source_id.to_string(), cfg.clone());
    Ok(cfg)
}

pub fn delete_connection(source_id: &str) -> Result<()> {
    // Remove from cache.
    {
        let mut guard = CONN_CACHE.lock().expect("CONN_CACHE poisoned");
        guard.remove(source_id);
    }
    let entry = crate::cred_store::Entry::new(SERVICE, source_id)
        .map_err(|e| AgentError::Config(format!("cred_store entry: {e}")))?;
    match entry.delete_password() {
        Ok(()) | Err(crate::cred_store::Error::NoEntry) => Ok(()),
        Err(e) => Err(AgentError::Config(format!("cred_store delete: {e}"))),
    }
}

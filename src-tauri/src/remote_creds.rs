//! Credentials for remote data sources (Phase B — S3).
//!
//! Stores AWS access keys in the macOS Keychain (and OS equivalents on
//! other platforms) via the `keyring` crate, one entry per URL.
//! Credentials never touch disk unencrypted and never leave the user's
//! machine — the only consumer is DuckDB's httpfs extension, running
//! locally in the scanner / profile paths.
//!
//! Entry naming: `sery-link-s3` / `<url>`. Using the URL as the account
//! means one set of creds per source, which is simple but does mean
//! a user with many buckets under the same account has to paste keys
//! per source. We'll consolidate to credential "profiles" later if
//! that friction is real — for Phase B1, simpler wins.
//!
//! The stored payload is a serialised `S3Credentials` JSON. Fields:
//!   * access_key_id     — AWS access key (mandatory)
//!   * secret_access_key — AWS secret (mandatory)
//!   * region            — bucket region (mandatory, e.g. us-east-1)
//!   * session_token     — optional, for temporary STS credentials

use crate::error::{AgentError, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

const SERVICE: &str = "sery-link-s3";

// Process-wide cache: URL → creds. Same prompt-storm motivation as
// keyring_store: scanning a folder with multiple S3 sources used to
// trigger a keychain prompt per source per scan. Now one prompt per
// URL per launch. Save/delete invalidate the relevant entry.
static CRED_CACHE: Lazy<Mutex<HashMap<String, S3Credentials>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Credentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub region: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
}

impl S3Credentials {
    pub fn is_valid(&self) -> bool {
        !self.access_key_id.trim().is_empty()
            && !self.secret_access_key.trim().is_empty()
            && !self.region.trim().is_empty()
    }
}

/// Store S3 credentials keyed on a URL. Overwrites any existing entry
/// for the same URL. Called by `add_remote_source` when the URL is
/// `s3://…`.
pub fn save(url: &str, creds: &S3Credentials) -> Result<()> {
    if !creds.is_valid() {
        return Err(AgentError::Config(
            "S3 credentials need access key, secret, and region".to_string(),
        ));
    }
    let entry = keyring::Entry::new(SERVICE, url)
        .map_err(|e| AgentError::Config(format!("keyring entry: {}", e)))?;
    let json = serde_json::to_string(creds)
        .map_err(|e| AgentError::Serialization(format!("serialize creds: {}", e)))?;
    entry
        .set_password(&json)
        .map_err(|e| AgentError::Config(format!("keyring write: {}", e)))?;
    CRED_CACHE
        .lock()
        .expect("CRED_CACHE poisoned")
        .insert(url.to_string(), creds.clone());
    Ok(())
}

/// Load credentials for a URL. Returns `Ok(None)` when no entry exists
/// (the URL was added without creds, or creds were never saved) so
/// callers can decide between "prompt the user" and "error out" based
/// on context.
pub fn load(url: &str) -> Result<Option<S3Credentials>> {
    if let Some(cached) = CRED_CACHE
        .lock()
        .expect("CRED_CACHE poisoned")
        .get(url)
    {
        return Ok(Some(cached.clone()));
    }
    let entry = match keyring::Entry::new(SERVICE, url) {
        Ok(e) => e,
        Err(e) => return Err(AgentError::Config(format!("keyring entry: {}", e))),
    };
    match entry.get_password() {
        Ok(json) => {
            let creds: S3Credentials = serde_json::from_str(&json).map_err(|e| {
                AgentError::Serialization(format!("parse creds: {}", e))
            })?;
            CRED_CACHE
                .lock()
                .expect("CRED_CACHE poisoned")
                .insert(url.to_string(), creds.clone());
            Ok(Some(creds))
        }
        // The keyring crate returns NoEntry when no matching item exists;
        // treat anything else as a real error so permission issues get
        // surfaced rather than silently falling back.
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AgentError::Config(format!("keyring read: {}", e))),
    }
}

/// Delete credentials for a URL. Used when the user removes a remote
/// source so we don't leave orphan keyring entries behind. Deleting a
/// non-existent entry is treated as success — idempotent by design.
pub fn delete(url: &str) -> Result<()> {
    let entry = match keyring::Entry::new(SERVICE, url) {
        Ok(e) => e,
        Err(e) => return Err(AgentError::Config(format!("keyring entry: {}", e))),
    };
    let result = match entry.delete_password() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AgentError::Config(format!("keyring delete: {}", e))),
    };
    CRED_CACHE
        .lock()
        .expect("CRED_CACHE poisoned")
        .remove(url);
    result
}

/// Produce the DuckDB statements that configure the current connection
/// to use these credentials for S3 access. Caller is responsible for
/// running `INSTALL httpfs; LOAD httpfs;` first — this function only
/// handles the credential-setting part.
///
/// Using `SET` (the legacy per-connection syntax) rather than
/// `CREATE SECRET` because:
///   * Every scanner/profile call opens a fresh in-memory DuckDB, so
///     per-connection scoping is already correct.
///   * SET is supported across all DuckDB 1.x; the SECRET syntax
///     landed in 1.1 but semantics shifted between point releases.
///   * One statement per key, trivially composable.
pub fn duckdb_setters(creds: &S3Credentials) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "SET s3_region='{}';\n",
        escape_sql(&creds.region)
    ));
    out.push_str(&format!(
        "SET s3_access_key_id='{}';\n",
        escape_sql(&creds.access_key_id)
    ));
    out.push_str(&format!(
        "SET s3_secret_access_key='{}';\n",
        escape_sql(&creds.secret_access_key)
    ));
    if let Some(token) = creds.session_token.as_deref() {
        if !token.is_empty() {
            out.push_str(&format!(
                "SET s3_session_token='{}';\n",
                escape_sql(token)
            ));
        }
    }
    out
}

fn escape_sql(s: &str) -> String {
    s.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_requires_all_mandatory_fields() {
        let base = S3Credentials {
            access_key_id: "AKIA".to_string(),
            secret_access_key: "secret".to_string(),
            region: "us-east-1".to_string(),
            session_token: None,
        };
        assert!(base.is_valid());

        let mut missing_key = base.clone();
        missing_key.access_key_id = String::new();
        assert!(!missing_key.is_valid());

        let mut whitespace_secret = base.clone();
        whitespace_secret.secret_access_key = "  ".to_string();
        assert!(!whitespace_secret.is_valid());

        let mut missing_region = base.clone();
        missing_region.region = String::new();
        assert!(!missing_region.is_valid());
    }

    #[test]
    fn duckdb_setters_produces_three_statements_without_token() {
        let creds = S3Credentials {
            access_key_id: "AKIA".to_string(),
            secret_access_key: "secret".to_string(),
            region: "us-east-1".to_string(),
            session_token: None,
        };
        let sql = duckdb_setters(&creds);
        assert!(sql.contains("SET s3_region='us-east-1'"));
        assert!(sql.contains("SET s3_access_key_id='AKIA'"));
        assert!(sql.contains("SET s3_secret_access_key='secret'"));
        assert!(!sql.contains("s3_session_token"));
    }

    #[test]
    fn duckdb_setters_includes_session_token_when_present() {
        let creds = S3Credentials {
            access_key_id: "AKIA".to_string(),
            secret_access_key: "secret".to_string(),
            region: "us-east-1".to_string(),
            session_token: Some("FwoGZ...".to_string()),
        };
        let sql = duckdb_setters(&creds);
        assert!(sql.contains("SET s3_session_token='FwoGZ...'"));
    }

    #[test]
    fn duckdb_setters_escapes_single_quotes() {
        // Pathological credential with a literal quote. Must not produce
        // a malformed statement that could be SQL-injection-like on the
        // user's own DuckDB — defense in depth even though creds come
        // from a trusted source (the user themselves).
        let creds = S3Credentials {
            access_key_id: "AKIA'injected".to_string(),
            secret_access_key: "secret".to_string(),
            region: "us-east-1".to_string(),
            session_token: None,
        };
        let sql = duckdb_setters(&creds);
        assert!(sql.contains("SET s3_access_key_id='AKIA''injected'"));
    }
}

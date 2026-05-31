use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use futures::StreamExt;
use mysql_async::consts::ColumnType;
use mysql_async::prelude::*;
use percent_encoding::percent_decode_str;
use rust_decimal::Decimal;
use std::borrow::Cow;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use crate::ssh_tunnel::SshTunnelConfig;
use crate::types::{ColumnInfo, QueryResult, TableInfo};

pub type MySqlPool = mysql_async::Pool;

pub const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;
pub const MAX_ROWS: usize = 100_000;

pub struct MySqlConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub database: String,
    pub password: String,
    pub ssl_mode: Option<String>,
    pub ssl_ca_cert: Option<String>,
    pub ssh: Option<SshTunnelConfig>,
}

impl MySqlConfig {
    /// Build a mysql:// URL from the structured fields.
    pub fn to_url(&self) -> String {
        let password_enc = percent_encoding::utf8_percent_encode(
            &self.password,
            percent_encoding::NON_ALPHANUMERIC,
        )
        .to_string();
        let username_enc = percent_encoding::utf8_percent_encode(
            &self.username,
            percent_encoding::NON_ALPHANUMERIC,
        )
        .to_string();
        let db_enc = percent_encoding::utf8_percent_encode(
            &self.database,
            percent_encoding::NON_ALPHANUMERIC,
        )
        .to_string();

        let (host, port) = self.effective_host_port();
        let mut url = format!("mysql://{}:{}@{}:{}/{}", username_enc, password_enc, host, port, db_enc);

        let mut params = Vec::new();
        if let Some(ssl_mode) = &self.ssl_mode {
            params.push(format!("ssl-mode={}", ssl_mode));
        }
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }
        url
    }

    fn effective_host_port(&self) -> (String, u16) {
        // SSH tunnel redirects to 127.0.0.1:<local_port> — callers must set
        // host/port to the tunnel endpoint before calling to_url().
        (self.host.clone(), self.port)
    }
}

// ─── helper utilities (inlined from dbx db/mod.rs) ───────────────────────────

const JS_MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

fn safe_i64_to_json(v: i64) -> serde_json::Value {
    if !(-JS_MAX_SAFE_INTEGER..=JS_MAX_SAFE_INTEGER).contains(&v) {
        serde_json::Value::String(v.to_string())
    } else {
        serde_json::Value::Number(v.into())
    }
}

fn safe_u64_to_json(v: u64) -> serde_json::Value {
    if v > JS_MAX_SAFE_INTEGER as u64 {
        serde_json::Value::String(v.to_string())
    } else {
        serde_json::Value::Number(v.into())
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn binary_value_to_json(bytes: &[u8]) -> serde_json::Value {
    serde_json::Value::String(format!("0x{}", hex_encode(bytes)))
}

fn starts_with_executable_sql_keyword(sql: &str, keywords: &[&str]) -> bool {
    let trimmed = sql.trim_start();
    // Skip leading block comments /* ... */
    let trimmed = {
        let mut s = trimmed;
        while s.starts_with("/*") {
            if let Some(end) = s.find("*/") {
                s = s[end + 2..].trim_start();
            } else {
                break;
            }
        }
        // Skip leading line comments -- ...
        while s.starts_with("--") {
            if let Some(end) = s.find('\n') {
                s = s[end + 1..].trim_start();
            } else {
                break;
            }
        }
        s
    };
    for kw in keywords {
        let kw_len = kw.len();
        if trimmed.len() >= kw_len {
            let prefix = &trimmed[..kw_len];
            if prefix.eq_ignore_ascii_case(kw) {
                let after = &trimmed[kw_len..];
                if after.is_empty() || after.starts_with(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
                    return true;
                }
            }
        }
    }
    false
}

// ─── internal helpers ─────────────────────────────────────────────────────────

fn quote_value(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn quote_identifier(s: &str) -> String {
    format!("`{}`", s.replace('`', "``"))
}

fn row_get<T, I>(row: &mysql_async::Row, index: I) -> Option<T>
where
    T: mysql_async::prelude::FromValue,
    I: mysql_async::prelude::ColumnIndex,
{
    row.get_opt::<T, I>(index).and_then(|result| result.ok())
}

fn get_str(row: &mysql_async::Row, idx: usize) -> String {
    row_get::<String, _>(row, idx)
        .or_else(|| row_get::<Vec<u8>, _>(row, idx).map(|b| String::from_utf8_lossy(&b).to_string()))
        .unwrap_or_default()
}

#[allow(dead_code)]
fn get_str_by_name(row: &mysql_async::Row, name: &str) -> String {
    row_get::<String, _>(row, name)
        .or_else(|| row_get::<Vec<u8>, _>(row, name).map(|b| String::from_utf8_lossy(&b).to_string()))
        .unwrap_or_default()
}

#[allow(dead_code)]
fn get_opt_str(row: &mysql_async::Row, name: &str) -> Option<String> {
    row_get::<String, _>(row, name)
        .or_else(|| row_get::<Vec<u8>, _>(row, name).map(|b| String::from_utf8_lossy(&b).to_string()))
}

fn is_lossless_integer_column(column: &mysql_async::Column) -> bool {
    matches!(column.column_type(), ColumnType::MYSQL_TYPE_LONGLONG | ColumnType::MYSQL_TYPE_NEWDECIMAL)
}

fn is_mysql_binary_charset(column: &mysql_async::Column) -> bool {
    column.character_set() == 63
}

fn is_mysql_blob_column(column: &mysql_async::Column) -> bool {
    is_mysql_binary_charset(column)
        && matches!(
            column.column_type(),
            ColumnType::MYSQL_TYPE_BLOB
                | ColumnType::MYSQL_TYPE_LONG_BLOB
                | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
                | ColumnType::MYSQL_TYPE_TINY_BLOB
        )
}

fn is_mysql_binary_string_column(column: &mysql_async::Column) -> bool {
    is_mysql_binary_charset(column)
        && matches!(
            column.column_type(),
            ColumnType::MYSQL_TYPE_STRING | ColumnType::MYSQL_TYPE_VAR_STRING | ColumnType::MYSQL_TYPE_VARCHAR
        )
}

fn mysql_printable_binary_preview(bytes: &[u8]) -> Option<String> {
    let trimmed = bytes.strip_suffix(&[0]).map_or(bytes, |mut value| {
        while let Some(rest) = value.strip_suffix(&[0]) {
            value = rest;
        }
        value
    });
    if trimmed.is_empty() {
        return Some(String::new());
    }
    let text = std::str::from_utf8(trimmed).ok()?;
    text.chars().all(|ch| !ch.is_control() || matches!(ch, '\t' | '\n' | '\r')).then(|| text.to_string())
}

fn mysql_blob_preview(bytes: &[u8], label: &str) -> serde_json::Value {
    serde_json::Value::String(format!("({label}) {} bytes", bytes.len()))
}

fn mysql_bit_value_to_string(bytes: &[u8], column: &mysql_async::Column) -> String {
    let bit_len = column.column_length();
    if bit_len > 1 {
        let total_bits = bytes.len() * 8;
        let mut bits = String::with_capacity(total_bits);
        for byte in bytes {
            bits.push_str(&format!("{byte:08b}"));
        }
        let start = bits.len().saturating_sub(bit_len as usize);
        return bits[start..].to_string();
    }
    let val = bytes.iter().fold(0u64, |acc, &b| (acc << 8) | b as u64);
    val.to_string()
}

fn mysql_bytes_to_json(bytes: Vec<u8>, column: &mysql_async::Column) -> serde_json::Value {
    if is_mysql_blob_column(column) {
        return mysql_blob_preview(&bytes, "BLOB");
    }
    if is_mysql_binary_string_column(column) {
        return mysql_printable_binary_preview(&bytes)
            .map(serde_json::Value::String)
            .unwrap_or_else(|| binary_value_to_json(&bytes));
    }
    serde_json::Value::String(String::from_utf8_lossy(&bytes).to_string())
}

fn mysql_value_to_json(row: &mysql_async::Row, idx: usize) -> serde_json::Value {
    let Some(column) = row.columns_ref().get(idx) else {
        return serde_json::Value::Null;
    };

    let Some(value) = row.as_ref(idx) else {
        return serde_json::Value::Null;
    };
    if matches!(value, mysql_async::Value::NULL) {
        return serde_json::Value::Null;
    }

    if is_mysql_binary_string_column(column) {
        return row_get::<Vec<u8>, _>(row, idx)
            .map(|bytes| mysql_bytes_to_json(bytes, column))
            .unwrap_or(serde_json::Value::Null);
    }

    match column.column_type() {
        ColumnType::MYSQL_TYPE_JSON => {
            if let Some(v) = row_get::<String, _>(row, idx) {
                return serde_json::Value::String(v);
            }
        }
        ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL | ColumnType::MYSQL_TYPE_LONGLONG => {
            if is_lossless_integer_column(column) {
                return row
                    .get_opt::<String, usize>(idx)
                    .and_then(|result| result.ok())
                    .map(serde_json::Value::String)
                    .or_else(|| {
                        row_get::<Decimal, _>(row, idx).map(|v: Decimal| serde_json::Value::String(v.to_string()))
                    })
                    .or_else(|| row_get::<i64, _>(row, idx).map(|v| serde_json::Value::String(v.to_string())))
                    .or_else(|| row_get::<u64, _>(row, idx).map(|v| serde_json::Value::String(v.to_string())))
                    .or_else(|| row_get::<Vec<u8>, _>(row, idx).map(|bytes| mysql_bytes_to_json(bytes, column)))
                    .unwrap_or(serde_json::Value::Null);
            }
            return row
                .get_opt::<Decimal, usize>(idx)
                .and_then(|result| result.ok())
                .map(|v: Decimal| serde_json::Value::String(v.to_string()))
                .unwrap_or(serde_json::Value::Null);
        }
        ColumnType::MYSQL_TYPE_BIT => {
            return row_get::<Vec<u8>, _>(row, idx)
                .map(|bytes| serde_json::Value::String(mysql_bit_value_to_string(&bytes, column)))
                .unwrap_or(serde_json::Value::Null);
        }
        ColumnType::MYSQL_TYPE_BLOB
        | ColumnType::MYSQL_TYPE_LONG_BLOB
        | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
        | ColumnType::MYSQL_TYPE_TINY_BLOB
        | ColumnType::MYSQL_TYPE_GEOMETRY => {
            return row_get::<Vec<u8>, _>(row, idx)
                .map(|bytes| {
                    if matches!(column.column_type(), ColumnType::MYSQL_TYPE_GEOMETRY) {
                        mysql_blob_preview(&bytes, "GEOMETRY")
                    } else {
                        mysql_bytes_to_json(bytes, column)
                    }
                })
                .unwrap_or(serde_json::Value::Null);
        }
        ColumnType::MYSQL_TYPE_TIMESTAMP
        | ColumnType::MYSQL_TYPE_TIMESTAMP2
        | ColumnType::MYSQL_TYPE_DATETIME
        | ColumnType::MYSQL_TYPE_DATETIME2
        | ColumnType::MYSQL_TYPE_DATE
        | ColumnType::MYSQL_TYPE_TIME
        | ColumnType::MYSQL_TYPE_TIME2
        | ColumnType::MYSQL_TYPE_NEWDATE => {
            if let Some(v) = row_get::<NaiveDateTime, _>(row, idx) {
                return serde_json::Value::String(v.to_string());
            }
            if let Some(v) = row_get::<NaiveDate, _>(row, idx) {
                return serde_json::Value::String(v.to_string());
            }
            if let Some(v) = row_get::<NaiveTime, _>(row, idx) {
                return serde_json::Value::String(v.to_string());
            }
        }
        _ => {}
    }

    row_get::<String, _>(row, idx)
        .map(serde_json::Value::String)
        .or_else(|| row_get::<i64, _>(row, idx).map(safe_i64_to_json))
        .or_else(|| row_get::<u64, _>(row, idx).map(safe_u64_to_json))
        .or_else(|| row_get::<i32, _>(row, idx).map(|v| serde_json::Value::Number(v.into())))
        .or_else(|| row_get::<i16, _>(row, idx).map(|v| serde_json::Value::Number(v.into())))
        .or_else(|| {
            row_get::<f64, _>(row, idx).map(|v| {
                serde_json::Number::from_f64(v).map(serde_json::Value::Number).unwrap_or(serde_json::Value::Null)
            })
        })
        .or_else(|| row_get::<bool, _>(row, idx).map(serde_json::Value::Bool))
        .or_else(|| row_get::<Vec<u8>, _>(row, idx).map(|bytes| mysql_bytes_to_json(bytes, column)))
        .unwrap_or(serde_json::Value::Null)
}

// ─── TLS / URL helpers ────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct MySqlTlsFiles {
    sslcert: Option<String>,
    sslkey: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MySqlTlsUrl {
    url: String,
    files: MySqlTlsFiles,
}

fn mysql_tls_url(url: &str) -> Result<MySqlTlsUrl, String> {
    let Some(query_start) = url.find('?') else {
        return Ok(MySqlTlsUrl { url: url.to_string(), files: MySqlTlsFiles::default() });
    };

    let prefix = &url[..query_start];
    let suffix = &url[query_start + 1..];
    let (query_string, fragment) = suffix.split_once('#').map_or((suffix, ""), |(query, fragment)| (query, fragment));
    let mut files = MySqlTlsFiles::default();
    let mut kept_params = Vec::new();

    for param in query_string.split('&') {
        if param.is_empty() {
            continue;
        }
        let Some((key, value)) = param.split_once('=') else {
            kept_params.push(param.to_string());
            continue;
        };
        if mysql_tls_file_param_is(key, "cert") || mysql_tls_file_param_is(key, "key") {
            let decoded = percent_decode_str(value)
                .decode_utf8()
                .map_err(|_| format!("Invalid URL encoding in {key}"))?
                .into_owned();
            if mysql_tls_file_param_is(key, "cert") {
                files.sslcert = Some(decoded);
            } else {
                files.sslkey = Some(decoded);
            }
        } else {
            kept_params.push(param.to_string());
        }
    }

    let mut sanitized_url = prefix.to_string();
    if !kept_params.is_empty() {
        sanitized_url.push('?');
        sanitized_url.push_str(&kept_params.join("&"));
    }
    if !fragment.is_empty() {
        sanitized_url.push('#');
        sanitized_url.push_str(fragment);
    }

    Ok(MySqlTlsUrl { url: sanitized_url, files })
}

fn mysql_tls_file_param_is(key: &str, target: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace(['-', '_'], "");
    normalized == format!("ssl{target}")
}

fn mysql_ssl_opts(
    base_ssl_opts: Option<mysql_async::SslOpts>,
    url: &str,
    ca_cert_path: Option<&str>,
    files: &MySqlTlsFiles,
) -> Result<Option<mysql_async::SslOpts>, String> {
    let ca_cert_path = ca_cert_path.map(str::trim).filter(|path| !path.is_empty());
    let has_client_identity = files.sslcert.as_deref().is_some() || files.sslkey.as_deref().is_some();
    if !mysql_url_requires_ssl(url) && !has_client_identity {
        return Ok(None);
    }

    let mut ssl_opts = base_ssl_opts.unwrap_or_default();
    if let Some(ca_cert_path) = ca_cert_path.filter(|_| mysql_url_requires_ssl(url) || has_client_identity) {
        ssl_opts = ssl_opts.with_root_certs(vec![PathBuf::from(ca_cert_path).into()]);
        if !mysql_url_verifies_identity(url) {
            ssl_opts = ssl_opts.with_danger_skip_domain_validation(true);
        }
    }

    match (files.sslcert.as_deref(), files.sslkey.as_deref()) {
        (Some(cert_path), Some(key_path)) => {
            ssl_opts = ssl_opts.with_client_identity(Some(mysql_async::ClientIdentity::new(
                PathBuf::from(cert_path).into(),
                PathBuf::from(key_path).into(),
            )));
        }
        (Some(_), None) => return Err("MySQL ssl-cert requires ssl-key".to_string()),
        (None, Some(_)) => return Err("MySQL ssl-key requires ssl-cert".to_string()),
        (None, None) => {}
    }

    Ok(Some(ssl_opts))
}

fn mysql_url_requires_ssl(url: &str) -> bool {
    let Some((_, query)) = url.split_once('?') else { return false; };
    query.split('&').any(|segment| {
        let Some((key, value)) = segment.split_once('=') else { return false; };
        let key = key.trim();
        let value = value.trim();
        (key.eq_ignore_ascii_case("require_ssl") && value.eq_ignore_ascii_case("true"))
            || mysql_tls_file_param_is(key, "cert")
            || mysql_tls_file_param_is(key, "key")
            || ((key.eq_ignore_ascii_case("ssl-mode") || key.eq_ignore_ascii_case("sslmode"))
                && matches!(
                    value.to_ascii_lowercase().replace('-', "_").as_str(),
                    "required" | "require" | "verify_ca" | "verify_identity"
                ))
    })
}

fn mysql_url_verifies_identity(url: &str) -> bool {
    let Some((_, query)) = url.split_once('?') else { return false; };
    query.split('&').any(|segment| {
        let Some((key, value)) = segment.split_once('=') else { return false; };
        let key = key.trim();
        let value = value.trim();
        (key.eq_ignore_ascii_case("verify_identity") && value.eq_ignore_ascii_case("true"))
            || ((key.eq_ignore_ascii_case("ssl-mode") || key.eq_ignore_ascii_case("sslmode"))
                && matches!(value.to_ascii_lowercase().replace('-', "_").as_str(), "verify_identity"))
    })
}

fn is_jdbc_param(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "useunicode"
            | "characterencoding"
            | "zerodatetimebehavior"
            | "usessl"
            | "servertimezone"
            | "allowpublickeyretrieval"
            | "autoreconnect"
            | "maxreconnects"
            | "uselegacydatetimecode"
            | "usecompression"
            | "cacheprepstmts"
            | "useserverprepstmts"
            | "useconfigs"
            | "usecursorfetch"
            | "defaultfetchsize"
            | "usejdbccomplianttimezoneshift"
            | "usesspscompatibletimezoneshift"
            | "failoverreadonly"
            | "maxallowedpacket"
            | "tinyint1isbit"
            | "transformedbitisboolean"
            | "yearisdatetype"
            | "createdatabaseifnotexist"
            | "noaccesstoprocedurebodies"
            | "nullcatalogmeanscurrent"
            | "nullnamepatternmatchesall"
            | "dumponqueriesexception"
            | "enablequerytimeouts"
            | "useinformationschema"
            | "gatherperfmetrics"
            | "reportmetricsintervalmillis"
            | "maxquerysizetolog"
            | "packetdebugbuffersize"
            | "usenanosforelapsedtime"
            | "slowquerythresholdmillis"
            | "autoslowlog"
            | "explainslowqueries"
            | "resultsetsizethreshold"
            | "nettimeoutforstreamingresults"
            | "useusageadvisor"
    )
}

fn mysql_async_url(url: &str) -> Cow<'_, str> {
    let Some((base, query)) = url.split_once('?') else { return Cow::Borrowed(url); };

    let original_count = query.split('&').filter(|segment| !segment.trim().is_empty()).count();
    let mut filtered: Vec<String> = Vec::new();
    let mut changed = false;
    for segment in query.split('&') {
        let segment = segment.trim();
        if segment.is_empty()
            || segment.starts_with("charset=")
            || segment.starts_with("time_zone=")
            || segment.starts_with("time-zone=")
            || segment.to_ascii_lowercase().starts_with("connect_timeout=")
            || segment.to_ascii_lowercase().starts_with("connecttimeout=")
        {
            changed = true;
            continue;
        }

        let Some((key, value)) = segment.split_once('=') else {
            filtered.push(segment.to_string());
            continue;
        };
        if key.eq_ignore_ascii_case("ssl-mode") || key.eq_ignore_ascii_case("sslmode") {
            changed = true;
            match value.to_ascii_lowercase().replace('-', "_").as_str() {
                "disabled" | "disable" => filtered.push("require_ssl=false".to_string()),
                "required" | "require" => {
                    filtered.push("require_ssl=true".to_string());
                    filtered.push("verify_ca=false".to_string());
                    filtered.push("verify_identity=false".to_string());
                }
                "verify_ca" => {
                    filtered.push("require_ssl=true".to_string());
                    filtered.push("verify_identity=false".to_string());
                }
                "verify_identity" => filtered.push("require_ssl=true".to_string()),
                _ => {}
            }
            continue;
        }
        if is_jdbc_param(key) {
            changed = true;
            continue;
        }
        filtered.push(segment.to_string());
    }

    if !changed && filtered.len() == original_count {
        Cow::Borrowed(url)
    } else if filtered.is_empty() {
        Cow::Owned(base.to_string())
    } else {
        Cow::Owned(format!("{base}?{}", filtered.join("&")))
    }
}

fn mysql_connection_charset(url: &str) -> Option<&str> {
    let (_, query) = url.split_once('?')?;
    query.split('&').find_map(|segment| {
        let (key, value) = segment.split_once('=')?;
        if !key.eq_ignore_ascii_case("charset") {
            return None;
        }
        let value = value.trim();
        is_safe_mysql_charset_name(value).then_some(value)
    })
}

fn mysql_connection_database(url: &str) -> Option<String> {
    let rest = url.strip_prefix("mysql://")?;
    let (_, path_and_query) = rest.split_once('/')?;
    let path = path_and_query.split(['?', '#']).next().unwrap_or(path_and_query);
    let database = path.trim_start_matches('/').split('/').next().unwrap_or("").trim();
    if database.is_empty() {
        return None;
    }
    percent_decode_str(database).decode_utf8().ok().map(|value| value.into_owned())
}

fn is_safe_mysql_charset_name(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn mysql_setup_queries(url: &str) -> Vec<String> {
    let charset = mysql_connection_charset(url).unwrap_or("utf8mb4");
    let mut queries = Vec::new();
    if let Some(database) = mysql_connection_database(url) {
        queries.push(format!("USE {}", quote_identifier(&database)));
    }
    queries.push(format!("SET NAMES {charset}"));
    queries
}

fn mysql_error_should_retry_without_ssl(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("handshakefailure")
        || error.contains("handshake")
        || error.contains("tls connection")
        || error.contains("server closed session")
}

fn mysql_error_should_retry_with_text_protocol(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    (lower.contains("1105") && lower.contains("hy000"))
        || lower.contains("com_stmt_prepare")
        || lower.contains("prepared statement protocol")
        || lower.contains("this command is not supported in the prepared statement protocol yet")
}

fn ssl_fallback_url(url: &str) -> Option<String> {
    if mysql_url_requires_ssl(url) {
        return None;
    }
    if url.contains("ssl-mode=preferred") {
        Some(url.replace("ssl-mode=preferred", "ssl-mode=disabled"))
    } else if !url.contains("ssl-mode=") {
        let sep = if url.contains('?') { "&" } else { "?" };
        Some(format!("{url}{sep}ssl-mode=disabled"))
    } else {
        None
    }
}

// ─── pool creation ────────────────────────────────────────────────────────────

fn create_pool_from_url(url: &str, ca_cert_path: Option<&str>) -> Result<MySqlPool, String> {
    let tls_url = mysql_tls_url(url)?;
    let opts =
        mysql_async::Opts::from_url(&mysql_async_url(&tls_url.url)).map_err(|e| format!("Invalid MySQL URL: {e}"))?;
    let base_ssl_opts = opts.ssl_opts().cloned();
    let pool_opts = mysql_async::PoolOpts::new()
        .with_constraints(mysql_async::PoolConstraints::new(1, 3).unwrap())
        .with_inactive_connection_ttl(Duration::from_secs(300));
    let mut builder = mysql_async::OptsBuilder::from_opts(opts)
        .stmt_cache_size(0)
        .prefer_socket(false)
        .pool_opts(Some(pool_opts))
        .setup(mysql_setup_queries(url));
    if let Some(ssl_opts) = mysql_ssl_opts(base_ssl_opts, url, ca_cert_path, &tls_url.files)? {
        builder = builder.ssl_opts(ssl_opts);
    }
    Ok(MySqlPool::new(builder))
}

async fn verify_pool_connection(pool: &MySqlPool, timeout: Duration) -> Result<(), String> {
    tokio::time::timeout(timeout, async {
        let mut conn = pool.get_conn().await.map_err(|e| format!("MySQL connection failed: {e}"))?;
        conn.ping().await.map_err(|e| format!("MySQL ping failed: {e}"))?;
        Ok(())
    })
    .await
    .map_err(|_| format!("MySQL connection timed out ({}s)", timeout.as_secs()))?
}

// ─── public API ───────────────────────────────────────────────────────────────

/// Create a connection pool from a MySqlConfig.
/// If `config.ssh` is set the caller must have already started the SSH tunnel
/// and updated `config.host`/`config.port` to the tunnel's local endpoint.
pub async fn create_pool(config: &MySqlConfig) -> Result<MySqlPool, String> {
    let url = config.to_url();
    let ca_cert_path = config.ssl_ca_cert.as_deref();
    let timeout = Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS);

    let pool = create_pool_from_url(&url, ca_cert_path)?;
    let result = verify_pool_connection(&pool, timeout).await;

    if let Err(ref e) = result {
        if mysql_error_should_retry_without_ssl(e) {
            if let Some(fallback_url) = ssl_fallback_url(&url) {
                log::info!("MySQL SSL handshake failed, retrying with ssl-mode=disabled");
                let fallback_pool = create_pool_from_url(&fallback_url, None)?;
                return match verify_pool_connection(&fallback_pool, timeout).await {
                    Ok(()) => Ok(fallback_pool),
                    Err(e) => Err(e),
                };
            }
        }
    }

    result.map(|_| pool)
}

fn is_result_set_query(sql: &str) -> bool {
    starts_with_executable_sql_keyword(sql, &["SELECT", "SHOW", "DESCRIBE", "EXPLAIN", "WITH"])
}

fn requires_text_protocol_query(sql: &str) -> bool {
    if !starts_with_executable_sql_keyword(sql, &["SHOW"]) {
        return false;
    }
    let tokens =
        sql.trim().trim_end_matches(';').split_whitespace().map(|token| token.to_ascii_lowercase()).collect::<Vec<_>>();
    if tokens.len() >= 2 && tokens[0] == "show" && tokens[1] == "grants" {
        return true;
    }
    matches!(
        tokens.iter().map(String::as_str).collect::<Vec<_>>().as_slice(),
        ["show", "processlist"]
            | ["show", "full", "processlist"]
            | ["show", "slave", "status"]
            | ["show", "replica", "status"]
    )
}

async fn get_conn_with_health_check(pool: &MySqlPool) -> Result<mysql_async::Conn, String> {
    let mut conn = pool.get_conn().await.map_err(|e| e.to_string())?;
    match conn.ping().await {
        Ok(()) => Ok(conn),
        Err(_) => {
            let _ = conn.disconnect().await;
            pool.get_conn().await.map_err(|e| e.to_string())
        }
    }
}

async fn execute_result_set_with_text_protocol_on_conn(
    conn: &mut mysql_async::Conn,
    sql: &str,
    row_limit: usize,
    start: Instant,
) -> Result<QueryResult, String> {
    let mut result = conn.query_iter(sql).await.map_err(|e| e.to_string())?;
    let columns: Vec<String> = result.columns_ref().iter().map(|c| c.name_str().to_string()).collect();

    let mut result_rows: Vec<Vec<serde_json::Value>> = Vec::new();
    let mut stream = result
        .stream::<mysql_async::Row>()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Empty result set stream".to_string())?;

    while let Some(row) = stream.next().await {
        let row = row.map_err(|e| e.to_string())?;
        let values: Vec<serde_json::Value> = (0..row.len()).map(|i| mysql_value_to_json(&row, i)).collect();
        result_rows.push(values);
        if result_rows.len() > row_limit {
            break;
        }
    }

    let truncated = result_rows.len() > row_limit;
    if truncated {
        result_rows.truncate(row_limit);
    }

    Ok(QueryResult {
        columns,
        row_count: result_rows.len(),
        rows: result_rows,
        duration_ms: start.elapsed().as_millis() as u64,
        truncated,
    })
}

async fn execute_result_set_with_prepared_protocol_on_conn(
    conn: &mut mysql_async::Conn,
    sql: &str,
    row_limit: usize,
    start: Instant,
) -> Result<QueryResult, String> {
    let mut result = conn.exec_iter(sql, ()).await.map_err(|e| e.to_string())?;
    let columns: Vec<String> = result.columns_ref().iter().map(|c| c.name_str().to_string()).collect();

    let mut result_rows: Vec<Vec<serde_json::Value>> = Vec::new();
    let mut stream = result
        .stream::<mysql_async::Row>()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Empty result set stream".to_string())?;

    while let Some(row) = stream.next().await {
        let row = row.map_err(|e| e.to_string())?;
        let values: Vec<serde_json::Value> = (0..row.len()).map(|i| mysql_value_to_json(&row, i)).collect();
        result_rows.push(values);
        if result_rows.len() > row_limit {
            break;
        }
    }

    let truncated = result_rows.len() > row_limit;
    if truncated {
        result_rows.truncate(row_limit);
    }

    Ok(QueryResult {
        columns,
        row_count: result_rows.len(),
        rows: result_rows,
        duration_ms: start.elapsed().as_millis() as u64,
        truncated,
    })
}

/// Execute a query against the pool, returning up to `max_rows` rows.
pub async fn execute_query(pool: &MySqlPool, sql: &str, max_rows: usize) -> Result<QueryResult, String> {
    let start = Instant::now();
    let row_limit = max_rows.max(1);
    let mut conn = get_conn_with_health_check(pool).await?;

    if is_result_set_query(sql) {
        if requires_text_protocol_query(sql) {
            execute_result_set_with_text_protocol_on_conn(&mut conn, sql, row_limit, start).await
        } else {
            match execute_result_set_with_prepared_protocol_on_conn(&mut conn, sql, row_limit, start).await {
                Ok(result) => Ok(result),
                Err(err) if mysql_error_should_retry_with_text_protocol(&err) => {
                    execute_result_set_with_text_protocol_on_conn(&mut conn, sql, row_limit, start).await
                }
                Err(err) => Err(err),
            }
        }
    } else {
        let result = conn.query_iter(sql).await.map_err(|e| e.to_string())?;
        let _affected_rows = result.affected_rows();
        result.drop_result().await.map_err(|e| e.to_string())?;
        Ok(QueryResult {
            columns: vec![],
            row_count: 0,
            rows: vec![],
            duration_ms: start.elapsed().as_millis() as u64,
            truncated: false,
        })
    }
}

/// Introspect the schema of a database, returning one TableInfo per table/view.
pub async fn introspect_schema(pool: &MySqlPool, database: &str) -> Result<Vec<TableInfo>, String> {
    let columns_sql = format!(
        "SELECT c.TABLE_NAME, c.COLUMN_NAME, c.COLUMN_TYPE, c.IS_NULLABLE, c.COLUMN_DEFAULT, \
         CASE WHEN pk.COLUMN_NAME IS NOT NULL THEN 1 ELSE 0 END AS is_pk \
         FROM information_schema.COLUMNS c \
         LEFT JOIN information_schema.KEY_COLUMN_USAGE pk \
           ON pk.TABLE_SCHEMA = c.TABLE_SCHEMA \
           AND pk.TABLE_NAME = c.TABLE_NAME \
           AND pk.COLUMN_NAME = c.COLUMN_NAME \
           AND pk.CONSTRAINT_NAME = 'PRIMARY' \
         WHERE c.TABLE_SCHEMA = {} \
         ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION",
        quote_value(database),
    );

    let mut conn = pool.get_conn().await.map_err(|e| e.to_string())?;
    let result = conn.query_iter(&columns_sql).await.map_err(|e| e.to_string())?;
    let rows: Vec<mysql_async::Row> = result.collect_and_drop().await.map_err(|e| e.to_string())?;

    let mut tables: std::collections::BTreeMap<String, Vec<ColumnInfo>> = std::collections::BTreeMap::new();
    for row in &rows {
        let table_name = get_str(row, 0);
        let col_name = get_str(row, 1);
        let data_type = get_str(row, 2);
        let nullable = get_str(row, 3) == "YES";
        let default_value: Option<String> = row.get_opt::<Option<String>, _>(4).and_then(|r| r.ok()).flatten();
        let is_pk = row.get::<i32, &str>("is_pk").unwrap_or(0) == 1;

        tables.entry(table_name).or_default().push(ColumnInfo {
            name: col_name,
            data_type,
            nullable,
            is_primary_key: is_pk,
            default_value,
        });
    }

    // Row counts and sizes
    let stats_sql = format!(
        "SELECT TABLE_NAME, TABLE_ROWS, DATA_LENGTH + INDEX_LENGTH \
         FROM information_schema.TABLES \
         WHERE TABLE_SCHEMA = {} AND TABLE_TYPE = 'BASE TABLE'",
        quote_value(database),
    );
    let result = conn.query_iter(&stats_sql).await.map_err(|e| e.to_string())?;
    let stat_rows: Vec<mysql_async::Row> = result.collect_and_drop().await.map_err(|e| e.to_string())?;

    let mut row_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    let mut size_bytes_map: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for row in &stat_rows {
        let tbl = get_str(row, 0);
        let cnt: u64 = row.get_opt::<u64, _>(1).and_then(|r| r.ok()).unwrap_or(0);
        let sz: u64 = row.get_opt::<u64, _>(2).and_then(|r| r.ok()).unwrap_or(0);
        row_counts.insert(tbl.clone(), cnt);
        if sz > 0 {
            size_bytes_map.insert(tbl, sz);
        }
    }

    Ok(tables
        .into_iter()
        .map(|(table_name, columns)| {
            let row_count_estimate = row_counts.get(&table_name).copied();
            let size_bytes = size_bytes_map.get(&table_name).copied();
            TableInfo { table_name, columns, row_count_estimate, size_bytes }
        })
        .collect())
}

/// Test a connection without creating a persistent pool.
pub async fn test_connection(config: &MySqlConfig) -> Result<(), String> {
    let pool = create_pool(config).await?;
    let _ = pool.disconnect().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mysql_with_queries_are_treated_as_result_sets() {
        let sql = "WITH RECURSIVE org_tree AS (SELECT 1 AS id) SELECT id FROM org_tree";
        assert!(is_result_set_query(sql));
    }

    #[test]
    fn mysql_management_show_queries_use_text_protocol() {
        assert!(requires_text_protocol_query("SHOW PROCESSLIST"));
        assert!(requires_text_protocol_query("show full processlist"));
        assert!(requires_text_protocol_query("SHOW SLAVE STATUS"));
        assert!(requires_text_protocol_query("show replica status"));
        assert!(requires_text_protocol_query("SHOW GRANTS"));
        assert!(requires_text_protocol_query("SHOW GRANTS FOR 'repl'@'%'"));
        assert!(!requires_text_protocol_query("SHOW TABLES"));
        assert!(!requires_text_protocol_query("SELECT * FROM users"));
    }

    #[test]
    fn mysql_tls_session_close_errors_retry_without_ssl() {
        let error = "MySQL connection failed: error communicating with database: \
            encountered error while attempting to establish a TLS connection: \
            server closed session with no notification";
        assert!(mysql_error_should_retry_without_ssl(error));
    }

    #[test]
    fn mysql_ssl_fallback_disabled_for_required() {
        assert_eq!(ssl_fallback_url("mysql://host:3306/db?require_ssl=true"), None);
        assert_eq!(ssl_fallback_url("mysql://host:3306/db?ssl-mode=verify_ca"), None);
    }

    #[test]
    fn mysql_setup_default_charset() {
        let queries = mysql_setup_queries("mysql://root:secret@localhost:3306/app");
        assert!(queries.iter().any(|q| q.contains("utf8mb4")));
    }
}

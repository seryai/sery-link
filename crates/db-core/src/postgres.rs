use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use deadpool_postgres::{ManagerConfig, Pool, RecyclingMethod, Runtime};
use futures::StreamExt;
use percent_encoding::percent_decode_str;
use rust_decimal::Decimal;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::verify_server_cert_signed_by_trust_anchor;
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::server::ParsedCertificate;
use std::fs::File;
use std::io::BufReader;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_postgres::config::SslMode;
use tokio_postgres::types::{FromSql, Type};
use tokio_postgres::{Row, SimpleQueryMessage};

use crate::ssh_tunnel::SshTunnelConfig;
use crate::types::{ColumnInfo, QueryResult, TableInfo};

pub type PgPool = Pool;

pub const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;
pub const MAX_ROWS: usize = 100_000;

pub struct PgConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub database: String,
    pub password: String,
    pub ssl_mode: Option<String>,
    pub ssl_ca_cert: Option<String>,
    pub ssh: Option<SshTunnelConfig>,
}

impl PgConfig {
    /// Build a postgres:// URL from the structured fields.
    pub fn to_url(&self) -> String {
        let password_enc = percent_encoding::utf8_percent_encode(
            &self.password,
            percent_encoding::NON_ALPHANUMERIC,
        )
        .to_string();
        let (host, port) = (self.host.clone(), self.port);
        let mut url = format!(
            "postgres://{}:{}@{}:{}/{}",
            self.username, password_enc, host, port, self.database
        );
        let mut params: Vec<String> = Vec::new();
        if let Some(ssl_mode) = &self.ssl_mode {
            params.push(format!("sslmode={}", ssl_mode));
        }
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }
        url
    }
}

// ─── helper utilities ────────────────────────────────────────────────────────

const JS_MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

fn safe_i64_to_json(v: i64) -> serde_json::Value {
    if !(-JS_MAX_SAFE_INTEGER..=JS_MAX_SAFE_INTEGER).contains(&v) {
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
    let mut trimmed = sql.trim_start();
    loop {
        if trimmed.starts_with("/*") {
            if let Some(end) = trimmed.find("*/") {
                trimmed = trimmed[end + 2..].trim_start();
                continue;
            }
        }
        if trimmed.starts_with("--") {
            if let Some(end) = trimmed.find('\n') {
                trimmed = trimmed[end + 1..].trim_start();
                continue;
            }
        }
        break;
    }
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

fn pg_quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

// ─── value converters ────────────────────────────────────────────────────────

fn pg_temporal_to_json_value(row: &Row, idx: usize) -> Option<serde_json::Value> {
    if let Ok(v) = row.try_get::<_, DateTime<Utc>>(idx) {
        return Some(serde_json::Value::String(v.to_rfc3339()));
    }
    if let Ok(v) = row.try_get::<_, NaiveDateTime>(idx) {
        return Some(serde_json::Value::String(v.to_string()));
    }
    if let Ok(v) = row.try_get::<_, NaiveDate>(idx) {
        return Some(serde_json::Value::String(v.to_string()));
    }
    if let Ok(v) = row.try_get::<_, NaiveTime>(idx) {
        return Some(serde_json::Value::String(v.to_string()));
    }
    None
}

struct PgSystemU32(u32);

impl<'a> FromSql<'a> for PgSystemU32 {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        let bytes: [u8; 4] = raw.try_into().map_err(|_| "expected 4 bytes for PostgreSQL system u32")?;
        Ok(Self(u32::from_be_bytes(bytes)))
    }

    fn accepts(ty: &Type) -> bool {
        matches!(*ty, Type::XID | Type::CID)
    }
}

struct PgAnyString(String);

impl<'a> FromSql<'a> for PgAnyString {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        std::str::from_utf8(raw)
            .map(|s| PgAnyString(s.to_string()))
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Sync + Send>)
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}

fn pg_u32_number(v: u32) -> serde_json::Value {
    serde_json::Value::Number(serde_json::Number::from(v))
}

fn pg_system_u32_to_json(row: &Row, idx: usize) -> Option<serde_json::Value> {
    if let Ok(v) = row.try_get::<_, u32>(idx) {
        return Some(pg_u32_number(v));
    }
    row.try_get::<_, PgSystemU32>(idx).ok().map(|v| pg_u32_number(v.0))
}

fn pg_optional_array_to_json<T>(
    values: Vec<Option<T>>,
    map_value: impl Fn(T) -> serde_json::Value,
) -> serde_json::Value {
    serde_json::Value::Array(
        values.into_iter().map(|value| value.map(&map_value).unwrap_or(serde_json::Value::Null)).collect(),
    )
}

fn pg_float_number(v: f64) -> serde_json::Value {
    serde_json::Number::from_f64(v).map(serde_json::Value::Number).unwrap_or(serde_json::Value::Null)
}

fn pg_array_to_json_value(row: &Row, idx: usize) -> Option<serde_json::Value> {
    if let Ok(values) = row.try_get::<_, Vec<Option<String>>>(idx) {
        return Some(pg_optional_array_to_json(values, serde_json::Value::String));
    }
    if let Ok(values) = row.try_get::<_, Vec<Option<bool>>>(idx) {
        return Some(pg_optional_array_to_json(values, serde_json::Value::Bool));
    }
    if let Ok(values) = row.try_get::<_, Vec<Option<Decimal>>>(idx) {
        return Some(pg_optional_array_to_json(values, |v| serde_json::Value::String(v.to_string())));
    }
    if let Ok(values) = row.try_get::<_, Vec<Option<DateTime<Utc>>>>(idx) {
        return Some(pg_optional_array_to_json(values, |v| serde_json::Value::String(v.to_rfc3339())));
    }
    if let Ok(values) = row.try_get::<_, Vec<Option<NaiveDateTime>>>(idx) {
        return Some(pg_optional_array_to_json(values, |v| serde_json::Value::String(v.to_string())));
    }
    if let Ok(values) = row.try_get::<_, Vec<Option<NaiveDate>>>(idx) {
        return Some(pg_optional_array_to_json(values, |v| serde_json::Value::String(v.to_string())));
    }
    if let Ok(values) = row.try_get::<_, Vec<Option<NaiveTime>>>(idx) {
        return Some(pg_optional_array_to_json(values, |v| serde_json::Value::String(v.to_string())));
    }
    if let Ok(values) = row.try_get::<_, Vec<Option<u32>>>(idx) {
        return Some(pg_optional_array_to_json(values, pg_u32_number));
    }
    if let Ok(values) = row.try_get::<_, Vec<Option<i8>>>(idx) {
        return Some(pg_optional_array_to_json(values, |v| serde_json::Value::Number(v.into())));
    }
    if let Ok(values) = row.try_get::<_, Vec<Option<i16>>>(idx) {
        return Some(pg_optional_array_to_json(values, |v| serde_json::Value::Number(v.into())));
    }
    if let Ok(values) = row.try_get::<_, Vec<Option<i32>>>(idx) {
        return Some(pg_optional_array_to_json(values, |v| serde_json::Value::Number(v.into())));
    }
    if let Ok(values) = row.try_get::<_, Vec<Option<i64>>>(idx) {
        return Some(pg_optional_array_to_json(values, safe_i64_to_json));
    }
    if let Ok(values) = row.try_get::<_, Vec<Option<f32>>>(idx) {
        return Some(pg_optional_array_to_json(values, |v| pg_float_number(v as f64)));
    }
    if let Ok(values) = row.try_get::<_, Vec<Option<f64>>>(idx) {
        return Some(pg_optional_array_to_json(values, pg_float_number));
    }
    if let Ok(values) = row.try_get::<_, Vec<Option<PgAnyString>>>(idx) {
        return Some(pg_optional_array_to_json(values, |v| serde_json::Value::String(v.0)));
    }
    None
}

fn pg_value_to_json(row: &Row, idx: usize, type_name: &str) -> serde_json::Value {
    let upper = type_name.to_uppercase();

    if upper == "BYTEA" {
        return row
            .try_get::<_, Vec<u8>>(idx)
            .map(|bytes| binary_value_to_json(&bytes))
            .unwrap_or(serde_json::Value::Null);
    }

    if upper == "JSON" || upper == "JSONB" {
        if let Ok(v) = row.try_get::<_, serde_json::Value>(idx) {
            return serde_json::Value::String(v.to_string());
        }
        if let Ok(v) = row.try_get::<_, String>(idx) {
            return serde_json::Value::String(v);
        }
        return serde_json::Value::Null;
    }

    if upper == "BOOL" {
        return row.try_get::<_, bool>(idx).map(serde_json::Value::Bool).unwrap_or(serde_json::Value::Null);
    }

    if upper.contains("TIMESTAMP")
        || upper == "DATE"
        || upper == "TIME"
        || upper == "TIMETZ"
        || upper.contains("INTERVAL")
    {
        if let Some(v) = pg_temporal_to_json_value(row, idx) {
            return v;
        }
    }

    if upper == "NUMERIC" || upper == "DECIMAL" || upper == "MONEY" {
        return row
            .try_get::<_, Decimal>(idx)
            .map(|v: Decimal| serde_json::Value::String(v.to_string()))
            .unwrap_or(serde_json::Value::Null);
    }

    if matches!(upper.as_str(), "OID" | "XID" | "CID") {
        return pg_system_u32_to_json(row, idx).unwrap_or(serde_json::Value::Null);
    }

    if upper.starts_with('_') {
        return pg_array_to_json_value(row, idx).unwrap_or(serde_json::Value::Null);
    }

    row.try_get::<_, String>(idx)
        .map(serde_json::Value::String)
        .or_else(|e| pg_system_u32_to_json(row, idx).ok_or(e))
        .or_else(|_| row.try_get::<_, i64>(idx).map(safe_i64_to_json))
        .or_else(|_| row.try_get::<_, i32>(idx).map(|v| serde_json::Value::Number(v.into())))
        .or_else(|_| row.try_get::<_, i16>(idx).map(|v| serde_json::Value::Number(v.into())))
        .or_else(|_| row.try_get::<_, i8>(idx).map(|v| serde_json::Value::Number(v.into())))
        .or_else(|e| pg_array_to_json_value(row, idx).ok_or(e))
        .or_else(|_| {
            row.try_get::<_, f64>(idx).map(|v| {
                serde_json::Number::from_f64(v).map(serde_json::Value::Number).unwrap_or(serde_json::Value::Null)
            })
        })
        .or_else(|_| {
            row.try_get::<_, f32>(idx).map(|v| {
                serde_json::Number::from_f64((v as f64 * 1_000_000.0).round() / 1_000_000.0)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            })
        })
        .or_else(|_| row.try_get::<_, bool>(idx).map(serde_json::Value::Bool))
        .or_else(|e| pg_temporal_to_json_value(row, idx).ok_or(e))
        .or_else(|_| row.try_get::<_, Vec<u8>>(idx).map(|bytes| binary_value_to_json(&bytes)))
        .or_else(|_| row.try_get::<_, PgAnyString>(idx).map(|v| serde_json::Value::String(v.0)))
        .unwrap_or(serde_json::Value::Null)
}

fn pg_error_to_string(err: tokio_postgres::Error) -> String {
    err.as_db_error().map(ToString::to_string).unwrap_or_else(|| err.to_string())
}

fn should_retry_postgres_text_query(err: &tokio_postgres::Error) -> bool {
    let message = err.as_db_error().map(ToString::to_string).unwrap_or_else(|| err.to_string()).to_ascii_lowercase();
    message.contains("no binary output function")
        || message.contains("no binary send function")
        || message.contains("cannot display a value of type")
}

// ─── TLS / URL helpers ────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct PostgresSslFiles {
    sslcert: Option<String>,
    sslkey: Option<String>,
    sslrootcert: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PostgresConnectionUrl {
    url: String,
    ssl_files: PostgresSslFiles,
    accepts_invalid_certs: bool,
    verifies_hostname: bool,
}

fn postgres_connection_url(url: &str) -> Result<PostgresConnectionUrl, String> {
    let Some(query_start) = url.find('?') else {
        let pg_config =
            tokio_postgres::Config::from_str(url).map_err(|e| format!("Invalid PostgreSQL connection URL: {e}"))?;
        return Ok(PostgresConnectionUrl {
            url: url.to_string(),
            ssl_files: PostgresSslFiles::default(),
            accepts_invalid_certs: postgres_sslmode_accepts_invalid_certs(pg_config.get_ssl_mode()),
            verifies_hostname: false,
        });
    };

    let prefix = &url[..query_start];
    let suffix = &url[query_start + 1..];
    let (query_string, fragment) = suffix.split_once('#').map_or((suffix, ""), |(query, fragment)| (query, fragment));
    let mut ssl_files = PostgresSslFiles::default();
    let mut kept_params = Vec::new();
    let mut accepts_invalid_certs = true;
    let mut verifies_hostname = false;

    for param in query_string.split('&') {
        if param.is_empty() {
            continue;
        }
        let Some((key, value)) = param.split_once('=') else {
            kept_params.push(param.to_string());
            continue;
        };
        if key.eq_ignore_ascii_case("sslcert")
            || key.eq_ignore_ascii_case("sslkey")
            || key.eq_ignore_ascii_case("sslrootcert")
        {
            let decoded = percent_decode_str(value)
                .decode_utf8()
                .map_err(|_| format!("Invalid URL encoding in {key}"))?
                .into_owned();
            if key.eq_ignore_ascii_case("sslcert") {
                ssl_files.sslcert = Some(decoded);
            } else if key.eq_ignore_ascii_case("sslkey") {
                ssl_files.sslkey = Some(decoded);
            } else {
                ssl_files.sslrootcert = Some(decoded);
            }
        } else if key.eq_ignore_ascii_case("sslmode") {
            match value.to_ascii_lowercase().as_str() {
                "verify-ca" => {
                    accepts_invalid_certs = false;
                    kept_params.push("sslmode=require".to_string());
                }
                "verify-full" | "verify_identity" | "verify-identity" => {
                    accepts_invalid_certs = false;
                    verifies_hostname = true;
                    kept_params.push("sslmode=require".to_string());
                }
                "disable" => {
                    accepts_invalid_certs = false;
                    kept_params.push(param.to_string());
                }
                "prefer" | "require" => {
                    accepts_invalid_certs = true;
                    kept_params.push(param.to_string());
                }
                _ => kept_params.push(param.to_string()),
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

    Ok(PostgresConnectionUrl { url: sanitized_url, ssl_files, accepts_invalid_certs, verifies_hostname })
}

fn postgres_tls_config(
    pg_config: &tokio_postgres::Config,
    ssl_files: &PostgresSslFiles,
    accepts_invalid_certs: bool,
    verifies_hostname: bool,
) -> Result<rustls::ClientConfig, String> {
    if pg_config.get_ssl_mode() != SslMode::Disable && accepts_invalid_certs {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let builder = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoPostgresCertVerification { provider }));
        return postgres_tls_client_auth(builder, ssl_files);
    }

    let root_store = postgres_root_cert_store(ssl_files)?;
    let builder = if verifies_hostname {
        rustls::ClientConfig::builder().with_root_certificates(root_store)
    } else {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        rustls::ClientConfig::builder().dangerous().with_custom_certificate_verifier(Arc::new(
            PostgresCaOnlyCertVerification { provider, roots: Arc::new(root_store) },
        ))
    };
    postgres_tls_client_auth(builder, ssl_files)
}

fn postgres_root_cert_store(ssl_files: &PostgresSslFiles) -> Result<rustls::RootCertStore, String> {
    let mut root_store = rustls::RootCertStore::empty();
    if let Some(path) = ssl_files.sslrootcert.as_deref() {
        let certs = read_postgres_pem_certs("sslrootcert", path)?;
        let (valid_count, _) = root_store.add_parsable_certificates(certs);
        if valid_count == 0 {
            return Err(format!("sslrootcert: no valid CA certificates found in {path}"));
        }
    } else {
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    }
    Ok(root_store)
}

fn postgres_tls_client_auth(
    builder: rustls::ConfigBuilder<rustls::ClientConfig, rustls::client::WantsClientCert>,
    ssl_files: &PostgresSslFiles,
) -> Result<rustls::ClientConfig, String> {
    match (ssl_files.sslcert.as_deref(), ssl_files.sslkey.as_deref()) {
        (Some(cert_path), Some(key_path)) => {
            let certs = read_postgres_pem_certs("sslcert", cert_path)?;
            if certs.is_empty() {
                return Err(format!("sslcert: no certificates found in {cert_path}"));
            }
            let private_key = read_postgres_private_key(key_path)?;
            builder
                .with_client_auth_cert(certs, private_key)
                .map_err(|e| format!("PostgreSQL client certificate/key mismatch or invalid key: {e}"))
        }
        (Some(_), None) => Err("PostgreSQL sslcert requires sslkey".to_string()),
        (None, Some(_)) => Err("PostgreSQL sslkey requires sslcert".to_string()),
        (None, None) => Ok(builder.with_no_client_auth()),
    }
}

fn read_postgres_pem_certs(label: &str, path: &str) -> Result<Vec<CertificateDer<'static>>, String> {
    let file = File::open(path).map_err(|e| format!("{label}: failed to open {path}: {e}"))?;
    let mut reader = BufReader::new(file);
    rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{label}: failed to read PEM certificates from {path}: {e}"))
}

fn read_postgres_private_key(path: &str) -> Result<PrivateKeyDer<'static>, String> {
    let file = File::open(path).map_err(|e| format!("sslkey: failed to open {path}: {e}"))?;
    let mut reader = BufReader::new(file);
    rustls_pemfile::private_key(&mut reader)
        .map_err(|e| format!("sslkey: failed to read PEM private key from {path}: {e}"))?
        .ok_or_else(|| format!("sslkey: no private key found in {path}"))
}

fn postgres_sslmode_accepts_invalid_certs(ssl_mode: SslMode) -> bool {
    matches!(ssl_mode, SslMode::Prefer | SslMode::Require)
}

#[derive(Debug)]
struct NoPostgresCertVerification {
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for NoPostgresCertVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

#[derive(Debug)]
struct PostgresCaOnlyCertVerification {
    provider: Arc<CryptoProvider>,
    roots: Arc<rustls::RootCertStore>,
}

impl ServerCertVerifier for PostgresCaOnlyCertVerification {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let cert = ParsedCertificate::try_from(end_entity)?;
        verify_server_cert_signed_by_trust_anchor(
            &cert,
            &self.roots,
            intermediates,
            now,
            self.provider.signature_verification_algorithms.all,
        )?;
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

fn pg_url_has_timezone_setting(url: &str) -> bool {
    let lower = url.to_lowercase();
    if let Some(query) = lower.split('?').nth(1) {
        if query.contains("timezone=") || query.contains("timezone%3d") {
            return true;
        }
    }
    false
}

// ─── pool creation ────────────────────────────────────────────────────────────

/// Create a connection pool from a PgConfig.
/// If `config.ssh` is set the caller must have already started the SSH tunnel
/// and updated `config.host`/`config.port` to the tunnel's local endpoint.
pub async fn create_pool(config: &PgConfig) -> Result<PgPool, String> {
    let url = config.to_url();
    let postgres_url = postgres_connection_url(&url)?;
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let timeout = Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS);

    let pg_config = tokio_postgres::Config::from_str(&postgres_url.url)
        .map_err(|e| format!("Invalid PostgreSQL connection URL: {e}"))?;

    let mgr_config = ManagerConfig { recycling_method: RecyclingMethod::Verified };
    let tls_config = postgres_tls_config(
        &pg_config,
        &postgres_url.ssl_files,
        postgres_url.accepts_invalid_certs,
        postgres_url.verifies_hostname,
    )?;
    let mgr = deadpool_postgres::Manager::from_config(
        pg_config.clone(),
        tokio_postgres_rustls::MakeRustlsConnect::new(tls_config),
        mgr_config,
    );
    let pool = Pool::builder(mgr)
        .max_size(3)
        .runtime(Runtime::Tokio1)
        .wait_timeout(Some(timeout))
        .build()
        .map_err(|e| format!("Failed to create PostgreSQL pool: {e}"))?;

    // Verify connectivity and optionally set timezone
    tokio::time::timeout(timeout, async {
        let client = pool.get().await.map_err(|e| format!("PostgreSQL connection failed: {e}"))?;
        if !pg_url_has_timezone_setting(&url) {
            // Best-effort: set local timezone. Ignore errors on restricted accounts.
            let tz = std::env::var("TZ").unwrap_or_else(|_| "UTC".to_string());
            let _ = client
                .execute(&format!("SET timezone = '{}'", tz.replace('\'', "''")), &[])
                .await;
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| format!("PostgreSQL connection timed out ({}s)", timeout.as_secs()))??;

    Ok(pool)
}

// ─── query execution ──────────────────────────────────────────────────────────

async fn execute_select_prepared(
    client: &deadpool_postgres::Client,
    sql: &str,
    start: Instant,
    row_limit: usize,
) -> Result<QueryResult, tokio_postgres::Error> {
    let stmt = client.prepare_cached(sql).await?;
    let columns: Vec<String> = stmt.columns().iter().map(|c| c.name().to_string()).collect();
    let column_types: Vec<String> = stmt.columns().iter().map(|c| c.type_().name().to_string()).collect();

    let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = Vec::new();
    let stream = client.query_raw(&stmt, params).await?;
    tokio::pin!(stream);
    let mut result_rows: Vec<Vec<serde_json::Value>> = Vec::new();
    let mut truncated = false;

    while let Some(row_result) = stream.next().await {
        if result_rows.len() >= row_limit {
            truncated = true;
            break;
        }
        let row = row_result?;
        result_rows.push(
            (0..row.columns().len())
                .map(|i| pg_value_to_json(&row, i, column_types.get(i).map(String::as_str).unwrap_or("")))
                .collect(),
        );
    }

    Ok(QueryResult {
        columns,
        row_count: result_rows.len(),
        rows: result_rows,
        duration_ms: start.elapsed().as_millis() as u64,
        truncated,
    })
}

async fn execute_select_text(
    client: &deadpool_postgres::Client,
    sql: &str,
    start: Instant,
    row_limit: usize,
) -> Result<QueryResult, String> {
    let messages = client.simple_query(sql).await.map_err(pg_error_to_string)?;
    let mut columns: Vec<String> = Vec::new();
    let mut result_rows: Vec<Vec<serde_json::Value>> = Vec::new();
    let mut truncated = false;

    for message in messages {
        match message {
            SimpleQueryMessage::RowDescription(cols) => {
                columns = cols.iter().map(|c| c.name().to_string()).collect();
            }
            SimpleQueryMessage::Row(row) => {
                if columns.is_empty() {
                    columns = row.columns().iter().map(|c| c.name().to_string()).collect();
                }
                if result_rows.len() >= row_limit {
                    truncated = true;
                    continue;
                }
                let mut values = Vec::with_capacity(row.len());
                for i in 0..row.len() {
                    values.push(match row.try_get(i).map_err(pg_error_to_string)? {
                        Some(value) => serde_json::Value::String(value.to_string()),
                        None => serde_json::Value::Null,
                    });
                }
                result_rows.push(values);
            }
            SimpleQueryMessage::CommandComplete(_) => {}
            _ => {}
        }
    }

    Ok(QueryResult {
        columns,
        row_count: result_rows.len(),
        rows: result_rows,
        duration_ms: start.elapsed().as_millis() as u64,
        truncated,
    })
}

async fn execute_select_query(
    client: &deadpool_postgres::Client,
    sql: &str,
    start: Instant,
    row_limit: usize,
) -> Result<QueryResult, String> {
    match execute_select_prepared(client, sql, start, row_limit).await {
        Ok(result) => Ok(result),
        Err(err) if should_retry_postgres_text_query(&err) => execute_select_text(client, sql, start, row_limit).await,
        Err(err) => Err(pg_error_to_string(err)),
    }
}

/// Execute a query against the pool, returning up to `max_rows` rows.
pub async fn execute_query(pool: &PgPool, sql: &str, max_rows: usize) -> Result<QueryResult, String> {
    let start = Instant::now();
    let row_limit = max_rows.max(1);

    if starts_with_executable_sql_keyword(sql, &["SELECT", "SHOW", "EXPLAIN", "WITH", "TABLE"]) {
        let client = pool.get().await.map_err(|e| e.to_string())?;
        execute_select_query(&client, sql, start, row_limit).await
    } else {
        let client = pool.get().await.map_err(|e| e.to_string())?;
        let affected = client.execute(sql, &[]).await.map_err(pg_error_to_string)?;
        Ok(QueryResult {
            columns: vec![],
            row_count: 0,
            rows: vec![],
            duration_ms: start.elapsed().as_millis() as u64,
            truncated: false,
        })
    }
}

/// Execute a query with an explicit schema search path.
pub async fn execute_query_with_schema(
    pool: &PgPool,
    schema: &str,
    sql: &str,
    max_rows: usize,
) -> Result<QueryResult, String> {
    let start = Instant::now();
    let row_limit = max_rows.max(1);
    let client = pool.get().await.map_err(|e| e.to_string())?;
    client.execute(&format!("SET search_path TO {}", pg_quote_ident(schema)), &[]).await.map_err(pg_error_to_string)?;

    let result = if starts_with_executable_sql_keyword(sql, &["SELECT", "SHOW", "EXPLAIN", "WITH", "TABLE"]) {
        execute_select_query(&client, sql, start, row_limit).await
    } else {
        let affected = client.execute(sql, &[]).await.map_err(pg_error_to_string)?;
        Ok(QueryResult {
            columns: vec![],
            row_count: 0,
            rows: vec![],
            duration_ms: start.elapsed().as_millis() as u64,
            truncated: false,
        })
    };
    let _ = client.execute("RESET search_path", &[]).await;
    result
}

/// Introspect the schema of a PostgreSQL database, returning one TableInfo per table/view.
pub async fn introspect_schema(pool: &PgPool, schema: &str) -> Result<Vec<TableInfo>, String> {
    let client = pool.get().await.map_err(|e| e.to_string())?;

    let stmt = client
        .prepare_cached(
            "SELECT a.attname AS column_name, \
             format_type(a.atttypid, a.atttypmod) AS full_type, \
             NOT a.attnotnull AS is_nullable, \
             pg_get_expr(ad.adbin, ad.adrelid) AS column_default, \
             EXISTS ( \
               SELECT 1 FROM pg_constraint co \
               JOIN pg_index i ON i.indrelid = co.conrelid AND co.conindid = i.indexrelid \
               WHERE co.conrelid = a.attrelid AND co.contype = 'p' \
               AND a.attnum = ANY(i.indkey) \
             ) AS is_pk, \
             (quote_ident(n.nspname) || '.' || quote_ident(c.relname)) AS table_fqn \
             FROM pg_attribute a \
             JOIN pg_class c ON c.oid = a.attrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_type t ON t.oid = a.atttypid \
             LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum \
             WHERE n.nspname = $1 \
               AND c.relkind IN ('r','v','m','f') \
               AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY c.relname, a.attnum",
        )
        .await
        .map_err(|e| e.to_string())?;

    let rows = client.query(&stmt, &[&schema]).await.map_err(|e| e.to_string())?;

    let mut tables: std::collections::BTreeMap<String, Vec<ColumnInfo>> = std::collections::BTreeMap::new();
    for row in &rows {
        let table_fqn: String = row.try_get::<_, String>(5).unwrap_or_default();
        // Use plain relname by stripping schema prefix
        let table_name = row
            .try_get::<_, String>(5)
            .ok()
            .and_then(|fqn| {
                // fqn = "schema"."table" — extract table part
                fqn.split('.').last().map(|t| t.trim_matches('"').to_string())
            })
            .unwrap_or(table_fqn);

        let full_type = row.try_get::<_, Option<String>>(1).ok().flatten().unwrap_or_default();
        let col = ColumnInfo {
            name: row.try_get::<_, String>(0).unwrap_or_default(),
            data_type: full_type,
            nullable: row.try_get::<_, bool>(2).unwrap_or(true),
            default_value: row.try_get::<_, Option<String>>(3).ok().flatten(),
            is_primary_key: row.try_get::<_, bool>(4).unwrap_or(false),
        };
        tables.entry(table_name).or_default().push(col);
    }

    // Row counts and sizes
    let stat_stmt = client
        .prepare_cached(
            "SELECT c.relname, \
             GREATEST(c.reltuples::bigint, 0)::bigint AS row_estimate, \
             pg_total_relation_size(c.oid)::bigint AS total_bytes \
             FROM pg_class c \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE c.relkind = 'r' AND n.nspname = $1",
        )
        .await
        .map_err(|e| e.to_string())?;

    let stat_rows = client.query(&stat_stmt, &[&schema]).await.map_err(|e| e.to_string())?;
    let mut row_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    let mut size_bytes_map: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for row in &stat_rows {
        let tbl: String = row.try_get::<_, String>(0).unwrap_or_default();
        let cnt: i64 = row.try_get::<_, i64>(1).unwrap_or(0);
        let sz: i64 = row.try_get::<_, i64>(2).unwrap_or(0);
        if cnt >= 0 {
            row_counts.insert(tbl.clone(), cnt as u64);
        }
        if sz > 0 {
            size_bytes_map.insert(tbl, sz as u64);
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
pub async fn test_connection(config: &PgConfig) -> Result<(), String> {
    let pool = create_pool(config).await?;
    let _ = pool.close();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pg_quote_ident_plain() {
        assert_eq!(pg_quote_ident("public"), "\"public\"");
    }

    #[test]
    fn pg_quote_ident_escapes_double_quotes() {
        assert_eq!(pg_quote_ident("my\"schema"), "\"my\"\"schema\"");
    }

    #[test]
    fn postgres_connection_url_verify_ca_semantics() {
        let parsed = postgres_connection_url("postgres://localhost/db?sslmode=verify-ca").unwrap();
        assert_eq!(parsed.url, "postgres://localhost/db?sslmode=require");
        assert!(!parsed.accepts_invalid_certs);
        assert!(!parsed.verifies_hostname);
    }

    #[test]
    fn postgres_connection_url_verify_full_semantics() {
        let parsed = postgres_connection_url("postgres://localhost/db?sslmode=verify-full").unwrap();
        assert_eq!(parsed.url, "postgres://localhost/db?sslmode=require");
        assert!(!parsed.accepts_invalid_certs);
        assert!(parsed.verifies_hostname);
    }

    #[test]
    fn url_with_options_timezone_returns_true() {
        assert!(pg_url_has_timezone_setting("postgres://localhost/db?options=-c timezone=UTC"));
    }

    #[test]
    fn url_without_timezone_returns_false() {
        assert!(!pg_url_has_timezone_setting("postgres://localhost/db?sslmode=require"));
    }
}

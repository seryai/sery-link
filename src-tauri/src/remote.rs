//! Remote data sources (Phase A — public HTTPS URLs).
//!
//! This module owns the "scan a URL" path. It mirrors what `scanner.rs`
//! does for local files but:
//!   * Skips the filesystem walk — the URL is a single file, not a folder.
//!   * Uses a HEAD request to establish a freshness signal (Last-Modified
//!     + Content-Length) for the scan cache, since URLs have no `fs::metadata`.
//!   * Runs DuckDB's `read_csv_auto` / `read_parquet` directly on the URL
//!     via the httpfs extension, which we load lazily per-connection.
//!
//! Everything produced here lands in the same `DatasetMetadata` shape as
//! local files, so FolderDetail, FileDetail, search, and the scan cache
//! all work unchanged downstream.

use crate::error::{AgentError, Result};
use crate::scanner::{ColumnSchema, DatasetMetadata};
use duckdb::Connection;
use std::time::Duration;

/// Result of probing a URL with HEAD. Values are best-effort — servers
/// don't have to return them, so both fields are optional and the
/// caller should treat missing data as "unknown, cache conservatively".
#[derive(Debug, Clone, Default)]
pub struct RemoteHeadInfo {
    /// `Last-Modified` header parsed as Unix seconds, or `None` if the
    /// server didn't send it / we couldn't parse it.
    pub last_modified_secs: Option<i64>,
    /// `Content-Length` header as bytes.
    pub content_length: Option<i64>,
}

/// Blocking HEAD request with a short timeout. Used by the scanner to
/// fill `(mtime_secs, size_bytes)` on the scan cache key when the
/// source is a URL instead of a file.
///
/// Errors only when the HEAD request itself fails (network, DNS). A
/// 2xx with no Last-Modified / Content-Length is a success with
/// `None` fields — the server is allowed to omit them.
///
/// Returns `Ok(default)` immediately for s3:// URLs — reqwest can't
/// sign AWS requests and the scan cache tolerates missing freshness
/// hints. S3 object metadata can still be fetched via DuckDB if we
/// need it later.
pub async fn head_probe(url: &str) -> Result<RemoteHeadInfo> {
    if crate::url::is_s3_url(url) {
        return Ok(RemoteHeadInfo::default());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        // Follow redirects — many CDNs 301 to a signed URL.
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| AgentError::Network(format!("build client: {}", e)))?;

    let resp = client
        .head(url)
        .send()
        .await
        .map_err(|e| AgentError::Network(format!("HEAD {}: {}", url, e)))?;

    if !resp.status().is_success() {
        return Err(AgentError::Network(format!(
            "HEAD {} returned {}",
            url,
            resp.status()
        )));
    }

    let last_modified_secs = resp
        .headers()
        .get(reqwest::header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_http_date);

    let content_length = resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok());

    Ok(RemoteHeadInfo {
        last_modified_secs,
        content_length,
    })
}

/// One object discovered by an S3 listing query. Keeps the glob
/// results typed so downstream per-file scanning has size/mtime to
/// seed the scan cache without a separate HEAD request.
#[derive(Debug, Clone)]
pub struct ListedObject {
    pub url: String,
    pub size_bytes: Option<i64>,
    pub last_modified_secs: Option<i64>,
}

/// Enumerate objects under an S3 bucket / prefix / glob. Runs one
/// DuckDB `glob(...)` query against the httpfs-enabled connection;
/// each row is one matching object URL.
///
/// Called inside a `spawn_blocking` because DuckDB is sync. The caller
/// (scanner) is responsible for loading creds + httpfs before the
/// glob query — same as for single-object scans.
pub fn list_s3_blocking(listing_url: &str) -> Result<Vec<ListedObject>> {
    // Cap the per-listing object count so a bucket with millions of
    // keys doesn't blow up memory or burn through S3 LIST budget. If
    // we hit this, the user can narrow with an explicit glob.
    const MAX_LISTED_OBJECTS: usize = 10_000;

    let conn = Connection::open_in_memory()
        .map_err(|e| AgentError::Database(format!("open DuckDB: {}", e)))?;
    install_httpfs(&conn)?;
    apply_s3_credentials(&conn, listing_url)?;

    let pattern = crate::url::expand_s3_listing_pattern(listing_url);
    let escaped = pattern.replace('\'', "''");

    // `glob(pattern)` returns a table with `file` (URL). The default
    // `expand_s3_listing_pattern` produces `<prefix>/**/*` — recursive
    // and no extension filter at the SQL layer. Filtering by extension
    // happens Rust-side below: DuckDB-httpfs brace expansion against
    // S3 listings was unreliable (matching files but returning empty
    // results in some setups), so we ask for everything and discard
    // non-tabular keys ourselves.
    let sql = format!("SELECT file FROM glob('{}')", escaped);
    eprintln!("[remote-list] ▶ {}", pattern);
    let _ = std::io::Write::flush(&mut std::io::stderr());

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| AgentError::Database(format!("prepare glob: {}", e)))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| AgentError::Database(format!("execute glob: {}", e)))?;

    // Allowed extensions match what scan_remote_blocking_with_creds
    // can actually read. Keep this in sync with that match arm.
    let allowed = ["csv", "tsv", "parquet"];
    let user_explicit_glob = listing_url.contains('*');

    let mut total_seen = 0usize;
    let mut skipped_unsupported = 0usize;
    let mut out = Vec::new();
    for row in rows {
        let Ok(url) = row else { continue };
        total_seen += 1;
        let ext = crate::url::extension_from_url(&url);
        // If the user typed an explicit glob (`s3://bucket/**/*.json`)
        // they get exactly what they asked for; don't second-guess.
        // Otherwise filter to the formats we can actually scan.
        if !user_explicit_glob && !allowed.contains(&ext.as_str()) {
            skipped_unsupported += 1;
            continue;
        }
        out.push(ListedObject {
            url,
            size_bytes: None,
            last_modified_secs: None,
        });
        if out.len() >= MAX_LISTED_OBJECTS {
            eprintln!(
                "[remote-list] ⚠ hit MAX_LISTED_OBJECTS ({}). Narrow with an explicit glob to see the rest.",
                MAX_LISTED_OBJECTS
            );
            break;
        }
    }
    eprintln!(
        "[remote-list] ✓ {} object(s) kept ({} total seen, {} skipped as non-tabular)",
        out.len(),
        total_seen,
        skipped_unsupported
    );
    Ok(out)
}

/// Synchronous schema + sample extraction for a remote URL via DuckDB.
/// Back-compat shim that uses the URL itself as the credential lookup
/// key — correct for single-object scans. Listing fan-out should call
/// `scan_remote_blocking_with_creds` instead, passing the parent
/// listing URL as the creds source.
pub fn scan_remote_blocking(
    url: &str,
    head: &RemoteHeadInfo,
) -> Result<DatasetMetadata> {
    scan_remote_blocking_with_creds(url, head, url)
}

/// Like `scan_remote_blocking` but takes the key-ring lookup key
/// explicitly. S3 listings store creds under the bucket/prefix URL
/// but then scan individual object URLs — the two don't match, so the
/// creds source has to be passed in separately.
///
/// Expected to run inside `tokio::task::spawn_blocking` since DuckDB's
/// Rust binding is sync.
pub fn scan_remote_blocking_with_creds(
    url: &str,
    head: &RemoteHeadInfo,
    creds_source: &str,
) -> Result<DatasetMetadata> {
    // Determine how to read the URL. DuckDB's httpfs extension gives us
    // `read_parquet('https://…')`, `read_csv_auto('https://…')`, etc.
    // We dispatch on the URL extension; unrecognised extensions fall
    // back to read_csv_auto which is the most permissive.
    let ext = crate::url::extension_from_url(url);
    let (read_func, file_format): (&str, &str) = match ext.as_str() {
        "parquet" => ("read_parquet", "parquet"),
        "csv" | "tsv" => ("read_csv_auto", "csv"),
        // xlsx-over-HTTP requires downloading the whole file first
        // (calamine doesn't stream). Defer to Phase B.
        other => {
            return Err(AgentError::Database(format!(
                "unsupported remote file type: {}. Phase A supports csv / parquet URLs",
                if other.is_empty() { "unknown" } else { other }
            )));
        }
    };

    let conn = Connection::open_in_memory()
        .map_err(|e| AgentError::Database(format!("open DuckDB: {}", e)))?;

    // Load httpfs on this fresh connection. INSTALL is a no-op if the
    // extension is already downloaded; LOAD makes it available for
    // queries. Runs once per scan — cheap.
    install_httpfs(&conn)?;

    // S3 URLs need credentials pushed into the connection before the
    // query. Loaded from the keyring entry the UI wrote when the
    // source was added (`creds_source` — for listings this is the
    // parent prefix, for single-object it's the URL itself). A
    // missing entry is a user-visible error — we can't silently try
    // an anonymous fetch against a private bucket.
    if crate::url::is_s3_url(url) {
        apply_s3_credentials(&conn, creds_source)?;
    }

    let escaped_url = url.replace('\'', "''");

    // Trace so a crash mid-fetch tells us which URL tripped things.
    eprintln!("[remote-scan] ▶ {}", url);
    let _ = std::io::Write::flush(&mut std::io::stderr());

    let schema = extract_remote_schema(&conn, read_func, &escaped_url)?;
    let row_count = count_remote_rows(&conn, read_func, &escaped_url).unwrap_or(-1);
    let (sample_rows, samples_redacted) =
        extract_remote_samples(&conn, read_func, &escaped_url, &schema)
            .unwrap_or((None, false));

    eprintln!("[remote-scan] ✓ {} ({} cols)", url, schema.len());

    // Synthesise a `DatasetMetadata` that matches what local files
    // produce, so FolderDetail / FileDetail / search all render it
    // identically.
    let relative_path = crate::url::infer_filename_from_url(url);
    let last_modified = head
        .last_modified_secs
        .and_then(|secs| chrono::DateTime::<chrono::Utc>::from_timestamp(secs, 0))
        .map(|t| t.to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let size_bytes = head.content_length.unwrap_or(0).max(0) as u64;

    Ok(DatasetMetadata {
        relative_path,
        file_format: file_format.to_string(),
        size_bytes,
        row_count_estimate: if row_count >= 0 { Some(row_count) } else { None },
        schema,
        last_modified,
        document_markdown: None,
        sample_rows,
        samples_redacted,
    })
}

/// Push stored S3 credentials into this DuckDB session via the legacy
/// `SET s3_*` syntax. Called by any remote query path that touches an
/// `s3://` URL — scanner and profile both share this.
///
/// Public so `commands::profile_remote` can reuse the same statements
/// without duplicating the keyring lookup.
pub(crate) fn apply_s3_credentials(conn: &Connection, url: &str) -> Result<()> {
    let creds = crate::remote_creds::load(url)
        .map_err(|e| AgentError::Config(format!("lookup s3 creds: {}", e)))?
        .ok_or_else(|| {
            AgentError::Config(format!(
                "no S3 credentials saved for {}. Remove and re-add the source.",
                url
            ))
        })?;
    let sql = crate::remote_creds::duckdb_setters(&creds);
    conn.execute_batch(&sql)
        .map_err(|e| AgentError::Database(format!("set s3 creds: {}", e)))
}

/// Pre-flight credential check used by `add_remote_source` before
/// persisting anything. Opens a fresh in-memory DuckDB, applies the
/// supplied creds (without going through the keyring), and runs a
/// minimal probe against the URL. Surfaces auth / region / network
/// failures synchronously so the user sees them on the modal instead
/// of as a mysterious empty scan.
///
/// Empty results are NOT an error here — the bucket may legitimately
/// be empty, and the post-scan silent-empty handler in scanner.rs
/// covers that case with a more informative message. We only fail
/// when DuckDB itself errors (auth rejection, region redirect, DNS,
/// httpfs misconfig, etc.).
pub fn test_s3_credentials_blocking(
    url: &str,
    creds: &crate::remote_creds::S3Credentials,
) -> Result<()> {
    let conn = Connection::open_in_memory()
        .map_err(|e| AgentError::Database(format!("open DuckDB: {}", e)))?;
    install_httpfs(&conn)?;

    // Apply creds directly without touching the keyring — we don't
    // want to write credentials we haven't verified yet.
    let setters = crate::remote_creds::duckdb_setters(creds);
    conn.execute_batch(&setters)
        .map_err(|e| AgentError::Database(format!("set s3 creds: {}", e)))?;

    if crate::url::is_s3_listing(url) {
        let pattern = crate::url::expand_s3_listing_pattern(url);
        let escaped = pattern.replace('\'', "''");
        let probe = format!("SELECT file FROM glob('{}') LIMIT 1", escaped);
        conn.execute_batch(&probe).map_err(|e| {
            AgentError::Database(format!(
                "S3 connection test failed: {}. Double-check the bucket name, region, and credentials.",
                e
            ))
        })?;
    } else {
        // Single object — DESCRIBE reads only the Parquet footer or
        // sniffs the first CSV rows, so it's the cheapest probe that
        // actually requires the bytes (and therefore the auth path).
        let ext = crate::url::extension_from_url(url);
        let read_func = match ext.as_str() {
            "parquet" => "read_parquet",
            "csv" | "tsv" => "read_csv_auto",
            // Other extensions aren't supported in Phase A anyway —
            // skip the probe and let the scan path produce the
            // canonical "unsupported remote file type" error.
            _ => return Ok(()),
        };
        let escaped = url.replace('\'', "''");
        let probe = format!("DESCRIBE SELECT * FROM {}('{}') LIMIT 0", read_func, escaped);
        conn.execute_batch(&probe).map_err(|e| {
            AgentError::Database(format!(
                "S3 connection test failed: {}. Double-check the URL, region, and credentials.",
                e
            ))
        })?;
    }

    Ok(())
}

/// Load DuckDB's httpfs extension. No-op if it's already installed in
/// the user's DuckDB extension directory (the download only happens on
/// first ever call, then it's cached in `~/.duckdb/extensions/`).
pub(crate) fn install_httpfs(conn: &Connection) -> Result<()> {
    // INSTALL before LOAD — bundled DuckDB only ships the core; the
    // httpfs extension is downloaded from extensions.duckdb.org on
    // first use and cached in the user's home dir.
    conn.execute_batch(
        "INSTALL httpfs; LOAD httpfs;",
    )
    .map_err(|e| AgentError::Database(format!("load httpfs: {}", e)))
}

fn extract_remote_schema(
    conn: &Connection,
    read_func: &str,
    escaped_url: &str,
) -> Result<Vec<ColumnSchema>> {
    // DESCRIBE over the remote URL pulls only the Parquet footer (or
    // sniffs the first CSV rows) — we don't download the whole file.
    let sql = format!("DESCRIBE SELECT * FROM {}('{}')", read_func, escaped_url);
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| AgentError::Database(format!("prepare DESCRIBE: {}", e)))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| AgentError::Database(format!("query DESCRIBE: {}", e)))?;

    let mut columns = Vec::new();
    for row in rows {
        if let Ok((name, col_type)) = row {
            columns.push(ColumnSchema {
                name,
                col_type,
                nullable: true,
            });
        }
    }
    Ok(columns)
}

fn count_remote_rows(conn: &Connection, read_func: &str, escaped_url: &str) -> Result<i64> {
    // COUNT(*) over a remote file is expensive for CSV (streams the
    // whole body) but cheap for parquet (metadata). Ignore failures —
    // a row count of -1 shows up as "unknown" in the UI.
    let sql = format!("SELECT COUNT(*) FROM {}('{}')", read_func, escaped_url);
    conn.query_row(&sql, [], |row| row.get::<_, i64>(0))
        .map_err(|e| AgentError::Database(format!("COUNT(*): {}", e)))
}

/// Best-effort sample rows for the cloud agent / UI preview. Same
/// shape as `scanner::extract_sample_rows` so the downstream rendering
/// is identical. PII scrubbing reuses the local helper via a public
/// re-export.
fn extract_remote_samples(
    conn: &Connection,
    read_func: &str,
    escaped_url: &str,
    schema: &[ColumnSchema],
) -> Result<(
    Option<Vec<serde_json::Map<String, serde_json::Value>>>,
    bool,
)> {
    if schema.is_empty() {
        return Ok((None, false));
    }

    let sql = format!(
        "SELECT * FROM {}('{}') LIMIT {}",
        read_func,
        escaped_url,
        crate::scanner::sample_row_limit()
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| AgentError::Database(format!("prepare samples: {}", e)))?;

    let redacted_indices: Vec<usize> = schema
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            if crate::scanner::is_pii_column_name(&c.name) {
                Some(i)
            } else {
                None
            }
        })
        .collect();

    let rows = stmt
        .query_map([], |row| {
            let mut obj = serde_json::Map::with_capacity(schema.len());
            for (i, col) in schema.iter().enumerate() {
                let value = if redacted_indices.contains(&i) {
                    serde_json::Value::String("<redacted>".to_string())
                } else {
                    crate::scanner::duckdb_cell_to_json(row, i)
                };
                obj.insert(col.name.clone(), value);
            }
            Ok(obj)
        })
        .map_err(|e| AgentError::Database(format!("query samples: {}", e)))?;

    let mut samples: Vec<serde_json::Map<String, serde_json::Value>> = Vec::new();
    for row in rows {
        if let Ok(obj) = row {
            samples.push(obj);
        }
        if samples.len() >= crate::scanner::sample_row_limit() {
            break;
        }
    }

    if samples.is_empty() {
        return Ok((None, false));
    }
    Ok((Some(samples), !redacted_indices.is_empty()))
}

/// Parse an HTTP `Last-Modified` value (RFC 7231 IMF-fixdate) to Unix
/// seconds. Tolerant of the handful of historically-valid formats —
/// on failure returns `None` so the caller treats the freshness as
/// unknown.
fn parse_http_date(s: &str) -> Option<i64> {
    // Try the modern RFC 7231 format first; fall back to the legacy
    // RFC 850 form.
    let formats = &[
        "%a, %d %b %Y %H:%M:%S GMT",      // IMF-fixdate
        "%A, %d-%b-%y %H:%M:%S GMT",      // RFC 850
        "%a %b %e %H:%M:%S %Y",            // asctime
    ];
    for fmt in formats {
        if let Ok(dt) = chrono::DateTime::parse_from_str(s, fmt) {
            return Some(dt.timestamp());
        }
        if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(ndt.and_utc().timestamp());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_http_date_handles_rfc7231() {
        // Epoch-style check so the tests don't depend on the machine TZ.
        let ts = parse_http_date("Wed, 21 Oct 2015 07:28:00 GMT").unwrap();
        // 2015-10-21 07:28:00 UTC == 1445412480 seconds since epoch.
        assert_eq!(ts, 1445412480);
    }

    #[test]
    fn parse_http_date_returns_none_on_junk() {
        assert!(parse_http_date("").is_none());
        assert!(parse_http_date("not a date").is_none());
        assert!(parse_http_date("2015-10-21").is_none());
    }
}

use duckdb::Connection;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::path::Path;
use std::time::Duration;
use crate::config::Config;
use crate::error::{AgentError, Result};
use crate::excel;

/// Hard upper bound on rows returned by a tunnel query. Protects the
/// client from OOM when the cloud asks for something unbounded, and
/// bounds the amount of data that could be exfiltrated in a single
/// compromised-backend scenario.
const MAX_ROWS_PER_QUERY: usize = 100_000;

/// Hard upper bound on wall-clock query time. Rescues the client from
/// runaway queries (intentional or accidental) that would otherwise
/// pin CPU indefinitely via `spawn_blocking`.
const QUERY_TIMEOUT_SECS: u64 = 60;

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: usize,
    pub duration_ms: u64,
    /// True when the MAX_ROWS_PER_QUERY cap was hit and the result
    /// was cut short. Surfaced to the caller so it can distinguish
    /// "the query naturally returned this many rows" from "we
    /// stopped reading."
    #[serde(default)]
    pub truncated: bool,
}

pub async fn execute_query(sql: &str, file_path: &str, config: &Config) -> Result<QueryResult> {
    // ─── Security gate ─────────────────────────────────────────────────────
    // The SQL string comes from the cloud tunnel. Before letting DuckDB
    // touch it we enforce three invariants that together bound the
    // "compromised backend / prompt injection" blast radius:
    //
    //   1. `file_path` parameter must be in a watched folder. This check
    //      is the original path sandbox and stays authoritative for the
    //      path the cloud explicitly names.
    //
    //   2. The SQL string must contain the `{{file}}` placeholder. The
    //      only path a query is allowed to read is the one we insert via
    //      placeholder substitution using the validated `file_path`.
    //      SQL that omits the placeholder is rejected — we refuse to
    //      execute raw SQL where we haven't proven which file it reads.
    //
    //   3. The SQL must not contain any DuckDB file-reading or filesystem
    //      functions of its own (`read_csv`, `read_parquet`, `read_json*`,
    //      `glob`, `ATTACH`, `COPY`, `INSTALL`, `LOAD`). Those would read
    //      paths OTHER than the validated `{{file}}` substitution.
    //
    // These together mean: the query can only read the single file that
    // the cloud named and that the user added to their watched folders.
    // A compromised backend that tries `SELECT * FROM read_csv('/etc/passwd')`
    // gets rejected here.

    if !is_path_allowed(file_path, config) {
        return Err(AgentError::Database(format!(
            "File path not in watched folders: {}",
            file_path
        )));
    }

    validate_sql_payload(sql)?;

    // Verify file exists
    if !Path::new(file_path).exists() {
        return Err(AgentError::Database(format!("File not found: {}", file_path)));
    }

    // Execute query in blocking task to avoid blocking async runtime,
    // with a hard timeout so a runaway query can't pin CPU forever.
    let sql = sql.to_string();
    let file_path = file_path.to_string();

    let task = tokio::task::spawn_blocking(move || execute_query_blocking(&sql, &file_path));

    match tokio::time::timeout(Duration::from_secs(QUERY_TIMEOUT_SECS), task).await {
        Ok(join_result) => join_result
            .map_err(|e| AgentError::Database(format!("Query task failed: {}", e)))?,
        Err(_) => {
            // The blocking task keeps running until DuckDB yields — we
            // can't cancel it — but we return promptly so the caller
            // isn't blocked.
            Err(AgentError::Database(format!(
                "Query exceeded {}-second timeout",
                QUERY_TIMEOUT_SECS
            )))
        }
    }
}

/// Security check for SQL received over the tunnel. See §Security gate
/// in `execute_query` for the full rationale.
///
/// Returns Err with a specific reason if the SQL is rejected. Pure:
/// does not execute anything, does not consult filesystem or config.
/// All unit-tested in `tests::validation_*` below.
pub(crate) fn validate_sql_payload(sql: &str) -> Result<()> {
    // Placeholder is required. SQL that doesn't name {{file}} would get
    // passed verbatim to DuckDB, which would happily read any path the
    // query names.
    if !sql.contains("{{file}}") {
        return Err(AgentError::Database(
            "SQL is missing the {{file}} placeholder — refusing to run a query that doesn't bind to a validated file path.".to_string(),
        ));
    }

    // Case-insensitive scan for function names and statements that can
    // touch files other than the {{file}} substitution. The scan is
    // whitespace-tolerant (`read_csv  (`) by normalizing, and
    // identifier-boundary-aware (doesn't false-positive on
    // `my_read_csv_col`).
    let lowered = sql.to_ascii_lowercase();

    // (token, display_name_for_error)
    const FORBIDDEN_FUNCTIONS: &[&str] = &[
        "read_csv",
        "read_csv_auto",
        "read_parquet",
        "read_json",
        "read_json_auto",
        "read_json_objects",
        "read_ndjson",
        "read_ndjson_auto",
        "read_ndjson_objects",
        "read_blob",
        "read_text",
        "glob",
        "parquet_scan",
        "parquet_metadata",
    ];

    for name in FORBIDDEN_FUNCTIONS {
        if contains_function_call(&lowered, name) {
            return Err(AgentError::Database(format!(
                "SQL contains forbidden file-access function: `{}`. Only the {{{{file}}}} placeholder is allowed to reference files.",
                name
            )));
        }
    }

    // Keyword-level bans. These aren't functions so the
    // contains_function_call helper doesn't fit; just require they
    // appear as a whole word.
    const FORBIDDEN_KEYWORDS: &[&str] = &[
        "attach", // `ATTACH 'other.db'` attaches a new database
        "copy",   // `COPY tbl FROM 'x.csv'` / `COPY tbl TO 'x.csv'`
        "install", // `INSTALL extension`
        "load",   // `LOAD extension`
        "pragma", // PRAGMAs can change security-relevant settings
        "export", // `EXPORT DATABASE 'dir'`
        "import", // `IMPORT DATABASE 'dir'`
    ];

    for keyword in FORBIDDEN_KEYWORDS {
        if contains_keyword(&lowered, keyword) {
            return Err(AgentError::Database(format!(
                "SQL contains forbidden keyword: `{}`. Only SELECT-shaped queries on the {{{{file}}}} placeholder are allowed.",
                keyword.to_ascii_uppercase()
            )));
        }
    }

    Ok(())
}

/// True iff `text` contains `fname` followed by `(` (optionally with
/// whitespace), AND the character before `fname` is not an identifier
/// character (so `my_read_csv(...)` doesn't match `read_csv`).
fn contains_function_call(text: &str, fname: &str) -> bool {
    let bytes = text.as_bytes();
    let fbytes = fname.as_bytes();
    let mut i = 0usize;
    while i + fbytes.len() <= bytes.len() {
        if &bytes[i..i + fbytes.len()] == fbytes {
            let before_ok = i == 0 || !is_ident_char(bytes[i - 1]);
            if before_ok {
                // Skip whitespace after fname
                let mut j = i + fbytes.len();
                while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t' || bytes[j] == b'\n') {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'(' {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// True iff `text` contains `keyword` as a standalone word (identifier
/// boundaries on both sides).
fn contains_keyword(text: &str, keyword: &str) -> bool {
    let bytes = text.as_bytes();
    let kbytes = keyword.as_bytes();
    let mut i = 0usize;
    while i + kbytes.len() <= bytes.len() {
        if &bytes[i..i + kbytes.len()] == kbytes {
            let before_ok = i == 0 || !is_ident_char(bytes[i - 1]);
            let after_ok =
                i + kbytes.len() == bytes.len() || !is_ident_char(bytes[i + kbytes.len()]);
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

#[allow(dead_code)]
#[allow(dead_code)]
fn
 execute_query_blocking(sql: &str, file_path: &str) -> Result<QueryResult> {
    let start = std::time::Instant::now();

    // Create in-memory connection
    let conn = Connection::open_in_memory()
        .map_err(|e| AgentError::Database(format!("Failed to open DuckDB connection: {}", e)))?;

    // Detect file type and prepare SQL
    let file_ext = Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    // For xlsx we transparently convert to cached CSV, then to Parquet.
    // For csv we transparently convert to cached Parquet for 10-100x faster queries.
    // The original file_path is kept for error reporting but the effective read
    // target is the cached Parquet file.
    let (effective_path, effective_ext): (Cow<str>, &str) = match file_ext {
        "xlsx" | "xls" => {
            let csv = excel::xlsx_to_csv(Path::new(file_path))?;
            let parquet = crate::csv::csv_to_parquet(&csv)?;
            (Cow::Owned(parquet.to_string_lossy().to_string()), "parquet")
        },
        "csv" => {
            let parquet = crate::csv::csv_to_parquet(Path::new(file_path))?;
            (Cow::Owned(parquet.to_string_lossy().to_string()), "parquet")
        },
        _ => (Cow::Borrowed(file_path), file_ext)
    };

    let read_func = match effective_ext {
        "parquet" => "read_parquet",
        "csv" => "read_csv_auto",
        _ => {
            return Err(AgentError::Database(format!(
                "Unsupported file format: {}",
                file_ext
            )))
        }
    };

    // Replace file placeholder in SQL. `execute_query` (the async
    // entry point) has already validated that the SQL contains
    // `{{file}}` and no other file-reading constructs, so we don't
    // repeat that check here; we just do the substitution.
    let final_sql = sql.replace(
        "{{file}}",
        &format!("{}('{}')", read_func, effective_path.as_ref()),
    );

    // Execute query
    let mut stmt = conn
        .prepare(&final_sql)
        .map_err(|e| AgentError::Database(format!("Failed to prepare query: {}", e)))?;

    // Get column names
    let columns: Vec<String> = stmt
        .column_names()
        .iter()
        .map(|s| s.to_string())
        .collect();

    // Execute and collect rows, capped at MAX_ROWS_PER_QUERY to bound
    // memory + limit the data a compromised backend could exfiltrate
    // in a single query.
    let mut rows = Vec::new();
    let mut truncated = false;
    let mut result_rows = stmt
        .query([])
        .map_err(|e| AgentError::Database(format!("Query execution failed: {}", e)))?;

    while let Some(row) = result_rows
        .next()
        .map_err(|e| AgentError::Database(format!("Row fetch failed: {}", e)))?
    {
        if rows.len() >= MAX_ROWS_PER_QUERY {
            truncated = true;
            break;
        }

        let mut row_values = Vec::new();
        for i in 0..columns.len() {
            // Convert DuckDB value to JSON
            let value = row_value_to_json(&row, i)?;
            row_values.push(value);
        }
        rows.push(row_values);
    }

    let row_count = rows.len();
    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(QueryResult {
        columns,
        rows,
        row_count,
        duration_ms,
        truncated,
    })
}

#[allow(dead_code)]
fn
 row_value_to_json(row: &duckdb::Row, idx: usize) -> Result<serde_json::Value> {
    // Try different types
    if let Ok(val) = row.get::<_, Option<i64>>(idx) {
        return Ok(val.map(|v| serde_json::json!(v)).unwrap_or(serde_json::Value::Null));
    }
    if let Ok(val) = row.get::<_, Option<f64>>(idx) {
        return Ok(val.map(|v| serde_json::json!(v)).unwrap_or(serde_json::Value::Null));
    }
    if let Ok(val) = row.get::<_, Option<String>>(idx) {
        return Ok(val.map(|v| serde_json::json!(v)).unwrap_or(serde_json::Value::Null));
    }
    if let Ok(val) = row.get::<_, Option<bool>>(idx) {
        return Ok(val.map(|v| serde_json::json!(v)).unwrap_or(serde_json::Value::Null));
    }

    // Default to string representation
    match row.get::<_, Option<String>>(idx) {
        Ok(Some(val)) => Ok(serde_json::json!(val)),
        _ => Ok(serde_json::Value::Null),
    }
}

#[allow(dead_code)]
fn
 is_path_allowed(path: &str, config: &Config) -> bool {
    let path = Path::new(path);

    config.watched_folders.iter().any(|folder| {
        let folder_path = Path::new(&folder.path);
        path.starts_with(folder_path)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_validation() {
        let mut config = Config::default();
        config.add_watched_folder("/tmp/data".to_string(), true);

        assert!(is_path_allowed("/tmp/data/file.parquet", &config));
        assert!(is_path_allowed("/tmp/data/subdir/file.parquet", &config));
        assert!(!is_path_allowed("/etc/passwd", &config));
    }

    // ─── SQL validation tests ──────────────────────────────────────────────
    //
    // These assert the specific shapes we care about when validating
    // tunnel-delivered SQL. A missing or weakened check here directly
    // reopens the path-sandbox bypass, so if you're editing
    // validate_sql_payload, make sure these still pass.

    #[test]
    fn validation_accepts_sql_with_placeholder_only() {
        assert!(validate_sql_payload("SELECT * FROM {{file}} LIMIT 10").is_ok());
        assert!(validate_sql_payload("SELECT COUNT(*) FROM {{file}}").is_ok());
        assert!(validate_sql_payload(
            "SELECT a, b FROM {{file}} WHERE c > 100 GROUP BY a ORDER BY b LIMIT 500"
        )
        .is_ok());
    }

    #[test]
    fn validation_accepts_multiple_placeholder_references() {
        // Self-joins / unions on the same file are legitimate.
        assert!(validate_sql_payload(
            "SELECT * FROM {{file}} UNION ALL SELECT * FROM {{file}}"
        )
        .is_ok());
    }

    #[test]
    fn validation_rejects_missing_placeholder() {
        // No placeholder — the query could be literally anything, refuse.
        assert!(validate_sql_payload("SELECT 1").is_err());
        assert!(validate_sql_payload("SELECT now()").is_err());
        assert!(validate_sql_payload("").is_err());
    }

    #[test]
    fn validation_rejects_direct_read_csv_calls() {
        // Classic bypass: the SQL names a validated placeholder AND then
        // also reads /etc/passwd as a side channel.
        let sqls = [
            "SELECT * FROM {{file}} UNION ALL SELECT * FROM read_csv_auto('/etc/passwd')",
            "SELECT * FROM {{file}}; SELECT * FROM read_csv_auto('/etc/passwd')",
            "SELECT content FROM read_csv_auto('/Users/victim/.ssh/id_rsa') WHERE {{file}}",
            "WITH t AS (SELECT * FROM read_csv('/tmp/secret')) SELECT * FROM t JOIN {{file}}",
        ];
        for s in sqls {
            assert!(
                validate_sql_payload(s).is_err(),
                "Should have rejected: {}",
                s
            );
        }
    }

    #[test]
    fn validation_rejects_direct_read_parquet_calls() {
        assert!(validate_sql_payload(
            "SELECT * FROM {{file}} UNION ALL SELECT * FROM read_parquet('/tmp/other.parquet')"
        )
        .is_err());
        // Whitespace variations.
        assert!(validate_sql_payload(
            "SELECT * FROM read_parquet  ('/tmp/other.parquet'), {{file}}"
        )
        .is_err());
    }

    #[test]
    fn validation_rejects_json_and_glob_reads() {
        assert!(validate_sql_payload(
            "SELECT * FROM read_json_auto('/tmp/other.json'), {{file}}"
        )
        .is_err());
        assert!(validate_sql_payload(
            "SELECT * FROM glob('/Users/victim/**/*'), {{file}}"
        )
        .is_err());
    }

    #[test]
    fn validation_rejects_attach_copy_load() {
        assert!(validate_sql_payload(
            "ATTACH '/tmp/other.duckdb' AS other; SELECT * FROM {{file}}"
        )
        .is_err());
        assert!(validate_sql_payload(
            "COPY (SELECT * FROM {{file}}) TO '/tmp/leak.csv'"
        )
        .is_err());
        assert!(validate_sql_payload(
            "INSTALL httpfs; SELECT * FROM {{file}}"
        )
        .is_err());
        assert!(validate_sql_payload(
            "LOAD 'httpfs'; SELECT * FROM {{file}}"
        )
        .is_err());
        assert!(validate_sql_payload(
            "PRAGMA enable_external_access=true; SELECT * FROM {{file}}"
        )
        .is_err());
    }

    #[test]
    fn validation_does_not_false_positive_on_similar_identifiers() {
        // Column names that share a substring with a forbidden function
        // shouldn't trigger the sandbox. Previously these would have
        // falsely rejected:
        assert!(validate_sql_payload(
            "SELECT my_read_csv_col, parquet_scanner FROM {{file}}"
        )
        .is_ok());
        // Keyword-as-identifier (DuckDB allows this with quoting; plain
        // usage shouldn't match `attach` as a bare word).
        assert!(validate_sql_payload(
            "SELECT attachment_id, imported_at FROM {{file}}"
        )
        .is_ok());
    }

    #[test]
    fn validation_is_case_insensitive() {
        // Mixed-case bypass attempts.
        assert!(validate_sql_payload(
            "SELECT * FROM Read_CSV_Auto('/etc/passwd'), {{file}}"
        )
        .is_err());
        assert!(validate_sql_payload(
            "ATTACH '/tmp/x.db' AS y; SELECT * FROM {{file}}"
        )
        .is_err());
    }
}

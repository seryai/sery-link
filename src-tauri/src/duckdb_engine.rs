use duckdb::Connection;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::path::Path;
use crate::config::Config;
use crate::error::{AgentError, Result};
use crate::excel;

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: usize,
    pub duration_ms: u64,
}

pub async fn execute_query(sql: &str, file_path: &str, config: &Config) -> Result<QueryResult> {
    // Validate file path is in approved folders
    if !is_path_allowed(file_path, config) {
        return Err(AgentError::Database(format!(
            "File path not in watched folders: {}",
            file_path
        )));
    }

    // Verify file exists
    if !Path::new(file_path).exists() {
        return Err(AgentError::Database(format!("File not found: {}", file_path)));
    }

    // Execute query in blocking task to avoid blocking async runtime
    let sql = sql.to_string();
    let file_path = file_path.to_string();

    tokio::task::spawn_blocking(move || {
        execute_query_blocking(&sql, &file_path)
    })
    .await
    .map_err(|e| AgentError::Database(format!("Query task failed: {}", e)))?
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

    // Replace file placeholder in SQL or use direct query
    let final_sql = if sql.contains("{{file}}") {
        sql.replace(
            "{{file}}",
            &format!("{}('{}')", read_func, effective_path.as_ref()),
        )
    } else {
        sql.to_string()
    };

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

    // Execute and collect rows
    let mut rows = Vec::new();
    let mut result_rows = stmt
        .query([])
        .map_err(|e| AgentError::Database(format!("Query execution failed: {}", e)))?;

    while let Some(row) = result_rows
        .next()
        .map_err(|e| AgentError::Database(format!("Row fetch failed: {}", e)))?
    {
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
    #[allow(dead_code)]
    fn
 test_path_validation() {
        let mut config = Config::default();
        config.add_watched_folder("/tmp/data".to_string(), true);

        assert!(is_path_allowed("/tmp/data/file.parquet", &config));
        assert!(is_path_allowed("/tmp/data/subdir/file.parquet", &config));
        assert!(!is_path_allowed("/etc/passwd", &config));
    }
}

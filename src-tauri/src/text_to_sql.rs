//! Text-to-SQL agent loop for the Ask page.
//!
//! Pipeline (BYOK, single-machine, local data only — v0.7 first cut):
//!
//!   1. Enumerate cached schemas + file paths for every tabular
//!      dataset across all `watched_folders`.
//!   2. Compose a system prompt advertising the available tables
//!      and asking the model to emit ONE SQL query inside a code
//!      fence (or a sentinel marker meaning "can't answer with
//!      this data").
//!   3. Send {system, user_question} to the BYOK provider.
//!   4. Extract the SQL from the response. If the sentinel fires,
//!      return the LLM's natural-language explanation directly.
//!   5. Run the SQL against DuckDB in-memory, with httpfs loaded
//!      so `read_csv` / `read_parquet` work for local + cached
//!      Drive paths. Connection is opened fresh per call so we
//!      can scope it read-only without affecting other queries.
//!   6. Truncate the result to `MAX_RESULT_ROWS` for the
//!      interpretation step + UI render.
//!   7. Send {original question, the result table, instruction to
//!      interpret} back to the LLM.
//!   8. Return AskResponseGrounded with final text + sql trail +
//!      result table.
//!
//! Out of scope for v0.7 first cut:
//!   - Multi-step agent (one SQL attempt, no retry loop)
//!   - Cross-machine queries (single-machine local-only)
//!   - Documents (markdown content; the existing search-based
//!     grounding already handles those — Ask treats text and
//!     tabular as complementary)
//!   - Sery-hosted LLM mode (BYOK only)

use crate::byok;
use crate::config::Config;
use crate::error::Result;
use crate::scan_cache;
use crate::scanner::DatasetMetadata;
use duckdb::Connection;
use serde::Serialize;

/// Cap how many rows we ship into the interpretation prompt + the
/// UI. Picked to fit comfortably in any provider's context window
/// even with wide tables, while still letting the LLM see real
/// distribution of values.
const MAX_RESULT_ROWS: usize = 50;

/// Soft cap on schema dump size sent to the LLM. With ~100 watched
/// datasets each ~250 chars of schema, a typical user fits well
/// under this. If a user with thousands of files trips it we just
/// truncate; future polish: rerank by search hits before truncating.
const MAX_PROMPT_SCHEMA_CHARS: usize = 60_000;

/// What happened on the SQL execution step.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SqlOutcome {
    /// SQL ran, here are the rows. `total_rows` is the count BEFORE
    /// truncation so the UI can show "showing 50 of 1234 rows".
    Rows {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        total_rows: usize,
        truncated: bool,
    },
    /// SQL ran but returned no rows.
    Empty,
    /// LLM declined to write SQL — the question can't be answered
    /// with the available data. The string carries the LLM's
    /// own explanation extracted from the sentinel marker.
    InsufficientData { reason: String },
    /// SQL parse / execution failed. We bubble the DuckDB error
    /// to the user (and forward it to the LLM for the
    /// interpretation step so the answer can mention "the query
    /// failed because…" instead of inventing an answer).
    Error { message: String },
    /// LLM didn't emit a recognisable SQL block AND didn't fire
    /// the sentinel. We fall back to a plain ungrounded answer.
    NoSqlGenerated,
}

/// One round-trip of SQL gen + exec. Frontend renders this as a
/// collapsible "Generated SQL" panel with the result table.
#[derive(Debug, Clone, Serialize)]
pub struct SqlAttempt {
    /// The SQL the LLM wrote, verbatim. Empty when outcome is
    /// `InsufficientData` or `NoSqlGenerated`.
    pub sql: String,
    pub outcome: SqlOutcome,
}

/// Public entry — returns the final natural-language answer plus
/// the SQL/result trail so the UI can show its work. Token usage
/// is summed across BOTH LLM calls (gen + interpret).
#[derive(Debug, Clone, Serialize)]
pub struct AgentAnswer {
    pub text: String,
    pub stop_reason: Option<String>,
    pub usage: Option<crate::byok::anthropic::Usage>,
    pub sql_attempt: Option<SqlAttempt>,
    /// Tables we considered (i.e. enumerated from scan_cache) for
    /// the prompt. The frontend uses the count for "asked over N
    /// tables" copy. Empty when the user has no tabular files
    /// indexed yet.
    pub considered_table_count: usize,
}

/// Snapshot of one tabular dataset, ready to mention in the LLM
/// prompt. We capture absolute path + schema only; sample rows are
/// loaded but kept short to control prompt size.
struct TablePrompt {
    abs_path: String,
    relative_path: String,
    file_format: String,
    schema: Vec<(String, String)>, // (col_name, col_type)
    /// First sample row pretty-printed (or empty). Helps the LLM
    /// pick the right column for substring searches without
    /// blowing the context budget on full rows.
    sample_first_row: Option<String>,
}

/// Snapshot of one document file (PDF, DOCX, PPTX, HTML, IPYNB).
/// SQL can't query these, but they're indexed and the LLM should
/// know they exist — otherwise questions like "do I have an
/// itinerary" get answered "no" because the only doc named
/// itinerary.pdf was invisible to the prompt.
struct DocumentPrompt {
    abs_path: String,
    relative_path: String,
    /// Up to PREVIEW_CHARS of extracted markdown so the LLM can
    /// answer simple "what's in this doc" questions. Documents
    /// without extracted markdown (Shallow tier) ship empty.
    content_preview: Option<String>,
}

/// Document content excerpt cap. 500 chars per doc keeps a 100-doc
/// listing under 50 KB even with previews — comfortable in any
/// provider's context window. Longer docs get truncated with an
/// ellipsis so the LLM knows there's more.
const DOC_PREVIEW_CHARS: usize = 500;

/// Walk every watched folder and split cached metadata into the
/// SQL-queryable bucket (tables) and the not-queryable-but-still-
/// referencable bucket (documents). Used files (gdrive cache,
/// local) flow through identically since the cache keys are
/// absolute paths.
fn enumerate_indexed_files() -> Result<(Vec<TablePrompt>, Vec<DocumentPrompt>)> {
    let cfg = Config::load()?;
    let mut tables: Vec<TablePrompt> = Vec::new();
    let mut documents: Vec<DocumentPrompt> = Vec::new();
    for folder in &cfg.watched_folders {
        let datasets: Vec<DatasetMetadata> = scan_cache::with_cache(|c| {
            c.get_all_for_folder(&folder.path)
        })
        .transpose()?
        .unwrap_or_default();

        for d in datasets {
            let abs_path = format!(
                "{}/{}",
                folder.path.trim_end_matches('/'),
                d.relative_path
            );
            if is_tabular(&d.file_format) {
                tables.push(TablePrompt {
                    abs_path,
                    relative_path: d.relative_path.clone(),
                    file_format: d.file_format.clone(),
                    schema: d
                        .schema
                        .iter()
                        .map(|c| (c.name.clone(), c.col_type.clone()))
                        .collect(),
                    sample_first_row: d.sample_rows.as_ref().and_then(|rows| rows.first()).map(
                        |r| {
                            let mut parts: Vec<String> = Vec::new();
                            for (k, v) in r {
                                let v_str = match v {
                                    serde_json::Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                };
                                parts.push(format!("{}={}", k, v_str));
                            }
                            parts.join(", ")
                        },
                    ),
                });
            } else if is_document(&d.file_format) {
                let preview = d.document_markdown.as_ref().map(|m| {
                    let cleaned = m.trim();
                    if cleaned.chars().count() > DOC_PREVIEW_CHARS {
                        let truncated: String =
                            cleaned.chars().take(DOC_PREVIEW_CHARS).collect();
                        format!("{}…", truncated)
                    } else {
                        cleaned.to_string()
                    }
                });
                documents.push(DocumentPrompt {
                    abs_path,
                    relative_path: d.relative_path.clone(),
                    content_preview: preview,
                });
            }
        }
    }
    Ok((tables, documents))
}

fn is_tabular(file_format: &str) -> bool {
    matches!(
        file_format.to_ascii_lowercase().as_str(),
        "parquet" | "csv" | "tsv" | "xlsx" | "xls"
    )
}

fn is_document(file_format: &str) -> bool {
    matches!(
        file_format.to_ascii_lowercase().as_str(),
        "docx" | "pptx" | "html" | "htm" | "ipynb" | "pdf"
    )
}

/// Pick the right DuckDB read function for a file format.
fn read_function(file_format: &str) -> Option<&'static str> {
    match file_format.to_ascii_lowercase().as_str() {
        "parquet" => Some("read_parquet"),
        "csv" | "tsv" => Some("read_csv_auto"),
        "xlsx" | "xls" => {
            // DuckDB has the spatial extension for xlsx but it's
            // not bundled by default. For v0.7 we punt: the LLM
            // can be told the file exists but can't query it.
            // Future polish: install the spatial ext on demand.
            None
        }
        _ => None,
    }
}

/// Compose the system prompt that advertises tables AND documents.
/// Truncates each section independently if either grows past
/// MAX_PROMPT_SCHEMA_CHARS so a doc-heavy folder doesn't crowd
/// out the table schemas (or vice versa).
fn build_sql_system_prompt(
    tables: &[TablePrompt],
    documents: &[DocumentPrompt],
) -> String {
    let mut s = String::from(
        "You are a data analyst answering questions about local files \
         on the user's computer. Tabular files (CSV, Parquet, XLSX) are \
         queryable via DuckDB. Document files (PDF, DOCX, etc.) are \
         indexed by name + extracted text but cannot be queried with SQL \
         — reference them directly when the user asks.\n\n",
    );

    if tables.is_empty() && documents.is_empty() {
        s.push_str(
            "(No files indexed yet. Ask the user to add a folder \
             containing data or documents.)\n\n",
        );
    }

    if !tables.is_empty() {
        s.push_str("Available tables (SQL-queryable):\n\n");
        let start_len = s.len();
        for t in tables {
            let read_fn = match read_function(&t.file_format) {
                Some(f) => f,
                None => continue, // can't query this file format
            };
            // Per-table block. The path is escaped for SQL string
            // literal safety BEFORE the model sees it.
            let escaped_path = t.abs_path.replace('\'', "''");
            s.push_str(&format!(
                "Table: {} ({}, query with `{}('{}')`)\n",
                t.relative_path, t.file_format, read_fn, escaped_path
            ));
            for (name, ty) in &t.schema {
                s.push_str(&format!("  - {}: {}\n", name, ty));
            }
            if let Some(sample) = &t.sample_first_row {
                s.push_str(&format!("  sample row: {}\n", sample));
            }
            s.push('\n');

            if s.len() - start_len > MAX_PROMPT_SCHEMA_CHARS {
                s.push_str(
                    "(further tables omitted — too many to fit in the prompt; \
                     ask a more specific question to narrow scope)\n\n",
                );
                break;
            }
        }
    }

    if !documents.is_empty() {
        s.push_str("Available documents (filename + content; not SQL-queryable):\n\n");
        let start_len = s.len();
        for d in documents {
            s.push_str(&format!("Document: {} ({})\n", d.relative_path, d.abs_path));
            if let Some(preview) = &d.content_preview {
                if !preview.is_empty() {
                    s.push_str(&format!("  excerpt: {}\n", preview));
                }
            }
            s.push('\n');
            if s.len() - start_len > MAX_PROMPT_SCHEMA_CHARS {
                s.push_str(
                    "(further documents omitted — too many to fit in the prompt; \
                     ask a more specific question to narrow scope)\n\n",
                );
                break;
            }
        }
    }

    s.push_str(
        "Instructions:\n\
         - If the user is asking ABOUT the table list itself (e.g. \"how \
           many tables\", \"what files do I have\", \"list my data\", \
           \"what columns does X have\"), answer DIRECTLY from the table \
           listing above — do NOT fire INSUFFICIENT_DATA, and do NOT \
           write SQL. Just give a clear natural-language answer using \
           the tables/columns shown.\n\
         - Otherwise, emit exactly ONE SQL query inside a markdown code \
           fence: ```sql ... ```\n\
         - Use the read_parquet / read_csv_auto helpers shown above with \
           the literal absolute paths.\n\
         - For substring search on text columns prefer `column ILIKE '%term%'`.\n\
         - SELECT only. No CREATE / DROP / DELETE / INSERT / UPDATE.\n\
         - If the available data can't answer the question (data isn't \
           there, not a meta-question), write \
           `INSUFFICIENT_DATA: <one-line reason>` instead of SQL.\n\
         - Keep the result small — add LIMIT 100 unless an aggregation is the answer.\n",
    );
    s
}

/// Pull the SQL out of a markdown code fence. Returns the trimmed
/// inner SQL (no fence markers, no leading/trailing whitespace) or
/// None if no SQL fence is present.
fn extract_sql(llm_output: &str) -> Option<String> {
    // Match ```sql ... ``` (case-insensitive on the language tag).
    // The simple states are good enough; no need to pull in a
    // markdown parser.
    let lower = llm_output.to_ascii_lowercase();
    let start_marker = lower.find("```sql")?;
    let after_marker = &llm_output[start_marker + 6..];
    let end_marker = after_marker.find("```")?;
    let inner = &after_marker[..end_marker];
    Some(inner.trim().to_string())
}

/// Detect the "I refuse to write SQL" sentinel.
fn extract_insufficient_data(llm_output: &str) -> Option<String> {
    for line in llm_output.lines() {
        let trimmed = line.trim().trim_start_matches('-').trim();
        if let Some(rest) = trimmed.strip_prefix("INSUFFICIENT_DATA:") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// Defensive parse — reject anything that isn't a SELECT (or WITH
/// for CTEs). Belt-and-suspenders given we already prompted for
/// SELECT-only; a misbehaving model could still try to mutate.
fn is_safe_select(sql: &str) -> bool {
    let cleaned = sql.trim().to_ascii_lowercase();
    let starts_safe = cleaned.starts_with("select")
        || cleaned.starts_with("with")
        || cleaned.starts_with("(select");
    if !starts_safe {
        return false;
    }
    // Quick token-boundary check for forbidden keywords. Catches
    // "DROP TABLE", "DELETE FROM", etc. without false-positiving on
    // column names that contain those substrings.
    const FORBIDDEN: &[&str] = &[
        " drop ",
        " delete ",
        " insert ",
        " update ",
        " alter ",
        " truncate ",
        " attach ",
        " detach ",
        " export ",
        " copy ",
    ];
    let padded = format!(" {} ", cleaned);
    !FORBIDDEN.iter().any(|kw| padded.contains(kw))
}

/// Run the SQL against a fresh in-memory DuckDB connection and
/// collect up to MAX_RESULT_ROWS rows. Returns SqlOutcome::Error on
/// any DuckDB failure; the caller still returns a useful answer by
/// telling the LLM what went wrong.
fn execute_sql(sql: &str) -> SqlOutcome {
    if !is_safe_select(sql) {
        return SqlOutcome::Error {
            message: "Refusing to run non-SELECT statement".to_string(),
        };
    }
    let conn = match Connection::open_in_memory() {
        Ok(c) => c,
        Err(e) => {
            return SqlOutcome::Error {
                message: format!("DuckDB open failed: {}", e),
            };
        }
    };

    // httpfs lets the LLM reference http(s) URLs the user has
    // added as remote sources. Ignore install errors — the
    // connection still works for purely local file paths.
    let _ = conn.execute("INSTALL httpfs", []);
    let _ = conn.execute("LOAD httpfs", []);

    // DuckDB's Rust binding panics if column_count() / column_name()
    // are called on a Statement before it's been executed. To get
    // the column names ahead of iteration we run a separate
    // `DESCRIBE (…)` query — that returns one row per output column
    // with the name in column 0. Cheap (no row scan, just schema
    // inference) and avoids fighting the Statement borrow checker
    // to read names from inside the row callback.
    //
    // We strip trailing semicolons + whitespace first because LLMs
    // habitually emit `… LIMIT 100;` and a semicolon can't appear
    // inside the parens of `DESCRIBE (…)` — DuckDB rejects it as a
    // parser error before even seeing the user's query.
    let normalised_sql = sql.trim().trim_end_matches(';').trim_end();
    let describe_sql = format!("DESCRIBE ({})", normalised_sql);
    let columns: Vec<String> = match conn.prepare(&describe_sql) {
        Ok(mut s) => match s.query_map([], |row| row.get::<_, String>(0)) {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                return SqlOutcome::Error {
                    message: format!("DESCRIBE failed: {}", e),
                };
            }
        },
        Err(e) => {
            return SqlOutcome::Error {
                message: format!("DESCRIBE prepare failed: {}", e),
            };
        }
    };
    let column_count = columns.len();

    let mut stmt = match conn.prepare(normalised_sql) {
        Ok(s) => s,
        Err(e) => {
            return SqlOutcome::Error {
                message: format!("SQL prepare failed: {}", e),
            };
        }
    };

    let rows_iter = match stmt.query_map([], |row| {
        let mut out: Vec<String> = Vec::with_capacity(column_count);
        for i in 0..column_count {
            // Duckdb's Value enum stringifies via Debug for the
            // generic case; we coerce to a stable string for the
            // UI. Strings come out unquoted; numbers / dates use
            // their natural repr.
            let v: duckdb::types::Value = row.get(i).unwrap_or(duckdb::types::Value::Null);
            out.push(value_to_string(&v));
        }
        Ok(out)
    }) {
        Ok(it) => it,
        Err(e) => {
            return SqlOutcome::Error {
                message: format!("SQL execution failed: {}", e),
            };
        }
    };

    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut total_rows: usize = 0;
    let mut truncated = false;
    for row_result in rows_iter {
        match row_result {
            Ok(row) => {
                total_rows += 1;
                if rows.len() < MAX_RESULT_ROWS {
                    rows.push(row);
                } else {
                    truncated = true;
                }
            }
            Err(e) => {
                return SqlOutcome::Error {
                    message: format!("SQL row read failed: {}", e),
                };
            }
        }
    }

    if rows.is_empty() && total_rows == 0 {
        SqlOutcome::Empty
    } else {
        SqlOutcome::Rows {
            columns,
            rows,
            total_rows,
            truncated,
        }
    }
}

/// Stringify a DuckDB Value for the UI table + the second LLM
/// call. Strings are returned without surrounding quotes (the UI
/// renders them as cells). Null becomes "NULL" so the LLM can see
/// missingness explicitly.
fn value_to_string(v: &duckdb::types::Value) -> String {
    use duckdb::types::Value;
    match v {
        Value::Null => "NULL".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::TinyInt(n) => n.to_string(),
        Value::SmallInt(n) => n.to_string(),
        Value::Int(n) => n.to_string(),
        Value::BigInt(n) => n.to_string(),
        Value::HugeInt(n) => n.to_string(),
        Value::UTinyInt(n) => n.to_string(),
        Value::USmallInt(n) => n.to_string(),
        Value::UInt(n) => n.to_string(),
        Value::UBigInt(n) => n.to_string(),
        Value::Float(n) => n.to_string(),
        Value::Double(n) => n.to_string(),
        Value::Decimal(d) => d.to_string(),
        Value::Text(s) => s.clone(),
        Value::Blob(b) => format!("<{} bytes>", b.len()),
        Value::Date32(d) => d.to_string(),
        Value::Time64(_, t) => t.to_string(),
        Value::Timestamp(_, t) => t.to_string(),
        // Catch-all for variants we don't care about (lists,
        // structs, intervals). The Debug repr is fine for the UI;
        // the LLM can interpret it well enough.
        other => format!("{:?}", other),
    }
}

/// Render the SQL outcome as a compact text table for the
/// interpretation-step LLM call. Caps row count + column width so
/// a wide result doesn't blow the prompt budget.
fn outcome_for_llm(outcome: &SqlOutcome) -> String {
    match outcome {
        SqlOutcome::Empty => "(no rows returned)".to_string(),
        SqlOutcome::Error { message } => format!("(SQL failed: {})", message),
        SqlOutcome::InsufficientData { reason } => {
            format!("(insufficient data: {})", reason)
        }
        SqlOutcome::NoSqlGenerated => "(no SQL was generated)".to_string(),
        SqlOutcome::Rows {
            columns,
            rows,
            total_rows,
            truncated,
        } => {
            let mut s = String::new();
            s.push_str(&columns.join(" | "));
            s.push('\n');
            for row in rows {
                let truncated_row: Vec<String> = row
                    .iter()
                    .map(|c| {
                        if c.len() > 80 {
                            format!("{}…", &c[..80])
                        } else {
                            c.clone()
                        }
                    })
                    .collect();
                s.push_str(&truncated_row.join(" | "));
                s.push('\n');
            }
            if *truncated {
                s.push_str(&format!(
                    "(showing {} of {} rows)\n",
                    rows.len(),
                    total_rows
                ));
            } else {
                s.push_str(&format!("({} rows)\n", total_rows));
            }
            s
        }
    }
}

/// Two-step agent loop: SQL gen → execute → interpret. Returns the
/// final answer + the SQL trail.
pub async fn ask(
    provider: byok::Provider,
    api_key: &str,
    user_question: &str,
    model: Option<&str>,
) -> Result<AgentAnswer> {
    // ── Step 1: enumerate schemas + documents ──
    let (tables, documents) = enumerate_indexed_files().unwrap_or_else(|e| {
        eprintln!("[text-to-sql] enumerate_indexed_files failed: {}", e);
        (Vec::new(), Vec::new())
    });
    // The "considered" count surfaces in the UI footer. Including
    // documents lets the user see "considered 23 files" rather
    // than "considered 8 tables" when most of their stuff is PDFs.
    let considered_table_count = tables.len() + documents.len();

    // ── Step 2: ask LLM for SQL ──
    let system_prompt = build_sql_system_prompt(&tables, &documents);
    let gen_prompt = format!("{}\n\nUser question: {}", system_prompt, user_question);
    let gen_response = byok::ask(provider, api_key, &gen_prompt, model).await?;

    // ── Step 3: extract SQL OR sentinel ──
    let (sql_attempt, sql_text) = if let Some(reason) =
        extract_insufficient_data(&gen_response.text)
    {
        (
            Some(SqlAttempt {
                sql: String::new(),
                outcome: SqlOutcome::InsufficientData {
                    reason: reason.clone(),
                },
            }),
            None,
        )
    } else if let Some(sql) = extract_sql(&gen_response.text) {
        let outcome = execute_sql(&sql);
        (
            Some(SqlAttempt {
                sql: sql.clone(),
                outcome,
            }),
            Some(sql),
        )
    } else {
        (
            Some(SqlAttempt {
                sql: String::new(),
                outcome: SqlOutcome::NoSqlGenerated,
            }),
            None,
        )
    };

    // ── Step 4: interpret OR fall back ──
    // If SQL produced rows / empty / error, ask the LLM to
    // interpret. If the LLM declined (InsufficientData) or didn't
    // emit SQL, return the LLM's first response as-is — it's
    // probably already a sensible natural-language explanation.
    let final_text: String;
    let stop_reason: Option<String>;
    let mut combined_usage = gen_response.usage.clone();

    let needs_interpretation = matches!(
        sql_attempt.as_ref().map(|a| &a.outcome),
        Some(SqlOutcome::Rows { .. })
            | Some(SqlOutcome::Empty)
            | Some(SqlOutcome::Error { .. })
    );

    if needs_interpretation {
        let outcome_str = outcome_for_llm(&sql_attempt.as_ref().unwrap().outcome);
        let interp_prompt = format!(
            "I ran the following SQL to answer the user's question:\n\
             ```sql\n{}\n```\n\n\
             Result:\n{}\n\n\
             The user's original question was: {}\n\n\
             Provide a clear, concise natural-language answer. \
             Cite specific values from the result. If the result \
             is empty or didn't answer the question, say so plainly \
             — do NOT invent data.",
            sql_text.as_deref().unwrap_or("-- (no SQL)"),
            outcome_str,
            user_question
        );
        let interp_response = byok::ask(provider, api_key, &interp_prompt, model).await?;
        final_text = interp_response.text;
        stop_reason = interp_response.stop_reason;
        // Sum the two LLM calls' token usage so the UI shows the
        // real total cost of the agent loop, not just one half.
        if let (Some(u1), Some(u2)) = (combined_usage.as_ref(), interp_response.usage.as_ref())
        {
            combined_usage = Some(crate::byok::anthropic::Usage {
                input_tokens: u1.input_tokens + u2.input_tokens,
                output_tokens: u1.output_tokens + u2.output_tokens,
            });
        } else {
            combined_usage = interp_response.usage.or(combined_usage);
        }
    } else {
        // Pass the model's own answer through. Strip any code
        // fences left behind so the user gets clean prose.
        final_text = strip_code_fences(&gen_response.text);
        stop_reason = gen_response.stop_reason;
    }

    Ok(AgentAnswer {
        text: final_text,
        stop_reason,
        usage: combined_usage,
        sql_attempt,
        considered_table_count,
    })
}

fn strip_code_fences(s: &str) -> String {
    s.lines()
        .filter(|line| !line.trim_start().starts_with("```"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_sql_finds_fenced_block() {
        let out = "Here you go:\n```sql\nSELECT 1\n```\nLet me know if…";
        assert_eq!(extract_sql(out), Some("SELECT 1".to_string()));
    }

    #[test]
    fn extract_sql_handles_uppercase_tag() {
        let out = "```SQL\nSELECT *\nFROM x\n```";
        assert_eq!(extract_sql(out), Some("SELECT *\nFROM x".to_string()));
    }

    #[test]
    fn extract_sql_returns_none_for_no_fence() {
        assert_eq!(extract_sql("just prose, no code"), None);
        assert_eq!(extract_sql("```python\nprint()\n```"), None);
    }

    #[test]
    fn extract_insufficient_data_finds_marker() {
        let out = "INSUFFICIENT_DATA: no orders table is indexed.";
        assert_eq!(
            extract_insufficient_data(out),
            Some("no orders table is indexed.".to_string())
        );
    }

    #[test]
    fn extract_insufficient_data_handles_dash_prefix() {
        // Some models emit it as a list bullet. Should still match.
        let out = "I can't answer this.\n- INSUFFICIENT_DATA: no schools table.";
        assert_eq!(
            extract_insufficient_data(out),
            Some("no schools table.".to_string())
        );
    }

    #[test]
    fn is_safe_select_accepts_select_and_with() {
        assert!(is_safe_select("SELECT * FROM foo"));
        assert!(is_safe_select("  select 1"));
        assert!(is_safe_select("WITH t AS (SELECT 1) SELECT * FROM t"));
        assert!(is_safe_select("(select 1)"));
    }

    #[test]
    fn is_safe_select_rejects_mutations() {
        assert!(!is_safe_select("DROP TABLE foo"));
        assert!(!is_safe_select("DELETE FROM foo"));
        assert!(!is_safe_select("INSERT INTO foo VALUES (1)"));
        assert!(!is_safe_select("UPDATE foo SET x = 1"));
        assert!(!is_safe_select("SELECT * FROM foo; DROP TABLE bar"));
        assert!(!is_safe_select("ATTACH 'http://evil.com/db'"));
    }

    #[test]
    fn is_safe_select_handles_select_with_trailing_drop() {
        // Adversarial: SELECT prefix + injected mutation. The
        // forbidden-keyword check picks this up.
        assert!(!is_safe_select("SELECT 1 ; DROP TABLE foo"));
    }

    #[test]
    fn read_function_dispatches_known_formats() {
        assert_eq!(read_function("parquet"), Some("read_parquet"));
        assert_eq!(read_function("CSV"), Some("read_csv_auto"));
        assert_eq!(read_function("tsv"), Some("read_csv_auto"));
        assert_eq!(read_function("xlsx"), None);
        assert_eq!(read_function("docx"), None);
    }

    #[test]
    fn build_prompt_handles_empty_tables() {
        let s = build_sql_system_prompt(&[], &[]);
        assert!(s.contains("No files indexed yet"));
        // Instructions still ship even with no tables — the LLM
        // should know what shape its response should take.
        assert!(s.contains("INSUFFICIENT_DATA:"));
    }

    #[test]
    fn build_prompt_lists_table_columns_with_read_fn() {
        let tables = vec![TablePrompt {
            abs_path: "/data/orders.csv".to_string(),
            relative_path: "orders.csv".to_string(),
            file_format: "csv".to_string(),
            schema: vec![
                ("id".to_string(), "BIGINT".to_string()),
                ("amount".to_string(), "DECIMAL".to_string()),
            ],
            sample_first_row: Some("id=1, amount=100.0".to_string()),
        }];
        let s = build_sql_system_prompt(&tables, &[]);
        assert!(s.contains("read_csv_auto('/data/orders.csv')"));
        assert!(s.contains("- id: BIGINT"));
        assert!(s.contains("sample row: id=1"));
    }

    #[test]
    fn build_prompt_lists_documents_with_excerpt() {
        let docs = vec![DocumentPrompt {
            abs_path: "/Users/me/itinerary.pdf".to_string(),
            relative_path: "itinerary.pdf".to_string(),
            content_preview: Some("Flight UA123 SFO→JFK on 2024-06-20".to_string()),
        }];
        let s = build_sql_system_prompt(&[], &docs);
        // The doc filename + path are visible to the LLM so it
        // can answer "do I have an itinerary?" without firing
        // the sentinel.
        assert!(s.contains("itinerary.pdf"));
        assert!(s.contains("/Users/me/itinerary.pdf"));
        assert!(s.contains("Flight UA123"));
        assert!(s.contains("not SQL-queryable"));
    }

    #[test]
    fn build_prompt_truncates_long_document_excerpts() {
        let long = "x".repeat(2000);
        let docs = vec![DocumentPrompt {
            abs_path: "/p/big.pdf".to_string(),
            relative_path: "big.pdf".to_string(),
            content_preview: Some(format!("{}…", &long[..DOC_PREVIEW_CHARS])),
        }];
        let s = build_sql_system_prompt(&[], &docs);
        // Excerpt size guard — caller (enumerate_indexed_files)
        // truncates at DOC_PREVIEW_CHARS; the prompt builder
        // shouldn't blow that budget further.
        assert!(s.len() < 5000, "prompt unexpectedly large: {}", s.len());
    }

    #[test]
    fn build_prompt_escapes_apostrophes_in_path() {
        let tables = vec![TablePrompt {
            abs_path: "/data/john's notes.csv".to_string(),
            relative_path: "john's notes.csv".to_string(),
            file_format: "csv".to_string(),
            schema: vec![],
            sample_first_row: None,
        }];
        let s = build_sql_system_prompt(&tables, &[]);
        // The SQL string literal must be safe — single-quote
        // doubled per SQL escape rules.
        assert!(s.contains("read_csv_auto('/data/john''s notes.csv')"));
    }

    #[test]
    fn execute_strips_trailing_semicolon_for_describe() {
        // Regression: LLMs habitually emit `… LIMIT 100;` which used
        // to break DESCRIBE because a semicolon can't appear inside
        // its parens. With normalisation the query runs cleanly.
        let outcome = execute_sql("SELECT 1 AS x LIMIT 100;");
        match outcome {
            SqlOutcome::Rows {
                columns, total_rows, ..
            } => {
                assert_eq!(columns, vec!["x".to_string()]);
                assert_eq!(total_rows, 1);
            }
            other => panic!("expected Rows, got {:?}", other),
        }
    }

    #[test]
    fn execute_handles_no_trailing_semicolon() {
        // Sanity: the strip is a no-op when the SQL is already clean.
        let outcome = execute_sql("SELECT 42 AS answer");
        match outcome {
            SqlOutcome::Rows { columns, rows, .. } => {
                assert_eq!(columns, vec!["answer".to_string()]);
                assert_eq!(rows, vec![vec!["42".to_string()]]);
            }
            other => panic!("expected Rows, got {:?}", other),
        }
    }

    #[test]
    fn strip_code_fences_removes_fence_lines() {
        let input = "Here:\n```sql\nSELECT 1\n```\nDone.";
        let out = strip_code_fences(input);
        assert!(!out.contains("```"));
        assert!(out.contains("Here:"));
        assert!(out.contains("SELECT 1"));
    }
}

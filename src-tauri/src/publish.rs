//! Publishing — turn a local file into a product the cloud serves.
//!
//! This is Sery Link's job under DECISIONS.md 2026-09-23: convert, upload,
//! keep fresh. Nothing here serves a buyer; the desktop pushes to the api
//! and the api serves from Sery's storage. Every call is initiated from
//! this side with the agent token — the api never reaches back in.
//!
//! Flow for a tabular file:
//!   1. `precheck`  — schema, size, and PII-looking columns, shown before
//!                    the publisher commits to selling
//!   2. `begin`     — POST /v1/products/{hash}/datasets → presigned PUT
//!   3. convert     — DuckDB → Parquet in a temp dir (never the user's folder)
//!   4. upload      — streamed PUT to the presigned URL; the file is never
//!                    read into memory
//!   5. `complete`  — POST .../complete; the api verifies the object and
//!                    binds the buyer-facing table name
//!
//! Documents skip 3–4: passages are extracted here and pushed to
//! /v1/products/{hash}/ingest in batches of 500.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Runtime};

use crate::config::Config;
use crate::error::{AgentError, Result};

pub const EVT_PUBLISH_PROGRESS: &str = "publish_progress";

/// Passages per ingest request — the api caps a request at 500.
const INGEST_BATCH: usize = 500;
const DEFAULT_MAX_CHARS: usize = 1200;
const DEFAULT_OVERLAP: usize = 150;

const TABULAR_EXTS: &[&str] = &["csv", "tsv", "xlsx", "xls", "parquet"];
const DOCUMENT_EXTS: &[&str] = &["pdf", "docx", "pptx", "html", "htm", "ipynb", "epub", "rtf", "md", "txt"];

#[derive(Debug, Clone, Serialize)]
pub struct PublishProgress {
    pub product_hash: String,
    pub relative_path: String,
    pub stage: String, // precheck | converting | uploading | completing | ingesting | done | error
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub detail: Option<String>,
}

fn emit<R: Runtime>(app: &AppHandle<R>, p: PublishProgress) {
    let _ = app.emit(EVT_PUBLISH_PROGRESS, p);
}

// ── api client ────────────────────────────────────────────────────────

struct Api {
    base: String,
    token: String,
    client: reqwest::Client,
}

impl Api {
    fn load() -> Result<Self> {
        let config = Config::load()?;
        let token = crate::keyring_store::get_token()
            .map_err(|_| AgentError::Auth("Not signed in — connect this machine to a workspace first".into()))?;
        Ok(Self {
            base: config.cloud.api_url.trim_end_matches('/').to_string(),
            token,
            client: reqwest::Client::new(),
        })
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Value> {
        let resp = req.bearer_auth(&self.token).send().await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            let detail = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.get("detail").map(|d| d.to_string()))
                .unwrap_or(text);
            return Err(AgentError::Network(format!("api {status}: {detail}")));
        }
        if text.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|e| AgentError::Serialization(e.to_string()))
    }

    async fn get(&self, path: &str) -> Result<Value> {
        self.send(self.client.get(format!("{}{}", self.base, path))).await
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        self.send(self.client.post(format!("{}{}", self.base, path)).json(body)).await
    }

    async fn patch(&self, path: &str, body: &Value) -> Result<Value> {
        self.send(self.client.patch(format!("{}{}", self.base, path)).json(body)).await
    }

    async fn delete(&self, path: &str) -> Result<Value> {
        self.send(self.client.delete(format!("{}{}", self.base, path))).await
    }
}

// ── products ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NewProduct {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// "tabular" | "document"
    pub kind: String,
    pub price_per_call: u32,
    #[serde(default)]
    pub password: Option<String>,
}

pub async fn list_products() -> Result<Value> {
    Api::load()?.get("/v1/products/mine").await
}

pub async fn create_product(p: NewProduct) -> Result<Value> {
    let mut body = json!({
        "name": p.name,
        "description": p.description,
        "tags": p.tags,
        "kind": p.kind,
        "price_per_call": p.price_per_call,
    });
    if let Some(pw) = p.password.filter(|s| !s.is_empty()) {
        body["password"] = json!(pw);
    }
    Api::load()?.post("/v1/products", &body).await
}

pub async fn update_product(hash: &str, patch: Value) -> Result<Value> {
    Api::load()?.patch(&format!("/v1/products/{hash}"), &patch).await
}

pub async fn withdraw_product(hash: &str) -> Result<()> {
    Api::load()?.delete(&format!("/v1/products/{hash}")).await.map(|_| ())
}

pub async fn list_product_datasets(hash: &str) -> Result<Value> {
    Api::load()?.get(&format!("/v1/products/{hash}/datasets")).await
}

pub async fn remove_dataset(hash: &str, dataset_id: &str) -> Result<()> {
    Api::load()?
        .delete(&format!("/v1/products/{hash}/datasets/{dataset_id}"))
        .await
        .map(|_| ())
}

// ── precheck ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Precheck {
    pub relative_path: String,
    pub file_format: String,
    /// "tabular" | "document" | "unsupported"
    pub kind: String,
    pub size_bytes: u64,
    pub row_count_estimate: Option<i64>,
    pub columns: Vec<Value>,
    /// Columns whose names look like personal data. Hosting a product
    /// makes this warning more important, not less: once published the
    /// data is on Sery's servers and queryable by strangers.
    pub pii_columns: Vec<String>,
    pub suggested_table: String,
}

fn ext_of(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn kind_of(ext: &str) -> &'static str {
    if TABULAR_EXTS.contains(&ext) {
        "tabular"
    } else if DOCUMENT_EXTS.contains(&ext) {
        "document"
    } else {
        "unsupported"
    }
}

/// Mirror of the api's table_name_for: basename only, safe identifier.
pub fn suggested_table_name(relative_path: &str) -> String {
    let stem = std::path::Path::new(relative_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("table");
    let mut out = String::new();
    let mut last_us = false;
    for ch in stem.to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_us = false;
        } else if !last_us {
            out.push('_');
            last_us = true;
        }
    }
    let mut name = out.trim_matches('_').to_string();
    if name.is_empty() {
        name = "table".into();
    }
    if name.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        name = format!("t_{name}");
    }
    name.chars().take(60).collect()
}

pub fn precheck(folder_path: &str, relative_path: &str) -> Result<Precheck> {
    let ext = ext_of(relative_path);
    let kind = kind_of(&ext);
    let meta = crate::scanner::reextract_file(folder_path, relative_path)?;
    let columns: Vec<Value> = meta
        .schema
        .iter()
        .map(|c| json!({"name": c.name, "type": c.col_type, "nullable": c.nullable}))
        .collect();
    let pii_columns = meta
        .schema
        .iter()
        .filter(|c| crate::scanner::is_pii_column_name(&c.name))
        .map(|c| c.name.clone())
        .collect();
    Ok(Precheck {
        relative_path: relative_path.to_string(),
        file_format: ext.clone(),
        kind: kind.to_string(),
        size_bytes: meta.size_bytes,
        row_count_estimate: meta.row_count_estimate,
        columns,
        pii_columns,
        suggested_table: suggested_table_name(relative_path),
    })
}

// ── conversion ────────────────────────────────────────────────────────

/// Temp directory for intermediate Parquet. Never the publisher's folder:
/// a file written there would be picked up by the watcher as a new dataset.
fn scratch_dir() -> Result<std::path::PathBuf> {
    let dir = std::env::temp_dir().join("seryai-publish");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Convert a CSV / TSV / Excel file to Parquet in the scratch dir. Parquet
/// sources are returned as-is (`converted == false`).
fn to_parquet(folder_path: &str, relative_path: &str) -> Result<(std::path::PathBuf, bool)> {
    use duckdb::Connection;

    let source = std::path::Path::new(folder_path).join(relative_path);
    if !source.exists() {
        return Err(AgentError::NotFound(format!("file not found: {}", source.display())));
    }
    let ext = ext_of(relative_path);
    if ext == "parquet" {
        return Ok((source, false));
    }
    if !TABULAR_EXTS.contains(&ext.as_str()) {
        return Err(AgentError::Validation(format!("{ext} is not a tabular format")));
    }

    let csv_tmp = if matches!(ext.as_str(), "xlsx" | "xls") {
        Some(crate::excel::xlsx_to_csv(&source)?)
    } else {
        None
    };
    let read_path = csv_tmp.as_deref().unwrap_or(source.as_path());
    let escaped_read = read_path.to_string_lossy().replace('\'', "''");

    let dest = scratch_dir()?.join(format!("{}.parquet", uuid::Uuid::new_v4()));
    let escaped_dest = dest.to_string_lossy().replace('\'', "''");

    let conn = Connection::open_in_memory().map_err(|e| AgentError::Database(e.to_string()))?;
    let attempts = [
        "read_csv_auto('{p}', header=true)",
        "read_csv_auto('{p}', header=true, ignore_errors=true, null_padding=true)",
        "read_csv_auto('{p}', header=false, ignore_errors=true, null_padding=true)",
        "read_csv_auto('{p}', header=true, ignore_errors=true, null_padding=true, all_varchar=true)",
    ];
    let mut last_err = String::new();
    for tmpl in attempts {
        let reader = tmpl.replace("{p}", &escaped_read);
        let sql = format!(
            "COPY (SELECT * FROM {reader}) TO '{escaped_dest}' (FORMAT PARQUET, COMPRESSION 'zstd')"
        );
        match conn.execute(&sql, []) {
            Ok(_) => return Ok((dest, true)),
            Err(e) => {
                let _ = std::fs::remove_file(&dest);
                last_err = e.to_string();
            }
        }
    }
    Err(AgentError::Database(format!("could not convert {}: {last_err}", source.display())))
}

// ── upload ────────────────────────────────────────────────────────────

/// Stream a file to a presigned PUT, reporting (sent, total) as it goes.
///
/// Content-Length is set explicitly so S3 accepts the body without
/// chunked transfer, and the file goes through a stream rather than into
/// memory — datasets can be gigabytes.
pub async fn upload_stream<F>(path: &std::path::Path, upload_url: &str, mut on_progress: F) -> Result<u64>
where
    F: FnMut(u64, u64) + Send + 'static,
{
    use futures::StreamExt;
    use tokio_util::io::ReaderStream;

    let file = tokio::fs::File::open(path).await?;
    let total = file.metadata().await?.len();

    let mut sent: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    let stream = ReaderStream::with_capacity(file, 1 << 20).map(move |chunk| {
        if let Ok(bytes) = &chunk {
            sent += bytes.len() as u64;
            if last_emit.elapsed() > std::time::Duration::from_millis(250) || sent == total {
                last_emit = std::time::Instant::now();
                on_progress(sent, total);
            }
        }
        chunk
    });

    let resp = reqwest::Client::new()
        .put(upload_url)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .header(reqwest::header::CONTENT_LENGTH, total)
        .body(reqwest::Body::wrap_stream(stream))
        .send()
        .await
        .map_err(|e| AgentError::Network(format!("upload failed: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(AgentError::Network(format!("upload rejected ({status}): {body}")));
    }
    Ok(total)
}

async fn upload_file<R: Runtime>(
    app: &AppHandle<R>,
    progress: &PublishProgress,
    path: &std::path::Path,
    upload_url: &str,
) -> Result<u64> {
    let app = app.clone();
    let base = progress.clone();
    upload_stream(path, upload_url, move |sent, total| {
        emit(&app, PublishProgress { stage: "uploading".into(), done_bytes: sent, total_bytes: total, ..base.clone() });
    })
    .await
}

// ── publish: tabular ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Published {
    pub dataset_id: String,
    pub table: Option<String>,
    pub size_bytes: u64,
    pub passages: usize,
}

pub async fn publish_tabular<R: Runtime>(
    app: &AppHandle<R>,
    product_hash: &str,
    folder_path: &str,
    relative_path: &str,
    table: Option<String>,
) -> Result<Published> {
    let api = Api::load()?;
    let mut progress = PublishProgress {
        product_hash: product_hash.to_string(),
        relative_path: relative_path.to_string(),
        stage: "precheck".into(),
        done_bytes: 0,
        total_bytes: 0,
        detail: None,
    };
    emit(app, progress.clone());

    let ext = ext_of(relative_path);
    let meta = tokio::task::spawn_blocking({
        let f = folder_path.to_string();
        let r = relative_path.to_string();
        move || crate::scanner::reextract_file(&f, &r)
    })
    .await
    .map_err(|e| AgentError::FileSystem(e.to_string()))??;

    let abs = std::path::Path::new(folder_path).join(relative_path);
    let columns: Vec<Value> = meta
        .schema
        .iter()
        .map(|c| json!({"name": c.name, "type": c.col_type, "nullable": c.nullable}))
        .collect();

    // 1. begin
    let begin = api
        .post(
            &format!("/v1/products/{product_hash}/datasets"),
            &json!({
                "query_path": abs.to_string_lossy(),
                "file_format": ext,
                "table": table,
                "size_bytes": meta.size_bytes,
                "row_count_estimate": meta.row_count_estimate,
                "columns": columns,
                "sample_rows": meta.sample_rows,
            }),
        )
        .await?;
    let dataset_id = begin["dataset_id"].as_str().unwrap_or_default().to_string();
    let upload_url = begin["upload_url"]
        .as_str()
        .ok_or_else(|| AgentError::Network("api returned no upload URL".into()))?
        .to_string();
    let bound_table = begin["table"].as_str().map(|s| s.to_string());

    // 2. convert
    progress.stage = "converting".into();
    emit(app, progress.clone());
    let (parquet, converted) = tokio::task::spawn_blocking({
        let f = folder_path.to_string();
        let r = relative_path.to_string();
        move || to_parquet(&f, &r)
    })
    .await
    .map_err(|e| AgentError::FileSystem(e.to_string()))??;

    // 3. upload (always clean up the intermediate, success or not)
    progress.stage = "uploading".into();
    let uploaded = upload_file(app, &progress, &parquet, &upload_url).await;
    if converted {
        let _ = std::fs::remove_file(&parquet);
    }
    let size_bytes = uploaded?;

    // 4. complete
    progress.stage = "completing".into();
    progress.done_bytes = size_bytes;
    progress.total_bytes = size_bytes;
    emit(app, progress.clone());
    let done = api
        .post(
            &format!("/v1/products/{product_hash}/datasets/{dataset_id}/complete"),
            &json!({ "size_bytes": size_bytes, "row_count_estimate": meta.row_count_estimate }),
        )
        .await?;

    progress.stage = "done".into();
    emit(app, progress);
    Ok(Published {
        dataset_id,
        table: done["table"].as_str().map(|s| s.to_string()).or(bound_table),
        size_bytes,
        passages: 0,
    })
}

// ── publish: document ─────────────────────────────────────────────────

/// Split one page of markdown into overlapping passages on paragraph
/// boundaries, falling back to a hard character cut for oversized
/// paragraphs. Same algorithm as agent_rpc/commands/files.rs.
pub fn split_passages(text: &str, max_chars: usize, overlap: usize) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if trimmed.chars().count() <= max_chars {
        return vec![trimmed.to_string()];
    }
    let mut out = Vec::new();
    let mut current = String::new();
    for para in trimmed.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        if para.chars().count() > max_chars {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            let chars: Vec<char> = para.chars().collect();
            let step = max_chars.saturating_sub(overlap).max(1);
            let mut start = 0;
            while start < chars.len() {
                let end = (start + max_chars).min(chars.len());
                out.push(chars[start..end].iter().collect());
                if end == chars.len() {
                    break;
                }
                start += step;
            }
            continue;
        }
        if current.chars().count() + para.chars().count() + 2 > max_chars {
            out.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(para);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

pub async fn publish_document<R: Runtime>(
    app: &AppHandle<R>,
    product_hash: &str,
    folder_path: &str,
    relative_path: &str,
) -> Result<Published> {
    let api = Api::load()?;
    let mut progress = PublishProgress {
        product_hash: product_hash.to_string(),
        relative_path: relative_path.to_string(),
        stage: "precheck".into(),
        done_bytes: 0,
        total_bytes: 0,
        detail: None,
    };
    emit(app, progress.clone());

    let ext = ext_of(relative_path);
    let meta = tokio::task::spawn_blocking({
        let f = folder_path.to_string();
        let r = relative_path.to_string();
        move || crate::scanner::reextract_file(&f, &r)
    })
    .await
    .map_err(|e| AgentError::FileSystem(e.to_string()))??;
    let markdown = meta.document_markdown.ok_or_else(|| {
        AgentError::Validation(format!(
            "Content extraction returned nothing for .{ext} — libpdfium (PDF) or pandoc (DOCX/PPTX) may not have loaded"
        ))
    })?;

    let abs = std::path::Path::new(folder_path).join(relative_path);
    let begin = api
        .post(
            &format!("/v1/products/{product_hash}/datasets"),
            &json!({
                "query_path": abs.to_string_lossy(),
                "file_format": ext,
                "size_bytes": meta.size_bytes,
            }),
        )
        .await?;
    let dataset_id = begin["dataset_id"].as_str().unwrap_or_default().to_string();

    // Pdfium marks page boundaries with form feeds; without them the page
    // is genuinely unknown and the citation carries null.
    let raw_pages: Vec<&str> = markdown.split('\x0C').collect();
    let has_pages = raw_pages.len() > 1;
    let doc_title = std::path::Path::new(relative_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(relative_path)
        .to_string();

    let mut chunks: Vec<Value> = Vec::new();
    let mut idx = 0usize;
    for (page_idx, page_text) in raw_pages.iter().enumerate() {
        for passage in split_passages(page_text, DEFAULT_MAX_CHARS, DEFAULT_OVERLAP) {
            let mut entry = json!({
                "dataset_id": dataset_id,
                "doc_path": abs.to_string_lossy(),
                "doc_title": doc_title,
                "chunk_index": idx,
                "text": passage,
            });
            if has_pages {
                entry["page"] = json!(page_idx + 1);
            }
            chunks.push(entry);
            idx += 1;
        }
    }
    if chunks.is_empty() {
        return Err(AgentError::Validation("Document produced no passages — it may be empty or image-only".into()));
    }

    let total = chunks.len();
    let mut ingested = 0usize;
    progress.stage = "ingesting".into();
    progress.total_bytes = total as u64;
    for batch in chunks.chunks(INGEST_BATCH) {
        progress.done_bytes = ingested as u64;
        emit(app, progress.clone());
        api.post(&format!("/v1/products/{product_hash}/ingest"), &json!({ "chunks": batch }))
            .await?;
        ingested += batch.len();
    }

    progress.stage = "done".into();
    progress.done_bytes = total as u64;
    emit(app, progress);
    Ok(Published { dataset_id, table: None, size_bytes: meta.size_bytes, passages: ingested })
}

// ── auto-republish ────────────────────────────────────────────────────
//
// A published file that changes on disk is re-published, so the cloud copy
// tracks the source and buyers see a fresh snapshot_at. The watcher calls
// `republish_changed` after its debounce; the published-file index is
// fetched from the api and cached briefly so a burst of edits does not
// turn into a burst of api calls.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct PublishedEntry {
    product_hash: String,
    table: Option<String>,
}

static PUBLISHED_INDEX: once_cell::sync::Lazy<Mutex<Option<(Instant, HashMap<String, PublishedEntry>)>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(None));
static REPUBLISHING: once_cell::sync::Lazy<Mutex<HashSet<String>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(HashSet::new()));

const INDEX_TTL: Duration = Duration::from_secs(300);

/// Drop the cached index — called after any publish or withdraw from the UI
/// so the next change on disk sees the new state.
pub fn invalidate_published_index() {
    *PUBLISHED_INDEX.lock().unwrap() = None;
}

async fn published_index(api: &Api) -> Result<HashMap<String, PublishedEntry>> {
    if let Some((at, idx)) = PUBLISHED_INDEX.lock().unwrap().as_ref() {
        if at.elapsed() < INDEX_TTL {
            return Ok(idx.clone());
        }
    }
    let mut idx = HashMap::new();
    let products = api.get("/v1/products/mine").await?;
    for p in products.as_array().cloned().unwrap_or_default() {
        let Some(hash) = p["hash"].as_str() else { continue };
        let datasets = api.get(&format!("/v1/products/{hash}/datasets")).await?;
        for d in datasets.as_array().cloned().unwrap_or_default() {
            if let Some(path) = d["query_path"].as_str() {
                idx.insert(
                    path.to_string(),
                    PublishedEntry {
                        product_hash: hash.to_string(),
                        table: d["table"].as_str().map(|s| s.to_string()),
                    },
                );
            }
        }
    }
    *PUBLISHED_INDEX.lock().unwrap() = Some((Instant::now(), idx.clone()));
    Ok(idx)
}

/// Split an absolute path into (watched folder, relative path) using the
/// configured local sources. None if the file is under no watched folder.
fn locate(config: &Config, abs: &std::path::Path) -> Option<(String, String)> {
    let mut roots: Vec<String> = config.watched_folders.iter().map(|f| f.path.clone()).collect();
    for s in &config.sources {
        if let crate::sources::SourceKind::Local { path, .. } = &s.kind {
            roots.push(path.to_string_lossy().into_owned());
        }
    }
    // Longest root first so nested folders resolve to the closest one.
    roots.sort_by_key(|r| std::cmp::Reverse(r.len()));
    for root in roots {
        if let Ok(rel) = abs.strip_prefix(&root) {
            return Some((root, rel.to_string_lossy().into_owned()));
        }
    }
    None
}

/// Re-publish every changed path that is part of a product. Best-effort:
/// failures are logged and emitted as `publish_progress` errors, never
/// propagated — a broken re-publish must not break the folder sync.
pub async fn republish_changed(paths: &[std::path::PathBuf]) {
    let api = match Api::load() {
        Ok(a) => a,
        Err(_) => return, // not signed in — nothing can be published
    };
    let index = match published_index(&api).await {
        Ok(i) => i,
        Err(e) => {
            eprintln!("[publish] could not load published index: {e}");
            return;
        }
    };
    if index.is_empty() {
        return;
    }
    let Ok(config) = Config::load() else { return };
    let Some(app) = crate::events::app_handle() else { return };

    for path in paths {
        let abs = path.to_string_lossy().into_owned();
        let Some(entry) = index.get(&abs) else { continue };
        if !path.exists() {
            continue; // deleted or renamed — the publisher decides what to do
        }
        let Some((folder, rel)) = locate(&config, path) else { continue };

        {
            let mut running = REPUBLISHING.lock().unwrap();
            if !running.insert(abs.clone()) {
                continue;
            }
        }
        eprintln!("[publish] {abs} changed — re-publishing into {}", entry.product_hash);
        let result = match kind_of(&ext_of(&rel)) {
            "tabular" => publish_tabular(app, &entry.product_hash, &folder, &rel, entry.table.clone()).await.map(|_| ()),
            "document" => publish_document(app, &entry.product_hash, &folder, &rel).await.map(|_| ()),
            _ => Ok(()),
        };
        REPUBLISHING.lock().unwrap().remove(&abs);
        if let Err(e) = result {
            eprintln!("[publish] re-publish failed for {abs}: {e}");
            emit(app, PublishProgress {
                product_hash: entry.product_hash.clone(),
                relative_path: rel,
                stage: "error".into(),
                done_bytes: 0,
                total_bytes: 0,
                detail: Some(format!("auto re-publish failed: {e}")),
            });
        }
    }
}

// ── Tauri commands ────────────────────────────────────────────────────

#[tauri::command]
pub async fn publish_list_products() -> std::result::Result<Value, String> {
    list_products().await.map_err(Into::into)
}

#[tauri::command]
pub async fn publish_create_product(product: NewProduct) -> std::result::Result<Value, String> {
    create_product(product).await.map_err(Into::into)
}

#[tauri::command]
pub async fn publish_update_product(hash: String, patch: Value) -> std::result::Result<Value, String> {
    update_product(&hash, patch).await.map_err(Into::into)
}

#[tauri::command]
pub async fn publish_withdraw_product(hash: String) -> std::result::Result<(), String> {
    invalidate_published_index();
    withdraw_product(&hash).await.map_err(Into::into)
}

#[tauri::command]
pub async fn publish_list_datasets(hash: String) -> std::result::Result<Value, String> {
    list_product_datasets(&hash).await.map_err(Into::into)
}

#[tauri::command]
pub async fn publish_remove_dataset(hash: String, dataset_id: String) -> std::result::Result<(), String> {
    invalidate_published_index();
    remove_dataset(&hash, &dataset_id).await.map_err(Into::into)
}

#[tauri::command]
pub async fn publish_precheck(folder_path: String, relative_path: String) -> std::result::Result<Precheck, String> {
    tokio::task::spawn_blocking(move || precheck(&folder_path, &relative_path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(Into::into)
}

/// Publish one file into a product. Picks the tabular or document path
/// from the extension; the product's kind is enforced by the api.
#[tauri::command]
pub async fn publish_file(
    app: AppHandle,
    hash: String,
    folder_path: String,
    relative_path: String,
    table: Option<String>,
) -> std::result::Result<Published, String> {
    invalidate_published_index();
    let ext = ext_of(&relative_path);
    let result = match kind_of(&ext) {
        "tabular" => publish_tabular(&app, &hash, &folder_path, &relative_path, table).await,
        "document" => publish_document(&app, &hash, &folder_path, &relative_path).await,
        _ => Err(AgentError::Validation(format!(".{ext} cannot be published as a dataset"))),
    };
    if let Err(e) = &result {
        emit(&app, PublishProgress {
            product_hash: hash.clone(),
            relative_path: relative_path.clone(),
            stage: "error".into(),
            done_bytes: 0,
            total_bytes: 0,
            detail: Some(e.to_string()),
        });
    }
    result.map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::{split_passages, suggested_table_name};

    #[test]
    fn table_name_uses_the_basename_only() {
        assert_eq!(suggested_table_name("private/dir/Catchments-2026.csv"), "catchments_2026");
        assert_eq!(suggested_table_name("2024 sales (final).xlsx"), "t_2024_sales_final");
        assert_eq!(suggested_table_name("---.csv"), "table");
    }

    #[test]
    fn passages_never_exceed_budget() {
        let text = vec!["x".repeat(100); 10].join("\n\n");
        for p in split_passages(&text, 250, 20) {
            assert!(p.chars().count() <= 250);
        }
    }
}

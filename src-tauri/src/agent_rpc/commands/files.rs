use crate::agent_rpc::registry::{AgentCommand, Ctx};
use async_trait::async_trait;
use serde_json::{json, Value};

/// Accept either `{ query_path }` (dashboard style) or `{ folder_path, relative_path }`
/// (Sery Link UI style) and return `(folder_path, relative_path)`.
fn resolve_path(args: &Value) -> Result<(String, String), String> {
    if let Some(qp) = args["query_path"].as_str() {
        // query_path may be the legacy local://agent_id/path form or a clean
        // absolute path (/Users/foo/file.csv) or a remote URL (s3://...).
        let abs = if qp.starts_with("local://") {
            // Legacy: strip local://agent_id/ → inner path
            let inner = qp.splitn(4, '/').nth(3).unwrap_or(qp);
            // Remote URLs (s3://...) and absolute paths pass through as-is
            if inner.contains("://") || inner.starts_with('/') {
                inner.to_string()
            } else {
                format!("/{inner}")
            }
        } else {
            // Clean path already
            qp.to_string()
        };
        let config = crate::config::Config::load().map_err(|e| e.to_string())?;
        config
            .watched_folders
            .iter()
            .find_map(|f| {
                let root = f.path.trim_end_matches('/');
                let prefix = format!("{root}/");
                if abs.starts_with(&prefix) {
                    Some((f.path.clone(), abs[prefix.len()..].to_string()))
                } else {
                    None
                }
            })
            .ok_or_else(|| format!("could not resolve query_path: {qp}"))
    } else {
        let fp = args["folder_path"].as_str().ok_or("missing folder_path or query_path")?.to_string();
        let rp = args["relative_path"].as_str().ok_or("missing relative_path")?.to_string();
        Ok((fp, rp))
    }
}

// ── files.list ─────────────────────────────────────────────────────────────

pub struct ListFilesCommand;

#[async_trait]
impl AgentCommand for ListFilesCommand {
    fn name(&self) -> &'static str { "files.list" }
    fn description(&self) -> &'static str {
        "List datasets/files known to the local metadata cache, optionally filtered by source."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "source_id": { "type": "string",  "description": "Filter by source UUID (optional)" },
                "query":     { "type": "string",  "description": "Search term (optional)" },
                "limit":     { "type": "integer", "description": "Max results (default 100)" }
            },
            "required": []
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let limit = ctx.args["limit"].as_u64().unwrap_or(100) as usize;
        let query = ctx.args["query"].as_str().unwrap_or("").to_string();
        let _source_id = ctx.args["source_id"].as_str().map(|s| s.to_string());

        let config = crate::config::Config::load().map_err(|e| e.to_string())?;
        let workspace_id = config.agent.workspace_id.as_deref().unwrap_or("local");

        let cache = crate::metadata_cache::MetadataCache::new()
            .map_err(|e| e.to_string())?;

        let datasets: Vec<crate::metadata_cache::CachedDataset> = if query.is_empty() {
            cache.get_all(workspace_id).map_err(|e| e.to_string())?
        } else {
            cache.search(workspace_id, &query, limit)
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|r| r.dataset)
                .collect()
        };

        let files: Vec<Value> = datasets.into_iter()
            .take(limit)
            .map(|r| json!({
                "id":          r.id,
                "name":        r.name,
                "path":        r.path,
                "file_format": r.file_format,
                "size_bytes":  r.size_bytes,
                "last_synced": r.last_synced,
            }))
            .collect();

        let total = files.len();
        Ok(json!({ "files": files, "total": total }))
    }
}

// ── files.preview ──────────────────────────────────────────────────────────

pub struct PreviewFileCommand;

#[async_trait]
impl AgentCommand for PreviewFileCommand {
    fn name(&self) -> &'static str { "files.preview" }
    fn description(&self) -> &'static str {
        "Return the first N rows of a tabular file as JSON."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path":  { "type": "string",  "description": "Absolute file path or query_path" },
                "limit": { "type": "integer", "description": "Max rows to return (default 50)" }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let path  = ctx.args["path"].as_str().ok_or("missing path")?.to_string();
        let limit = ctx.args["limit"].as_u64().unwrap_or(50);

        let config = crate::config::Config::load().map_err(|e| e.to_string())?;
        let sql = format!("SELECT * FROM read_auto('{path}') LIMIT {limit}");
        let result = crate::duckdb_engine::execute_query(&sql, &path, &config)
            .await
            .map_err(|e| e.to_string())?;

        Ok(json!({
            "rows":     result.rows,
            "columns":  result.columns,
            "row_count": result.row_count,
        }))
    }
}

// ── files.schema ───────────────────────────────────────────────────────────

pub struct FileSchemaCommand;

#[async_trait]
impl AgentCommand for FileSchemaCommand {
    fn name(&self) -> &'static str { "files.schema" }
    fn description(&self) -> &'static str {
        "Return the column schema of a tabular file."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute file path" }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let path = ctx.args["path"].as_str().ok_or("missing path")?.to_string();
        let config = crate::config::Config::load().map_err(|e| e.to_string())?;
        let sql = format!("DESCRIBE SELECT * FROM read_auto('{path}')");
        let result = crate::duckdb_engine::execute_query(&sql, &path, &config)
            .await
            .map_err(|e| e.to_string())?;
        Ok(json!({ "columns": result.rows }))
    }
}

// ── files.extract ──────────────────────────────────────────────────────────

pub struct ExtractFileCommand;

#[async_trait]
impl AgentCommand for ExtractFileCommand {
    fn name(&self) -> &'static str { "files.extract" }
    fn description(&self) -> &'static str {
        "Re-extract content (markdown text) from a document file (PDF/DOCX/PPTX/HTML), \
         bypassing the cache. Returns the extracted markdown."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query_path": { "type": "string", "description": "Absolute file path (/Users/foo/file.pdf) or legacy local://agent_id/... form" },
                "max_pages": { "type": "integer", "description": "For PDF files: extract only the first N pages. Omit to extract the full document." }
            },
            "required": ["query_path"]
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let query_path = ctx.args["query_path"].as_str().ok_or("missing query_path")?.to_string();
        let max_pages = ctx.args.get("max_pages").and_then(|v| v.as_u64()).map(|n| n as usize);

        // Resolve to an absolute path.  query_path is now a clean absolute
        // path (/Users/foo/file.pdf) from the dashboard, or the legacy
        // local://agent_id/path form from older callers.
        let abs_path = if query_path.starts_with("local://") {
            let inner = query_path.splitn(4, '/').nth(3).unwrap_or(&query_path);
            if inner.starts_with('/') {
                inner.to_string()
            } else {
                format!("/{inner}")
            }
        } else {
            query_path.clone()
        };

        // Fast path: PDF + max_pages → partial extraction via pdfium directly.
        // Avoids running the full mdkit pipeline (all pages) when the caller
        // only wants a preview of the first N pages.
        let ext = std::path::Path::new(&abs_path)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if ext == "pdf" {
            if let Some(n) = max_pages {
                let path = std::path::PathBuf::from(&abs_path);
                let markdown = tokio::task::spawn_blocking(move || {
                    crate::scanner::extract_pdf_first_pages(&path, n)
                })
                .await
                .map_err(|e| e.to_string())??;

                return Ok(json!({
                    "document_markdown": markdown,
                    "relative_path":     abs_path,
                    "file_format":       "pdf",
                    "truncated":         true,
                }));
            }
        }

        // Full extraction: find which watched folder this path lives under
        // and run reextract_file (all pages, full quality).
        let config = crate::config::Config::load().map_err(|e| e.to_string())?;
        let (folder_path, relative_path) = config
            .watched_folders
            .iter()
            .find_map(|f| {
                let root = f.path.trim_end_matches('/');
                let prefix = format!("{root}/");
                if abs_path.starts_with(&prefix) {
                    let rel = abs_path[prefix.len()..].to_string();
                    Some((f.path.clone(), rel))
                } else {
                    None
                }
            })
            .ok_or_else(|| format!("could not resolve query_path to a watched folder: {query_path}"))?;

        let meta = tokio::task::spawn_blocking(move || {
            crate::scanner::reextract_file(&folder_path, &relative_path)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

        if meta.document_markdown.is_none() {
            let fmt = &meta.file_format;
            return Err(format!(
                "Content extraction returned nothing for .{fmt}. \
                 Check Sery Link app logs for details — libpdfium (PDF) or \
                 pandoc (DOCX/PPTX) may not have loaded."
            ));
        }

        Ok(json!({
            "document_markdown": meta.document_markdown,
            "relative_path":     meta.relative_path,
            "file_format":       meta.file_format,
            "truncated":         false,
        }))
    }
}

// ── files.get_metadata ─────────────────────────────────────────────────────

pub struct GetCachedMetadataCommand;

#[async_trait]
impl AgentCommand for GetCachedMetadataCommand {
    fn name(&self) -> &'static str { "files.get_metadata" }
    fn description(&self) -> &'static str {
        "Return cached metadata for all files in a folder (fast, no re-scan)."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "folder_path": { "type": "string", "description": "Absolute local folder path" }
            },
            "required": ["folder_path"]
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let folder_path = ctx.args["folder_path"].as_str().ok_or("missing folder_path")?.to_string();
        let datasets = crate::commands::get_cached_folder_metadata(folder_path)
            .await
            .map_err(|e| e.to_string())?;
        let value = serde_json::to_value(&datasets).map_err(|e| e.to_string())?;
        Ok(json!({ "datasets": value }))
    }
}

// ── files.profile ──────────────────────────────────────────────────────────

pub struct ProfileFileCommand;

#[async_trait]
impl AgentCommand for ProfileFileCommand {
    fn name(&self) -> &'static str { "files.profile" }
    fn description(&self) -> &'static str {
        "Return per-column statistics (null %, unique count, min/max/avg) for a tabular file."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "folder_path":   { "type": "string" },
                "relative_path": { "type": "string" }
            },
            "required": ["folder_path", "relative_path"]
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let (folder_path, relative_path) = resolve_path(&ctx.args)?;
        let profile = crate::commands::profile_dataset(folder_path, relative_path)
            .await
            .map_err(|e| e.to_string())?;
        let value = serde_json::to_value(&profile).map_err(|e| e.to_string())?;
        Ok(json!({ "columns": value }))
    }
}

// ── files.rows ─────────────────────────────────────────────────────────────

pub struct ReadRowsCommand;

#[async_trait]
impl AgentCommand for ReadRowsCommand {
    fn name(&self) -> &'static str { "files.rows" }
    fn description(&self) -> &'static str {
        "Return rows from a local tabular file with an optional client-side filter."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "folder_path":   { "type": "string" },
                "relative_path": { "type": "string" },
                "limit":         { "type": "integer", "description": "Max rows (default 5000)" }
            },
            "required": ["folder_path", "relative_path"]
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let (folder_path, relative_path) = resolve_path(&ctx.args)?;
        let result = crate::commands::read_dataset_rows(folder_path, relative_path)
            .await
            .map_err(|e| e.to_string())?;
        let value = serde_json::to_value(&result).map_err(|e| e.to_string())?;
        Ok(value)
    }
}

// ── files.convert ──────────────────────────────────────────────────────────

pub struct ConvertFileCommand;

#[async_trait]
impl AgentCommand for ConvertFileCommand {
    fn name(&self) -> &'static str { "files.convert" }
    fn description(&self) -> &'static str {
        "Convert a CSV/TSV/Excel file to Parquet next to the source."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "folder_path":   { "type": "string" },
                "relative_path": { "type": "string" }
            },
            "required": ["folder_path", "relative_path"]
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let (folder_path, relative_path) = resolve_path(&ctx.args)?;
        let result = crate::commands::convert_to_parquet(folder_path, relative_path)
            .await
            .map_err(|e| e.to_string())?;
        let value = serde_json::to_value(&result).map_err(|e| e.to_string())?;
        Ok(value)
    }
}

// ── files.rich_metadata ────────────────────────────────────────────────────
//
// Returns filesystem facts + type-specific metadata for any file:
//   - all files:  name, extension, size_bytes, modified, created, abs_path
//   - images:     width, height, EXIF datetime, GPS lat/lon, camera make/model,
//                 ISO, aperture (f-number), shutter speed, orientation
//   - archives:   file_count (zip only for now), uncompressed_size_bytes
//
// The caller (UI) decides which fields to render; unknown fields are ignored.

pub struct RichMetadataCommand;

#[async_trait]
impl AgentCommand for RichMetadataCommand {
    fn name(&self) -> &'static str { "files.rich_metadata" }
    fn description(&self) -> &'static str {
        "Return filesystem metadata + type-specific metadata (EXIF for images, \
         file listing for archives)."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "folder_path":   { "type": "string" },
                "relative_path": { "type": "string" }
            },
            "required": ["folder_path", "relative_path"]
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let (folder_path, relative_path) = resolve_path(&ctx.args)?;
        tokio::task::spawn_blocking(move || {
            rich_metadata_sync(&folder_path, &relative_path)
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

// ── files.rescan_dataset ────────────────────────────────────────────────────

pub struct RescanDatasetCommand;

#[async_trait]
impl AgentCommand for RescanDatasetCommand {
    fn name(&self) -> &'static str { "files.rescan_dataset" }
    fn description(&self) -> &'static str {
        "Re-scan a single dataset (local file or remote URL), refresh its metadata \
         including sample_rows, and sync the result back to the cloud. \
         Returns the updated DatasetMetadata."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query_path": {
                    "type": "string",
                    "description": "Full path or URL of the dataset to re-scan \
                                    (e.g. s3://bucket/file.csv or /Users/foo/data.csv)"
                },
                "creds_source": {
                    "type": "string",
                    "description": "Optional: S3 listing URL used as the keyring key \
                                    when query_path is a per-object URL under a prefix."
                }
            },
            "required": ["query_path"]
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let query_path = ctx.args["query_path"]
            .as_str()
            .ok_or("missing query_path")?
            .to_string();

        // For remote URLs (s3://, https://, sftp://, …) re-run the remote
        // scanner which collects schema + row_count + sample_rows in one pass.
        // For local paths fall through to reextract_file which already handles
        // the full extraction pipeline.
        if crate::url::is_remote_url(&query_path) {
            let creds_source = ctx.args["creds_source"]
                .as_str()
                .unwrap_or(&query_path)
                .to_string();

            let url = query_path.clone();
            let metadata = tokio::task::spawn_blocking(move || -> Result<crate::scanner::DatasetMetadata, String> {
                // HEAD probe gives us content-type + size — same as the regular
                // remote scanner path. Use block_on since we're already in a
                // blocking thread.
                let rt = tokio::runtime::Handle::current();
                let head = rt.block_on(crate::remote::head_probe(&url))
                    .map_err(|e| e.to_string())?;
                crate::remote::scan_remote_blocking_with_creds(&url, &head, &creds_source)
                    .map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())??;

            // Sync to cloud so API picks up the fresh sample_rows.
            let config = crate::config::Config::load().map_err(|e| e.to_string())?;
            if let Ok(token) = crate::keyring_store::get_token() {
                let _ = crate::scanner::sync_metadata_to_cloud(
                    &config.cloud.api_url,
                    &token,
                    Some(&query_path),
                    None,
                    vec![metadata.clone()],
                ).await;
            }

            return serde_json::to_value(&metadata).map_err(|e| e.to_string());
        }

        // Local file — reuse the existing reextract_file path which handles
        // CSV → Parquet conversion, markdown extraction, PII redaction, etc.
        let folder_path;
        let relative_path;
        {
            let config = crate::config::Config::load().map_err(|e| e.to_string())?;
            let abs = if query_path.starts_with('/') {
                query_path.clone()
            } else {
                format!("/{query_path}")
            };
            let resolved = config.watched_folders.iter().find_map(|f| {
                let root = f.path.trim_end_matches('/');
                let prefix = format!("{root}/");
                if abs.starts_with(&prefix) {
                    Some((f.path.clone(), abs[prefix.len()..].to_string()))
                } else {
                    None
                }
            });
            let (fp, rp) = resolved.ok_or_else(|| format!("path not in any watched folder: {query_path}"))?;
            folder_path = fp;
            relative_path = rp;
        }

        let fp2 = folder_path.clone();
        let rp2 = relative_path.clone();
        let metadata = tokio::task::spawn_blocking(move || {
            crate::scanner::reextract_file(&fp2, &rp2).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;

        // Sync to cloud.
        let config = crate::config::Config::load().map_err(|e| e.to_string())?;
        if let Ok(token) = crate::keyring_store::get_token() {
            let _ = crate::scanner::sync_metadata_to_cloud(
                &config.cloud.api_url,
                &token,
                Some(&folder_path),
                None,
                vec![metadata.clone()],
            ).await;
        }

        Ok(serde_json::to_value(&metadata).map_err(|e| e.to_string())?)
    }
}

fn rich_metadata_sync(folder_path: &str, relative_path: &str) -> Result<Value, String> {
    use std::path::Path;

    let abs = Path::new(folder_path).join(relative_path);
    let fs_meta = std::fs::metadata(&abs).map_err(|e| e.to_string())?;

    let size_bytes = fs_meta.len();
    let modified   = fs_meta.modified().ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let created    = fs_meta.created().ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    let ext = abs.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    let name = abs.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();

    let mut meta = json!({
        "name":       name,
        "extension":  ext,
        "abs_path":   abs.to_string_lossy(),
        "size_bytes": size_bytes,
        "modified":   modified,
        "created":    created,
    });

    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "tiff" | "bmp" | "heic" | "avif" | "ico" => {
            // Dimensions (fast — reads only the header, not the full image)
            if let Ok(size) = imagesize::size(&abs) {
                meta["width"]  = json!(size.width);
                meta["height"] = json!(size.height);
            }
            // EXIF (JPEG / TIFF only; other formats silently skip)
            if let Ok(file) = std::fs::File::open(&abs) {
                let mut reader = std::io::BufReader::new(file);
                if let Ok(exif) = exif::Reader::new().read_from_container(&mut reader) {
                    let get = |tag: exif::Tag| -> Option<String> {
                        exif.get_field(tag, exif::In::PRIMARY)
                            .map(|f| f.display_value().with_unit(&exif).to_string())
                    };
                    let get_rational = |tag: exif::Tag| -> Option<f64> {
                        exif.get_field(tag, exif::In::PRIMARY).and_then(|f| {
                            match f.value {
                                exif::Value::Rational(ref v) if !v.is_empty() =>
                                    Some(v[0].num as f64 / v[0].denom as f64),
                                _ => None,
                            }
                        })
                    };

                    if let Some(v) = get(exif::Tag::DateTimeOriginal)
                        .or_else(|| get(exif::Tag::DateTime))
                    {
                        meta["exif_datetime"] = json!(v);
                    }
                    if let Some(v) = get(exif::Tag::Make)        { meta["camera_make"]  = json!(v); }
                    if let Some(v) = get(exif::Tag::Model)       { meta["camera_model"] = json!(v); }
                    if let Some(v) = get(exif::Tag::PhotographicSensitivity)
                        .or_else(|| get(exif::Tag::ISOSpeed))
                    {
                        meta["iso"] = json!(v);
                    }
                    if let Some(v) = get(exif::Tag::FNumber)      { meta["aperture"]      = json!(v); }
                    if let Some(v) = get(exif::Tag::ExposureTime) { meta["shutter_speed"] = json!(v); }
                    if let Some(v) = get(exif::Tag::FocalLength)  { meta["focal_length"]  = json!(v); }
                    if let Some(v) = get(exif::Tag::Orientation)  { meta["orientation"]   = json!(v); }
                    if let Some(v) = get(exif::Tag::LensModel)    { meta["lens_model"]    = json!(v); }

                    // GPS: decode from DMS rationals → decimal degrees
                    let lat = decode_gps_dms(&exif, exif::Tag::GPSLatitude,    exif::Tag::GPSLatitudeRef);
                    let lon = decode_gps_dms(&exif, exif::Tag::GPSLongitude,   exif::Tag::GPSLongitudeRef);
                    let alt = get_rational(exif::Tag::GPSAltitude);

                    if lat.is_some() || lon.is_some() {
                        meta["gps"] = json!({
                            "latitude":  lat,
                            "longitude": lon,
                            "altitude":  alt,
                        });
                    }
                }
            }
        }

        "zip" => {
            if let Ok(file) = std::fs::File::open(&abs) {
                if let Ok(archive) = zip::ZipArchive::new(file) {
                    let file_count = archive.len();
                    let uncompressed: u64 = (0..file_count)
                        .filter_map(|i| {
                            // ZipArchive::by_index takes &mut self; re-open each time is expensive
                            // but zip 0.6 doesn't expose a way to iterate without mut.
                            // For large archives this is still fast because we only read the
                            // central directory (no decompression).
                            let mut a = zip::ZipArchive::new(
                                std::fs::File::open(&abs).ok()?
                            ).ok()?;
                            a.by_index(i).ok().map(|f| f.size())
                        })
                        .sum();
                    meta["file_count"]          = json!(file_count);
                    meta["uncompressed_bytes"]  = json!(uncompressed);
                }
            }
        }

        _ => {}
    }

    Ok(meta)
}

/// Convert GPS DMS rationals + reference string (N/S or E/W) to decimal degrees.
fn decode_gps_dms(exif: &exif::Exif, dms_tag: exif::Tag, ref_tag: exif::Tag) -> Option<f64> {
    let dms_field = exif.get_field(dms_tag, exif::In::PRIMARY)?;
    let ref_str   = exif.get_field(ref_tag, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string())
        .unwrap_or_default();

    let rationals = match &dms_field.value {
        exif::Value::Rational(v) if v.len() >= 3 => v,
        _ => return None,
    };

    let deg = rationals[0].num as f64 / rationals[0].denom as f64;
    let min = rationals[1].num as f64 / rationals[1].denom as f64;
    let sec = rationals[2].num as f64 / rationals[2].denom as f64;
    let dd  = deg + min / 60.0 + sec / 3600.0;

    if ref_str.contains('S') || ref_str.contains('W') { Some(-dd) } else { Some(dd) }
}

use crate::agent_rpc::registry::{AgentCommand, Ctx};
use async_trait::async_trait;
use serde_json::{json, Value};

/// Accept either `{ query_path }` (dashboard style) or `{ folder_path, relative_path }`
/// (Sery Link UI style) and return `(folder_path, relative_path)`.
fn resolve_path(args: &Value) -> Result<(String, String), String> {
    if let Some(qp) = args["query_path"].as_str() {
        // Strip local://agent_id/ prefix. The remainder is either:
        //   "Users/me/docs/file.pdf"          → local abs path, prepend /
        //   "s3://bucket/path/file.parquet"   → remote URL, keep as-is
        let inner = qp.splitn(4, '/').nth(3).unwrap_or(qp);
        let abs = if inner.contains("://") {
            inner.to_string()
        } else if inner.starts_with('/') {
            inner.to_string()
        } else {
            format!("/{inner}")
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
                "query_path": { "type": "string", "description": "query_path from the dataset (local://agent_id/...)" }
            },
            "required": ["query_path"]
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let query_path = ctx.args["query_path"].as_str().ok_or("missing query_path")?.to_string();

        // Strip local://agent_id/ prefix → remainder is the absolute path minus
        // its leading slash (e.g. "Users/hepang/Documents/file.pdf").
        // Re-add the slash to get the real absolute path.
        let inner_rel = query_path
            .splitn(4, '/')
            .nth(3)
            .unwrap_or(&query_path);
        let abs_path = if inner_rel.starts_with('/') {
            inner_rel.to_string()
        } else {
            format!("/{inner_rel}")
        };

        // Find which watched folder this absolute path lives under.
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

        // If the extraction returned nothing, surface it as an error so the
        // dashboard can show the user a useful message instead of silently
        // rendering a blank content panel.
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

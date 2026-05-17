use crate::agent_rpc::registry::{AgentCommand, Ctx};
use async_trait::async_trait;
use serde_json::{json, Value};

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

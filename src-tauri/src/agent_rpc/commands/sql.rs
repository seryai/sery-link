use crate::agent_rpc::registry::{AgentCommand, Ctx, Progress};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct ExecSqlCommand;

#[async_trait]
impl AgentCommand for ExecSqlCommand {
    fn name(&self) -> &'static str { "sql.exec" }
    fn description(&self) -> &'static str {
        "Execute a SQL query. For file sources pass `path` (file path or query_path). \
         For database sources pass `path` as `db://<source_id>`."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "sql":   { "type": "string", "description": "SQL query to execute" },
                "path":  {
                    "type": "string",
                    "description": "File path / query_path for file sources, or db://<source_id> for database sources"
                },
                "limit": { "type": "integer", "description": "Row cap override (max 10 000)" }
            },
            "required": ["sql", "path"]
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let sql   = ctx.args["sql"].as_str().ok_or("missing sql")?.to_string();
        let path  = ctx.args["path"].as_str().ok_or("missing path")?.to_string();
        let limit = ctx.args["limit"].as_u64().unwrap_or(10_000).min(10_000);

        let _ = ctx.progress.send(Progress {
            data: json!({ "phase": "executing" }),
        }).await;

        // ── Database source fast-path ─────────────────────────────────────
        // path = "db://<source_id>" → route to DuckDB mysql/postgres engine.
        if let Some(source_id) = path.strip_prefix("db://") {
            let config = crate::config::Config::load().map_err(|e| e.to_string())?;
            let result = crate::db_engine::execute_db_query(&sql, source_id, &config)
                .await
                .map_err(|e| e.to_string())?;
            return Ok(json!({
                "rows":        result.rows,
                "columns":     result.columns,
                "row_count":   result.row_count,
                "duration_ms": result.duration_ms,
                "truncated":   result.truncated,
            }));
        }

        // ── File source path (existing behaviour) ─────────────────────────
        let config = crate::config::Config::load().map_err(|e| e.to_string())?;

        let capped_sql = if sql.to_lowercase().contains("limit") {
            sql.clone()
        } else {
            format!("SELECT * FROM ({sql}) _q LIMIT {limit}")
        };

        let result = crate::duckdb_engine::execute_query(&capped_sql, &path, &config)
            .await
            .map_err(|e| e.to_string())?;

        Ok(json!({
            "rows":        result.rows,
            "columns":     result.columns,
            "row_count":   result.row_count,
            "duration_ms": result.duration_ms,
        }))
    }
}

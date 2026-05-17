use crate::agent_rpc::registry::{AgentCommand, Ctx, Progress};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct ExecSqlCommand;

#[async_trait]
impl AgentCommand for ExecSqlCommand {
    fn name(&self) -> &'static str { "sql.exec" }
    fn description(&self) -> &'static str {
        "Execute a SQL query against a local file via DuckDB. \
         Returns rows as JSON. Capped at 10 000 rows."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "sql":   { "type": "string", "description": "SQL query to execute" },
                "path":  { "type": "string", "description": "File path or query_path context" },
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

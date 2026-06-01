use crate::agent_rpc::registry::{AgentCommand, Ctx};
use crate::sources::SourceKind;
use async_trait::async_trait;
use serde_json::{json, Value};

fn kind_str(kind: &SourceKind) -> &'static str {
    match kind {
        SourceKind::Local { .. }      => "local",
        SourceKind::S3 { .. }         => "s3",
        SourceKind::Https { .. }      => "https",
        SourceKind::GoogleDrive { .. }=> "google_drive",
        SourceKind::Sftp { .. }       => "sftp",
        SourceKind::WebDav { .. }     => "web_dav",
        SourceKind::Dropbox { .. }    => "dropbox",
        SourceKind::AzureBlob { .. }  => "azure_blob",
        SourceKind::OneDrive { .. }    => "one_drive",
        SourceKind::Mysql { .. }       => "mysql",
        SourceKind::Postgresql { .. }  => "postgresql",
        SourceKind::Snowflake { .. }   => "snowflake",
        SourceKind::Clickhouse { .. }  => "clickhouse",
        SourceKind::Mongodb { .. }     => "mongodb",
        SourceKind::Redis { .. }       => "redis",
        SourceKind::Sqlite { .. }      => "sqlite",
        SourceKind::AgentDb { .. }     => "agent_db",
    }
}

// ── sources.list ───────────────────────────────────────────────────────────

pub struct ListSourcesCommand;

#[async_trait]
impl AgentCommand for ListSourcesCommand {
    fn name(&self) -> &'static str { "sources.list" }
    fn description(&self) -> &'static str { "List all configured sources with scan stats." }
    fn schema(&self) -> Value {
        json!({ "type": "object", "properties": {}, "required": [] })
    }

    async fn execute(&self, _ctx: Ctx) -> Result<Value, String> {
        let config = crate::config::Config::load().map_err(|e| e.to_string())?;
        let sources: Vec<Value> = config.sources.iter().map(|s| {
            json!({
                "id":            s.id,
                "name":          s.name,
                "kind":          kind_str(&s.kind),
                "mcp_enabled":   s.mcp_enabled,
                "last_scan_at":  s.last_scan_at,
                "last_scan_stats": s.last_scan_stats,
                "sort_order":    s.sort_order,
                "group":         s.group,
            })
        }).collect();
        Ok(json!({ "sources": sources }))
    }
}

// ── sources.scan ───────────────────────────────────────────────────────────

pub struct ScanSourceCommand;

#[async_trait]
impl AgentCommand for ScanSourceCommand {
    fn name(&self) -> &'static str { "sources.scan" }
    fn description(&self) -> &'static str {
        "Trigger a rescan of a source. Streams progress events during the scan."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "source_id": { "type": "string", "description": "Source UUID" }
            },
            "required": ["source_id"]
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let source_id = ctx.args["source_id"]
            .as_str()
            .ok_or("missing source_id")?
            .to_string();

        let app = ctx.app.ok_or("AppHandle required for sources.scan")?;

        let _ = ctx.progress.send(crate::agent_rpc::registry::Progress {
            data: json!({ "phase": "starting", "source_id": source_id }),
        }).await;

        let result = crate::commands::rescan_source_by_id(app, source_id).await?;

        Ok(result)
    }
}

// ── sources.rename ─────────────────────────────────────────────────────────

pub struct RenameSourceCommand;

#[async_trait]
impl AgentCommand for RenameSourceCommand {
    fn name(&self) -> &'static str { "sources.rename" }
    fn description(&self) -> &'static str { "Rename a source." }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "source_id": { "type": "string" },
                "name":      { "type": "string" }
            },
            "required": ["source_id", "name"]
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let source_id = ctx.args["source_id"].as_str().ok_or("missing source_id")?.to_string();
        let name      = ctx.args["name"].as_str().ok_or("missing name")?.to_string();
        crate::commands::rename_source(source_id.clone(), name.clone()).await?;
        Ok(json!({ "source_id": source_id, "name": name }))
    }
}

// ── sources.remove ─────────────────────────────────────────────────────────

pub struct RemoveSourceCommand;

#[async_trait]
impl AgentCommand for RemoveSourceCommand {
    fn name(&self) -> &'static str { "sources.remove" }
    fn description(&self) -> &'static str { "Remove a source." }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "source_id": { "type": "string" }
            },
            "required": ["source_id"]
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let source_id = ctx.args["source_id"].as_str().ok_or("missing source_id")?.to_string();
        crate::commands::remove_source(source_id.clone()).await?;
        Ok(json!({ "removed": source_id }))
    }
}

// ── sources.status ─────────────────────────────────────────────────────────

pub struct SourceStatusCommand;

#[async_trait]
impl AgentCommand for SourceStatusCommand {
    fn name(&self) -> &'static str { "sources.status" }
    fn description(&self) -> &'static str { "Get the current scan status of a source." }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "source_id": { "type": "string" }
            },
            "required": ["source_id"]
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let source_id = ctx.args["source_id"].as_str().ok_or("missing source_id")?;
        let config = crate::config::Config::load().map_err(|e| e.to_string())?;
        let source = config.sources.iter()
            .find(|s| s.id == source_id)
            .ok_or_else(|| format!("source {source_id:?} not found"))?;
        Ok(json!({
            "source_id":       source.id,
            "name":            source.name,
            "last_scan_at":    source.last_scan_at,
            "last_scan_stats": source.last_scan_stats,
        }))
    }
}

// ── sources.add ────────────────────────────────────────────────────────────

pub struct AddSourceCommand;

#[async_trait]
impl AgentCommand for AddSourceCommand {
    fn name(&self) -> &'static str { "sources.add" }
    fn description(&self) -> &'static str { "Add a local folder as a watched source." }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute path to the folder" }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let path = ctx.args["path"].as_str().ok_or("missing path")?.to_string();
        crate::commands::add_watched_folder(path.clone()).await?;
        Ok(json!({ "added": path }))
    }
}

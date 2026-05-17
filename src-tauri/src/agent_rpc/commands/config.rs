use crate::agent_rpc::registry::{AgentCommand, Ctx};
use async_trait::async_trait;
use serde_json::{json, Value};

// ── config.get ─────────────────────────────────────────────────────────────

pub struct GetConfigCommand;

#[async_trait]
impl AgentCommand for GetConfigCommand {
    fn name(&self) -> &'static str { "config.get" }
    fn description(&self) -> &'static str {
        "Return the current agent configuration. Credentials are never included."
    }
    fn schema(&self) -> Value {
        json!({ "type": "object", "properties": {}, "required": [] })
    }

    async fn execute(&self, _ctx: Ctx) -> Result<Value, String> {
        let config = crate::config::Config::load().map_err(|e| e.to_string())?;
        Ok(json!({
            "agent": {
                "name":         config.agent.name,
                "platform":     config.agent.platform,
                "workspace_id": config.agent.workspace_id,
            },
            "sync": {
                "scan_interval_minutes":  config.sync.auto_scan_interval_minutes,
                "include_document_text":  config.sync.include_document_text,
                "scan_tier_overrides":    config.sync.scan_tier_overrides,
            },
            "app": {
                "theme":                 config.app.theme,
                "launch_at_login":       config.app.launch_at_login,
                "notifications_enabled": config.app.notifications_enabled,
            },
        }))
    }
}

// ── config.set ─────────────────────────────────────────────────────────────

pub struct SetConfigCommand;

#[async_trait]
impl AgentCommand for SetConfigCommand {
    fn name(&self) -> &'static str { "config.set" }
    fn description(&self) -> &'static str {
        "Update writable config fields. Only the supplied fields are changed."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "scan_interval_minutes": {
                    "type": ["integer", "null"],
                    "description": "Auto-scan interval in minutes. null disables."
                },
                "include_document_text": {
                    "type": "boolean",
                    "description": "Upload extracted document text to the cloud catalog."
                },
                "notifications_enabled": {
                    "type": "boolean"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let mut config = crate::config::Config::load().map_err(|e| e.to_string())?;
        let mut changed = false;

        if let Some(v) = ctx.args.get("scan_interval_minutes") {
            config.sync.auto_scan_interval_minutes = v.as_u64().map(|n| n as u32);
            changed = true;
        }
        if let Some(v) = ctx.args["include_document_text"].as_bool() {
            config.sync.include_document_text = v;
            changed = true;
        }
        if let Some(v) = ctx.args["notifications_enabled"].as_bool() {
            config.app.notifications_enabled = v;
            changed = true;
        }

        if changed {
            config.save().map_err(|e| e.to_string())?;
        }

        Ok(json!({ "saved": changed }))
    }
}

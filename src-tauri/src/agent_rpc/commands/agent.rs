use crate::agent_rpc::registry::{AgentCommand, Ctx, REGISTRY};
use async_trait::async_trait;
use serde_json::{json, Value};

// ── agent.info ─────────────────────────────────────────────────────────────

pub struct AgentInfoCommand;

#[async_trait]
impl AgentCommand for AgentInfoCommand {
    fn name(&self) -> &'static str { "agent.info" }
    fn description(&self) -> &'static str {
        "Return machine name, agent ID, workspace, OS, and Sery Link version."
    }
    fn schema(&self) -> Value {
        json!({ "type": "object", "properties": {}, "required": [] })
    }

    async fn execute(&self, _ctx: Ctx) -> Result<Value, String> {
        let config = crate::config::Config::load().map_err(|e| e.to_string())?;
        Ok(json!({
            "name":         config.agent.name,
            "agent_id":     config.agent.agent_id,
            "machine_id":   config.agent.machine_id,
            "workspace_id": config.agent.workspace_id,
            "platform":     config.agent.platform,
            "hostname":     config.agent.hostname,
            "version":      env!("CARGO_PKG_VERSION"),
            "os":           std::env::consts::OS,
            "arch":         std::env::consts::ARCH,
        }))
    }
}

// ── agent.ping ─────────────────────────────────────────────────────────────

pub struct PingCommand;

#[async_trait]
impl AgentCommand for PingCommand {
    fn name(&self) -> &'static str { "agent.ping" }
    fn description(&self) -> &'static str { "Health check. Returns pong + timestamp." }
    fn schema(&self) -> Value {
        json!({ "type": "object", "properties": {}, "required": [] })
    }

    async fn execute(&self, _ctx: Ctx) -> Result<Value, String> {
        Ok(json!({
            "pong": true,
            "ts":   chrono::Utc::now().to_rfc3339(),
            "version": env!("CARGO_PKG_VERSION"),
        }))
    }
}

// ── agent.commands ─────────────────────────────────────────────────────────

/// Returns the full command manifest so the dashboard can discover what
/// this Sery Link instance supports — name, description, JSON Schema.
/// Mirrors MCP's tool listing pattern.
pub struct ListCommandsCommand;

#[async_trait]
impl AgentCommand for ListCommandsCommand {
    fn name(&self) -> &'static str { "agent.commands" }
    fn description(&self) -> &'static str {
        "Return the full manifest of all commands supported by this agent."
    }
    fn schema(&self) -> Value {
        json!({ "type": "object", "properties": {}, "required": [] })
    }

    async fn execute(&self, _ctx: Ctx) -> Result<Value, String> {
        Ok(REGISTRY.manifest())
    }
}

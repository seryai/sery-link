use async_trait::async_trait;
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

/// One progress event sent back to the caller while a command runs.
#[derive(Clone, Debug)]
pub struct Progress {
    pub data: Value,
}

pub type ProgressTx = mpsc::Sender<Progress>;

/// Execution context passed to every command.
pub struct Ctx {
    pub args: Value,
    pub progress: ProgressTx,
    /// Present for commands that need to emit Tauri events to the local UI.
    pub app: Option<tauri::AppHandle<tauri::Wry>>,
}

/// Every remotely-invocable command implements this trait.
#[async_trait]
pub trait AgentCommand: Send + Sync {
    /// Dot-namespaced name, e.g. `"sources.scan"`.
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    /// JSON Schema object describing `args`. Used by the dashboard for
    /// discovery and by the API to validate input before forwarding.
    fn schema(&self) -> Value;
    async fn execute(&self, ctx: Ctx) -> Result<Value, String>;
}

pub struct CommandRegistry {
    commands: HashMap<&'static str, Arc<dyn AgentCommand>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self { commands: HashMap::new() }
    }

    pub fn register(&mut self, cmd: impl AgentCommand + 'static) {
        self.commands.insert(cmd.name(), Arc::new(cmd));
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn AgentCommand>> {
        self.commands.get(name).cloned()
    }

    /// Returns the full command manifest — name, description, schema.
    /// Sent back on `agent.commands` invocation.
    pub fn manifest(&self) -> Value {
        let list: Vec<Value> = self.commands.values().map(|cmd| {
            json!({
                "name":        cmd.name(),
                "description": cmd.description(),
                "schema":      cmd.schema(),
            })
        }).collect();
        json!({ "commands": list })
    }
}

// ── Global registry ────────────────────────────────────────────────────────

pub static REGISTRY: Lazy<CommandRegistry> = Lazy::new(|| {
    let mut r = CommandRegistry::new();

    // sources.*
    r.register(super::commands::sources::ListSourcesCommand);
    r.register(super::commands::sources::ScanSourceCommand);
    r.register(super::commands::sources::RenameSourceCommand);
    r.register(super::commands::sources::RemoveSourceCommand);
    r.register(super::commands::sources::SourceStatusCommand);
    r.register(super::commands::sources::AddSourceCommand);

    // files.*
    r.register(super::commands::files::ListFilesCommand);
    r.register(super::commands::files::PreviewFileCommand);
    r.register(super::commands::files::FileSchemaCommand);
    r.register(super::commands::files::ExtractFileCommand);
    r.register(super::commands::files::GetCachedMetadataCommand);
    r.register(super::commands::files::ProfileFileCommand);
    r.register(super::commands::files::ReadRowsCommand);
    r.register(super::commands::files::ConvertFileCommand);
    r.register(super::commands::files::RichMetadataCommand);

    // sql.*
    r.register(super::commands::sql::ExecSqlCommand);

    // config.*
    r.register(super::commands::config::GetConfigCommand);
    r.register(super::commands::config::SetConfigCommand);

    // system.*
    r.register(super::commands::system::SystemInfoCommand);
    r.register(super::commands::system::ShowNotificationCommand);
    r.register(super::commands::system::OpenPathCommand);
    r.register(super::commands::system::TailLogsCommand);

    // agent.*
    r.register(super::commands::agent::AgentInfoCommand);
    r.register(super::commands::agent::PingCommand);
    r.register(super::commands::agent::ListCommandsCommand);

    r
});

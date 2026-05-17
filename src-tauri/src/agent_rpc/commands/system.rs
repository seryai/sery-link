use crate::agent_rpc::registry::{AgentCommand, Ctx};
use async_trait::async_trait;
use serde_json::{json, Value};

// ── system.info ────────────────────────────────────────────────────────────

pub struct SystemInfoCommand;

#[async_trait]
impl AgentCommand for SystemInfoCommand {
    fn name(&self) -> &'static str { "system.info" }
    fn description(&self) -> &'static str {
        "Return OS, CPU, memory, and disk info for this machine."
    }
    fn schema(&self) -> Value {
        json!({ "type": "object", "properties": {}, "required": [] })
    }

    async fn execute(&self, _ctx: Ctx) -> Result<Value, String> {
        let storage = crate::commands::get_storage_info().await?;

        Ok(json!({
            "os":      std::env::consts::OS,
            "arch":    std::env::consts::ARCH,
            "version": env!("CARGO_PKG_VERSION"),
            "storage": {
                "gdrive_bytes":   storage.gdrive_cache_bytes,
                "data_dir_bytes": storage.data_dir_bytes,
                "free_bytes":     storage.free_bytes,
            }
        }))
    }
}

// ── system.notify ──────────────────────────────────────────────────────────

pub struct ShowNotificationCommand;

#[async_trait]
impl AgentCommand for ShowNotificationCommand {
    fn name(&self) -> &'static str { "system.notify" }
    fn description(&self) -> &'static str {
        "Show an OS notification on this machine. \
         Useful for alerting the user to a completed job or dashboard message."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "body":  { "type": "string" },
                "level": {
                    "type": "string",
                    "enum": ["info", "success", "warning", "error"],
                    "description": "Visual level — maps to notification icon (default: info)"
                }
            },
            "required": ["title", "body"]
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let title = ctx.args["title"].as_str().ok_or("missing title")?.to_string();
        let body  = ctx.args["body"].as_str().ok_or("missing body")?.to_string();
        let level = ctx.args["level"].as_str().unwrap_or("info").to_string();

        // Emit to the local Tauri frontend (shows a toast in the app UI).
        if let Some(app) = &ctx.app {
            use tauri::Emitter;
            let _ = app.emit("remote-notification", json!({
                "title": title,
                "body":  body,
                "level": level,
            }));
        }

        // Also fire an OS-level notification if enabled in config.
        if crate::config::Config::load()
            .map(|c| c.app.notifications_enabled)
            .unwrap_or(true)
        {
            #[cfg(not(test))]
            {
                use tauri_plugin_notification::NotificationExt;
                if let Some(app) = &ctx.app {
                    let _ = app.notification()
                        .builder()
                        .title(&title)
                        .body(&body)
                        .show();
                }
            }
        }

        Ok(json!({ "sent": true, "title": title }))
    }
}

// ── system.open ────────────────────────────────────────────────────────────

pub struct OpenPathCommand;

#[async_trait]
impl AgentCommand for OpenPathCommand {
    fn name(&self) -> &'static str { "system.open" }
    fn description(&self) -> &'static str {
        "Open a file or folder in the OS file manager (Finder / Explorer)."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute path to open" }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let path = ctx.args["path"].as_str().ok_or("missing path")?.to_string();

        // Security: only open paths that are inside a configured source.
        let config = crate::config::Config::load().map_err(|e| e.to_string())?;
        let allowed = config.sources.iter().any(|s| {
            if let crate::sources::SourceKind::Local { path: src_path, .. } = &s.kind {
                path.starts_with(&src_path.to_string_lossy().as_ref())
            } else {
                false
            }
        });
        if !allowed {
            return Err(format!("path {path:?} is not inside a configured local source"));
        }

        #[cfg(target_os = "macos")]
        std::process::Command::new("open").arg(&path).spawn().map_err(|e| e.to_string())?;
        #[cfg(target_os = "windows")]
        std::process::Command::new("explorer").arg(&path).spawn().map_err(|e| e.to_string())?;
        #[cfg(target_os = "linux")]
        std::process::Command::new("xdg-open").arg(&path).spawn().map_err(|e| e.to_string())?;

        Ok(json!({ "opened": path }))
    }
}

// ── system.logs ────────────────────────────────────────────────────────────

pub struct TailLogsCommand;

#[async_trait]
impl AgentCommand for TailLogsCommand {
    fn name(&self) -> &'static str { "system.logs" }
    fn description(&self) -> &'static str {
        "Return the most recent N lines from the Sery Link log file."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "lines": { "type": "integer", "description": "Number of lines to return (default 100, max 500)" }
            },
            "required": []
        })
    }

    async fn execute(&self, ctx: Ctx) -> Result<Value, String> {
        let n = ctx.args["lines"].as_u64().unwrap_or(100).min(500) as usize;

        let log_path = crate::config::Config::data_dir()
            .map(|d| d.join("sery-link.log"))
            .map_err(|e| e.to_string())?;
        let content = std::fs::read_to_string(&log_path)
            .unwrap_or_else(|_| String::new());

        let lines: Vec<&str> = content.lines().rev().take(n).collect();
        let lines: Vec<&str> = lines.into_iter().rev().collect();

        Ok(json!({
            "lines": lines,
            "path":  log_path.to_string_lossy(),
        }))
    }
}

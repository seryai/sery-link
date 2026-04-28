//! Model Context Protocol (MCP) stdio server mode.
//!
//! When `sery-link` is invoked with `--mcp-stdio --root <dir>`, this
//! module takes over: instead of starting the Tauri GUI, it runs a
//! stdio MCP server exposing the given folder to whatever LLM client
//! spawned us (Claude Desktop, Cursor, Zed, Continue, …).
//!
//! ## Why in-process
//!
//! The same binary doing two jobs (GUI app **or** MCP server, by
//! flag) keeps the install footprint small — one binary, one
//! auto-update channel, one bundled `libpdfium` + `pandoc`. Users who
//! enable the MCP toggle in Settings get the AI bridge without
//! installing anything else.
//!
//! ## Why stdio
//!
//! Stdio is the canonical MCP transport — every MCP client supports
//! it, no port conflicts, no auth needed (the LLM client spawned us
//! directly, so it's already trusted). Cloud-routed MCP (eventual
//! `mcp.sery.ai`) is a separate transport on top of the existing
//! WebSocket tunnel; this module is the local-first path.
//!
//! ## What runs
//!
//! [`sery_mcp::SeryMcpServer`] from the library crate published at
//! [crates.io/crates/sery-mcp](https://crates.io/crates/sery-mcp).
//! Same six tools, same JSON schemas, same path-traversal hardening
//! as the standalone `sery-mcp` binary. We just embed the library
//! and serve it over the stdio transport.

use std::path::PathBuf;

use rmcp::{transport::stdio, ServiceExt};
use sery_mcp::SeryMcpServer;

/// Boxed dyn error to keep the surface light without pulling in
/// `anyhow` for one helper. Any error propagates up to the
/// caller in `lib.rs::run()` where we log + exit.
type StdError = Box<dyn std::error::Error + Send + Sync>;

/// Run as a stdio MCP server until the LLM client closes our pipe
/// or we hit a fatal error.
///
/// **Critical invariant:** `stdout` is the MCP transport channel
/// (JSON-RPC frames). Anything we print there breaks the protocol —
/// every diagnostic in this module uses `eprintln!` (or `tracing` →
/// stderr if a subscriber is installed elsewhere).
pub fn run_stdio(root: PathBuf) -> Result<(), StdError> {
    // We're outside Tauri's runtime here, so spin up our own
    // multi-thread tokio runtime for rmcp's serve loop.
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|e| format!("failed to start tokio runtime: {e}"))?;

    runtime.block_on(async move {
        // Canonicalise once so all subsequent path validation against
        // `--root` matches what the user passed.
        let canonical = root
            .canonicalize()
            .map_err(|e| format!("--root {} is not readable: {e}", root.display()))?;

        // Direct stderr diagnostic — never write to stdout in this
        // mode (it's the JSON-RPC transport). rmcp's own internal
        // `tracing` events are dropped silently because we don't
        // install a subscriber here; that's deliberate, the user
        // doesn't need MCP-internals logging cluttering their
        // LLM-client UI.
        eprintln!(
            "sery-link MCP stdio server starting — sery-link v{}, sery-mcp v{}, root={}",
            env!("CARGO_PKG_VERSION"),
            sery_mcp::VERSION,
            canonical.display()
        );

        let server = SeryMcpServer::new(canonical);
        let service = server
            .serve(stdio())
            .await
            .map_err(|e| format!("rmcp serve failed: {e}"))?;

        service
            .waiting()
            .await
            .map_err(|e| format!("rmcp serve loop ended: {e}"))?;
        Ok::<(), StdError>(())
    })
}

/// Detect and parse `--mcp-stdio` invocation from CLI args. Returns
/// the resolved `--root` path when both flags are present, `None`
/// otherwise.
///
/// Recognised forms:
/// - `sery-link --mcp-stdio --root /path/to/folder`
/// - `sery-link --mcp-stdio --root=/path/to/folder`
///
/// Anything else (no `--mcp-stdio`, missing `--root`) returns `None`
/// and the caller falls through to Tauri startup.
pub fn parse_stdio_args(args: &[String]) -> Option<PathBuf> {
    if !args.iter().any(|a| a == "--mcp-stdio") {
        return None;
    }
    // --root <path>
    if let Some(idx) = args.iter().position(|a| a == "--root") {
        if let Some(path) = args.get(idx + 1) {
            return Some(PathBuf::from(path));
        }
    }
    // --root=<path>
    if let Some(arg) = args.iter().find(|a| a.starts_with("--root=")) {
        return Some(PathBuf::from(&arg["--root=".len()..]));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn parse_returns_none_without_mcp_stdio_flag() {
        assert!(parse_stdio_args(&args(&["sery-link"])).is_none());
        assert!(parse_stdio_args(&args(&["sery-link", "--root", "/tmp"])).is_none());
    }

    #[test]
    fn parse_returns_none_with_mcp_stdio_but_no_root() {
        assert!(parse_stdio_args(&args(&["sery-link", "--mcp-stdio"])).is_none());
    }

    #[test]
    fn parse_handles_separate_root_arg() {
        let parsed = parse_stdio_args(&args(&["sery-link", "--mcp-stdio", "--root", "/tmp"]));
        assert_eq!(parsed, Some(PathBuf::from("/tmp")));
    }

    #[test]
    fn parse_handles_equals_root_arg() {
        let parsed = parse_stdio_args(&args(&["sery-link", "--mcp-stdio", "--root=/tmp"]));
        assert_eq!(parsed, Some(PathBuf::from("/tmp")));
    }

    #[test]
    fn parse_tolerates_extra_args() {
        let parsed = parse_stdio_args(&args(&[
            "sery-link",
            "--verbose",
            "--mcp-stdio",
            "--root",
            "/Users/me/Documents",
            "--something-else",
        ]));
        assert_eq!(parsed, Some(PathBuf::from("/Users/me/Documents")));
    }
}

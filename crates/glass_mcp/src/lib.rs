//! glass_mcp — MCP server exposing Glass terminal capabilities to AI assistants.
//!
//! Provides 33 tools via the Model Context Protocol spanning history queries,
//! context summaries, undo/diff, tab/pane management, pipe inspection,
//! agent coordination, and scripting. All logging goes to stderr; stdout
//! carries only JSON-RPC messages.

pub mod context;
pub mod ipc_client;
pub mod tools;

use std::collections::HashSet;

use glass_core::config::{GlassConfig, PermissionMatrix};
use rmcp::ServiceExt;

/// Start the MCP server over stdio.
///
/// Resolves the Glass history database path and snapshot glass directory from
/// the current working directory, creates a `GlassServer`, and serves it over
/// stdin/stdout using JSON-RPC 2.0.
///
/// Loads `~/.glass/config.toml` to read `[agent].allowed_tools` and
/// `[agent.permissions]` for MCP tool gating.
///
/// # Errors
///
/// Returns an error if the server fails to start or encounters a fatal transport error.
pub async fn run_mcp_server() -> anyhow::Result<()> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let db_path = glass_history::resolve_db_path(&cwd);
    let glass_dir = glass_snapshot::resolve_glass_dir(&cwd);
    let coord_db_path = glass_coordination::resolve_db_path();
    tracing::info!(
        "MCP server starting, db_path={}, glass_dir={}, coord_db_path={}",
        db_path.display(),
        glass_dir.display(),
        coord_db_path.display()
    );

    // Load config for permission gating
    let config = GlassConfig::load();
    let (allowed_tools, permissions) = if let Some(ref agent) = config.agent {
        let tools_set: HashSet<String> = agent
            .allowed_tools
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let perms = agent.permissions.clone().unwrap_or_default();
        (tools_set, perms)
    } else {
        (HashSet::new(), PermissionMatrix::default())
    };
    tracing::info!(
        allowed_tools_count = allowed_tools.len(),
        run_commands = ?permissions.run_commands,
        edit_files = ?permissions.edit_files,
        "MCP permission config loaded"
    );

    // Always create the IPC client -- it handles connection failures lazily
    // (returns clear error messages when the GUI isn't running).
    let ipc_client = Some(ipc_client::IpcClient::new());

    let server = tools::GlassServer::new(
        db_path,
        glass_dir,
        coord_db_path,
        ipc_client,
        allowed_tools,
        permissions,
    );
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

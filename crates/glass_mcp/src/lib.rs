//! glass_mcp — MCP server exposing Glass terminal history to AI assistants.
//!
//! Provides four tools via the Model Context Protocol:
//! - **GlassHistory**: Query command history with text, time, exit code, cwd, and limit filters
//! - **GlassContext**: Get an activity summary with command counts, failure rate, and directories
//! - **GlassUndo**: Undo a file-modifying command by restoring pre-command state
//! - **GlassFileDiff**: Inspect pre-command file contents for a given command
//!
//! All logging goes to stderr; stdout carries only JSON-RPC messages.

pub mod context;
pub mod ipc_client;
pub mod tools;

use rmcp::ServiceExt;

/// Start the MCP server over stdio.
///
/// Resolves the Glass history database path and snapshot glass directory from
/// the current working directory, creates a `GlassServer`, and serves it over
/// stdin/stdout using JSON-RPC 2.0.
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

    // Always create the IPC client -- it handles connection failures lazily
    // (returns clear error messages when the GUI isn't running).
    let ipc_client = Some(ipc_client::IpcClient::new());

    let server = tools::GlassServer::new(db_path, glass_dir, coord_db_path, ipc_client);
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

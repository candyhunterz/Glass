//! glass_mcp — MCP server exposing Glass terminal history to AI assistants.
//!
//! Provides two tools via the Model Context Protocol:
//! - **GlassHistory**: Query command history with text, time, exit code, cwd, and limit filters
//! - **GlassContext**: Get an activity summary with command counts, failure rate, and directories
//!
//! All logging goes to stderr; stdout carries only JSON-RPC messages.

pub mod context;
pub mod tools;

use rmcp::ServiceExt;

/// Start the MCP server over stdio.
///
/// Resolves the Glass history database path from the current working directory,
/// creates a `GlassServer`, and serves it over stdin/stdout using JSON-RPC 2.0.
///
/// # Errors
///
/// Returns an error if the server fails to start or encounters a fatal transport error.
pub async fn run_mcp_server() -> anyhow::Result<()> {
    let db_path = glass_history::resolve_db_path(
        &std::env::current_dir().unwrap_or_default(),
    );
    tracing::info!("MCP server starting, db_path={}", db_path.display());

    let server = tools::GlassServer::new(db_path);
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

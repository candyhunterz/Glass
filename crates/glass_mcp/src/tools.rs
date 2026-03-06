//! MCP tool definitions for the Glass server.
//!
//! Defines `GlassServer` with four tools:
//! - `glass_history`: Query terminal command history with filters
//! - `glass_context`: Get a summary of recent terminal activity
//! - `glass_undo`: Undo a file-modifying command by restoring pre-command state
//! - `glass_file_diff`: Inspect pre-command file contents for a given command
//!
//! Uses rmcp's `#[tool_router]` and `#[tool_handler]` macros for
//! zero-boilerplate MCP tool registration and dispatch.

use std::path::PathBuf;

use glass_history::db::{CommandRecord, HistoryDb};
use glass_history::query::{self, QueryFilter};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo,
};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use serde::{Deserialize, Serialize};

use crate::context;

// ---------------------------------------------------------------------------
// Parameter types (schemars for auto-schema generation)
// ---------------------------------------------------------------------------

/// Parameters for the glass_history tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HistoryParams {
    /// Search text to filter commands (FTS5 full-text search).
    #[schemars(description = "Search text to filter commands")]
    pub text: Option<String>,
    /// Only commands after this time (e.g. '1h', '2d', '2024-01-15').
    #[schemars(description = "Only commands after this time (e.g. '1h', '2d', '2024-01-15')")]
    pub after: Option<String>,
    /// Only commands before this time (e.g. '1h', '2d', '2024-01-15').
    #[schemars(description = "Only commands before this time (e.g. '1h', '2d', '2024-01-15')")]
    pub before: Option<String>,
    /// Filter by exit code (0 for success).
    #[schemars(description = "Filter by exit code (0 for success)")]
    pub exit_code: Option<i32>,
    /// Filter by working directory prefix.
    #[schemars(description = "Filter by working directory prefix")]
    pub cwd: Option<String>,
    /// Maximum number of results (default 25).
    #[schemars(description = "Maximum number of results (default 25)")]
    pub limit: Option<usize>,
}

/// Parameters for the glass_context tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ContextParams {
    /// Only include activity after this time (e.g. '1h', '2d', '2024-01-15').
    #[schemars(description = "Only include activity after this time (e.g. '1h', '2d', '2024-01-15')")]
    pub after: Option<String>,
}

/// Parameters for the glass_undo tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UndoParams {
    /// The command ID to undo (from glass_history results).
    #[schemars(description = "The command ID to undo (from glass_history results)")]
    pub command_id: i64,
}

/// Parameters for the glass_file_diff tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FileDiffParams {
    /// The command ID to get file diffs for (from glass_history results).
    #[schemars(description = "The command ID to get file diffs for (from glass_history results)")]
    pub command_id: i64,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// A single command history entry returned by the glass_history tool.
#[derive(Debug, Serialize)]
pub struct HistoryEntry {
    pub command: String,
    pub cwd: String,
    pub exit_code: Option<i32>,
    pub started_at: i64,
    pub finished_at: i64,
    pub duration_ms: i64,
    pub output_preview: Option<String>,
}

impl From<CommandRecord> for HistoryEntry {
    fn from(r: CommandRecord) -> Self {
        Self {
            command: r.command,
            cwd: r.cwd,
            exit_code: r.exit_code,
            started_at: r.started_at,
            finished_at: r.finished_at,
            duration_ms: r.duration_ms,
            output_preview: r.output.map(|o| {
                if o.len() > 500 {
                    format!("{}...", &o[..500])
                } else {
                    o
                }
            }),
        }
    }
}

/// Helper to convert anyhow::Error to McpError.
fn internal_err(e: impl std::fmt::Display) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

// ---------------------------------------------------------------------------
// GlassServer
// ---------------------------------------------------------------------------

/// MCP server exposing Glass terminal history, undo, and file diff tools.
#[derive(Clone)]
pub struct GlassServer {
    tool_router: ToolRouter<Self>,
    db_path: PathBuf,
    glass_dir: PathBuf,
}

#[tool_router]
impl GlassServer {
    /// Create a new GlassServer pointing at the given history database and glass directory.
    pub fn new(db_path: PathBuf, glass_dir: PathBuf) -> Self {
        Self {
            tool_router: Self::tool_router(),
            db_path,
            glass_dir,
        }
    }

    /// Query Glass terminal command history with filters.
    /// Returns commands matching the specified criteria, ordered by most recent first.
    #[tool(description = "Query Glass terminal command history with filters. Returns commands matching the specified criteria.")]
    async fn glass_history(
        &self,
        Parameters(params): Parameters<HistoryParams>,
    ) -> Result<CallToolResult, McpError> {
        let db_path = self.db_path.clone();

        let records = tokio::task::spawn_blocking(move || {
            let db = HistoryDb::open(&db_path).map_err(internal_err)?;

            let after = params
                .after
                .as_deref()
                .map(query::parse_time)
                .transpose()
                .map_err(internal_err)?;

            let before = params
                .before
                .as_deref()
                .map(query::parse_time)
                .transpose()
                .map_err(internal_err)?;

            let filter = QueryFilter {
                text: params.text,
                exit_code: params.exit_code,
                after,
                before,
                cwd: params.cwd,
                limit: params.limit.unwrap_or(25),
            };

            db.filtered_query(&filter).map_err(internal_err)
        })
        .await
        .map_err(internal_err)??;

        let entries: Vec<HistoryEntry> = records.into_iter().map(HistoryEntry::from).collect();
        let content = Content::json(&entries)?;
        Ok(CallToolResult::success(vec![content]))
    }

    /// Get a summary of recent terminal activity including command counts,
    /// failure rate, and active directories.
    #[tool(description = "Get a summary of recent terminal activity including command counts, failure rate, and active directories.")]
    async fn glass_context(
        &self,
        Parameters(params): Parameters<ContextParams>,
    ) -> Result<CallToolResult, McpError> {
        let db_path = self.db_path.clone();

        let summary = tokio::task::spawn_blocking(move || {
            let db = HistoryDb::open(&db_path).map_err(internal_err)?;

            let after_epoch = params
                .after
                .as_deref()
                .map(query::parse_time)
                .transpose()
                .map_err(internal_err)?;

            context::build_context_summary(db.conn(), after_epoch).map_err(internal_err)
        })
        .await
        .map_err(internal_err)??;

        let content = Content::json(&summary)?;
        Ok(CallToolResult::success(vec![content]))
    }

    /// Undo a file-modifying command by restoring files to their pre-command state.
    /// Returns per-file outcomes (restored, deleted, skipped, conflict, error).
    #[tool(description = "Undo a file-modifying command by restoring files to their pre-command state. Returns per-file outcomes.")]
    async fn glass_undo(
        &self,
        Parameters(params): Parameters<UndoParams>,
    ) -> Result<CallToolResult, McpError> {
        let glass_dir = self.glass_dir.clone();
        let result = tokio::task::spawn_blocking(move || {
            let store =
                glass_snapshot::SnapshotStore::open(&glass_dir).map_err(internal_err)?;
            let engine = glass_snapshot::UndoEngine::new(&store);
            engine.undo_command(params.command_id).map_err(internal_err)
        })
        .await
        .map_err(internal_err)??;

        match result {
            Some(undo_result) => {
                let outcomes: Vec<serde_json::Value> = undo_result
                    .files
                    .iter()
                    .map(|(path, outcome)| {
                        let status = match outcome {
                            glass_snapshot::FileOutcome::Restored => "restored",
                            glass_snapshot::FileOutcome::Deleted => "deleted",
                            glass_snapshot::FileOutcome::Skipped => "skipped",
                            glass_snapshot::FileOutcome::Conflict { .. } => "conflict",
                            glass_snapshot::FileOutcome::Error(_) => "error",
                        };
                        serde_json::json!({
                            "path": path.display().to_string(),
                            "status": status,
                        })
                    })
                    .collect();
                let response = serde_json::json!({
                    "command_id": undo_result.command_id,
                    "confidence": format!("{:?}", undo_result.confidence),
                    "files": outcomes,
                });
                let content = Content::json(&response)?;
                Ok(CallToolResult::success(vec![content]))
            }
            None => {
                let content = Content::text(format!(
                    "No snapshot found for command {}",
                    params.command_id
                ));
                Ok(CallToolResult::success(vec![content]))
            }
        }
    }

    /// Inspect file contents from before a command executed.
    /// Returns the pre-command file contents for all files tracked in the snapshot.
    #[tool(description = "Inspect file contents from before a command executed. Returns the pre-command file contents for all files tracked in the snapshot.")]
    async fn glass_file_diff(
        &self,
        Parameters(params): Parameters<FileDiffParams>,
    ) -> Result<CallToolResult, McpError> {
        let glass_dir = self.glass_dir.clone();
        let result = tokio::task::spawn_blocking(
            move || -> Result<Vec<serde_json::Value>, McpError> {
                let store = glass_snapshot::SnapshotStore::open(&glass_dir)
                    .map_err(internal_err)?;
                let snapshots = store
                    .db()
                    .get_snapshots_by_command(params.command_id)
                    .map_err(internal_err)?;
                let mut file_diffs = Vec::new();
                for snapshot in &snapshots {
                    let files = store
                        .db()
                        .get_snapshot_files(snapshot.id)
                        .map_err(internal_err)?;
                    for file_rec in &files {
                        if file_rec.source != "parser" {
                            continue;
                        }
                        let pre_content = match &file_rec.blob_hash {
                            Some(hash) => match store.blobs().read_blob(hash) {
                                Ok(bytes) => {
                                    Some(String::from_utf8_lossy(&bytes).into_owned())
                                }
                                Err(_) => None,
                            },
                            None => None, // File did not exist before command
                        };
                        file_diffs.push(serde_json::json!({
                            "path": file_rec.file_path,
                            "existed_before": file_rec.blob_hash.is_some(),
                            "pre_command_content": pre_content,
                            "file_size": file_rec.file_size,
                        }));
                    }
                }
                Ok(file_diffs)
            },
        )
        .await
        .map_err(internal_err)??;

        if result.is_empty() {
            let content = Content::text(format!(
                "No file snapshots found for command {}",
                params.command_id
            ));
            Ok(CallToolResult::success(vec![content]))
        } else {
            let response =
                serde_json::json!({ "command_id": params.command_id, "files": result });
            let content = Content::json(&response)?;
            Ok(CallToolResult::success(vec![content]))
        }
    }
}

#[tool_handler]
impl ServerHandler for GlassServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("glass-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Glass terminal history, context, undo, and file diff server. \
                 Use glass_history to search commands, glass_context for activity overview, \
                 glass_undo to revert file changes, glass_file_diff to inspect pre-command file contents.",
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glass_server_accepts_glass_dir() {
        let db_path = PathBuf::from("/tmp/history.db");
        let glass_dir = PathBuf::from("/tmp/.glass");
        let server = GlassServer::new(db_path.clone(), glass_dir.clone());
        assert_eq!(server.db_path, db_path);
        assert_eq!(server.glass_dir, glass_dir);
    }

    #[test]
    fn test_undo_params_deserialize() {
        let json = r#"{"command_id": 42}"#;
        let params: UndoParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.command_id, 42);
    }

    #[test]
    fn test_file_diff_params_deserialize() {
        let json = r#"{"command_id": 99}"#;
        let params: FileDiffParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.command_id, 99);
    }

    #[test]
    fn test_history_entry_from_record_truncates_long_output() {
        let record = CommandRecord {
            id: None,
            command: "echo hello".to_string(),
            cwd: "/tmp".to_string(),
            exit_code: Some(0),
            started_at: 1700000000,
            finished_at: 1700000005,
            duration_ms: 5000,
            output: Some("x".repeat(600)),
        };
        let entry = HistoryEntry::from(record);
        assert!(entry.output_preview.is_some());
        let preview = entry.output_preview.unwrap();
        assert_eq!(preview.len(), 503); // 500 + "..."
        assert!(preview.ends_with("..."));
    }

    #[test]
    fn test_history_entry_from_record_preserves_short_output() {
        let record = CommandRecord {
            id: None,
            command: "ls".to_string(),
            cwd: "/home".to_string(),
            exit_code: Some(0),
            started_at: 1700000000,
            finished_at: 1700000001,
            duration_ms: 1000,
            output: Some("file1\nfile2\n".to_string()),
        };
        let entry = HistoryEntry::from(record);
        assert_eq!(entry.output_preview, Some("file1\nfile2\n".to_string()));
    }

    #[test]
    fn test_history_entry_from_record_none_output() {
        let record = CommandRecord {
            id: None,
            command: "cd /tmp".to_string(),
            cwd: "/tmp".to_string(),
            exit_code: Some(0),
            started_at: 1700000000,
            finished_at: 1700000001,
            duration_ms: 1000,
            output: None,
        };
        let entry = HistoryEntry::from(record);
        assert!(entry.output_preview.is_none());
    }
}

//! MCP tool definitions for the Glass server.
//!
//! Defines `GlassServer` with twenty-eight tools:
//! - `glass_history`: Query terminal command history with filters
//! - `glass_context`: Get a summary of recent terminal activity
//! - `glass_undo`: Undo a file-modifying command by restoring pre-command state
//! - `glass_file_diff`: Inspect pre-command file contents for a given command
//! - `glass_pipe_inspect`: Inspect intermediate output from a pipeline stage
//! - `glass_agent_register`: Register an agent in the coordination database
//! - `glass_agent_deregister`: Remove an agent from the coordination database
//! - `glass_agent_list`: List active agents for a project
//! - `glass_agent_status`: Update an agent's status and current task
//! - `glass_agent_heartbeat`: Refresh an agent's liveness timestamp
//! - `glass_agent_lock`: Atomically claim advisory file locks
//! - `glass_agent_unlock`: Release file locks
//! - `glass_agent_locks`: List all active file locks
//! - `glass_agent_broadcast`: Broadcast a message to all project agents
//! - `glass_agent_send`: Send a directed message to a specific agent
//! - `glass_agent_messages`: Read unread messages
//! - `glass_ping`: Check if the Glass GUI process is running and responsive
//! - `glass_tab_list`: List all open tabs with their state
//! - `glass_tab_create`: Create a new terminal tab with optional shell and cwd
//! - `glass_tab_send`: Send a command to a specific tab's terminal
//! - `glass_tab_output`: Read output from a tab (head/tail mode) or from history DB by command_id
//! - `glass_tab_close`: Close a specific tab
//! - `glass_cache_check`: Check if a previous command's cached result is still valid
//! - `glass_command_diff`: Show unified diffs of files a command modified
//! - `glass_compressed_context`: Get budget-aware compressed context summary
//! - `glass_extract_errors`: Extract structured errors from raw command output
//! - `glass_has_running_command`: Check if a command is running in a tab with elapsed time
//! - `glass_cancel_command`: Cancel a running command (Ctrl+C) in a tab
//!
//! Uses rmcp's `#[tool_router]` and `#[tool_handler]` macros for
//! zero-boilerplate MCP tool registration and dispatch.

use std::path::PathBuf;
use std::sync::Arc;

use glass_history::db::{CommandRecord, HistoryDb};
use glass_history::query::{self, QueryFilter};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use serde::{Deserialize, Serialize};

use similar::TextDiff;

use crate::context;
use crate::ipc_client;

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
    #[schemars(
        description = "Only include activity after this time (e.g. '1h', '2d', '2024-01-15')"
    )]
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

/// Parameters for the glass_pipe_inspect tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PipeInspectParams {
    /// The command ID to inspect pipe stages for.
    #[schemars(description = "The command ID to inspect pipe stages for")]
    pub command_id: i64,
    /// Optional 0-based stage index. If omitted, returns all stages.
    #[schemars(description = "Optional stage index (0-based). If omitted, returns all stages")]
    pub stage: Option<i64>,
}

/// Parameters for the glass_agent_register tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RegisterParams {
    /// Human-readable agent name.
    #[schemars(description = "Human-readable agent name")]
    pub name: String,
    /// Agent type (e.g. 'claude-code', 'cursor', 'human').
    #[schemars(description = "Agent type (e.g. 'claude-code', 'cursor', 'human')")]
    pub agent_type: String,
    /// Project root path for scoping.
    #[schemars(description = "Project root path for scoping")]
    pub project: String,
    /// Current working directory.
    #[schemars(description = "Current working directory")]
    pub cwd: String,
    /// OS process ID for liveness detection.
    #[schemars(description = "OS process ID for liveness detection")]
    pub pid: Option<u32>,
}

/// Parameters for the glass_agent_deregister tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeregisterParams {
    /// Agent UUID to deregister.
    #[schemars(description = "Agent UUID to deregister")]
    pub agent_id: String,
}

/// Parameters for the glass_agent_list tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListAgentsParams {
    /// Project root path to list agents for.
    #[schemars(description = "Project root path to list agents for")]
    pub project: String,
}

/// Parameters for the glass_agent_status tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StatusParams {
    /// Agent UUID.
    #[schemars(description = "Agent UUID")]
    pub agent_id: String,
    /// New status (e.g. 'active', 'idle', 'editing').
    #[schemars(description = "New status (e.g. 'active', 'idle', 'editing')")]
    pub status: String,
    /// Current task description.
    #[schemars(description = "Current task description")]
    pub task: Option<String>,
}

/// Parameters for the glass_agent_heartbeat tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HeartbeatParams {
    /// Agent UUID to refresh heartbeat for.
    #[schemars(description = "Agent UUID to refresh heartbeat for")]
    pub agent_id: String,
}

/// Parameters for the glass_agent_lock tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LockParams {
    /// Agent UUID requesting the locks.
    #[schemars(description = "Agent UUID requesting the locks")]
    pub agent_id: String,
    /// File paths to lock.
    #[schemars(description = "File paths to lock")]
    pub paths: Vec<String>,
    /// Reason for locking (shown to other agents).
    #[schemars(description = "Reason for locking (shown to other agents)")]
    pub reason: Option<String>,
}

/// Parameters for the glass_agent_unlock tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UnlockParams {
    /// Agent UUID releasing locks.
    #[schemars(description = "Agent UUID releasing locks")]
    pub agent_id: String,
    /// Specific file paths to unlock. Omit to release all locks.
    #[schemars(description = "Specific file paths to unlock. Omit to release all locks.")]
    pub paths: Option<Vec<String>>,
}

/// Parameters for the glass_agent_locks tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListLocksParams {
    /// Project root path to filter locks. Omit for all locks.
    #[schemars(description = "Project root path to filter locks. Omit for all locks.")]
    pub project: Option<String>,
}

/// Parameters for the glass_agent_broadcast tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BroadcastParams {
    /// Agent UUID of the sender.
    #[schemars(description = "Agent UUID of the sender")]
    pub agent_id: String,
    /// Project root path (broadcast reaches all agents in this project).
    #[schemars(description = "Project root path (broadcast reaches all agents in this project)")]
    pub project: String,
    /// Message type (e.g. 'status', 'file_saved', 'conflict', 'chat').
    #[schemars(description = "Message type (e.g. 'status', 'file_saved', 'conflict', 'chat')")]
    pub msg_type: String,
    /// Message content.
    #[schemars(description = "Message content")]
    pub content: String,
}

/// Parameters for the glass_agent_send tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SendParams {
    /// Agent UUID of the sender.
    #[schemars(description = "Agent UUID of the sender")]
    pub agent_id: String,
    /// Agent UUID of the recipient.
    #[schemars(description = "Agent UUID of the recipient")]
    pub to_agent: String,
    /// Message type (e.g. 'request_unlock', 'chat', 'conflict').
    #[schemars(description = "Message type (e.g. 'request_unlock', 'chat', 'conflict')")]
    pub msg_type: String,
    /// Message content.
    #[schemars(description = "Message content")]
    pub content: String,
}

/// Parameters for the glass_agent_messages tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MessagesParams {
    /// Agent UUID to read messages for.
    #[schemars(description = "Agent UUID to read messages for")]
    pub agent_id: String,
}

/// Parameters for glass_tab_create.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TabCreateParams {
    /// Shell to use (e.g. "bash", "pwsh"). Uses default if omitted.
    #[schemars(description = "Shell to use (e.g. 'bash', 'pwsh'). Uses default if omitted")]
    pub shell: Option<String>,
    /// Working directory for the new tab.
    #[schemars(description = "Working directory for the new tab")]
    pub cwd: Option<String>,
}

/// Parameters for glass_tab_send.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TabSendParams {
    /// 0-based tab index.
    #[schemars(description = "0-based tab index (provide this OR session_id)")]
    pub tab_index: Option<u64>,
    /// Stable session ID.
    #[schemars(description = "Stable session ID (provide this OR tab_index)")]
    pub session_id: Option<u64>,
    /// Command string to send (Enter is appended automatically).
    #[schemars(description = "Command string to send (Enter is appended automatically)")]
    pub command: String,
}

/// Parameters for glass_tab_output.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TabOutputParams {
    /// 0-based tab index.
    #[schemars(description = "0-based tab index (provide this OR session_id)")]
    pub tab_index: Option<u64>,
    /// Stable session ID.
    #[schemars(description = "Stable session ID (provide this OR tab_index)")]
    pub session_id: Option<u64>,
    /// Number of lines to return (default 50).
    #[schemars(description = "Number of lines to return (default 50, max 10000)")]
    pub lines: Option<usize>,
    /// Regex pattern to filter lines.
    #[schemars(description = "Regex pattern to filter output lines")]
    pub pattern: Option<String>,
    /// Output mode: 'head' for first N lines, 'tail' for last N lines (default 'tail').
    #[schemars(
        description = "Output mode: 'head' for first N lines, 'tail' for last N lines (default 'tail')"
    )]
    pub mode: Option<String>,
    /// History command ID. If provided, returns filtered output from history DB instead of live terminal (no GUI required).
    #[schemars(
        description = "History command ID. If provided, returns filtered output from history DB instead of live terminal (no GUI required)"
    )]
    pub command_id: Option<i64>,
}

/// Parameters for glass_cache_check.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CacheCheckParams {
    /// History command ID to check staleness for.
    #[schemars(description = "History command ID to check staleness for")]
    pub command_id: i64,
}

/// Parameters for glass_command_diff.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CommandDiffParams {
    /// History command ID to get file diffs for.
    #[schemars(description = "History command ID to get file diffs for")]
    pub command_id: i64,
}

/// Parameters for glass_compressed_context.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CompressedContextParams {
    /// Maximum number of tokens in the response (approximate, 1 token ~ 4 chars).
    #[schemars(description = "Maximum tokens in response (approximate, 1 token ~ 4 chars)")]
    pub token_budget: usize,
    /// Focus mode: 'errors' (failed commands), 'files' (file changes), 'history' (recent commands), or null for balanced.
    #[schemars(
        description = "Focus: 'errors', 'files', 'history', or null for balanced across all"
    )]
    pub focus: Option<String>,
    /// Time filter: only include activity after this time. Supports '1h', '2d', ISO dates.
    #[schemars(description = "Time filter: '1h', '2d', ISO date. Default: last 1 hour")]
    pub after: Option<String>,
}

/// Parameters for glass_extract_errors.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExtractErrorsParams {
    /// Raw command output text to extract errors from.
    #[schemars(description = "Raw command output text to extract errors from")]
    pub output: String,
    /// Optional command hint for parser auto-detection (e.g. 'cargo build', 'gcc').
    #[schemars(
        description = "Optional command hint for parser auto-detection (e.g. 'cargo build', 'gcc')"
    )]
    pub command_hint: Option<String>,
}

/// Parameters for glass_has_running_command.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HasRunningCommandParams {
    /// 0-based tab index.
    #[schemars(description = "0-based tab index (provide this OR session_id)")]
    pub tab_index: Option<u64>,
    /// Stable session ID.
    #[schemars(description = "Stable session ID (provide this OR tab_index)")]
    pub session_id: Option<u64>,
}

/// Parameters for glass_cancel_command.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CancelCommandParams {
    /// 0-based tab index.
    #[schemars(description = "0-based tab index (provide this OR session_id)")]
    pub tab_index: Option<u64>,
    /// Stable session ID.
    #[schemars(description = "Stable session ID (provide this OR tab_index)")]
    pub session_id: Option<u64>,
}

/// Parameters for glass_tab_close.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TabCloseParams {
    /// 0-based tab index.
    #[schemars(description = "0-based tab index (provide this OR session_id)")]
    pub tab_index: Option<u64>,
    /// Stable session ID.
    #[schemars(description = "Stable session ID (provide this OR tab_index)")]
    pub session_id: Option<u64>,
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

/// A single pipeline stage entry returned by the glass_pipe_inspect tool.
#[derive(Debug, Serialize)]
pub struct PipeStageEntry {
    pub stage_index: i64,
    pub command: String,
    pub output: Option<String>,
    pub total_bytes: i64,
    pub is_binary: bool,
    pub is_sampled: bool,
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

/// Check whether raw bytes look like binary content (contains null byte in first 8 KiB).
fn is_binary_content(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|&b| b == 0)
}

/// Truncate text to a character budget, appending "..." if truncated.
fn truncate_to_budget(text: &str, char_budget: usize) -> String {
    if text.len() <= char_budget {
        return text.to_string();
    }
    let truncated: String = text.chars().take(char_budget.saturating_sub(3)).collect();
    format!("{}...", truncated)
}

// ---------------------------------------------------------------------------
// GlassServer
// ---------------------------------------------------------------------------

/// MCP server exposing Glass terminal history, undo, file diff, and live GUI tools.
#[derive(Clone)]
pub struct GlassServer {
    tool_router: ToolRouter<Self>,
    db_path: PathBuf,
    glass_dir: PathBuf,
    coord_db_path: PathBuf,
    /// IPC client for communicating with the live Glass GUI process.
    /// `None` only if explicitly disabled; the client itself handles connection
    /// failures gracefully (returns clear error messages).
    ipc_client: Option<Arc<ipc_client::IpcClient>>,
}

#[tool_router]
impl GlassServer {
    /// Create a new GlassServer pointing at the given history database, glass directory,
    /// and coordination database. Optionally accepts an IPC client for live GUI communication.
    pub fn new(
        db_path: PathBuf,
        glass_dir: PathBuf,
        coord_db_path: PathBuf,
        ipc_client: Option<ipc_client::IpcClient>,
    ) -> Self {
        Self {
            tool_router: Self::tool_router(),
            db_path,
            glass_dir,
            coord_db_path,
            ipc_client: ipc_client.map(Arc::new),
        }
    }

    /// Query Glass terminal command history with filters.
    /// Returns commands matching the specified criteria, ordered by most recent first.
    #[tool(
        description = "Query Glass terminal command history with filters. Returns commands matching the specified criteria."
    )]
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
    #[tool(
        description = "Get a summary of recent terminal activity including command counts, failure rate, and active directories."
    )]
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
    #[tool(
        description = "Undo a file-modifying command by restoring files to their pre-command state. Returns per-file outcomes."
    )]
    async fn glass_undo(
        &self,
        Parameters(params): Parameters<UndoParams>,
    ) -> Result<CallToolResult, McpError> {
        let glass_dir = self.glass_dir.clone();
        let result = tokio::task::spawn_blocking(move || {
            let store = glass_snapshot::SnapshotStore::open(&glass_dir).map_err(internal_err)?;
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
    #[tool(
        description = "Inspect file contents from before a command executed. Returns the pre-command file contents for all files tracked in the snapshot."
    )]
    async fn glass_file_diff(
        &self,
        Parameters(params): Parameters<FileDiffParams>,
    ) -> Result<CallToolResult, McpError> {
        let glass_dir = self.glass_dir.clone();
        let result =
            tokio::task::spawn_blocking(move || -> Result<Vec<serde_json::Value>, McpError> {
                let store =
                    glass_snapshot::SnapshotStore::open(&glass_dir).map_err(internal_err)?;
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
                                Ok(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
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
            })
            .await
            .map_err(internal_err)??;

        if result.is_empty() {
            let content = Content::text(format!(
                "No file snapshots found for command {}",
                params.command_id
            ));
            Ok(CallToolResult::success(vec![content]))
        } else {
            let response = serde_json::json!({ "command_id": params.command_id, "files": result });
            let content = Content::json(&response)?;
            Ok(CallToolResult::success(vec![content]))
        }
    }

    /// Inspect intermediate output from a pipeline stage.
    /// Returns captured output for each pipe stage of a command.
    #[tool(
        description = "Inspect intermediate output from a pipeline stage. Returns captured output for each pipe stage of a command."
    )]
    async fn glass_pipe_inspect(
        &self,
        Parameters(params): Parameters<PipeInspectParams>,
    ) -> Result<CallToolResult, McpError> {
        let db_path = self.db_path.clone();
        let stage_filter = params.stage;

        let stages = tokio::task::spawn_blocking(move || {
            let db = HistoryDb::open(&db_path).map_err(internal_err)?;
            db.get_pipe_stages(params.command_id).map_err(internal_err)
        })
        .await
        .map_err(internal_err)??;

        let entries: Vec<PipeStageEntry> = stages
            .into_iter()
            .map(|row| PipeStageEntry {
                stage_index: row.stage_index,
                command: row.command,
                output: row.output,
                total_bytes: row.total_bytes,
                is_binary: row.is_binary,
                is_sampled: row.is_sampled,
            })
            .collect();

        let response = if let Some(idx) = stage_filter {
            let stage = entries.into_iter().find(|e| e.stage_index == idx);
            match stage {
                Some(s) => serde_json::json!({
                    "command_id": params.command_id,
                    "stage": s,
                }),
                None => serde_json::json!({
                    "command_id": params.command_id,
                    "stages": Vec::<PipeStageEntry>::new(),
                }),
            }
        } else {
            serde_json::json!({
                "command_id": params.command_id,
                "stages": entries,
            })
        };

        let content = Content::json(&response)?;
        Ok(CallToolResult::success(vec![content]))
    }

    /// Register an agent in the coordination database.
    /// Returns the new agent UUID and count of active agents for the project.
    #[tool(
        description = "Register an agent in the coordination database. Returns the new agent UUID and count of active agents."
    )]
    async fn glass_agent_register(
        &self,
        Parameters(params): Parameters<RegisterParams>,
    ) -> Result<CallToolResult, McpError> {
        let coord_path = self.coord_db_path.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut db =
                glass_coordination::CoordinationDb::open(&coord_path).map_err(internal_err)?;
            let agent_id = db
                .register(
                    &params.name,
                    &params.agent_type,
                    &params.project,
                    &params.cwd,
                    params.pid,
                )
                .map_err(internal_err)?;
            let agents = db.list_agents(&params.project).map_err(internal_err)?;
            Ok::<_, McpError>(serde_json::json!({
                "agent_id": agent_id,
                "agents_active": agents.len(),
            }))
        })
        .await
        .map_err(internal_err)??;

        let content = Content::json(&result)?;
        Ok(CallToolResult::success(vec![content]))
    }

    /// Deregister an agent from the coordination database.
    /// Returns whether the agent was successfully removed.
    #[tool(
        description = "Deregister an agent from the coordination database. Returns whether the agent was successfully removed."
    )]
    async fn glass_agent_deregister(
        &self,
        Parameters(params): Parameters<DeregisterParams>,
    ) -> Result<CallToolResult, McpError> {
        let coord_path = self.coord_db_path.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut db =
                glass_coordination::CoordinationDb::open(&coord_path).map_err(internal_err)?;
            let ok = db.deregister(&params.agent_id).map_err(internal_err)?;
            Ok::<_, McpError>(serde_json::json!({ "ok": ok }))
        })
        .await
        .map_err(internal_err)??;

        let content = Content::json(&result)?;
        Ok(CallToolResult::success(vec![content]))
    }

    /// List active agents for a project.
    /// Prunes stale agents (10 min timeout), then returns remaining active agents.
    #[tool(
        description = "List active agents for a project. Prunes stale agents (10 min timeout), then returns remaining active agents."
    )]
    async fn glass_agent_list(
        &self,
        Parameters(params): Parameters<ListAgentsParams>,
    ) -> Result<CallToolResult, McpError> {
        let coord_path = self.coord_db_path.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut db =
                glass_coordination::CoordinationDb::open(&coord_path).map_err(internal_err)?;
            db.prune_stale(600).map_err(internal_err)?;
            let agents = db.list_agents(&params.project).map_err(internal_err)?;
            Ok::<_, McpError>(serde_json::json!({ "agents": agents }))
        })
        .await
        .map_err(internal_err)??;

        let content = Content::json(&result)?;
        Ok(CallToolResult::success(vec![content]))
    }

    /// Update an agent's status and current task description.
    /// Returns whether the update was successful.
    #[tool(
        description = "Update an agent's status and current task description. Returns whether the update was successful."
    )]
    async fn glass_agent_status(
        &self,
        Parameters(params): Parameters<StatusParams>,
    ) -> Result<CallToolResult, McpError> {
        let coord_path = self.coord_db_path.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut db =
                glass_coordination::CoordinationDb::open(&coord_path).map_err(internal_err)?;
            let ok = db
                .update_status(&params.agent_id, &params.status, params.task.as_deref())
                .map_err(internal_err)?;
            Ok::<_, McpError>(serde_json::json!({ "ok": ok }))
        })
        .await
        .map_err(internal_err)??;

        let content = Content::json(&result)?;
        Ok(CallToolResult::success(vec![content]))
    }

    /// Refresh an agent's liveness timestamp.
    /// Returns whether the heartbeat was successfully recorded.
    #[tool(
        description = "Refresh an agent's liveness timestamp. Returns whether the heartbeat was successfully recorded."
    )]
    async fn glass_agent_heartbeat(
        &self,
        Parameters(params): Parameters<HeartbeatParams>,
    ) -> Result<CallToolResult, McpError> {
        let coord_path = self.coord_db_path.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut db =
                glass_coordination::CoordinationDb::open(&coord_path).map_err(internal_err)?;
            let ok = db.heartbeat(&params.agent_id).map_err(internal_err)?;
            Ok::<_, McpError>(serde_json::json!({ "ok": ok }))
        })
        .await
        .map_err(internal_err)??;

        let content = Content::json(&result)?;
        Ok(CallToolResult::success(vec![content]))
    }

    /// Atomically claim advisory file locks.
    /// Returns conflicts if any file is held by another agent.
    #[tool(
        description = "Atomically claim advisory file locks. Returns conflicts if any file is held by another agent."
    )]
    async fn glass_agent_lock(
        &self,
        Parameters(params): Parameters<LockParams>,
    ) -> Result<CallToolResult, McpError> {
        let coord_path = self.coord_db_path.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut db =
                glass_coordination::CoordinationDb::open(&coord_path).map_err(internal_err)?;
            let paths: Vec<PathBuf> = params.paths.iter().map(PathBuf::from).collect();
            let lock_result = db
                .lock_files(&params.agent_id, &paths, params.reason.as_deref())
                .map_err(internal_err)?;
            match lock_result {
                glass_coordination::types::LockResult::Acquired(locked) => {
                    Ok::<_, McpError>(serde_json::json!({
                        "locked": locked,
                        "conflicts": [],
                    }))
                }
                glass_coordination::types::LockResult::Conflict(conflicts) => {
                    let conflict_entries: Vec<serde_json::Value> = conflicts
                        .iter()
                        .map(|c| {
                            serde_json::json!({
                                "path": c.path,
                                "held_by": c.held_by_agent_name,
                                "held_by_id": c.held_by_agent_id,
                                "reason": c.reason,
                                "retry_hint": "Wait and retry, or send a 'request_unlock' message to the holder",
                            })
                        })
                        .collect();
                    Ok::<_, McpError>(serde_json::json!({
                        "locked": [],
                        "conflicts": conflict_entries,
                    }))
                }
            }
        })
        .await
        .map_err(internal_err)??;

        let content = Content::json(&result)?;
        Ok(CallToolResult::success(vec![content]))
    }

    /// Release file locks. Omit paths to release all locks.
    #[tool(description = "Release file locks. Omit paths to release all locks.")]
    async fn glass_agent_unlock(
        &self,
        Parameters(params): Parameters<UnlockParams>,
    ) -> Result<CallToolResult, McpError> {
        let coord_path = self.coord_db_path.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut db =
                glass_coordination::CoordinationDb::open(&coord_path).map_err(internal_err)?;
            let released = if let Some(paths) = &params.paths {
                let mut count = 0u64;
                for p in paths {
                    let ok = db
                        .unlock_file(&params.agent_id, std::path::Path::new(p))
                        .map_err(internal_err)?;
                    if ok {
                        count += 1;
                    }
                }
                count
            } else {
                db.unlock_all(&params.agent_id).map_err(internal_err)?
            };
            // MCP-12: implicit heartbeat refresh on unlock
            let _ = db.heartbeat(&params.agent_id);
            Ok::<_, McpError>(serde_json::json!({ "released": released }))
        })
        .await
        .map_err(internal_err)??;

        let content = Content::json(&result)?;
        Ok(CallToolResult::success(vec![content]))
    }

    /// List all active file locks, optionally filtered by project.
    #[tool(description = "List all active file locks, optionally filtered by project.")]
    async fn glass_agent_locks(
        &self,
        Parameters(params): Parameters<ListLocksParams>,
    ) -> Result<CallToolResult, McpError> {
        let coord_path = self.coord_db_path.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut db =
                glass_coordination::CoordinationDb::open(&coord_path).map_err(internal_err)?;
            let locks = db
                .list_locks(params.project.as_deref())
                .map_err(internal_err)?;
            Ok::<_, McpError>(serde_json::json!({ "locks": locks }))
        })
        .await
        .map_err(internal_err)??;

        let content = Content::json(&result)?;
        Ok(CallToolResult::success(vec![content]))
    }

    /// Broadcast a typed message to all agents in the same project.
    #[tool(description = "Broadcast a typed message to all agents in the same project.")]
    async fn glass_agent_broadcast(
        &self,
        Parameters(params): Parameters<BroadcastParams>,
    ) -> Result<CallToolResult, McpError> {
        let coord_path = self.coord_db_path.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut db =
                glass_coordination::CoordinationDb::open(&coord_path).map_err(internal_err)?;
            let count = db
                .broadcast(
                    &params.agent_id,
                    &params.project,
                    &params.msg_type,
                    &params.content,
                )
                .map_err(internal_err)?;
            Ok::<_, McpError>(serde_json::json!({ "delivered_to": count }))
        })
        .await
        .map_err(internal_err)??;

        let content = Content::json(&result)?;
        Ok(CallToolResult::success(vec![content]))
    }

    /// Send a directed message to a specific agent.
    #[tool(description = "Send a directed message to a specific agent.")]
    async fn glass_agent_send(
        &self,
        Parameters(params): Parameters<SendParams>,
    ) -> Result<CallToolResult, McpError> {
        let coord_path = self.coord_db_path.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut db =
                glass_coordination::CoordinationDb::open(&coord_path).map_err(internal_err)?;
            let msg_id = db
                .send_message(
                    &params.agent_id,
                    &params.to_agent,
                    &params.msg_type,
                    &params.content,
                )
                .map_err(internal_err)?;
            Ok::<_, McpError>(serde_json::json!({ "message_id": msg_id }))
        })
        .await
        .map_err(internal_err)??;

        let content = Content::json(&result)?;
        Ok(CallToolResult::success(vec![content]))
    }

    /// Read unread messages. Messages are marked as read after retrieval.
    #[tool(description = "Read unread messages. Messages are marked as read after retrieval.")]
    async fn glass_agent_messages(
        &self,
        Parameters(params): Parameters<MessagesParams>,
    ) -> Result<CallToolResult, McpError> {
        let coord_path = self.coord_db_path.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut db =
                glass_coordination::CoordinationDb::open(&coord_path).map_err(internal_err)?;
            let messages = db.read_messages(&params.agent_id).map_err(internal_err)?;
            Ok::<_, McpError>(serde_json::json!({ "messages": messages }))
        })
        .await
        .map_err(internal_err)??;

        let content = Content::json(&result)?;
        Ok(CallToolResult::success(vec![content]))
    }

    /// Check if the Glass GUI process is running and responsive.
    /// Returns status "ok" if the GUI is reachable via IPC, or an error if not.
    /// This is the pattern all future live MCP tools follow:
    /// check ipc_client -> send_request -> handle result/error.
    #[tool(
        description = "Check if the Glass GUI process is running and responsive. Returns status 'ok' if the GUI is reachable via IPC, or an error if not."
    )]
    async fn glass_ping(&self) -> Result<CallToolResult, McpError> {
        let client = match self.ipc_client.as_ref() {
            Some(c) => c,
            None => {
                return Ok(CallToolResult::error(vec![Content::text(
                    "Glass GUI is not running. Live tools require a running Glass window.",
                )]));
            }
        };
        match client.send_request("ping", serde_json::json!({})).await {
            Ok(resp) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&resp).unwrap_or_default(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to communicate with Glass GUI: {}",
                e
            ))])),
        }
    }

    /// List all open tabs with their state.
    #[tool(
        description = "List all open tabs with their state: name, working directory, session ID, and whether a command is running."
    )]
    async fn glass_tab_list(&self) -> Result<CallToolResult, McpError> {
        let client = match self.ipc_client.as_ref() {
            Some(c) => c,
            None => {
                return Ok(CallToolResult::error(vec![Content::text(
                    "Glass GUI is not running. Live tools require a running Glass window.",
                )]));
            }
        };
        match client.send_request("tab_list", serde_json::json!({})).await {
            Ok(resp) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&resp).unwrap_or_default(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to communicate with Glass GUI: {}",
                e
            ))])),
        }
    }

    /// Create a new terminal tab.
    #[tool(
        description = "Create a new terminal tab with optional shell and working directory. Returns the new tab's index and session ID."
    )]
    async fn glass_tab_create(
        &self,
        Parameters(input): Parameters<TabCreateParams>,
    ) -> Result<CallToolResult, McpError> {
        let client = match self.ipc_client.as_ref() {
            Some(c) => c,
            None => {
                return Ok(CallToolResult::error(vec![Content::text(
                    "Glass GUI is not running. Live tools require a running Glass window.",
                )]));
            }
        };
        let mut params = serde_json::json!({});
        if let Some(shell) = &input.shell {
            params["shell"] = serde_json::json!(shell);
        }
        if let Some(cwd) = &input.cwd {
            params["cwd"] = serde_json::json!(cwd);
        }
        match client.send_request("tab_create", params).await {
            Ok(resp) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&resp).unwrap_or_default(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to communicate with Glass GUI: {}",
                e
            ))])),
        }
    }

    /// Send a command to a specific tab's terminal.
    #[tool(
        description = "Send a command to a specific tab's terminal. The command is executed immediately (Enter is appended). Use tab_output to read results later."
    )]
    async fn glass_tab_send(
        &self,
        Parameters(input): Parameters<TabSendParams>,
    ) -> Result<CallToolResult, McpError> {
        let client = match self.ipc_client.as_ref() {
            Some(c) => c,
            None => {
                return Ok(CallToolResult::error(vec![Content::text(
                    "Glass GUI is not running. Live tools require a running Glass window.",
                )]));
            }
        };
        let mut params = serde_json::json!({ "command": input.command });
        if let Some(idx) = input.tab_index {
            params["tab_index"] = serde_json::json!(idx);
        }
        if let Some(sid) = input.session_id {
            params["session_id"] = serde_json::json!(sid);
        }
        match client.send_request("tab_send", params).await {
            Ok(resp) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&resp).unwrap_or_default(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to communicate with Glass GUI: {}",
                e
            ))])),
        }
    }

    /// Read the last N lines of output from a specific tab.
    #[tool(
        description = "Read output from a specific tab or from history. Supports head/tail mode and optional regex filter. Default 50 lines, max 10000. If command_id is provided, returns output from history DB (no GUI required)."
    )]
    async fn glass_tab_output(
        &self,
        Parameters(input): Parameters<TabOutputParams>,
    ) -> Result<CallToolResult, McpError> {
        // If command_id is provided, bypass IPC and read from history DB directly.
        if let Some(cmd_id) = input.command_id {
            let db_path = self.db_path.clone();
            let lines_count = input.lines.unwrap_or(50).min(10000);
            let mode = input.mode.clone().unwrap_or_else(|| "tail".to_string());
            let pattern = input.pattern.clone();

            let result =
                tokio::task::spawn_blocking(move || -> Result<serde_json::Value, String> {
                    let db = HistoryDb::open(&db_path)
                        .map_err(|e| format!("Failed to open history DB: {}", e))?;
                    let record = db
                        .get_command(cmd_id)
                        .map_err(|e| format!("DB error: {}", e))?
                        .ok_or_else(|| format!("Command not found: {}", cmd_id))?;

                    let output = record.output.unwrap_or_default();
                    let all_lines: Vec<&str> = output.lines().collect();

                    // Apply head/tail mode
                    let sliced: Vec<&str> = if mode == "head" {
                        all_lines.into_iter().take(lines_count).collect()
                    } else {
                        let len = all_lines.len();
                        let start = len.saturating_sub(lines_count);
                        all_lines[start..].to_vec()
                    };

                    // Apply regex filter
                    let filtered: Vec<String> = if let Some(ref pat) = pattern {
                        let re =
                            regex::Regex::new(pat).map_err(|e| format!("Invalid regex: {}", e))?;
                        sliced
                            .into_iter()
                            .filter(|l| re.is_match(l))
                            .map(|s| s.to_string())
                            .collect()
                    } else {
                        sliced.into_iter().map(|s| s.to_string()).collect()
                    };

                    let count = filtered.len();
                    Ok(serde_json::json!({
                        "lines": filtered,
                        "line_count": count,
                        "command_id": cmd_id,
                        "source": "history",
                    }))
                })
                .await
                .map_err(|e| McpError::internal_error(format!("Task join error: {}", e), None))?;

            return match result {
                Ok(json) => Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&json).unwrap_or_default(),
                )])),
                Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
            };
        }

        // Live IPC path
        let client = match self.ipc_client.as_ref() {
            Some(c) => c,
            None => {
                return Ok(CallToolResult::error(vec![Content::text(
                    "Glass GUI is not running. Live tools require a running Glass window.",
                )]));
            }
        };
        let mut params = serde_json::json!({});
        if let Some(idx) = input.tab_index {
            params["tab_index"] = serde_json::json!(idx);
        }
        if let Some(sid) = input.session_id {
            params["session_id"] = serde_json::json!(sid);
        }
        if let Some(lines) = input.lines {
            params["lines"] = serde_json::json!(lines);
        }
        if let Some(pattern) = &input.pattern {
            params["pattern"] = serde_json::json!(pattern);
        }
        if let Some(mode) = &input.mode {
            params["mode"] = serde_json::json!(mode);
        }
        match client.send_request("tab_output", params).await {
            Ok(resp) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&resp).unwrap_or_default(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to communicate with Glass GUI: {}",
                e
            ))])),
        }
    }

    /// Close a specific tab.
    #[tool(
        description = "Close a specific tab. Refuses to close the last remaining tab. Use session_id for stability (tab indices shift on close)."
    )]
    async fn glass_tab_close(
        &self,
        Parameters(input): Parameters<TabCloseParams>,
    ) -> Result<CallToolResult, McpError> {
        let client = match self.ipc_client.as_ref() {
            Some(c) => c,
            None => {
                return Ok(CallToolResult::error(vec![Content::text(
                    "Glass GUI is not running. Live tools require a running Glass window.",
                )]));
            }
        };
        let mut params = serde_json::json!({});
        if let Some(idx) = input.tab_index {
            params["tab_index"] = serde_json::json!(idx);
        }
        if let Some(sid) = input.session_id {
            params["session_id"] = serde_json::json!(sid);
        }
        match client.send_request("tab_close", params).await {
            Ok(resp) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&resp).unwrap_or_default(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to communicate with Glass GUI: {}",
                e
            ))])),
        }
    }

    /// Check if a previous command's cached result is still valid.
    #[tool(
        description = "Check if a previous command's cached result is still valid. Compares file modification times against when the command finished. Returns stale=false if no files have changed, stale=true with a list of changed files if any have been modified or deleted since."
    )]
    async fn glass_cache_check(
        &self,
        Parameters(params): Parameters<CacheCheckParams>,
    ) -> Result<CallToolResult, McpError> {
        let db_path = self.db_path.clone();
        let glass_dir = self.glass_dir.clone();
        let command_id = params.command_id;

        let result = tokio::task::spawn_blocking(move || -> Result<serde_json::Value, String> {
            // Look up the command in history DB
            let db = HistoryDb::open(&db_path)
                .map_err(|e| format!("Failed to open history DB: {}", e))?;
            let command = db
                .get_command(command_id)
                .map_err(|e| format!("DB error: {}", e))?
                .ok_or_else(|| format!("Command not found: {}", command_id))?;

            // Open snapshot store
            let store = glass_snapshot::SnapshotStore::open(&glass_dir)
                .map_err(|e| format!("Failed to open snapshot store: {}", e))?;

            // Get snapshots for this command
            let snapshots = store
                .db()
                .get_snapshots_by_command(command_id)
                .map_err(|e| format!("Failed to query snapshots: {}", e))?;

            if snapshots.is_empty() {
                return Ok(serde_json::json!({
                    "command_id": command_id,
                    "stale": false,
                    "reason": "no_snapshots",
                    "message": "No file snapshots recorded for this command",
                }));
            }

            let mut stale = false;
            let mut changed_files: Vec<String> = Vec::new();
            let mut checked_count: usize = 0;

            for snapshot in &snapshots {
                let files = store
                    .db()
                    .get_snapshot_files(snapshot.id)
                    .map_err(|e| format!("Failed to query snapshot files: {}", e))?;

                for file_rec in &files {
                    if file_rec.source != "parser" {
                        continue;
                    }
                    checked_count += 1;

                    match std::fs::metadata(&file_rec.file_path) {
                        Err(_) => {
                            // File deleted
                            stale = true;
                            changed_files.push(file_rec.file_path.clone());
                        }
                        Ok(meta) => {
                            if let Ok(modified) = meta.modified() {
                                let mtime = modified
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs() as i64;
                                if mtime > command.finished_at {
                                    stale = true;
                                    changed_files.push(file_rec.file_path.clone());
                                }
                            }
                        }
                    }
                }
            }

            Ok(serde_json::json!({
                "command_id": command_id,
                "stale": stale,
                "changed_files": changed_files,
                "checked_files": checked_count,
            }))
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task join error: {}", e), None))?;

        match result {
            Ok(json) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&json).unwrap_or_default(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    /// Show which files a command modified with unified diffs.
    /// Compares pre-command snapshot content against current file content.
    #[tool(
        description = "Show which files a command modified with unified diffs. Compares pre-command snapshot content against current file content. Returns unified diff format for each changed file."
    )]
    async fn glass_command_diff(
        &self,
        Parameters(params): Parameters<CommandDiffParams>,
    ) -> Result<CallToolResult, McpError> {
        let glass_dir = self.glass_dir.clone();
        let command_id = params.command_id;

        let result = tokio::task::spawn_blocking(move || -> Result<serde_json::Value, String> {
            let store = glass_snapshot::SnapshotStore::open(&glass_dir)
                .map_err(|e| format!("Failed to open snapshot store: {}", e))?;
            let snapshots = store
                .db()
                .get_snapshots_by_command(command_id)
                .map_err(|e| format!("Failed to query snapshots: {}", e))?;

            let mut files = Vec::new();
            for snapshot in &snapshots {
                let file_recs = store
                    .db()
                    .get_snapshot_files(snapshot.id)
                    .map_err(|e| format!("Failed to query snapshot files: {}", e))?;

                for file_rec in &file_recs {
                    if file_rec.source != "parser" {
                        continue;
                    }

                    // Read pre-command content from blob store
                    let pre_bytes: Vec<u8> = file_rec
                        .blob_hash
                        .as_ref()
                        .and_then(|hash| store.blobs().read_blob(hash).ok())
                        .unwrap_or_default();

                    // Read current file content (empty if file deleted)
                    let current_bytes: Vec<u8> =
                        std::fs::read(&file_rec.file_path).unwrap_or_default();

                    // Binary detection
                    if is_binary_content(&pre_bytes) || is_binary_content(&current_bytes) {
                        files.push(serde_json::json!({
                            "path": file_rec.file_path,
                            "status": "binary",
                            "diff": "[binary file]",
                        }));
                        continue;
                    }

                    let pre_content = String::from_utf8_lossy(&pre_bytes);
                    let current_content = String::from_utf8_lossy(&current_bytes);

                    // Determine status
                    let status = if file_rec.blob_hash.is_none() && !current_bytes.is_empty() {
                        "created"
                    } else if !pre_bytes.is_empty() && current_bytes.is_empty() {
                        "deleted"
                    } else if pre_content == current_content {
                        "unchanged"
                    } else {
                        "modified"
                    };

                    // Generate unified diff
                    let diff = TextDiff::from_lines(pre_content.as_ref(), current_content.as_ref());
                    let unified = diff
                        .unified_diff()
                        .context_radius(3)
                        .header(
                            &format!("a/{}", file_rec.file_path),
                            &format!("b/{}", file_rec.file_path),
                        )
                        .to_string();

                    files.push(serde_json::json!({
                        "path": file_rec.file_path,
                        "status": status,
                        "diff": if unified.is_empty() { String::new() } else { unified },
                    }));
                }
            }

            Ok(serde_json::json!({
                "command_id": command_id,
                "files": files,
            }))
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task join error: {}", e), None))?;

        match result {
            Ok(json) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&json).unwrap_or_default(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    /// Get a compressed context summary within a token budget.
    /// Focus on errors, files, history, or get a balanced overview.
    #[tool(
        description = "Get a compressed context summary of terminal activity within a token budget. Focus on specific aspects (errors, files, history) or get a balanced overview. Uses approximate token counting (1 token ~ 4 chars)."
    )]
    async fn glass_compressed_context(
        &self,
        Parameters(params): Parameters<CompressedContextParams>,
    ) -> Result<CallToolResult, McpError> {
        let db_path = self.db_path.clone();
        let glass_dir = self.glass_dir.clone();

        let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
            let char_budget = params.token_budget * 4;

            // Parse time filter (default to "1h")
            let after_str = params.after.as_deref().unwrap_or("1h");
            let after_epoch =
                query::parse_time(after_str).map_err(|e| format!("Invalid time: {}", e))?;

            // Open history DB and build summary
            let db = HistoryDb::open(&db_path)
                .map_err(|e| format!("Failed to open history DB: {}", e))?;
            let summary = context::build_context_summary(db.conn(), Some(after_epoch))
                .map_err(|e| format!("Failed to build context: {}", e))?;

            // Always-included summary header
            let header = format!(
                "## Context Summary\nCommands: {}, Failed: {}, Dirs: {}\n\n",
                summary.command_count,
                summary.failure_count,
                summary.recent_directories.len(),
            );

            if char_budget <= header.len() {
                return Ok(truncate_to_budget(&header, char_budget));
            }
            let remaining = char_budget - header.len();

            // Build section content based on focus mode
            let section = match params.focus.as_deref() {
                Some("errors") => build_errors_section(&db, Some(after_epoch), remaining)?,
                Some("files") => {
                    build_files_section(&glass_dir, &db, Some(after_epoch), remaining)?
                }
                Some("history") => build_history_section(&db, Some(after_epoch), remaining)?,
                _ => {
                    // Balanced: errors first, then history, then files
                    let third = remaining / 3;
                    let mut parts = String::new();

                    let errors = build_errors_section(&db, Some(after_epoch), third)?;
                    if !errors.is_empty() {
                        parts.push_str(&errors);
                        parts.push('\n');
                    }

                    let history = build_history_section(&db, Some(after_epoch), third)?;
                    if !history.is_empty() {
                        parts.push_str(&history);
                        parts.push('\n');
                    }

                    let files_budget = remaining.saturating_sub(parts.len());
                    let files =
                        build_files_section(&glass_dir, &db, Some(after_epoch), files_budget)?;
                    if !files.is_empty() {
                        parts.push_str(&files);
                    }

                    parts
                }
            };

            let full = format!("{}{}", header, section);
            Ok(truncate_to_budget(&full, char_budget))
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task join error: {}", e), None))?;

        match result {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(
        description = "Extract structured errors (file, line, column, message, severity) from raw command output. Auto-detects the compiler/language from the command hint or output patterns. Supports Rust (JSON and human-readable), and generic file:line:col formats."
    )]
    async fn glass_extract_errors(
        &self,
        Parameters(params): Parameters<ExtractErrorsParams>,
    ) -> Result<CallToolResult, McpError> {
        let json = build_extract_errors_json(&params.output, params.command_hint.as_deref());
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Check if a command is currently running in a specific tab.
    /// Returns is_running boolean and elapsed_seconds if running.
    /// Use this to poll command completion or decide whether to cancel.
    #[tool(
        description = "Check if a command is currently running in a specific tab. Returns is_running boolean and elapsed_seconds if running. Use this to poll command completion or decide whether to cancel."
    )]
    async fn glass_has_running_command(
        &self,
        Parameters(input): Parameters<HasRunningCommandParams>,
    ) -> Result<CallToolResult, McpError> {
        let client = match self.ipc_client.as_ref() {
            Some(c) => c,
            None => {
                return Ok(CallToolResult::error(vec![Content::text(
                    "Glass GUI is not running. Live tools require a running Glass window.",
                )]));
            }
        };
        let mut params = serde_json::json!({});
        if let Some(idx) = input.tab_index {
            params["tab_index"] = serde_json::json!(idx);
        }
        if let Some(sid) = input.session_id {
            params["session_id"] = serde_json::json!(sid);
        }
        match client.send_request("has_running_command", params).await {
            Ok(resp) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&resp).unwrap_or_default(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to communicate with Glass GUI: {}",
                e
            ))])),
        }
    }

    /// Cancel the currently running command in a tab by sending Ctrl+C (SIGINT).
    /// Idempotent -- safe to call even if no command is running.
    /// Returns was_running to indicate if a command was actually interrupted.
    #[tool(
        description = "Cancel the currently running command in a tab by sending Ctrl+C (SIGINT). Idempotent -- safe to call even if no command is running. Returns was_running to indicate if a command was actually interrupted."
    )]
    async fn glass_cancel_command(
        &self,
        Parameters(input): Parameters<CancelCommandParams>,
    ) -> Result<CallToolResult, McpError> {
        let client = match self.ipc_client.as_ref() {
            Some(c) => c,
            None => {
                return Ok(CallToolResult::error(vec![Content::text(
                    "Glass GUI is not running. Live tools require a running Glass window.",
                )]));
            }
        };
        let mut params = serde_json::json!({});
        if let Some(idx) = input.tab_index {
            params["tab_index"] = serde_json::json!(idx);
        }
        if let Some(sid) = input.session_id {
            params["session_id"] = serde_json::json!(sid);
        }
        match client.send_request("cancel_command", params).await {
            Ok(resp) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&resp).unwrap_or_default(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "Failed to communicate with Glass GUI: {}",
                e
            ))])),
        }
    }
}

/// Build the errors section for compressed context.
fn build_errors_section(
    db: &HistoryDb,
    after: Option<i64>,
    budget: usize,
) -> Result<String, String> {
    let filter = QueryFilter {
        text: None,
        exit_code: None,
        after,
        before: None,
        cwd: None,
        limit: 50,
    };
    let commands = db
        .filtered_query(&filter)
        .map_err(|e| format!("Query error: {}", e))?;
    let failed: Vec<&CommandRecord> = commands.iter().filter(|c| c.exit_code != Some(0)).collect();

    if failed.is_empty() {
        return Ok(String::new());
    }

    let mut section = String::from("### Errors\n");
    for cmd in &failed {
        let line = format!(
            "- [{}] `{}` in {}\n",
            cmd.exit_code.unwrap_or(-1),
            cmd.command,
            cmd.cwd,
        );
        if section.len() + line.len() > budget {
            break;
        }
        section.push_str(&line);
    }
    Ok(truncate_to_budget(&section, budget))
}

/// Build the history section for compressed context.
fn build_history_section(
    db: &HistoryDb,
    after: Option<i64>,
    budget: usize,
) -> Result<String, String> {
    let filter = QueryFilter {
        text: None,
        exit_code: None,
        after,
        before: None,
        cwd: None,
        limit: 50,
    };
    let commands = db
        .filtered_query(&filter)
        .map_err(|e| format!("Query error: {}", e))?;

    if commands.is_empty() {
        return Ok(String::new());
    }

    let mut section = String::from("### Recent Commands\n");
    for cmd in &commands {
        let line = format!(
            "- `{}` [{}] ({}ms) in {}\n",
            cmd.command,
            cmd.exit_code.unwrap_or(-1),
            cmd.duration_ms,
            cmd.cwd,
        );
        if section.len() + line.len() > budget {
            break;
        }
        section.push_str(&line);
    }
    Ok(truncate_to_budget(&section, budget))
}

/// Build the files section for compressed context using snapshot store.
fn build_files_section(
    glass_dir: &std::path::Path,
    db: &HistoryDb,
    after: Option<i64>,
    budget: usize,
) -> Result<String, String> {
    // Get recent commands, then check which have snapshots
    let filter = QueryFilter {
        text: None,
        exit_code: None,
        after,
        before: None,
        cwd: None,
        limit: 25,
    };
    let commands = db
        .filtered_query(&filter)
        .map_err(|e| format!("Query error: {}", e))?;

    let store = match glass_snapshot::SnapshotStore::open(glass_dir) {
        Ok(s) => s,
        Err(_) => return Ok(String::new()),
    };

    let mut section = String::from("### File Changes\n");
    let mut found_any = false;

    for cmd in &commands {
        let cmd_id = match cmd.id {
            Some(id) => id,
            None => continue,
        };
        let snapshots = store
            .db()
            .get_snapshots_by_command(cmd_id)
            .unwrap_or_default();

        for snapshot in &snapshots {
            let files = store
                .db()
                .get_snapshot_files(snapshot.id)
                .unwrap_or_default();

            for file_rec in &files {
                if file_rec.source != "parser" {
                    continue;
                }
                found_any = true;
                let change_type = if file_rec.blob_hash.is_some() {
                    "modified"
                } else {
                    "created"
                };
                let line = format!(
                    "- {} ({}): `{}`\n",
                    file_rec.file_path, change_type, cmd.command
                );
                if section.len() + line.len() > budget {
                    return Ok(truncate_to_budget(&section, budget));
                }
                section.push_str(&line);
            }
        }
    }

    if !found_any {
        return Ok(String::new());
    }
    Ok(truncate_to_budget(&section, budget))
}

/// Build JSON response for extract_errors tool.
fn build_extract_errors_json(output: &str, command_hint: Option<&str>) -> String {
    let errors = glass_errors::extract_errors(output, command_hint);
    let count = errors.len();
    let response = serde_json::json!({
        "errors": errors,
        "count": count,
    });
    serde_json::to_string(&response).unwrap_or_else(|_| r#"{"errors":[],"count":0}"#.to_string())
}

#[tool_handler]
impl ServerHandler for GlassServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("glass-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Glass is a terminal emulator with persistent command history and file snapshots. \
                 These tools give you capabilities beyond your built-in tools.\n\n\
                 ## After context reset or at session start\n\
                 Call glass_context or glass_compressed_context to recover what the user was working on. \
                 This is especially important after /clear — Glass remembers commands and output across sessions.\n\n\
                 ## Use Glass tools when they add unique value\n\
                 - glass_history: search past commands and output across sessions (your shell history can't do this)\n\
                 - glass_undo: restore files to pre-command state (faster and safer than manual restoration)\n\
                 - glass_file_diff / glass_command_diff: see what a command changed on disk\n\
                 - glass_pipe_inspect: inspect intermediate pipeline stage output\n\
                 - glass_context / glass_compressed_context: get activity summary with budget-aware focus modes\n\n\
                 ## Do NOT use Glass tools when your built-in tools work fine\n\
                 - Running commands: use your Bash tool, not glass_tab_send\n\
                 - Reading files: use your Read tool, not glass_tab_output\n\
                 - Glass tab tools (glass_tab_*) are for advanced orchestration only\n\n\
                 ## Multi-agent coordination\n\
                 When working alongside other AI agents on the same project:\n\
                 1. glass_agent_register on session start (returns your agent_id)\n\
                 2. glass_agent_lock before editing files (prevents conflicts)\n\
                 3. glass_agent_unlock after editing\n\
                 4. glass_agent_messages to check for messages from other agents\n\
                 5. glass_agent_deregister on session end",
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
        let coord_db_path = PathBuf::from("/tmp/agents.db");
        let server = GlassServer::new(
            db_path.clone(),
            glass_dir.clone(),
            coord_db_path.clone(),
            None,
        );
        assert_eq!(server.db_path, db_path);
        assert_eq!(server.glass_dir, glass_dir);
        assert!(server.ipc_client.is_none());
        assert_eq!(server.coord_db_path, coord_db_path);
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

    #[test]
    fn test_pipe_inspect_params_deserialize() {
        let json = r#"{"command_id": 42}"#;
        let params: PipeInspectParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.command_id, 42);
        assert!(params.stage.is_none());
    }

    #[test]
    fn test_pipe_inspect_params_stage_filter() {
        let json = r#"{"command_id": 10, "stage": 1}"#;
        let params: PipeInspectParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.command_id, 10);
        assert_eq!(params.stage, Some(1));
    }

    #[test]
    fn test_pipe_stage_entry_serializes() {
        let entry = PipeStageEntry {
            stage_index: 0,
            command: "cat file".to_string(),
            output: Some("hello\n".to_string()),
            total_bytes: 6,
            is_binary: false,
            is_sampled: false,
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["stage_index"], 0);
        assert_eq!(json["command"], "cat file");
        assert_eq!(json["output"], "hello\n");
        assert_eq!(json["total_bytes"], 6);
        assert_eq!(json["is_binary"], false);
        assert_eq!(json["is_sampled"], false);
    }

    #[test]
    fn test_register_params_deserialize() {
        let json = r#"{"name":"claude-1","agent_type":"claude-code","project":"/tmp/proj","cwd":"/tmp/proj/src","pid":1234}"#;
        let params: RegisterParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.name, "claude-1");
        assert_eq!(params.agent_type, "claude-code");
        assert_eq!(params.project, "/tmp/proj");
        assert_eq!(params.cwd, "/tmp/proj/src");
        assert_eq!(params.pid, Some(1234));
    }

    #[test]
    fn test_register_params_deserialize_no_pid() {
        let json = r#"{"name":"cursor-1","agent_type":"cursor","project":"/home/user/proj","cwd":"/home/user/proj"}"#;
        let params: RegisterParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.name, "cursor-1");
        assert_eq!(params.agent_type, "cursor");
        assert!(params.pid.is_none());
    }

    #[test]
    fn test_deregister_params_deserialize() {
        let json = r#"{"agent_id":"550e8400-e29b-41d4-a716-446655440000"}"#;
        let params: DeregisterParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.agent_id, "550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn test_list_agents_params_deserialize() {
        let json = r#"{"project":"/home/user/myproject"}"#;
        let params: ListAgentsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.project, "/home/user/myproject");
    }

    #[test]
    fn test_status_params_deserialize() {
        let json = r#"{"agent_id":"abc-123","status":"editing","task":"Refactoring auth module"}"#;
        let params: StatusParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.agent_id, "abc-123");
        assert_eq!(params.status, "editing");
        assert_eq!(params.task, Some("Refactoring auth module".to_string()));
    }

    #[test]
    fn test_status_params_deserialize_no_task() {
        let json = r#"{"agent_id":"abc-123","status":"idle"}"#;
        let params: StatusParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.agent_id, "abc-123");
        assert_eq!(params.status, "idle");
        assert!(params.task.is_none());
    }

    #[test]
    fn test_heartbeat_params_deserialize() {
        let json = r#"{"agent_id":"def-456"}"#;
        let params: HeartbeatParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.agent_id, "def-456");
    }

    #[test]
    fn test_lock_params_deserialize() {
        let json = r#"{"agent_id":"abc-123","paths":["/tmp/a.rs","/tmp/b.rs"],"reason":"editing auth module"}"#;
        let params: LockParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.agent_id, "abc-123");
        assert_eq!(params.paths, vec!["/tmp/a.rs", "/tmp/b.rs"]);
        assert_eq!(params.reason, Some("editing auth module".to_string()));
    }

    #[test]
    fn test_lock_params_deserialize_no_reason() {
        let json = r#"{"agent_id":"abc-123","paths":["/tmp/a.rs"]}"#;
        let params: LockParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.agent_id, "abc-123");
        assert_eq!(params.paths, vec!["/tmp/a.rs"]);
        assert!(params.reason.is_none());
    }

    #[test]
    fn test_unlock_params_deserialize_with_paths() {
        let json = r#"{"agent_id":"abc-123","paths":["/tmp/a.rs","/tmp/b.rs"]}"#;
        let params: UnlockParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.agent_id, "abc-123");
        assert_eq!(
            params.paths,
            Some(vec!["/tmp/a.rs".to_string(), "/tmp/b.rs".to_string()])
        );
    }

    #[test]
    fn test_unlock_params_deserialize_no_paths() {
        let json = r#"{"agent_id":"abc-123"}"#;
        let params: UnlockParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.agent_id, "abc-123");
        assert!(params.paths.is_none());
    }

    #[test]
    fn test_list_locks_params_deserialize() {
        let json = r#"{"project":"/home/user/proj"}"#;
        let params: ListLocksParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.project, Some("/home/user/proj".to_string()));
    }

    #[test]
    fn test_list_locks_params_deserialize_no_project() {
        let json = r#"{}"#;
        let params: ListLocksParams = serde_json::from_str(json).unwrap();
        assert!(params.project.is_none());
    }

    #[test]
    fn test_broadcast_params_deserialize() {
        let json = r#"{"agent_id":"abc-123","project":"/tmp/proj","msg_type":"status","content":"Working on auth"}"#;
        let params: BroadcastParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.agent_id, "abc-123");
        assert_eq!(params.project, "/tmp/proj");
        assert_eq!(params.msg_type, "status");
        assert_eq!(params.content, "Working on auth");
    }

    #[test]
    fn test_send_params_deserialize() {
        let json = r#"{"agent_id":"abc-123","to_agent":"def-456","msg_type":"request_unlock","content":"Need access to auth.rs"}"#;
        let params: SendParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.agent_id, "abc-123");
        assert_eq!(params.to_agent, "def-456");
        assert_eq!(params.msg_type, "request_unlock");
        assert_eq!(params.content, "Need access to auth.rs");
    }

    #[test]
    fn test_messages_params_deserialize() {
        let json = r#"{"agent_id":"abc-123"}"#;
        let params: MessagesParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.agent_id, "abc-123");
    }

    #[test]
    fn test_tab_create_params_deserialize() {
        let json = r#"{"shell": "bash", "cwd": "/tmp"}"#;
        let params: TabCreateParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.shell.as_deref(), Some("bash"));
        assert_eq!(params.cwd.as_deref(), Some("/tmp"));
    }

    #[test]
    fn test_tab_create_params_defaults() {
        let json = r#"{}"#;
        let params: TabCreateParams = serde_json::from_str(json).unwrap();
        assert!(params.shell.is_none());
        assert!(params.cwd.is_none());
    }

    #[test]
    fn test_tab_send_params_deserialize() {
        let json = r#"{"tab_index": 0, "command": "ls -la"}"#;
        let params: TabSendParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.tab_index, Some(0));
        assert!(params.session_id.is_none());
        assert_eq!(params.command, "ls -la");
    }

    #[test]
    fn test_tab_send_params_with_session_id() {
        let json = r#"{"session_id": 42, "command": "echo hello"}"#;
        let params: TabSendParams = serde_json::from_str(json).unwrap();
        assert!(params.tab_index.is_none());
        assert_eq!(params.session_id, Some(42));
    }

    #[test]
    fn test_tab_output_params_deserialize() {
        let json = r#"{"tab_index": 1, "lines": 100, "pattern": "error"}"#;
        let params: TabOutputParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.tab_index, Some(1));
        assert_eq!(params.lines, Some(100));
        assert_eq!(params.pattern.as_deref(), Some("error"));
    }

    #[test]
    fn test_tab_output_params_minimal() {
        let json = r#"{"session_id": 5}"#;
        let params: TabOutputParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.session_id, Some(5));
        assert!(params.lines.is_none());
        assert!(params.pattern.is_none());
    }

    #[test]
    fn test_tab_close_params_deserialize() {
        let json = r#"{"tab_index": 2}"#;
        let params: TabCloseParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.tab_index, Some(2));
        assert!(params.session_id.is_none());
    }

    #[test]
    fn test_tab_output_params_mode() {
        let json = r#"{"tab_index": 0, "mode": "head", "lines": 10}"#;
        let params: TabOutputParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.mode.as_deref(), Some("head"));
        assert_eq!(params.tab_index, Some(0));
        assert_eq!(params.lines, Some(10));
    }

    #[test]
    fn test_tab_output_params_command_id() {
        let json = r#"{"command_id": 42, "lines": 20}"#;
        let params: TabOutputParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.command_id, Some(42));
        assert_eq!(params.lines, Some(20));
        assert!(params.tab_index.is_none());
    }

    #[test]
    fn test_tab_output_params_backward_compat() {
        let json = r#"{"tab_index": 0}"#;
        let params: TabOutputParams = serde_json::from_str(json).unwrap();
        assert!(params.mode.is_none());
        assert!(params.command_id.is_none());
    }

    #[test]
    fn test_cache_check_params_deserialize() {
        let json = r#"{"command_id": 42}"#;
        let params: CacheCheckParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.command_id, 42);
    }

    #[test]
    fn test_command_diff_params_deserialize() {
        let json = r#"{"command_id": 99}"#;
        let params: CommandDiffParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.command_id, 99);
    }

    #[test]
    fn test_is_binary_content_with_null_byte() {
        assert!(is_binary_content(b"hello\x00world"));
    }

    #[test]
    fn test_is_binary_content_without_null_byte() {
        assert!(!is_binary_content(b"hello world"));
    }

    #[test]
    fn test_is_binary_content_empty() {
        assert!(!is_binary_content(b""));
    }

    #[test]
    fn test_compressed_context_params_deserialize() {
        let json = r#"{"token_budget": 500, "focus": "errors"}"#;
        let params: CompressedContextParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.token_budget, 500);
        assert_eq!(params.focus.as_deref(), Some("errors"));
        assert!(params.after.is_none());
    }

    #[test]
    fn test_compressed_context_params_defaults() {
        let json = r#"{"token_budget": 1000}"#;
        let params: CompressedContextParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.token_budget, 1000);
        assert!(params.focus.is_none());
        assert!(params.after.is_none());
    }

    #[test]
    fn test_compressed_context_params_with_after() {
        let json = r#"{"token_budget": 200, "focus": "history", "after": "1h"}"#;
        let params: CompressedContextParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.token_budget, 200);
        assert_eq!(params.focus.as_deref(), Some("history"));
        assert_eq!(params.after.as_deref(), Some("1h"));
    }

    #[test]
    fn test_truncate_to_budget_long_text() {
        let text = "a".repeat(100);
        let result = truncate_to_budget(&text, 50);
        assert!(result.len() <= 50);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_to_budget_no_truncation() {
        let text = "hello world";
        let result = truncate_to_budget(text, 50);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_truncate_to_budget_exact_boundary() {
        let text = "abcde";
        let result = truncate_to_budget(text, 5);
        assert_eq!(result, "abcde");
    }

    #[test]
    fn test_extract_errors_params_deserialize() {
        let json = r#"{"output":"src/main.rs:10:5: error: test","command_hint":"rustc"}"#;
        let params: ExtractErrorsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.output, "src/main.rs:10:5: error: test");
        assert_eq!(params.command_hint, Some("rustc".to_string()));
    }

    #[test]
    fn test_extract_errors_params_no_hint() {
        let json = r#"{"output":"hello world"}"#;
        let params: ExtractErrorsParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.output, "hello world");
        assert!(params.command_hint.is_none());
    }

    #[test]
    fn test_extract_errors_empty_output() {
        let result = build_extract_errors_json("", None);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["count"], 0);
        assert!(parsed["errors"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_extract_errors_generic_gcc() {
        let output = "src/main.c:10:5: error: undeclared identifier 'foo'";
        let result = build_extract_errors_json(output, None);
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["count"], 1);
        let errors = parsed["errors"].as_array().unwrap();
        assert_eq!(errors[0]["file"], "src/main.c");
        assert_eq!(errors[0]["line"], 10);
        assert_eq!(errors[0]["column"], 5);
        assert_eq!(errors[0]["severity"], "error");
        assert_eq!(errors[0]["message"], "undeclared identifier 'foo'");
    }

    #[test]
    fn test_has_running_command_params_deserialize() {
        let json = r#"{"tab_index": 0}"#;
        let params: HasRunningCommandParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.tab_index, Some(0));
        assert!(params.session_id.is_none());

        let json = r#"{"session_id": 42}"#;
        let params: HasRunningCommandParams = serde_json::from_str(json).unwrap();
        assert!(params.tab_index.is_none());
        assert_eq!(params.session_id, Some(42));
    }

    #[test]
    fn test_has_running_command_params_empty() {
        let json = r#"{}"#;
        let params: HasRunningCommandParams = serde_json::from_str(json).unwrap();
        assert!(params.tab_index.is_none());
        assert!(params.session_id.is_none());
    }

    #[test]
    fn test_cancel_command_params_deserialize() {
        let json = r#"{"tab_index": 1}"#;
        let params: CancelCommandParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.tab_index, Some(1));
        assert!(params.session_id.is_none());

        let json = r#"{"session_id": 99}"#;
        let params: CancelCommandParams = serde_json::from_str(json).unwrap();
        assert!(params.tab_index.is_none());
        assert_eq!(params.session_id, Some(99));
    }

    #[test]
    fn test_cancel_command_params_empty() {
        let json = r#"{}"#;
        let params: CancelCommandParams = serde_json::from_str(json).unwrap();
        assert!(params.tab_index.is_none());
        assert!(params.session_id.is_none());
    }

    #[test]
    fn test_extract_errors_rust_json_cargo() {
        let output = r#"{"reason":"compiler-message","package_id":"test","manifest_path":"Cargo.toml","message":{"message":"mismatched types","code":{"code":"E0308","explanation":null},"level":"error","spans":[{"file_name":"src/main.rs","byte_start":100,"byte_end":110,"line_start":10,"line_end":10,"column_start":5,"column_end":15,"is_primary":true,"text":[],"label":null}],"children":[],"rendered":"error[E0308]"}}"#;
        let result = build_extract_errors_json(output, Some("cargo build"));
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["count"], 1);
        let errors = parsed["errors"].as_array().unwrap();
        assert_eq!(errors[0]["file"], "src/main.rs");
        assert_eq!(errors[0]["line"], 10);
        assert_eq!(errors[0]["severity"], "error");
        assert_eq!(errors[0]["code"], "E0308");
    }
}

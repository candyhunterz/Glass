//! MCP tool definitions for the Glass server.
//!
//! Defines `GlassServer` with seventeen tools:
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
}

#[tool_handler]
impl ServerHandler for GlassServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("glass-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Glass terminal server with history, undo, multi-agent coordination, and live GUI tools. \
                 Use glass_history to search commands, glass_context for activity overview, \
                 glass_undo to revert file changes, glass_file_diff to inspect pre-command file contents, \
                 glass_pipe_inspect to inspect pipeline stage output. \
                 For multi-agent coordination: glass_agent_register to join, glass_agent_lock/unlock for file locks, \
                 glass_agent_broadcast/send/messages for communication, glass_agent_heartbeat for liveness. \
                 Live GUI tools: glass_ping to check if the GUI is running and responsive.",
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
        let server = GlassServer::new(db_path.clone(), glass_dir.clone(), coord_db_path.clone(), None);
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
}

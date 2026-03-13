//! Core types for agent worktree management.

use std::path::PathBuf;

/// Whether the agent worktree is a git linked worktree or a plain directory copy.
#[derive(Debug, Clone)]
pub enum WorktreeKind {
    /// Project is a git repository; a linked worktree was created under `~/.glass/worktrees/`.
    Git { repo_path: PathBuf },
    /// Project is not a git repository; a plain directory copy was created.
    TempDir,
}

/// A live handle to a pending agent worktree.
///
/// Holds the information needed to generate diffs, apply changes to the working
/// tree, or dismiss the proposal without modifying the working tree.
#[derive(Debug)]
pub struct WorktreeHandle {
    /// UUID — used as both the git worktree name and directory name.
    pub id: String,
    /// Absolute path to the worktree directory (`~/.glass/worktrees/<id>/`).
    pub worktree_path: PathBuf,
    /// Absolute path to the project root.
    pub project_root: PathBuf,
    /// Whether this is a git linked worktree or a plain directory copy.
    pub kind: WorktreeKind,
    /// Project-relative paths of files the agent changed.
    pub changed_files: Vec<PathBuf>,
}

/// A row in the `pending_worktrees` SQLite table.
///
/// Rows survive process crashes and are used to prune orphaned worktrees on startup.
#[derive(Debug, Clone)]
pub struct PendingWorktree {
    /// UUID matching the worktree directory name and git worktree name.
    pub id: String,
    /// Absolute path to the worktree directory.
    pub worktree_path: PathBuf,
    /// Absolute path to the project root.
    pub project_root: PathBuf,
    /// Links back to the `AgentProposalData` that created this worktree.
    pub proposal_id: String,
    /// Unix timestamp when the row was created.
    pub created_at: i64,
}

/// Structured handoff data emitted by an agent at the end of a session.
///
/// The agent outputs a `GLASS_HANDOFF: {...}` marker in its final assistant
/// message. This struct is the parsed form of that JSON payload.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct HandoffData {
    /// Summary of work the agent completed in this session.
    pub work_completed: String,
    /// Summary of work that remains to be done.
    pub work_remaining: String,
    /// Key decisions or context for the next session.
    pub key_decisions: String,
    /// The `session_id` from the prior session, if this session was a continuation.
    #[serde(default)]
    pub previous_session_id: Option<String>,
}

/// A row in the `agent_sessions` SQLite table.
///
/// Rows are inserted when the agent subprocess emits a `GLASS_HANDOFF` marker.
/// The `previous_session_id` field forms a linked list across sessions.
#[derive(Debug, Clone)]
pub struct AgentSessionRecord {
    /// UUID for this record row.
    pub id: String,
    /// Canonicalized project root path (string form for SQLite storage).
    pub project_root: String,
    /// Claude session UUID from the system/init message.
    pub session_id: String,
    /// The `session_id` of the session that preceded this one (if any).
    pub previous_session_id: Option<String>,
    /// Parsed handoff data.
    pub handoff: HandoffData,
    /// Raw JSON string of the handoff marker payload.
    pub raw_handoff: String,
    /// Unix timestamp when the row was created.
    pub created_at: i64,
}

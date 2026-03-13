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

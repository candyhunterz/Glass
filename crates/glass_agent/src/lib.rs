//! `glass_agent` -- agent worktree isolation and lifecycle management.
//!
//! Provides `WorktreeManager` for creating git worktrees (or plain directory
//! copies for non-git projects) that isolate proposed agent file changes from
//! the user's working tree until explicitly approved.
//!
//! Also provides `AgentSessionDb` for persisting agent session handoff records
//! across process restarts, enabling session continuity.

pub mod session_db;
pub mod types;
pub mod worktree_db;
pub mod worktree_manager;

pub use session_db::AgentSessionDb;
pub use types::{AgentSessionRecord, HandoffData, PendingWorktree, WorktreeHandle, WorktreeKind};
pub use worktree_db::WorktreeDb;
pub use worktree_manager::WorktreeManager;

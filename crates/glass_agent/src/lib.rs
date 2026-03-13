//! `glass_agent` -- agent worktree isolation and lifecycle management.
//!
//! Provides `WorktreeManager` for creating git worktrees (or plain directory
//! copies for non-git projects) that isolate proposed agent file changes from
//! the user's working tree until explicitly approved.

pub mod types;
pub mod worktree_db;
pub mod worktree_manager;

pub use types::{PendingWorktree, WorktreeHandle, WorktreeKind};
pub use worktree_db::WorktreeDb;
pub use worktree_manager::WorktreeManager;

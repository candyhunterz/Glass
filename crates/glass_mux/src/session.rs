//! Per-session terminal state.
//!
//! `Session` holds all the state that was previously embedded in `WindowContext`,
//! enabling multiple sessions per window for tabs and split panes.

use std::sync::Arc;

use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use glass_history::db::HistoryDb;
use glass_terminal::{BlockManager, DefaultColors, EventProxy, PtySender, StatusState};

use crate::search_overlay::SearchOverlay;
use crate::types::SessionId;

/// Most recent SOI parse result for this session.
/// Updated by AppEvent::SoiReady handler in main.rs.
#[derive(Debug, Clone)]
pub struct SoiSummary {
    pub command_id: i64,
    pub one_line: String,
    pub severity: String,
}

/// A single terminal session with all associated state.
///
/// Each `Session` owns a PTY connection, terminal grid, block manager,
/// and all per-command tracking state. In single-session mode this is
/// equivalent to the old `WindowContext` fields.
pub struct Session {
    /// Unique identifier for this session.
    pub id: SessionId,
    /// Sender to write input to the PTY or resize it.
    pub pty_sender: PtySender,
    /// Shared terminal state grid.
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    /// Default terminal colors for snapshot resolution.
    pub default_colors: DefaultColors,
    /// Block manager tracking command lifecycle via shell integration.
    pub block_manager: BlockManager,
    /// Status bar state: CWD and git info.
    pub status: StatusState,
    /// History database for this session.
    pub history_db: Option<HistoryDb>,
    /// Row ID of the last inserted command, for attaching output later.
    pub last_command_id: Option<i64>,
    /// Most recent SOI parse result for this session.
    pub last_soi_summary: Option<SoiSummary>,
    /// Wall-clock time when the current command started executing.
    pub command_started_wall: Option<std::time::SystemTime>,
    /// Search overlay state. None when overlay is closed.
    pub search_overlay: Option<SearchOverlay>,
    /// Snapshot store for content-addressed file snapshots.
    pub snapshot_store: Option<glass_snapshot::SnapshotStore>,
    /// Command text extracted at CommandExecuted time.
    pub pending_command_text: Option<String>,
    /// Active filesystem watcher during command execution.
    pub active_watcher: Option<glass_snapshot::FsWatcher>,
    /// Snapshot ID created at CommandExecuted time.
    pub pending_snapshot_id: Option<i64>,
    /// Parser confidence for the pending pre-exec snapshot.
    pub pending_parse_confidence: Option<glass_snapshot::Confidence>,
    /// Current cursor position in physical pixels (for pipeline click hit testing).
    pub cursor_position: Option<(f64, f64)>,
    /// Display title for this session (e.g. tab title).
    pub title: String,
}

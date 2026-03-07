//! Tab representation (stub for Phase 23).

use crate::types::{SessionId, TabId};

/// A tab holding a reference to its session.
///
/// Each tab maps 1:1 to a session. The title is derived from the
/// session's CWD or process name and displayed in the tab bar.
pub struct Tab {
    /// Unique identifier for this tab.
    pub id: TabId,
    /// The session displayed in this tab.
    pub session_id: SessionId,
    /// Display title for this tab (e.g. CWD basename or process name).
    pub title: String,
}

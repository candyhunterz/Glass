//! Tab representation (stub for Phase 23).

use crate::types::{SessionId, TabId};

/// A tab holding a reference to its session.
///
/// In Phase 21 each tab maps 1:1 to a session. Phase 23 will add
/// split pane support where a tab may own a `SplitNode` tree.
pub struct Tab {
    /// Unique identifier for this tab.
    pub id: TabId,
    /// The session displayed in this tab.
    pub session_id: SessionId,
}

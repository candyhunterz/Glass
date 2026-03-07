//! Tab representation with split pane support.

use crate::split_tree::SplitNode;
use crate::types::{SessionId, TabId};

/// A tab holding a split pane tree.
///
/// Each tab contains a `SplitNode` root representing one or more panes
/// arranged in a binary tree. The `focused_pane` tracks which pane
/// currently receives input.
pub struct Tab {
    /// Unique identifier for this tab.
    pub id: TabId,
    /// Root of the split pane tree. Each leaf holds a SessionId.
    pub root: SplitNode,
    /// The currently focused pane's session ID.
    pub focused_pane: SessionId,
    /// Display title for this tab (e.g. CWD basename or process name).
    pub title: String,
}

impl Tab {
    /// Collect all session IDs from all leaf panes in this tab.
    pub fn session_ids(&self) -> Vec<SessionId> {
        self.root.session_ids()
    }

    /// Return the number of panes in this tab.
    pub fn pane_count(&self) -> usize {
        self.root.leaf_count()
    }
}

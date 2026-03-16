//! Session multiplexer for managing multiple terminal sessions.
//!
//! `SessionMux` wraps one or more sessions, providing tab-based navigation
//! and focus management. Tabs hold split pane trees via `SplitNode`.

use std::collections::HashMap;

use crate::session::Session;
use crate::split_tree::SplitNode;
use crate::tab::Tab;
use crate::types::{SessionId, SplitDirection, TabId};

/// Multiplexer that manages terminal sessions organized into tabs.
///
/// Each tab holds a `SplitNode` tree of panes. The focused pane in the
/// active tab receives keyboard input.
pub struct SessionMux {
    /// All sessions indexed by their unique ID.
    sessions: HashMap<SessionId, Session>,
    /// Ordered list of tabs.
    tabs: Vec<Tab>,
    /// Index of the currently active tab in `tabs`.
    active_tab: usize,
    /// Counter for generating unique session IDs.
    next_id: u64,
}

impl SessionMux {
    /// Create a new `SessionMux` with a single session.
    ///
    /// The session becomes the sole pane in the first tab.
    pub fn new(session: Session) -> Self {
        let session_id = session.id;
        let tab = Tab {
            id: TabId::new(0),
            root: SplitNode::Leaf(session_id),
            focused_pane: session_id,
            title: session.title.clone(),
        };
        let mut sessions = HashMap::new();
        sessions.insert(session_id, session);

        Self {
            sessions,
            tabs: vec![tab],
            active_tab: 0,
            next_id: session_id.val() + 1,
        }
    }

    /// Get an immutable reference to the focused session.
    pub fn focused_session(&self) -> Option<&Session> {
        let tab = self.tabs.get(self.active_tab)?;
        self.sessions.get(&tab.focused_pane)
    }

    /// Get a mutable reference to the focused session.
    pub fn focused_session_mut(&mut self) -> Option<&mut Session> {
        let focused_pane = self.tabs.get(self.active_tab)?.focused_pane;
        self.sessions.get_mut(&focused_pane)
    }

    /// Look up a session by its ID.
    pub fn session(&self, id: SessionId) -> Option<&Session> {
        self.sessions.get(&id)
    }

    /// Look up a session mutably by its ID.
    pub fn session_mut(&mut self, id: SessionId) -> Option<&mut Session> {
        self.sessions.get_mut(&id)
    }

    /// Get the SessionId of the currently focused session.
    pub fn focused_session_id(&self) -> Option<SessionId> {
        let tab = self.tabs.get(self.active_tab)?;
        Some(tab.focused_pane)
    }

    /// Generate the next unique SessionId.
    pub fn next_session_id(&mut self) -> SessionId {
        let id = SessionId::new(self.next_id);
        self.next_id += 1;
        id
    }

    /// Add a new tab with the given session.
    ///
    /// When `background` is false, the tab is inserted after the active tab
    /// and becomes active. When `background` is true, the tab is appended
    /// to the end without changing focus (used for MCP-created tabs during
    /// orchestration).
    pub fn add_tab(&mut self, session: Session, background: bool) -> TabId {
        let tab_id = TabId::new(self.next_id);
        let session_id = session.id;
        let title = session.title.clone();
        self.next_id += 1;

        let tab = Tab {
            id: tab_id,
            root: SplitNode::Leaf(session_id),
            focused_pane: session_id,
            title,
        };

        self.sessions.insert(session_id, session);

        if background {
            // Background tabs go at the end, don't change focus
            self.tabs.push(tab);
        } else {
            // Foreground tabs insert after active and become active
            let insert_pos = if self.tabs.is_empty() {
                0
            } else {
                self.active_tab + 1
            };
            self.tabs.insert(insert_pos, tab);
            self.active_tab = insert_pos;
        }

        tab_id
    }

    /// Close the tab at `index`, returning the removed sessions.
    ///
    /// Removes ALL sessions referenced by the tab's split tree.
    /// Returns the first session (for backward compatibility) or None.
    /// Adjusts `active_tab` if the closed tab was at or before it.
    pub fn close_tab(&mut self, index: usize) -> Option<Session> {
        if index >= self.tabs.len() {
            return None;
        }

        let tab = self.tabs.remove(index);

        // Remove all sessions in the tab's split tree
        let session_ids = tab.session_ids();
        let mut first_session = None;
        for (i, sid) in session_ids.iter().enumerate() {
            let removed = self.sessions.remove(sid);
            if i == 0 {
                first_session = removed;
            }
        }

        // Adjust active_tab after removal
        if self.tabs.is_empty() {
            self.active_tab = 0;
        } else if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if index < self.active_tab {
            self.active_tab -= 1;
        }

        first_session
    }

    /// Split the focused pane in the active tab.
    ///
    /// Replaces the focused Leaf with a Split node where left=old session,
    /// right=new session. Sets focused_pane to new session. Returns new session_id.
    pub fn split_pane(&mut self, direction: SplitDirection, new_session: Session) -> SessionId {
        let new_id = new_session.id;
        self.sessions.insert(new_id, new_session);

        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            let target = tab.focused_pane;
            tab.root.split_leaf(target, direction, new_id);
            tab.focused_pane = new_id;
        }

        new_id
    }

    /// Remove a pane from the active tab's split tree.
    ///
    /// If the tree becomes empty (last pane), closes the entire tab.
    /// Otherwise updates focused_pane to the first leaf of the remaining tree.
    /// Returns the removed session.
    pub fn close_pane(&mut self, session_id: SessionId) -> Option<Session> {
        let tab = self.tabs.get_mut(self.active_tab)?;

        // Take ownership of the root to call remove_leaf (which consumes self)
        let old_root = std::mem::replace(&mut tab.root, SplitNode::Leaf(session_id));
        match old_root.remove_leaf(session_id) {
            Some(new_root) => {
                // Update focused_pane to first leaf of remaining tree
                let new_focus = new_root.first_leaf();
                tab.root = new_root;
                tab.focused_pane = new_focus;
                self.sessions.remove(&session_id)
            }
            None => {
                // Last pane removed -- close the entire tab
                // Restore the root temporarily (close_tab will remove it)
                tab.root = SplitNode::Leaf(session_id);
                self.close_tab(self.active_tab)
            }
        }
    }

    /// Return the active tab's SplitNode root for layout computation.
    pub fn active_tab_root(&self) -> Option<&SplitNode> {
        self.tabs.get(self.active_tab).map(|t| &t.root)
    }

    /// Activate the tab at `index`. No-op if index is out of bounds.
    pub fn activate_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = index;
        }
    }

    /// Cycle to the next tab with wraparound.
    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab = (self.active_tab + 1) % self.tabs.len();
        }
    }

    /// Cycle to the previous tab with wraparound.
    pub fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            if self.active_tab == 0 {
                self.active_tab = self.tabs.len() - 1;
            } else {
                self.active_tab -= 1;
            }
        }
    }

    /// Return the number of tabs.
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Return the index of the currently active tab.
    pub fn active_tab_index(&self) -> usize {
        self.active_tab
    }

    /// Return a slice of all tabs in order.
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Return a mutable reference to the tabs vector (for updating tab titles).
    pub fn tabs_mut(&mut self) -> &mut Vec<Tab> {
        &mut self.tabs
    }

    /// Set the focused pane in the active tab. No-op if the session
    /// doesn't exist in the tab's split tree.
    pub fn set_focused_pane(&mut self, session_id: SessionId) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            if tab.root.contains(session_id) {
                tab.focused_pane = session_id;
            }
        }
    }

    /// Resize the split ratio of the nearest ancestor Split matching
    /// `direction` around the focused pane. Delta is typically +/- 0.05.
    pub fn resize_focused_split(&mut self, direction: SplitDirection, delta: f32) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            let focused = tab.focused_pane;
            tab.root.resize_ratio(focused, direction, delta);
        }
    }

    /// Move the tab at `from` to the position `to`.
    ///
    /// `to` is the final position index (post-removal). This means
    /// `remove(from)` then `insert(to, tab)` is the correct sequence.
    /// Adjusts `active_tab` to follow the active tab's new position.
    /// No-op if `from == to` or either index is out of bounds.
    pub fn reorder_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return;
        }

        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);

        // Adjust active_tab to track correctly
        if self.active_tab == from {
            // The active tab was the one moved
            self.active_tab = to;
        } else if from < self.active_tab && to >= self.active_tab {
            // Tab removed before active, inserted at/after -> active shifts down
            self.active_tab -= 1;
        } else if from > self.active_tab && to <= self.active_tab {
            // Tab removed after active, inserted at/before -> active shifts up
            self.active_tab += 1;
        }
    }

    /// Return the number of panes in the active tab.
    pub fn active_tab_pane_count(&self) -> usize {
        self.tabs
            .get(self.active_tab)
            .map(|t| t.pane_count())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SessionId;

    /// Create a SessionMux with `n` tabs for testing tab index logic.
    ///
    /// Sessions are not real (the HashMap will be empty), but tabs are
    /// properly constructed with SplitNode::Leaf roots.
    fn test_mux(n: usize) -> SessionMux {
        let tabs: Vec<Tab> = (0..n)
            .map(|i| {
                let sid = SessionId::new(i as u64);
                Tab {
                    id: TabId::new(i as u64),
                    root: SplitNode::Leaf(sid),
                    focused_pane: sid,
                    title: format!("Tab {}", i),
                }
            })
            .collect();
        SessionMux {
            sessions: HashMap::new(),
            tabs,
            active_tab: 0,
            next_id: n as u64,
        }
    }

    #[test]
    fn next_session_id_increments() {
        let id1 = SessionId::new(0);
        let id2 = SessionId::new(1);
        assert_ne!(id1, id2);
        assert_eq!(id1.val(), 0);
        assert_eq!(id2.val(), 1);
    }

    #[test]
    fn tab_count_returns_correct_count() {
        let mux = test_mux(3);
        assert_eq!(mux.tab_count(), 3);
    }

    #[test]
    fn tab_count_empty() {
        let mux = test_mux(0);
        assert_eq!(mux.tab_count(), 0);
    }

    #[test]
    fn tabs_returns_slice() {
        let mux = test_mux(3);
        let tabs = mux.tabs();
        assert_eq!(tabs.len(), 3);
        assert_eq!(tabs[0].title, "Tab 0");
        assert_eq!(tabs[2].title, "Tab 2");
    }

    #[test]
    fn active_tab_index_default() {
        let mux = test_mux(3);
        assert_eq!(mux.active_tab_index(), 0);
    }

    #[test]
    fn activate_tab_sets_active() {
        let mut mux = test_mux(3);
        mux.activate_tab(2);
        assert_eq!(mux.active_tab_index(), 2);
    }

    #[test]
    fn activate_tab_invalid_noop() {
        let mut mux = test_mux(3);
        mux.activate_tab(1);
        mux.activate_tab(99); // out of bounds
        assert_eq!(mux.active_tab_index(), 1); // unchanged
    }

    #[test]
    fn next_tab_cycles_forward() {
        let mut mux = test_mux(3);
        mux.next_tab();
        assert_eq!(mux.active_tab_index(), 1);
        mux.next_tab();
        assert_eq!(mux.active_tab_index(), 2);
    }

    #[test]
    fn next_tab_wraps_around() {
        let mut mux = test_mux(3);
        mux.activate_tab(2);
        mux.next_tab();
        assert_eq!(mux.active_tab_index(), 0);
    }

    #[test]
    fn prev_tab_cycles_backward() {
        let mut mux = test_mux(3);
        mux.activate_tab(2);
        mux.prev_tab();
        assert_eq!(mux.active_tab_index(), 1);
    }

    #[test]
    fn prev_tab_wraps_around() {
        let mut mux = test_mux(3);
        mux.prev_tab();
        assert_eq!(mux.active_tab_index(), 2);
    }

    #[test]
    fn close_tab_removes_and_adjusts_active() {
        let mut mux = test_mux(3);
        mux.activate_tab(2);
        // Close middle tab (index 1), active was 2 -> should become 1
        let removed = mux.close_tab(1);
        assert!(removed.is_none()); // no real sessions in test_mux
        assert_eq!(mux.tab_count(), 2);
        assert_eq!(mux.active_tab_index(), 1);
    }

    #[test]
    fn close_tab_out_of_bounds() {
        let mut mux = test_mux(3);
        let removed = mux.close_tab(99);
        assert!(removed.is_none());
        assert_eq!(mux.tab_count(), 3);
    }

    #[test]
    fn close_tab_last_remaining() {
        let mut mux = test_mux(1);
        let _removed = mux.close_tab(0);
        assert_eq!(mux.tab_count(), 0);
        assert_eq!(mux.active_tab_index(), 0);
    }

    #[test]
    fn close_tab_active_at_end_adjusts() {
        let mut mux = test_mux(3);
        mux.activate_tab(2);
        // Close the active (last) tab
        let _removed = mux.close_tab(2);
        assert_eq!(mux.tab_count(), 2);
        // active_tab should clamp to new last index
        assert_eq!(mux.active_tab_index(), 1);
    }

    #[test]
    fn tab_has_title_field() {
        let sid = SessionId::new(0);
        let tab = Tab {
            id: TabId::new(0),
            root: SplitNode::Leaf(sid),
            focused_pane: sid,
            title: "My Tab".to_string(),
        };
        assert_eq!(tab.title, "My Tab");
    }

    // ---- SPLIT-08: Tab with SplitNode tracks focused_pane correctly ----

    #[test]
    fn tab_session_ids_single_pane() {
        let sid = SessionId::new(42);
        let tab = Tab {
            id: TabId::new(0),
            root: SplitNode::Leaf(sid),
            focused_pane: sid,
            title: "test".into(),
        };
        assert_eq!(tab.session_ids(), vec![sid]);
        assert_eq!(tab.pane_count(), 1);
    }

    #[test]
    fn tab_session_ids_after_split() {
        let sid1 = SessionId::new(1);
        let sid2 = SessionId::new(2);
        let tab = Tab {
            id: TabId::new(0),
            root: SplitNode::Split {
                direction: SplitDirection::Horizontal,
                left: Box::new(SplitNode::Leaf(sid1)),
                right: Box::new(SplitNode::Leaf(sid2)),
                ratio: 0.5,
            },
            focused_pane: sid2,
            title: "test".into(),
        };
        let ids = tab.session_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&sid1));
        assert!(ids.contains(&sid2));
        assert_eq!(tab.pane_count(), 2);
    }

    #[test]
    fn active_tab_root_returns_split_node() {
        let mux = test_mux(2);
        let root = mux.active_tab_root().unwrap();
        assert_eq!(root.leaf_count(), 1);
    }

    // ---- SPLIT-11: Closing last pane closes tab ----

    #[test]
    fn close_pane_last_pane_closes_tab() {
        // Create a real SessionMux with 2 tabs, each having 1 pane
        let sid1 = SessionId::new(0);
        let sid2 = SessionId::new(1);
        let tab1 = Tab {
            id: TabId::new(0),
            root: SplitNode::Leaf(sid1),
            focused_pane: sid1,
            title: "Tab 0".into(),
        };
        let tab2 = Tab {
            id: TabId::new(1),
            root: SplitNode::Leaf(sid2),
            focused_pane: sid2,
            title: "Tab 1".into(),
        };
        let mut mux = SessionMux {
            sessions: HashMap::new(),
            tabs: vec![tab1, tab2],
            active_tab: 0,
            next_id: 2,
        };
        assert_eq!(mux.tab_count(), 2);

        // Close the last (only) pane in tab 0 -> should close the tab
        let _removed = mux.close_pane(sid1);
        assert_eq!(mux.tab_count(), 1);
        // Remaining tab should be tab2
        assert_eq!(mux.tabs()[0].title, "Tab 1");
    }

    #[test]
    fn close_pane_two_pane_split_leaves_single_pane() {
        // Create a tab with a horizontal split (2 panes)
        let sid1 = SessionId::new(10);
        let sid2 = SessionId::new(11);
        let tab = Tab {
            id: TabId::new(0),
            root: SplitNode::Split {
                direction: SplitDirection::Horizontal,
                left: Box::new(SplitNode::Leaf(sid1)),
                right: Box::new(SplitNode::Leaf(sid2)),
                ratio: 0.5,
            },
            focused_pane: sid2,
            title: "Split Tab".into(),
        };
        let mut mux = SessionMux {
            sessions: HashMap::new(),
            tabs: vec![tab],
            active_tab: 0,
            next_id: 12,
        };
        assert_eq!(mux.tab_count(), 1);
        assert_eq!(mux.tabs()[0].pane_count(), 2);

        // Close one pane -> tab remains with 1 pane
        let _removed = mux.close_pane(sid2);
        assert_eq!(mux.tab_count(), 1);
        assert_eq!(mux.tabs()[0].pane_count(), 1);
        // Focus should move to sid1
        assert_eq!(mux.focused_session_id(), Some(sid1));
    }

    // ---- SessionMux helper methods ----

    #[test]
    fn set_focused_pane_changes_focus() {
        let sid1 = SessionId::new(10);
        let sid2 = SessionId::new(11);
        let tab = Tab {
            id: TabId::new(0),
            root: SplitNode::Split {
                direction: SplitDirection::Horizontal,
                left: Box::new(SplitNode::Leaf(sid1)),
                right: Box::new(SplitNode::Leaf(sid2)),
                ratio: 0.5,
            },
            focused_pane: sid1,
            title: "test".into(),
        };
        let mut mux = SessionMux {
            sessions: HashMap::new(),
            tabs: vec![tab],
            active_tab: 0,
            next_id: 12,
        };
        assert_eq!(mux.focused_session_id(), Some(sid1));
        mux.set_focused_pane(sid2);
        assert_eq!(mux.focused_session_id(), Some(sid2));
    }

    // ---- Tab reorder tests ----

    #[test]
    fn reorder_tab_forward() {
        // [A, B, C] -> reorder(0, 2) -> [B, C, A]
        let mut mux = test_mux(3);
        mux.reorder_tab(0, 2);
        assert_eq!(mux.tabs()[0].title, "Tab 1");
        assert_eq!(mux.tabs()[1].title, "Tab 2");
        assert_eq!(mux.tabs()[2].title, "Tab 0");
    }

    #[test]
    fn reorder_tab_backward() {
        // [A, B, C] -> reorder(2, 0) -> [C, A, B]
        let mut mux = test_mux(3);
        mux.reorder_tab(2, 0);
        assert_eq!(mux.tabs()[0].title, "Tab 2");
        assert_eq!(mux.tabs()[1].title, "Tab 0");
        assert_eq!(mux.tabs()[2].title, "Tab 1");
    }

    #[test]
    fn reorder_tab_same_index_noop() {
        let mut mux = test_mux(3);
        mux.reorder_tab(1, 1);
        assert_eq!(mux.tabs()[0].title, "Tab 0");
        assert_eq!(mux.tabs()[1].title, "Tab 1");
        assert_eq!(mux.tabs()[2].title, "Tab 2");
    }

    #[test]
    fn reorder_tab_active_follows_moved_tab() {
        let mut mux = test_mux(3);
        mux.activate_tab(0); // active is tab 0
        mux.reorder_tab(0, 2); // move tab 0 to position 2
        assert_eq!(mux.active_tab_index(), 2); // active follows
    }

    #[test]
    fn reorder_tab_active_shifts_when_between_forward() {
        // active_tab=1, reorder(0, 2): tab removed before active, inserted after
        // active should shift from 1 to 0
        let mut mux = test_mux(3);
        mux.activate_tab(1);
        mux.reorder_tab(0, 2);
        assert_eq!(mux.active_tab_index(), 0);
    }

    #[test]
    fn reorder_tab_active_shifts_when_between_backward() {
        // active_tab=1, reorder(2, 0): tab removed after active, inserted before
        // active should shift from 1 to 2
        let mut mux = test_mux(3);
        mux.activate_tab(1);
        mux.reorder_tab(2, 0);
        assert_eq!(mux.active_tab_index(), 2);
    }

    #[test]
    fn reorder_tab_out_of_bounds_noop() {
        let mut mux = test_mux(3);
        mux.reorder_tab(5, 0); // from out of bounds
        assert_eq!(mux.tabs()[0].title, "Tab 0");
        mux.reorder_tab(0, 5); // to out of bounds
        assert_eq!(mux.tabs()[0].title, "Tab 0");
    }

    #[test]
    fn reorder_tab_single_tab_noop() {
        let mut mux = test_mux(1);
        mux.reorder_tab(0, 0);
        assert_eq!(mux.tabs()[0].title, "Tab 0");
    }

    #[test]
    fn set_focused_pane_invalid_noop() {
        let sid1 = SessionId::new(10);
        let tab = Tab {
            id: TabId::new(0),
            root: SplitNode::Leaf(sid1),
            focused_pane: sid1,
            title: "test".into(),
        };
        let mut mux = SessionMux {
            sessions: HashMap::new(),
            tabs: vec![tab],
            active_tab: 0,
            next_id: 11,
        };
        mux.set_focused_pane(SessionId::new(99)); // not in tree
        assert_eq!(mux.focused_session_id(), Some(sid1)); // unchanged
    }

    #[test]
    fn focused_session_uses_focused_pane() {
        // Create a tab with 2 panes, focused_pane on the second
        let sid1 = SessionId::new(10);
        let sid2 = SessionId::new(11);
        let tab = Tab {
            id: TabId::new(0),
            root: SplitNode::Split {
                direction: SplitDirection::Horizontal,
                left: Box::new(SplitNode::Leaf(sid1)),
                right: Box::new(SplitNode::Leaf(sid2)),
                ratio: 0.5,
            },
            focused_pane: sid2,
            title: "test".into(),
        };
        let mux = SessionMux {
            sessions: HashMap::new(),
            tabs: vec![tab],
            active_tab: 0,
            next_id: 12,
        };
        assert_eq!(mux.focused_session_id(), Some(sid2));
    }
}

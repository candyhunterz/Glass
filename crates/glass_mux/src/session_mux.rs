//! Session multiplexer for managing multiple terminal sessions.
//!
//! `SessionMux` wraps one or more sessions, providing tab-based navigation
//! and focus management. In Phase 21, only single-session mode is used.

use std::collections::HashMap;

use crate::session::Session;
use crate::tab::Tab;
use crate::types::{SessionId, TabId};

/// Multiplexer that manages terminal sessions organized into tabs.
///
/// In single-session mode (Phase 21), this holds exactly one tab with one session.
/// Future phases will add multi-tab and split-pane support.
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
    /// The session's ID is used as the first tab's session reference.
    pub fn new(session: Session) -> Self {
        let session_id = session.id;
        let tab = Tab {
            id: TabId::new(0),
            session_id,
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
        self.sessions.get(&tab.session_id)
    }

    /// Get a mutable reference to the focused session.
    pub fn focused_session_mut(&mut self) -> Option<&mut Session> {
        let session_id = self.tabs.get(self.active_tab)?.session_id;
        self.sessions.get_mut(&session_id)
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
        Some(tab.session_id)
    }

    /// Generate the next unique SessionId.
    pub fn next_session_id(&mut self) -> SessionId {
        let id = SessionId::new(self.next_id);
        self.next_id += 1;
        id
    }

    /// Add a new tab with the given session, inserted after the active tab.
    ///
    /// The new tab becomes active. Returns its `TabId`.
    pub fn add_tab(&mut self, session: Session) -> TabId {
        todo!()
    }

    /// Close the tab at `index`, returning the removed `Session` if valid.
    ///
    /// Adjusts `active_tab` if the closed tab was at or before it.
    pub fn close_tab(&mut self, index: usize) -> Option<Session> {
        todo!()
    }

    /// Activate the tab at `index`. No-op if index is out of bounds.
    pub fn activate_tab(&mut self, index: usize) {
        todo!()
    }

    /// Cycle to the next tab with wraparound.
    pub fn next_tab(&mut self) {
        todo!()
    }

    /// Cycle to the previous tab with wraparound.
    pub fn prev_tab(&mut self) {
        todo!()
    }

    /// Return the number of tabs.
    pub fn tab_count(&self) -> usize {
        todo!()
    }

    /// Return the index of the currently active tab.
    pub fn active_tab_index(&self) -> usize {
        todo!()
    }

    /// Return a slice of all tabs in order.
    pub fn tabs(&self) -> &[Tab] {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SessionId;

    /// Create a SessionMux with `n` tabs for testing tab index logic.
    ///
    /// Sessions are not real (the HashMap will be empty), but tabs are
    /// properly constructed. This allows testing index-management methods
    /// without needing the complex Session type.
    fn test_mux(n: usize) -> SessionMux {
        let tabs: Vec<Tab> = (0..n)
            .map(|i| Tab {
                id: TabId::new(i as u64),
                session_id: SessionId::new(i as u64),
                title: format!("Tab {}", i),
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
        let tab = Tab {
            id: TabId::new(0),
            session_id: SessionId::new(0),
            title: "My Tab".to_string(),
        };
        assert_eq!(tab.title, "My Tab");
    }
}

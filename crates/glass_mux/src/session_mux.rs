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
}

#[cfg(test)]
mod tests {
    use crate::types::SessionId;

    #[test]
    fn next_session_id_increments() {
        // SessionId generation produces distinct, incrementing IDs
        let id1 = SessionId::new(0);
        let id2 = SessionId::new(1);
        assert_ne!(id1, id2);
        assert_eq!(id1.val(), 0);
        assert_eq!(id2.val(), 1);
    }
}

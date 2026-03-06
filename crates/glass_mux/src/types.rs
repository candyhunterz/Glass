//! Core type definitions for the glass_mux crate.

use std::fmt;

/// Unique identifier for a terminal session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionId(u64);

impl SessionId {
    /// Create a new SessionId from a u64.
    pub fn new(n: u64) -> Self {
        Self(n)
    }

    /// Return the inner u64 value.
    pub fn val(self) -> u64 {
        self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "session-{}", self.0)
    }
}

/// Unique identifier for a tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TabId(u64);

impl TabId {
    /// Create a new TabId from a u64.
    pub fn new(n: u64) -> Self {
        Self(n)
    }

    /// Return the inner u64 value.
    pub fn val(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TabId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tab-{}", self.0)
    }
}

/// Direction for splitting a pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// Direction for moving focus between panes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusDirection {
    Up,
    Down,
    Left,
    Right,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn session_id_new_and_val() {
        let id = SessionId::new(42);
        assert_eq!(id.val(), 42);
    }

    #[test]
    fn session_id_display() {
        let id = SessionId::new(7);
        assert_eq!(format!("{}", id), "session-7");
    }

    #[test]
    fn session_id_copy_clone() {
        let id = SessionId::new(1);
        let id2 = id; // Copy
        let id3 = id.clone(); // Clone
        assert_eq!(id, id2);
        assert_eq!(id, id3);
    }

    #[test]
    fn session_id_eq_hash() {
        let a = SessionId::new(5);
        let b = SessionId::new(5);
        let c = SessionId::new(6);
        assert_eq!(a, b);
        assert_ne!(a, c);

        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
        assert!(!set.contains(&c));
    }

    #[test]
    fn tab_id_new_and_val() {
        let id = TabId::new(10);
        assert_eq!(id.val(), 10);
    }

    #[test]
    fn tab_id_display() {
        let id = TabId::new(3);
        assert_eq!(format!("{}", id), "tab-3");
    }

    #[test]
    fn split_direction_variants() {
        let h = SplitDirection::Horizontal;
        let v = SplitDirection::Vertical;
        assert_ne!(h, v);
        let h2 = h; // Copy
        assert_eq!(h, h2);
    }

    #[test]
    fn focus_direction_variants() {
        let dirs = [
            FocusDirection::Up,
            FocusDirection::Down,
            FocusDirection::Left,
            FocusDirection::Right,
        ];
        // All distinct
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
        }
    }
}

//! Persistent app state tracking across sessions.
//!
//! Stores session count and first-run detection in `~/.glass/state.toml`.
//! This file is separate from config.toml to avoid conflation of user
//! preferences with internal state.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Persistent Glass application state, stored in `~/.glass/state.toml`.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct GlassState {
    /// Number of sessions (app launches) completed.
    #[serde(default)]
    pub session_count: u32,
}

impl GlassState {
    /// Returns the path to `~/.glass/state.toml`, or None if home dir is unavailable.
    pub fn state_path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".glass").join("state.toml"))
    }

    /// Load state from `~/.glass/state.toml`. Returns defaults on any error.
    pub fn load() -> Self {
        let Some(path) = Self::state_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save state to `~/.glass/state.toml`. Silently ignores errors.
    pub fn save(&self) {
        let Some(path) = Self::state_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::warn!("Failed to create state directory {}: {e}", parent.display());
            }
        }
        if let Ok(contents) = toml::to_string_pretty(self) {
            if let Err(e) = std::fs::write(&path, contents) {
                tracing::warn!("Failed to write state file {}: {e}", path.display());
            }
        }
    }

    /// True if this is the very first launch (session_count == 0 before increment).
    pub fn is_first_run(&self) -> bool {
        self.session_count == 0
    }

    /// True if the settings hint should be shown (first 3 sessions).
    pub fn should_show_hint(&self) -> bool {
        self.session_count <= 3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_first_run() {
        let state = GlassState::default();
        assert!(state.is_first_run());
        assert!(state.should_show_hint());
    }

    #[test]
    fn not_first_run_after_increment() {
        let mut state = GlassState::default();
        state.session_count += 1;
        assert!(!state.is_first_run());
        assert!(state.should_show_hint()); // still within first 3
    }

    #[test]
    fn hint_stops_after_three_sessions() {
        let state = GlassState { session_count: 4 };
        assert!(!state.is_first_run());
        assert!(!state.should_show_hint());
    }

    #[test]
    fn roundtrip_serialize() {
        let state = GlassState { session_count: 42 };
        let toml_str = toml::to_string_pretty(&state).unwrap();
        let loaded: GlassState = toml::from_str(&toml_str).unwrap();
        assert_eq!(loaded.session_count, 42);
    }
}

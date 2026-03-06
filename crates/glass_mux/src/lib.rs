//! glass_mux --- Session multiplexer for tabs and split panes.
//!
//! This crate provides:
//! - `Session`: per-terminal state extracted from `WindowContext`
//! - `SessionMux`: multiplexer managing sessions organized into tabs
//! - `Tab`, `SplitNode`, `ViewportLayout`: stub types for future phases
//! - Platform helpers for cross-platform shell and shortcut detection

pub mod layout;
pub mod platform;
pub mod search_overlay;
pub mod session;
pub mod session_mux;
pub mod split_tree;
pub mod tab;
pub mod types;

pub use layout::ViewportLayout;
pub use platform::{config_dir, data_dir, default_shell, is_action_modifier, is_glass_shortcut};
pub use search_overlay::{SearchOverlay, SearchOverlayData, SearchResultDisplay};
pub use session::Session;
pub use session_mux::SessionMux;
pub use split_tree::SplitNode;
pub use tab::Tab;
pub use types::{FocusDirection, SessionId, SplitDirection, TabId};

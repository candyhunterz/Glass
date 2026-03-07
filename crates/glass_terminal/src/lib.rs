//! glass_terminal — PTY management via alacritty_terminal.
//!
//! This crate provides:
//! - `EventProxy`: bridges PTY reader thread events to the winit event loop
//! - `spawn_pty`: spawns PowerShell via ConPTY and starts the dedicated reader thread

pub mod block_manager;
pub mod event_proxy;
pub mod grid_snapshot;
pub mod input;
pub mod osc_scanner;
pub mod output_capture;
pub mod pty;
pub mod status;

pub use block_manager::{Block, BlockManager, BlockState, PipelineHit, format_duration};
pub use event_proxy::EventProxy;
pub use grid_snapshot::{DefaultColors, GridSnapshot, RenderedCell, resolve_color, snapshot_term};
pub use input::encode_key;
pub use osc_scanner::{OscEvent, OscScanner};
pub use pty::{spawn_pty, PtyMsg, PtySender};
pub use status::{GitInfo, StatusState, query_git_status};

#[cfg(test)]
mod tests;

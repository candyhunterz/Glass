//! glass_terminal — PTY management via alacritty_terminal.
//!
//! This crate provides:
//! - `EventProxy`: bridges PTY reader thread events to the winit event loop
//! - `spawn_pty`: spawns PowerShell via ConPTY and starts the dedicated reader thread

pub mod event_proxy;
pub mod pty;

pub use event_proxy::EventProxy;
pub use pty::spawn_pty;

#[cfg(test)]
mod tests;

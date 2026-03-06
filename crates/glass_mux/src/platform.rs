//! Platform-specific helpers for cross-platform behavior.
//!
//! Provides cfg-gated functions for shell detection, modifier key handling,
//! and platform-appropriate directory paths.

use std::path::PathBuf;

use winit::keyboard::ModifiersState;

/// Return the default shell for the current platform.
///
/// - Windows: probes for `pwsh`, falls back to `powershell`
/// - macOS: `$SHELL` or `/bin/zsh`
/// - Linux: `$SHELL` or `/bin/bash`
#[cfg(target_os = "windows")]
pub fn default_shell() -> String {
    match std::process::Command::new("pwsh")
        .arg("--version")
        .output()
    {
        Ok(output) if output.status.success() => "pwsh".to_string(),
        _ => "powershell".to_string(),
    }
}

#[cfg(target_os = "macos")]
pub fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())
}

#[cfg(target_os = "linux")]
pub fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
}

/// Check if the platform's primary action modifier is pressed.
///
/// - macOS: Meta (Cmd)
/// - Windows/Linux: Ctrl
#[cfg(target_os = "macos")]
pub fn is_action_modifier(mods: ModifiersState) -> bool {
    mods.super_key()
}

#[cfg(not(target_os = "macos"))]
pub fn is_action_modifier(mods: ModifiersState) -> bool {
    mods.control_key()
}

/// Check if the Glass-specific shortcut modifier combination is pressed.
///
/// - macOS: Meta (Cmd)
/// - Windows/Linux: Ctrl+Shift
#[cfg(target_os = "macos")]
pub fn is_glass_shortcut(mods: ModifiersState) -> bool {
    mods.super_key()
}

#[cfg(not(target_os = "macos"))]
pub fn is_glass_shortcut(mods: ModifiersState) -> bool {
    mods.control_key() && mods.shift_key()
}

/// Return the platform-appropriate configuration directory with "glass" subfolder.
pub fn config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("glass"))
}

/// Return the platform-appropriate data directory with "glass" subfolder.
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|p| p.join("glass"))
}

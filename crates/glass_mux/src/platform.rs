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
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW (0x08000000) prevents a visible console flash
    // when probing for pwsh from a GUI subsystem process.
    match std::process::Command::new("pwsh")
        .arg("--version")
        .creation_flags(0x08000000)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shell_returns_valid_value() {
        let shell = default_shell();
        assert!(!shell.is_empty());
        // On Windows, should be pwsh or powershell
        #[cfg(target_os = "windows")]
        assert!(
            shell == "pwsh" || shell == "powershell",
            "Expected pwsh or powershell, got: {}",
            shell
        );
        #[cfg(target_os = "macos")]
        assert!(
            shell.starts_with('/') || !shell.is_empty(),
            "Expected path or env var, got: {}",
            shell
        );
        #[cfg(target_os = "linux")]
        assert!(
            shell.starts_with('/') || !shell.is_empty(),
            "Expected path or env var, got: {}",
            shell
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn is_action_modifier_ctrl_on_windows_linux() {
        let ctrl = ModifiersState::CONTROL;
        assert!(is_action_modifier(ctrl));

        let empty = ModifiersState::empty();
        assert!(!is_action_modifier(empty));

        let shift_only = ModifiersState::SHIFT;
        assert!(!is_action_modifier(shift_only));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn is_glass_shortcut_ctrl_shift_on_windows_linux() {
        let ctrl_shift = ModifiersState::CONTROL | ModifiersState::SHIFT;
        assert!(is_glass_shortcut(ctrl_shift));

        let ctrl_only = ModifiersState::CONTROL;
        assert!(!is_glass_shortcut(ctrl_only));

        let shift_only = ModifiersState::SHIFT;
        assert!(!is_glass_shortcut(shift_only));

        let empty = ModifiersState::empty();
        assert!(!is_glass_shortcut(empty));
    }

    #[test]
    fn config_dir_returns_glass_subfolder() {
        let dir = config_dir();
        assert!(
            dir.is_some(),
            "config_dir should return Some on this platform"
        );
        let path = dir.unwrap();
        assert!(
            path.ends_with("glass"),
            "config_dir should end with 'glass', got: {}",
            path.display()
        );
    }

    #[test]
    fn data_dir_returns_glass_subfolder() {
        let dir = data_dir();
        assert!(
            dir.is_some(),
            "data_dir should return Some on this platform"
        );
        let path = dir.unwrap();
        assert!(
            path.ends_with("glass"),
            "data_dir should end with 'glass', got: {}",
            path.display()
        );
    }
}

//! Keyboard input encoding for terminal escape sequences.
//!
//! Translates winit `Key` events + modifier state + terminal mode into byte sequences
//! to send to the PTY. Handles Ctrl+letter, Alt+key, arrow keys (normal + app cursor),
//! function keys F1-F12, Home/End/PageUp/PageDown/Insert/Delete, Enter/Tab/Backspace/Escape.
//!
//! Returns `None` for keys that Glass handles internally (clipboard, scrollback).

use alacritty_terminal::term::TermMode;
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// Encode a keyboard event into bytes to send to the PTY.
///
/// Returns `None` for keys handled by Glass (Ctrl+Shift+C/V, Shift+PageUp/Down)
/// or keys that produce no output.
pub fn encode_key(key: &Key, modifiers: ModifiersState, mode: TermMode) -> Option<Vec<u8>> {
    // Intercept Glass-handled keys: return None so main.rs handles them
    if modifiers.control_key() && modifiers.shift_key() {
        if let Key::Character(c) = key {
            let ch = c.as_str();
            if ch.eq_ignore_ascii_case("c") || ch.eq_ignore_ascii_case("v") {
                return None;
            }
        }
    }

    // Shift+PageUp/Down: scrollback, not forwarded to PTY
    if modifiers.shift_key() && !modifiers.control_key() && !modifiers.alt_key() {
        if let Key::Named(NamedKey::PageUp | NamedKey::PageDown) = key {
            return None;
        }
    }

    match key {
        // Ctrl+letter: send ASCII control character (letter & 0x1f)
        Key::Character(c) if modifiers.control_key() && !modifiers.shift_key() => {
            let ch = c.chars().next()?;
            if ch.is_ascii_alphabetic() {
                Some(vec![(ch.to_ascii_lowercase() as u8) & 0x1f])
            } else {
                match ch {
                    '[' | '3' => Some(vec![0x1b]),  // ESC
                    '\\' | '4' => Some(vec![0x1c]), // FS
                    ']' | '5' => Some(vec![0x1d]),  // GS
                    '6' => Some(vec![0x1e]),        // RS
                    '/' | '7' => Some(vec![0x1f]),  // US
                    '8' => Some(vec![0x7f]),        // DEL
                    _ => None,
                }
            }
        }
        // Alt+key: send ESC prefix then the character
        Key::Character(c) if modifiers.alt_key() && !modifiers.control_key() => {
            let mut bytes = vec![0x1b];
            bytes.extend(c.as_bytes());
            Some(bytes)
        }
        // Named keys with modifier encoding
        Key::Named(named) => encode_named_key(*named, modifiers, mode),
        // Plain character (no relevant modifiers)
        Key::Character(c) => Some(c.as_bytes().to_vec()),
        _ => None,
    }
}

/// Encode named keys (arrows, function keys, etc.) with modifier parameters.
fn encode_named_key(key: NamedKey, mods: ModifiersState, mode: TermMode) -> Option<Vec<u8>> {
    let modifier_param = modifier_code(mods);
    let app_cursor = mode.contains(TermMode::APP_CURSOR);

    match key {
        // Arrow keys
        NamedKey::ArrowUp => Some(arrow_seq(b'A', modifier_param, app_cursor)),
        NamedKey::ArrowDown => Some(arrow_seq(b'B', modifier_param, app_cursor)),
        NamedKey::ArrowRight => Some(arrow_seq(b'C', modifier_param, app_cursor)),
        NamedKey::ArrowLeft => Some(arrow_seq(b'D', modifier_param, app_cursor)),

        // Simple keys
        NamedKey::Enter if mods.control_key() => Some(vec![0x0a]), // Ctrl+Enter: LF (newline)
        NamedKey::Enter => Some(vec![0x0d]),                       // CR
        NamedKey::Tab => Some(vec![0x09]),                         // HT
        NamedKey::Backspace => Some(vec![0x7f]),                   // DEL
        NamedKey::Escape => Some(vec![0x1b]),                      // ESC
        NamedKey::Space => Some(vec![0x20]),                       // Space

        // Navigation keys (CSI tilde sequences)
        NamedKey::Home => Some(csi_tilde(1, modifier_param)),
        NamedKey::Insert => Some(csi_tilde(2, modifier_param)),
        NamedKey::Delete => Some(csi_tilde(3, modifier_param)),
        NamedKey::End => Some(csi_tilde(4, modifier_param)),
        NamedKey::PageUp => Some(csi_tilde(5, modifier_param)),
        NamedKey::PageDown => Some(csi_tilde(6, modifier_param)),

        // Function keys F1-F4: SS3 without modifiers, CSI with modifiers
        NamedKey::F1 => Some(ss3_or_csi(b'P', 11, modifier_param)),
        NamedKey::F2 => Some(ss3_or_csi(b'Q', 12, modifier_param)),
        NamedKey::F3 => Some(ss3_or_csi(b'R', 13, modifier_param)),
        NamedKey::F4 => Some(ss3_or_csi(b'S', 14, modifier_param)),

        // Function keys F5-F12: CSI tilde sequences
        NamedKey::F5 => Some(csi_tilde(15, modifier_param)),
        NamedKey::F6 => Some(csi_tilde(17, modifier_param)),
        NamedKey::F7 => Some(csi_tilde(18, modifier_param)),
        NamedKey::F8 => Some(csi_tilde(19, modifier_param)),
        NamedKey::F9 => Some(csi_tilde(20, modifier_param)),
        NamedKey::F10 => Some(csi_tilde(21, modifier_param)),
        NamedKey::F11 => Some(csi_tilde(23, modifier_param)),
        NamedKey::F12 => Some(csi_tilde(24, modifier_param)),

        _ => None,
    }
}

/// Calculate xterm modifier parameter.
/// Shift=1, Alt=2, Ctrl=4. Result = 1 + bitmask if any modifier, else 0.
fn modifier_code(mods: ModifiersState) -> u8 {
    let mut code: u8 = 0;
    if mods.shift_key() {
        code |= 1;
    }
    if mods.alt_key() {
        code |= 2;
    }
    if mods.control_key() {
        code |= 4;
    }
    if code > 0 {
        code + 1
    } else {
        0
    }
}

/// Arrow key sequence: SS3 in app cursor mode (no mods), CSI otherwise.
fn arrow_seq(letter: u8, modifier: u8, app_cursor: bool) -> Vec<u8> {
    if modifier == 0 && app_cursor {
        vec![0x1b, b'O', letter] // SS3
    } else if modifier == 0 {
        vec![0x1b, b'[', letter] // CSI
    } else {
        format!("\x1b[1;{}{}", modifier, letter as char).into_bytes()
    }
}

/// CSI tilde sequence: ESC[code~ or ESC[code;modifier~
fn csi_tilde(code: u8, modifier: u8) -> Vec<u8> {
    if modifier == 0 {
        format!("\x1b[{}~", code).into_bytes()
    } else {
        format!("\x1b[{};{}~", code, modifier).into_bytes()
    }
}

/// F1-F4: SS3 letter without modifiers, CSI code;modifier~ with modifiers.
fn ss3_or_csi(ss3_letter: u8, csi_code: u8, modifier: u8) -> Vec<u8> {
    if modifier == 0 {
        vec![0x1b, b'O', ss3_letter] // SS3 P/Q/R/S
    } else {
        format!("\x1b[{};{}~", csi_code, modifier).into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_mods() -> ModifiersState {
        ModifiersState::empty()
    }

    fn ctrl() -> ModifiersState {
        ModifiersState::CONTROL
    }

    fn alt() -> ModifiersState {
        ModifiersState::ALT
    }

    fn shift() -> ModifiersState {
        ModifiersState::SHIFT
    }

    fn ctrl_shift() -> ModifiersState {
        ModifiersState::CONTROL | ModifiersState::SHIFT
    }

    fn normal_mode() -> TermMode {
        TermMode::empty()
    }

    fn app_cursor_mode() -> TermMode {
        TermMode::APP_CURSOR
    }

    #[test]
    fn plain_character() {
        let result = encode_key(&Key::Character("a".into()), no_mods(), normal_mode());
        assert_eq!(result, Some(b"a".to_vec()));
    }

    #[test]
    fn ctrl_c_sends_etx() {
        let result = encode_key(&Key::Character("c".into()), ctrl(), normal_mode());
        assert_eq!(result, Some(vec![0x03]));
    }

    #[test]
    fn ctrl_a_sends_soh() {
        let result = encode_key(&Key::Character("a".into()), ctrl(), normal_mode());
        assert_eq!(result, Some(vec![0x01]));
    }

    #[test]
    fn ctrl_z_sends_sub() {
        let result = encode_key(&Key::Character("z".into()), ctrl(), normal_mode());
        assert_eq!(result, Some(vec![0x1a]));
    }

    #[test]
    fn ctrl_bracket_sends_esc() {
        let result = encode_key(&Key::Character("[".into()), ctrl(), normal_mode());
        assert_eq!(result, Some(vec![0x1b]));
    }

    #[test]
    fn alt_x_sends_esc_prefix() {
        let result = encode_key(&Key::Character("x".into()), alt(), normal_mode());
        assert_eq!(result, Some(b"\x1bx".to_vec()));
    }

    #[test]
    fn arrow_up_normal_mode() {
        let result = encode_key(&Key::Named(NamedKey::ArrowUp), no_mods(), normal_mode());
        assert_eq!(result, Some(b"\x1b[A".to_vec()));
    }

    #[test]
    fn arrow_up_app_cursor_mode() {
        let result = encode_key(&Key::Named(NamedKey::ArrowUp), no_mods(), app_cursor_mode());
        assert_eq!(result, Some(b"\x1bOA".to_vec()));
    }

    #[test]
    fn arrow_up_ctrl_normal_mode() {
        // Ctrl modifier param = 1 + 4 = 5
        let result = encode_key(&Key::Named(NamedKey::ArrowUp), ctrl(), normal_mode());
        assert_eq!(result, Some(b"\x1b[1;5A".to_vec()));
    }

    #[test]
    fn arrow_right_shift_normal_mode() {
        // Shift modifier param = 1 + 1 = 2
        let result = encode_key(&Key::Named(NamedKey::ArrowRight), shift(), normal_mode());
        assert_eq!(result, Some(b"\x1b[1;2C".to_vec()));
    }

    #[test]
    fn enter_sends_cr() {
        let result = encode_key(&Key::Named(NamedKey::Enter), no_mods(), normal_mode());
        assert_eq!(result, Some(vec![0x0d]));
    }

    #[test]
    fn ctrl_enter_sends_lf() {
        let result = encode_key(&Key::Named(NamedKey::Enter), ctrl(), normal_mode());
        assert_eq!(result, Some(vec![0x0a]));
    }

    #[test]
    fn tab_sends_ht() {
        let result = encode_key(&Key::Named(NamedKey::Tab), no_mods(), normal_mode());
        assert_eq!(result, Some(vec![0x09]));
    }

    #[test]
    fn backspace_sends_del() {
        let result = encode_key(&Key::Named(NamedKey::Backspace), no_mods(), normal_mode());
        assert_eq!(result, Some(vec![0x7f]));
    }

    #[test]
    fn escape_key() {
        let result = encode_key(&Key::Named(NamedKey::Escape), no_mods(), normal_mode());
        assert_eq!(result, Some(vec![0x1b]));
    }

    #[test]
    fn home_key() {
        let result = encode_key(&Key::Named(NamedKey::Home), no_mods(), normal_mode());
        assert_eq!(result, Some(b"\x1b[1~".to_vec()));
    }

    #[test]
    fn delete_with_ctrl() {
        // Ctrl modifier param = 5
        let result = encode_key(&Key::Named(NamedKey::Delete), ctrl(), normal_mode());
        assert_eq!(result, Some(b"\x1b[3;5~".to_vec()));
    }

    #[test]
    fn f1_no_mods() {
        let result = encode_key(&Key::Named(NamedKey::F1), no_mods(), normal_mode());
        assert_eq!(result, Some(b"\x1bOP".to_vec()));
    }

    #[test]
    fn f5_no_mods() {
        let result = encode_key(&Key::Named(NamedKey::F5), no_mods(), normal_mode());
        assert_eq!(result, Some(b"\x1b[15~".to_vec()));
    }

    #[test]
    fn shift_page_up_returns_none() {
        // Shift+PageUp is scrollback, not forwarded to PTY
        let result = encode_key(&Key::Named(NamedKey::PageUp), shift(), normal_mode());
        assert_eq!(result, None);
    }

    #[test]
    fn ctrl_shift_c_returns_none() {
        // Ctrl+Shift+C is clipboard copy, not forwarded to PTY
        let result = encode_key(&Key::Character("c".into()), ctrl_shift(), normal_mode());
        assert_eq!(result, None);
    }

    #[test]
    fn ctrl_shift_v_returns_none() {
        // Ctrl+Shift+V is clipboard paste, not forwarded to PTY
        let result = encode_key(&Key::Character("v".into()), ctrl_shift(), normal_mode());
        assert_eq!(result, None);
    }
}

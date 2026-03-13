//! ANSI escape sequence stripping utility.

use std::sync::OnceLock;

use regex::Regex;

/// Strip ANSI/VT escape sequences from `s`, returning clean text.
///
/// Handles:
/// - CSI sequences: `ESC [ ... <letter>` (colors, cursor movement, etc.)
/// - OSC sequences: `ESC ] ... BEL` (window title, hyperlinks, etc.)
/// - Character set designation: `ESC ( B`
pub fn strip_ansi(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]|\x1b\][^\x07]*\x07|\x1b\(B")
            .expect("ansi regex is valid")
    });
    re.replace_all(s, "").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_color_sequences() {
        let input = "\x1b[31mhello\x1b[0m world";
        assert_eq!(strip_ansi(input), "hello world");
    }

    #[test]
    fn strips_osc_sequences() {
        // OSC window title: ESC ] 0 ; title BEL
        let input = "\x1b]0;My Terminal\x07plain text";
        assert_eq!(strip_ansi(input), "plain text");
    }

    #[test]
    fn strips_character_set_designation() {
        let input = "\x1b(Bhello";
        assert_eq!(strip_ansi(input), "hello");
    }

    #[test]
    fn clean_text_unchanged() {
        let input = "no escape sequences here";
        assert_eq!(strip_ansi(input), input);
    }

    #[test]
    fn empty_string_unchanged() {
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn strips_bold_and_underline() {
        let input = "\x1b[1mbold\x1b[0m \x1b[4munderline\x1b[0m";
        assert_eq!(strip_ansi(input), "bold underline");
    }

    #[test]
    fn strips_cursor_movement() {
        // ESC [ 2 J (clear screen) and ESC [ H (cursor home)
        let input = "\x1b[2J\x1b[Hcontent";
        assert_eq!(strip_ansi(input), "content");
    }
}

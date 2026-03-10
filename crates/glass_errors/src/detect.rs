//! Auto-detection of parser kind based on command hint and output content.

use crate::ParserKind;

/// Detect which parser to use for the given output.
///
/// Priority:
/// 1. Command hint containing "cargo" or "rustc" -> check for JSON -> RustJson, else RustHuman
/// 2. Content sniffing for Rust JSON markers
/// 3. Content sniffing for Rust human-readable markers
/// 4. Fallback to Generic
pub(crate) fn detect_parser(output: &str, command_hint: Option<&str>) -> ParserKind {
    // 1. Command hint takes priority
    if let Some(cmd) = command_hint {
        let cmd_lower = cmd.to_lowercase();
        if cmd_lower.contains("cargo") || cmd_lower.contains("rustc") {
            // Check if output looks like JSON
            if output.lines().any(|l| l.trim_start().starts_with('{')) {
                return ParserKind::RustJson;
            }
            return ParserKind::RustHuman;
        }
    }

    // 2. Content sniffing for Rust JSON
    if output.contains(r#""$message_type":"diagnostic""#)
        || output.contains(r#""reason":"compiler-message""#)
    {
        return ParserKind::RustJson;
    }

    // 3. Content sniffing for Rust human-readable
    if output.contains("error[E") || output.contains("warning[") {
        return ParserKind::RustHuman;
    }

    // 4. Fallback
    ParserKind::Generic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_cargo_build_with_json() {
        let output = r#"{"reason":"compiler-message","message":{"level":"error"}}"#;
        assert_eq!(
            detect_parser(output, Some("cargo build")),
            ParserKind::RustJson
        );
    }

    #[test]
    fn detect_cargo_test_no_json() {
        let output = "error[E0308]: mismatched types\n --> src/main.rs:10:5";
        assert_eq!(
            detect_parser(output, Some("cargo test")),
            ParserKind::RustHuman
        );
    }

    #[test]
    fn detect_no_hint_error_e() {
        let output = "error[E0308]: mismatched types\n --> src/main.rs:10:5";
        assert_eq!(detect_parser(output, None), ParserKind::RustHuman);
    }

    #[test]
    fn detect_no_hint_generic() {
        let output = "src/main.c:10:5: error: undeclared";
        assert_eq!(detect_parser(output, None), ParserKind::Generic);
    }

    #[test]
    fn detect_content_sniff_message_type() {
        let output = r#"{"$message_type":"diagnostic","message":"unused"}"#;
        assert_eq!(detect_parser(output, None), ParserKind::RustJson);
    }

    #[test]
    fn detect_content_sniff_compiler_message() {
        let output = r#"{"reason":"compiler-message","message":{}}"#;
        assert_eq!(detect_parser(output, None), ParserKind::RustJson);
    }

    #[test]
    fn detect_rustc_hint_with_json() {
        let output = r#"{"$message_type":"diagnostic","level":"error"}"#;
        assert_eq!(
            detect_parser(output, Some("rustc main.rs")),
            ParserKind::RustJson
        );
    }

    #[test]
    fn detect_warning_bracket() {
        let output = "warning[unused_imports]: unused import";
        assert_eq!(detect_parser(output, None), ParserKind::RustHuman);
    }
}

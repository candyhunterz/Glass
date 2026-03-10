//! Rust human-readable error parser.
//!
//! Parses the standard rustc human output format:
//! ```text
//! error[E0308]: mismatched types
//!  --> src/main.rs:10:5
//! ```

use std::sync::OnceLock;

use regex::Regex;

use crate::{Severity, StructuredError};

/// Regex for error/warning header line: `error[E0308]: message` or `warning: message`.
fn header_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(error|warning|note|help)(\[([A-Z]\d+)\])?:\s*(.+)$").unwrap()
    })
}

/// Regex for span line: ` --> file:line:col`.
fn span_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*-->\s*(.+):(\d+):(\d+)$").unwrap())
}

/// Map severity string to Severity enum.
fn map_severity(s: &str) -> Severity {
    match s {
        "error" => Severity::Error,
        "warning" => Severity::Warning,
        "note" => Severity::Note,
        "help" => Severity::Help,
        _ => Severity::Error,
    }
}

/// Parse Rust human-readable error output into structured errors.
///
/// Looks for two-line patterns:
/// 1. Header: `error[E0308]: mismatched types`
/// 2. Span: ` --> src/main.rs:10:5`
pub(crate) fn parse_rust_human(output: &str) -> Vec<StructuredError> {
    let mut errors = Vec::new();
    let header_re = header_regex();
    let span_re = span_regex();

    // State machine: track pending header info
    let mut pending_severity: Option<Severity> = None;
    let mut pending_code: Option<String> = None;
    let mut pending_message: Option<String> = None;

    for line in output.lines() {
        // Check for header line
        if let Some(caps) = header_re.captures(line.trim()) {
            // If we had a pending header without a span, discard it
            pending_severity = Some(map_severity(&caps[1]));
            pending_code = caps.get(3).map(|m| m.as_str().to_string());
            pending_message = Some(caps[4].to_string());
            continue;
        }

        // Check for span line (only if we have a pending header)
        if pending_severity.is_some() {
            if let Some(caps) = span_re.captures(line) {
                errors.push(StructuredError {
                    file: caps[1].to_string(),
                    line: caps[2].parse().unwrap_or(0),
                    column: Some(caps[3].parse().unwrap_or(0)),
                    severity: pending_severity.take().unwrap(),
                    message: pending_message.take().unwrap_or_default(),
                    code: pending_code.take(),
                });
                continue;
            }
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Severity;

    #[test]
    fn parse_error_with_code() {
        let output = "error[E0308]: mismatched types\n --> src/main.rs:10:5\n  |\n10 |     let x: u32 = \"hello\";\n  |                   ^^^^^^^ expected `u32`, found `&str`";
        let errors = parse_rust_human(output);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, "src/main.rs");
        assert_eq!(errors[0].line, 10);
        assert_eq!(errors[0].column, Some(5));
        assert_eq!(errors[0].severity, Severity::Error);
        assert_eq!(errors[0].message, "mismatched types");
        assert_eq!(errors[0].code, Some("E0308".to_string()));
    }

    #[test]
    fn parse_warning_no_code() {
        let output = "warning: unused variable\n --> src/lib.rs:3:9";
        let errors = parse_rust_human(output);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, "src/lib.rs");
        assert_eq!(errors[0].line, 3);
        assert_eq!(errors[0].column, Some(9));
        assert_eq!(errors[0].severity, Severity::Warning);
        assert_eq!(errors[0].message, "unused variable");
        assert_eq!(errors[0].code, None);
    }

    #[test]
    fn parse_multiple_errors() {
        let output = "error[E0308]: mismatched types\n --> src/main.rs:10:5\n  |\n\nwarning: unused variable `x`\n --> src/main.rs:3:9\n  |";
        let errors = parse_rust_human(output);
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].severity, Severity::Error);
        assert_eq!(errors[0].code, Some("E0308".to_string()));
        assert_eq!(errors[1].severity, Severity::Warning);
        assert_eq!(errors[1].file, "src/main.rs");
        assert_eq!(errors[1].line, 3);
    }

    #[test]
    fn skip_header_without_span() {
        let output = "error: aborting due to 2 previous errors\n\nFor more information, try `rustc --explain E0308`.";
        let errors = parse_rust_human(output);
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn parse_note_with_span() {
        let output = "note: required by a bound in `foo`\n --> src/lib.rs:5:10";
        let errors = parse_rust_human(output);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].severity, Severity::Note);
    }

    #[test]
    fn parse_help_with_span() {
        let output = "help: consider changing this to `&str`\n --> src/main.rs:10:5";
        let errors = parse_rust_human(output);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].severity, Severity::Help);
    }
}

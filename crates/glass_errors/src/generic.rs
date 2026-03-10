//! Generic file:line:col error parser.
//!
//! Handles the ubiquitous `file:line:col: severity: message` format used by
//! GCC, Clang, Go, TypeScript, and most compilers. Supports Windows paths.

use std::sync::OnceLock;

use regex::Regex;

use crate::{Severity, StructuredError};

/// Regex for file:line:col: severity: message (with Windows path support).
fn regex_full() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^([A-Za-z]:\\[^:]+|[^:\s][^:]*):(\d+):(\d+):\s*(?i)(error|warning|note|info|hint):\s*(.+)$",
        )
        .unwrap()
    })
}

/// Regex for file:line: severity: message (no column).
fn regex_no_col() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^([A-Za-z]:\\[^:]+|[^:\s][^:]*):(\d+):\s*(?i)(error|warning|note|info|hint):\s*(.+)$",
        )
        .unwrap()
    })
}

/// Regex for file:line:col: message (no explicit severity, assume error).
fn regex_no_severity() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([A-Za-z]:\\[^:]+|[^:\s][^:]*):(\d+):(\d+):\s*(.+)$").unwrap())
}

/// Map severity string to Severity enum (case-insensitive).
fn map_severity(s: &str) -> Severity {
    match s.to_lowercase().as_str() {
        "error" => Severity::Error,
        "warning" => Severity::Warning,
        "note" | "info" => Severity::Note,
        "hint" | "help" => Severity::Help,
        _ => Severity::Error,
    }
}

/// Parse generic compiler output into structured errors.
pub(crate) fn parse_generic(output: &str) -> Vec<StructuredError> {
    let mut errors = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Try full format: file:line:col: severity: message
        if let Some(caps) = regex_full().captures(line) {
            errors.push(StructuredError {
                file: caps[1].to_string(),
                line: caps[2].parse().unwrap_or(0),
                column: Some(caps[3].parse().unwrap_or(0)),
                severity: map_severity(&caps[4]),
                message: caps[5].trim().to_string(),
                code: None,
            });
            continue;
        }

        // Try no-column format: file:line: severity: message
        if let Some(caps) = regex_no_col().captures(line) {
            errors.push(StructuredError {
                file: caps[1].to_string(),
                line: caps[2].parse().unwrap_or(0),
                column: None,
                severity: map_severity(&caps[3]),
                message: caps[4].trim().to_string(),
                code: None,
            });
            continue;
        }

        // Try no-severity format: file:line:col: message (assume error)
        if let Some(caps) = regex_no_severity().captures(line) {
            errors.push(StructuredError {
                file: caps[1].to_string(),
                line: caps[2].parse().unwrap_or(0),
                column: Some(caps[3].parse().unwrap_or(0)),
                severity: Severity::Error,
                message: caps[4].trim().to_string(),
                code: None,
            });
            continue;
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_standard_format() {
        let output = "src/main.c:10:5: error: undeclared identifier 'x'";
        let errors = parse_generic(output);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, "src/main.c");
        assert_eq!(errors[0].line, 10);
        assert_eq!(errors[0].column, Some(5));
        assert_eq!(errors[0].severity, Severity::Error);
        assert_eq!(errors[0].message, "undeclared identifier 'x'");
    }

    #[test]
    fn generic_windows_path() {
        let output = r"C:\Users\foo\main.rs:10:5: warning: unused variable";
        let errors = parse_generic(output);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, r"C:\Users\foo\main.rs");
        assert_eq!(errors[0].line, 10);
        assert_eq!(errors[0].column, Some(5));
        assert_eq!(errors[0].severity, Severity::Warning);
    }

    #[test]
    fn generic_no_column() {
        let output = "src/lib.rs:42: warning: deprecated function";
        let errors = parse_generic(output);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, "src/lib.rs");
        assert_eq!(errors[0].line, 42);
        assert_eq!(errors[0].column, None);
        assert_eq!(errors[0].severity, Severity::Warning);
    }

    #[test]
    fn generic_no_severity() {
        let output = "main.c:10:5: some error message here";
        let errors = parse_generic(output);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].severity, Severity::Error);
        assert_eq!(errors[0].message, "some error message here");
    }

    #[test]
    fn generic_mixed_output() {
        let output = "Compiling project...\nsrc/main.c:10:5: error: undeclared\nBuilding...\nsrc/lib.c:20:1: warning: implicit declaration\nDone.";
        let errors = parse_generic(output);
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].severity, Severity::Error);
        assert_eq!(errors[1].severity, Severity::Warning);
    }

    #[test]
    fn generic_info_maps_to_note() {
        let output = "src/main.c:10:5: info: see previous definition";
        let errors = parse_generic(output);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].severity, Severity::Note);
    }

    #[test]
    fn generic_hint_maps_to_help() {
        let output = "src/main.c:10:5: hint: did you mean 'y'?";
        let errors = parse_generic(output);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].severity, Severity::Help);
    }

    #[test]
    fn generic_case_insensitive_severity() {
        let output = "src/main.c:10:5: ERROR: bad thing\nsrc/main.c:11:1: Warning: mild thing";
        let errors = parse_generic(output);
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].severity, Severity::Error);
        assert_eq!(errors[1].severity, Severity::Warning);
    }
}

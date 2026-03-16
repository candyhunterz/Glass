//! Generic compiler output parser.
//!
//! Catch-all parser for the common `file:line:col: severity: message` pattern.
//! Handles GCC-style, Clang-style, MSVC-style, and rustc-style diagnostics
//! as a fallback for compilers not covered by specific parsers.

use std::sync::OnceLock;

use regex::Regex;

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity};

/// Standard `file:line:col: severity: message` pattern (col optional).
fn re_diagnostic() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(.+?):(\d+):(?:(\d+):)?\s*(fatal error|error|warning|note|info):\s*(.+)$")
            .expect("generic diagnostic regex")
    })
}

/// MSVC-style `file(line,col): severity Cxxxx: message` pattern.
fn re_msvc() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(.+?)\((\d+)(?:,(\d+))?\)\s*:\s*(error|warning)\s*(\w+)?\s*:\s*(.+)$")
            .expect("msvc diagnostic regex")
    })
}

/// Parse generic compiler output into structured `CompilerError` records.
pub fn parse(output: &str) -> ParsedOutput {
    let clean = crate::ansi::strip_ansi(output);
    let raw_line_count = output.lines().count();
    let raw_byte_count = output.len();

    let mut records: Vec<OutputRecord> = Vec::new();
    let mut error_count: u32 = 0;
    let mut warning_count: u32 = 0;

    for line in clean.lines() {
        if line.len() > 4096 {
            continue;
        }

        if let Some(caps) = re_diagnostic().captures(line) {
            let file = caps[1].to_string();
            let line_num: u32 = caps[2].parse().unwrap_or(0);
            let col: Option<u32> = caps.get(3).and_then(|m| m.as_str().parse().ok());
            let sev_str = &caps[4];
            let message = caps[5].to_string();

            let severity = match sev_str {
                "error" | "fatal error" => {
                    error_count += 1;
                    Severity::Error
                }
                "warning" => {
                    warning_count += 1;
                    Severity::Warning
                }
                _ => Severity::Info,
            };

            records.push(OutputRecord::CompilerError {
                file,
                line: line_num,
                column: col,
                severity,
                code: None,
                message,
                context_lines: None,
            });
            continue;
        }

        if let Some(caps) = re_msvc().captures(line) {
            let file = caps[1].to_string();
            let line_num: u32 = caps[2].parse().unwrap_or(0);
            let col: Option<u32> = caps.get(3).and_then(|m| m.as_str().parse().ok());
            let sev_str = &caps[4];
            let code = caps.get(5).map(|m| m.as_str().to_string());
            let message = caps[6].to_string();

            let severity = match sev_str {
                "error" => {
                    error_count += 1;
                    Severity::Error
                }
                "warning" => {
                    warning_count += 1;
                    Severity::Warning
                }
                _ => Severity::Info,
            };

            records.push(OutputRecord::CompilerError {
                file,
                line: line_num,
                column: col,
                severity,
                code,
                message,
                context_lines: None,
            });
            continue;
        }
    }

    if records.is_empty() {
        return crate::freeform_parse(output, Some(OutputType::GenericCompiler), None);
    }

    let severity = if error_count > 0 {
        Severity::Error
    } else if warning_count > 0 {
        Severity::Warning
    } else {
        Severity::Info
    };

    let one_line = format!("{error_count} errors, {warning_count} warnings");
    let token_estimate = one_line.split_whitespace().count() + records.len() * 8 + raw_line_count;

    ParsedOutput {
        output_type: OutputType::GenericCompiler,
        summary: OutputSummary {
            one_line,
            token_estimate,
            severity,
        },
        records,
        raw_line_count,
        raw_byte_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_file_line_col() {
        let output = "main.rs:10:5: error: mismatched types\n";
        let parsed = parse(output);
        assert_eq!(parsed.output_type, OutputType::GenericCompiler);
        assert_eq!(parsed.summary.severity, Severity::Error);
        if let OutputRecord::CompilerError {
            file,
            line,
            column,
            message,
            ..
        } = &parsed.records[0]
        {
            assert_eq!(file, "main.rs");
            assert_eq!(*line, 10);
            assert_eq!(*column, Some(5));
            assert!(message.contains("mismatched"));
        } else {
            panic!("expected CompilerError");
        }
    }

    #[test]
    fn generic_no_column() {
        let output = "main.rs:10: warning: unused import\n";
        let parsed = parse(output);
        assert_eq!(parsed.summary.severity, Severity::Warning);
        if let OutputRecord::CompilerError { column, .. } = &parsed.records[0] {
            assert_eq!(*column, None);
        } else {
            panic!("expected CompilerError");
        }
    }

    #[test]
    fn msvc_style() {
        let output = "main.cpp(10): error C2065: undeclared identifier\n";
        let parsed = parse(output);
        assert_eq!(parsed.summary.severity, Severity::Error);
        if let OutputRecord::CompilerError { code, file, .. } = &parsed.records[0] {
            assert_eq!(file, "main.cpp");
            assert_eq!(code.as_deref(), Some("C2065"));
        } else {
            panic!("expected CompilerError");
        }
    }

    #[test]
    fn mixed_errors_warnings() {
        let output = "a.c:1:1: error: something wrong\nb.c:2:3: warning: be careful\nc.c:3:5: error: another error\n";
        let parsed = parse(output);
        assert_eq!(parsed.summary.severity, Severity::Error);
        assert!(parsed.summary.one_line.contains("2 errors"));
        assert!(parsed.summary.one_line.contains("1 warnings"));
    }

    #[test]
    fn empty_fallback() {
        let parsed = parse("");
        assert_eq!(parsed.output_type, OutputType::GenericCompiler);
        assert!(matches!(
            parsed.records[0],
            OutputRecord::FreeformChunk { .. }
        ));
    }
}

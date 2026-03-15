//! Parser for C/C++ compiler output (`gcc`, `g++`, `clang`, `clang++`).
//!
//! Extracts `CompilerError` records from gcc/clang diagnostic output,
//! including errors, warnings, and notes with file:line:col locations.

use std::sync::OnceLock;

use regex::Regex;

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity};

fn re_diagnostic() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(.+?):(\d+):(\d+):\s*(fatal error|error|warning|note):\s*(.+)$")
            .expect("cpp diagnostic regex")
    })
}

/// Parse gcc/clang compiler output into structured `CompilerError` records.
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

        if line.starts_with("In file included from") {
            continue;
        }

        if let Some(caps) = re_diagnostic().captures(line) {
            let file = caps[1].to_string();
            let line_num: u32 = caps[2].parse().unwrap_or(0);
            let col: u32 = caps[3].parse().unwrap_or(0);
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
                column: Some(col),
                severity,
                code: None,
                message,
                context_lines: None,
            });
        }
    }

    if records.is_empty() {
        return crate::freeform_parse(output, Some(OutputType::CppCompiler), None);
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
        output_type: OutputType::CppCompiler,
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
    fn gcc_errors_and_warnings() {
        let output = "main.c:10:5: warning: unused variable [-Wunused-variable]\nmain.c:15:12: error: use of undeclared identifier\nmain.c:20:1: warning: control reaches end [-Wreturn-type]\nmain.c:25:8: error: expected semicolon\n";
        let parsed = parse(output);
        assert_eq!(parsed.output_type, OutputType::CppCompiler);
        assert_eq!(parsed.summary.severity, Severity::Error);
        assert!(parsed.summary.one_line.contains("2 errors"));
        assert!(parsed.summary.one_line.contains("2 warnings"));
    }

    #[test]
    fn clang_fatal_error() {
        let parsed = parse("main.c:1:10: fatal error: file not found\n");
        assert_eq!(parsed.summary.severity, Severity::Error);
        if let OutputRecord::CompilerError { file, line, .. } = &parsed.records[0] {
            assert_eq!(file, "main.c");
            assert_eq!(*line, 1);
        } else {
            panic!("expected CompilerError");
        }
    }

    #[test]
    fn warnings_only_severity() {
        let output = "util.c:5:10: warning: implicit declaration\nutil.c:12:3: warning: unused variable\n";
        let parsed = parse(output);
        assert_eq!(parsed.summary.severity, Severity::Warning);
        assert_eq!(parsed.records.len(), 2);
    }

    #[test]
    fn note_lines_severity() {
        let output = "main.c:10:5: note: declared here\nmain.c:15:5: note: candidate function not viable\n";
        let parsed = parse(output);
        assert_eq!(parsed.summary.severity, Severity::Info);
    }

    #[test]
    fn empty_fallback() {
        let parsed = parse("");
        assert_eq!(parsed.output_type, OutputType::CppCompiler);
        assert!(matches!(parsed.records[0], OutputRecord::FreeformChunk { .. }));
    }
}

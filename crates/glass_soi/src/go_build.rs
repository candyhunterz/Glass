//! Parser for `go build` and `go vet` output.
//!
//! Extracts `CompilerError` records from Go build error lines of the form
//! `file:line:col: message`. Falls through to freeform if no patterns match.

use std::sync::OnceLock;

use regex::Regex;

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity};

fn re_go_error() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(.+?):(\d+):(\d+): (.+)$").expect("go build error regex"))
}

/// Parse Go build output into structured `CompilerError` records.
///
/// Extracts lines matching `file:line:col: message`. Skips lines starting with `#`
/// (Go module path comments). All records have `severity=Error` and `code=None`.
///
/// Returns a freeform fallback if no patterns match.
pub fn parse(output: &str) -> ParsedOutput {
    let re = re_go_error();
    let mut records: Vec<OutputRecord> = Vec::new();

    for line in output.lines() {
        if line.len() > 4096 {
            continue;
        }
        // Skip module path comment lines (e.g. "# example.com/myapp")
        if line.starts_with('#') {
            continue;
        }
        if let Some(caps) = re.captures(line) {
            let file = caps[1].to_string();
            let line_num: u32 = caps[2].parse().unwrap_or(0);
            let col: u32 = caps[3].parse().unwrap_or(0);
            let message = caps[4].to_string();

            records.push(OutputRecord::CompilerError {
                file,
                line: line_num,
                column: Some(col),
                severity: Severity::Error,
                code: None,
                message,
                context_lines: None,
            });
        }
    }

    if records.is_empty() {
        return crate::freeform_parse(output, Some(OutputType::GoBuild), None);
    }

    let error_count = records.len();
    let one_line = format!(
        "{} error{}",
        error_count,
        if error_count == 1 { "" } else { "s" }
    );
    let token_estimate = 5 + records.len() * 10;

    ParsedOutput {
        output_type: OutputType::GoBuild,
        summary: OutputSummary {
            one_line,
            token_estimate,
            severity: Severity::Error,
        },
        records,
        raw_line_count: output.lines().count(),
        raw_byte_count: output.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GO_BUILD_ERROR: &str = "./main.go:10:5: undefined: fmt.Println2";

    const GO_BUILD_MULTIPLE: &str =
        "# example.com/myapp\n./main.go:10:5: undefined: fmt.Println2\n./main.go:15:9: cannot use x (type int) as type string";

    const GO_BUILD_NO_ERRORS: &str = "# example.com/myapp\nbuild successful";

    #[test]
    fn go_build_error_produces_compiler_error_record() {
        let parsed = parse(GO_BUILD_ERROR);
        assert_eq!(parsed.output_type, OutputType::GoBuild);
        assert_eq!(parsed.records.len(), 1);
        if let OutputRecord::CompilerError {
            file,
            line,
            column,
            severity,
            code,
            message,
            ..
        } = &parsed.records[0]
        {
            assert_eq!(file, "./main.go");
            assert_eq!(*line, 10);
            assert_eq!(*column, Some(5));
            assert_eq!(*severity, Severity::Error);
            assert!(code.is_none());
            assert!(message.contains("undefined"));
        } else {
            panic!("Expected CompilerError record");
        }
    }

    #[test]
    fn go_build_multiple_errors_all_records_present() {
        let parsed = parse(GO_BUILD_MULTIPLE);
        assert_eq!(parsed.output_type, OutputType::GoBuild);
        // # line is skipped; 2 error lines
        assert_eq!(parsed.records.len(), 2);
    }

    #[test]
    fn go_build_module_comment_lines_skipped() {
        // The "# example.com/myapp" line must not produce a record
        let parsed = parse(GO_BUILD_MULTIPLE);
        for r in &parsed.records {
            if let OutputRecord::CompilerError { file, .. } = r {
                assert!(
                    !file.starts_with('#'),
                    "# comment line should be skipped, got file: {}",
                    file
                );
            }
        }
    }

    #[test]
    fn go_build_no_matches_falls_through_to_freeform() {
        let parsed = parse(GO_BUILD_NO_ERRORS);
        assert!(
            matches!(parsed.records[0], OutputRecord::FreeformChunk { .. }),
            "Expected FreeformChunk"
        );
    }

    #[test]
    fn go_build_error_severity_is_error() {
        let parsed = parse(GO_BUILD_ERROR);
        assert_eq!(parsed.summary.severity, Severity::Error);
    }

    #[test]
    fn go_build_code_is_none() {
        let parsed = parse(GO_BUILD_ERROR);
        if let OutputRecord::CompilerError { code, .. } = &parsed.records[0] {
            assert!(code.is_none(), "Go build errors have no error code");
        }
    }

    #[test]
    fn go_build_summary_one_line_has_error_count() {
        let parsed = parse(GO_BUILD_MULTIPLE);
        assert!(
            parsed.summary.one_line.contains("2 errors"),
            "one_line was: {}",
            parsed.summary.one_line
        );
    }

    #[test]
    fn go_build_raw_metrics_populated() {
        let parsed = parse(GO_BUILD_ERROR);
        assert_eq!(parsed.raw_line_count, 1);
        assert_eq!(parsed.raw_byte_count, GO_BUILD_ERROR.len());
    }
}

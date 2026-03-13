//! Parser for TypeScript compiler (`tsc`) output.
//!
//! Parses tsc error lines of the form `file(line,col): error|warning TSxxxx: message`
//! into `CompilerError` records. Falls through to freeform if no matches are found.

use std::sync::OnceLock;

use regex::Regex;

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity};

fn re_tsc_error() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(.+?)\((\d+),(\d+)\): (error|warning) (TS\d+): (.+)$")
            .expect("tsc error regex")
    })
}

/// Parse TypeScript compiler output into structured `CompilerError` records.
///
/// Strips ANSI codes first. Extracts error and warning lines matching the tsc format:
/// `file(line,col): error|warning TSxxxx: message`
///
/// Returns a freeform fallback if no tsc patterns match.
pub fn parse(output: &str) -> ParsedOutput {
    let stripped = crate::strip_ansi(output);
    let re = re_tsc_error();
    let mut records: Vec<OutputRecord> = Vec::new();

    for line in stripped.lines() {
        if line.len() > 4096 {
            continue;
        }
        if let Some(caps) = re.captures(line) {
            let file = caps[1].to_string();
            let line_num: u32 = caps[2].parse().unwrap_or(0);
            let col: u32 = caps[3].parse().unwrap_or(0);
            let severity = match &caps[4] {
                "error" => Severity::Error,
                "warning" => Severity::Warning,
                _ => Severity::Info,
            };
            let code = Some(caps[5].to_string());
            let message = caps[6].to_string();

            records.push(OutputRecord::CompilerError {
                file,
                line: line_num,
                column: Some(col),
                severity,
                code,
                message,
                context_lines: None,
            });
        }
    }

    if records.is_empty() {
        return crate::freeform_parse(output, Some(OutputType::TypeScript), None);
    }

    let error_count = records
        .iter()
        .filter(|r| {
            matches!(
                r,
                OutputRecord::CompilerError {
                    severity: Severity::Error,
                    ..
                }
            )
        })
        .count();

    let warning_count = records
        .iter()
        .filter(|r| {
            matches!(
                r,
                OutputRecord::CompilerError {
                    severity: Severity::Warning,
                    ..
                }
            )
        })
        .count();

    let one_line = if error_count > 0 && warning_count > 0 {
        format!("{} errors, {} warnings", error_count, warning_count)
    } else if error_count > 0 {
        format!(
            "{} error{}",
            error_count,
            if error_count == 1 { "" } else { "s" }
        )
    } else if warning_count > 0 {
        format!(
            "{} warning{}",
            warning_count,
            if warning_count == 1 { "" } else { "s" }
        )
    } else {
        "tsc: no issues found".to_string()
    };

    let severity = if error_count > 0 {
        Severity::Error
    } else if warning_count > 0 {
        Severity::Warning
    } else {
        Severity::Success
    };

    let token_estimate = 5 + records.len() * 10;

    ParsedOutput {
        output_type: OutputType::TypeScript,
        summary: OutputSummary {
            one_line,
            token_estimate,
            severity,
        },
        records,
        raw_line_count: output.lines().count(),
        raw_byte_count: output.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TSC_ERROR: &str =
        "src/main.ts(10,5): error TS2345: Argument of type 'string' is not assignable to 'number'";

    const TSC_WARNING: &str =
        "src/utils.ts(3,1): warning TS6133: 'x' is declared but its value is never read.";

    const TSC_MULTIPLE: &str =
        "src/main.ts(10,5): error TS2345: Argument of type 'string' is not assignable to 'number'
src/api.ts(22,3): error TS2551: Property 'bar' does not exist on type 'Foo'. Did you mean 'baz'?
src/utils.ts(3,1): warning TS6133: 'x' is declared but its value is never read.";

    const TSC_NO_ERRORS: &str =
        "Watching for file changes.\nFound 0 errors. Watching for file changes.";

    #[test]
    fn tsc_error_produces_compiler_error_record() {
        let parsed = parse(TSC_ERROR);
        assert_eq!(parsed.output_type, OutputType::TypeScript);
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
            assert_eq!(file, "src/main.ts");
            assert_eq!(*line, 10);
            assert_eq!(*column, Some(5));
            assert_eq!(*severity, Severity::Error);
            assert_eq!(code.as_deref(), Some("TS2345"));
            assert!(message.contains("Argument of type"));
        } else {
            panic!("Expected CompilerError record");
        }
    }

    #[test]
    fn tsc_warning_produces_warning_severity() {
        let parsed = parse(TSC_WARNING);
        assert_eq!(parsed.output_type, OutputType::TypeScript);
        assert_eq!(parsed.records.len(), 1);
        if let OutputRecord::CompilerError { severity, code, .. } = &parsed.records[0] {
            assert_eq!(*severity, Severity::Warning);
            assert_eq!(code.as_deref(), Some("TS6133"));
        } else {
            panic!("Expected CompilerError record");
        }
    }

    #[test]
    fn tsc_multiple_errors_all_records_present() {
        let parsed = parse(TSC_MULTIPLE);
        assert_eq!(parsed.output_type, OutputType::TypeScript);
        assert_eq!(parsed.records.len(), 3);
        let errors: Vec<_> = parsed
            .records
            .iter()
            .filter(|r| {
                matches!(
                    r,
                    OutputRecord::CompilerError {
                        severity: Severity::Error,
                        ..
                    }
                )
            })
            .collect();
        let warnings: Vec<_> = parsed
            .records
            .iter()
            .filter(|r| {
                matches!(
                    r,
                    OutputRecord::CompilerError {
                        severity: Severity::Warning,
                        ..
                    }
                )
            })
            .collect();
        assert_eq!(errors.len(), 2);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn tsc_multiple_summary_one_line_has_counts() {
        let parsed = parse(TSC_MULTIPLE);
        assert!(
            parsed.summary.one_line.contains("2 errors"),
            "one_line was: {}",
            parsed.summary.one_line
        );
        assert!(
            parsed.summary.one_line.contains("1 warning"),
            "one_line was: {}",
            parsed.summary.one_line
        );
    }

    #[test]
    fn tsc_no_matches_falls_through_to_freeform() {
        let parsed = parse(TSC_NO_ERRORS);
        // No tsc error patterns -> freeform
        assert!(
            matches!(parsed.records[0], OutputRecord::FreeformChunk { .. }),
            "Expected FreeformChunk"
        );
    }

    #[test]
    fn tsc_error_severity_is_error() {
        let parsed = parse(TSC_ERROR);
        assert_eq!(parsed.summary.severity, Severity::Error);
    }

    #[test]
    fn tsc_warning_only_severity_is_warning() {
        let parsed = parse(TSC_WARNING);
        assert_eq!(parsed.summary.severity, Severity::Warning);
    }

    #[test]
    fn tsc_raw_metrics_populated() {
        let parsed = parse(TSC_ERROR);
        assert_eq!(parsed.raw_line_count, 1);
        assert_eq!(parsed.raw_byte_count, TSC_ERROR.len());
    }
}

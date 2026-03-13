//! Parser for NDJSON (newline-delimited JSON) / structured log output.
//!
//! Extracts `GenericDiagnostic` records from JSON lines, mapping the `level`/`severity`
//! field to SOI `Severity` and `msg`/`message` field to the diagnostic message.
//! Falls through to freeform if fewer than 2 valid JSON lines are found.

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity};

/// Map a JSON level/severity string to SOI `Severity`.
fn map_level(level: &str) -> Severity {
    match level.to_lowercase().as_str() {
        "error" | "fatal" | "critical" | "err" => Severity::Error,
        "warn" | "warning" => Severity::Warning,
        _ => Severity::Info,
    }
}

/// Parse NDJSON/structured log output into `GenericDiagnostic` records.
///
/// Each line is attempted as a JSON object. Valid objects with a `msg`/`message` field
/// produce a `GenericDiagnostic`. Lines not starting with `{` are skipped.
/// If fewer than 2 valid JSON lines are parsed, falls through to freeform.
pub fn parse(output: &str) -> ParsedOutput {
    let mut records: Vec<OutputRecord> = Vec::new();

    for line in output.lines() {
        if line.len() > 4096 {
            continue;
        }
        let trimmed = line.trim();
        if !trimmed.starts_with('{') {
            continue;
        }
        let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };
        let obj = match val.as_object() {
            Some(o) => o,
            None => continue,
        };

        // Extract severity from "level" or "severity" field
        let severity = obj
            .get("level")
            .or_else(|| obj.get("severity"))
            .and_then(|v| v.as_str())
            .map(map_level)
            .unwrap_or(Severity::Info);

        // Extract message from "msg" or "message" field
        let message = obj
            .get("msg")
            .or_else(|| obj.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if message.is_empty() {
            continue;
        }

        // Extract optional file field
        let file = obj.get("file").and_then(|v| v.as_str()).map(String::from);

        // Extract optional line number field
        let line_num = obj.get("line").and_then(|v| v.as_u64()).map(|n| n as u32);

        records.push(OutputRecord::GenericDiagnostic {
            file,
            line: line_num,
            severity,
            message,
        });
    }

    if records.len() < 2 {
        return crate::freeform_parse(output, Some(OutputType::JsonLines), None);
    }

    let error_count = records
        .iter()
        .filter(|r| {
            matches!(
                r,
                OutputRecord::GenericDiagnostic {
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
                OutputRecord::GenericDiagnostic {
                    severity: Severity::Warning,
                    ..
                }
            )
        })
        .count();

    let total = records.len();
    let one_line = format!(
        "{} log entr{} ({} errors, {} warnings)",
        total,
        if total == 1 { "y" } else { "ies" },
        error_count,
        warning_count
    );

    let severity = if error_count > 0 {
        Severity::Error
    } else if warning_count > 0 {
        Severity::Warning
    } else {
        Severity::Info
    };

    let token_estimate = 5 + records.len() * 8;

    ParsedOutput {
        output_type: OutputType::JsonLines,
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

    const JSON_ERROR_LINE: &str =
        r#"{"level":"error","msg":"connection failed","error":"timeout"}"#;
    const JSON_INFO_LINE: &str = r#"{"level":"info","msg":"server started","port":8080}"#;
    const JSON_WARN_LINE: &str =
        r#"{"level":"warn","msg":"rate limit approaching","threshold":0.9}"#;

    const JSON_TWO_LINES: &str =
        "{\"level\":\"error\",\"msg\":\"connection failed\",\"error\":\"timeout\"}\n{\"level\":\"info\",\"msg\":\"server started\",\"port\":8080}";

    const JSON_WITH_FILE: &str =
        "{\"level\":\"error\",\"msg\":\"parse error\",\"file\":\"main.go\",\"line\":42}\n{\"level\":\"info\",\"msg\":\"done\"}";

    const NDJSON_FATAL: &str =
        "{\"level\":\"fatal\",\"msg\":\"out of memory\"}\n{\"level\":\"critical\",\"msg\":\"disk full\"}";

    const JSON_SINGLE_LINE: &str = r#"{"level":"error","msg":"only one line"}"#;

    const NON_JSON: &str = "plain text output\nnot json at all";

    #[test]
    fn json_two_lines_produces_two_diagnostic_records() {
        let parsed = parse(JSON_TWO_LINES);
        assert_eq!(parsed.output_type, OutputType::JsonLines);
        assert_eq!(parsed.records.len(), 2);
    }

    #[test]
    fn json_error_level_maps_to_error_severity() {
        let parsed = parse(JSON_TWO_LINES);
        if let OutputRecord::GenericDiagnostic {
            severity, message, ..
        } = &parsed.records[0]
        {
            assert_eq!(*severity, Severity::Error);
            assert_eq!(message, "connection failed");
        } else {
            panic!("Expected GenericDiagnostic");
        }
    }

    #[test]
    fn json_info_level_maps_to_info_severity() {
        let parsed = parse(JSON_TWO_LINES);
        if let OutputRecord::GenericDiagnostic {
            severity, message, ..
        } = &parsed.records[1]
        {
            assert_eq!(*severity, Severity::Info);
            assert_eq!(message, "server started");
        } else {
            panic!("Expected GenericDiagnostic");
        }
    }

    #[test]
    fn json_warn_level_maps_to_warning_severity() {
        let input = format!("{}\n{}", JSON_WARN_LINE, JSON_INFO_LINE);
        let parsed = parse(&input);
        if let OutputRecord::GenericDiagnostic { severity, .. } = &parsed.records[0] {
            assert_eq!(*severity, Severity::Warning);
        } else {
            panic!("Expected GenericDiagnostic");
        }
    }

    #[test]
    fn json_fatal_and_critical_map_to_error() {
        let parsed = parse(NDJSON_FATAL);
        let all_error = parsed.records.iter().all(|r| {
            matches!(
                r,
                OutputRecord::GenericDiagnostic {
                    severity: Severity::Error,
                    ..
                }
            )
        });
        assert!(all_error, "fatal and critical should map to Error severity");
    }

    #[test]
    fn json_file_field_extracted() {
        let parsed = parse(JSON_WITH_FILE);
        if let OutputRecord::GenericDiagnostic { file, line, .. } = &parsed.records[0] {
            assert_eq!(file.as_deref(), Some("main.go"));
            assert_eq!(*line, Some(42));
        } else {
            panic!("Expected GenericDiagnostic");
        }
    }

    #[test]
    fn json_fewer_than_2_valid_lines_falls_through_to_freeform() {
        let parsed = parse(JSON_SINGLE_LINE);
        assert!(
            matches!(parsed.records[0], OutputRecord::FreeformChunk { .. }),
            "Expected FreeformChunk for single JSON line"
        );
    }

    #[test]
    fn non_json_lines_skipped_falls_through_to_freeform() {
        let parsed = parse(NON_JSON);
        assert!(
            matches!(parsed.records[0], OutputRecord::FreeformChunk { .. }),
            "Expected FreeformChunk for non-JSON output"
        );
    }

    #[test]
    fn json_non_brace_lines_skipped() {
        // Mix of JSON and plain text — only JSON lines counted
        let input = format!(
            "plain text\n{}\nnot json\n{}",
            JSON_ERROR_LINE, JSON_INFO_LINE
        );
        let parsed = parse(&input);
        assert_eq!(parsed.output_type, OutputType::JsonLines);
        assert_eq!(parsed.records.len(), 2);
    }

    #[test]
    fn json_summary_one_line_has_counts() {
        let parsed = parse(JSON_TWO_LINES);
        assert!(
            parsed.summary.one_line.contains("2 log entries"),
            "one_line was: {}",
            parsed.summary.one_line
        );
    }

    #[test]
    fn json_error_present_severity_is_error() {
        let parsed = parse(JSON_TWO_LINES);
        assert_eq!(parsed.summary.severity, Severity::Error);
    }

    #[test]
    fn json_all_info_severity_is_info() {
        let input = format!(
            "{}\n{}",
            r#"{"level":"info","msg":"one"}"#, r#"{"level":"info","msg":"two"}"#
        );
        let parsed = parse(&input);
        assert_eq!(parsed.summary.severity, Severity::Info);
    }

    #[test]
    fn json_raw_metrics_populated() {
        let parsed = parse(JSON_TWO_LINES);
        assert_eq!(parsed.raw_line_count, 2);
        assert_eq!(parsed.raw_byte_count, JSON_TWO_LINES.len());
    }
}

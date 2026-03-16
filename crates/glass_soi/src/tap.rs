//! Parser for TAP (Test Anything Protocol) output.
//!
//! Extracts `TestResult` and `TestSummary` records from TAP-formatted output
//! produced by any TAP-compatible test runner.

use std::sync::OnceLock;

use regex::Regex;

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity, TestStatus};

fn re_plan() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^1\.\.(\d+)$").expect("tap plan regex"))
}

fn re_test_line() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^(ok|not ok)\s+(\d+)\s*(?:-\s*(.+?))?(?:\s*#\s*(skip|todo)\s*(.*))?$")
            .expect("tap test line regex")
    })
}

fn re_bail_out() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^Bail out!\s*(.*)$").expect("tap bail out regex"))
}

/// Parse TAP output into structured `TestResult` and `TestSummary` records.
pub fn parse(output: &str) -> ParsedOutput {
    let clean = crate::ansi::strip_ansi(output);
    let raw_line_count = output.lines().count();
    let raw_byte_count = output.len();

    let mut records: Vec<OutputRecord> = Vec::new();
    let mut passed: u32 = 0;
    let mut failed: u32 = 0;
    let mut skipped: u32 = 0;
    let mut bail = false;

    for line in clean.lines() {
        if line.len() > 4096 {
            continue;
        }

        if line.starts_with('#') {
            continue;
        }

        if let Some(caps) = re_bail_out().captures(line) {
            bail = true;
            let reason = caps.get(1).map_or("", |m| m.as_str()).trim().to_string();
            records.push(OutputRecord::TestResult {
                name: "Bail out!".to_string(),
                status: TestStatus::Failed,
                duration_ms: None,
                failure_message: if reason.is_empty() {
                    None
                } else {
                    Some(reason)
                },
                failure_location: None,
            });
            failed += 1;
            break;
        }

        if re_plan().is_match(line) {
            continue;
        }

        if let Some(caps) = re_test_line().captures(line) {
            let ok = caps[1].eq_ignore_ascii_case("ok");
            let name = caps
                .get(3)
                .map_or_else(|| format!("test {}", &caps[2]), |m| m.as_str().to_string());
            let directive = caps.get(4).map(|m| m.as_str().to_lowercase());

            let status =
                if directive.as_deref() == Some("skip") || directive.as_deref() == Some("todo") {
                    skipped += 1;
                    TestStatus::Skipped
                } else if ok {
                    passed += 1;
                    TestStatus::Passed
                } else {
                    failed += 1;
                    TestStatus::Failed
                };

            records.push(OutputRecord::TestResult {
                name,
                status,
                duration_ms: None,
                failure_message: None,
                failure_location: None,
            });
        }
    }

    if records.is_empty() {
        return crate::freeform_parse(output, Some(OutputType::GenericTAP), None);
    }

    let total = passed + failed + skipped;
    records.push(OutputRecord::TestSummary {
        passed,
        failed,
        skipped,
        ignored: 0,
        total_duration_ms: None,
    });

    let severity = if failed > 0 || bail {
        Severity::Error
    } else if skipped > 0 {
        Severity::Warning
    } else {
        Severity::Success
    };

    let one_line =
        format!("{passed} passed, {failed} failed, {skipped} skipped out of {total} tests");
    let token_estimate = one_line.split_whitespace().count() + records.len() * 5 + raw_line_count;

    ParsedOutput {
        output_type: OutputType::GenericTAP,
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
    fn tap_all_pass() {
        let output = "1..3\nok 1 - test a\nok 2 - test b\nok 3 - test c\n";
        let parsed = parse(output);
        assert_eq!(parsed.output_type, OutputType::GenericTAP);
        assert_eq!(parsed.summary.severity, Severity::Success);
        assert!(parsed.summary.one_line.contains("3 passed"));
        assert!(parsed.summary.one_line.contains("0 failed"));
    }

    #[test]
    fn tap_with_failures() {
        let output = "1..4\nok 1 - pass a\nnot ok 2 - fail b\nok 3 - pass c\nnot ok 4 - fail d\n";
        let parsed = parse(output);
        assert_eq!(parsed.summary.severity, Severity::Error);
        assert!(parsed.summary.one_line.contains("2 passed"));
        assert!(parsed.summary.one_line.contains("2 failed"));
    }

    #[test]
    fn tap_with_skip() {
        let output = "1..3\nok 1 - test a\nok 2 - test b # skip not implemented\nok 3 - test c\n";
        let parsed = parse(output);
        assert_eq!(parsed.summary.severity, Severity::Warning);
        assert!(parsed.summary.one_line.contains("1 skipped"));
    }

    #[test]
    fn tap_bail_out() {
        let output = "1..5\nok 1 - test a\nBail out! Database connection failed\n";
        let parsed = parse(output);
        assert_eq!(parsed.summary.severity, Severity::Error);
        assert!(parsed.summary.one_line.contains("1 failed"));
    }

    #[test]
    fn empty_fallback() {
        let parsed = parse("");
        assert_eq!(parsed.output_type, OutputType::GenericTAP);
        assert!(matches!(
            parsed.records[0],
            OutputRecord::FreeformChunk { .. }
        ));
    }
}

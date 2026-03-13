//! Parser for `pytest` / `python -m pytest` output.
//!
//! Extracts per-test `TestResult` records and a `TestSummary` from pytest output.

use std::sync::OnceLock;

use regex::Regex;

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity, TestStatus};

// --- Compiled regex patterns ---

fn re_result() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Matches: "path::test_name STATUS" with optional trailing content (percentage, etc.)
    RE.get_or_init(|| {
        Regex::new(r"^(.+::[\w\[\] ,.-]+)\s+(PASSED|FAILED|ERROR|SKIPPED|XFAIL|XPASS)")
            .expect("pytest result regex")
    })
}

fn re_summary() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Matches lines like: "5 passed, 1 failed, 2 skipped in 0.42s"
    // Also: "5 passed in 0.42s" or "1 failed in 0.1s"
    RE.get_or_init(|| {
        Regex::new(
            r"(\d+) passed(?:,\s*(\d+) failed)?(?:,\s*(\d+) error(?:s)?)?(?:,\s*(\d+) skipped)?.*in ([\d.]+)s",
        )
        .expect("pytest summary regex")
    })
}

fn re_summary_alt() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Matches: "1 failed in 0.1s" (when there are 0 passed)
    RE.get_or_init(|| {
        Regex::new(r"(\d+) failed(?:,\s*(\d+) error(?:s)?)?(?:,\s*(\d+) skipped)?.*in ([\d.]+)s")
            .expect("pytest summary alt regex")
    })
}

fn re_short_summary() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Matches: "FAILED tests/test_auth.py::test_logout - assert False"
    RE.get_or_init(|| {
        Regex::new(r"^FAILED (.+) - (.+)$").expect("pytest short summary regex")
    })
}

/// Parse pytest output into structured `TestResult` and `TestSummary` records.
pub fn parse(output: &str) -> ParsedOutput {
    let raw_line_count = output.lines().count();
    let raw_byte_count = output.len();

    let mut records: Vec<OutputRecord> = Vec::new();
    let mut test_results: Vec<(String, TestStatus)> = Vec::new(); // name, status for later failure matching

    // Map from test name -> failure message (populated from short summary lines)
    let mut failure_messages: std::collections::HashMap<String, String> = Default::default();

    let mut summary_record: Option<OutputRecord> = None;
    let mut any_failed = false;

    for line in output.lines() {
        if line.len() > 4096 {
            continue;
        }

        // Per-test result line
        if let Some(caps) = re_result().captures(line) {
            let name = caps[1].trim().to_string();
            let status_str = &caps[2];
            let status = match status_str {
                "PASSED" | "XPASS" => TestStatus::Passed,
                "FAILED" | "ERROR" => {
                    any_failed = true;
                    TestStatus::Failed
                }
                "SKIPPED" | "XFAIL" => TestStatus::Skipped,
                _ => TestStatus::Failed,
            };
            test_results.push((name, status));
            continue;
        }

        // Short summary info: "FAILED path::test - reason"
        if let Some(caps) = re_short_summary().captures(line) {
            let name = caps[1].trim().to_string();
            let msg = caps[2].trim().to_string();
            failure_messages.insert(name, msg);
            continue;
        }

        // Summary line with counts and duration
        if let Some(caps) = re_summary().captures(line) {
            let passed: u32 = caps.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let failed: u32 = caps.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let errors: u32 = caps.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let skipped: u32 = caps.get(4).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let duration_s: f64 = caps.get(5).and_then(|m| m.as_str().parse().ok()).unwrap_or(0.0);
            let total_duration_ms = Some((duration_s * 1000.0) as u64);
            if failed + errors > 0 {
                any_failed = true;
            }
            summary_record = Some(OutputRecord::TestSummary {
                passed,
                failed: failed + errors,
                skipped,
                ignored: 0,
                total_duration_ms,
            });
            continue;
        }

        // Alt summary line: only failures, no passed count
        if summary_record.is_none() {
            if let Some(caps) = re_summary_alt().captures(line) {
                let failed: u32 = caps.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
                let errors: u32 = caps.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
                let skipped: u32 =
                    caps.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
                let duration_s: f64 =
                    caps.get(4).and_then(|m| m.as_str().parse().ok()).unwrap_or(0.0);
                let total_duration_ms = Some((duration_s * 1000.0) as u64);
                if failed + errors > 0 {
                    any_failed = true;
                }
                summary_record = Some(OutputRecord::TestSummary {
                    passed: 0,
                    failed: failed + errors,
                    skipped,
                    ignored: 0,
                    total_duration_ms,
                });
            }
        }
    }

    // If nothing was extracted, fall back to freeform
    if test_results.is_empty() && summary_record.is_none() {
        return crate::freeform_parse(output, Some(OutputType::Pytest), None);
    }

    // Build TestResult records with failure messages
    for (name, status) in test_results {
        let failure_message = failure_messages.get(&name).cloned();
        records.push(OutputRecord::TestResult {
            name,
            status,
            duration_ms: None,
            failure_message,
            failure_location: None,
        });
    }

    // Append summary record if present
    if let Some(sr) = summary_record {
        records.push(sr);
    }

    let severity = if any_failed {
        Severity::Error
    } else {
        Severity::Success
    };

    // Build summary one-liner from counts
    let one_line = build_one_line(&records);

    let token_estimate = one_line.split_whitespace().count() + records.len() * 5 + raw_line_count;

    ParsedOutput {
        output_type: OutputType::Pytest,
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

fn build_one_line(records: &[OutputRecord]) -> String {
    for r in records.iter().rev() {
        if let OutputRecord::TestSummary {
            passed,
            failed,
            skipped,
            ..
        } = r
        {
            let mut parts = Vec::new();
            if *failed > 0 {
                parts.push(format!("{failed} failed"));
            }
            if *passed > 0 {
                parts.push(format!("{passed} passed"));
            }
            if *skipped > 0 {
                parts.push(format!("{skipped} skipped"));
            }
            if parts.is_empty() {
                return "0 tests run".to_string();
            }
            return parts.join(", ");
        }
    }

    // No TestSummary — count from TestResult records
    let passed = records
        .iter()
        .filter(|r| matches!(r, OutputRecord::TestResult { status: TestStatus::Passed, .. }))
        .count();
    let failed = records
        .iter()
        .filter(|r| matches!(r, OutputRecord::TestResult { status: TestStatus::Failed, .. }))
        .count();
    let skipped = records
        .iter()
        .filter(|r| matches!(r, OutputRecord::TestResult { status: TestStatus::Skipped, .. }))
        .count();

    let mut parts = Vec::new();
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    if passed > 0 {
        parts.push(format!("{passed} passed"));
    }
    if skipped > 0 {
        parts.push(format!("{skipped} skipped"));
    }
    if parts.is_empty() {
        return "pytest output parsed".to_string();
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PYTEST_MIXED: &str = r#"collected 5 items

tests/test_auth.py::test_login PASSED                     [ 20%]
tests/test_auth.py::test_logout FAILED                    [ 40%]
tests/test_auth.py::test_register PASSED                  [ 60%]
tests/test_auth.py::test_delete SKIPPED (reason: not impl) [ 80%]
tests/test_auth.py::test_xfail XFAIL                      [100%]

FAILURES
============================================================
tests/test_auth.py::test_logout
auth.py:42: AssertionError: assert False
============================================================
short test summary info
FAILED tests/test_auth.py::test_logout - assert False
============================================================
3 passed, 1 failed, 1 skipped in 0.42s
"#;

    const PYTEST_ALL_PASSING: &str = r#"collected 3 items

tests/test_api.py::test_get PASSED                        [ 33%]
tests/test_api.py::test_post PASSED                       [ 66%]
tests/test_api.py::test_put PASSED                        [100%]

3 passed in 0.15s
"#;

    const PYTEST_WITH_XFAIL: &str = r#"collected 4 items

tests/test_core.py::test_basic PASSED                     [ 25%]
tests/test_core.py::test_skip SKIPPED                     [ 50%]
tests/test_core.py::test_xfail XFAIL                      [ 75%]
tests/test_core.py::test_xpass XPASS                      [100%]

4 passed in 0.20s
"#;

    const PYTEST_FAILURE_MESSAGE: &str = r#"collected 2 items

tests/test_db.py::test_connect PASSED                     [ 50%]
tests/test_db.py::test_query FAILED                       [100%]

FAILURES
============================================================
tests/test_db.py::test_query
db.py:88: AssertionError: Expected row count 5, got 0
============================================================
short test summary info
FAILED tests/test_db.py::test_query - AssertionError: Expected row count 5, got 0
============================================================
1 failed, 1 passed in 0.33s
"#;

    #[test]
    fn pytest_mixed_results_extracted() {
        let parsed = parse(PYTEST_MIXED);
        assert_eq!(parsed.output_type, OutputType::Pytest);

        let test_results: Vec<_> = parsed
            .records
            .iter()
            .filter(|r| matches!(r, OutputRecord::TestResult { .. }))
            .collect();
        assert_eq!(test_results.len(), 5, "should have 5 test results");
    }

    #[test]
    fn pytest_mixed_statuses_correct() {
        let parsed = parse(PYTEST_MIXED);

        let passed_count = parsed
            .records
            .iter()
            .filter(|r| matches!(r, OutputRecord::TestResult { status: TestStatus::Passed, .. }))
            .count();
        let failed_count = parsed
            .records
            .iter()
            .filter(|r| matches!(r, OutputRecord::TestResult { status: TestStatus::Failed, .. }))
            .count();
        let skipped_count = parsed
            .records
            .iter()
            .filter(|r| matches!(r, OutputRecord::TestResult { status: TestStatus::Skipped, .. }))
            .count();

        assert_eq!(passed_count, 2, "2 PASSED (login + register)");
        assert_eq!(failed_count, 1, "1 FAILED (logout)");
        assert_eq!(skipped_count, 2, "2 skipped (SKIPPED + XFAIL)");
    }

    #[test]
    fn pytest_summary_record_extracted() {
        let parsed = parse(PYTEST_MIXED);
        let summary = parsed.records.iter().find_map(|r| {
            if let OutputRecord::TestSummary {
                passed,
                failed,
                skipped,
                total_duration_ms,
                ..
            } = r
            {
                Some((*passed, *failed, *skipped, *total_duration_ms))
            } else {
                None
            }
        });
        let (passed, failed, skipped, duration) =
            summary.expect("should have TestSummary record");
        assert_eq!(passed, 3);
        assert_eq!(failed, 1);
        assert_eq!(skipped, 1);
        assert!(duration.is_some());
        assert_eq!(duration, Some(420)); // 0.42s -> 420ms
    }

    #[test]
    fn pytest_mixed_severity_is_error() {
        let parsed = parse(PYTEST_MIXED);
        assert_eq!(parsed.summary.severity, Severity::Error);
    }

    #[test]
    fn pytest_all_passing_severity_is_success() {
        let parsed = parse(PYTEST_ALL_PASSING);
        assert_eq!(parsed.summary.severity, Severity::Success);
    }

    #[test]
    fn pytest_skipped_and_xfail_are_skipped_status() {
        let parsed = parse(PYTEST_WITH_XFAIL);
        let skipped_count = parsed
            .records
            .iter()
            .filter(|r| matches!(r, OutputRecord::TestResult { status: TestStatus::Skipped, .. }))
            .count();
        // SKIPPED + XFAIL = 2 Skipped; XPASS maps to Passed
        assert_eq!(skipped_count, 2);
    }

    #[test]
    fn pytest_xpass_is_passed_status() {
        let parsed = parse(PYTEST_WITH_XFAIL);
        let xpass = parsed.records.iter().find(|r| {
            if let OutputRecord::TestResult { name, .. } = r {
                name.contains("test_xpass")
            } else {
                false
            }
        });
        assert!(xpass.is_some());
        if let Some(OutputRecord::TestResult { status, .. }) = xpass {
            assert_eq!(*status, TestStatus::Passed);
        }
    }

    #[test]
    fn pytest_failure_message_extracted() {
        let parsed = parse(PYTEST_FAILURE_MESSAGE);
        let test_query = parsed.records.iter().find_map(|r| {
            if let OutputRecord::TestResult {
                name,
                failure_message,
                ..
            } = r
            {
                if name.contains("test_query") {
                    return failure_message.clone();
                }
            }
            None
        });
        let msg = test_query.expect("test_query should have failure message");
        assert!(
            msg.contains("Expected row count"),
            "failure message should contain assertion text: {msg}"
        );
    }

    #[test]
    fn pytest_empty_output_freeform_fallback() {
        let parsed = parse("");
        assert_eq!(parsed.output_type, OutputType::Pytest);
        assert!(matches!(
            parsed.records[0],
            OutputRecord::FreeformChunk { .. }
        ));
    }

    #[test]
    fn pytest_unrecognized_output_freeform_fallback() {
        let parsed = parse("some random text\nno test lines here\n");
        assert_eq!(parsed.output_type, OutputType::Pytest);
        assert!(matches!(
            parsed.records[0],
            OutputRecord::FreeformChunk { .. }
        ));
    }

    #[test]
    fn pytest_summary_one_line_has_counts() {
        let parsed = parse(PYTEST_MIXED);
        assert!(
            parsed.summary.one_line.contains("failed") || parsed.summary.one_line.contains("passed"),
            "one_line should mention test outcomes: {}",
            parsed.summary.one_line
        );
    }

    #[test]
    fn pytest_duration_parsed_correctly() {
        let parsed = parse(PYTEST_ALL_PASSING);
        let duration = parsed.records.iter().find_map(|r| {
            if let OutputRecord::TestSummary {
                total_duration_ms, ..
            } = r
            {
                *total_duration_ms
            } else {
                None
            }
        });
        assert_eq!(duration, Some(150)); // 0.15s -> 150ms
    }
}

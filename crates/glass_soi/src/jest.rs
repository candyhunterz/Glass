//! Parser for `jest` / `npx jest` output.
//!
//! Strips ANSI escape sequences then extracts per-test `TestResult` and
//! `TestSummary` records from jest output.

use std::sync::OnceLock;

use regex::Regex;

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity, TestStatus};

// --- Compiled regex patterns ---

fn re_suite() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Matches: "PASS src/auth.test.js" or "FAIL src/auth.test.ts"
    RE.get_or_init(|| Regex::new(r"^(PASS|FAIL)\s+(.+)$").expect("jest suite regex"))
}

fn re_test_pass() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Matches: "    ✓ test name (42 ms)" or "    ✔ test name"
    // Unicode check marks: U+2713 (✓), U+2714 (✔)
    RE.get_or_init(|| {
        Regex::new(r"^\s+[✓✔]\s+(.+?)(?:\s+\((\d+)\s*m?s\))?$").expect("jest test pass regex")
    })
}

fn re_test_fail() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Matches: "    ✕ test name (12 ms)" or "    ✗ test name" or "    × test name"
    // Failure marks: U+2715 (✕), U+2717 (✗), U+00D7 (×)
    RE.get_or_init(|| {
        Regex::new(r"^\s+[✕✗×]\s+(.+?)(?:\s+\((\d+)\s*m?s\))?$").expect("jest test fail regex")
    })
}

fn re_summary() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Matches: "Tests:  1 failed, 5 passed, 6 total"
    // or: "Tests:  5 passed, 5 total"
    RE.get_or_init(|| {
        Regex::new(r"Tests:\s+(?:(\d+) failed,\s+)?(?:(\d+) skipped,\s+)?(\d+) passed,\s+\d+ total")
            .expect("jest summary regex")
    })
}

fn re_time() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Matches: "Time:  2.345 s"
    RE.get_or_init(|| Regex::new(r"Time:\s+([\d.]+)\s*s").expect("jest time regex"))
}

/// Parse jest output into structured `TestResult` and `TestSummary` records.
///
/// ANSI escape sequences are stripped before any regex matching.
pub fn parse(output: &str) -> ParsedOutput {
    let clean = crate::ansi::strip_ansi(output);
    parse_clean(&clean, output.len())
}

fn parse_clean(clean: &str, raw_byte_count: usize) -> ParsedOutput {
    let raw_line_count = clean.lines().count();

    let mut records: Vec<OutputRecord> = Vec::new();
    let mut current_suite: Option<String> = None;
    let mut any_failed = false;

    // Track failure message collection for the most recently seen failing test
    let mut last_fail_idx: Option<usize> = None; // index into records
    let mut collecting_failure = false;
    let mut failure_lines: Vec<String> = Vec::new();

    let mut summary_failed: u32 = 0;
    let mut summary_passed: u32 = 0;
    let mut summary_skipped: u32 = 0;
    let mut total_duration_ms: Option<u64> = None;
    let mut has_summary = false;

    for line in clean.lines() {
        if line.len() > 4096 {
            continue;
        }

        // Suite-level result: "PASS src/auth.test.js"
        if let Some(caps) = re_suite().captures(line) {
            // Flush any pending failure message
            if let Some(idx) = last_fail_idx.take() {
                if !failure_lines.is_empty() {
                    let msg = failure_lines.join("\n");
                    if let Some(OutputRecord::TestResult {
                        failure_message, ..
                    }) = records.get_mut(idx)
                    {
                        *failure_message = Some(msg);
                    }
                    failure_lines.clear();
                }
            }
            collecting_failure = false;

            current_suite = Some(caps[2].trim().to_string());
            continue;
        }

        // Individual passing test
        if let Some(caps) = re_test_pass().captures(line) {
            // Flush any pending failure message first
            if let Some(idx) = last_fail_idx.take() {
                if !failure_lines.is_empty() {
                    let msg = failure_lines.join("\n");
                    if let Some(OutputRecord::TestResult {
                        failure_message, ..
                    }) = records.get_mut(idx)
                    {
                        *failure_message = Some(msg);
                    }
                    failure_lines.clear();
                }
            }
            collecting_failure = false;

            let test_name = caps[1].trim().to_string();
            let duration_ms = caps.get(2).and_then(|m| m.as_str().parse().ok());
            let full_name = match &current_suite {
                Some(suite) => format!("{suite} > {test_name}"),
                None => test_name,
            };
            records.push(OutputRecord::TestResult {
                name: full_name,
                status: TestStatus::Passed,
                duration_ms,
                failure_message: None,
                failure_location: None,
            });
            continue;
        }

        // Individual failing test
        if let Some(caps) = re_test_fail().captures(line) {
            // Flush any previous failure message
            if let Some(idx) = last_fail_idx.take() {
                if !failure_lines.is_empty() {
                    let msg = failure_lines.join("\n");
                    if let Some(OutputRecord::TestResult {
                        failure_message, ..
                    }) = records.get_mut(idx)
                    {
                        *failure_message = Some(msg);
                    }
                    failure_lines.clear();
                }
            }

            let test_name = caps[1].trim().to_string();
            let duration_ms = caps.get(2).and_then(|m| m.as_str().parse().ok());
            let full_name = match &current_suite {
                Some(suite) => format!("{suite} > {test_name}"),
                None => test_name,
            };
            records.push(OutputRecord::TestResult {
                name: full_name,
                status: TestStatus::Failed,
                duration_ms,
                failure_message: None,
                failure_location: None,
            });
            any_failed = true;
            last_fail_idx = Some(records.len() - 1);
            collecting_failure = true;
            failure_lines.clear();
            continue;
        }

        // Summary line: "Tests: 1 failed, 5 passed, 6 total"
        if let Some(caps) = re_summary().captures(line) {
            // Flush failure message
            if let Some(idx) = last_fail_idx.take() {
                if !failure_lines.is_empty() {
                    let msg = failure_lines.join("\n");
                    if let Some(OutputRecord::TestResult {
                        failure_message, ..
                    }) = records.get_mut(idx)
                    {
                        *failure_message = Some(msg);
                    }
                    failure_lines.clear();
                }
            }
            collecting_failure = false;

            summary_failed = caps
                .get(1)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            summary_skipped = caps
                .get(2)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            summary_passed = caps
                .get(3)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            if summary_failed > 0 {
                any_failed = true;
            }
            has_summary = true;
            continue;
        }

        // Time line: "Time: 2.345 s"
        if let Some(caps) = re_time().captures(line) {
            let secs: f64 = caps[1].parse().unwrap_or(0.0);
            total_duration_ms = Some((secs * 1000.0) as u64);
            continue;
        }

        // Collect failure diff lines (indented lines after a failing test, before next test/suite)
        if collecting_failure {
            // Stop collecting on blank lines if we already have content, to avoid collecting too much
            if line.trim().is_empty() {
                if !failure_lines.is_empty() {
                    // Keep up to 20 lines of diff
                    if failure_lines.len() < 20 {
                        failure_lines.push(String::new());
                    }
                }
            } else {
                failure_lines.push(line.trim_end().to_string());
            }
        }
    }

    // Flush any remaining failure message
    if let Some(idx) = last_fail_idx {
        if !failure_lines.is_empty() {
            let msg = failure_lines.join("\n");
            if let Some(OutputRecord::TestResult {
                failure_message, ..
            }) = records.get_mut(idx)
            {
                *failure_message = Some(msg);
            }
        }
    }

    // If nothing was extracted, fall back to freeform
    if records.is_empty() && !has_summary {
        return crate::freeform_parse(
            // reconstruct approximate original from clean text
            clean,
            Some(OutputType::Jest),
            None,
        );
    }

    // Append TestSummary if we saw a summary line
    if has_summary {
        records.push(OutputRecord::TestSummary {
            passed: summary_passed,
            failed: summary_failed,
            skipped: summary_skipped,
            ignored: 0,
            total_duration_ms,
        });
    }

    let severity = if any_failed {
        Severity::Error
    } else {
        Severity::Success
    };

    let one_line = build_one_line(&records);
    let token_estimate = one_line.split_whitespace().count() + records.len() * 5 + raw_line_count;

    ParsedOutput {
        output_type: OutputType::Jest,
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

    let passed = records
        .iter()
        .filter(|r| {
            matches!(
                r,
                OutputRecord::TestResult {
                    status: TestStatus::Passed,
                    ..
                }
            )
        })
        .count();
    let failed = records
        .iter()
        .filter(|r| {
            matches!(
                r,
                OutputRecord::TestResult {
                    status: TestStatus::Failed,
                    ..
                }
            )
        })
        .count();

    let mut parts = Vec::new();
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    if passed > 0 {
        parts.push(format!("{passed} passed"));
    }
    if parts.is_empty() {
        "jest output parsed".to_string()
    } else {
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ANSI-colored jest output (realistic example).
    // Colors: green for PASS, red for FAIL, bold for test names.
    // Note: escape sequences are on the same source line to preserve indentation.
    const JEST_WITH_ANSI: &str = concat!(
        "\x1b[1m\x1b[32mPASS\x1b[0m src/auth.test.js\n",
        "  \x1b[32m✓\x1b[0m \x1b[2mshould login with valid credentials\x1b[0m \x1b[2m(15 ms)\x1b[0m\n",
        "  \x1b[32m✓\x1b[0m \x1b[2mshould reject invalid password\x1b[0m \x1b[2m(5 ms)\x1b[0m\n",
        "\n",
        "\x1b[1m\x1b[31mFAIL\x1b[0m src/db.test.js\n",
        "  \x1b[32m✓\x1b[0m \x1b[2mshould connect\x1b[0m \x1b[2m(20 ms)\x1b[0m\n",
        "  \x1b[31m✕\x1b[0m \x1b[1mshould run query\x1b[0m \x1b[2m(8 ms)\x1b[0m\n",
        "\n",
        "Tests: 1 failed, 3 passed, 4 total\n",
        "Time:  1.234 s\n",
    );

    const JEST_ALL_PASSING: &str = concat!(
        "PASS src/utils.test.js\n",
        "  ✓ adds numbers correctly (3 ms)\n",
        "  ✓ subtracts numbers correctly (2 ms)\n",
        "  ✓ handles edge cases (1 ms)\n",
        "\n",
        "Tests:  3 passed, 3 total\n",
        "Time:  0.5 s\n",
    );

    const JEST_WITH_DIFF: &str = concat!(
        "FAIL src/calc.test.js\n",
        "  ✕ should return 42 (10 ms)\n",
        "    expect(received).toBe(expected)\n",
        "\n",
        "    Expected: 42\n",
        "    Received: 0\n",
        "\n",
        "Tests: 1 failed, 0 passed, 1 total\n",
        "Time:  0.1 s\n",
    );

    #[test]
    fn jest_ansi_stripped_before_parsing() {
        let parsed = parse(JEST_WITH_ANSI);
        assert_eq!(parsed.output_type, OutputType::Jest);
        // Should have found test results despite ANSI codes
        let test_count = parsed
            .records
            .iter()
            .filter(|r| matches!(r, OutputRecord::TestResult { .. }))
            .count();
        assert_eq!(test_count, 4, "should parse all 4 tests from ANSI output");
    }

    #[test]
    fn jest_ansi_pass_fail_statuses_correct() {
        let parsed = parse(JEST_WITH_ANSI);

        let passed_count = parsed
            .records
            .iter()
            .filter(|r| {
                matches!(
                    r,
                    OutputRecord::TestResult {
                        status: TestStatus::Passed,
                        ..
                    }
                )
            })
            .count();
        let failed_count = parsed
            .records
            .iter()
            .filter(|r| {
                matches!(
                    r,
                    OutputRecord::TestResult {
                        status: TestStatus::Failed,
                        ..
                    }
                )
            })
            .count();

        assert_eq!(passed_count, 3, "3 tests should pass");
        assert_eq!(failed_count, 1, "1 test should fail");
    }

    #[test]
    fn jest_test_name_includes_suite() {
        let parsed = parse(JEST_ALL_PASSING);
        let first_test = parsed.records.iter().find_map(|r| {
            if let OutputRecord::TestResult { name, .. } = r {
                Some(name.clone())
            } else {
                None
            }
        });
        let name = first_test.expect("should have test result");
        assert!(
            name.contains("src/utils.test.js"),
            "test name should be prefixed with suite: {name}"
        );
        assert!(
            name.contains("adds numbers correctly"),
            "test name should include test description: {name}"
        );
    }

    #[test]
    fn jest_duration_extracted() {
        let parsed = parse(JEST_WITH_ANSI);
        let test_pass = parsed.records.iter().find_map(|r| {
            if let OutputRecord::TestResult {
                name, duration_ms, ..
            } = r
            {
                if name.contains("should login") {
                    return Some(*duration_ms);
                }
            }
            None
        });
        assert_eq!(
            test_pass,
            Some(Some(15)),
            "should login should have 15ms duration"
        );
    }

    #[test]
    fn jest_summary_record_extracted() {
        let parsed = parse(JEST_WITH_ANSI);
        let summary = parsed.records.iter().find_map(|r| {
            if let OutputRecord::TestSummary {
                passed,
                failed,
                total_duration_ms,
                ..
            } = r
            {
                Some((*passed, *failed, *total_duration_ms))
            } else {
                None
            }
        });
        let (passed, failed, duration) = summary.expect("should have TestSummary");
        assert_eq!(passed, 3);
        assert_eq!(failed, 1);
        assert_eq!(duration, Some(1234)); // 1.234s -> 1234ms
    }

    #[test]
    fn jest_all_passing_severity_success() {
        let parsed = parse(JEST_ALL_PASSING);
        assert_eq!(parsed.summary.severity, Severity::Success);
    }

    #[test]
    fn jest_with_failures_severity_error() {
        let parsed = parse(JEST_WITH_ANSI);
        assert_eq!(parsed.summary.severity, Severity::Error);
    }

    #[test]
    fn jest_failure_diff_extracted() {
        let parsed = parse(JEST_WITH_DIFF);
        let failure_msg = parsed.records.iter().find_map(|r| {
            if let OutputRecord::TestResult {
                status: TestStatus::Failed,
                failure_message,
                ..
            } = r
            {
                failure_message.clone()
            } else {
                None
            }
        });
        let msg = failure_msg.expect("failing test should have failure_message");
        assert!(
            msg.contains("Expected") || msg.contains("toBe"),
            "failure message should contain diff: {msg}"
        );
    }

    #[test]
    fn jest_empty_output_freeform_fallback() {
        let parsed = parse("");
        assert_eq!(parsed.output_type, OutputType::Jest);
        assert!(matches!(
            parsed.records[0],
            OutputRecord::FreeformChunk { .. }
        ));
    }

    #[test]
    fn jest_unrecognized_output_freeform_fallback() {
        let parsed = parse("some random jest-unrelated text\n");
        assert_eq!(parsed.output_type, OutputType::Jest);
        assert!(matches!(
            parsed.records[0],
            OutputRecord::FreeformChunk { .. }
        ));
    }

    #[test]
    fn jest_summary_one_line_has_counts() {
        let parsed = parse(JEST_WITH_ANSI);
        assert!(
            parsed.summary.one_line.contains("failed")
                || parsed.summary.one_line.contains("passed"),
            "one_line should mention test outcomes: {}",
            parsed.summary.one_line
        );
    }

    #[test]
    fn jest_time_parsed_correctly() {
        let parsed = parse(JEST_ALL_PASSING);
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
        assert_eq!(duration, Some(500)); // 0.5s -> 500ms
    }
}

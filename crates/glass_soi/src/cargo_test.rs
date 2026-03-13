//! Parser for `cargo test` output.
//!
//! Parses per-test results, failure message blocks, and the aggregate summary line.
//! Falls back to `cargo_build::parse` when compilation fails (no "running N tests" line found).

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity, TestStatus};

fn test_line_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^test (.+) \.\.\. (ok|FAILED|ignored)$").unwrap())
}

fn failure_header_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^---- (.+) stdout ----$").unwrap())
}

fn test_summary_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored")
            .unwrap()
    })
}

fn test_duration_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"finished in (\d+\.?\d*)s").unwrap())
}

fn running_tests_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"running \d+ tests?").unwrap())
}

/// Parse `cargo test` output into structured records.
///
/// If no "running N tests" line is found, the output is likely a compilation failure.
/// In that case, delegates to `cargo_build::parse` to extract compiler errors instead.
pub fn parse(output: &str) -> ParsedOutput {
    // Check for "running N tests" line — absence means compilation failure
    if !running_tests_regex().is_match(output) {
        return super::cargo_build::parse(output, Some("cargo test"));
    }

    let test_line_re = test_line_regex();
    let failure_header_re = failure_header_regex();
    let summary_re = test_summary_regex();
    let duration_re = test_duration_regex();

    let mut test_results: Vec<OutputRecord> = Vec::new();
    let mut summary_record: Option<OutputRecord> = None;
    let mut total_duration_ms: Option<u64> = None;

    // Failure message collection:
    // key = test name, value = accumulated failure message lines
    let mut failure_messages: HashMap<String, Vec<String>> = HashMap::new();
    let mut current_failure_name: Option<String> = None;
    let mut in_failure_block = false;

    for line in output.lines() {
        // Check for failure header: "---- test_name stdout ----"
        if let Some(caps) = failure_header_re.captures(line) {
            let name = caps[1].to_string();
            in_failure_block = true;
            current_failure_name = Some(name);
            continue;
        }

        // If in a failure block, collect lines until separator
        if in_failure_block {
            // Separator lines: empty line before "test result:", or a new "----" header, or "failures:" section
            let is_separator = line.starts_with("failures:")
                || line.starts_with("test result:")
                || failure_header_re.is_match(line)
                || line.trim() == "failures:";

            if is_separator {
                in_failure_block = false;
                // Don't consume this line — fall through to normal processing
                // (but we need to re-handle new failure headers — the regex check above already does)
            } else {
                if let Some(ref name) = current_failure_name {
                    failure_messages
                        .entry(name.clone())
                        .or_default()
                        .push(line.to_string());
                }
                continue;
            }
        }

        // Check for test result line: "test foo::bar ... ok"
        if let Some(caps) = test_line_re.captures(line) {
            let name = caps[1].to_string();
            let status = match &caps[2] {
                "ok" => TestStatus::Passed,
                "FAILED" => TestStatus::Failed,
                _ => TestStatus::Ignored,
            };
            test_results.push(OutputRecord::TestResult {
                name,
                status,
                duration_ms: None,
                failure_message: None,
                failure_location: None,
            });
            continue;
        }

        // Check for summary line: "test result: ok. 3 passed; 0 failed; 1 ignored; finished in 0.02s"
        // Duration may appear on the same line as the summary.
        if let Some(caps) = summary_re.captures(line) {
            let passed: u32 = caps[1].parse().unwrap_or(0);
            let failed: u32 = caps[2].parse().unwrap_or(0);
            let ignored: u32 = caps[3].parse().unwrap_or(0);
            // Also extract duration from same line if present
            if let Some(dur_caps) = duration_re.captures(line) {
                let secs: f64 = dur_caps[1].parse().unwrap_or(0.0);
                total_duration_ms = Some((secs * 1000.0) as u64);
            }
            summary_record = Some(OutputRecord::TestSummary {
                passed,
                failed,
                skipped: 0,
                ignored,
                total_duration_ms: None, // filled in after loop using total_duration_ms
            });
            continue;
        }

        // Check for standalone duration: "finished in 0.02s" (on its own line)
        if let Some(caps) = duration_re.captures(line) {
            let secs: f64 = caps[1].parse().unwrap_or(0.0);
            total_duration_ms = Some((secs * 1000.0) as u64);
            continue;
        }
    }

    // Attach failure messages to Failed TestResult records
    for record in test_results.iter_mut() {
        if let OutputRecord::TestResult {
            name,
            status: TestStatus::Failed,
            failure_message,
            ..
        } = record
        {
            if let Some(lines) = failure_messages.get(name) {
                // Filter out empty trailing lines
                let trimmed: Vec<&str> = lines
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .skip_while(|s| s.trim().is_empty())
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                if !trimmed.is_empty() {
                    *failure_message = Some(trimmed.join("\n"));
                }
            }
        }
    }

    // Update TestSummary with duration
    if let Some(OutputRecord::TestSummary {
        total_duration_ms: ref mut dur,
        ..
    }) = summary_record
    {
        *dur = total_duration_ms;
    }

    // Build the records list: test results first, then summary
    let mut records: Vec<OutputRecord> = test_results;
    if let Some(s) = summary_record {
        records.push(s);
    }

    // Build OutputSummary
    let (passed, failed, ignored) = records.iter().fold((0u32, 0u32, 0u32), |acc, r| match r {
        OutputRecord::TestSummary {
            passed,
            failed,
            ignored,
            ..
        } => (*passed, *failed, *ignored),
        _ => acc,
    });

    let one_line = if failed == 0 && passed > 0 {
        if ignored > 0 {
            format!("all {} passed, {} ignored", passed, ignored)
        } else {
            format!("all {} passed", passed)
        }
    } else if failed > 0 {
        format!("{} passed, {} failed, {} ignored", passed, failed, ignored)
    } else {
        // No summary line found — count from individual records
        let result_count = records
            .iter()
            .filter(|r| matches!(r, OutputRecord::TestResult { .. }))
            .count();
        format!("{} test results", result_count)
    };

    let severity = if failed > 0 {
        Severity::Error
    } else if passed > 0 || ignored > 0 {
        Severity::Success
    } else {
        Severity::Info
    };

    let token_estimate = 5 + records.len() * 8;

    ParsedOutput {
        output_type: OutputType::RustTest,
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

    const MIXED_RESULTS: &str = "running 5 tests
test foo::test_a ... ok
test foo::test_b ... FAILED
test foo::test_c ... ignored
test foo::test_d ... ok
test foo::test_e ... ok

failures:

---- foo::test_b stdout ----
thread 'foo::test_b' panicked at 'assertion failed: `(left == right)`
  left: `1`,
 right: `2`', src/foo.rs:42:9

failures:
    foo::test_b

test result: FAILED. 3 passed; 1 failed; 1 ignored; finished in 0.05s";

    const ALL_PASSING: &str = "running 3 tests
test bar::test_x ... ok
test bar::test_y ... ok
test bar::test_z ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; finished in 0.02s";

    const COMPILE_FAILURE: &str =
        "error[E0308]: mismatched types\n --> src/main.rs:10:5\n  |\n10 |     let x: u32 = \"hello\";";

    #[test]
    fn mixed_results_produces_test_result_records() {
        let parsed = parse(MIXED_RESULTS);
        assert_eq!(parsed.output_type, OutputType::RustTest);

        let test_results: Vec<_> = parsed
            .records
            .iter()
            .filter(|r| matches!(r, OutputRecord::TestResult { .. }))
            .collect();
        assert_eq!(test_results.len(), 5);
    }

    #[test]
    fn mixed_results_has_correct_statuses() {
        let parsed = parse(MIXED_RESULTS);
        let results: Vec<_> = parsed
            .records
            .iter()
            .filter_map(|r| {
                if let OutputRecord::TestResult { name, status, .. } = r {
                    Some((name.as_str(), status))
                } else {
                    None
                }
            })
            .collect();

        let test_a = results.iter().find(|(n, _)| *n == "foo::test_a");
        assert!(test_a.is_some());
        assert_eq!(test_a.unwrap().1, &TestStatus::Passed);

        let test_b = results.iter().find(|(n, _)| *n == "foo::test_b");
        assert!(test_b.is_some());
        assert_eq!(test_b.unwrap().1, &TestStatus::Failed);

        let test_c = results.iter().find(|(n, _)| *n == "foo::test_c");
        assert!(test_c.is_some());
        assert_eq!(test_c.unwrap().1, &TestStatus::Ignored);
    }

    #[test]
    fn mixed_results_has_test_summary() {
        let parsed = parse(MIXED_RESULTS);
        let summary = parsed.records.iter().find_map(|r| {
            if let OutputRecord::TestSummary {
                passed,
                failed,
                ignored,
                ..
            } = r
            {
                Some((*passed, *failed, *ignored))
            } else {
                None
            }
        });
        assert!(summary.is_some(), "Expected TestSummary record");
        let (passed, failed, ignored) = summary.unwrap();
        assert_eq!(passed, 3);
        assert_eq!(failed, 1);
        assert_eq!(ignored, 1);
    }

    #[test]
    fn mixed_results_summary_severity_is_error() {
        let parsed = parse(MIXED_RESULTS);
        assert_eq!(parsed.summary.severity, Severity::Error);
    }

    #[test]
    fn mixed_results_summary_one_line_contains_counts() {
        let parsed = parse(MIXED_RESULTS);
        assert!(
            parsed.summary.one_line.contains("3 passed"),
            "one_line was: {}",
            parsed.summary.one_line
        );
        assert!(
            parsed.summary.one_line.contains("1 failed"),
            "one_line was: {}",
            parsed.summary.one_line
        );
    }

    #[test]
    fn failure_message_extracted_from_block() {
        let parsed = parse(MIXED_RESULTS);
        let failed_test = parsed.records.iter().find_map(|r| {
            if let OutputRecord::TestResult {
                name,
                status: TestStatus::Failed,
                failure_message,
                ..
            } = r
            {
                Some((name.as_str(), failure_message.clone()))
            } else {
                None
            }
        });
        assert!(failed_test.is_some());
        let (name, msg) = failed_test.unwrap();
        assert_eq!(name, "foo::test_b");
        let msg = msg.expect("failure_message should be populated");
        assert!(
            msg.contains("assertion failed") || msg.contains("panicked"),
            "failure message was: {}",
            msg
        );
    }

    #[test]
    fn all_passing_produces_success_severity() {
        let parsed = parse(ALL_PASSING);
        assert_eq!(parsed.output_type, OutputType::RustTest);
        assert_eq!(parsed.summary.severity, Severity::Success);
    }

    #[test]
    fn all_passing_one_line_says_all_passed() {
        let parsed = parse(ALL_PASSING);
        assert!(
            parsed.summary.one_line.contains("all") || parsed.summary.one_line.contains("3"),
            "one_line was: {}",
            parsed.summary.one_line
        );
        assert!(
            parsed.summary.one_line.contains("passed"),
            "one_line was: {}",
            parsed.summary.one_line
        );
    }

    #[test]
    fn all_passing_has_duration_in_summary() {
        let parsed = parse(ALL_PASSING);
        let dur = parsed.records.iter().find_map(|r| {
            if let OutputRecord::TestSummary {
                total_duration_ms, ..
            } = r
            {
                *total_duration_ms
            } else {
                None
            }
        });
        assert!(dur.is_some(), "Expected duration in TestSummary");
        // "finished in 0.02s" -> 20ms
        assert_eq!(dur.unwrap(), 20);
    }

    #[test]
    fn compilation_failure_delegates_to_cargo_build_parser() {
        // No "running N tests" line -> compilation failure -> delegates to cargo_build
        let parsed = parse(COMPILE_FAILURE);
        // Result should be RustCompiler type (from cargo_build parser)
        assert_eq!(parsed.output_type, OutputType::RustCompiler);
        // Should have CompilerError records
        let has_compiler_error = parsed
            .records
            .iter()
            .any(|r| matches!(r, OutputRecord::CompilerError { .. }));
        assert!(
            has_compiler_error,
            "Expected CompilerError records, got: {:?}",
            parsed.records
        );
    }

    #[test]
    fn empty_output_returns_freeform_not_crash() {
        let parsed = parse("");
        // Empty output has no "running N tests" -> delegates to cargo_build
        // cargo_build with empty returns Success/no records but RustCompiler type
        assert_eq!(parsed.output_type, OutputType::RustCompiler);
        assert_eq!(parsed.records.len(), 0);
    }

    #[test]
    fn token_estimate_is_reasonable() {
        let parsed = parse(ALL_PASSING);
        // 3 TestResult + 1 TestSummary = 4 records; 5 + 4*8 = 37
        assert_eq!(parsed.summary.token_estimate, 5 + parsed.records.len() * 8);
    }

    #[test]
    fn raw_metrics_populated_correctly() {
        let parsed = parse(ALL_PASSING);
        let expected_lines = ALL_PASSING.lines().count();
        assert_eq!(parsed.raw_line_count, expected_lines);
        assert_eq!(parsed.raw_byte_count, ALL_PASSING.len());
    }
}

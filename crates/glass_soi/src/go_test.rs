//! Parser for `go test` output.
//!
//! Handles both verbose (`-v`) output with per-test `--- PASS/FAIL` lines and
//! non-verbose output with package-level `ok`/`FAIL` lines.
//! Chains to `go_build::parse` when the output looks like a compilation failure.

use std::sync::OnceLock;

use regex::Regex;

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity, TestStatus};

fn re_run() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^=== RUN\s+(\S+)").expect("go test run regex"))
}

fn re_result() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^--- (PASS|FAIL|SKIP): (\S+) \(([0-9.]+)s\)$").expect("go test result regex")
    })
}

fn re_ok_line() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^ok\s+\S+\s+([0-9.]+)s").expect("go test ok line regex"))
}

fn re_fail_line() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^FAIL\s+\S+\s+([0-9.]+)s").expect("go test fail line regex"))
}

fn re_go_build_error() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^.+?:\d+:\d+: .+$").expect("go build error detection regex"))
}

/// Parse `go test` output into structured records.
///
/// - Verbose output: `=== RUN` + `--- PASS/FAIL/SKIP` → `TestResult` records
/// - Non-verbose: `ok` / `FAIL` package lines → `TestSummary` record
/// - Compilation failure (no `=== RUN`, has `file:line:col:` errors) → delegates to `go_build::parse`
///
/// Returns a freeform fallback if no patterns match.
pub fn parse(output: &str) -> ParsedOutput {
    // Check if this is a compilation failure — no === RUN lines but has build error pattern
    let has_run_lines = re_run().is_match(output);
    let has_ok_or_fail = re_ok_line().is_match(output) || re_fail_line().is_match(output);

    if !has_run_lines && !has_ok_or_fail {
        // Possibly a build failure — check for go build error pattern
        if re_go_build_error().is_match(output) {
            return super::go_build::parse(output);
        }
        return crate::freeform_parse(output, Some(OutputType::GoTest), None);
    }

    let run_re = re_run();
    let result_re = re_result();
    let ok_re = re_ok_line();
    let fail_re = re_fail_line();

    let mut records: Vec<OutputRecord> = Vec::new();
    let mut current_test: Option<String> = None;
    let mut failure_lines: Vec<String> = Vec::new();
    let mut in_failure_output = false;

    // For non-verbose summary
    let mut non_verbose_passed = 0u32;
    let mut non_verbose_failed = 0u32;
    let mut non_verbose_duration: Option<f64> = None;

    for line in output.lines() {
        if line.len() > 4096 {
            continue;
        }

        // === RUN line: track current test name
        if let Some(caps) = run_re.captures(line) {
            current_test = Some(caps[1].to_string());
            in_failure_output = false;
            failure_lines.clear();
            continue;
        }

        // --- PASS/FAIL/SKIP result line
        if let Some(caps) = result_re.captures(line) {
            let status_str = &caps[1];
            let name = caps[2].to_string();
            let duration_secs: f64 = caps[3].parse().unwrap_or(0.0);
            let duration_ms = Some((duration_secs * 1000.0) as u64);

            let status = match status_str {
                "PASS" => TestStatus::Passed,
                "FAIL" => TestStatus::Failed,
                "SKIP" => TestStatus::Skipped,
                _ => TestStatus::Passed,
            };

            let failure_message = if status == TestStatus::Failed && !failure_lines.is_empty() {
                // Trim trailing empty lines
                let trimmed: Vec<&str> = failure_lines
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
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.join("\n"))
                }
            } else {
                None
            };

            records.push(OutputRecord::TestResult {
                name: name.clone(),
                status,
                duration_ms,
                failure_message,
                failure_location: None,
            });

            current_test = None;
            in_failure_output = false;
            failure_lines.clear();
            continue;
        }

        // Non-verbose: "ok  \texample.com/myapp\t0.013s"
        if let Some(caps) = ok_re.captures(line) {
            let secs: f64 = caps[1].parse().unwrap_or(0.0);
            non_verbose_passed += 1;
            non_verbose_duration = Some(secs);
            continue;
        }

        // Non-verbose: "FAIL\texample.com/myapp\t0.005s"
        if let Some(caps) = fail_re.captures(line) {
            let secs: f64 = caps[1].parse().unwrap_or(0.0);
            non_verbose_failed += 1;
            non_verbose_duration = Some(secs);
            continue;
        }

        // Collect indented output as potential failure message for current test
        if current_test.is_some() && (line.starts_with("    ") || line.starts_with('\t')) {
            in_failure_output = true;
            failure_lines.push(line.to_string());
        } else if in_failure_output && line.trim().is_empty() {
            failure_lines.push(line.to_string());
        }
    }

    // Non-verbose path: produce TestSummary records
    if non_verbose_passed > 0 || non_verbose_failed > 0 {
        let total_duration_ms = non_verbose_duration.map(|s| (s * 1000.0) as u64);
        records.push(OutputRecord::TestSummary {
            passed: non_verbose_passed,
            failed: non_verbose_failed,
            skipped: 0,
            ignored: 0,
            total_duration_ms,
        });
    }

    if records.is_empty() {
        return crate::freeform_parse(output, Some(OutputType::GoTest), None);
    }

    // Count results for summary
    let (passed, failed, skipped) = records.iter().fold((0u32, 0u32, 0u32), |acc, r| match r {
        OutputRecord::TestResult { status, .. } => match status {
            TestStatus::Passed => (acc.0 + 1, acc.1, acc.2),
            TestStatus::Failed => (acc.0, acc.1 + 1, acc.2),
            TestStatus::Skipped | TestStatus::Ignored => (acc.0, acc.1, acc.2 + 1),
        },
        OutputRecord::TestSummary {
            passed,
            failed,
            skipped,
            ignored,
            ..
        } => (acc.0 + passed, acc.1 + failed, acc.2 + skipped + ignored),
        _ => acc,
    });

    let one_line = if failed == 0 && passed > 0 {
        format!("all {} passed", passed)
    } else if failed > 0 {
        format!("{} passed, {} failed", passed, failed)
    } else if skipped > 0 {
        format!("{} skipped", skipped)
    } else {
        format!(
            "{} test result{}",
            records.len(),
            if records.len() == 1 { "" } else { "s" }
        )
    };

    let severity = if failed > 0 {
        Severity::Error
    } else if passed > 0 || skipped > 0 {
        Severity::Success
    } else {
        Severity::Info
    };

    let token_estimate = 5 + records.len() * 8;

    ParsedOutput {
        output_type: OutputType::GoTest,
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

    const GO_TEST_VERBOSE_PASS: &str = "=== RUN   TestAdd
--- PASS: TestAdd (0.00s)
=== RUN   TestMultiply
--- PASS: TestMultiply (0.00s)
ok  \texample.com/myapp\t0.002s";

    const GO_TEST_VERBOSE_FAIL: &str = "=== RUN   TestAdd
--- PASS: TestAdd (0.00s)
=== RUN   TestSubtract
    main_test.go:15: got 3, want 2
--- FAIL: TestSubtract (0.01s)
FAIL\texample.com/myapp\t0.015s";

    const GO_TEST_NON_VERBOSE_OK: &str = "ok  \texample.com/myapp\t0.013s";

    const GO_TEST_NON_VERBOSE_FAIL: &str = "FAIL\texample.com/myapp\t0.005s";

    const GO_TEST_BUILD_FAILURE: &str = "./main_test.go:10:5: undefined: NonExistentFunc";

    #[test]
    fn go_test_verbose_pass_produces_test_result_records() {
        let parsed = parse(GO_TEST_VERBOSE_PASS);
        assert_eq!(parsed.output_type, OutputType::GoTest);
        let test_results: Vec<_> = parsed
            .records
            .iter()
            .filter(|r| matches!(r, OutputRecord::TestResult { .. }))
            .collect();
        assert_eq!(test_results.len(), 2);
    }

    #[test]
    fn go_test_verbose_pass_correct_names_and_status() {
        let parsed = parse(GO_TEST_VERBOSE_PASS);
        let results: Vec<_> = parsed
            .records
            .iter()
            .filter_map(|r| {
                if let OutputRecord::TestResult { name, status, .. } = r {
                    Some((name.as_str(), status.clone()))
                } else {
                    None
                }
            })
            .collect();
        let test_add = results.iter().find(|(n, _)| *n == "TestAdd");
        assert!(test_add.is_some());
        assert_eq!(test_add.unwrap().1, TestStatus::Passed);
    }

    #[test]
    fn go_test_verbose_pass_duration_extracted() {
        let parsed = parse(GO_TEST_VERBOSE_PASS);
        let dur = parsed.records.iter().find_map(|r| {
            if let OutputRecord::TestResult {
                name, duration_ms, ..
            } = r
            {
                if name == "TestAdd" {
                    *duration_ms
                } else {
                    None
                }
            } else {
                None
            }
        });
        assert!(dur.is_some());
        assert_eq!(dur.unwrap(), 0); // 0.00s -> 0ms
    }

    #[test]
    fn go_test_verbose_fail_status_is_failed() {
        let parsed = parse(GO_TEST_VERBOSE_FAIL);
        let failed = parsed.records.iter().find_map(|r| {
            if let OutputRecord::TestResult { name, status, .. } = r {
                if name == "TestSubtract" {
                    Some(status.clone())
                } else {
                    None
                }
            } else {
                None
            }
        });
        assert!(failed.is_some());
        assert_eq!(failed.unwrap(), TestStatus::Failed);
    }

    #[test]
    fn go_test_verbose_fail_message_extracted() {
        let parsed = parse(GO_TEST_VERBOSE_FAIL);
        let msg = parsed.records.iter().find_map(|r| {
            if let OutputRecord::TestResult {
                name,
                status: TestStatus::Failed,
                failure_message,
                ..
            } = r
            {
                if name == "TestSubtract" {
                    failure_message.clone()
                } else {
                    None
                }
            } else {
                None
            }
        });
        assert!(msg.is_some(), "Expected failure message");
        assert!(
            msg.unwrap().contains("got 3, want 2"),
            "Failure message should contain the assertion detail"
        );
    }

    #[test]
    fn go_test_non_verbose_ok_produces_test_summary() {
        let parsed = parse(GO_TEST_NON_VERBOSE_OK);
        assert_eq!(parsed.output_type, OutputType::GoTest);
        let summary = parsed.records.iter().find_map(|r| {
            if let OutputRecord::TestSummary { passed, failed, .. } = r {
                Some((*passed, *failed))
            } else {
                None
            }
        });
        assert!(summary.is_some(), "Expected TestSummary");
        let (passed, failed) = summary.unwrap();
        assert_eq!(passed, 1);
        assert_eq!(failed, 0);
    }

    #[test]
    fn go_test_non_verbose_fail_produces_test_summary_with_failed() {
        let parsed = parse(GO_TEST_NON_VERBOSE_FAIL);
        assert_eq!(parsed.output_type, OutputType::GoTest);
        let summary = parsed.records.iter().find_map(|r| {
            if let OutputRecord::TestSummary { passed, failed, .. } = r {
                Some((*passed, *failed))
            } else {
                None
            }
        });
        assert!(summary.is_some(), "Expected TestSummary");
        let (passed, failed) = summary.unwrap();
        assert_eq!(passed, 0);
        assert_eq!(failed, 1);
    }

    #[test]
    fn go_test_build_failure_delegates_to_go_build() {
        let parsed = parse(GO_TEST_BUILD_FAILURE);
        // Delegates to go_build -> GoBuild output type
        assert_eq!(parsed.output_type, OutputType::GoBuild);
        let has_compiler_error = parsed
            .records
            .iter()
            .any(|r| matches!(r, OutputRecord::CompilerError { .. }));
        assert!(
            has_compiler_error,
            "Expected CompilerError from go_build delegation"
        );
    }

    #[test]
    fn go_test_all_passing_severity_is_success() {
        let parsed = parse(GO_TEST_VERBOSE_PASS);
        assert_eq!(parsed.summary.severity, Severity::Success);
    }

    #[test]
    fn go_test_failure_severity_is_error() {
        let parsed = parse(GO_TEST_VERBOSE_FAIL);
        assert_eq!(parsed.summary.severity, Severity::Error);
    }

    #[test]
    fn go_test_all_passing_one_line_says_all_passed() {
        let parsed = parse(GO_TEST_VERBOSE_PASS);
        assert!(
            parsed.summary.one_line.contains("passed"),
            "one_line was: {}",
            parsed.summary.one_line
        );
    }

    #[test]
    fn go_test_raw_metrics_populated() {
        let parsed = parse(GO_TEST_NON_VERBOSE_OK);
        assert_eq!(parsed.raw_line_count, 1);
        assert_eq!(parsed.raw_byte_count, GO_TEST_NON_VERBOSE_OK.len());
    }
}

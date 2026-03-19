//! Test-name extraction and coverage gap correlation.
//!
//! Parses test names from verification command output, correlates them
//! with changed files from `git diff --name-only`, and flags files
//! that have no matching tests.

use regex::Regex;
use std::sync::OnceLock;

/// A file modified in git diff that has few or no matching tests.
#[derive(Debug, Clone, PartialEq)]
pub struct CoverageGap {
    pub file: String,
    pub matched_test_count: usize,
}

/// A parsed test name with its module/path segments.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedTest {
    pub full_name: String,
    pub segments: Vec<String>,
    pub passed: bool,
}

/// Extract test names from verification command output.
///
/// Supports Rust, Jest, Pytest, and Go output formats.
/// Uses early-return: once a framework matches, others are skipped.
/// This assumes one project = one test framework.
pub fn extract_test_names(output: &str) -> Vec<ParsedTest> {
    let mut tests = Vec::new();

    // Rust: "test module::submod::test_name ... ok"
    static RE_RUST: OnceLock<Regex> = OnceLock::new();
    let re_rust = RE_RUST.get_or_init(|| Regex::new(r"test ([\w:]+) \.\.\. (ok|FAILED)").unwrap());
    for caps in re_rust.captures_iter(output) {
        let name = caps[1].to_string();
        let passed = &caps[2] == "ok";
        let segments = name
            .split("::")
            .filter(|s| *s != "tests" && *s != "test")
            .flat_map(|s| {
                // "test_login" -> "login", "test_connect" -> "connect"
                let stripped = s.strip_prefix("test_").unwrap_or(s);
                // split on "_" to get sub-segments
                stripped
                    .split('_')
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
            })
            .map(|s| s.to_lowercase())
            .collect();
        tests.push(ParsedTest {
            full_name: name,
            segments,
            passed,
        });
    }
    if !tests.is_empty() {
        return tests;
    }

    // Jest: "PASS src/auth.test.js" or "FAIL src/auth.test.js"
    static RE_JEST_FILE: OnceLock<Regex> = OnceLock::new();
    let re_jest_file = RE_JEST_FILE.get_or_init(|| Regex::new(r"(PASS|FAIL)\s+(\S+)").unwrap());
    for caps in re_jest_file.captures_iter(output) {
        let path = caps[2].to_string();
        let passed = &caps[1] == "PASS";
        let segments = path_to_segments(&path);
        tests.push(ParsedTest {
            full_name: path,
            segments,
            passed,
        });
    }
    if !tests.is_empty() {
        return tests;
    }

    // Pytest: "tests/test_auth.py::test_login PASSED"
    static RE_PYTEST: OnceLock<Regex> = OnceLock::new();
    let re_pytest = RE_PYTEST.get_or_init(|| Regex::new(r"(\S+::\S+)\s+(PASSED|FAILED)").unwrap());
    for caps in re_pytest.captures_iter(output) {
        let name = caps[1].to_string();
        let passed = &caps[2] == "PASSED";
        // Split into path part (before first "::") and function names (after).
        let parts: Vec<&str> = name.splitn(2, "::").collect();
        let path_part = parts.first().copied().unwrap_or("");
        let func_part = parts.get(1).copied().unwrap_or("");
        // Path segments: strip "test_" prefix from file stems.
        let path_segs = path_part
            .split('/')
            .flat_map(|part| part.split('.'))
            .filter(|s| !s.is_empty() && *s != "py" && *s != "tests")
            .flat_map(|s| {
                let stripped = s.strip_prefix("test_").unwrap_or(s);
                stripped
                    .split('_')
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
            })
            .map(|s| s.to_lowercase());
        // Function name segments: keep as-is (e.g. "test_login").
        let func_segs = func_part
            .split("::")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase());
        let segments: Vec<String> = path_segs.chain(func_segs).collect();
        tests.push(ParsedTest {
            full_name: name,
            segments,
            passed,
        });
    }
    if !tests.is_empty() {
        return tests;
    }

    // Go: "--- PASS: TestAuth (0.00s)" — subtests use parent only
    static RE_GO: OnceLock<Regex> = OnceLock::new();
    let re_go = RE_GO.get_or_init(|| Regex::new(r"--- (PASS|FAIL): (\w+)").unwrap());
    for caps in re_go.captures_iter(output) {
        let name = caps[2].to_string();
        let passed = &caps[1] == "PASS";
        let segments = camel_to_segments(&name);
        tests.push(ParsedTest {
            full_name: name,
            segments,
            passed,
        });
    }

    tests
}

/// Extract path segments from a file path, filtering noise.
/// `src/auth/login.rs` -> `["auth", "login"]`
pub fn path_to_segments(path: &str) -> Vec<String> {
    path.replace('\\', "/")
        .split('/')
        .flat_map(|part| part.split('.'))
        .filter(|s| {
            !s.is_empty()
                && !matches!(
                    *s,
                    "src"
                        | "lib"
                        | "mod"
                        | "index"
                        | "rs"
                        | "ts"
                        | "tsx"
                        | "js"
                        | "jsx"
                        | "py"
                        | "go"
                        | "test"
                        | "tests"
                        | "spec"
                        | "crates"
                        | "__tests__"
                )
        })
        .map(|s| s.to_lowercase())
        .collect()
}

/// Convert CamelCase to lowercase segments.
/// `TestAuthLogin` -> `["auth", "login"]`
fn camel_to_segments(name: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let name = name.strip_prefix("Test").unwrap_or(name);
    for ch in name.chars() {
        if ch.is_uppercase() && !current.is_empty() {
            segments.push(current.to_lowercase());
            current.clear();
        }
        current.push(ch);
    }
    if !current.is_empty() {
        segments.push(current.to_lowercase());
    }
    segments
}

/// Find files with no matching tests in the verification output.
///
/// For each changed file, extract its path segments and check if any
/// test name shares at least one segment. Files with zero matches
/// are reported as coverage gaps.
pub fn find_coverage_gaps(test_output: &str, changed_files: &[String]) -> Vec<CoverageGap> {
    let tests = extract_test_names(test_output);

    changed_files
        .iter()
        .filter_map(|file| {
            let file_segments = path_to_segments(file);
            if file_segments.is_empty() {
                return None;
            }

            let matched = tests
                .iter()
                .filter(|t| {
                    t.segments
                        .iter()
                        .any(|ts| file_segments.iter().any(|fs| fs == ts))
                })
                .count();

            if matched == 0 {
                Some(CoverageGap {
                    file: file.clone(),
                    matched_test_count: 0,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Format coverage gaps for inclusion in the agent context string.
/// Returns an empty string if there are no gaps.
pub fn format_gaps_for_context(gaps: &[CoverageGap]) -> String {
    if gaps.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for gap in gaps {
        out.push_str(&format!(
            "[COVERAGE_GAP] {} was modified but no tests appear to reference it (approximate match)\n",
            gap.file
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_rust_test_names() {
        let output = "\
test auth::tests::test_login ... ok
test auth::tests::test_logout ... ok
test db::tests::test_connect ... FAILED
test result: FAILED. 2 passed; 1 failed; 0 ignored";
        let tests = extract_test_names(output);
        assert_eq!(tests.len(), 3);
        assert_eq!(tests[0].full_name, "auth::tests::test_login");
        assert!(tests[0].passed);
        assert!(tests[0].segments.contains(&"auth".to_string()));
        assert!(tests[0].segments.contains(&"login".to_string()));
        assert!(!tests[0].segments.contains(&"tests".to_string()));
        assert!(!tests[2].passed);
    }

    #[test]
    fn extract_rust_ignores_doc_tests() {
        let output = "test src/lib.rs - MyType (line 42) ... ok\n";
        let tests = extract_test_names(output);
        assert!(tests.is_empty());
    }

    #[test]
    fn extract_rust_ignores_ignored_tests() {
        let output = "test auth::test_slow ... ignored\n";
        let tests = extract_test_names(output);
        assert!(tests.is_empty());
    }

    #[test]
    fn extract_jest_test_names() {
        let output = "PASS src/auth.test.js\nFAIL src/db.test.js";
        let tests = extract_test_names(output);
        assert_eq!(tests.len(), 2);
        assert!(tests[0].passed);
        assert!(tests[0].segments.contains(&"auth".to_string()));
        assert!(!tests[1].passed);
    }

    #[test]
    fn extract_pytest_test_names() {
        let output =
            "tests/test_auth.py::test_login PASSED\ntests/test_auth.py::test_logout FAILED";
        let tests = extract_test_names(output);
        assert_eq!(tests.len(), 2);
        assert!(tests[0].passed);
        assert!(tests[0].segments.contains(&"auth".to_string()));
        assert!(tests[0].segments.contains(&"test_login".to_string()));
    }

    #[test]
    fn extract_go_test_names() {
        let output = "--- PASS: TestAuthLogin (0.00s)\n--- FAIL: TestDbConnect (0.12s)";
        let tests = extract_test_names(output);
        assert_eq!(tests.len(), 2);
        assert!(tests[0].passed);
        assert!(tests[0].segments.contains(&"auth".to_string()));
        assert!(tests[0].segments.contains(&"login".to_string()));
    }

    #[test]
    fn extract_go_subtests_uses_parent() {
        let output = "--- PASS: TestAuth/login_success (0.00s)\n--- PASS: TestAuth (0.00s)";
        let tests = extract_test_names(output);
        assert!(tests.iter().all(|t| t.full_name == "TestAuth"));
    }

    #[test]
    fn path_segments_basic() {
        let segs = path_to_segments("src/auth/login.rs");
        assert_eq!(segs, vec!["auth", "login"]);
    }

    #[test]
    fn path_segments_filters_noise() {
        let segs = path_to_segments("crates/glass_feedback/src/coverage.rs");
        assert_eq!(segs, vec!["glass_feedback", "coverage"]);
    }

    #[test]
    fn path_segments_windows() {
        let segs = path_to_segments("src\\auth\\login.rs");
        assert_eq!(segs, vec!["auth", "login"]);
    }

    #[test]
    fn camel_to_segments_basic() {
        let segs = camel_to_segments("TestAuthLogin");
        assert_eq!(segs, vec!["auth", "login"]);
    }

    #[test]
    fn camel_to_segments_no_test_prefix() {
        let segs = camel_to_segments("AuthLogin");
        assert_eq!(segs, vec!["auth", "login"]);
    }

    // --- Correlation ---

    #[test]
    fn correlate_finds_gap() {
        let test_output = "\
test auth::tests::test_login ... ok
test auth::tests::test_logout ... ok";
        let changed_files = vec!["src/auth.rs".to_string(), "src/db.rs".to_string()];
        let gaps = find_coverage_gaps(test_output, &changed_files);
        // auth.rs has matching tests (segment "auth"), db.rs does not
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].file, "src/db.rs");
        assert_eq!(gaps[0].matched_test_count, 0);
    }

    #[test]
    fn correlate_no_gap_when_all_covered() {
        let test_output = "\
test auth::tests::test_login ... ok
test db::tests::test_connect ... ok";
        let changed_files = vec!["src/auth.rs".to_string(), "src/db.rs".to_string()];
        let gaps = find_coverage_gaps(test_output, &changed_files);
        assert!(gaps.is_empty());
    }

    #[test]
    fn correlate_common_segments_still_match() {
        let test_output = "test util::tests::test_format ... ok\n";
        let changed_files = vec!["src/util.rs".to_string()];
        let gaps = find_coverage_gaps(test_output, &changed_files);
        assert!(gaps.is_empty());
    }

    #[test]
    fn correlate_empty_test_output() {
        let changed_files = vec!["src/auth.rs".to_string()];
        let gaps = find_coverage_gaps("", &changed_files);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].file, "src/auth.rs");
    }

    #[test]
    fn correlate_empty_changed_files() {
        let test_output = "test auth::tests::test_login ... ok\n";
        let gaps = find_coverage_gaps(test_output, &[]);
        assert!(gaps.is_empty());
    }

    #[test]
    fn format_coverage_gaps_message() {
        let gaps = vec![
            CoverageGap {
                file: "src/db.rs".to_string(),
                matched_test_count: 0,
            },
            CoverageGap {
                file: "src/config.rs".to_string(),
                matched_test_count: 0,
            },
        ];
        let msg = format_gaps_for_context(&gaps);
        assert!(msg.contains("[COVERAGE_GAP]"));
        assert!(msg.contains("src/db.rs"));
        assert!(msg.contains("approximate match"));
    }

    #[test]
    fn format_no_gaps_returns_empty() {
        let msg = format_gaps_for_context(&[]);
        assert!(msg.is_empty());
    }
}

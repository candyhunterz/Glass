//! Output type classifier for the SOI pipeline.
//!
//! Uses a two-stage approach:
//! 1. Command hint matching (fast, deterministic)
//! 2. Content sniffing (regex-based, for hint-less invocation)

use std::sync::OnceLock;

use regex::Regex;

use crate::types::OutputType;

/// Classify command output into an [`OutputType`].
///
/// # Arguments
///
/// * `output` - The raw command output (may contain ANSI sequences)
/// * `command_hint` - The command that produced this output (e.g. `"cargo build"`)
///
/// Returns [`OutputType::FreeformText`] when no pattern matches.
pub fn classify(output: &str, command_hint: Option<&str>) -> OutputType {
    // Stage 1: command hint
    if let Some(hint) = command_hint {
        if let Some(t) = classify_by_hint(hint) {
            return t;
        }
    }

    // Stage 2: content sniff
    classify_by_content(output)
}

/// Match against known command prefixes/patterns.
fn classify_by_hint(hint: &str) -> Option<OutputType> {
    let cmd = hint.trim().to_lowercase();

    // Rust / cargo
    if cmd.starts_with("cargo build")
        || cmd.starts_with("cargo check")
        || cmd.starts_with("cargo clippy")
        || cmd.starts_with("rustc ")
        || cmd == "rustc"
    {
        return Some(OutputType::RustCompiler);
    }
    if cmd.starts_with("cargo test") {
        return Some(OutputType::RustTest);
    }
    // cargo add / cargo update / cargo fetch / cargo publish
    if cmd.starts_with("cargo ") {
        return Some(OutputType::Cargo);
    }

    // Jest (must come before generic npx check)
    if cmd == "jest"
        || cmd.starts_with("jest ")
        || cmd == "npx jest"
        || cmd.starts_with("npx jest ")
    {
        return Some(OutputType::Jest);
    }

    // npm / npx (generic)
    if cmd.starts_with("npm ") || cmd.starts_with("npx ") {
        return Some(OutputType::Npm);
    }

    // Pytest
    if cmd == "pytest"
        || cmd.starts_with("pytest ")
        || cmd == "python -m pytest"
        || cmd.starts_with("python -m pytest ")
        || cmd == "python3 -m pytest"
        || cmd.starts_with("python3 -m pytest ")
    {
        return Some(OutputType::Pytest);
    }

    // Go
    if cmd.starts_with("go build") || cmd.starts_with("go vet") {
        return Some(OutputType::GoBuild);
    }
    if cmd.starts_with("go test") {
        return Some(OutputType::GoTest);
    }

    // TypeScript
    if cmd == "tsc" || cmd.starts_with("tsc ") || cmd.starts_with("npx tsc") {
        return Some(OutputType::TypeScript);
    }

    // Docker
    if cmd.starts_with("docker ") || cmd.starts_with("docker-compose ") {
        return Some(OutputType::Docker);
    }

    // kubectl
    if cmd.starts_with("kubectl ") {
        return Some(OutputType::Kubectl);
    }

    // Terraform
    if cmd.starts_with("terraform ") {
        return Some(OutputType::Terraform);
    }

    // Git
    if cmd.starts_with("git ") || cmd == "git" {
        return Some(OutputType::Git);
    }

    // pip / pip3
    if cmd.starts_with("pip ") || cmd.starts_with("pip3 ") {
        return Some(OutputType::Pip);
    }

    None
}

/// Sniff output content for known format signatures.
fn classify_by_content(output: &str) -> OutputType {
    // Rust compiler JSON output
    if has_rust_json_marker(output) {
        return OutputType::RustCompiler;
    }

    // Rust human-readable compiler output (error[E####] pattern)
    if has_rust_human_marker(output) {
        return OutputType::RustCompiler;
    }

    // Rust test runner output
    if has_rust_test_marker(output) {
        return OutputType::RustTest;
    }

    // npm package install output
    if has_npm_marker(output) {
        return OutputType::Npm;
    }

    // pytest output
    if has_pytest_marker(output) {
        return OutputType::Pytest;
    }

    // Jest output
    if has_jest_marker(output) {
        return OutputType::Jest;
    }

    // Git status output
    if has_git_marker(output) {
        return OutputType::Git;
    }

    // TypeScript compiler output: "file(line,col): error|warning TSxxxx:"
    if has_tsc_marker(output) {
        return OutputType::TypeScript;
    }

    // Go test output: verbose test markers
    if has_go_test_marker(output) {
        return OutputType::GoTest;
    }

    OutputType::FreeformText
}

fn has_rust_json_marker(output: &str) -> bool {
    output.contains(r#""reason":"compiler-message""#)
        || output.contains(r#""$message_type":"diagnostic""#)
}

fn has_rust_human_marker(output: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"error\[E\d+\]|warning\[E\d+\]").expect("valid regex"));
    re.is_match(output)
}

fn has_rust_test_marker(output: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re =
        RE.get_or_init(|| Regex::new(r"running \d+ tests?|test result:").expect("valid regex"));
    re.is_match(output)
}

fn has_npm_marker(output: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"added \d+ packages?").expect("valid regex"));
    re.is_match(output)
}

fn has_pytest_marker(output: &str) -> bool {
    // Conservative: require both "::" (test id separator) and PASSED/FAILED
    (output.contains("PASSED") || output.contains("FAILED")) && output.contains("::")
}

fn has_jest_marker(output: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        // Lines like "PASS src/app.test.js" or "FAIL src/app.test.ts"
        Regex::new(r"(?m)^(PASS|FAIL) \S+\.(js|ts|jsx|tsx|mjs|cjs)").expect("valid regex")
    });
    re.is_match(output)
}

fn has_git_marker(output: &str) -> bool {
    output.contains("On branch ")
        || output.contains("Changes not staged for commit")
        || output.contains("nothing to commit")
        || output.contains("Untracked files:")
}

fn has_tsc_marker(output: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"\(\d+,\d+\): (?:error|warning) TS\d+:").expect("tsc marker regex")
    });
    re.is_match(output)
}

fn has_go_test_marker(output: &str) -> bool {
    output.contains("--- PASS:") || output.contains("--- FAIL:") || output.contains("=== RUN   ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- command hint tests -----

    #[test]
    fn hint_cargo_build_is_rust_compiler() {
        assert_eq!(classify("", Some("cargo build")), OutputType::RustCompiler);
    }

    #[test]
    fn hint_cargo_check_is_rust_compiler() {
        assert_eq!(classify("", Some("cargo check")), OutputType::RustCompiler);
    }

    #[test]
    fn hint_cargo_clippy_is_rust_compiler() {
        assert_eq!(classify("", Some("cargo clippy")), OutputType::RustCompiler);
    }

    #[test]
    fn hint_cargo_test_is_rust_test() {
        assert_eq!(classify("", Some("cargo test")), OutputType::RustTest);
    }

    #[test]
    fn hint_npm_install_is_npm() {
        assert_eq!(classify("", Some("npm install")), OutputType::Npm);
    }

    #[test]
    fn hint_npm_update_is_npm() {
        assert_eq!(classify("", Some("npm update")), OutputType::Npm);
    }

    #[test]
    fn hint_npx_jest_is_jest() {
        assert_eq!(classify("", Some("npx jest")), OutputType::Jest);
    }

    #[test]
    fn hint_jest_is_jest() {
        assert_eq!(classify("", Some("jest")), OutputType::Jest);
    }

    #[test]
    fn hint_jest_with_args_is_jest() {
        assert_eq!(classify("", Some("jest --watch")), OutputType::Jest);
    }

    #[test]
    fn hint_pytest_is_pytest() {
        assert_eq!(classify("", Some("pytest")), OutputType::Pytest);
    }

    #[test]
    fn hint_python_m_pytest_is_pytest() {
        assert_eq!(classify("", Some("python -m pytest")), OutputType::Pytest);
    }

    #[test]
    fn hint_pytest_with_args_is_pytest() {
        assert_eq!(classify("", Some("pytest tests/ -v")), OutputType::Pytest);
    }

    // Phase 54 future: these arms are wired now, parsers implemented later
    #[test]
    fn hint_git_status_is_git() {
        assert_eq!(classify("", Some("git status")), OutputType::Git);
    }

    #[test]
    fn hint_docker_build_is_docker() {
        assert_eq!(classify("", Some("docker build .")), OutputType::Docker);
    }

    #[test]
    fn hint_kubectl_get_is_kubectl() {
        assert_eq!(classify("", Some("kubectl get pods")), OutputType::Kubectl);
    }

    #[test]
    fn hint_tsc_is_typescript() {
        assert_eq!(classify("", Some("tsc")), OutputType::TypeScript);
    }

    #[test]
    fn hint_go_build_is_go_build() {
        assert_eq!(classify("", Some("go build ./...")), OutputType::GoBuild);
    }

    #[test]
    fn hint_go_test_is_go_test() {
        assert_eq!(classify("", Some("go test ./...")), OutputType::GoTest);
    }

    // ----- content sniff tests -----

    #[test]
    fn sniff_rust_json_marker_is_rust_compiler() {
        let output = r#"{"reason":"compiler-message","package_id":"foo"}"#;
        assert_eq!(classify(output, None), OutputType::RustCompiler);
    }

    #[test]
    fn sniff_rust_message_type_diagnostic_is_rust_compiler() {
        let output = r#"{"$message_type":"diagnostic","message":"unused"}"#;
        assert_eq!(classify(output, None), OutputType::RustCompiler);
    }

    #[test]
    fn sniff_running_n_tests_is_rust_test() {
        let output = "running 5 tests\ntest foo ... ok\ntest result: ok. 5 passed";
        assert_eq!(classify(output, None), OutputType::RustTest);
    }

    #[test]
    fn sniff_test_result_line_is_rust_test() {
        let output = "test result: FAILED. 1 failed; 4 passed; 0 ignored";
        assert_eq!(classify(output, None), OutputType::RustTest);
    }

    #[test]
    fn sniff_added_n_packages_is_npm() {
        let output = "added 42 packages in 3.5s";
        assert_eq!(classify(output, None), OutputType::Npm);
    }

    #[test]
    fn sniff_unrecognized_is_freeform() {
        let output = "Hello, world! This is completely unstructured.";
        assert_eq!(classify(output, None), OutputType::FreeformText);
    }

    #[test]
    fn sniff_empty_is_freeform() {
        assert_eq!(classify("", None), OutputType::FreeformText);
    }

    #[test]
    fn hint_takes_precedence_over_content() {
        // Content looks like npm but hint says cargo build
        let output = "added 10 packages";
        assert_eq!(
            classify(output, Some("cargo build")),
            OutputType::RustCompiler
        );
    }

    #[test]
    fn sniff_pytest_conservative() {
        // Must have both "::" and PASSED/FAILED
        let output =
            "PASSED tests/test_auth.py::test_login\nFAILED tests/test_auth.py::test_logout";
        assert_eq!(classify(output, None), OutputType::Pytest);
    }

    #[test]
    fn sniff_pytest_no_false_positive_without_colons() {
        // PASSED without "::" should NOT classify as Pytest
        let output = "All tests PASSED successfully";
        assert_eq!(classify(output, None), OutputType::FreeformText);
    }

    #[test]
    fn sniff_jest_pass_fail_lines() {
        let output = "PASS src/auth.test.ts\nFAIL src/user.test.js\nTests: 2 failed, 8 passed";
        assert_eq!(classify(output, None), OutputType::Jest);
    }

    #[test]
    fn sniff_git_status_detected() {
        let output = "On branch main\nnothing to commit";
        assert_eq!(classify(output, None), OutputType::Git);
    }

    #[test]
    fn sniff_git_status_untracked_detected() {
        let output = "On branch main\nUntracked files:\n\tfoo.txt\n\nnothing added to commit but untracked files present";
        assert_eq!(classify(output, None), OutputType::Git);
    }

    #[test]
    fn sniff_git_changes_not_staged_detected() {
        let output = "Changes not staged for commit:\n\tmodified: src/main.rs\n";
        assert_eq!(classify(output, None), OutputType::Git);
    }

    #[test]
    fn sniff_tsc_error_detected() {
        let output = "src/main.ts(10,5): error TS2345: Argument of type 'string' is not assignable";
        assert_eq!(classify(output, None), OutputType::TypeScript);
    }

    #[test]
    fn sniff_tsc_warning_detected() {
        let output =
            "src/utils.ts(3,1): warning TS6133: 'x' is declared but its value is never read.";
        assert_eq!(classify(output, None), OutputType::TypeScript);
    }

    #[test]
    fn sniff_go_test_run_detected() {
        let output = "=== RUN   TestFoo\n--- PASS: TestFoo (0.00s)";
        assert_eq!(classify(output, None), OutputType::GoTest);
    }

    #[test]
    fn sniff_go_test_pass_only_detected() {
        let output = "--- PASS: TestBar (0.01s)";
        assert_eq!(classify(output, None), OutputType::GoTest);
    }

    #[test]
    fn sniff_go_test_fail_detected() {
        let output = "--- FAIL: TestBaz (0.00s)";
        assert_eq!(classify(output, None), OutputType::GoTest);
    }
}

---
phase: 48-soi-classifier-and-parser-crate
plan: "02"
subsystem: glass_soi
tags: [soi, cargo, rust, parser, compiler-errors, test-results]
dependency_graph:
  requires: [glass_errors, glass_soi (plan 01 scaffold)]
  provides: [cargo_build::parse, cargo_test::parse]
  affects: [glass_soi::parse dispatch (already wired in lib.rs)]
tech_stack:
  added: []
  patterns: [glass_errors::extract_errors delegation, OnceLock<Regex>, compilation-failure fallback chaining]
key_files:
  created: []
  modified:
    - crates/glass_soi/src/cargo_build.rs
    - crates/glass_soi/src/cargo_test.rs
decisions:
  - "cargo_build::parse delegates entirely to glass_errors::extract_errors -- no duplicate parsing logic"
  - "glass_errors::Severity::Note and Help both map to SOI Severity::Info (outcome-oriented scale)"
  - "cargo_test::parse detects compilation failure by absence of 'running N tests' line and chains to cargo_build::parse"
  - "Duration is extracted from the summary line itself ('test result: ok. N passed; finished in Xs') not a separate line"
  - "warning: format requires cargo hint for RustHuman detection -- plain warning without [code] falls back to Generic parser in glass_errors"
metrics:
  duration_seconds: 314
  completed_date: "2026-03-12"
  tasks_completed: 2
  files_created: 0
  files_modified: 2
  tests_added: 23
requirements-completed: [SOIP-02, SOIP-03]
---

# Phase 48 Plan 02: Cargo Build/Clippy and Cargo Test Parsers Summary

**One-liner:** cargo_build parser delegates to glass_errors::extract_errors producing CompilerError records with severity mapping (Note/Help -> Info); cargo_test parser extracts per-test TestResult records and TestSummary with failure messages and duration, chaining to cargo_build on compilation failure.

## What Was Built

### cargo_build.rs (crates/glass_soi/src/cargo_build.rs)

Replaced the Plan 01 stub with a full implementation:

- `pub fn parse(output: &str, command_hint: Option<&str>) -> ParsedOutput`
- Calls `glass_errors::extract_errors(output, command_hint)` to get `Vec<StructuredError>`
- Maps each `StructuredError` to `OutputRecord::CompilerError` with all 6 fields: file, line, column, severity, code, message
- Severity mapping: `glass_errors::Severity::Error` → `Severity::Error`, `Warning` → `Warning`, `Note/Help` → `Severity::Info`
- `OutputSummary.one_line`: "N errors, M warnings in file.rs" (includes first error filename) when errors exist; "M warnings" when warnings only; "build succeeded" when clean
- `OutputSummary.severity`: Error if any errors, Warning if warnings only, Success if clean
- `token_estimate`: 5 + records.len() * 10
- Sets `raw_line_count` and `raw_byte_count` from output

10 tests:
- JSON compiler message → CompilerError record with all fields
- Human-readable `error[E0308]` → CompilerError record
- Clean build → empty records, Success severity, "build succeeded"
- Warning-only → Warning severity
- Mixed errors + warnings → Error severity with both record types
- Empty output → Success, no records
- Raw metrics, token estimate, Note/Help → Info mapping

### cargo_test.rs (crates/glass_soi/src/cargo_test.rs)

Replaced the Plan 01 stub with a full implementation:

- `pub fn parse(output: &str) -> ParsedOutput`
- Detects compilation failure by checking for `running \d+ tests?` regex; if absent, chains to `super::cargo_build::parse(output, Some("cargo test"))`
- Regex patterns (OnceLock): TEST_LINE, FAILURE_HEADER, TEST_SUMMARY, TEST_DURATION
- Line-by-line parsing:
  - `test name ... ok/FAILED/ignored` → `OutputRecord::TestResult`
  - `---- name stdout ----` → starts failure message collection
  - `test result: FAILED. N passed; M failed; P ignored; finished in Xs` → `OutputRecord::TestSummary` with duration extracted from same line
- Attaches collected failure messages to matching Failed TestResult records
- `OutputSummary.one_line`: "all N passed" if no failures; "N passed, M failed, P ignored" with failures
- `OutputSummary.severity`: Error if any failed, Success if all passed/ignored, Info otherwise

13 tests:
- Mixed results → correct TestResult records for each test
- Status mapping: ok→Passed, FAILED→Failed, ignored→Ignored
- TestSummary with correct counts (3 passed, 1 failed, 1 ignored)
- Failure message extracted from block and attached to correct Failed record
- All passing → Success severity, "all N passed" one_line
- Duration from summary line (e.g., `finished in 0.02s` → 20ms)
- Compilation failure (no "running" line) → delegates to cargo_build, returns CompilerError records
- Empty output → no crash, returns RustCompiler type
- Token estimate, raw metrics

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed warning fixture to trigger RustHuman parser correctly**
- **Found during:** Task 1 implementation
- **Issue:** `warning: unused variable` format without `command_hint` falls to Generic parser in glass_errors (not RustHuman). The detect module requires `warning[` bracket format OR a cargo command hint for RustHuman. Tests using warning fixtures needed either a `warning[code]:` format with bracket OR a `command_hint` argument.
- **Fix:** Changed warning test fixtures to use `command_hint = Some("cargo build")` to force RustHuman detection. This is correct behavior since in production cargo_build::parse is always called with a command hint from the classifier.
- **Files modified:** `crates/glass_soi/src/cargo_build.rs`
- **Commit:** 89f7a2a

**2. [Rule 1 - Bug] Fixed duration extraction: duration appears on summary line, not separate line**
- **Found during:** Task 2 TDD cycle (test `all_passing_has_duration_in_summary` failed)
- **Issue:** Real cargo test output places duration on the same line as the summary: `test result: ok. 3 passed; 0 failed; 0 ignored; finished in 0.02s`. Initial implementation parsed summary then `continue`d, missing the duration.
- **Fix:** Added duration extraction within the summary regex match block (in addition to a standalone duration line handler for robustness).
- **Files modified:** `crates/glass_soi/src/cargo_test.rs`
- **Commit:** 7b0c8fe

**3. [Rule 2 - Clippy] Fixed 3 clippy warnings in cargo_test.rs**
- **Found during:** Task 2 clippy check
- **Issues:** `redundant_guards` (two `s if s == "ok"` guards → plain string literals); `collapsible_match` (nested if-let on Option + enum)
- **Fix:** Applied clippy suggestions directly.
- **Files modified:** `crates/glass_soi/src/cargo_test.rs`
- **Commit:** 7b0c8fe

## Out-of-Scope Items (Deferred)

5 pre-existing test failures in `jest.rs` were present before this plan's changes. These are stub tests for the jest parser (plan 48-03). Logged to `deferred-items.md`.

## Self-Check

- [x] `crates/glass_soi/src/cargo_build.rs` implements `pub fn parse(output, command_hint) -> ParsedOutput`
- [x] `crates/glass_soi/src/cargo_test.rs` implements `pub fn parse(output) -> ParsedOutput`
- [x] All 10 cargo_build tests pass
- [x] All 13 cargo_test tests pass (14 with classifier::hint_cargo_test_is_rust_test)
- [x] `cargo clippy -p glass_soi -- -D warnings` clean
- [x] Commit 89f7a2a (cargo_build) exists
- [x] Commit 7b0c8fe (cargo_test) exists

## Self-Check: PASSED

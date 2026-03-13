---
phase: 48-soi-classifier-and-parser-crate
plan: "01"
subsystem: glass_soi
tags: [soi, classifier, ansi, parser, types, new-crate]
dependency_graph:
  requires: [glass_errors]
  provides: [glass_soi crate, OutputType, classify, strip_ansi, parse, ParsedOutput]
  affects: []
tech_stack:
  added: [glass_soi crate, regex = "1"]
  patterns: [OnceLock<Regex> for compiled regexes, two-stage command-hint + content-sniff classification, freeform fallback parse]
key_files:
  created:
    - crates/glass_soi/Cargo.toml
    - crates/glass_soi/src/types.rs
    - crates/glass_soi/src/ansi.rs
    - crates/glass_soi/src/classifier.rs
    - crates/glass_soi/src/lib.rs
    - crates/glass_soi/src/cargo_build.rs
    - crates/glass_soi/src/cargo_test.rs
    - crates/glass_soi/src/npm.rs
    - crates/glass_soi/src/pytest.rs
    - crates/glass_soi/src/jest.rs
  modified: []
decisions:
  - "SOI Severity enum (Error/Warning/Info/Success) intentionally differs from glass_errors::Severity (Error/Warning/Note/Help) -- SOI uses outcome-oriented scale"
  - "OutputRecord is an enum (not a trait object) for zero-cost dispatch and easy serde serialization"
  - "classify() is a pure function (not a struct method) matching glass_errors::extract_errors pattern"
  - "Stub parsers return freeform_parse fallback with correct OutputType set -- plans 02/03 implement real parsing without changing function signatures"
  - "All future OutputType variants (Git, Docker, Kubectl, TypeScript, Go) wired in classify_by_hint now so Phase 54 only adds parser implementations"
metrics:
  duration_seconds: 177
  completed_date: "2026-03-13"
  tasks_completed: 2
  files_created: 10
  files_modified: 0
  tests_added: 45
requirements-completed: [SOIP-01]
---

# Phase 48 Plan 01: glass_soi Crate Scaffold Summary

**One-liner:** New glass_soi crate with 21-variant OutputType taxonomy, ANSI stripper using OnceLock<Regex>, and two-stage command-hint + content-sniff classifier returning correct types for cargo, npm, pytest, jest with FreeformText fallback.

## What Was Built

### glass_soi crate

A new workspace crate at `crates/glass_soi/` that establishes the foundational types and classifier for the entire SOI pipeline. All subsequent plans (48-02, 48-03) build on this without changing any public API signatures.

### Types (types.rs)

- `OutputType` enum: 21 variants covering compilers, test runners, package managers, DevOps tools, structured data formats, and FreeformText fallback
- `Severity` enum: Error/Warning/Info/Success (distinct from glass_errors::Severity which uses Note/Help)
- `TestStatus` enum: Passed/Failed/Skipped/Ignored
- `OutputSummary` struct: one_line, token_estimate, severity
- `OutputRecord` enum: 8 variants (CompilerError, TestResult, TestSummary, PackageEvent, GitEvent, DockerEvent, GenericDiagnostic, FreeformChunk)
- `ParsedOutput` struct: output_type, summary, records, raw_line_count, raw_byte_count
- All types derive Debug, Clone, Serialize; enums additionally derive PartialEq

### ANSI Stripper (ansi.rs)

`strip_ansi(s: &str) -> String` using a single `OnceLock<Regex>` compiled once at first call. Handles:
- CSI sequences (`ESC [ ... <letter>`) — colors, cursor movement, etc.
- OSC sequences (`ESC ] ... BEL`) — window title, hyperlinks
- Character set designation (`ESC ( B`)

### Classifier (classifier.rs)

`classify(output: &str, command_hint: Option<&str>) -> OutputType` with two stages:

**Stage 1 — command hint matching** (fast, deterministic):
- Rust/cargo: `cargo build/check/clippy` → RustCompiler, `cargo test` → RustTest, other `cargo` → Cargo
- npm/npx: `npx jest` → Jest (higher priority), other `npm/npx` → Npm
- pytest: `pytest`, `python -m pytest` → Pytest
- Future (Phase 54): git, docker, kubectl, tsc, go patterns also wired

**Stage 2 — content sniffing** (regex-based, activates when no hint matches):
- `"reason":"compiler-message"` or `"$message_type":"diagnostic"` → RustCompiler
- `running N tests` or `test result:` → RustTest
- `added N packages` → Npm
- `PASSED/FAILED` + `::` → Pytest (conservative, avoids false positives)
- `PASS/FAIL <file.ts/js>` → Jest

Fallback: FreeformText

### Public API (lib.rs)

```rust
pub fn classify(output: &str, command_hint: Option<&str>) -> OutputType
pub fn parse(output: &str, output_type: OutputType, command_hint: Option<&str>) -> ParsedOutput
pub fn strip_ansi(s: &str) -> String
// All types re-exported: OutputType, OutputRecord, ParsedOutput, OutputSummary, Severity, TestStatus
```

`parse()` dispatches to per-type parsers; all are stubs returning `freeform_parse()` output with the correct `OutputType` set. Plans 48-02 and 48-03 implement the real parsers without API changes.

## Test Coverage

45 tests across 4 modules:
- `ansi`: 7 tests (colored text, OSC, charset, clean passthrough, empty, bold, cursor movement)
- `classifier`: 29 tests covering every command hint variant and every content sniff path, plus false-positive guards
- `types`: 4 tests (equality, struct construction)
- `lib`: 4 tests (dispatch, freeform fallback, git fallback, line counting) + 1 doc test

## Deviations from Plan

None — plan executed exactly as written. Task 1 (scaffold + types) and Task 2 (ANSI stripper + classifier) were developed in a single pass since they share no circular dependencies and the TDD behavior spec was fully captured in the implementation.

## Self-Check

- [x] `crates/glass_soi/Cargo.toml` exists
- [x] `crates/glass_soi/src/types.rs` exports OutputType, Severity, TestStatus, OutputRecord, ParsedOutput, OutputSummary
- [x] `crates/glass_soi/src/ansi.rs` exports strip_ansi
- [x] `crates/glass_soi/src/classifier.rs` exports classify
- [x] `crates/glass_soi/src/lib.rs` re-exports all public API
- [x] `cargo check -p glass_soi` clean
- [x] `cargo clippy -p glass_soi -- -D warnings` clean
- [x] `cargo test -p glass_soi` 45 tests pass (0 failed)
- [x] Commit 0a0a69c exists

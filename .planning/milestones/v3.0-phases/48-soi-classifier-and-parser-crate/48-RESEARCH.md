# Phase 48: SOI Classifier and Parser Crate - Research

**Researched:** 2026-03-12
**Domain:** Rust output parsing, regex-based text classification, new workspace crate creation
**Confidence:** HIGH

## Summary

Phase 48 creates `glass_soi` — a new pure-logic Rust crate (no async, no SQLite, no rendering) that
classifies command output into typed categories and extracts machine-readable records. The crate is
consumed by later phases (49 for storage, 50 for pipeline wiring) so its public API is the primary
deliverable.

The project already has a `glass_errors` crate (4 source files) that implements the Rust compiler
parser pattern this phase extends. The SOI classifier and all parsers should follow the same
architectural pattern: command-hint + content-sniffing detection feeding a dispatch table of
format-specific parsers, with graceful fallback to `Freeform` for unrecognized output. No regex
engine beyond the already-used `regex = "1"` crate is needed — all existing parsers in
`glass_errors` use `regex` and `serde_json`.

The full type design for `ParsedOutput`, `OutputRecord`, `OutputType`, `Severity`, and
`TestStatus` is pre-specified in `SOI_AND_AGENT_MODE.md` and should be treated as the authoritative
schema — diverge only where implementation reveals a concrete problem.

**Primary recommendation:** Create `crates/glass_soi/` mirroring the `glass_errors` structure
(lib.rs + types.rs + classifier.rs + one file per parser), wire in `glass_errors` as a dependency
or copy its parsers in, and deliver the full public API without SQLite or async.

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SOIP-01 | SOI classifier detects output type from command text and content patterns (Rust compiler, test runner, package manager, git, docker, kubectl, structured data, freeform) | Classifier pattern from `glass_errors::detect`, command-hint + content-sniff approach proven in codebase |
| SOIP-02 | Rust/cargo compiler error parser extracts file, line, column, severity, error code, message from cargo build/clippy output | `glass_errors` already implements this (rust_json.rs + rust_human.rs); promote/wrap rather than rewrite |
| SOIP-03 | Rust/cargo test parser extracts test name, status (passed/failed/ignored), duration, failure message from cargo test output | New parser; `cargo test` emits predictable text format with `test X ... ok/FAILED` lines |
| SOIP-04 | npm/Node parser extracts package events (added, removed, audited, vulnerabilities) from npm install/update output | New parser; npm output lines follow `added N packages` / `N vulnerabilities found` pattern |
| SOIP-05 | pytest parser extracts test name, status, duration, failure message from pytest output | New parser; pytest emits `PASSED`/`FAILED`/`ERROR` per test line plus `=== N failed, M passed ===` summary |
| SOIP-06 | jest parser extracts test suite results, individual test status, failure diffs from jest output | New parser; jest emits `PASS`/`FAIL` per suite and `✓`/`✕` per test — must strip ANSI before matching |
</phase_requirements>

---

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `regex` | 1 | Pattern matching in output text | Already in workspace via `glass_errors`; Rust standard for structured regex |
| `serde` | 1.0.228 | Serialize/deserialize OutputRecord to JSON | Workspace dep; needed for Phase 49 DB storage |
| `serde_json` | 1.0 | JSON parsing for cargo's `--message-format=json` output | Already used in `glass_errors::rust_json` |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `anyhow` | 1.0.102 | Error propagation in parsers | Workspace dep; use for parse errors that bubble up |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `regex` crate | `nom` parser combinators | nom is overkill for line-oriented output; regex is already proven in codebase |
| `regex` crate | `fancy-regex` | Only needed for lookahead/lookbehind; none required here |
| Hand-written ANSI stripper | `strip-ansi-escapes` crate | Extra dep; single-pass byte scanner is ~15 lines and sufficient for this use |

**Installation:**
```bash
# No new workspace deps needed — regex, serde, serde_json already in workspace
# glass_soi Cargo.toml:
# regex = "1"
# serde = { workspace = true }
# serde_json = "1.0"
# anyhow = { workspace = true }
```

---

## Architecture Patterns

### Recommended Project Structure
```
crates/glass_soi/
├── Cargo.toml
└── src/
    ├── lib.rs           # pub use everything; crate-level doc
    ├── types.rs         # OutputType, OutputRecord, ParsedOutput, Severity, TestStatus
    ├── classifier.rs    # OutputClassifier: detect() -> OutputType
    ├── ansi.rs          # strip_ansi(s: &str) -> String (jest/npm use ANSI colors)
    ├── cargo_build.rs   # cargo build/check/clippy parser (wraps glass_errors logic)
    ├── cargo_test.rs    # cargo test parser
    ├── npm.rs           # npm install/update/audit parser
    ├── pytest.rs        # pytest parser
    └── jest.rs          # jest parser
```

### Pattern 1: Two-Stage Classify-Then-Parse
**What:** Classifier runs first (cheap, command-hint + content sniff), then the matched parser
receives the full output string and produces `ParsedOutput`.
**When to use:** This is THE pattern for all SOI parsers.
**Example:**
```rust
// Source: glass_errors/src/detect.rs (existing codebase pattern)
pub fn classify(output: &str, command_hint: Option<&str>) -> OutputType {
    if let Some(cmd) = command_hint {
        let cmd_lower = cmd.to_ascii_lowercase();
        if cmd_lower.contains("cargo build") || cmd_lower.contains("cargo check")
            || cmd_lower.contains("cargo clippy")
        {
            return OutputType::RustCompiler;
        }
        if cmd_lower.contains("cargo test") {
            return OutputType::RustTest;
        }
        if cmd_lower.starts_with("npm ") || cmd_lower.starts_with("npx ") {
            return OutputType::Npm;
        }
        if cmd_lower.starts_with("pytest") || cmd_lower.contains("python -m pytest") {
            return OutputType::Pytest;
        }
        if cmd_lower.starts_with("jest") || cmd_lower.contains("npx jest") {
            return OutputType::Jest;
        }
    }
    // Content sniff fallback
    if output.contains(r#""reason":"compiler-message""#) {
        return OutputType::RustCompiler;
    }
    OutputType::Freeform
}
```

### Pattern 2: Registry Dispatch
**What:** `parse(output: &str, output_type: OutputType) -> ParsedOutput` dispatches to per-type parser.
**When to use:** After classification; keeps each parser independent and testable in isolation.
**Example:**
```rust
// Source: glass_errors/src/lib.rs pattern
pub fn parse(output: &str, output_type: OutputType, command_hint: Option<&str>) -> ParsedOutput {
    match output_type {
        OutputType::RustCompiler => cargo_build::parse(output, command_hint),
        OutputType::RustTest     => cargo_test::parse(output),
        OutputType::Npm          => npm::parse(output),
        OutputType::Pytest       => pytest::parse(output),
        OutputType::Jest         => jest::parse(output),
        OutputType::Freeform     => freeform_parse(output),
    }
}
```

### Pattern 3: ANSI Strip Before Parsing
**What:** Strip ANSI escape sequences from output before applying regex patterns.
**When to use:** Always for jest output; optionally for npm. `cargo test` and `pytest` are usually
clean but defensive stripping is safe.
**Example:**
```rust
// Inline implementation — no external crate needed
pub fn strip_ansi(s: &str) -> String {
    // Matches ESC[ ... m sequences and OSC sequences
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"\x1b\[[0-9;]*[mABCDEFGHJKSTfnsulh]|\x1b\][^\x07]*\x07").unwrap()
    });
    re.replace_all(s, "").into_owned()
}
```

### Pattern 4: Test-Fixture-Driven Parser Tests
**What:** Capture real output from tools as string literals in `#[cfg(test)]` blocks and assert on
extracted record counts, field values.
**When to use:** Every parser MUST have fixture tests covering: empty output, success run, failure
run, partial output (truncated), and mixed output (warnings + errors).
**Example:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    const CARGO_TEST_FAILED: &str = r#"
running 3 tests
test foo::bar::test_one ... ok
test foo::bar::test_two ... FAILED
test foo::bar::test_three ... ignored

failures:

---- foo::bar::test_two stdout ----
thread 'foo::bar::test_two' panicked at 'assertion failed', src/foo.rs:42:5

test result: FAILED. 1 failed; 1 passed; 1 ignored; 0 measured
"#;

    #[test]
    fn cargo_test_parses_results() {
        let result = parse(CARGO_TEST_FAILED);
        assert_eq!(result.records.iter().filter(|r| matches!(r, OutputRecord::TestResult { status: TestStatus::Failed, .. })).count(), 1);
    }
}
```

### Anti-Patterns to Avoid
- **Parsing in the classifier**: The classify step should be cheap (O(lines) scan, no record extraction). Keep classification separate from parsing.
- **Panicking on malformed output**: All parsers must be infallible — use `Option`/`Result` internally but always return a valid `ParsedOutput` with at least `OutputType::Freeform` records.
- **Global mutable regex state**: Use `std::sync::OnceLock<Regex>` per static pattern (already the pattern in `glass_errors`).
- **Depending on glass_history**: This crate is pure parsing logic — NO SQLite, NO async, NO DB access. Storage is Phase 49's job.
- **Depending on glass_core**: Avoid creating a dependency on glass_core unless you need AppEvent or Config types. In Phase 48, you don't.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Rust compiler output parsing | Custom regex from scratch | Promote `glass_errors` rust_json + rust_human parsers | Already implemented, tested, handles both JSON and human format |
| ANSI escape stripping | Custom byte scanner | ~15-line `strip_ansi()` using existing `regex` crate | Trivial with regex already in dep tree; no new dep needed |
| JSON parsing for cargo `--message-format=json` | Manual string splitting | `serde_json::from_str` on each line | cargo JSON is structured; serde_json already in workspace |

**Key insight:** The `glass_errors` crate already solves SOIP-02 (Rust compiler extraction). The
glass_soi crate should depend on or directly copy `glass_errors` logic rather than reimplementing
it. The design document in `SOI_AND_AGENT_MODE.md` explicitly notes: "glass_errors becomes one
parser within SOI's parser registry."

---

## Common Pitfalls

### Pitfall 1: cargo test output intermixes compilation and test output
**What goes wrong:** `cargo test` first compiles, then runs tests. Compilation output (including
errors) can appear before `running N tests`. If compilation fails, there are no test lines at all.
**Why it happens:** cargo runs `rustc` then the test binary; both write to stdout.
**How to avoid:** Check for `running N tests` line as the entry point. If absent, fall back to
treating the output as a compilation failure (route through RustCompiler parser or produce a
TestSummary with 0 results and an error record).
**Warning signs:** Parser returns empty record list on a `cargo test` that failed to compile.

### Pitfall 2: jest uses ANSI colors and Unicode symbols
**What goes wrong:** jest emits `✓`, `✕`, and ANSI color codes. Naive `contains("PASS")` matches
colored output incorrectly; regex on raw bytes fails.
**Why it happens:** jest targets interactive terminals; it emits `\x1b[32m✓\x1b[0m` patterns.
**How to avoid:** Strip ANSI before classification AND before parsing. Use Unicode-aware regex
patterns (Rust regex crate handles Unicode by default).
**Warning signs:** Jest test count is 0 or NaN on a real jest run.

### Pitfall 3: npm output varies significantly by version
**What goes wrong:** npm 6, 7, 8, 9, 10 each have different output formats. `added N packages`
exists in npm 7+; older versions say `added N packages from M contributors`.
**Why it happens:** npm changed its output format with major versions.
**How to avoid:** Write permissive regexes that extract the numeric count regardless of surrounding
text: `r"(\d+) packages?"` rather than matching exact phrases. Use `(?i)` flag for case-insensitive
matching.
**Warning signs:** Package count is 0 when user runs `npm install`.

### Pitfall 4: pytest output format depends on installed plugins
**What goes wrong:** pytest-xdist (parallel), pytest-timeout, and other plugins modify output format.
The `PASSED`/`FAILED` prefix can appear in different positions.
**Why it happens:** pytest's output is designed for extensibility; plugins intercept and modify it.
**How to avoid:** Anchor on the per-test line suffix pattern `::test_name PASSED` / `::test_name FAILED`
rather than line prefixes. The summary line `=== N failed, M passed in X.XXs ===` is stable across
plugins.
**Warning signs:** Test count differs from actual test run count.

### Pitfall 5: Freeform fallback must produce no false positive records
**What goes wrong:** The classifier falls through to `Freeform` for unknown output types, but
residual regex patterns from other parsers accidentally match freeform text.
**Why it happens:** If classify() uses content sniffing with broad patterns, it may misclassify.
**How to avoid:** Freeform parser must produce ONLY `OutputRecord::FreeformChunk` records — no
`CompilerError`, `TestResult`, or `PackageEvent` records. Strict type safety enforced by the match arm.
**Warning signs:** Success criterion 5 of Phase 48 fails (false positive records in freeform output).

### Pitfall 6: Large output (>50KB) causes regex backtracking
**What goes wrong:** Certain regex patterns exhibit exponential backtracking on adversarial input or
very long lines.
**Why it happens:** Rust's `regex` crate uses a finite automata engine that guarantees O(n) time
for most patterns, but complex nested groups can still be slow.
**How to avoid:** Process output line-by-line rather than applying multi-line regex to the full
string. Set a max-line-length guard (e.g., skip lines > 4096 bytes for pattern matching).
**Warning signs:** Parsing hangs or takes >100ms on a 50KB test output.

---

## Code Examples

Verified patterns from codebase:

### cargo test output format (stable since Rust 1.0)
```
running 5 tests
test module::test_foo ... ok
test module::test_bar ... FAILED
test module::test_baz ... ignored

failures:

---- module::test_bar stdout ----
thread 'module::test_bar' panicked at 'assertion `left == right` failed
  left: 1
 right: 2', src/lib.rs:42:5
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

test result: FAILED. 1 failed; 3 passed; 1 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

Key regexes:
```rust
// Per-test line
static TEST_LINE: &str = r"^test (.+) \.\.\. (ok|FAILED|ignored)$";
// Summary line
static TEST_SUMMARY: &str = r"test result: (?:ok|FAILED)\. (\d+) failed; (\d+) passed; (\d+) ignored";
// Duration (optional, Rust 1.73+)
static TEST_DURATION: &str = r"finished in (\d+\.\d+)s";
```

### cargo test failure block parsing
```rust
// Failure messages appear in the "failures:" block after a separator:
// ---- module::test_name stdout ----
// (failure output lines)
static FAILURE_HEADER: &str = r"^---- (.+) stdout ----$";
```

### npm install output (npm 7+)
```
npm warn deprecated X: message

added 142 packages, and audited 143 packages in 3s

14 packages are looking for funding
  run `npm fund` for details

3 vulnerabilities (1 moderate, 2 high)
```

Key regexes:
```rust
static NPM_ADDED: &str = r"(?i)added (\d+) packages?";
static NPM_REMOVED: &str = r"(?i)removed (\d+) packages?";
static NPM_AUDITED: &str = r"(?i)audited (\d+) packages?";
static NPM_VULNS: &str = r"(\d+) vulnerabilit(?:y|ies)";
static NPM_VULN_DETAIL: &str = r"(\d+) (critical|high|moderate|low)";
```

### pytest output format
```
collected 5 items

tests/test_auth.py::test_login PASSED                     [100%]
tests/test_auth.py::test_logout FAILED                    [ 80%]

FAILURES
============================================================
test_auth.py::test_logout
auth.py:42: AssertionError: assert False
============================================================
short test summary info
FAILED tests/test_auth.py::test_logout - assert False
============================================================
5 passed, 1 failed in 0.42s
```

Key regexes:
```rust
// Per-test line: "path::test_name STATUS"
static PYTEST_RESULT: &str = r"^(.+::[\w\[\]]+)\s+(PASSED|FAILED|ERROR|SKIPPED|XFAIL|XPASS)";
// Summary line
static PYTEST_SUMMARY: &str = r"(\d+) passed(?:, (\d+) failed)?(?:, (\d+) error)?.*in ([\d.]+)s";
// Duration from per-test line (pytest-duration-insights plugin)
static PYTEST_DURATION: &str = r"\[([\d.]+)s\]";
```

### Rust compiler JSON message format (cargo --message-format=json)
```json
{"reason":"compiler-message","package_id":"...","manifest_path":"...","message":{"message":"mismatched types","code":{"code":"E0308"},"level":"error","spans":[{"file_name":"src/main.rs","line_start":10,"column_start":5}]}}
```

This is already parsed by `glass_errors::rust_json`. The glass_soi cargo_build parser can delegate:
```rust
// Source: glass_errors/src/lib.rs pattern
pub fn parse(output: &str, command_hint: Option<&str>) -> ParsedOutput {
    let errors = glass_errors::extract_errors(output, command_hint);
    // Convert StructuredError -> OutputRecord::CompilerError
    let records = errors.into_iter().map(|e| OutputRecord::CompilerError {
        file: e.file,
        line: e.line,
        column: e.column,
        severity: map_severity(e.severity),
        code: e.code,
        message: e.message,
        context_lines: None,
    }).collect();
    // ...
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| cargo human-readable errors only | cargo `--message-format=json` available | Rust ~1.24 (2018) | JSON is machine-readable; prefer for classification when available |
| pytest output only in human format | pytest-json-report plugin for JSON | ~2019 | Plugin not universally installed; parse human output as primary path |
| jest text output | jest `--json` flag for machine-readable | jest 20+ | `--json` requires Glass to modify how commands are invoked; parse human output instead |

**Deprecated/outdated:**
- cargo `--message-format=human` as the primary parse path: JSON is now available and more reliable for error extraction, but human format remains the fallback when JSON markers are absent.

---

## Open Questions

1. **Should glass_soi depend on glass_errors or copy its parsers?**
   - What we know: `glass_errors` is a workspace crate with clean public API; adding it as a dep is one line
   - What's unclear: Whether to keep `glass_errors` alive for backward compatibility (MCP tools use it) or fold it entirely into `glass_soi`
   - Recommendation: Depend on `glass_errors` from `glass_soi` in Phase 48 (avoids duplication); folding can happen later as a cleanup

2. **OutputRecord enum vs trait object for parser extensibility**
   - What we know: `SOI_AND_AGENT_MODE.md` defines a flat `OutputRecord` enum; Phase 54 adds 6 more parser types
   - What's unclear: Whether to use an open enum (requires adding variants later, non-additive change) or a trait-based approach
   - Recommendation: Use the flat enum as specified; Phase 54 will extend it. Enums are the idiomatic Rust approach and match existing codebase style.

3. **jest ANSI output on Windows**
   - What we know: jest emits ANSI on TTY; ConPTY on Windows may or may not strip ANSI before Glass captures output
   - What's unclear: Whether Glass receives ANSI codes in captured output on Windows
   - Recommendation: Always strip ANSI defensively. The ANSI stripper is cheap and harmless on clean text.

---

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` via `cargo test` |
| Config file | none (workspace-level `cargo test --workspace`) |
| Quick run command | `cargo test -p glass_soi` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SOIP-01 | Classifier returns correct OutputType for each tool's output | unit | `cargo test -p glass_soi classifier` | Wave 0 |
| SOIP-02 | cargo build errors produce CompilerError records with file/line/col/severity/code/message | unit | `cargo test -p glass_soi cargo_build` | Wave 0 |
| SOIP-03 | cargo test output produces TestResult records with name/status/duration/failure_message | unit | `cargo test -p glass_soi cargo_test` | Wave 0 |
| SOIP-04 | npm install output produces PackageEvent records with added/removed/audited/vulns | unit | `cargo test -p glass_soi npm` | Wave 0 |
| SOIP-05 | pytest output produces TestResult records with name/status/duration/failure details | unit | `cargo test -p glass_soi pytest` | Wave 0 |
| SOIP-06 | jest output produces TestResult records with suite/test/failure diffs | unit | `cargo test -p glass_soi jest` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_soi`
- **Per wave merge:** `cargo test --workspace && cargo clippy --workspace -- -D warnings`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_soi/` — entire crate does not exist yet; Wave 0 creates scaffold with all test stubs
- [ ] `crates/glass_soi/src/lib.rs` — public API declaration
- [ ] `crates/glass_soi/src/types.rs` — OutputType, OutputRecord, ParsedOutput, Severity, TestStatus
- [ ] `crates/glass_soi/src/classifier.rs` — OutputClassifier with test fixtures per tool
- [ ] `crates/glass_soi/src/cargo_build.rs` — Rust compiler parser with SOIP-02 tests
- [ ] `crates/glass_soi/src/cargo_test.rs` — cargo test parser with SOIP-03 tests
- [ ] `crates/glass_soi/src/npm.rs` — npm parser with SOIP-04 tests
- [ ] `crates/glass_soi/src/pytest.rs` — pytest parser with SOIP-05 tests
- [ ] `crates/glass_soi/src/jest.rs` — jest parser with SOIP-06 tests
- [ ] `Cargo.toml` workspace members update — add `crates/glass_soi` to `members`

---

## Sources

### Primary (HIGH confidence)
- `crates/glass_errors/` — existing Rust output parser; direct template for cargo_build parser design
- `crates/glass_pipes/src/` — model for simple no-async new crate structure
- `SOI_AND_AGENT_MODE.md` — authoritative type specifications (OutputType, OutputRecord, ParsedOutput enums)
- `Cargo.toml` (workspace root) — dependency versions already in workspace

### Secondary (MEDIUM confidence)
- `.planning/REQUIREMENTS.md` — SOIP-01 through SOIP-06 requirement text (parsed for exact field specs)
- `.planning/ROADMAP.md` — Phase 48 success criteria (5 criteria for phase gate)
- `crates/glass_errors/src/detect.rs` — exact pattern for two-stage classify-then-parse

### Tertiary (LOW confidence)
- Knowledge of npm 7+/8+/9+/10+ output format differences — verify with fixture captures on actual runs

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — regex + serde_json already proven in codebase; no new deps required
- Architecture: HIGH — directly modeled on existing `glass_errors` crate in same repo
- Pitfalls: MEDIUM — cargo/pytest/npm pitfalls verified by output format documentation; jest ANSI behavior on Windows ConPTY is LOW confidence

**Research date:** 2026-03-12
**Valid until:** 2026-04-12 (stable domain; cargo/npm/pytest/jest output formats change rarely)

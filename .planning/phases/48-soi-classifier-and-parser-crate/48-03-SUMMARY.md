---
phase: 48-soi-classifier-and-parser-crate
plan: "03"
subsystem: glass_soi
tags: [soi, npm, pytest, jest, parser, regex, ansi, package-events, test-results]

dependency_graph:
  requires:
    - phase: 48-01
      provides: glass_soi crate scaffold with OutputType, OutputRecord, ParsedOutput types, OnceLock<Regex> pattern, freeform_parse fallback
  provides:
    - npm install/update output parser producing PackageEvent records (added/removed/audited/vulnerabilities/deprecated)
    - pytest output parser producing TestResult + TestSummary records with failure message extraction
    - jest output parser with ANSI stripping producing TestResult + TestSummary records with failure diff extraction
  affects: [48-04, phase-49-soi-storage, phase-50-soi-wiring, phase-53-mcp-tools]

tech-stack:
  added: []
  patterns:
    - "Multi-match per line: when a single line can match multiple patterns (npm 'added N and audited M'), do NOT use continue after first match"
    - "concat!() macro for test fixture strings with embedded whitespace to avoid Rust line-continuation stripping leading spaces"
    - "Failure message accumulation: collect lines into a buffer, flush when next test/suite line is seen"
    - "OnceLock<Regex> per parser function with descriptive .expect() messages"
    - "ANSI strip as first step in jest::parse() before any line-by-line processing"

key-files:
  created: []
  modified:
    - crates/glass_soi/src/npm.rs
    - crates/glass_soi/src/pytest.rs
    - crates/glass_soi/src/jest.rs

key-decisions:
  - "Multi-pattern lines in npm output: do NOT use `continue` after matching 'added N' -- same line contains 'audited M' in npm 7+ format"
  - "Test fixture strings use concat!() macro instead of Rust line-continuation backslash to preserve leading whitespace (indentation is significant for jest regex matching)"
  - "jest failure diff collected by buffering indented lines after failing test until next PASS/FAIL/suite/summary line is seen"
  - "pytest XPASS maps to TestStatus::Passed, XFAIL and SKIPPED both map to TestStatus::Skipped"
  - "npm vulnerability severity: Error for critical/high, Warning for moderate/low, Success when no vuln pattern matched"

requirements-completed: [SOIP-04, SOIP-05, SOIP-06]

duration: 7min
completed: "2026-03-12"
---

# Phase 48 Plan 03: npm, pytest, and jest Parsers Summary

**Three production-ready output parsers for JavaScript/Python toolchains: npm PackageEvent extraction with per-severity vulnerability breakdown, pytest per-test TestResult with failure message extraction, and jest ANSI-stripped TestResult parser with failure diff capture.**

## Performance

- **Duration:** 7 min
- **Started:** 2026-03-12T~05:24Z
- **Completed:** 2026-03-12T~05:31Z
- **Tasks:** 2 completed
- **Files modified:** 3

## Accomplishments

- npm parser extracts added/removed/audited counts, vulnerability breakdown (critical/high/moderate/low), and deprecated warnings; handles npm 6-10+ format variations with permissive regexes; severity correctly maps to Error/Warning/Success
- pytest parser extracts per-test TestResult records for all status variants (PASSED/FAILED/ERROR/SKIPPED/XFAIL/XPASS), TestSummary with counts and duration in ms, and failure messages from short summary info lines
- jest parser strips ANSI before parsing, extracts per-test TestResult with suite-prefixed names and duration, captures failure diff blocks, and produces TestSummary with total time
- All three parsers return FreeformChunk fallback for empty or unrecognized input without panicking
- 103 total tests in glass_soi (45 from plan 01 + 58 new), full workspace green

## Task Commits

1. **Task 1: npm parser** - `826d1bd` (feat)
2. **Task 2: pytest + jest parsers** - `10d6c8a` (feat)

## Files Created/Modified

- `crates/glass_soi/src/npm.rs` - Full npm install/update/audit output parser replacing stub; 13 tests
- `crates/glass_soi/src/pytest.rs` - Full pytest output parser replacing stub; 11 tests
- `crates/glass_soi/src/jest.rs` - Full jest output parser with ANSI stripping replacing stub; 12 tests

## Decisions Made

- Multi-pattern lines in npm output: after matching "added N packages" on a line, do NOT use `continue` — the same npm 7+ line also contains "audited M packages". Matching both requires falling through to all pattern checks.
- Test fixture strings use `concat!()` macro instead of Rust multiline string with line-continuation backslash (`\n\` at end of source line). The backslash continuation strips leading whitespace on the next source line, which would destroy the 2-space indentation that jest's `^\s+[✓✔]` regex requires.
- jest failure diff collection: buffer indented/non-empty lines after a failing test until the next PASS/FAIL suite header, test result line, or summary line is encountered, then attach to the test's `failure_message`.
- npm vulnerability severity follows the plan exactly: `has_critical_high` → Error, `has_moderate_low` → Warning, otherwise Success.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed npm multi-match-per-line logic**
- **Found during:** Task 1 (npm parser)
- **Issue:** Initial implementation used `continue` after matching "added N packages", causing audited count (on same line) to be silently skipped. 3 tests failed.
- **Fix:** Removed `continue` after added/removed/audited matches; all three checks run on every line. Only deprecated and vulnerability lines use `continue` since they cannot co-appear with added/audited on the same line.
- **Files modified:** crates/glass_soi/src/npm.rs
- **Verification:** cargo test -p glass_soi npm -- all 15 tests green
- **Committed in:** 826d1bd (Task 1 commit)

**2. [Rule 1 - Bug] Fixed jest test fixture whitespace stripping**
- **Found during:** Task 2 (jest parser)
- **Issue:** Jest test line regex requires `^\s+[✓✔]` (leading spaces). Rust's string line-continuation syntax (`\n\` at end of line) strips leading whitespace from the continuation line, producing `✓ test name` without indentation. 5 tests failed.
- **Fix:** Rewrote all jest test fixture constants to use `concat!()` macro with one string per line, preserving exact content including leading spaces.
- **Files modified:** crates/glass_soi/src/jest.rs
- **Verification:** cargo test -p glass_soi jest -- all 16 tests green
- **Committed in:** 10d6c8a (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (2 Rule 1 bugs)
**Impact on plan:** Both fixes were correctness issues discovered during TDD red→green cycle. No scope change.

## Issues Encountered

- None beyond the two auto-fixed bugs documented above.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- SOIP-04, SOIP-05, SOIP-06 complete; all three non-Rust parser categories operational
- glass_soi now has full parsing coverage for cargo (plan 02) + npm/pytest/jest (plan 03)
- Phase 49 (SOI storage) can begin immediately — ParsedOutput is Serialize-ready and all OutputRecord variants are populated correctly
- No blockers

---
*Phase: 48-soi-classifier-and-parser-crate*
*Completed: 2026-03-12*

---
phase: 53-soi-mcp-tools
plan: 01
subsystem: api
tags: [rust, mcp, sqlite, soi, compression, rusqlite, rmcp]

# Dependency graph
requires:
  - phase: 51-soi-compress
    provides: compress_output(), diff_compress(), TokenBudget, CompressedOutput, DiffSummary types
  - phase: 49-soi-db
    provides: OutputRecordRow, CommandOutputSummaryRow, get_output_records(), get_previous_run_records()
  - phase: 52-soi-display
    provides: SOI pipeline fully wired end-to-end
provides:
  - glass_query MCP tool: compressed SOI output for any command_id at requested token budget
  - glass_query_trend MCP tool: per-run summaries and diffs for last N runs of a command pattern
  - glass_query_drill MCP tool: full record detail by record_id from output_records table
  - get_last_n_run_ids() HistoryDb helper with LIKE pattern support and oldest-first ordering
affects: [phase-56-agent, phase-58-agent-toolchain, any phase consuming MCP tools]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - spawn_blocking pattern for DB access in MCP tools (pre-existing, extended)
    - JSON data column inspection for regression detection (TestResult status in data, not severity column)
    - LIKE pattern forwarding from MCP params to SQL for flexible command matching

key-files:
  created: []
  modified:
    - crates/glass_history/src/db.rs
    - crates/glass_mcp/src/tools.rs

key-decisions:
  - "TestResult regression detection inspects JSON data column for status=Failed, not DB severity column (severity is always None for TestResult records)"
  - "glass_query_trend uses has_regression = failed_test_in_curr && !failed_test_in_prev to detect new failures across consecutive runs"
  - "parse_budget() maps None/unknown to Summary as safe default budget"
  - "glass_query_drill uses inline SQL with .optional() (not a HistoryDb method) since it is a one-off lookup not worth adding to the public DB API"

patterns-established:
  - "MCP tool regression detection: parse JSON data column directly when severity is not stored in DB"
  - "get_last_n_run_ids: DESC LIMIT then reverse for oldest-first -- avoids subquery complexity"

requirements-completed: [SOIM-01, SOIM-02, SOIM-03]

# Metrics
duration: 25min
completed: 2026-03-13
---

# Phase 53 Plan 01: SOI MCP Tools Summary

**Three new MCP tools (glass_query, glass_query_trend, glass_query_drill) exposing the full SOI compression and diff pipeline to AI agents, with get_last_n_run_ids() LIKE-pattern DB helper**

## Performance

- **Duration:** ~25 min
- **Started:** 2026-03-13T08:11:23Z
- **Completed:** 2026-03-13T08:36:00Z
- **Tasks:** 1 (TDD: tests + implementation)
- **Files modified:** 7 (2 functional + 5 formatting)

## Accomplishments
- Added `get_last_n_run_ids(&self, command_pattern, n)` to HistoryDb with SQL LIKE pattern support and oldest-first result ordering
- Implemented `glass_query` MCP tool: returns CompressedOutput JSON at requested token budget, or informative text when no SOI data
- Implemented `glass_query_trend` MCP tool: compares last N runs of a command, builds per-run summaries and consecutive diffs, detects regressions by inspecting TestResult JSON data
- Implemented `glass_query_drill` MCP tool: returns full record detail including parsed data JSON for any record_id from glass_query's record_ids list
- Added 5 db tests and 13 mcp tests; all 186 tests pass workspace-wide

## Task Commits

Each task was committed atomically:

1. **Task 1: Add get_last_n_run_ids and implement three SOI MCP tools** - `a645a62` (feat)

## Files Created/Modified
- `crates/glass_history/src/db.rs` - Added get_last_n_run_ids() method + 5 tests
- `crates/glass_mcp/src/tools.rs` - Added QueryParams/QueryTrendParams/QueryDrillParams structs, parse_budget(), glass_query/glass_query_trend/glass_query_drill tool methods + 13 tests
- `crates/glass_history/src/compress.rs` - cargo fmt only (pre-existing formatting debt)
- `crates/glass_history/src/lib.rs` - cargo fmt only
- `crates/glass_renderer/src/block_renderer.rs` - cargo fmt only
- `crates/glass_terminal/src/lib.rs` - cargo fmt only
- `src/main.rs` - cargo fmt only

## Decisions Made
- TestResult regression detection inspects the JSON `data` column for `"status": "Failed"` inside the `"TestResult"` serde wrapper object. DB `severity` is always `None` for TestResult records (established in Phase 49), so the plan's suggestion of checking `severity == "Error"` on RecordFingerprint was incorrect.
- `glass_query_drill` uses inline SQL with `.optional()` rather than a new HistoryDb method since it is a one-off single-record lookup.
- `parse_budget()` maps `None` and any unrecognized string to `TokenBudget::Summary` as a safe default.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed regression detection to use JSON data inspection instead of severity column**
- **Found during:** Task 1 (glass_query_trend implementation)
- **Issue:** Plan specified checking `record_type == "TestResult" && severity == "Error"` on RecordFingerprint, but TestResult records always have `severity = None` in the DB (Phase 49 decision). This caused the regression test to fail.
- **Fix:** Changed regression detection to parse the JSON `data` column and check `inner.get("status") == Some("Failed")` for TestResult records in the current run that were not failed in the previous run.
- **Files modified:** crates/glass_mcp/src/tools.rs
- **Verification:** `test_glass_query_trend_regression_detection` passes
- **Committed in:** a645a62 (Task 1 commit)

**2. [Rule 3 - Blocking] Ran cargo fmt to fix pre-existing formatting failures**
- **Found during:** Task 1 (final verification)
- **Issue:** `cargo fmt --all -- --check` failed due to pre-existing formatting debt in compress.rs, block_renderer.rs, lib.rs, main.rs from phases 51/52 (uncommitted fmt changes)
- **Fix:** Ran `cargo fmt --all` to format all files workspace-wide
- **Files modified:** crates/glass_history/src/compress.rs, crates/glass_history/src/lib.rs, crates/glass_renderer/src/block_renderer.rs, crates/glass_terminal/src/lib.rs, src/main.rs
- **Verification:** `cargo fmt --all -- --check` passes with exit code 0
- **Committed in:** a645a62 (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking)
**Impact on plan:** Both fixes necessary for correctness and CI compliance. No scope creep.

## Issues Encountered
- Initial TDD RED phase required correcting glass_soi type usage: `OutputRecord::TestResult` is a struct variant (not tuple struct), and the output type is `OutputType::RustTest` (not `OutputType::CargoTest`). Fixed by reading crates/glass_soi/src/types.rs.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All three SOI MCP tools registered and tested; glass_query, glass_query_trend, glass_query_drill are ready for use by AI agents
- Plan 53-02 (SOI MCP context integration) was already completed in a prior session
- Phase 53 plan 03 (if any) is the next work item

---
*Phase: 53-soi-mcp-tools*
*Completed: 2026-03-13*

---
phase: 50-soi-pipeline-integration
plan: 01
subsystem: database
tags: [rust, rusqlite, sqlite, soi, history, events]

# Dependency graph
requires:
  - phase: 49-soi-storage-schema
    provides: glass_history HistoryDb, SOI tables (command_output_records, output_records)
  - phase: 48-soi-classifier-parser
    provides: glass_soi::ParsedOutput, Severity, OutputRecord types
provides:
  - HistoryDb path field and path() accessor for worker thread re-opening
  - HistoryDb::get_output_for_command() - fetch stored output by command_id
  - HistoryDb::get_command_text() - fetch command text by command_id
  - AppEvent::SoiReady variant with command_id, summary, severity fields
  - SoiSummary struct and last_soi_summary field on Session
affects: [50-02-soi-worker-spawn, glass_history, glass_core, glass_mux]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "OptionalExtension flattening: query_row returns Option<Option<T>> for nullable columns, use .flatten() for ergonomic Option<T>"
    - "Severity as String in AppEvent::SoiReady to avoid glass_core depending on glass_soi"

key-files:
  created: []
  modified:
    - crates/glass_history/src/db.rs
    - crates/glass_core/src/event.rs
    - crates/glass_mux/src/session.rs
    - src/main.rs

key-decisions:
  - "get_output_for_command uses Option<Option<String>> + flatten() to handle NULL output column -- avoids rusqlite FromSql Null error on absent output"
  - "SoiReady severity field is String not glass_soi::Severity to keep glass_core dep-free of glass_soi"
  - "SoiReady stub match arm added in main.rs to keep workspace compiling -- full handler is Plan 02 work"

patterns-established:
  - "Nullable SQLite columns via row.get::<_, Option<T>>(col_idx) + optional().flatten() pattern"

requirements-completed: [SOIL-03, SOIL-04]

# Metrics
duration: 15min
completed: 2026-03-13
---

# Phase 50 Plan 01: SOI Pipeline Integration Infrastructure Summary

**HistoryDb path accessor and output fetch helpers, AppEvent::SoiReady variant, and SoiSummary on Session -- providing all building blocks for the Plan 02 SOI worker thread**

## Performance

- **Duration:** ~15 min
- **Started:** 2026-03-13T06:30:00Z
- **Completed:** 2026-03-13T06:47:20Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Added `path: PathBuf` field and `pub fn path()` accessor to `HistoryDb` for worker thread re-opening
- Added `get_output_for_command()` and `get_command_text()` helpers with correct NULL column handling
- Added SOI edge case tests: `soi_worker_no_output` and `soi_worker_binary` (SOIL-04)
- Added `AppEvent::SoiReady` variant to `glass_core` with `app_event_soi_ready_variant` test using `WindowId::dummy()` (SOIL-03)
- Added `SoiSummary` struct and `last_soi_summary: Option<SoiSummary>` field to `Session`

## Task Commits

Each task was committed atomically:

1. **Task 1: HistoryDb path field, output fetch helpers, SOI edge case tests** - `34d3adf` (feat)
2. **Task 2: AppEvent::SoiReady variant and SoiSummary on Session** - `97f7b91` (feat)

**Plan metadata:** (docs commit below)

## Files Created/Modified
- `crates/glass_history/src/db.rs` - Added path field, path() accessor, get_output_for_command, get_command_text, and 5 new tests
- `crates/glass_core/src/event.rs` - Added AppEvent::SoiReady variant with test
- `crates/glass_mux/src/session.rs` - Added SoiSummary struct and last_soi_summary field on Session
- `src/main.rs` - Added last_soi_summary: None to Session construction, stub SoiReady match arm

## Decisions Made
- `get_output_for_command` uses `row.get::<_, Option<String>>(0)` + `.optional().map_err().flatten()` to handle the NULL output column -- the naive `row.get::<_, String>(0)` fails with "Invalid column type Null" when output is NULL
- `AppEvent::SoiReady.severity` is `String` not `glass_soi::Severity` to keep `glass_core` free of a `glass_soi` dependency
- Added stub `AppEvent::SoiReady { .. } => {}` match arm in `main.rs` to satisfy exhaustiveness -- full handler is Plan 02 scope

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed NULL column handling in get_output_for_command**
- **Found during:** Task 1 (test execution)
- **Issue:** Plan's suggested `row.get(0)` + `.optional()` fails with "Invalid column type Null at index: 0" when the output column is NULL (not the same as no row)
- **Fix:** Changed to `row.get::<_, Option<String>>(0)` and added `.flatten()` to collapse `Option<Option<String>>` into `Option<String>`
- **Files modified:** crates/glass_history/src/db.rs
- **Verification:** `soi_worker_no_output` and `test_get_output_for_command` tests pass
- **Committed in:** 34d3adf (Task 1 commit)

**2. [Rule 3 - Blocking] Added SoiReady stub match arm in main.rs**
- **Found during:** Task 2 (cargo build --workspace)
- **Issue:** Adding AppEvent::SoiReady made the match in main.rs non-exhaustive, failing compilation
- **Fix:** Added `AppEvent::SoiReady { .. } => {}` stub with comment that full handler is in Plan 02
- **Files modified:** src/main.rs
- **Verification:** `cargo build --workspace` succeeds
- **Committed in:** 97f7b91 (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (1 bug fix, 1 blocking issue)
**Impact on plan:** Both auto-fixes necessary for correctness and compilation. No scope creep.

## Issues Encountered
- None beyond the two auto-fixed deviations above.

## Next Phase Readiness
- Plan 02 (SOI worker spawn + handler) has all infrastructure it needs:
  - `HistoryDb::path()` for reopening DB on worker thread
  - `HistoryDb::get_output_for_command()` and `get_command_text()` for input to parser
  - `AppEvent::SoiReady` to fire result back to main thread
  - `Session::last_soi_summary` to store the result
  - Stub match arm in main.rs ready to replace with real handler

---
*Phase: 50-soi-pipeline-integration*
*Completed: 2026-03-13*

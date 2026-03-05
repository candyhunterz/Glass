---
phase: 06-output-capture-writer-integration
plan: 01
subsystem: database
tags: [sqlite, schema-migration, ansi, binary-detection, output-processing, tdd]

# Dependency graph
requires:
  - phase: 05-history-database-foundation
    provides: HistoryDb with CommandRecord, insert/get/search/prune, FTS5
provides:
  - output processing pipeline (strip_ansi, is_binary, truncate_head_tail, process_output)
  - CommandRecord.output field with schema migration v0->v1
  - PRAGMA user_version migration pattern for future schema changes
  - HistoryConfig.max_output_capture_kb config field (default 50)
affects: [06-02-PLAN, 06-03-PLAN, glass_history consumers]

# Tech tracking
tech-stack:
  added: [strip-ansi-escapes 0.2, toml (dev-dep)]
  patterns: [PRAGMA user_version migration, head+tail truncation, binary detection threshold]

key-files:
  created: [crates/glass_history/src/output.rs]
  modified: [crates/glass_history/src/db.rs, crates/glass_history/src/config.rs, crates/glass_history/src/retention.rs, crates/glass_history/src/lib.rs, crates/glass_history/Cargo.toml]

key-decisions:
  - "Binary detection runs on raw bytes before ANSI stripping to preserve accurate non-printable ratio"
  - "Migration uses SELECT probe to check if output column exists before ALTER TABLE (idempotent for fresh DBs)"
  - "serde default function for max_output_capture_kb enables backward-compatible TOML parsing"

patterns-established:
  - "PRAGMA user_version migration: check version, apply ALTER TABLE, bump version"
  - "Output processing pipeline: binary check -> ANSI strip -> UTF-8 lossy -> truncate"
  - "Head+tail truncation with floor_char_boundary/ceil_char_boundary for UTF-8 safety"

requirements-completed: [HIST-02]

# Metrics
duration: 5min
completed: 2026-03-05
---

# Phase 6 Plan 1: Output Storage Foundation Summary

**Output processing pipeline with ANSI stripping, binary detection, head+tail truncation, and schema migration adding output column to CommandRecord**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-05T15:50:11Z
- **Completed:** 2026-03-05T15:55:06Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- Output processing module with 4 exported functions: strip_ansi, is_binary, truncate_head_tail, process_output
- Schema migration v0->v1 using PRAGMA user_version pattern, adding output TEXT column
- CommandRecord gains output: Option<String> field with full insert/get support
- HistoryConfig gains max_output_capture_kb: u32 (default 50) with serde deserialization
- 38 glass_history tests passing, 112 workspace tests passing with zero regressions

## Task Commits

Each task was committed atomically:

1. **Task 1: Output processing module (RED)** - `fc8a783` (test)
2. **Task 1: Output processing module (GREEN)** - `27ad369` (feat)
3. **Task 2: Schema migration and CommandRecord output field** - `ce690cd` (feat)

_TDD: Task 1 had separate RED/GREEN commits. Task 2 combined RED+GREEN since struct field addition required all code to compile together._

## Files Created/Modified
- `crates/glass_history/src/output.rs` - Output processing: strip_ansi, is_binary, truncate_head_tail, process_output with 16 tests
- `crates/glass_history/src/db.rs` - CommandRecord.output field, schema migration v0->v1, updated insert/get, 6 new tests
- `crates/glass_history/src/config.rs` - max_output_capture_kb field with default 50, 3 new tests
- `crates/glass_history/src/retention.rs` - Updated CommandRecord constructions for output field
- `crates/glass_history/src/lib.rs` - Added pub mod output
- `crates/glass_history/Cargo.toml` - Added strip-ansi-escapes dep, toml dev-dep

## Decisions Made
- Binary detection runs on raw bytes before ANSI stripping -- stripping null bytes changes the non-printable ratio, giving false negatives
- Migration uses a SELECT probe to detect existing output column rather than unconditional ALTER TABLE -- idempotent for fresh databases that already have the column from create_schema
- Used serde default function for max_output_capture_kb to allow backward-compatible TOML parsing when the field is absent

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Binary detection order in process_output**
- **Found during:** Task 1 (GREEN phase)
- **Issue:** Plan implied binary detection after ANSI stripping, but strip-ansi-escapes removes null bytes, reducing the non-printable ratio below the 30% threshold
- **Fix:** Moved is_binary check to run on raw bytes before ANSI stripping
- **Files modified:** crates/glass_history/src/output.rs
- **Verification:** test_process_output_binary passes
- **Committed in:** 27ad369 (Task 1 GREEN commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Essential for correctness of binary detection. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Output processing functions ready for Plan 02 to wire into the PTY capture pipeline
- CommandRecord.output field ready for database writes from captured output
- max_output_capture_kb config available for configuring capture buffer size
- PRAGMA user_version migration pattern established for any future schema changes

---
*Phase: 06-output-capture-writer-integration*
*Completed: 2026-03-05*

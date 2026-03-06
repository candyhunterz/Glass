---
phase: 18-storage-retention
plan: 01
subsystem: database
tags: [sqlite, schema-migration, pipeline-stages, retention, fts5]

# Dependency graph
requires:
  - phase: 16-pipe-capture
    provides: CapturedStage and FinalizedBuffer types for pipeline data
  - phase: 06-output-capture
    provides: HistoryDb schema v1 with output column and CommandRecord
provides:
  - pipe_stages table with FK to commands(id) ON DELETE CASCADE
  - PipeStageRow struct for pipeline stage storage
  - insert_pipe_stages() and get_pipe_stages() methods on HistoryDb
  - Schema v2 migration (v0->v1->v2 chain)
  - Retention cascade to pipe_stages in age and size pruning
  - CommandFinished handler persistence of pipeline stage data
affects: [19-history-ui, query, mcp-tools]

# Tech tracking
tech-stack:
  added: []
  patterns: [schema migration chain with hardcoded version steps, belt-and-suspenders CASCADE + explicit DELETE]

key-files:
  created: []
  modified:
    - crates/glass_history/src/db.rs
    - crates/glass_history/src/retention.rs
    - crates/glass_history/src/lib.rs
    - src/main.rs

key-decisions:
  - "Hardcoded version numbers in migration steps (1, 2) instead of SCHEMA_VERSION constant to prevent version skipping"
  - "Belt-and-suspenders deletion: explicit DELETE FROM pipe_stages + ON DELETE CASCADE for safety"
  - "FinalizedBuffer-to-PipeStageRow conversion in main.rs to avoid coupling glass_history to glass_pipes"
  - "PRAGMA foreign_keys = ON enabled globally in HistoryDb::open()"

patterns-established:
  - "Schema migration chain: each version step uses hardcoded target, not SCHEMA_VERSION constant"
  - "Child table deletion before parent in both retention and delete_command (defense in depth with CASCADE)"

requirements-completed: [STOR-01, STOR-02]

# Metrics
duration: 6min
completed: 2026-03-06
---

# Phase 18 Plan 01: Storage + Retention Summary

**pipe_stages table with schema v2 migration, insert/get/cascade methods, and CommandFinished persistence wiring**

## Performance

- **Duration:** 6 min
- **Started:** 2026-03-06T17:45:28Z
- **Completed:** 2026-03-06T17:51:58Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Schema v2 migration creating pipe_stages table with FK to commands(id) ON DELETE CASCADE
- PipeStageRow struct with insert_pipe_stages() and get_pipe_stages() methods on HistoryDb
- Retention pruning (age + size) cascades to pipe_stages before commands deletion
- delete_command() explicitly deletes pipe_stages before commands
- CommandFinished handler in main.rs converts FinalizedBuffer variants to PipeStageRow and persists
- 8 comprehensive tests covering migration, CRUD, buffer variants, and cascade behavior
- All 368 workspace tests pass, clean release build

## Task Commits

Each task was committed atomically:

1. **Task 1: Schema migration, DB methods, and retention cascade** - `d040e84` (feat)
2. **Task 2: Wire pipe stage persistence in main.rs CommandFinished handler** - `cb0d673` (feat)

## Files Created/Modified
- `crates/glass_history/src/db.rs` - Schema v2 migration, PipeStageRow struct, insert/get methods, delete cascade, 8 new tests
- `crates/glass_history/src/retention.rs` - pipe_stages deletion in both age-based and size-based pruning loops
- `crates/glass_history/src/lib.rs` - Re-export of PipeStageRow
- `src/main.rs` - FinalizedBuffer-to-PipeStageRow conversion and persistence in CommandFinished handler

## Decisions Made
- Hardcoded version numbers in migration steps (1, 2) instead of using SCHEMA_VERSION constant to prevent v0 databases from skipping the v2 migration by jumping straight to SCHEMA_VERSION
- Belt-and-suspenders deletion: explicit DELETE FROM pipe_stages before commands deletion PLUS ON DELETE CASCADE as safety net
- FinalizedBuffer-to-PipeStageRow conversion happens in main.rs (not glass_history) to avoid coupling glass_history to glass_pipes, matching the existing pattern where CommandRecord is constructed in main.rs
- PRAGMA foreign_keys = ON enabled globally in HistoryDb::open() to support CASCADE

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- pipe_stages table is ready for query integration (history UI, MCP tools)
- Schema migration chain tested: v0->v1->v2 with data preservation
- All existing tests continue to pass with no regressions

## Self-Check: PASSED

All files verified present. All commits verified in git log.

---
*Phase: 18-storage-retention*
*Completed: 2026-03-06*

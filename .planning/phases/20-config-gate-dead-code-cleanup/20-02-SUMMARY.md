---
phase: 20-config-gate-dead-code-cleanup
plan: 02
subsystem: pipes
tags: [rust, dead-code-removal, glass_pipes, refactor]

# Dependency graph
requires: []
provides:
  - "Leaner glass_pipes crate with no unused classify module or PipelineClassification type"
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns: []

key-files:
  created: []
  modified:
    - crates/glass_pipes/src/lib.rs
    - crates/glass_pipes/src/types.rs
    - crates/glass_pipes/src/parser.rs

key-decisions:
  - "Removed test_parse_pipeline_default_classification test alongside PipelineClassification removal (test was validating dead code defaults)"

patterns-established: []

requirements-completed: [PIPE-02]

# Metrics
duration: 2min
completed: 2026-03-06
---

# Phase 20 Plan 02: Dead Classify Module Removal Summary

**Removed 255 lines of dead code: classify.rs module, PipelineClassification struct, and all unused TTY/opt-out classification exports from glass_pipes**

## Performance

- **Duration:** 2 min
- **Started:** 2026-03-06T19:06:33Z
- **Completed:** 2026-03-06T19:08:12Z
- **Tasks:** 1
- **Files modified:** 4 (1 deleted, 3 edited)

## Accomplishments
- Deleted classify.rs (216 lines including 11 tests for dead TTY detection and opt-out logic)
- Removed PipelineClassification struct and its Default impl from types.rs
- Removed classification field from Pipeline struct, simplifying it to {raw_command, stages}
- Cleaned up lib.rs exports and parser.rs imports
- All 46 tests pass (43 unit + 3 integration); 12 dead tests removed

## Task Commits

Each task was committed atomically:

1. **Task 1: Delete classify.rs and remove PipelineClassification** - `5cc3b20` (refactor)

## Files Created/Modified
- `crates/glass_pipes/src/classify.rs` - DELETED (dead TTY classification and opt-out logic)
- `crates/glass_pipes/src/lib.rs` - Removed classify module declaration and re-exports
- `crates/glass_pipes/src/types.rs` - Removed PipelineClassification struct, Default impl, and classification field from Pipeline
- `crates/glass_pipes/src/parser.rs` - Removed PipelineClassification import and default construction; removed classification test

## Decisions Made
- Removed the `parse_pipeline_default_classification` test since it tested the now-deleted PipelineClassification defaults -- this was dead code validation, not live behavior

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- glass_pipes crate now exports only live code: types, parser (split_pipes, parse_pipeline), StageBuffer, BufferPolicy
- No further cleanup needed in this crate

## Self-Check: PASSED

- lib.rs: FOUND
- types.rs: FOUND
- parser.rs: FOUND
- classify.rs: CONFIRMED DELETED
- Commit 5cc3b20: FOUND

---
*Phase: 20-config-gate-dead-code-cleanup*
*Completed: 2026-03-06*

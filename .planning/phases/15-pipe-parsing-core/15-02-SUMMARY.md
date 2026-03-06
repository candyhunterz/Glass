---
phase: 15-pipe-parsing-core
plan: 02
subsystem: pipes
tags: [tty-detection, binary-detection, buffer-management, pipeline-classification, shlex]

# Dependency graph
requires:
  - phase: 15-pipe-parsing-core/01
    provides: PipeStage, PipelineClassification, BufferPolicy, StageBuffer, FinalizedBuffer types
provides:
  - Pipeline classification with TTY detection and --no-glass opt-out
  - StageBuffer with overflow head/tail sampling and binary detection
  - Fully functional glass_pipes crate ready for shell integration
affects: [16-shell-integration, 17-pipe-visualization]

# Tech tracking
tech-stack:
  added: []
  patterns: [rolling-tail-window, binary-detection-by-control-char-ratio, tty-command-allowlist]

key-files:
  created: [crates/glass_pipes/src/classify.rs]
  modified: [crates/glass_pipes/src/types.rs, crates/glass_pipes/src/lib.rs]

key-decisions:
  - "Control char ratio for binary detection (bytes < 0x08 or 0x0E..0x1F, >30% threshold) matching glass_history pattern"
  - "Rolling tail window via drain for O(n) overflow append rather than ring buffer"

patterns-established:
  - "TTY allowlist pattern: static const list with special-case git subcommand handling"
  - "Overflow sampling: head truncated on transition, tail as rolling window"

requirements-completed: [PIPE-02, PIPE-03, CAPT-03, CAPT-04]

# Metrics
duration: 3min
completed: 2026-03-06
---

# Phase 15 Plan 02: Pipeline Classification and StageBuffer Summary

**TTY detection across 30+ programs with git pager subcommands, --no-glass opt-out, and StageBuffer with 10MB overflow sampling and binary detection**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-06T06:28:02Z
- **Completed:** 2026-03-06T06:30:39Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Pipeline classification detecting TTY-sensitive commands across 30+ programs including git pager subcommands
- Exact token --no-glass opt-out flag detection with no false positives on substrings
- StageBuffer with head/tail sampling on overflow (configurable policy, default 10MB/512KB)
- Binary data detection using control character ratio on first 8KB sample
- 30 new unit tests (16 classify + 14 buffer) with full workspace regression pass (333 tests)

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement pipeline classification with TDD** - `da427a2` (feat)
2. **Task 2: Implement StageBuffer with overflow sampling and binary detection** - `f947f06` (feat)

## Files Created/Modified
- `crates/glass_pipes/src/classify.rs` - TTY detection, opt-out check, classify_pipeline function
- `crates/glass_pipes/src/types.rs` - Full StageBuffer append/finalize with overflow and binary detection
- `crates/glass_pipes/src/lib.rs` - Added classify module export

## Decisions Made
- Used control char ratio (bytes < 0x08 or 0x0E..0x1F exceeding 30%) for binary detection, matching glass_history::output::is_binary pattern
- Rolling tail window via Vec::drain rather than ring buffer -- simpler, adequate for the append-heavy workload

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- glass_pipes crate is fully functional with parsing (Plan 01) and classification/buffering (Plan 02)
- Ready for Phase 16 shell integration to consume classify_pipeline and StageBuffer
- All 50 glass_pipes tests pass, no regressions in 333 workspace tests

---
*Phase: 15-pipe-parsing-core*
*Completed: 2026-03-06*

---
phase: 16-shell-capture-terminal-transport
plan: 01
subsystem: terminal
tags: [osc, shell-integration, pipeline-capture, pty, event-system]

# Dependency graph
requires:
  - phase: 15-pipe-parsing-core
    provides: FinalizedBuffer type, pipeline parser, classification
provides:
  - CapturedStage type in glass_pipes for downstream pipeline capture consumers
  - OscEvent::PipelineStart and PipelineStage variants for OSC 133;S and 133;P parsing
  - ShellEvent::PipelineStart and PipelineStage variants for cross-crate event flow
  - OscScanner parsing of OSC 133;S;{count} and OSC 133;P;{index};{size};{path}
  - convert_osc_to_shell and shell_event_to_osc updated for new variants
affects: [16-02-block-wiring, 16-03-shell-scripts]

# Tech tracking
tech-stack:
  added: []
  patterns: [OSC 133;S/P protocol for pipeline stage capture signaling]

key-files:
  created: []
  modified:
    - crates/glass_pipes/src/types.rs
    - crates/glass_core/src/event.rs
    - crates/glass_terminal/src/osc_scanner.rs
    - crates/glass_terminal/src/pty.rs
    - crates/glass_terminal/src/block_manager.rs
    - src/main.rs

key-decisions:
  - "OSC 133;P encodes index, total_bytes, and temp_path in semicolon-delimited fields with splitn(3) for path to preserve Windows path colons"
  - "CapturedStage uses Option<String> for temp_path to support both temp-file and in-memory capture paths"

patterns-established:
  - "Pipeline OSC protocol: S;{count} for start, P;{index};{bytes};{path} for stage data"
  - "New OscEvent/ShellEvent variants require updating block_manager.rs, pty.rs, and main.rs match arms"

requirements-completed: [CAPT-01, CAPT-02]

# Metrics
duration: 3min
completed: 2026-03-06
---

# Phase 16 Plan 01: Pipeline Capture Types and OSC Parsing Summary

**CapturedStage type, OscEvent/ShellEvent pipeline variants, and OscScanner parsing for OSC 133;S/P sequences with full convert function wiring**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-06T07:00:15Z
- **Completed:** 2026-03-06T07:03:28Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- Defined CapturedStage struct in glass_pipes with index, total_bytes, data (FinalizedBuffer), and temp_path fields
- Extended OscEvent and ShellEvent enums with PipelineStart and PipelineStage variants
- Implemented OscScanner parsing for OSC 133;S;{count} and OSC 133;P;{index};{size};{path} sequences
- Wired convert_osc_to_shell (pty.rs) and shell_event_to_osc (main.rs) for new variants
- Added wildcard arm in block_manager.rs to handle pipeline events gracefully
- All 346 workspace tests pass, full workspace compiles cleanly

## Task Commits

Each task was committed atomically:

1. **Task 1: Define CapturedStage type and extend event enums** - `592645f` (feat)
2. **Task 2: Extend OscScanner parsing and wire convert functions** - `18d2415` (feat)

_Note: TDD tasks -- tests written first (RED), then implementation (GREEN), committed together._

## Files Created/Modified
- `crates/glass_pipes/src/types.rs` - Added CapturedStage struct after FinalizedBuffer
- `crates/glass_core/src/event.rs` - Added PipelineStart/PipelineStage to ShellEvent, added tests module
- `crates/glass_terminal/src/osc_scanner.rs` - Added PipelineStart/PipelineStage to OscEvent, S/P parsing in parse_osc133, 8 new tests
- `crates/glass_terminal/src/pty.rs` - Extended convert_osc_to_shell with new variant mappings
- `crates/glass_terminal/src/block_manager.rs` - Added wildcard arm for pipeline events
- `src/main.rs` - Extended shell_event_to_osc with reverse mappings

## Decisions Made
- Used splitn(3, ';') for P sequence parsing so Windows paths with colons (C:/...) are preserved intact in the temp_path field
- CapturedStage temp_path is Option<String> to support both temp-file-based and future in-memory capture workflows

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added block_manager.rs wildcard arm for new OscEvent variants**
- **Found during:** Task 2 (wiring convert functions)
- **Issue:** block_manager.rs had a non-exhaustive match on OscEvent that blocked compilation after adding PipelineStart/PipelineStage variants
- **Fix:** Added wildcard arm `OscEvent::PipelineStart { .. } | OscEvent::PipelineStage { .. } => {}` to handle_event match
- **Files modified:** crates/glass_terminal/src/block_manager.rs
- **Verification:** Full workspace compiles and all tests pass
- **Committed in:** 18d2415 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Required for compilation. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Pipeline capture types and parsing infrastructure complete
- Plan 02 (block wiring) can consume PipelineStart/PipelineStage ShellEvents from the main event loop
- Plan 03 (shell scripts) can emit OSC 133;S and 133;P sequences that the OscScanner will parse

---
*Phase: 16-shell-capture-terminal-transport*
*Completed: 2026-03-06*

---
phase: 25-terminalexit-multi-pane-fix
plan: 01
subsystem: terminal
tags: [rust, split-panes, event-handling, pty]

# Dependency graph
requires:
  - phase: 24-split-panes
    provides: close_pane/close_tab APIs in SessionMux
provides:
  - Pane-aware TerminalExit handler that closes only the exited pane in multi-pane tabs
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns: [pane-count dispatch before close in event handlers]

key-files:
  created: []
  modified: [src/main.rs]

key-decisions:
  - "Mirror Ctrl+Shift+W pane-count logic in TerminalExit handler"
  - "Use session_id from event directly (not focused_session_id) for tab lookup"

patterns-established:
  - "TerminalExit and Ctrl+Shift+W both check pane_count before dispatching close_pane vs close_tab"

requirements-completed: [SPLIT-11]

# Metrics
duration: 2min
completed: 2026-03-07
---

# Phase 25 Plan 01: TerminalExit Multi-Pane Fix Summary

**Pane-aware TerminalExit handler that uses close_pane() for multi-pane tabs instead of close_tab(), completing SPLIT-11**

## Performance

- **Duration:** 2 min
- **Started:** 2026-03-07T04:54:26Z
- **Completed:** 2026-03-07T04:56:05Z
- **Tasks:** 2
- **Files modified:** 1

## Accomplishments
- TerminalExit handler now checks pane_count on the tab containing the exited session
- Multi-pane tabs close only the exited pane via close_pane(session_id)
- Single-pane tabs still close the entire tab via close_tab(idx) as before
- Remaining panes resize after pane closure
- All 66 glass_mux unit tests pass

## Task Commits

Each task was committed atomically:

1. **Task 1: Fix TerminalExit handler to use close_pane for multi-pane tabs** - `7ba2674` (fix)
2. **Task 2: Verify close_pane unit tests still pass** - no commit (verification-only task, no code changes)

## Files Created/Modified
- `src/main.rs` - TerminalExit handler updated with pane-count dispatch logic

## Decisions Made
- Mirror Ctrl+Shift+W pane-count logic in TerminalExit handler
- Use session_id from the event directly (not focused_session_id) for tab lookup -- TerminalExit knows which specific session exited

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- SPLIT-11 fully satisfied
- Both shell-exit and Ctrl+Shift+W now handle multi-pane tabs correctly

---
*Phase: 25-terminalexit-multi-pane-fix*
*Completed: 2026-03-07*

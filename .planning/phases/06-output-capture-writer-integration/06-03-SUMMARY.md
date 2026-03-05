---
phase: 06-output-capture-writer-integration
plan: 03
subsystem: renderer
tags: [scrollback, display_offset, block-decorations, wgpu]

# Dependency graph
requires:
  - phase: 05-history-database-foundation
    provides: block_renderer with display_offset parameter support
provides:
  - Real display_offset wired through frame.rs for block decoration scrollback
  - Correct scroll direction, absolute line tracking, and viewport coordinate math
affects: [08-search-navigation, 07-mcp-server]

# Tech tracking
tech-stack:
  added: []
  patterns: [absolute-coordinate block tracking, viewport_abs_start calculation]

key-files:
  created: []
  modified:
    - crates/glass_renderer/src/frame.rs
    - crates/glass_renderer/src/grid_renderer.rs
    - crates/glass_terminal/src/grid_snapshot.rs
    - crates/glass_terminal/src/pty.rs
    - src/main.rs

key-decisions:
  - "Use absolute line numbers from PTY for block tracking instead of viewport-relative"
  - "Add history_size to GridSnapshot for absolute coordinate math in block viewport calculation"

patterns-established:
  - "Absolute coordinates: block positions use absolute line numbers, converted to viewport-relative at render time"

requirements-completed: [INFR-02]

# Metrics
duration: 25min
completed: 2026-03-05
---

# Phase 6 Plan 3: Block Decoration Scrollback Summary

**Wired real display_offset into block decoration rendering and fixed 5 scrollback bugs (scroll direction, grid offset, absolute line tracking, viewport calculation, GridSnapshot history_size)**

## Performance

- **Duration:** ~25 min (across checkpoint)
- **Started:** 2026-03-05T17:00:00Z
- **Completed:** 2026-03-05T17:36:00Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- Replaced hardcoded `display_offset = 0` in frame.rs with `snapshot.display_offset` so block decorations scroll with content
- Fixed 5 related bugs discovered during human verification: scroll direction, grid renderer offset, absolute line tracking, viewport coordinate math, and GridSnapshot history_size field
- Block decorations (separator lines, exit code badges) now correctly scroll with terminal content during scrollback navigation

## Task Commits

Each task was committed atomically:

1. **Task 1: Wire snapshot.display_offset through frame.rs** - `1f9c5a6` (feat)
2. **Task 2: Verify block decorations scroll correctly** - `0b5733a` (fix - 5 bugs found during verification)

## Files Created/Modified
- `crates/glass_renderer/src/frame.rs` - Replaced hardcoded display_offset=0 with snapshot.display_offset; added viewport_abs_start calculation
- `crates/glass_renderer/src/grid_renderer.rs` - Account for display_offset in cell rect/cursor/text positions
- `crates/glass_terminal/src/grid_snapshot.rs` - Added history_size field for absolute coordinate math
- `crates/glass_terminal/src/pty.rs` - Send absolute line numbers instead of viewport-relative for block tracking
- `src/main.rs` - Fixed inverted scroll direction (Delta(-lines) to Delta(lines))
- `Cargo.lock` - Updated lockfile

## Decisions Made
- Use absolute line numbers from PTY for block tracking instead of viewport-relative numbers
- Add history_size to GridSnapshot to enable absolute-to-viewport coordinate conversion in block rendering

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Inverted scroll direction in main.rs**
- **Found during:** Task 2 (human verification)
- **Issue:** Scroll::Delta(-lines) caused scrolling in wrong direction
- **Fix:** Changed to Scroll::Delta(lines)
- **Files modified:** src/main.rs
- **Verification:** Manual testing confirmed correct scroll direction
- **Committed in:** 0b5733a

**2. [Rule 1 - Bug] Grid renderer not accounting for display_offset**
- **Found during:** Task 2 (human verification)
- **Issue:** Cell rects, cursor, and text positions in grid_renderer.rs ignored display_offset
- **Fix:** Added display_offset adjustment to coordinate calculations
- **Files modified:** crates/glass_renderer/src/grid_renderer.rs
- **Verification:** Visual verification during scrollback
- **Committed in:** 0b5733a

**3. [Rule 1 - Bug] PTY reader sending viewport-relative line numbers**
- **Found during:** Task 2 (human verification)
- **Issue:** Block tracking received viewport-relative line numbers instead of absolute, causing incorrect decoration placement during scroll
- **Fix:** Changed to send absolute line numbers
- **Files modified:** crates/glass_terminal/src/pty.rs
- **Verification:** Block decorations align correctly at all scroll positions
- **Committed in:** 0b5733a

**4. [Rule 1 - Bug] Block viewport calculation needed absolute coordinates**
- **Found during:** Task 2 (human verification)
- **Issue:** viewport_abs_start needed for converting absolute block positions to screen coordinates
- **Fix:** Added viewport_abs_start calculation in frame.rs and history_size to GridSnapshot
- **Files modified:** crates/glass_renderer/src/frame.rs, crates/glass_terminal/src/grid_snapshot.rs
- **Verification:** Block decorations render at correct positions during scrollback
- **Committed in:** 0b5733a

---

**Total deviations:** 4 auto-fixed (4 bugs, all Rule 1)
**Impact on plan:** All bugs were latent issues exposed by wiring in the real display_offset. Fixes were essential for the feature to work correctly. No scope creep.

## Issues Encountered
None beyond the bugs documented above, which were expected consequences of activating a previously-stubbed feature.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- display_offset blocker (INFR-02) is resolved -- Phase 8 search navigation can now scroll to results
- All Phase 6 plans complete -- ready to proceed to Phase 7 (MCP Server) or Phase 8 (Search)

## Self-Check: PASSED

All modified files verified present. Both commits (1f9c5a6, 0b5733a) verified in git log.

---
*Phase: 06-output-capture-writer-integration*
*Completed: 2026-03-05*

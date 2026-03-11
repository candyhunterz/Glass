---
phase: 47-tab-drag-reorder
plan: 02
subsystem: ui
tags: [drag-drop, tab-bar, reorder, event-loop, winit]

requires:
  - phase: 47-tab-drag-reorder
    provides: SessionMux::reorder_tab(), TabBarRenderer::drag_drop_index(), build_tab_rects drop_index param
provides:
  - TabDragState struct for drag tracking in event loop
  - Complete tab drag-to-reorder user interaction
  - Visual insertion indicator during active drag
affects: []

tech-stack:
  added: []
  patterns: [drag threshold state machine, event consumption with early return]

key-files:
  created: []
  modified:
    - src/main.rs
    - crates/glass_renderer/src/frame.rs

key-decisions:
  - "5px horizontal threshold before drag activates to prevent accidental drags"
  - "Drop slot index converted to final-position with shift adjustment for reorder_tab"
  - "CursorMoved returns early during drag to prevent hover/selection side effects"

patterns-established:
  - "Tab drag state machine: press->threshold->active drag->release pattern"

requirements-completed: [TAB-DRAG-REORDER-WIRE]

duration: 2min
completed: 2026-03-11
---

# Phase 47 Plan 02: Tab Drag Reorder Event Wiring Summary

**TabDragState wired into winit event loop with 5px drag threshold, drop indicator rendering, and click-vs-drag disambiguation**

## Performance

- **Duration:** 2 min
- **Started:** 2026-03-11T04:11:25Z
- **Completed:** 2026-03-11T04:13:51Z
- **Tasks:** 1
- **Files modified:** 2

## Accomplishments
- TabDragState struct tracks drag source, start position, active flag, and drop target
- Left-click on tab body starts potential drag; 5px threshold activates it
- CursorMoved computes drop_index via drag_drop_index() and triggers redraw
- Release completes reorder (with index shift adjustment) or activates tab if no drag
- drop_index threaded through both draw_frame and draw_multi_pane_frame for indicator rendering
- Close button, new-tab button, middle-click close, scrollbar drag, text selection all unaffected

## Task Commits

Each task was committed atomically:

1. **Task 1: Add TabDragState and wire press/move/release events** - `715e5f8` (feat)

## Files Created/Modified
- `src/main.rs` - Added TabDragState struct, drag state field on WindowContext, press/move/release event handlers, drop_index threading to render calls
- `crates/glass_renderer/src/frame.rs` - Added drop_index parameter to draw_frame and draw_multi_pane_frame, passed through to build_tab_rects

## Decisions Made
- 5px horizontal threshold prevents accidental drags on normal tab clicks
- Drop slot index adjusted by -1 when dropping after source to account for removal shift
- Early return in CursorMoved during drag prevents tab hover updates and text selection interference

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed collapsible if clippy warning**
- **Found during:** Task 1
- **Issue:** Nested if statements for drag threshold check triggered clippy::collapsible_if
- **Fix:** Combined into single if with && condition
- **Files modified:** src/main.rs
- **Committed in:** 715e5f8

---

**Total deviations:** 1 auto-fixed (1 bug/lint)
**Impact on plan:** Trivial style fix required by -D warnings. No scope change.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Tab drag-to-reorder feature is fully wired and functional
- Phase 47 complete: core logic (Plan 01) + event wiring (Plan 02)

---
*Phase: 47-tab-drag-reorder*
*Completed: 2026-03-11*

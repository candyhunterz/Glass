---
phase: 47-tab-drag-reorder
plan: 01
subsystem: ui
tags: [drag-drop, tab-bar, reorder, wgpu]

requires:
  - phase: 46-tab-bar-controls
    provides: TabBarRenderer with hit-testing and build_tab_rects
provides:
  - SessionMux::reorder_tab() for moving tabs with active_tab tracking
  - TabBarRenderer::drag_drop_index() for computing drop slot from mouse X
  - Drag indicator rendering in build_tab_rects via drop_index parameter
affects: [47-tab-drag-reorder plan 02 event wiring]

tech-stack:
  added: []
  patterns: [remove-insert reorder with index tracking, slot-based drop index computation]

key-files:
  created: []
  modified:
    - crates/glass_mux/src/session_mux.rs
    - crates/glass_renderer/src/tab_bar.rs
    - crates/glass_renderer/src/frame.rs

key-decisions:
  - "to index is final position (post-removal), not insertion-before index"
  - "Drop slot computed via midpoint rounding: ((x / stride) + 0.5) as usize"
  - "Indicator is 2px blue rect at gap between tabs"

patterns-established:
  - "reorder_tab active_tab adjustment: 3-branch if for moved/shifted-down/shifted-up"

requirements-completed: [TAB-DRAG-REORDER-LOGIC]

duration: 3min
completed: 2026-03-11
---

# Phase 47 Plan 01: Tab Drag Reorder Core Logic Summary

**SessionMux::reorder_tab() with active_tab tracking, TabBarRenderer::drag_drop_index() with blue insertion indicator**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-11T04:05:42Z
- **Completed:** 2026-03-11T04:09:07Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- reorder_tab() on SessionMux correctly moves tabs and adjusts active_tab with 8 unit tests
- drag_drop_index() computes correct insertion slot for any mouse X position
- build_tab_rects renders a 2px blue insertion indicator when drop_index is provided
- All 182 tests pass across both crates, zero clippy warnings

## Task Commits

Each task was committed atomically:

1. **Task 1: Add reorder_tab() to SessionMux with tests**
   - `9b340c5` (test: failing tests for reorder_tab)
   - `5313fbd` (feat: implement reorder_tab)
2. **Task 2: Add drag_drop_index() and drag indicator rendering**
   - `dbe7ce5` (test: failing tests for drag_drop_index and indicator)
   - `59a2243` (feat: implement drag_drop_index and drag indicator)

_Note: TDD tasks have RED (test) and GREEN (feat) commits_

## Files Created/Modified
- `crates/glass_mux/src/session_mux.rs` - Added reorder_tab() method with active_tab adjustment
- `crates/glass_renderer/src/tab_bar.rs` - Added drag_drop_index(), DRAG_INDICATOR constants, drop_index param on build_tab_rects
- `crates/glass_renderer/src/frame.rs` - Updated 2 call sites to pass None for drop_index

## Decisions Made
- `to` index uses final-position semantics (post-removal) rather than insertion-before semantics
- Drop slot computed via midpoint rounding for natural feel: `((x / stride) + 0.5) as usize`
- Indicator is a 2px-wide blue accent rect ([0.4, 0.6, 1.0, 1.0]) positioned at gap center

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Core reorder logic and rendering primitives ready for Plan 02 (event wiring)
- Plan 02 will thread drag state through event loop and connect to reorder_tab/drag_drop_index

---
*Phase: 47-tab-drag-reorder*
*Completed: 2026-03-11*

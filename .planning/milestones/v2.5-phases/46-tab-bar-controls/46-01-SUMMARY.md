---
phase: 46-tab-bar-controls
plan: 01
subsystem: ui
tags: [wgpu, tab-bar, hit-testing, layout-engine]

requires:
  - phase: 45-scrollbar
    provides: hover state pattern on WindowContext
provides:
  - TabHitResult enum for tab/close/new-tab click distinction
  - Variable-width tab layout with MIN_TAB_WIDTH floor
  - Close button highlight rect (hover-only)
  - "+" new tab button positioning and hit-testing
  - hit_test_tab_index convenience method for hover tracking
affects: [46-tab-bar-controls plan 02, 47-tab-drag-reorder]

tech-stack:
  added: []
  patterns: [TabHitResult enum for multi-target hit-testing, compute_tab_width helper for layout reuse]

key-files:
  created: []
  modified:
    - crates/glass_renderer/src/tab_bar.rs
    - crates/glass_renderer/src/frame.rs
    - src/main.rs

key-decisions:
  - "Close button hit-test checked before tab body to prevent click-through"
  - "Tab width = (viewport - plus_button - gaps) / count, clamped to MIN_TAB_WIDTH 60px"
  - "truncate_title accepts max_len parameter for dynamic truncation with close button"

patterns-established:
  - "TabHitResult enum: multi-target hit-testing pattern for UI elements with sub-regions"

requirements-completed: [TAB-01, TAB-02, TAB-03, TAB-04, TAB-05, TAB-06, TAB-07]

duration: 4min
completed: 2026-03-11
---

# Phase 46 Plan 01: Tab Bar Layout Engine Summary

**TabHitResult enum with variable-width layout, close/new-tab button rendering, and 24 unit tests**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-11T03:33:54Z
- **Completed:** 2026-03-11T03:37:53Z
- **Tasks:** 1
- **Files modified:** 3

## Accomplishments
- Added TabHitResult enum (Tab, CloseButton, NewTabButton) with Debug/Clone/PartialEq derives
- Rewrote layout engine with variable-width tabs, MIN_TAB_WIDTH (60px) floor, and "+" button (32px)
- Added close button highlight rect rendering only on hovered tab
- Rewrote hit_test to return Option<TabHitResult> with close button checked before tab body
- Added hit_test_tab_index convenience method for hover tracking
- Updated truncate_title to accept max_len for dynamic truncation when close button visible
- 24 unit tests passing (12 existing updated + 12 new)

## Task Commits

Each task was committed atomically:

1. **Task 1: Add TabHitResult enum and rewrite layout/hit-test engine** - `af60e3c` (feat)

## Files Created/Modified
- `crates/glass_renderer/src/tab_bar.rs` - TabHitResult enum, variable-width layout, close/new-tab buttons, 24 tests
- `crates/glass_renderer/src/frame.rs` - Updated build_tab_rects/build_tab_text calls with hovered_tab: None
- `src/main.rs` - Updated hit_test callers to handle TabHitResult enum

## Decisions Made
- Close button hit-test checked before tab body rect to prevent click-through (per research pitfall 1)
- Tab width computed as (viewport - NEW_TAB_BUTTON_WIDTH - gaps) / count, clamped to MIN_TAB_WIDTH
- truncate_title refactored to accept max_len parameter instead of using fixed constant
- "+" button always rendered at bar bg color (invisible until hover highlight added in Plan 02)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated frame.rs and main.rs callers for new signatures**
- **Found during:** Task 1
- **Issue:** Changing build_tab_rects/build_tab_text/hit_test signatures breaks callers in frame.rs and main.rs
- **Fix:** Updated 4 call sites in frame.rs to pass `None` for hovered_tab, updated 2 call sites in main.rs to handle TabHitResult enum
- **Files modified:** crates/glass_renderer/src/frame.rs, src/main.rs
- **Verification:** cargo clippy --workspace -- -D warnings passes
- **Committed in:** af60e3c (part of task commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary to maintain compilation. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- TabHitResult enum and layout engine ready for Plan 02 to wire mouse events
- frame.rs passes None for hovered_tab - Plan 02 will thread real hover state through
- main.rs left-click only handles Tab(i) - Plan 02 will add CloseButton and NewTabButton handling

---
*Phase: 46-tab-bar-controls*
*Completed: 2026-03-11*

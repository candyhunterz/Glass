---
phase: 23-tabs
plan: 01
subsystem: terminal-mux
tags: [tabs, session-mux, tab-lifecycle, rust]

# Dependency graph
requires:
  - phase: 21-mux-extraction
    provides: SessionMux struct, Tab struct, Session struct, TabId/SessionId types
provides:
  - Tab title field for display in tab bar
  - Tab CRUD methods (add_tab, close_tab, activate_tab, next_tab, prev_tab)
  - Tab read accessors (tab_count, active_tab_index, tabs)
affects: [23-02-tab-bar-rendering, 23-03-main-integration, 24-split-panes]

# Tech tracking
tech-stack:
  added: []
  patterns: [tab-index-management-with-wraparound, insert-after-active-pattern]

key-files:
  created: []
  modified:
    - crates/glass_mux/src/tab.rs
    - crates/glass_mux/src/session_mux.rs

key-decisions:
  - "Tab title cloned from session.title at creation time (not live-linked)"
  - "add_tab inserts after active tab (not at end) for natural tab ordering"
  - "close_tab adjusts active_tab index to prevent out-of-bounds after removal"
  - "Test helper uses synthetic tabs without Session instances for unit testing tab index logic"

patterns-established:
  - "Insert-after-active: new tabs appear right of current tab, matching browser convention"
  - "Wraparound navigation: next/prev use modulo arithmetic for seamless cycling"

requirements-completed: [TAB-01, TAB-02, TAB-03]

# Metrics
duration: 2min
completed: 2026-03-07
---

# Phase 23 Plan 01: Tab Lifecycle Summary

**Tab CRUD methods on SessionMux with title field and 14 unit tests covering add/close/activate/cycle with wraparound**

## Performance

- **Duration:** 2 min
- **Started:** 2026-03-07T00:53:38Z
- **Completed:** 2026-03-07T00:55:49Z
- **Tasks:** 1
- **Files modified:** 2

## Accomplishments
- Added `pub title: String` field to Tab struct for tab bar display
- Implemented 8 new methods on SessionMux: add_tab, close_tab, activate_tab, next_tab, prev_tab, tab_count, active_tab_index, tabs
- 14 new unit tests covering all tab lifecycle operations including edge cases (empty mux, wraparound, close-adjusts-index)
- Full project compiles clean (cargo check passes)

## Task Commits

Each task was committed atomically (TDD):

1. **Task 1 RED: Add failing tests for tab CRUD** - `8bd9d77` (test)
2. **Task 1 GREEN: Implement tab CRUD methods** - `fdd0e50` (feat)

## Files Created/Modified
- `crates/glass_mux/src/tab.rs` - Added `pub title: String` field to Tab struct
- `crates/glass_mux/src/session_mux.rs` - Added 8 tab management methods + 14 unit tests + test helper

## Decisions Made
- Tab title is cloned from session.title at creation time rather than being a live reference -- keeps ownership simple
- add_tab inserts after the currently active tab (browser convention) rather than appending at end
- close_tab returns the removed Session for cleanup by caller; adjusts active_tab to prevent out-of-bounds
- Unit tests use synthetic test_mux helper that creates Tab entries without real Session instances, keeping tests fast and dependency-free

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Tab data model and lifecycle methods ready for Plan 02 (tab bar rendering)
- Plan 03 (main.rs integration) can wire up keyboard shortcuts to add_tab/close_tab/next_tab/prev_tab
- All methods tested and compiling

---
*Phase: 23-tabs*
*Completed: 2026-03-07*

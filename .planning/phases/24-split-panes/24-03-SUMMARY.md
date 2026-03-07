---
phase: 24-split-panes
plan: 03
subsystem: ui
tags: [split-panes, keyboard-shortcuts, mouse-input, pty-resize, winit]

# Dependency graph
requires:
  - phase: 24-split-panes/02
    provides: "Tab restructure with SplitNode tree, per-pane scissor-clipped rendering"
provides:
  - "Keyboard shortcuts for split/close/navigate/resize panes"
  - "Mouse click pane focus routing"
  - "Per-pane PTY resize on split/close/window-resize"
  - "Last-pane-close-closes-tab lifecycle"
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "resize_all_panes helper for per-pane PTY dimension sync"
    - "pane_at_position hit-test for mouse click routing"
    - "Alt+Arrow focus navigation via find_neighbor"
    - "Alt+Shift+Arrow ratio resize via resize_focused_split"

key-files:
  created: []
  modified:
    - src/main.rs
    - crates/glass_mux/src/session_mux.rs
    - crates/glass_renderer/src/frame.rs
    - crates/glass_renderer/src/grid_renderer.rs
    - crates/glass_terminal/src/block_manager.rs

key-decisions:
  - "Ctrl+Shift+W disambiguated: close pane if multi-pane, close tab if single pane"
  - "Per-pane PTY resize uses pane viewport dimensions divided by cell size (not full window)"
  - "Block decoration reflow triggered on resize to prevent stale decorations in resized panes"

patterns-established:
  - "resize_all_panes: centralized PTY resize after any layout change (split/close/window-resize)"
  - "pane_at_position: viewport hit-test for mouse-to-pane routing"

requirements-completed: [SPLIT-09, SPLIT-11]

# Metrics
duration: ~25min
completed: 2026-03-07
---

# Phase 24 Plan 03: Split Pane Interaction Summary

**Full split pane keyboard/mouse interaction with per-pane PTY resize, focus navigation, ratio adjustment, and pane lifecycle management**

## Performance

- **Duration:** ~25 min (across two sessions with checkpoint)
- **Started:** 2026-03-07T03:10:00Z
- **Completed:** 2026-03-07T03:35:00Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments

- Wired all split pane keyboard shortcuts: Ctrl+Shift+D/E for split, Alt+Arrow for focus, Alt+Shift+Arrow for resize, Ctrl+Shift+W for close
- Mouse click routes focus to clicked pane via viewport hit-testing
- Per-pane PTY resize sends correct cell dimensions after split/close/window-resize (SPLIT-09)
- Last-pane-close correctly closes the tab (SPLIT-11)
- Fixed multi-pane text rendering and block decoration reflow bugs discovered during verification

## Task Commits

Each task was committed atomically:

1. **Task 1: Keyboard shortcuts, mouse routing, and per-pane PTY resize** - `8d2f17a` (feat)
2. **Task 2: Verify full split pane interaction (bug fixes)** - `bd67aa7` (fix)

## Files Created/Modified

- `src/main.rs` - Keyboard shortcuts, mouse click routing, resize_all_panes helper, pane lifecycle
- `crates/glass_mux/src/session_mux.rs` - set_focused_pane, resize_focused_split, SPLIT-11 test
- `crates/glass_renderer/src/frame.rs` - Multi-pane rendering fix
- `crates/glass_renderer/src/grid_renderer.rs` - Grid renderer pane-aware adjustment
- `crates/glass_terminal/src/block_manager.rs` - Block decoration reflow on resize

## Decisions Made

- Ctrl+Shift+W disambiguated: closes focused pane when multiple panes exist, closes tab when single pane (preserves existing tab-close behavior)
- Per-pane PTY resize uses pane viewport dimensions divided by cell size, not full window dimensions (research Pitfall 2)
- Block decoration reflow triggered on resize to prevent stale decorations in resized panes

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed multi-pane text rendering and block decoration reflow**
- **Found during:** Task 2 (human verification)
- **Issue:** Text rendering had artifacts in multi-pane mode; block decorations became stale after pane resize
- **Fix:** Fixed frame.rs rendering for multi-pane, adjusted grid_renderer.rs, added reflow trigger in block_manager.rs
- **Files modified:** crates/glass_renderer/src/frame.rs, crates/glass_renderer/src/grid_renderer.rs, crates/glass_terminal/src/block_manager.rs, src/main.rs
- **Verification:** Human verified all split pane interactions work correctly
- **Committed in:** bd67aa7

---

**Total deviations:** 1 auto-fixed (1 bug fix)
**Impact on plan:** Bug fix was necessary for correct multi-pane rendering. No scope creep.

## Issues Encountered

- Multi-pane text rendering required fixes to both the frame renderer and grid renderer to account for per-pane viewport offsets
- Block decorations did not reflow when panes were resized, requiring a reflow trigger in block_manager.rs

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Split pane system is fully interactive and complete
- Phase 24 (Split Panes) is now complete with all 3 plans finished
- All v2.0 milestone features (cross-platform, tabs, split panes) are delivered

## Self-Check: PASSED

All 5 modified files verified present. Both commit hashes (8d2f17a, bd67aa7) verified in git log.

---
*Phase: 24-split-panes*
*Completed: 2026-03-07*

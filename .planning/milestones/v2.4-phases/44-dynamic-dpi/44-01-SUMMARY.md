---
phase: 44-dynamic-dpi
plan: 01
subsystem: renderer
tags: [dpi, scale-factor, winit, wgpu, glyphon, font-metrics]

# Dependency graph
requires:
  - phase: 39-per-cell-rendering
    provides: "GridRenderer with scale_factor parameter and Metrics-based DPI scaling"
provides:
  - "Dynamic DPI handler replacing ScaleFactorChanged stub"
  - "Scale factor unit tests validating cell dimension scaling"
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "ScaleFactorChanged mirrors Resized handler pattern for consistency"

key-files:
  created: []
  modified:
    - src/main.rs
    - crates/glass_renderer/src/grid_renderer.rs

key-decisions:
  - "Full handler (not lean): rebuild fonts AND resize surface/PTYs in ScaleFactorChanged for cross-platform safety"

patterns-established:
  - "DPI changes handled via update_font() + resize() + PTY resize, same as config hot-reload"

requirements-completed: [DPI-01, DPI-02]

# Metrics
duration: 5min
completed: 2026-03-11
---

# Phase 44 Plan 01: Dynamic DPI Summary

**Full ScaleFactorChanged handler rebuilding font metrics, wgpu surface, and all PTYs on DPI change**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-11T00:18:27Z
- **Completed:** 2026-03-11T00:23:30Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Added unit tests validating scale factor produces proportional cell dimensions and preserves grid alignment
- Replaced ScaleFactorChanged stub with full handler: font rebuild, surface reconfigure, active + background tab PTY resize
- Removed "not yet supported" warning message

## Task Commits

Each task was committed atomically:

1. **Task 1: Add scale factor unit tests to GridRenderer** - `cac3c4c` (test)
2. **Task 2: Implement ScaleFactorChanged handler in main.rs** - `18a7b88` (feat)

## Files Created/Modified
- `crates/glass_renderer/src/grid_renderer.rs` - Added scale_factor_changes_cell_dimensions and scale_factor_preserves_grid_alignment tests
- `src/main.rs` - Replaced ScaleFactorChanged stub with full DPI handler

## Decisions Made
- Used full handler approach (not lean) per RESEARCH.md recommendation: both font rebuild AND surface/PTY resize in ScaleFactorChanged to be safe across all platforms, since Resized event delivery after ScaleFactorChanged is not guaranteed on all platforms

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Dynamic DPI support complete
- Multi-monitor HiDPI transitions now handled automatically
- No blockers or concerns

---
*Phase: 44-dynamic-dpi*
*Completed: 2026-03-11*

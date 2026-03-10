---
phase: 40-grid-alignment
plan: 01
subsystem: renderer
tags: [glyphon, cosmic-text, grid-rendering, font-metrics, per-cell-buffers]

# Dependency graph
requires: []
provides:
  - Per-cell Buffer creation via build_cell_buffers()
  - Font-metric cell height derivation (LayoutRun.line_height)
  - Grid-locked TextArea positioning via build_cell_text_areas_offset()
  - set_monospace_width for glyph snapping
affects: [40-02-PLAN, Phase 41 wide chars, Phase 42 decorations, Phase 43 font fallback]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Per-cell Buffer with set_monospace_width for grid alignment"
    - "Font-metric cell height from LayoutRun.line_height instead of hardcoded multiplier"
    - "Parallel positions vec alongside buffers to prevent index mismatch"
    - "Stack-allocated [u8; 4] char encoding for zero-alloc single-char path"

key-files:
  created: []
  modified:
    - crates/glass_renderer/src/grid_renderer.rs

key-decisions:
  - "Derive cell_height from LayoutRun.line_height.max(physical_font_size).ceil() for safety floor"
  - "Keep old build_text_buffers/build_text_areas_offset as legacy wrappers for Plan 02 migration"
  - "Track cell positions as separate Vec<(usize, i32)> alongside buffers for mismatch prevention"

patterns-established:
  - "Per-cell Buffer: one glyphon Buffer per non-empty terminal cell with set_monospace_width"
  - "Grid-locked positioning: TextArea left/top computed from col*cell_width, line*cell_height"
  - "Never use TextArea.scale for DPI (glyphon issue #117); scale Metrics instead"

requirements-completed: [GRID-01, GRID-02]

# Metrics
duration: 4min
completed: 2026-03-10
---

# Phase 40 Plan 01: GridRenderer Core Rewrite Summary

**Per-cell Buffer rendering with font-metric cell height, eliminating horizontal drift and vertical gaps in TUI apps**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-10T18:48:49Z
- **Completed:** 2026-03-10T18:52:30Z
- **Tasks:** 2
- **Files modified:** 1

## Accomplishments
- Rewrote GridRenderer::new() to derive cell_height from cosmic-text LayoutRun.line_height instead of hardcoded 1.2x multiplier
- Added build_cell_buffers() creating one Buffer per non-empty cell with set_monospace_width for grid snapping
- Added build_cell_text_areas_offset() positioning each cell at exact grid coordinates using parallel positions vec
- 5 new unit tests validating font-metric derivation, cell skipping, and grid positioning
- Full backward compatibility: old API methods preserved for frame.rs until Plan 02 migration

## Task Commits

Each task was committed atomically:

1. **Task 1 RED: Failing tests** - `8572a30` (test)
2. **Task 1 GREEN: Per-cell buffers and font-metric cell height** - `30d173c` (feat)
3. **Task 2: Verification** - no changes needed (compilation, tests, clippy all clean)

## Files Created/Modified
- `crates/glass_renderer/src/grid_renderer.rs` - Rewrote new() for font-metric cell height, added build_cell_buffers() and build_cell_text_areas_offset(), kept legacy wrappers, added 5 unit tests

## Decisions Made
- Used LayoutRun.line_height.max(physical_font_size).ceil() as cell_height with safety floor to prevent too-small heights on unusual fonts
- Kept old build_text_buffers() and build_text_areas_offset() unchanged as legacy methods so frame.rs continues compiling without changes
- Used parallel Vec<(usize, i32)> for cell positions instead of embedding in buffer struct, keeping the API clean for Plan 02 migration

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- New per-cell methods (build_cell_buffers, build_cell_text_areas_offset) ready for Plan 02 to migrate frame.rs call sites
- Old API preserved so frame.rs compiles unchanged
- cell_height now reflects font metrics, which will automatically propagate to rect renderers and sub-renderers via cell_size()

---
*Phase: 40-grid-alignment*
*Completed: 2026-03-10*

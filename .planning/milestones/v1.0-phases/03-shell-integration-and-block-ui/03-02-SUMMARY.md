---
phase: 03-shell-integration-and-block-ui
plan: 02
subsystem: renderer
tags: [wgpu, block-rendering, status-bar, gpu-pipeline, glyphon]

# Dependency graph
requires:
  - phase: 03-shell-integration-and-block-ui
    provides: "BlockManager, Block, StatusState, GitInfo, format_duration from Plan 01"
  - phase: 02-terminal-core
    provides: "FrameRenderer, GridRenderer, RectRenderer, GlyphCache GPU pipeline"
provides:
  - "BlockRenderer for separator lines, exit code badges, and duration labels"
  - "StatusBarRenderer for bottom-pinned CWD and git info display"
  - "Extended FrameRenderer.draw_frame with blocks and status parameters"
affects: [03-04-wiring]

# Tech tracking
tech-stack:
  added: []
  patterns: [two-phase overlay buffer for borrow-checker-safe text areas, stateless renderer helpers]

key-files:
  created:
    - crates/glass_renderer/src/block_renderer.rs
    - crates/glass_renderer/src/status_bar.rs
  modified:
    - crates/glass_renderer/src/frame.rs
    - crates/glass_renderer/src/grid_renderer.rs
    - crates/glass_renderer/src/lib.rs
    - src/main.rs

key-decisions:
  - "Two-phase overlay buffer pattern: build all Buffers first (mutable), then create TextAreas (immutable borrows) to satisfy Rust borrow checker"
  - "Badge text uses 'OK' for exit 0 and 'X' for non-zero (ASCII-safe, no Unicode dependency)"
  - "Status bar overlaps last terminal line for now; PTY resize adjustment deferred to Plan 04"
  - "GridRenderer.font_family made pub to allow overlay text buffer creation in FrameRenderer"

patterns-established:
  - "Overlay metadata struct pattern: store layout info (left, top, color) alongside buffers, then iterate both for TextArea creation"
  - "Backward-compatible draw_frame: empty blocks slice and None status produce identical Phase 2 rendering"

requirements-completed: [BLOK-01, BLOK-02, BLOK-03, STAT-01, STAT-02]

# Metrics
duration: 4min
completed: 2026-03-05
---

# Phase 3 Plan 2: Block Decoration and Status Bar Rendering Summary

**BlockRenderer for separator lines/exit badges/duration labels and StatusBarRenderer for bottom-pinned CWD/git display, integrated into FrameRenderer GPU pipeline**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-05T05:37:32Z
- **Completed:** 2026-03-05T05:41:11Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- BlockRenderer generates horizontal separator rects, colored exit code badges (green/red), and text labels for badge symbols and duration display
- StatusBarRenderer produces bottom-pinned background rect with CWD path and git branch/dirty count text
- FrameRenderer extended with two-phase overlay buffer pattern that builds all buffers before creating TextAreas, solving Rust borrow checker constraints
- Backward compatible: empty blocks + None status produces identical Phase 2 rendering

## Task Commits

Each task was committed atomically:

1. **Task 1: Create BlockRenderer and StatusBarRenderer** - `cdc69ad` (feat)
2. **Task 2: Integrate block and status rendering into FrameRenderer** - `6f868af` (feat)

## Files Created/Modified
- `crates/glass_renderer/src/block_renderer.rs` - BlockRenderer with build_block_rects and build_block_text
- `crates/glass_renderer/src/status_bar.rs` - StatusBarRenderer with build_status_rects and build_status_text
- `crates/glass_renderer/src/frame.rs` - Extended FrameRenderer with block/status rendering in draw pipeline
- `crates/glass_renderer/src/grid_renderer.rs` - Made font_family field pub for overlay text access
- `crates/glass_renderer/src/lib.rs` - Added block_renderer and status_bar module declarations with re-exports
- `src/main.rs` - Updated draw_frame call with empty blocks/None status defaults

## Decisions Made
- Two-phase overlay buffer pattern: build all glyphon Buffers in a mutable phase, store layout metadata in a parallel Vec, then create TextAreas from both in an immutable phase. This avoids the borrow checker error from interleaving push() and get() on the same Vec.
- Badge text uses ASCII "OK"/"X" rather than Unicode checkmark/cross for maximum font compatibility.
- Status bar renders overlapping the last terminal line; adjusting PTY rows to account for the status bar height is deferred to Plan 04 (wiring).
- GridRenderer.font_family changed from private to pub so FrameRenderer can create overlay text buffers with the same font family.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Borrow checker error in overlay buffer/TextArea interleaving**
- **Found during:** Task 2
- **Issue:** Original approach pushed to overlay_buffers then immediately created TextAreas referencing them, causing simultaneous mutable and immutable borrows
- **Fix:** Introduced two-phase approach with OverlayMeta struct to separate buffer mutation from TextArea creation
- **Files modified:** crates/glass_renderer/src/frame.rs
- **Committed in:** 6f868af (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Essential fix for Rust borrow checker compliance. Same functionality, cleaner ownership model.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- BlockRenderer and StatusBarRenderer are public in glass_renderer, ready for:
  - Plan 04 (wiring) to pass real BlockManager visible_blocks and StatusState to draw_frame
  - Plan 04 to adjust PTY resize to subtract status bar height from terminal rows

---
*Phase: 03-shell-integration-and-block-ui*
*Completed: 2026-03-05*

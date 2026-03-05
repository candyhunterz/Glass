---
phase: 02-terminal-core
plan: 02
subsystem: renderer
tags: [wgpu, glyphon, gpu-text-rendering, instanced-rendering, wgsl, cosmic-text]

# Dependency graph
requires:
  - phase: 02-terminal-core/01
    provides: "GridSnapshot with color resolution, GlyphCache with glyphon pipeline"
  - phase: 01-scaffold
    provides: "wgpu surface, PTY integration, winit event loop"
provides:
  - "RectRenderer: instanced wgpu pipeline for colored quads (backgrounds, cursor, selection)"
  - "GridRenderer: GridSnapshot-to-TextArea conversion with per-cell color and font attributes"
  - "FrameRenderer: orchestrated clear -> rects -> text -> present GPU rendering pipeline"
  - "Font-metrics-based window resize with PTY and Term grid reflow"
affects: [02-terminal-core/03, 03-shell-intelligence]

# Tech tracking
tech-stack:
  added: [bytemuck]
  patterns: [instanced-wgpu-rendering, wgsl-inline-shaders, font-metrics-cell-sizing, lock-minimizing-snapshot]

key-files:
  created:
    - crates/glass_renderer/src/rect_renderer.rs
    - crates/glass_renderer/src/grid_renderer.rs
    - crates/glass_renderer/src/frame.rs
  modified:
    - crates/glass_renderer/src/surface.rs
    - crates/glass_renderer/src/lib.rs
    - crates/glass_renderer/Cargo.toml
    - src/main.rs

key-decisions:
  - "Instanced WGSL quad rendering for cell backgrounds — 6 vertices per instance, no index buffer"
  - "Per-line cosmic_text::Buffer with set_rich_text for per-character fg color and font weight/style"
  - "Font metrics cell sizing via measuring 'M' advance width — replaces hardcoded 8x16"

patterns-established:
  - "RectInstance Pod/Zeroable pattern for GPU instance buffer upload"
  - "FrameRenderer orchestration: clear -> rect backgrounds -> glyphon text -> present"
  - "GridSnapshot lock-minimizing pattern: brief Term lock for snapshot, GPU draw outside lock"

requirements-completed: [RNDR-02, RNDR-03, RNDR-04, CORE-07]

# Metrics
duration: 8min
completed: 2026-03-05
---

# Phase 2 Plan 02: GPU Text Rendering Pipeline Summary

**Instanced wgpu rect renderer, glyphon grid-to-text conversion, and FrameRenderer orchestrating visible terminal output with colors, cursor, and font-metrics resize**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-05T04:24:00Z
- **Completed:** 2026-03-05T04:32:00Z
- **Tasks:** 3
- **Files modified:** 7

## Accomplishments
- RectRenderer draws cell backgrounds and cursor shapes via instanced wgpu pipeline with inline WGSL shaders
- GridRenderer converts GridSnapshot cells to per-line glyphon TextAreas with per-character color, bold, and italic attributes
- FrameRenderer orchestrates the full clear -> rects -> text -> present pipeline
- main.rs wired to render actual terminal output on the GPU surface — Glass is now a visible terminal
- Window resize computes cell dimensions from font metrics and reflows both PTY (ConPTY) and Term grid
- Cursor renders as block, beam, or underline shape at correct grid position

## Task Commits

Each task was committed atomically:

1. **Task 1: Create RectRenderer, GridRenderer, and FrameRenderer** - `af29c8f` (feat)
2. **Task 2: Wire rendering pipeline into main.rs with font-metrics resize** - `ec307f0` (feat)
3. **Task 3: Verify GPU text rendering pipeline** - checkpoint:human-verify approved

## Files Created/Modified
- `crates/glass_renderer/src/rect_renderer.rs` - Instanced wgpu pipeline for colored rectangles (backgrounds, cursor, selection)
- `crates/glass_renderer/src/grid_renderer.rs` - Converts GridSnapshot cells to glyphon TextAreas and RectInstances
- `crates/glass_renderer/src/frame.rs` - FrameRenderer orchestrating clear -> rects -> text -> present
- `crates/glass_renderer/src/surface.rs` - Added accessor methods (device, queue, surface_format, surface_config)
- `crates/glass_renderer/src/lib.rs` - Added module declarations and re-exports
- `crates/glass_renderer/Cargo.toml` - Added bytemuck dependency
- `src/main.rs` - Wired FrameRenderer, font-metrics resize, snapshot-based rendering

## Decisions Made
- Instanced WGSL quad rendering for cell backgrounds: 6 vertices per instance with no index buffer, viewport uniform for pixel-to-NDC conversion
- Per-line cosmic_text::Buffer with set_rich_text for per-character foreground color and font weight/style attributes
- Font metrics cell sizing via measuring 'M' advance width replaces hardcoded 8x16 cell dimensions
- Term lock held only during snapshot_term() call; GPU draw_frame() runs outside lock (lock-minimizing pattern)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- GPU text rendering pipeline complete — terminal output visible with colors and cursor
- Ready for Plan 03 (input handling / escape sequences) to complete the terminal core
- FrameRenderer API stable for future enhancements (selection highlighting, scrollback rendering)

## Self-Check: PASSED

All files verified present. Both task commits (af29c8f, ec307f0) confirmed in git history.

---
*Phase: 02-terminal-core*
*Completed: 2026-03-05*

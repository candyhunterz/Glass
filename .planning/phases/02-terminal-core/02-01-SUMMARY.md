---
phase: 02-terminal-core
plan: 01
subsystem: terminal, renderer
tags: [alacritty_terminal, glyphon, color-resolution, 256-color, grid-snapshot, font-system]

# Dependency graph
requires:
  - phase: 01-scaffold
    provides: wgpu surface, PTY spawn, EventProxy, workspace structure
provides:
  - GridSnapshot struct for lock-minimizing terminal grid extraction
  - RenderedCell struct with resolved RGB colors
  - resolve_color function handling Spec/Named/Indexed with DIM/BOLD/INVERSE
  - 256-color palette (ANSI, 6x6x6 cube, grayscale ramp)
  - GlyphCache wrapping all glyphon text rendering state
  - DefaultColors struct for terminal fg/bg defaults
  - snapshot_term function for extracting renderable content from Term
affects: [02-terminal-core plan 02 (rendering), 02-terminal-core plan 03 (input/clipboard)]

# Tech tracking
tech-stack:
  added: [glyphon 0.10.0, arboard 3.6.1, cosmic-text 0.15.0 (transitive)]
  patterns: [lock-minimizing GridSnapshot, color resolution pipeline, GlyphCache initialization]

key-files:
  created:
    - crates/glass_terminal/src/grid_snapshot.rs
    - crates/glass_renderer/src/glyph_cache.rs
  modified:
    - Cargo.toml
    - Cargo.lock
    - crates/glass_terminal/Cargo.toml
    - crates/glass_terminal/src/lib.rs
    - crates/glass_terminal/src/pty.rs
    - crates/glass_renderer/Cargo.toml
    - crates/glass_renderer/src/lib.rs

key-decisions:
  - "RenderableCursor does not implement Debug; GridSnapshot omits derive(Debug)"
  - "xterm default ANSI palette used for 256-color fallback (matches standard terminal behavior)"
  - "DefaultColors fg=204,204,204 bg=26,26,26 matching GlassRenderer clear color"

patterns-established:
  - "GridSnapshot pattern: brief lock on Term -> copy all renderable data -> release lock -> render freely"
  - "Color resolution pipeline: Color -> resolve_color(Color, Colors, DefaultColors, Flags) -> Rgb"
  - "GlyphCache wraps all glyphon state: FontSystem, SwashCache, Cache, TextAtlas, TextRenderer, Viewport"

requirements-completed: [CORE-02, CORE-05, CORE-08, RNDR-02]

# Metrics
duration: 5min
completed: 2026-03-05
---

# Phase 2 Plan 1: Data Pipeline Foundation Summary

**GridSnapshot with 256-color resolution pipeline and GlyphCache glyphon text rendering initialization**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-05T04:12:26Z
- **Completed:** 2026-03-05T04:17:44Z
- **Tasks:** 2
- **Files modified:** 9

## Accomplishments
- GridSnapshot extracts all cell data (char, fg, bg, flags, zerowidth) from Term under a brief lock
- Color resolution handles Named (with DIM/BOLD variants), Spec (truecolor), and Indexed (256-color)
- INVERSE flag swaps fg and bg during resolution; WIDE_CHAR_SPACER preserved for renderer skip
- GlyphCache initializes FontSystem, TextAtlas, TextRenderer, SwashCache, Cache, Viewport
- Scrollback history explicitly configured to 10,000 lines
- Workspace dependencies added: glyphon 0.10.0, arboard 3.x

## Task Commits

Each task was committed atomically:

1. **Task 1: Add workspace dependencies and create GridSnapshot with color resolution** - `87b89a4` (feat)
2. **Task 2: Create GlyphCache with glyphon initialization** - `50ff4e8` (feat)

## Files Created/Modified
- `crates/glass_terminal/src/grid_snapshot.rs` - GridSnapshot, RenderedCell, resolve_color, default_indexed_color, snapshot_term, DefaultColors + 10 tests
- `crates/glass_renderer/src/glyph_cache.rs` - GlyphCache wrapping all glyphon state for text rendering
- `Cargo.toml` - Added glyphon 0.10.0 and arboard 3 workspace dependencies
- `crates/glass_terminal/Cargo.toml` - Added arboard.workspace = true
- `crates/glass_renderer/Cargo.toml` - Added glyphon.workspace = true
- `crates/glass_terminal/src/lib.rs` - Added grid_snapshot module and re-exports
- `crates/glass_terminal/src/pty.rs` - Explicit scrollback_history: 10_000 in TermConfig
- `crates/glass_renderer/src/lib.rs` - Added glyph_cache module and GlyphCache re-export

## Decisions Made
- RenderableCursor from alacritty_terminal does not implement Debug, so GridSnapshot omits derive(Debug)
- Used standard xterm ANSI color palette for 256-color default_indexed_color fallback
- DefaultColors fg/bg values match the existing GlassRenderer dark gray clear color (0.1, 0.1, 0.1 -> Rgb 26,26,26)
- TermConfig scrolling_history already defaults to 10,000 in alacritty_terminal 0.25.1, but made explicit per plan

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] RenderableCursor missing Debug trait**
- **Found during:** Task 1 (GridSnapshot compilation)
- **Issue:** GridSnapshot derived Debug but RenderableCursor (from alacritty_terminal) does not implement Debug
- **Fix:** Removed derive(Debug) from GridSnapshot struct
- **Files modified:** crates/glass_terminal/src/grid_snapshot.rs
- **Verification:** cargo build --workspace succeeds
- **Committed in:** 87b89a4

**2. [Rule 1 - Bug] Unused import and unreachable pattern warnings**
- **Found during:** Task 1 (initial compilation)
- **Issue:** Imported Indexed type not used in module scope; exhaustive NamedColor match had unreachable wildcard
- **Fix:** Removed unused Indexed import; removed wildcard arm from default_named_color match
- **Files modified:** crates/glass_terminal/src/grid_snapshot.rs
- **Verification:** cargo build --workspace succeeds with zero warnings in grid_snapshot.rs
- **Committed in:** 87b89a4

---

**Total deviations:** 2 auto-fixed (2 bugs)
**Impact on plan:** Minor compilation fixes. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- GridSnapshot and GlyphCache are ready for Plan 02 (grid-to-glyphon rendering bridge)
- RenderedCell provides resolved RGB colors that map directly to glyphon Color::rgba()
- GlyphCache.font_system ready for Buffer creation with Metrics
- Plan 03 can use arboard dependency already added to glass_terminal

---
*Phase: 02-terminal-core*
*Completed: 2026-03-05*

# Phase 42: Text Decorations - Research

**Researched:** 2026-03-10
**Domain:** GPU terminal rendering - underline and strikethrough decorations
**Confidence:** HIGH

## Summary

Text decorations (underline and strikethrough) are rendered as thin colored rectangles positioned relative to each cell's grid coordinates. The existing `RectInstance` pipeline and `build_rects` pattern in `GridRenderer` already handles cursor underlines, selection highlights, and cell backgrounds -- adding decoration rects is a natural extension of this same system.

alacritty_terminal 0.25.1 already parses SGR 4 (underline) and SGR 9 (strikethrough) and exposes them as `Flags::UNDERLINE` (0x0008) and `Flags::STRIKEOUT` (0x0200) on each cell. These flags are already preserved through `snapshot_term()` into `RenderedCell.flags`. No terminal/VTE changes are needed -- this is purely a renderer-side feature.

**Primary recommendation:** Add a `build_decoration_rects` method to `GridRenderer` that iterates cells, checks for `UNDERLINE`/`STRIKEOUT` flags, and emits `RectInstance` lines at the appropriate vertical positions. Append these to the existing rect pipeline in `draw_frame()`.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| DECO-01 | Underlined text renders with a 1px line below the baseline | `Flags::UNDERLINE` is already set by alacritty_terminal; render a 1px-high `RectInstance` at `y + cell_height - 1.0` (or baseline-derived position) using the cell's fg color |
| DECO-02 | Strikethrough text renders with a 1px line through the middle | `Flags::STRIKEOUT` is already set by alacritty_terminal; render a 1px-high `RectInstance` at `y + cell_height / 2.0` using the cell's fg color |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| alacritty_terminal | =0.25.1 | VT parsing, cell flags (UNDERLINE, STRIKEOUT) | Already in use, provides the flags |
| wgpu | 28.0 | GPU rendering via RectRenderer | Already in use for all rect drawing |
| glyphon | 0.10 | Text rendering (not needed for decorations) | Decorations are rects, not text |

### Supporting
No new dependencies needed. Zero new crates.

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Rect-based decorations | glyphon text decorations / cosmic-text underline | glyphon/cosmic-text have no decoration API; rect approach matches how cursor underline already works |
| Per-cell decoration rects | Run-length merged decoration rects | Run-length merging is more complex; per-cell is simpler, matches existing per-cell pattern, and rect renderer handles thousands of instances efficiently |

## Architecture Patterns

### Rendering Pipeline (Existing)
```
draw_frame():
  1. build_rects()           -> cell backgrounds + cursor
  1a2. selection rects
  1b. block decoration rects
  1c. status bar rects
  1d. search overlay rects
  1e. pipeline panel rects
  2. rect_renderer.prepare() -> upload all rects to GPU
  3. build_cell_buffers()    -> text buffers
  ...
  7. rect_renderer.draw()    -> render all rects
  8. text_renderer.draw()    -> render text on top
```

Decoration rects should be inserted between step 1 (backgrounds) and step 2 (upload). They render UNDER text (same layer as backgrounds/cursor), which is correct -- underlines and strikethrough lines should appear behind the text glyphs.

### Pattern: Decoration Rect Generation
**What:** New method `build_decoration_rects(&self, snapshot: &GridSnapshot) -> Vec<RectInstance>` on `GridRenderer`
**When to use:** Every frame, for all cells with UNDERLINE or STRIKEOUT flags
**Example:**
```rust
// In GridRenderer
pub fn build_decoration_rects(&self, snapshot: &GridSnapshot) -> Vec<RectInstance> {
    let mut rects = Vec::new();
    let line_offset = snapshot.display_offset as i32;

    for cell in &snapshot.cells {
        // Skip spacers (decoration handled by primary cell)
        if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            continue;
        }

        let is_wide = cell.flags.contains(Flags::WIDE_CHAR);
        let rect_width = if is_wide { self.cell_width * 2.0 } else { self.cell_width };
        let x = cell.point.column.0 as f32 * self.cell_width;
        let y = (cell.point.line.0 + line_offset) as f32 * self.cell_height;
        let color = rgb_to_color(cell.fg, 1.0);

        if cell.flags.contains(Flags::UNDERLINE) {
            // 1px line at bottom of cell (just above cell boundary)
            rects.push(RectInstance {
                pos: [x, y + self.cell_height - 1.0, rect_width, 1.0],
                color,
            });
        }

        if cell.flags.contains(Flags::STRIKEOUT) {
            // 1px line at vertical center of cell
            rects.push(RectInstance {
                pos: [x, y + (self.cell_height / 2.0).floor(), rect_width, 1.0],
                color,
            });
        }
    }

    rects
}
```

### Pattern: Offset Variant for Split Panes
**What:** Also need `build_decoration_rects_offset` that adds x/y offset (same pattern as `build_rects_offset`)
**When to use:** Multi-pane rendering in `draw_frame_split_pane`

### Anti-Patterns to Avoid
- **Using text rendering for decorations:** glyphon has no underline/strikethrough API. Decorations must be rects.
- **Forgetting wide char double-width:** Decoration line width must be `cell_width * 2.0` for WIDE_CHAR cells, matching the existing bg/cursor pattern.
- **Drawing decorations on spacer cells:** Spacer cells must be skipped (primary WIDE_CHAR cell handles the full width).
- **Using 2px height for decoration lines:** The requirements specify "1px line". The cursor underline uses 2px, but text decorations should be 1px for correct appearance.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| VT escape parsing for SGR 4/9 | Custom SGR parser | alacritty_terminal Flags | Already parsed and exposed as UNDERLINE/STRIKEOUT flags |
| GPU rect pipeline | Custom decoration shader | Existing RectRenderer | Instanced rect pipeline already handles thousands of rects per frame |
| Color resolution | Manual color lookup | Cell fg color (already resolved in snapshot) | RenderedCell.fg is already resolved RGB |

## Common Pitfalls

### Pitfall 1: Decoration on Space-Only Cells
**What goes wrong:** Underline/strikethrough flags can be set on space characters (e.g., `echo -e "\e[4m   \e[0m"` produces underlined spaces)
**Why it happens:** `build_cell_buffers` skips space cells (no text buffer needed), but decorations should still render on spaces
**How to avoid:** `build_decoration_rects` iterates ALL cells (not just those with text buffers), checking flags on every cell including spaces
**Warning signs:** Underlined spaces don't show the underline

### Pitfall 2: Underline Position Too Low
**What goes wrong:** Underline at `y + cell_height` would be at the very bottom pixel of the cell, potentially overlapping with the next row
**Why it happens:** Off-by-one in vertical positioning
**How to avoid:** Use `y + cell_height - 1.0` for a 1px underline, keeping it within the cell boundary (same principle as cursor underline at `y + cell_height - 2.0`)

### Pitfall 3: Missing Offset in Split Pane Rendering
**What goes wrong:** Decorations render at wrong position in split panes
**Why it happens:** `draw_frame_split_pane` applies x/y offsets to rects and text areas; decorations need the same offset
**How to avoid:** Either use `build_decoration_rects_offset` or add offset after building, matching the `build_rects_offset` pattern

### Pitfall 4: Forgetting to Update draw_frame_split_pane
**What goes wrong:** Decorations only work in single-pane mode
**Why it happens:** There are separate code paths for single-pane (`draw_frame`) and multi-pane (`draw_frame_split_pane`)
**How to avoid:** Add decoration rects in BOTH frame rendering methods

## Code Examples

### Key alacritty_terminal Flags (from cell.rs)
```rust
// Source: alacritty_terminal 0.25.1 src/term/cell.rs
pub struct Flags: u16 {
    const UNDERLINE                 = 0b0000_0000_0000_1000;  // SGR 4
    const STRIKEOUT                 = 0b0000_0010_0000_0000;  // SGR 9
    const WIDE_CHAR                 = 0b0000_0000_0010_0000;
    const WIDE_CHAR_SPACER          = 0b0000_0000_0100_0000;
    const LEADING_WIDE_CHAR_SPACER  = 0b0000_0100_0000_0000;
    // Future (out of scope for Phase 42):
    const DOUBLE_UNDERLINE          = 0b0000_1000_0000_0000;  // SGR 21
    const UNDERCURL                 = 0b0001_0000_0000_0000;  // SGR 4:3
    const DOTTED_UNDERLINE          = 0b0010_0000_0000_0000;  // SGR 4:4
    const DASHED_UNDERLINE          = 0b0100_0000_0000_0000;  // SGR 4:5
}
```

### Existing Pattern: Cursor Underline (in build_rects)
```rust
// Source: grid_renderer.rs lines 161-171
CursorShape::Underline => {
    rects.push(RectInstance {
        pos: [
            cursor_x,
            cursor_y + self.cell_height - 2.0,
            cursor_cell_width,
            2.0,
        ],
        color: cursor_color,
    });
}
```

### Integration Point: draw_frame (frame.rs)
```rust
// After line ~192 (build_rects), before rect_renderer.prepare():
// Add decoration rects
let mut deco_rects = if grid_y_offset > 0.0 {
    let mut dr = self.grid_renderer.build_decoration_rects(snapshot);
    for rect in &mut dr {
        rect.pos[1] += grid_y_offset;
    }
    dr
} else {
    self.grid_renderer.build_decoration_rects(snapshot)
};
rect_instances.extend(deco_rects);
```

### Test: Underline on Single Cell
```rust
#[test]
fn underline_rect_position_and_size() {
    let mut font_system = FontSystem::new();
    let renderer = GridRenderer::new(&mut font_system, "monospace", 14.0, 1.0);

    let cells = vec![make_cell('A', 0, 0, Flags::UNDERLINE)];
    let snapshot = make_snapshot(cells, 1);
    let rects = renderer.build_decoration_rects(&snapshot);

    assert_eq!(rects.len(), 1);
    let r = &rects[0];
    assert!((r.pos[0] - 0.0).abs() < 0.001, "x at column 0");
    assert!((r.pos[2] - renderer.cell_width).abs() < 0.001, "width = cell_width");
    assert!((r.pos[3] - 1.0).abs() < 0.001, "height = 1px");
    // y should be near bottom of cell
    assert!((r.pos[1] - (renderer.cell_height - 1.0)).abs() < 0.001);
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Per-line text buffers | Per-cell text buffers | Phase 40 (v2.4) | Each cell individually grid-locked |
| No decoration support | Rect-based decorations | Phase 42 (this phase) | Underline + strikethrough via RectInstance |

**Future (deferred per REQUIREMENTS.md):**
- DECO-03 through DECO-07: Double underline, dashed, dotted, undercurl, colored underlines -- all deferred to future release
- The `Flags::DOUBLE_UNDERLINE`, `Flags::UNDERCURL`, `Flags::DOTTED_UNDERLINE`, `Flags::DASHED_UNDERLINE` flags are already exposed by alacritty_terminal but should NOT be handled in this phase

## Open Questions

1. **Underline vertical position: baseline vs bottom-of-cell**
   - What we know: Font metrics include baseline info via cosmic-text LayoutRun, but extracting per-cell baseline adds complexity. The cursor underline uses `cell_height - 2.0` as a simple approach.
   - What's unclear: Whether `cell_height - 1.0` looks correct for all font sizes or if baseline-derived position would be better.
   - Recommendation: Start with `cell_height - 1.0` (simple, matches cursor underline pattern). This satisfies DECO-01 ("below the baseline") since the baseline is always above the bottom of the cell. Can refine later if needed.

2. **Decoration color: fg color vs dedicated underline color**
   - What we know: alacritty_terminal supports `underline_color` via `CellExtra`, but `RenderedCell` does not currently carry this field. DECO-07 (colored underlines) is deferred.
   - Recommendation: Use cell fg color for Phase 42. Colored underlines are explicitly deferred to DECO-07.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in Rust test framework) |
| Config file | Cargo.toml workspace |
| Quick run command | `cargo test -p glass_renderer` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DECO-01 | Underline flag produces 1px rect at bottom of cell | unit | `cargo test -p glass_renderer -- underline` | No - Wave 0 |
| DECO-01 | Underline on wide char produces double-width rect | unit | `cargo test -p glass_renderer -- underline_wide` | No - Wave 0 |
| DECO-01 | Underline on space cell still renders | unit | `cargo test -p glass_renderer -- underline_space` | No - Wave 0 |
| DECO-02 | Strikeout flag produces 1px rect at cell midpoint | unit | `cargo test -p glass_renderer -- strikeout` | No - Wave 0 |
| DECO-02 | Strikeout on wide char produces double-width rect | unit | `cargo test -p glass_renderer -- strikeout_wide` | No - Wave 0 |
| DECO-01+02 | Cell with both flags produces two rects | unit | `cargo test -p glass_renderer -- both_decorations` | No - Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_renderer`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] Tests for `build_decoration_rects` in `grid_renderer.rs` -- all test cases above
- No new framework install needed -- existing cargo test infrastructure covers everything

## Sources

### Primary (HIGH confidence)
- alacritty_terminal 0.25.1 `src/term/cell.rs` -- Flags bitfield definition (UNDERLINE=0x0008, STRIKEOUT=0x0200), read directly from cargo registry
- `crates/glass_renderer/src/grid_renderer.rs` -- existing build_rects pattern, cell iteration, wide char handling
- `crates/glass_renderer/src/frame.rs` -- draw_frame pipeline, rect integration points
- `crates/glass_terminal/src/grid_snapshot.rs` -- RenderedCell struct, snapshot_term preserving flags
- `crates/glass_renderer/src/rect_renderer.rs` -- RectInstance struct definition

### Secondary (MEDIUM confidence)
- None needed -- all findings verified from source code

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - no new dependencies, all existing code verified by reading source
- Architecture: HIGH - follows exact same pattern as cursor underline and cell backgrounds in grid_renderer.rs
- Pitfalls: HIGH - identified from direct code analysis (space cells, split panes, wide chars)

**Research date:** 2026-03-10
**Valid until:** 2026-04-10 (stable -- no external dependencies changing)

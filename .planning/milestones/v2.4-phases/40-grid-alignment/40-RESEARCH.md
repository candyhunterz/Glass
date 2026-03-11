# Phase 40: Grid Alignment - Research

**Researched:** 2026-03-10
**Domain:** GPU text rendering -- per-cell glyph positioning and font-metric line height
**Confidence:** HIGH

## Summary

Phase 40 addresses the two root causes of broken TUI rendering in Glass: horizontal character drift and vertical line gaps. The current `GridRenderer` builds one glyphon `Buffer` per terminal line, letting cosmic-text's shaping engine determine horizontal glyph placement. This causes cumulative horizontal drift because cosmic-text applies proportional spacing, kerning, and fractional advance widths that do not snap to the terminal's fixed-width grid. The second problem is a hardcoded `line_height = font_size * 1.2` multiplier that adds inter-line spacing, preventing box-drawing characters (U+2500-U+257F) from connecting vertically.

The fix requires two changes to `GridRenderer`: (1) replace per-line Buffers with per-cell Buffers so each glyph is positioned at exactly `column * cell_width`, and (2) derive `cell_height` from the font's actual ascent+descent metrics via cosmic-text's `LayoutRun.line_height` instead of the 1.2x multiplier. No new dependencies are needed -- all required APIs exist in the current glyphon 0.10.0 / cosmic-text 0.15.0 stack.

**Primary recommendation:** Rewrite `build_text_buffers()` to create one `Buffer` per non-empty cell with `set_monospace_width(Some(cell_width))`, and change `GridRenderer::new()` to derive line height from font metrics.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| GRID-01 | Terminal renders each glyph at exactly column * cell_width, eliminating horizontal drift | Per-cell Buffer approach positions each cell's TextArea at `col * cell_width`. The `set_monospace_width()` API on cosmic-text Buffer ensures even non-monospace fallback glyphs are resized to cell_width. |
| GRID-02 | Line height derived from font ascent+descent metrics, box-drawing characters connect seamlessly vertically | cosmic-text `LayoutRun.line_height` provides the font's natural line height (ascent + descent). Using this instead of `font_size * 1.2` eliminates inter-line gaps. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| glyphon | 0.10.0 | wgpu text rendering (Buffer, TextArea, TextRenderer) | Already in use; re-exports cosmic-text Buffer with all needed methods |
| cosmic-text | 0.15.0 | Text shaping, layout, font metrics (transitive via glyphon) | Provides Metrics, LayoutRun, set_monospace_width -- all needed for grid alignment |
| wgpu | 28.0.0 | GPU rendering pipeline | Already in use; no changes needed |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| alacritty_terminal | =0.25.1 | VTE parsing, cell Flags | Already provides WIDE_CHAR_SPACER flag used in cell filtering |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Per-cell Buffers | Post-layout x-snap (override LayoutGlyph.x after shaping) | Fragile; fights the shaping engine; complex with RTL/combining chars. Per-cell is simpler and correct. |
| Per-cell Buffers | `set_monospace_width()` on per-line Buffers | Could work but still builds one Buffer per line -- may accumulate fractional rounding errors across 80+ columns. Per-cell with set_monospace_width is belt-and-suspenders correct. |
| Font-metric line height | Keep 1.2x multiplier | Box-drawing gaps are the primary visual bug; the multiplier is the root cause |

**Installation:**
```bash
# No new dependencies needed. All APIs exist in current stack.
```

## Architecture Patterns

### Current vs New Data Flow

**Current (broken):**
```
GridSnapshot
  -> build_text_buffers(): ONE Buffer per line, all chars concatenated as rich text
     cosmic-text shapes the line -> glyph positions accumulate kerning/fractional drift
  -> build_text_areas(): ONE TextArea per line at (0, line_idx * cell_height)
     cell_height = (font_size * scale * 1.2).ceil()  [hardcoded 1.2x multiplier]
```

**New (correct):**
```
GridSnapshot
  -> build_cell_buffers(): ONE Buffer per non-empty cell
     Buffer::new(font_system, metrics)
     buffer.set_monospace_width(font_system, Some(cell_width))  [force grid snap]
     buffer.set_text(font_system, &cell_char, attrs, Shaping::Advanced, None)
     buffer.shape_until_scroll(font_system, false)
  -> build_cell_text_areas(): ONE TextArea per cell at (col * cell_width, line * cell_height)
     cell_height = LayoutRun.line_height  [font-metric derived]
```

### Recommended Project Structure (files changed)
```
crates/glass_renderer/src/
  grid_renderer.rs       # MAJOR REWRITE: per-cell Buffers, font-metric line height
  frame.rs               # MODERATE: update draw_frame() and draw_multi_pane_frame()
                         #           to use new buffer flow
```

### Pattern 1: Font-Metric Cell Height Derivation
**What:** Replace hardcoded 1.2x line height multiplier with actual font metrics
**When to use:** GridRenderer::new() initialization and update_font() rebuild
**Example:**
```rust
// Source: cosmic-text LayoutRun docs (docs.rs/cosmic-text/0.15.0)
pub fn new(font_system: &mut FontSystem, font_family: &str, font_size: f32, scale_factor: f32) -> Self {
    let physical_font_size = font_size * scale_factor;
    // Use font_size as initial line_height to measure natural metrics
    let metrics = Metrics::new(physical_font_size, physical_font_size);

    let mut measure_buf = Buffer::new(font_system, metrics);
    measure_buf.set_size(font_system, Some(1000.0), Some(physical_font_size * 2.0));
    measure_buf.set_text(font_system, "M", &Attrs::new().family(Family::Name(font_family)),
        Shaping::Advanced, None);
    measure_buf.shape_until_scroll(font_system, false);

    let run = measure_buf.layout_runs().next().unwrap();
    let cell_width = run.glyphs.first().map(|g| g.w).unwrap_or(physical_font_size * 0.6);
    // line_height from font metrics -- NOT font_size * 1.2
    let cell_height = run.line_height;

    GridRenderer { cell_width, cell_height, font_size, scale_factor, font_family: font_family.to_string() }
}
```

### Pattern 2: Per-Cell Buffer Creation
**What:** Create one glyphon Buffer per non-empty terminal cell for grid-locked positioning
**When to use:** Every frame in build_cell_buffers()
**Example:**
```rust
// Source: glyphon Buffer API, cosmic-text set_monospace_width
fn build_cell_buffers(&self, font_system: &mut FontSystem, snapshot: &GridSnapshot, buffers: &mut Vec<Buffer>) {
    let physical_font_size = self.font_size * self.scale_factor;
    let metrics = Metrics::new(physical_font_size, self.cell_height);
    let line_offset = snapshot.display_offset as i32;
    let mut char_buf = [0u8; 4]; // stack buffer for char encoding

    for cell in &snapshot.cells {
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) { continue; }
        if cell.c == ' ' && cell.zerowidth.is_empty() { continue; }

        let mut buffer = Buffer::new(font_system, metrics);
        let buf_width = self.cell_width; // 2*cell_width for WIDE_CHAR (Phase 41)
        buffer.set_size(font_system, Some(buf_width), Some(self.cell_height));
        buffer.set_monospace_width(font_system, Some(self.cell_width));

        // Build text: char + zero-width combining chars
        let s = cell.c.encode_utf8(&mut char_buf);
        let mut attrs = Attrs::new()
            .family(Family::Name(&self.font_family))
            .color(GlyphonColor::rgba(cell.fg.r, cell.fg.g, cell.fg.b, 255));
        if cell.flags.contains(Flags::BOLD) { attrs = attrs.weight(Weight::BOLD); }
        if cell.flags.contains(Flags::ITALIC) { attrs = attrs.style(Style::Italic); }

        if cell.zerowidth.is_empty() {
            buffer.set_text(font_system, s, &attrs, Shaping::Advanced, None);
        } else {
            let mut text = String::with_capacity(4 + cell.zerowidth.len() * 4);
            text.push(cell.c);
            for &zw in &cell.zerowidth { text.push(zw); }
            buffer.set_text(font_system, &text, &attrs, Shaping::Advanced, None);
        }

        buffer.shape_until_scroll(font_system, false);
        buffers.push(buffer);
    }
}
```

### Pattern 3: Per-Cell TextArea Positioning
**What:** Position each cell's TextArea at exact grid coordinates
**When to use:** After build_cell_buffers, create TextAreas for rendering
**Example:**
```rust
// Each TextArea is positioned at exact grid pixel coordinates
fn build_cell_text_areas<'a>(&self, buffers: &'a [Buffer], snapshot: &GridSnapshot,
    viewport_width: u32, viewport_height: u32, x_offset: f32, y_offset: f32) -> Vec<TextArea<'a>> {
    let bounds = TextBounds {
        left: x_offset as i32, top: y_offset as i32,
        right: (x_offset as u32 + viewport_width) as i32,
        bottom: (y_offset as u32 + viewport_height) as i32,
    };
    let line_offset = snapshot.display_offset as i32;
    let mut areas = Vec::with_capacity(buffers.len());
    let mut buf_idx = 0;

    for cell in &snapshot.cells {
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) { continue; }
        if cell.c == ' ' && cell.zerowidth.is_empty() { continue; }

        let left = x_offset + cell.point.column.0 as f32 * self.cell_width;
        let top = y_offset + (cell.point.line.0 + line_offset) as f32 * self.cell_height;
        areas.push(TextArea {
            buffer: &buffers[buf_idx],
            left, top, scale: 1.0, bounds,
            default_color: GlyphonColor::rgba(204, 204, 204, 255),
            custom_glyphs: &[],
        });
        buf_idx += 1;
    }
    areas
}
```

### Anti-Patterns to Avoid
- **Per-character String allocation:** Do NOT create a `String` for each cell. Use `char::encode_utf8()` into a stack `[u8; 4]` buffer. Only allocate a String when zero-width combining characters are present.
- **Conditional per-line vs per-cell:** Do NOT use per-line buffers for "simple" lines and per-cell for "complex" lines. Two code paths create maintenance burden and edge case bugs. Always use per-cell.
- **Rebuilding FontSystem on font change:** FontSystem::new() takes 50-200ms for system font discovery. Use the existing `update_font()` path that keeps FontSystem and rebuilds GridRenderer only.
- **Using TextArea.scale for DPI:** glyphon issue #117 documents that `TextArea.scale` breaks cosmic-text alignment. Always scale the `Metrics` (font_size, line_height) instead and keep `TextArea.scale = 1.0`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Monospace glyph snapping | Custom x-position correction after shaping | `Buffer::set_monospace_width(Some(cell_width))` | cosmic-text handles resizing internally; custom correction fights the shaping engine |
| Font metric extraction | Manual font file parsing for ascent/descent | `LayoutRun.line_height` from cosmic-text | cosmic-text already computes correct metrics including hinting adjustments |
| Box-drawing rendering | Custom GPU geometry for box-drawing chars | Font glyphs with correct line_height (for now) | Most monospace fonts render box-drawing correctly when line_height matches font metrics. Custom geometry (BOXD-01/02) is deferred to future requirements. |
| Glyph atlas management | Custom texture atlas for per-cell rendering | glyphon's TextAtlas | Same glyph at different positions shares atlas entries; glyphon handles this automatically |

**Key insight:** The root cause of both GRID-01 and GRID-02 is misuse of the existing cosmic-text API (per-line shaping instead of per-cell, hardcoded multiplier instead of font metrics). The fix is changing how we call the same APIs, not adding new infrastructure.

## Common Pitfalls

### Pitfall 1: Performance Regression from Per-Cell Buffers
**What goes wrong:** Creating ~2000-4000 Buffers per frame instead of ~50 could regress frame time.
**Why it happens:** Buffer::new() + set_text() + shape_until_scroll() cost per cell.
**How to avoid:** Skip empty/space cells (most terminals are <50% filled). Reuse Vec capacity between frames (already done via `text_buffers.clear()`). Use `set_monospace_width()` which may help cosmic-text fast-path single-glyph shaping. Benchmark after implementation -- must stay under 8ms for 120fps budget.
**Warning signs:** Frame time > 5ms in `build_cell_buffers()`, visible stuttering when scrolling through dense TUI output.

### Pitfall 2: Buffer-TextArea Index Mismatch
**What goes wrong:** The buffer Vec and TextArea positions get out of sync because both skip cells with the same filter logic.
**Why it happens:** `build_cell_buffers()` and `build_cell_text_areas()` must iterate cells identically. If one skips a cell the other doesn't, buffer indices misalign.
**How to avoid:** Use the exact same iteration and skip logic in both methods. Consider returning cell metadata (column, line) alongside each buffer to guarantee correct positioning. Or combine both into a single pass.
**Warning signs:** Characters rendered at wrong positions, garbled display.

### Pitfall 3: Cell Height Too Small With Some Fonts
**What goes wrong:** Using `LayoutRun.line_height` directly could produce cell_height smaller than expected for fonts with unusual metrics, causing text overlap or clipping.
**Why it happens:** Some fonts have metrics that don't account for all glyph extents (e.g., accented characters extending above ascent).
**How to avoid:** After computing cell_height from font metrics, ensure it is at least `physical_font_size` (the em height). Use `cell_height = line_height.max(physical_font_size).ceil()` as a safety floor. Test with multiple common terminal fonts (Cascadia Code, Consolas, JetBrains Mono, Fira Code, Source Code Pro).
**Warning signs:** Accented characters clipped at top, glyphs from adjacent lines overlapping.

### Pitfall 4: draw_multi_pane_frame Not Updated
**What goes wrong:** Single-pane rendering works but split panes break.
**Why it happens:** `draw_frame()` and `draw_multi_pane_frame()` both call `build_text_buffers()` and `build_text_areas_offset()`. Both must be updated to use the new per-cell approach.
**How to avoid:** Search for ALL call sites of `build_text_buffers` and `build_text_areas` in frame.rs. Update both single-pane and multi-pane paths.
**Warning signs:** Correct rendering in single pane, garbled rendering after pressing Ctrl+Shift+D to split.

### Pitfall 5: Overlay Buffers Affected by Cell Height Change
**What goes wrong:** Block labels, status bar, tab bar, and search overlay render at wrong positions after cell_height changes.
**Why it happens:** These sub-renderers use `cell_height` from GridRenderer. The cascade via `update_font()` is already wired, but the initial construction path must also use the new metric.
**How to avoid:** The cascade is already correct in `FrameRenderer::with_font_system()` -- it calls `GridRenderer::new()` then passes cell_size to all sub-renderers. Just verify the sub-renderers still render correctly after the cell_height value changes.
**Warning signs:** Status bar or tab bar height is wrong, block separator lines mispositioned.

## Code Examples

### Example 1: Measuring Cell Dimensions from Font Metrics
```rust
// Source: cosmic-text Metrics + LayoutRun API
// Current code (grid_renderer.rs line 48-49):
//   let line_height = (physical_font_size * 1.2).ceil();  // BAD: hardcoded 1.2x
//   let metrics = Metrics::new(physical_font_size, line_height);

// Fixed code:
let metrics = Metrics::new(physical_font_size, physical_font_size); // initial: line_height = font_size
let mut measure_buf = Buffer::new(font_system, metrics);
measure_buf.set_size(font_system, Some(1000.0), Some(physical_font_size * 2.0));
measure_buf.set_text(font_system, "M", &Attrs::new().family(Family::Name(font_family)),
    Shaping::Advanced, None);
measure_buf.shape_until_scroll(font_system, false);

let run = measure_buf.layout_runs().next().expect("font must have 'M' glyph");
let cell_width = run.glyphs.first().map(|g| g.w).unwrap_or(physical_font_size * 0.6);
// Use font's natural line_height instead of 1.2x multiplier
let cell_height = run.line_height.max(physical_font_size).ceil();
```

### Example 2: set_monospace_width for Grid Snapping
```rust
// Source: cosmic-text Buffer::set_monospace_width docs
// Forces all glyphs to be resized to match cell_width
let mut buffer = Buffer::new(font_system, metrics);
buffer.set_size(font_system, Some(cell_width), Some(cell_height));
buffer.set_monospace_width(font_system, Some(cell_width));
// Now even non-monospace fallback glyphs will be exactly cell_width wide
```

### Example 3: Stack-Allocated Char Encoding (Avoid Heap Allocation)
```rust
// For single characters without zero-width combiners:
let mut char_buf = [0u8; 4];
let s = cell.c.encode_utf8(&mut char_buf);
buffer.set_text(font_system, s, &attrs, Shaping::Advanced, None);
// s is a &str pointing into the stack buffer -- zero allocation
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Per-line text buffers | Per-cell text buffers | Standard for terminals (Alacritty, WezTerm, cosmic-term) | Eliminates horizontal drift in TUI apps |
| Hardcoded line height multiplier | Font-metric derived line height | Standard practice | Box-drawing characters connect seamlessly |
| Manual glyph positioning | `set_monospace_width()` API | cosmic-text 0.12+ | Simplifies grid-locked rendering |

**Deprecated/outdated:**
- The 1.2x line height multiplier was a placeholder from Phase 2 (Terminal Core, v1.0). It was adequate for basic text output but breaks TUI applications.

## Open Questions

1. **Performance of per-cell Buffers**
   - What we know: ~2000-4000 Buffers per frame instead of ~50. Each Buffer::new() + set_text() + shape_until_scroll() is small. glyphon atlas caching means GPU cost is similar.
   - What's unclear: Exact wall-clock cost per frame. STATE.md mentions benchmarking after Phase 40.
   - Recommendation: Implement, then benchmark. If >5ms, consider batching runs of identical-attribute ASCII chars into single buffers or caching unchanged cells.

2. **set_monospace_width vs pure per-cell positioning**
   - What we know: `set_monospace_width(Some(cell_width))` tells cosmic-text to resize all glyphs to match cell_width. Per-cell positioning already grid-locks each cell. Using both is belt-and-suspenders.
   - What's unclear: Whether set_monospace_width is needed when each buffer contains only one character. The glyph advance width may not matter if the TextArea is positioned at exact grid coordinates.
   - Recommendation: Use set_monospace_width as insurance. It costs nothing and prevents edge cases where a glyph's rendered width exceeds cell_width and bleeds into adjacent cells.

3. **Font-metric cell_height vs visible box-drawing gaps**
   - What we know: LayoutRun.line_height should give the font's natural height. Most quality monospace fonts (Cascadia Code, JetBrains Mono) have box-drawing chars designed to fill this height.
   - What's unclear: Whether ALL fonts work or if some fonts have box-drawing glyphs that don't fill the full line_height. Custom box-drawing rendering (BOXD-01/02) is explicitly deferred to future requirements.
   - Recommendation: Implement font-metric line height. Test with 3-4 common fonts. If gaps persist with specific fonts, note for future BOXD-01/02 work.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in #[test]) |
| Config file | None (uses Cargo default test runner) |
| Quick run command | `cargo test -p glass_renderer` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| GRID-01 | Glyph positioned at col * cell_width | unit | `cargo test -p glass_renderer grid_alignment` | No -- Wave 0 |
| GRID-02 | Line height from font metrics, not 1.2x | unit | `cargo test -p glass_renderer cell_height_from_metrics` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_renderer`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before /gsd:verify-work

### Wave 0 Gaps
- [ ] `crates/glass_renderer/src/grid_renderer.rs` -- add #[cfg(test)] mod tests with:
  - Test that cell_height is derived from font metrics (not 1.2x multiplier)
  - Test that cell_width matches "M" glyph advance width
  - Test that build_cell_buffers produces correct number of buffers (skips spaces and spacers)
  - Test that build_cell_text_areas positions cells at exact grid coordinates
- [ ] Need to create a FontSystem in tests -- may require test helper that loads a bundled test font or uses system default

## Sources

### Primary (HIGH confidence)
- [glyphon 0.10.0 docs](https://docs.rs/glyphon/0.10.0/glyphon/) -- confirms Buffer re-export from cosmic-text with all methods including set_monospace_width
- [cosmic-text Metrics docs](https://docs.rs/cosmic-text/0.12/cosmic_text/struct.Metrics.html) -- font_size and line_height fields
- [cosmic-text LayoutRun docs](https://docs.rs/cosmic-text/0.12/cosmic_text/struct.LayoutRun.html) -- line_y, line_top, line_height fields
- [cosmic-text Buffer::set_monospace_width](https://docs.rs/cosmic-text/0.12/cosmic_text/struct.Buffer.html) -- forces glyph width matching for monospace rendering
- [cosmic-text LayoutGlyph docs](https://docs.rs/cosmic-text/0.12/cosmic_text/struct.LayoutGlyph.html) -- x, y, w, x_offset, y_offset fields
- Direct source code analysis: grid_renderer.rs, frame.rs, glyph_cache.rs, surface.rs (HIGH confidence)
- Prior v2.4 research: .planning/research/ARCHITECTURE.md, .planning/research/STACK.md (HIGH confidence)

### Secondary (MEDIUM confidence)
- [cosmic-term terminal rendering](https://deepwiki.com/pop-os/cosmic-term/2.3-terminal-widget-and-rendering) -- reference implementation using same stack with set_monospace_width
- [glyphon issue #117](https://github.com/grovesNL/glyphon/issues/117) -- TextArea.scale breaks cosmic-text alignment; must scale Metrics instead

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - verified from Cargo.lock and docs.rs; no new deps needed
- Architecture: HIGH - prior research + source analysis; two well-defined changes to GridRenderer
- Pitfalls: HIGH - performance concern well-documented in STATE.md; buffer mismatch is a known pattern risk

**Research date:** 2026-03-10
**Valid until:** 2026-04-10 (stable stack, no upcoming breaking changes known)

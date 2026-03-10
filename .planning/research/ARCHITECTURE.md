# Architecture Patterns

**Domain:** Rendering correctness features for GPU terminal emulator (v2.4)
**Researched:** 2026-03-09

## Current Architecture Summary

The rendering stack has three layers:

1. **GridSnapshot** (glass_terminal) -- Flat `Vec<RenderedCell>` with per-cell `Point`, `char`, `fg`, `bg`, `Flags`, `zerowidth`. Flags include UNDERLINE, STRIKETHROUGH (STRIKEOUT), DOUBLE_UNDERLINE, UNDERCURL, DOTTED_UNDERLINE, DASHED_UNDERLINE, WIDE_CHAR, WIDE_CHAR_SPACER, BOLD, ITALIC, DIM, INVERSE -- all already preserved from alacritty_terminal.

2. **GridRenderer** (glass_renderer) -- Converts snapshot to GPU primitives:
   - `build_rects()`: cell bg rects at `column * cell_width`, `line * cell_height`
   - `build_text_buffers()`: one glyphon `Buffer` per line with rich text spans (fg color, bold, italic)
   - `build_text_areas()`: positions each line buffer at `line_idx * cell_height`

3. **FrameRenderer** (glass_renderer) -- Orchestrates: rects -> text -> present. Owns GlyphCache (FontSystem, TextAtlas, TextRenderer, SwashCache), GridRenderer, RectRenderer. Has `draw_frame()` for single-pane and `draw_multi_pane_frame()` for split panes.

**Key observation:** The current architecture builds ONE glyphon Buffer per line and lets glyphon handle horizontal glyph placement. This means glyph x-positions are determined by glyphon's shaping, NOT by `column * cell_width`. This is the root cause of horizontal drift in TUI apps.

**Second key observation:** Line height uses `(font_size * scale * 1.2).ceil()` -- a hard-coded 1.2x multiplier. This adds inter-line spacing that prevents box-drawing characters from connecting vertically.

## Integration Analysis: What Changes vs What Gets Added

### Feature 1: Per-Cell Glyph Positioning

**Problem:** glyphon positions glyphs via its own shaping engine. If a glyph's advance width differs from cell_width (common with non-monospace fallback glyphs, combining characters, or rounding), characters drift horizontally. TUI borders misalign.

**What CHANGES (modify existing):**

- `GridRenderer::build_text_buffers()` -- MAJOR REWRITE. Instead of one Buffer per line with all characters concatenated, create one Buffer per cell. Each cell's buffer is positioned at exactly `column * cell_width`.

- `GridRenderer::build_text_areas()` / `build_text_areas_offset()` -- MAJOR REWRITE. Instead of one TextArea per line at `line_idx * cell_height`, produce one TextArea per cell at `(column * cell_width, line_idx * cell_height)`.

- `FrameRenderer::text_buffers` field -- Type stays `Vec<Buffer>` but the count changes from `screen_lines` (~50) to non-empty cells (~2000-4000).

**Recommended approach -- Per-cell Buffers:**

Create one Buffer per non-empty, non-spacer cell. Skip empty/space cells entirely (most terminals are <50% filled). Position each buffer at grid-locked coordinates.

The simpler and more correct approach used by Alacritty itself: render each glyph individually at grid-locked positions.

**New data flow:**
```
snapshot.cells
  -> for each non-spacer, non-empty cell:
       Buffer::new() with single char (+ zerowidth combining chars)
       set_size(cell_width, cell_height)  [or 2*cell_width for WIDE_CHAR]
       shape_until_scroll()
  -> TextArea { left: col * cell_width + x_offset, top: line * cell_height + y_offset }
```

**Performance mitigation:**
- Only create Buffers for non-empty cells (skip spaces and WIDE_CHAR_SPACER)
- Reuse the Buffer Vec capacity between frames (already done with `text_buffers.clear()`)
- glyphon's TextAtlas caches rasterized glyphs -- same glyph at different positions shares atlas entry
- The hot path is `Buffer::new()` + `set_text()` + `shape_until_scroll()` per cell -- micro-benchmark needed

### Feature 2: Correct Line Height (Box-Drawing)

**Problem:** `cell_height = (font_size * scale * 1.2).ceil()` adds inter-line spacing. Box-drawing characters (U+2500-U+257F) need to connect vertically with zero gap.

**What CHANGES (modify existing):**

- `GridRenderer::new()` -- Change line height calculation. Use actual font metrics (ascent + descent) from cosmic-text instead of `font_size * 1.2`. Measure via the same Buffer used for cell_width measurement -- examine `layout_runs()` for `line_height`.

  Specifically: after shaping "M" to get cell_width, read the `line_y` or metric values from the layout run. The font's own metrics are authoritative. If the resulting line height is too tight (text overlaps), add minimal leading but NOT 20%.

- `GridRenderer::cell_height` -- Value changes. This cascades through `FrameRenderer::update_font()` to ALL consumers: BlockRenderer, StatusBarRenderer, TabBarRenderer, SearchOverlayRenderer. The cascade is already wired correctly.

- `main.rs` terminal size calculation -- Uses `cell_height` for computing num_lines. Value changes but code stays the same.

**What STAYS:** `FrameRenderer::update_font()` already rebuilds all sub-renderers when cell_height changes. No structural change needed.

**Box-drawing consideration:** Even with correct line height, box-drawing glyphs from the font may not perfectly fill the cell. Two options:
1. Trust the font (simpler, usually works with good monospace fonts like Cascadia Code)
2. Custom-render box-drawing characters as GPU rects (pixel-perfect, what Alacritty/WezTerm do)

**Recommendation:** Start with option 1 (correct metrics). If box-drawing still has gaps with specific fonts, add option 2 as a follow-up. Custom box-drawing rendering would add a detection step in `build_cell_buffers()` that emits RectInstances instead of text Buffers for U+2500-U+257F.

### Feature 3: Wide Character / CJK Support

**Problem:** CJK characters occupy 2 columns. `build_text_buffers()` already skips `WIDE_CHAR_SPACER` cells. But with per-line Buffers, the wide char is shaped as part of a line and may not be centered in 2 * cell_width.

**What CHANGES (modify existing):**

- `GridRenderer::build_rects()` -- WIDE_CHAR cells need `2 * cell_width` for their background rect. WIDE_CHAR_SPACER cells should be skipped entirely (currently they get a bg rect if bg != default, which causes a duplicate background).

  ```rust
  if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
      continue; // skip spacer, wide char already covers 2 cells
  }
  let width = if cell.flags.contains(Flags::WIDE_CHAR) {
      self.cell_width * 2.0
  } else {
      self.cell_width
  };
  ```

- `build_cell_buffers()` (the replacement for build_text_buffers) -- When a cell has `Flags::WIDE_CHAR`, its Buffer should have `set_size(2 * cell_width, cell_height)` so the glyph is properly shaped and centered within the double-width space.

- `build_selection_rects()` -- May need awareness that a wide char selection should highlight 2 cells, though this likely already works since selection ranges use column indices from alacritty_terminal which handle wide chars.

**What STAYS:** `snapshot_term()` already preserves WIDE_CHAR and WIDE_CHAR_SPACER flags. No changes to glass_terminal.

### Feature 4: Underline and Strikethrough Rendering

**Problem:** Flags (UNDERLINE, STRIKEOUT, DOUBLE_UNDERLINE, UNDERCURL, DOTTED_UNDERLINE, DASHED_UNDERLINE) are preserved in `RenderedCell.flags` but never read during rendering.

**What GETS ADDED (new code):**

- `GridRenderer::build_decoration_rects()` -- NEW METHOD. Iterates cells, checks decoration flags, emits RectInstances using the cell's fg color for decoration color.

  Positions (relative to cell top-left at `col * cell_width, line * cell_height`):
  - UNDERLINE: 1px rect at `y + cell_height - 2` (above cell bottom)
  - DOUBLE_UNDERLINE: two 1px rects, 2px apart near bottom
  - STRIKEOUT: 1px rect at `y + cell_height * 0.5` (vertical center)
  - UNDERCURL: approximate with alternating small rects (wavy). Or defer to simple underline.
  - DOTTED_UNDERLINE: 1px rects with 1px gaps
  - DASHED_UNDERLINE: 3px rects with 2px gaps
  - WIDE_CHAR_SPACER: skip (decoration only on the WIDE_CHAR cell, spanning 2*cell_width)

**What CHANGES (modify existing):**

- `FrameRenderer::draw_frame()` -- Insert `build_decoration_rects()` call. Decoration rects should render between bg rects and text:
  1. bg rects (existing)
  2. decoration rects (NEW -- underlines go under text)
  3. text (existing)

- `FrameRenderer::draw_multi_pane_frame()` -- Same addition with viewport offset applied to decoration rect positions.

**No changes needed to:** RectRenderer (decorations are just more RectInstances), GridSnapshot, snapshot_term.

### Feature 5: Font Fallback Configuration

**What cosmic-text already does:** FontSystem automatically discovers all system fonts at startup and performs font fallback when the primary font family lacks a glyph. glyphon's `Shaping::Advanced` (already used) enables HarfBuzz shaping which includes fallback.

**What CHANGES:** Likely nothing for basic fallback to work. The automatic fallback already handles most CJK and symbol cases.

**What MAY NEED ADDING:**

- If users need to control fallback order (e.g., prioritize "Noto Sans CJK SC" over other CJK fonts), add config support:

  ```toml
  [font]
  family = "Cascadia Code"
  fallback = ["Noto Sans CJK SC", "Segoe UI Symbol"]
  ```

- `GlyphCache::new()` or `FrameRenderer::new()` -- Load specified fallback fonts with higher priority via `FontSystem::db_mut()`.

- With per-cell positioning (Feature 1), fallback font glyph width mismatches are already handled -- each cell is positioned at `col * cell_width` regardless of the actual glyph width.

**Confidence:** MEDIUM. cosmic-text does automatic font fallback but the quality/ordering may not be ideal without explicit configuration. Need to test with CJK text.

### Feature 6: Dynamic DPI / Scale Factor Handling

**Problem:** `WindowEvent::ScaleFactorChanged` in main.rs is currently log-only. Comment says "FrameRenderer does not yet support dynamic scale factor updates."

**What CHANGES (modify existing):**

- `main.rs` ScaleFactorChanged handler -- Must call:
  1. `frame_renderer.update_font(font_family, font_size, new_scale_factor)` -- already exists and works
  2. Get new cell dimensions: `frame_renderer.cell_size()`
  3. Recalculate columns/rows from window size and new cell dims
  4. Resize PTY via `pty_sender.send(PtyMsg::Resize { cols, rows })`
  5. Resize wgpu surface via `renderer.resize(new_width, new_height)` if inner_size changed
  6. `window.request_redraw()`

**What STAYS:** `FrameRenderer::update_font()` already handles scale_factor. `GlassRenderer::resize()` already handles surface resize. The `update_font` path is already exercised by config hot-reload.

**Key detail:** The `ScaleFactorChanged` event on Windows may come with a new `PhysicalSize` via `inner_size_writer`. The handler must apply the new size.

## Component Boundaries

| Component | Responsibility | Changes For v2.4 |
|-----------|---------------|-------------------|
| `GridSnapshot` / `RenderedCell` | Cell data with flags | NO CHANGE -- already has all needed data |
| `snapshot_term()` | Extract cells from alacritty_terminal | NO CHANGE -- already preserves all flags |
| `GridRenderer` | Cell -> GPU primitives | MAJOR CHANGE -- per-cell positioning, line height, wide char rects, decoration rects |
| `RectRenderer` | Instanced quad pipeline | NO CHANGE -- just receives more RectInstances |
| `GlyphCache` / `FontSystem` | Font discovery, atlas, shaping | MINOR CHANGE -- possible explicit fallback font loading |
| `FrameRenderer` | Orchestrate draw pipeline | MODERATE CHANGE -- integrate decoration rects, update draw order, per-cell buffer flow |
| `GlassRenderer` | wgpu surface management | NO CHANGE |
| `main.rs` | Event loop, window management | MINOR CHANGE -- ScaleFactorChanged handler wiring |
| `BlockRenderer` et al. | Overlay rendering | NO CHANGE -- cell_size cascades automatically via update_font |

## Data Flow: Current vs New

### Current Flow
```
GridSnapshot
  -> build_rects(): cell bgs at (col*cw, line*ch), each width=cw
  -> build_text_buffers(): one Buffer per line, chars concatenated as rich text
  -> build_text_areas(): one TextArea per line at (0, line*ch)
  -> FrameRenderer: prepare rects, prepare text, render pass (rects then text)
```

### New Flow
```
GridSnapshot
  -> build_rects(): cell bgs at (col*cw, line*ch)
     - Skip WIDE_CHAR_SPACER cells
     - WIDE_CHAR cells get width=2*cw
  -> build_cell_buffers(): one Buffer per non-empty, non-spacer cell
     - Each positioned at (col*cw, line*ch) via TextArea
     - WIDE_CHAR gets buffer size 2*cw
  -> build_decoration_rects(): underline/strikethrough RectInstances
     - Uses cell fg color for decoration color
     - WIDE_CHAR decorations span 2*cw, skip WIDE_CHAR_SPACER
  -> build_cell_text_areas(): one TextArea per cell buffer at exact grid position
  -> FrameRenderer: prepare bg_rects + decoration_rects, prepare cell text,
     render pass: bg rects -> decoration rects -> text
```

## Suggested Build Order

Order follows dependencies. Each phase produces testable, shippable improvement.

### Phase 1: Line Height Fix (Foundation)
**Why first:** Affects cell_height which cascades everywhere. All subsequent work uses correct metrics. Smallest code change, largest visual impact on box-drawing.
- Change `GridRenderer::new()` to derive line height from font ascent+descent instead of `font_size * 1.2`
- Verify box-drawing characters (U+2500-U+257F) connect vertically
- All existing tests pass (cell_height is just a different number)
- Verify status bar, tab bar, block decorations still render correctly (cascades via update_font)

### Phase 2: Per-Cell Glyph Positioning (Core Fix)
**Why second:** Biggest change and most impactful for TUI correctness. Depends on Phase 1 for correct cell_height.
- Rename/rewrite `build_text_buffers()` -> `build_cell_buffers()` (per-cell Buffers)
- Rewrite `build_text_areas()` / `build_text_areas_offset()` for per-cell positioning
- Update `draw_frame()` and `draw_multi_pane_frame()` to use new buffer flow
- Performance benchmark: compare frame time per-cell vs per-line
- Visual test: TUI apps (vim, htop, Claude Code) render with aligned borders

### Phase 3: Wide Character / CJK Support
**Why third:** Builds directly on per-cell positioning. Without it, wide chars cannot be correctly placed.
- Modify `build_rects()` for WIDE_CHAR (2*cw bg) and skip WIDE_CHAR_SPACER bg
- Modify `build_cell_buffers()` to use 2*cw for WIDE_CHAR cells
- Modify `build_selection_rects()` for wide char awareness if needed
- Test with CJK text, `htop` in CJK locale, mixed ASCII/CJK content

### Phase 4: Underline / Strikethrough Rendering
**Why fourth:** Independent feature, but benefits from correct cell positioning for pixel alignment.
- Add `build_decoration_rects()` to GridRenderer
- Integrate into `draw_frame()` and `draw_multi_pane_frame()` render pipeline
- Support: UNDERLINE, DOUBLE_UNDERLINE, STRIKEOUT (most common)
- Defer or approximate: UNDERCURL, DOTTED_UNDERLINE, DASHED_UNDERLINE (lower priority)

### Phase 5: Font Fallback Configuration
**Why fifth:** cosmic-text already does automatic fallback. This phase adds user control.
- Add `[font] fallback = [...]` to config schema
- Load fallback fonts into FontSystem on startup and hot-reload
- Test with mixed Latin/CJK/Symbol text to verify fallback ordering
- May be unnecessary if automatic fallback proves sufficient

### Phase 6: Dynamic DPI Handling
**Why last:** Smallest change, isolated to main.rs event handler. Depends on update_font() working correctly (validated by earlier phases).
- Wire `ScaleFactorChanged` to `update_font()` + PTY resize + surface resize
- Handle the `inner_size_writer` from the event
- Test with Windows display scaling changes (100% -> 150% -> 100%)

### Phase 7: Tech Debt Cleanup
- Remove 1.2x line height constant (replaced in Phase 1)
- Consolidate build_rects / build_rects_offset if possible
- Profile per-cell buffer creation, optimize if >5ms per frame
- Add box-drawing custom rendering if font-based rendering has gaps

## Performance Considerations

| Concern | Current (per-line) | After Per-Cell | Mitigation |
|---------|-------------------|----------------|------------|
| Buffer count per frame | ~50 (screen_lines) | ~2000-4000 (non-empty cells) | Skip empty/space cells, reuse Vec capacity |
| Buffer::new() + set_text() cost | ~50 calls | ~2000-4000 calls | Profile; each call is <1us for single char |
| TextArea count | ~50 | ~2000-4000 | glyphon TextRenderer handles many TextAreas |
| Atlas pressure | Low | Same (same glyphs, different positions) | Atlas caches rasterized glyphs by font+size+glyph |
| Total build time | ~0.5ms typical | ~2-5ms estimate | Must stay under 8ms for 120fps budget |
| Memory | ~1MB text buffers | ~5-10MB estimate | Reuse Vec capacity between frames |

**Critical performance validation:** After Phase 2, benchmark `build_cell_buffers()` + `TextRenderer::prepare()` with a full terminal screen. If >5ms, consider:
1. Batching runs of identical-attribute ASCII characters (confirmed monospace) into single buffers
2. Skipping Buffer creation for common ASCII chars that are known to be exactly cell_width
3. Caching Buffer objects between frames for unchanged cells (complex but effective)

## Anti-Patterns to Avoid

### Anti-Pattern 1: Per-Character String Allocation
**What:** Creating a new `String` for every cell character.
**Why bad:** 2000+ allocations per frame at 60fps.
**Instead:** Use `&str` from a pre-built lookup or a reusable small buffer. For single chars, `char::encode_utf8()` into a stack `[u8; 4]` avoids allocation.

### Anti-Pattern 2: Rebuilding FontSystem on DPI Change
**What:** Destroying and recreating FontSystem (which rediscovers all system fonts).
**Why bad:** FontSystem::new() takes 50-200ms for font discovery.
**Instead:** Keep FontSystem, rebuild GridRenderer only. The existing `update_font()` path already does this correctly.

### Anti-Pattern 3: Custom WGSL Shader for Decorations
**What:** Writing a new wgpu shader pipeline for underlines/strikethrough.
**Why bad:** Unnecessary complexity, another GPU pipeline to manage.
**Instead:** Use existing RectRenderer instanced pipeline. Underlines are just thin rectangles. The pipeline already handles thousands of rects efficiently with alpha blending.

### Anti-Pattern 4: Modifying RenderedCell for Rendering Hints
**What:** Adding rendering-specific fields (pixel positions, buffer indices) to RenderedCell.
**Why bad:** Couples glass_terminal to glass_renderer concerns. RenderedCell is a data transfer type.
**Instead:** Compute all rendering positions in GridRenderer from `cell.point` and cell dimensions.

### Anti-Pattern 5: Conditional Per-Line vs Per-Cell Based on Content
**What:** Using per-line buffers for "simple" lines and per-cell for "complex" lines.
**Why bad:** Two code paths, hard to maintain, edge cases where detection is wrong.
**Instead:** Always use per-cell positioning. Optimize the per-cell path to be fast enough.

## Sources

- [alacritty_terminal Flags documentation](https://docs.rs/alacritty_terminal/latest/alacritty_terminal/term/cell/struct.Flags.html) -- confirms UNDERLINE, STRIKEOUT, DOUBLE_UNDERLINE, UNDERCURL, DOTTED_UNDERLINE, DASHED_UNDERLINE, WIDE_CHAR, WIDE_CHAR_SPACER flags (HIGH confidence)
- [cosmic-text FontSystem](https://pop-os.github.io/cosmic-text/cosmic_text/struct.FontSystem.html) -- font discovery and automatic fallback (MEDIUM confidence)
- [cosmic-text font fallback in Bevy](https://github.com/bevyengine/bevy/issues/16354) -- real-world font fallback behavior with cosmic-text (MEDIUM confidence)
- Direct source code analysis of grid_renderer.rs, frame.rs, surface.rs, grid_snapshot.rs, rect_renderer.rs, glyph_cache.rs, main.rs (HIGH confidence)

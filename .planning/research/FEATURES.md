# Feature Landscape

**Domain:** GPU terminal emulator rendering correctness
**Researched:** 2026-03-09

## Table Stakes

Features users expect from any terminal emulator claiming TUI app support. Missing = broken rendering in vim, htop, tmux, Claude Code, etc.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Per-cell glyph positioning | Every glyph must land at column * cell_width. Without it, characters drift horizontally and TUI borders misalign. Every GPU terminal (Alacritty, Ghostty, Kitty, WezTerm) positions glyphs per-cell. | Medium | Current approach: per-line glyphon Buffer with text shaping. Shaping can shift glyphs off-grid. Must switch to per-cell x-offset or post-shaping snap. |
| Correct line height from font metrics | Line height must equal ascent + descent (+ optional leading) so box-drawing characters (U+2500-U+259F) connect vertically without gaps. The current `1.2 * font_size` multiplier creates visible gaps between lines. | Low | Change `Metrics::new()` to derive line_height from font's actual ascent + descent instead of hardcoded 1.2x multiplier. |
| Wide character / CJK support | CJK characters occupy 2 cells. Without double-width rendering, Chinese/Japanese/Korean text overlaps or misaligns. alacritty_terminal already sets WIDE_CHAR and WIDE_CHAR_SPACER flags. | Medium | Renderer already skips WIDE_CHAR_SPACER cells. Need: (1) render wide chars at 2x cell_width, (2) background rects span 2 cells, (3) PTY column count must account for wide chars during resize. |
| Underline rendering | SGR 4 (underline) is universally used. grep --color, compiler errors, TUI highlights all use it. alacritty_terminal provides UNDERLINE flag. | Low | Draw a 1-2px rect at cell bottom (y + ascent + 1px). Color from cell fg or underline_color if set. Reuse existing RectInstance pipeline. |
| Strikethrough rendering | SGR 9 (strikethrough) used by diff tools, todo apps, and TUI frameworks. alacritty_terminal provides STRIKEOUT flag. | Low | Draw a 1px rect at vertical center of cell (y + ascent/2). Same approach as underline -- rect instance per cell with flag. |
| Dynamic DPI / scale factor handling | Users drag windows between monitors with different DPI. Without handling ScaleFactorChanged, text becomes blurry or wrong size on the new monitor. | Medium | Infrastructure exists: `update_font()` on FrameRenderer already rebuilds everything. Just need to wire ScaleFactorChanged event to call it with new scale_factor, then resize PTY. |

## Differentiators

Features that go beyond baseline but make the terminal feel polished. Not expected, but appreciated.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Built-in box drawing character rendering | Custom-draw U+2500-U+259F geometrically instead of using font glyphs. Eliminates font-dependent gaps/overlaps. Alacritty, Kitty, Ghostty, WezTerm all do this. | High | ~100 characters to implement as procedural geometry. Each is a combination of horizontal/vertical lines, arcs, and filled regions. Can be done as RectInstances. Worth doing but deferrable. |
| Multiple underline styles (double, curly, dotted, dashed) | Modern terminals support SGR 4:2 (double), 4:3 (curly/undercurl), 4:4 (dotted), 4:5 (dashed). Neovim, tmux, and editors rely on these. alacritty_terminal provides all five flags. | Medium | Double underline: two 1px rects. Curly: sine wave via small rect steps or custom shader. Dotted/dashed: alternating rects. Curly is hardest. |
| Colored underlines (SGR 58) | Underline color independent of foreground. Used heavily by LSP error highlighting in Neovim (red underline under errors). alacritty_terminal stores underline_color in CellExtra. | Low | Need to extract underline_color from Cell during snapshot. Add optional `underline_color: Option<Rgb>` to RenderedCell. Use it instead of fg for underline rect color. |
| Font fallback cascade | When primary font lacks a glyph (emoji, Nerd Font icons, CJK), automatically find a system font that has it. cosmic-text/glyphon has built-in fallback via fontdb. | Low-Medium | glyphon/cosmic-text already does font fallback internally via FontSystem. The main work is: (1) verify it works with current per-line Buffer approach, (2) ensure fallback glyphs are sized to match primary font metrics, (3) optionally allow configuring fallback font list. |
| Powerline symbol rendering | Powerline glyphs (U+E0B0-U+E0B3) used by Starship, Oh My Posh, Powerlevel10k. Custom rendering ensures pixel-perfect triangles regardless of font. | Medium | 4 characters: right triangle, left triangle, right half-circle, left half-circle. Can be done as custom geometry alongside box drawing. |
| DIM text rendering | SGR 2 (faint/dim) reduces text brightness. alacritty_terminal already resolves DIM colors in the grid. | Already done | Color resolution in grid_snapshot.rs already handles DIM via `name.to_dim()`. No renderer changes needed. |

## Anti-Features

Features to explicitly NOT build in this milestone.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| Font ligatures | Requires HarfBuzz shaping pipeline, fundamentally changes per-cell rendering model, massive complexity. Glass already uses `Shaping::Advanced` via cosmic-text but ligatures would merge multiple cells into one glyph. | Explicitly out of scope per PROJECT.md. Would require rethinking the entire cell-grid rendering model. |
| Image protocol support (Kitty, Sixel) | Separate rendering layer (texture upload, placement, scrolling). Orthogonal to text rendering correctness. | Defer to future milestone. No dependency on text rendering fixes. |
| Custom glyph atlas / texture atlas rendering | Building a custom glyph atlas and doing texture-mapped quad rendering (like Alacritty does) instead of using glyphon. Would be faster but requires abandoning glyphon entirely. | Use glyphon as-is. The per-cell positioning fix works within glyphon's API. Only consider atlas approach if profiling shows glyphon is a bottleneck. |
| HarfBuzz text shaping | Full complex text layout for Arabic, Thai, Devanagari, etc. Requires HarfBuzz integration, bidirectional text support, and fundamentally different cell model. | cosmic-text already uses harfrust (a Rust HarfBuzz port) internally. For terminal use, complex scripts aren't grid-aligned anyway. Not a terminal-emulator concern. |
| Sub-pixel anti-aliasing | ClearType-style rendering with per-subpixel color channels. Would require knowledge of physical pixel layout and changes to the rasterization pipeline. | Let the OS/GPU driver handle sub-pixel rendering. glyphon/swash handle rasterization; Glass doesn't need to intervene. |
| Configurable line height / cell padding | Allowing users to add extra padding between lines or within cells. Nice for readability but complicates box-drawing alignment. | Get the correct default line height first. Config option can be added later as a simple multiplier. |

## Feature Dependencies

```
Correct line height ─────────────┐
                                 ├──> Box drawing looks correct
Per-cell glyph positioning ──────┘
                                 ├──> TUI apps render correctly (vim, htop, tmux)
Wide char / CJK support ─────────┘

Per-cell glyph positioning ──────> Underline/strikethrough positioned correctly
                                   (decorations need accurate cell boundaries)

Font fallback cascade ───────────> CJK characters actually render
                                   (without fallback, CJK shows tofu/missing glyphs)

Dynamic DPI handling ────────────> Requires: update_font() (already exists)
                                   Independent of other features.

Underline rendering ─────────────> Multiple underline styles (extension)
                                   Colored underlines (extension)
```

## MVP Recommendation

Prioritize (in implementation order):

1. **Correct line height from font metrics** -- Lowest complexity, highest visual impact. Changes one line in GridRenderer::new(). Fixes box-drawing gaps immediately.

2. **Per-cell glyph positioning** -- Core fix. Without this, all other features render at wrong positions. Change build_text_buffers() to position each cell's glyph at column * cell_width instead of relying on text shaping to place them.

3. **Underline and strikethrough rendering** -- Low complexity, high value. Read UNDERLINE/STRIKEOUT flags (already in RenderedCell.flags), emit RectInstances at appropriate positions.

4. **Wide character / CJK support** -- Medium complexity but critical for internationalization. Render WIDE_CHAR cells at 2x width, ensure backgrounds span correctly.

5. **Font fallback cascade** -- Verify/configure cosmic-text's built-in fallback. May already partially work. Test with emoji and CJK characters.

6. **Dynamic DPI handling** -- Wire ScaleFactorChanged to existing update_font(). Infrastructure already exists, just needs the event handler.

Defer to later:
- **Built-in box drawing rendering**: High complexity, and correct line height + per-cell positioning will fix most box-drawing issues with good fonts. Only needed for fonts with poorly-designed box drawing glyphs.
- **Multiple underline styles**: Extension of basic underline. Add after basic underline works.
- **Colored underlines**: Requires RenderedCell schema change. Add after basic underline works.
- **Powerline symbols**: Niche. Most users have Nerd Fonts installed which include these glyphs.

## Detailed Feature Specifications

### Per-Cell Glyph Positioning

**Current behavior:** `build_text_buffers()` creates one glyphon Buffer per line, sets the full line text, and lets cosmic-text's shaper position glyphs. The shaper uses each glyph's advance width, which may differ from cell_width, causing cumulative drift.

**Expected behavior:** Each glyph's x-position must be `column * cell_width`, regardless of the glyph's natural advance. This is how all GPU terminals work -- the terminal grid dictates position, not the font metrics.

**Implementation approaches:**
1. **Per-cell Buffer (simple, possibly slow):** Create one glyphon Buffer per cell. Guarantees positioning but creates N*M buffers per frame.
2. **Post-shaping position override (ideal):** Shape text per-line for shaping benefits, then override each glyph's x-position to snap to grid. Requires glyphon API access to glyph positions after shaping.
3. **Pre-padded text (hacky):** Insert spaces to force grid alignment. Fragile, doesn't work with variable-width glyphs.

Approach 2 is what Alacritty, Ghostty, and other terminals do. The exact API depends on whether glyphon exposes glyph positions for modification after shaping. If not, approach 1 (per-cell Buffer) is the fallback -- performance impact can be mitigated by caching buffers between frames.

### Line Height from Font Metrics

**Current:** `line_height = (physical_font_size * 1.2).ceil()` -- hardcoded 1.2x multiplier.

**Expected:** `line_height = ascent + descent` from the font's actual metrics. cosmic-text provides these via `FontSystem`. The `Metrics` struct takes `(font_size, line_height)` where line_height should match the font's natural height for box-drawing to connect.

**Key detail:** Some terminals use `ascent + descent + leading` while others use just `ascent + descent`. For box-drawing correctness, the line height must exactly match what the font designer intended for the cell height. If the font's `ascent + descent` is smaller than expected, cell_height should be `max(ascent + descent, font_size)` to prevent overlap.

### Wide Character / CJK Support

**Current:** WIDE_CHAR_SPACER cells are skipped in text building. WIDE_CHAR cells are rendered at 1x width.

**Expected:**
- WIDE_CHAR cells render their glyph centered over 2x cell_width
- Background rect for WIDE_CHAR spans 2 cells
- WIDE_CHAR_SPACER cells contribute no glyph but may need background rect if bg differs from default
- Cursor on a wide char should be 2x cell_width
- Selection highlighting should cover both cells

**Edge cases:**
- Wide char at last column wraps to next line (terminal handles this, but renderer must not clip)
- Half-overwritten wide char (cursor or overwrite in middle) -- alacritty_terminal handles this by clearing the spacer
- LEADING_WIDE_CHAR_SPACER flag exists for wide chars that wrap -- spacer is at end of previous line

### Underline / Strikethrough

**alacritty_terminal flags available:**
- `UNDERLINE` -- standard single underline (SGR 4)
- `DOUBLE_UNDERLINE` -- two parallel lines (SGR 21)
- `UNDERCURL` -- wavy/curly line (SGR 4:3)
- `DOTTED_UNDERLINE` -- dotted line (SGR 4:4)
- `DASHED_UNDERLINE` -- dashed line (SGR 4:5)
- `STRIKEOUT` -- strikethrough (SGR 9)

**Positioning (standard practice):**
- Underline: 1-2px below baseline. Position = `y + descent - underline_position` (from font metrics) or `y + cell_height - 2px` as fallback.
- Strikethrough: centered vertically. Position = `y + ascent * 0.65` (approximate strikethrough position).
- All decorations span full cell_width, colored with fg color (or underline_color if set via SGR 58).

**For MVP:** Implement UNDERLINE and STRIKEOUT only. Both are simple horizontal rects added to the RectInstance list during `build_rects()`.

### Font Fallback

**How it works in cosmic-text/glyphon:** FontSystem loads all system fonts via fontdb. When shaping text, if the primary font family lacks a glyph, cosmic-text automatically searches loaded fonts for one that has it. This is built-in behavior.

**What Glass needs to do:**
1. Verify fallback works by testing with CJK text and emoji
2. Ensure fallback glyph sizes are reasonable (cosmic-text should handle this)
3. Optionally expose a `font.fallback` config array to let users prioritize specific fallback fonts
4. Handle the case where NO font has the glyph (show a replacement character, not crash)

**Known issue (from Bevy/cosmic-text research):** cosmic-text's fallback can sometimes produce inconsistent sizing. Ghostty addresses this by adjusting fallback font sizes to match the primary font's metrics. Glass may need similar adjustment if fallback glyphs appear too large/small.

### Dynamic DPI Handling

**Current:** ScaleFactorChanged event is logged but ignored (documented tech debt).

**Required:**
1. In ScaleFactorChanged handler, call `frame_renderer.update_font(font_family, font_size, new_scale_factor)`
2. update_font() already rebuilds GridRenderer with new metrics
3. After font rebuild, recalculate terminal columns/rows and resize PTY
4. Request redraw

**Platform behavior:**
- Windows: Per-monitor DPI (125%, 150%, 200% common). Triggered by dragging between monitors.
- macOS: Retina (2x) vs non-Retina. Less common to change dynamically.
- Linux/Wayland: Integer scale factors (1x, 2x). X11: global DPI via Xft.dpi.

## Sources

- [Alacritty box drawing PR](https://github.com/alacritty/alacritty/commit/f7177101eda589596ab08866892bd4629bd1ef44) -- builtin box drawing implementation
- [Alacritty box drawing issues](https://github.com/alacritty/alacritty/issues/7067) -- community discussion on box drawing rendering
- [Alacritty cell Flags docs](https://docs.rs/alacritty_terminal/0.25.1/alacritty_terminal/term/cell/struct.Flags.html) -- all available flags including underline variants
- [Ghostty font system](https://deepwiki.com/ghostty-org/ghostty/5.5-font-system) -- per-cell positioning and fallback size adjustment
- [Ghostty config reference](https://ghostty.org/docs/config/reference) -- glyph positioning configuration
- [Contour terminal text stack](https://contour-terminal.org/internals/text-stack/) -- CJK wide char and grid alignment internals
- [Zutty GPU rendering](https://tomscii.sig7.se/2020/11/How-Zutty-works) -- compute shader approach to grid rendering
- [WezTerm font height issues](https://github.com/wezterm/wezterm/issues/2753) -- line height discrepancies between terminals
- [cosmic-text font fallback issue](https://github.com/pop-os/cosmic-term/issues/104) -- fallback font search strictness
- [Bevy cosmic-text fallback issue](https://github.com/bevyengine/bevy/issues/16354) -- inconsistent fallback rendering
- [winit DPI documentation](https://docs.rs/winit/latest/winit/dpi/index.html) -- ScaleFactorChanged event semantics
- [Ghostty CJK height constraining](https://github.com/ghostty-org/ghostty/issues/8709) -- CJK glyph height adjustment
- [Alacritty underline support PR](https://github.com/alacritty/alacritty/pull/1078) -- underline and strikethrough implementation

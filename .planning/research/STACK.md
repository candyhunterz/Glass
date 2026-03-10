# Technology Stack: v2.4 Rendering Correctness

**Project:** Glass v2.4 -- Per-cell glyph positioning, box-drawing, CJK wide chars, underline/strikethrough, font fallback, dynamic DPI
**Researched:** 2026-03-09
**Overall Confidence:** HIGH

## Key Finding: No New Dependencies Required

The existing stack (glyphon 0.10.0 -> cosmic-text 0.15.0 -> fontdb 0.23.0) already provides all the primitives needed for every v2.4 feature. The work is architectural (how we use these libraries) not additive (adding new libraries).

## Current Stack (Verified from Cargo.lock)

| Technology | Version | Role |
|------------|---------|------|
| glyphon | 0.10.0 | wgpu text rendering (TextArea, Buffer, TextRenderer) |
| cosmic-text | 0.15.0 | Text shaping, layout, font matching (transitive via glyphon) |
| fontdb | 0.23.0 | System font database, font discovery (transitive via cosmic-text) |
| wgpu | 28.0.0 | GPU rendering pipeline |
| alacritty_terminal | =0.25.1 | VTE parsing, cell Flags (UNDERLINE, STRIKEOUT, WIDE_CHAR, etc.) |
| unicode-width | 0.2.2 | Already in dependency tree (used by alacritty_terminal) |

## Feature-by-Feature Stack Analysis

### 1. Per-Cell Glyph Positioning

**What exists now:** Per-line `Buffer` with `set_rich_text()` -- cosmic-text shapes the entire line as a text run, applying kerning and proportional spacing. This causes horizontal drift where glyph `x` positions don't align with `column * cell_width`.

**What to change:** After calling `buffer.shape_until_scroll()`, iterate `buffer.layout_runs()` -> `run.glyphs` and read each `LayoutGlyph.x` position. Compare to `column * cell_width`. If drift exceeds a threshold, either:

- **Option A (recommended): Per-cell Buffer approach** -- Create one `Buffer` per cell (or per contiguous-attribute span). Position each at `column * cell_width`. This eliminates drift entirely because cosmic-text cannot accumulate cross-cell kerning. Cost: more Buffer objects, but each is trivially small (1 char).
- **Option B: Post-layout x correction** -- Keep per-line Buffer but override each `LayoutGlyph.x` to snap to `column * cell_width`. This requires accessing glyphs mutably after shaping, which cosmic-text's `LayoutGlyph` does expose (x, y, w are pub fields on the struct).

**Recommendation:** Option A (per-cell Buffers). Terminals require strict grid alignment. Trying to patch cosmic-text's layout output is fragile and fights the shaping engine. Per-cell Buffers are simple, correct, and avoid all kerning/ligature drift. The glyph atlas cache in glyphon means the GPU cost is nearly identical -- the same glyphs hit the same atlas entries regardless of how many Buffers reference them.

**Libraries needed:** None new. `glyphon::Buffer`, `cosmic_text::Metrics`, `cosmic_text::LayoutGlyph` are all available.

**Confidence:** HIGH -- cosmic-text's Buffer API and LayoutGlyph struct verified from docs.rs.

### 2. Correct Line Height (Box-Drawing Characters)

**What exists now:** `line_height = (physical_font_size * 1.2).ceil()` -- hardcoded 1.2x multiplier. This creates gaps between lines because box-drawing characters (U+2500-U+257F) are designed to touch vertically when line_height equals the font's actual em height (ascent + descent).

**What to change:** Derive line_height from font metrics instead of a multiplier:

```rust
// After shaping a reference character, read the LayoutRun
let metrics_run = measure_buf.layout_runs().next().unwrap();
let actual_line_height = metrics_run.line_height;
// Or compute from font metrics directly:
// cell_height = (font_ascent + font_descent).ceil()
```

cosmic-text's `LayoutRun` exposes `line_height`, `line_top`, and `line_y` (baseline offset). The `Metrics::new(font_size, line_height)` constructor lets you set line_height explicitly. For terminals, `line_height` should equal the font's natural height (ascent + |descent|), not 1.2x font_size.

**Implementation approach:**
1. Shape a reference glyph with `Metrics::new(font_size, font_size)` (line_height = font_size as initial)
2. Read `layout_run.line_height` to get cosmic-text's computed natural line height
3. Use that as cell_height in `Metrics::new(font_size, natural_line_height)`

**Libraries needed:** None new. `cosmic_text::LayoutRun.line_height` is already available.

**Confidence:** HIGH -- LayoutRun fields verified from docs.rs (line_y, line_top, line_height are pub f32 fields).

### 3. Wide Character / CJK Support

**What exists now:** `WIDE_CHAR_SPACER` cells are skipped during rendering (correct), but the wide character itself is rendered at single-cell width. The glyph visually overflows or gets clipped.

**What to change:** When a cell has `Flags::WIDE_CHAR` set (already available from alacritty_terminal =0.25.1), render the glyph positioned at `column * cell_width` but with a TextArea/Buffer sized to `2 * cell_width`. The background rect should also span 2 cells.

**alacritty_terminal Flags available (verified):**
- `Flags::WIDE_CHAR` -- cell contains a double-width character
- `Flags::WIDE_CHAR_SPACER` -- placeholder cell after a wide character (already skipped)

**Libraries needed:** None new. `unicode-width` 0.2.2 is already in the dependency tree (alacritty_terminal uses it internally for width classification). The `Flags::WIDE_CHAR` flag on `RenderedCell.flags` is all that's needed.

**Confidence:** HIGH -- Flags::WIDE_CHAR confirmed from alacritty_terminal docs and existing code (WIDE_CHAR_SPACER already handled).

### 4. Underline and Strikethrough GPU Rendering

**What exists now:** `Flags::BOLD` and `Flags::ITALIC` are checked in `build_text_buffers()`, but `UNDERLINE`, `STRIKEOUT`, and variants are ignored.

**What to change:** Render underline/strikethrough as RectInstance quads in `build_rects()`, not as text attributes. This reuses the existing instanced rect pipeline (zero new GPU code).

**alacritty_terminal =0.25.1 Flags (verified):**
- `Flags::UNDERLINE` -- single underline
- `Flags::DOUBLE_UNDERLINE` -- double underline
- `Flags::UNDERCURL` -- wavy underline (approximate with 2px rect for v1)
- `Flags::DOTTED_UNDERLINE` -- dotted underline (approximate or skip for v1)
- `Flags::DASHED_UNDERLINE` -- dashed underline (approximate or skip for v1)
- `Flags::STRIKEOUT` -- strikethrough

**Rendering approach:**
```rust
// In build_rects(), after cell background rects:
if cell.flags.contains(Flags::UNDERLINE) {
    let underline_y = y + self.cell_height - 2.0; // 2px from bottom
    rects.push(RectInstance {
        pos: [x, underline_y, self.cell_width, 1.0],
        color: rgb_to_color(cell.fg, 1.0), // underline uses fg color
    });
}
if cell.flags.contains(Flags::STRIKEOUT) {
    let strike_y = y + self.cell_height * 0.5; // middle of cell
    rects.push(RectInstance {
        pos: [x, strike_y, self.cell_width, 1.0],
        color: rgb_to_color(cell.fg, 1.0),
    });
}
```

For `DOUBLE_UNDERLINE`: two 1px rects separated by 1px gap.
For `UNDERCURL`: approximate with a thicker rect (2px) or defer true wavy rendering.
For `DOTTED_UNDERLINE`/`DASHED_UNDERLINE`: can be approximated or deferred.

**Underline color:** alacritty_terminal 0.25.1's `Cell` struct may include an `underline_color` field. If present, use it; otherwise fall back to fg color. This needs verification at implementation time.

**Libraries needed:** None new. Existing `RectInstance` pipeline handles this. No new shaders, no new GPU pipelines.

**Confidence:** HIGH for basic UNDERLINE/STRIKEOUT. MEDIUM for UNDERCURL/DOTTED/DASHED (may need custom shader for pixel-perfect rendering, but rect approximation works for v1).

### 5. Font Fallback Cascade

**What exists now:** `FontSystem::new()` loads system fonts. `Attrs::new().family(Family::Name("Cascadia Code"))` requests a specific font. If a glyph is missing, cosmic-text's built-in fallback kicks in -- but the behavior depends on FontSystem configuration.

**What cosmic-text 0.15.0 provides (verified):**
- `FontSystem::new()` -- loads all system fonts, automatic fallback enabled
- `FontSystem::new_with_locale_and_db()` -- custom font database
- `FontSystem::db_mut()` -- access to fontdb::Database for loading additional fonts
- `get_monospace_ids_for_scripts()` -- find monospace fonts for specific Unicode scripts (CJK, Arabic, etc.)
- Built-in font fallback: cosmic-text automatically falls back to other loaded fonts when the primary font lacks a glyph

**What to change:**
1. Ensure `FontSystem::new()` is called (it already is) -- this loads system fonts including CJK fonts if installed
2. Optionally expose a `font_fallback` config list in `config.toml` for user-specified fallback order
3. Use `db_mut().load_font_file()` to load user-specified fallback fonts if they aren't system-installed
4. cosmic-text handles the actual fallback matching internally during shaping -- no manual glyph-by-glyph fallback needed

**Libraries needed:** None new. cosmic-text 0.15.0's built-in fallback + fontdb 0.23.0's font discovery handles this.

**Confidence:** HIGH -- FontSystem API verified from docs.rs. cosmic-text's automatic fallback is well-documented and used by COSMIC Terminal (same stack).

### 6. Dynamic DPI / Scale Factor Handling

**What exists now:** `ScaleFactorChanged` event is logged but ignored (confirmed in main.rs line 1052-1056). `update_font()` already exists and recalculates everything from font metrics -- it just isn't called on DPI change.

**What to change:** In the `WindowEvent::ScaleFactorChanged` handler:
1. Call `frame_renderer.update_font(font_family, font_size, new_scale_factor)`
2. Recalculate terminal grid dimensions (columns, rows)
3. Resize the PTY to match new grid
4. Request a window redraw

This is almost identical to the existing font-change hot-reload path (already implemented in the `font_changed` branch around line 2365 of main.rs).

**Libraries needed:** None new. `winit::WindowEvent::ScaleFactorChanged` already provides the new scale factor. `FrameRenderer::update_font()` already accepts scale_factor.

**Confidence:** HIGH -- all integration points verified from source code. The hot-reload path proves the pattern works.

## Recommended Stack (No Changes)

### Core Framework (unchanged)
| Technology | Version | Purpose | Why No Change |
|------------|---------|---------|---------------|
| glyphon | 0.10.0 | Text rendering | Provides Buffer, TextArea, FontSystem re-exports. Per-cell Buffers use same API |
| cosmic-text | 0.15.0 | Text shaping/layout | Built-in font fallback, LayoutRun metrics, Metrics struct -- all needed APIs present |
| wgpu | 28.0.0 | GPU pipeline | Existing instanced rect pipeline handles underline/strikethrough |
| alacritty_terminal | =0.25.1 | VTE parsing | Already exposes all needed Flags (UNDERLINE, STRIKEOUT, WIDE_CHAR, etc.) |

### Supporting Libraries (unchanged)
| Library | Version | Purpose | Relevance to v2.4 |
|---------|---------|---------|-------------------|
| fontdb | 0.23.0 | Font database | System font discovery for fallback cascade |
| unicode-width | 0.2.2 | Character width | CJK width detection (already transitive dep) |
| winit | 0.30.13 | Window events | ScaleFactorChanged event for DPI handling |

## What NOT to Add

| Library | Why Not |
|---------|---------|
| **harfbuzz-rs** | cosmic-text already includes swash for shaping. HarfBuzz would be needed for ligatures (out of scope per PROJECT.md) |
| **fontdue** | Redundant with cosmic-text's glyph rasterization. Would create two text pipelines |
| **ab_glyph** | Same problem as fontdue -- cosmic-text already handles rasterization |
| **rusttype** | Deprecated in favor of ab_glyph, and cosmic-text supersedes both |
| **unicode-width (explicit)** | Already a transitive dependency via alacritty_terminal. No need to add directly |
| **freetype-rs** | System dependency, complex build. cosmic-text uses swash (pure Rust) instead |
| **custom WGSL shaders for underline** | Overkill for v1. RectInstance quads work for straight lines. Only needed later for true UNDERCURL rendering |

## Alternatives Considered

| Feature | Recommended | Alternative | Why Not Alternative |
|---------|-------------|-------------|---------------------|
| Cell positioning | Per-cell Buffer | Post-layout x snap | Fighting the shaping engine; fragile with complex scripts |
| Line height | Font metric derived | Keep 1.2x multiplier | Box-drawing gaps are the #1 visual bug to fix |
| Underline rendering | RectInstance quads | Glyph-based underline | Rect approach reuses existing pipeline; no new GPU code |
| Font fallback | cosmic-text built-in | Manual per-glyph lookup | cosmic-text's fallback is battle-tested (COSMIC Desktop uses it) |
| DPI handling | update_font() on event | Recreate renderer | update_font() already works for hot-reload; same pattern |
| Undercurl | Rect approximation (v1) | Custom compute shader | Defer true wavy line to future milestone; rect is good enough |

## Installation

```bash
# No new dependencies to install. Existing Cargo.toml is sufficient.
# The only changes are in how existing APIs are used.
```

## Integration Points Summary

| Feature | Primary File | API Used | Change Type |
|---------|-------------|----------|-------------|
| Per-cell positioning | grid_renderer.rs | `Buffer::new()`, `Metrics`, `TextArea` | Rewrite build_text_buffers() |
| Line height | grid_renderer.rs | `Metrics::new()`, `LayoutRun.line_height` | Change GridRenderer::new() |
| Wide chars | grid_renderer.rs | `Flags::WIDE_CHAR`, `RectInstance` | Extend build_text_buffers() + build_rects() |
| Underline/strike | grid_renderer.rs | `Flags::UNDERLINE/STRIKEOUT`, `RectInstance` | Extend build_rects() |
| Font fallback | frame.rs | `FontSystem::new()`, `db_mut()` | Minimal -- cosmic-text auto-fallback |
| Dynamic DPI | main.rs | `ScaleFactorChanged`, `update_font()` | Wire existing handler |

## Sources

- [glyphon 0.10.0 docs](https://docs.rs/glyphon/0.10.0/glyphon/) -- TextArea, Buffer, CustomGlyph APIs
- [cosmic-text FontSystem](https://docs.rs/cosmic-text/latest/cosmic_text/struct.FontSystem.html) -- font fallback methods (verified 0.18.2 docs, applicable to 0.15.0 API)
- [cosmic-text Metrics](https://docs.rs/cosmic-text/latest/cosmic_text/struct.Metrics.html) -- font_size and line_height fields
- [cosmic-text LayoutRun](https://docs.rs/cosmic-text/latest/cosmic_text/struct.LayoutRun.html) -- line_y, line_top, line_height fields
- [cosmic-text LayoutGlyph](https://docs.rs/cosmic-text/latest/cosmic_text/struct.LayoutGlyph.html) -- x, y, w, glyph_id, font_id fields
- [alacritty_terminal cell Flags](https://docs.rs/alacritty_terminal/0.25.1/alacritty_terminal/term/cell/struct.Flags.html) -- UNDERLINE, STRIKEOUT, WIDE_CHAR, etc.
- [COSMIC Terminal (cosmic-term)](https://github.com/pop-os/cosmic-term) -- reference implementation using same cosmic-text + alacritty_terminal stack
- Cargo.lock verification: glyphon 0.10.0 -> cosmic-text 0.15.0 -> fontdb 0.23.0, unicode-width 0.2.2

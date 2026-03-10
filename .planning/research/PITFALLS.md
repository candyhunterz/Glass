# Pitfalls Research

**Domain:** GPU terminal emulator rendering correctness (per-cell positioning, box-drawing, CJK, decorations, font fallback, DPI)
**Researched:** 2026-03-09
**Confidence:** HIGH (verified against codebase, glyphon issues, alacritty patterns, cosmic-term architecture)

## Critical Pitfalls

### Pitfall 1: Per-Line Buffer Shaping Causes Horizontal Glyph Drift

**What goes wrong:**
The current `build_text_buffers` creates one `cosmic_text::Buffer` per terminal line, concatenates all cell characters into a string, and lets cosmic-text shape/layout the entire line. Cosmic-text applies kerning and proportional spacing between glyphs, so glyph positions drift from the expected `column * cell_width` grid. By column 40+, characters can be off by several pixels. TUI apps like vim, htop, and tmux assume exact grid alignment -- drift makes borders misalign and text appear in wrong columns.

**Why it happens:**
The per-line approach was a correct initial design: it gives cosmic-text rich text spans with per-character color/weight/style, which is how glyphon is designed to work. But terminal emulators are NOT proportional text -- they require each glyph locked to `column * cell_width` regardless of what the shaping engine thinks.

**How to avoid:**
Switch to per-cell rendering: create one `Buffer` per cell (or per run of identical-attribute cells where each glyph is positioned at its exact column offset). Set each `TextArea.left` to `column * cell_width` to force grid alignment. The key insight: do NOT let cosmic-text decide horizontal positioning -- override it with the grid math. An alternative is to use one Buffer per line but post-process glyph positions, but this fights the API rather than working with it.

**Warning signs:**
- Box-drawing borders don't connect horizontally between adjacent cells
- TUI column separators (pipes `|`) don't align vertically across lines
- Text in TUI apps appears progressively shifted right on longer lines
- `vim` column numbers don't match actual cursor position visually

**Phase to address:**
Phase 1 (Per-cell glyph positioning) -- this is the foundational fix that all other rendering improvements depend on.

---

### Pitfall 2: Line Height 1.2x Multiplier Breaks Box-Drawing Vertical Continuity

**What goes wrong:**
The current `GridRenderer::new()` uses `line_height = (physical_font_size * 1.2).ceil()` (line 49 of grid_renderer.rs). This 1.2x multiplier adds vertical padding between lines. Box-drawing characters (U+2500-U+259F) are designed to fill the entire cell vertically so that vertical lines connect seamlessly between rows. The extra padding creates visible gaps between rows, breaking TUI borders.

**Why it happens:**
The 1.2x multiplier is standard for paragraph text (readability spacing). Terminal emulators must NOT use paragraph line spacing -- they need cell height derived from font metrics: `ascent + descent` (plus optional minimal padding), which gives the tightest cell that fits all glyphs without gaps.

**How to avoid:**
Derive line height from font metrics: query `cosmic_text::FontSystem` for the primary font's ascent + descent (and optionally + leading). Use `ceil(ascent + descent)` as cell_height. Do NOT add a multiplier. Verify with box-drawing test: render `tmux` or a box-drawing grid and confirm no gaps between rows. The font metrics are available via `Buffer::metrics()` after shaping, or by querying the font database directly.

**Warning signs:**
- Visible horizontal gaps between rows of box-drawing characters
- tmux pane borders have dashed appearance instead of solid lines
- Powerline prompt symbols don't connect vertically

**Phase to address:**
Phase 2 (Line height from font metrics) -- must be done before or alongside per-cell positioning since both affect cell dimensions.

---

### Pitfall 3: Wide Character Background Rect Uses Single Cell Width

**What goes wrong:**
When adding CJK/wide character support, the background rectangle for a wide character cell must span 2 * cell_width, but the spacer cell (WIDE_CHAR_SPACER) is currently skipped entirely. If the wide character's background color differs from default, the second cell appears as default background -- creating a visual "hole" behind CJK characters.

**Why it happens:**
The current `build_rects` iterates cells and draws `cell_width` wide rects. It doesn't check `Flags::WIDE_CHAR` to double the width. The spacer cell is correctly skipped in text rendering but its background still needs coverage. Developers fix text rendering (skip spacer) and forget that backgrounds need the inverse treatment (extend to cover spacer).

**How to avoid:**
In `build_rects`, when a cell has `Flags::WIDE_CHAR`, emit a rect with width `2 * cell_width`. When a cell has `Flags::WIDE_CHAR_SPACER`, skip it for background rects (since the wide char's rect already covers it). Apply the same logic to selection highlighting in `build_selection_rects`.

**Warning signs:**
- CJK characters have correct text but wrong/missing background on their right half
- Selection highlighting appears to have gaps on CJK text
- Cursor block appears half-width on wide characters

**Phase to address:**
Phase 3 (CJK/wide character support) -- specifically the rect rendering portion.

---

### Pitfall 4: glyphon TextArea.scale Breaks Alignment (Do NOT Use It for DPI)

**What goes wrong:**
Glyphon's `TextArea` has a `scale` field that appears to be the right place to handle DPI scaling. Using it causes text to render at the wrong horizontal position -- text moves "way to the right and off the screen" for any alignment other than Left. This is a confirmed bug in glyphon (GitHub issue #117).

**Why it happens:**
The `scale` parameter was added before cosmic-text had horizontal alignment support. It incorrectly multiplies alignment offsets by the scale factor, breaking positioning. The Glass codebase currently sets `scale: 1.0` (correct), but any DPI handling code that touches this field will break rendering.

**How to avoid:**
NEVER set `TextArea.scale` to anything other than `1.0`. Handle DPI by applying the scale factor to font metrics instead: `Metrics::new(font_size * scale_factor, line_height * scale_factor)`. This is the documented workaround from the glyphon issue. Add a code comment warning against changing `scale` with a link to the issue.

**Warning signs:**
- Text renders correctly at 100% DPI but is mispositioned at 125%, 150%, etc.
- Text works with Left alignment but breaks with Center alignment (status bar, overlays)
- Moving window between monitors causes text to jump to wrong position

**Phase to address:**
Phase 6 (Dynamic DPI handling) -- but the constraint must be documented from Phase 1 since per-cell positioning will be tempted to use `scale`.

---

### Pitfall 5: ScaleFactorChanged Without Full Pipeline Rebuild

**What goes wrong:**
When a window moves between monitors with different DPI (e.g., 100% to 150%), winit fires `ScaleFactorChanged`. Currently Glass only logs this event (line 1052 of main.rs). A naive fix updates just the surface size but not font metrics, cell dimensions, or the PTY's terminal size. Result: text appears too small/large, grid misaligns, and the terminal reports wrong column/row counts to the shell.

**Why it happens:**
DPI changes require rebuilding the entire rendering pipeline in the correct order: (1) update scale factor, (2) recalculate font metrics (cell_width, cell_height), (3) resize the wgpu surface, (4) resize the PTY terminal (new rows/cols), (5) clear the glyph atlas (old glyphs are wrong size), (6) trigger a full redraw. Missing any step causes subtle bugs. The existing `update_font` method does steps 1-2 but callers must also do 3-6.

**How to avoid:**
Create a single `handle_scale_factor_changed(new_factor)` method that performs ALL steps atomically in the correct order. Reuse the existing `update_font` path (which already handles font rebuild on config change) but add surface resize and PTY resize. Critically: call `atlas.trim()` or rebuild the atlas entirely, since rasterized glyphs at the old DPI are the wrong pixel size.

**Warning signs:**
- Text looks blurry after moving window to a different-DPI monitor
- Terminal content appears zoomed but cursor/selection rects are at original scale
- Shell commands report wrong COLUMNS/LINES after DPI change
- Box-drawing gaps appear only on one monitor but not another

**Phase to address:**
Phase 6 (Dynamic DPI handling) -- this is the entire purpose of that phase.

---

### Pitfall 6: Box-Drawing Characters Rendered Through Font Instead of Custom GPU Geometry

**What goes wrong:**
Box-drawing characters (U+2500-U+259F) rendered through the font's glyphs often have gaps at cell boundaries because font designers don't guarantee glyphs fill the exact cell dimensions. Even with correct line height, sub-pixel differences in font metrics vs cell metrics cause hairline gaps between adjacent box-drawing cells.

**Why it happens:**
Font glyphs are designed with their own internal metrics (bearings, advance width). These metrics rarely match exactly to terminal cell dimensions. The mismatch is invisible for text characters but catastrophic for box-drawing characters that must tile seamlessly.

**How to avoid:**
Render box-drawing characters (U+2500-U+259F) as custom GPU geometry using the rect renderer, not through the font/glyph pipeline. For each box-drawing codepoint, emit the appropriate lines/rectangles that exactly fill the cell bounds. This is what Alacritty does with its `builtin_box_drawing` feature. Start with the most common characters (single/double lines, corners, T-junctions) and add diagonals later. Powerline symbols (U+E0B0-U+E0B3) benefit from the same treatment.

**Warning signs:**
- Box-drawing borders have hairline gaps even with correct line height
- Gaps appear/disappear depending on font choice or font size
- Zoom level changes cause gaps to appear at certain sizes but not others

**Phase to address:**
Phase 2 (Line height) should include basic box-drawing GPU rendering. Could be a separate sub-phase if scope is too large.

---

### Pitfall 7: Font Fallback Causes Inconsistent Cell Width

**What goes wrong:**
When cosmic-text falls back to a different font for a missing glyph (e.g., emoji, CJK character), the fallback font may have different metrics than the primary font. The glyph's advance width from the fallback font doesn't match cell_width, causing subsequent glyphs on the line to shift.

**Why it happens:**
cosmic-text's `FontSystem` automatically discovers system fonts and falls back to them when the primary font lacks a glyph. This is correct behavior for text rendering, but terminal emulators need every glyph locked to cell_width regardless of which font provided it.

**How to avoid:**
With per-cell positioning (Pitfall 1 fix), each cell's TextArea has its left position set to `column * cell_width`, so fallback font metrics are irrelevant to positioning -- the grid forces alignment. This is why per-cell positioning MUST come before font fallback work. Additionally, for CJK fallback fonts, the glyph should be scaled/centered within the 2*cell_width space rather than rendered at its natural width.

**Warning signs:**
- Emoji or CJK characters cause all following text on the line to shift
- Different fallback fonts produce different amounts of drift
- Works perfectly with one system font set but breaks on another machine

**Phase to address:**
Phase 5 (Font fallback) -- but depends on Phase 1 (per-cell positioning) being complete first.

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Per-line Buffer (current) | Simple code, one Buffer per line | Horizontal drift, can't fix CJK properly | Never acceptable for a daily-driver terminal |
| 1.2x line height multiplier | Readable text spacing | Broken box-drawing, broken TUI apps | Never acceptable for terminal emulator |
| Skip spacer cells entirely | Simple wide char handling | Missing backgrounds, broken selection | Only in MVP before CJK support |
| Font glyph box-drawing | No custom rendering code needed | Hairline gaps between cells | Acceptable if CJK/TUI usage is rare |
| Single font family config | Simple configuration | Missing glyphs render as boxes | Acceptable until non-Latin users report issues |
| Log-only ScaleFactorChanged | Ship faster | Broken rendering on multi-monitor setups | Only acceptable during single-monitor development |

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| glyphon TextArea.scale | Using scale for DPI | Set scale=1.0, apply DPI to Metrics instead |
| cosmic-text Buffer sizing | Setting buffer width to viewport_width | For per-cell: set width to cell_width (or 2*cell_width for wide chars) |
| alacritty_terminal Flags | Checking WIDE_CHAR but not WIDE_CHAR_SPACER | Always handle both: render WIDE_CHAR at double width, skip WIDE_CHAR_SPACER |
| alacritty_terminal underline | Checking only UNDERLINE flag | Also handle DOUBLE_UNDERLINE, UNDERCURL, DOTTED_UNDERLINE, DASHED_UNDERLINE |
| wgpu surface resize | Resizing surface without reconfiguring | Must call surface.configure() after every resize, before next frame |
| winit ScaleFactorChanged | Treating it like a simple resize | Must rebuild fonts, cell metrics, PTY size, AND surface size |
| glyphon TextAtlas after DPI change | Keeping old atlas glyphs | Glyphs rasterized at old DPI are wrong size; trim or rebuild atlas |

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| One Buffer per cell (naive per-cell) | Frame time spikes on full-screen redraws (200+ cells/line * 50 lines = 10,000 Buffers) | Batch consecutive cells with identical attributes into one Buffer, position at first cell's column | Immediately on any 80x50 terminal |
| Rebuilding all Buffers every frame | 10-20ms frame time instead of <5ms | Cache Buffers and only rebuild lines that changed (dirty-line tracking) | Noticeable with fast-scrolling output (e.g., cargo build) |
| Atlas overflow with CJK + emoji + Latin | GPU memory grows unbounded, eventually OOM or atlas rebuild stall | Call atlas.trim() each frame (already done), monitor atlas size, consider atlas size cap | After ~30min session with diverse Unicode output |
| Custom box-drawing geometry per frame | Thousands of rect instances for complex TUI apps | Pre-compute box-drawing rects only when grid content changes, not every frame | Complex TUI apps (htop, btop) with full-screen box-drawing |
| Font fallback system font scan | FontSystem::new() scans all system fonts on first miss | Pre-warm FontSystem at startup (already done); ensure fallback scan doesn't block rendering thread | Systems with 500+ installed fonts |

## UX Pitfalls

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| Underline too thick (>1px at 1x DPI) | Underlined text looks bold/ugly, obscures descenders | Use 1px at 1x, scale with DPI: max(1, round(scale_factor)) |
| Underline on baseline instead of below descenders | Underline cuts through g, j, p, q, y | Position underline at descent line or slightly below baseline + descent |
| Strikethrough at wrong vertical position | Line through ascenders instead of through middle | Position at roughly 0.3 * (ascent + descent) above baseline |
| Curly underline (UNDERCURL) as straight line | Users expect wavy line for spell-check indicators | Render as sine wave segments using small rects or a dedicated shader |
| CJK text clipped at right edge | Last CJK character on a line is half-visible | Wrap CJK characters to next line when only 1 column remains (alacritty_terminal handles this, but verify rect clipping) |
| DPI change causes layout jump | Content appears to jump/flash during DPI transition | Apply all changes atomically in one frame, don't render intermediate states |

## "Looks Done But Isn't" Checklist

- [ ] **Per-cell positioning:** Often missing zero-width combining characters -- verify combining chars (accents, emoji modifiers) attach to the correct base cell
- [ ] **Box-drawing:** Often missing corner/junction characters -- verify U+250C (top-left), U+2514 (bottom-left), U+253C (cross) all connect seamlessly
- [ ] **Box-drawing:** Often missing block elements (U+2580-U+259F) -- verify half-blocks and shade characters render correctly
- [ ] **CJK support:** Often missing background rects for wide chars -- verify bg color spans full 2-cell width
- [ ] **CJK support:** Often missing cursor width -- verify cursor block covers 2 cells on wide characters
- [ ] **CJK support:** Often missing selection width -- verify selection highlight spans 2 cells for wide characters
- [ ] **Underline:** Often missing colored underlines -- verify `Flags::UNDERLINE_COLOR` from alacritty_terminal is respected (not just white)
- [ ] **Underline:** Often missing underline style variants -- verify DOUBLE_UNDERLINE, UNDERCURL, DOTTED, DASHED are visually distinct
- [ ] **Font fallback:** Often missing on Windows -- verify fallback works with CJK (MS Gothic/Yu Gothic), emoji (Segoe UI Emoji), and symbols
- [ ] **DPI handling:** Often missing PTY resize -- verify COLUMNS/LINES reported by `stty size` update after DPI change
- [ ] **DPI handling:** Often missing atlas invalidation -- verify text isn't blurry after moving between 1x and 2x displays

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Per-line drift shipped | MEDIUM | Replace build_text_buffers with per-cell approach; update build_text_areas to use per-cell offsets; no API changes to callers |
| Wrong line height shipped | LOW | Change one line in GridRenderer::new to use font metrics instead of 1.2x; rebuild cell dimensions; retest all renderers |
| Missing wide char backgrounds | LOW | Add WIDE_CHAR flag check in build_rects to double width; add WIDE_CHAR_SPACER skip; ~10 lines changed |
| TextArea.scale misuse | LOW | Set scale=1.0, move scaling to Metrics; ~5 lines changed but must audit all TextArea creation sites |
| DPI change partially handled | HIGH | Must wire ScaleFactorChanged through font rebuild, surface resize, PTY resize, atlas clear; touches main.rs + frame.rs + grid_renderer.rs |
| Font glyphs for box-drawing | MEDIUM | Add box-drawing renderer with codepoint-to-geometry mapping; ~200-400 lines; intercept in build_text_buffers to skip box chars |

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| Per-line horizontal drift | Phase 1: Per-cell positioning | Run `vim` with line numbers, verify column alignment across all 80+ columns |
| 1.2x line height gaps | Phase 2: Line height from metrics | Run `tmux` with pane borders, verify no horizontal gaps between rows |
| Box-drawing font gaps | Phase 2: Custom GPU box-drawing | Render U+2500-U+259F grid, verify all characters connect seamlessly |
| Wide char background holes | Phase 3: CJK support | Display CJK text with colored backgrounds, verify no gaps in bg rects |
| Wide char cursor/selection | Phase 3: CJK support | Place cursor on CJK character, verify block cursor spans 2 cells |
| Underline position/thickness | Phase 4: Underline/strikethrough | Display underlined text with descenders (g, j, y), verify underline doesn't clip them |
| Strikethrough position | Phase 4: Underline/strikethrough | Display strikethrough text, verify line is centered on x-height |
| Fallback font drift | Phase 5: Font fallback (depends on Phase 1) | Display mixed Latin + CJK + emoji on one line, verify grid alignment maintained |
| Fallback font missing on platform | Phase 5: Font fallback | Test on Windows, macOS, Linux with CJK and emoji content |
| TextArea.scale bug | Phase 6: DPI handling | Verify scale=1.0 is used everywhere; test on 150% DPI display |
| Partial DPI rebuild | Phase 6: DPI handling | Move window between 1x and 2x monitors, verify text/rects/PTY all update correctly |
| Atlas stale after DPI | Phase 6: DPI handling | After DPI change, verify text is crisp (not blurry from old-DPI cached glyphs) |

## Sources

- [glyphon TextArea::scale alignment bug (Issue #117)](https://github.com/grovesNL/glyphon/issues/117)
- [Alacritty builtin box drawing (Issue #5809)](https://github.com/alacritty/alacritty/issues/5809)
- [Alacritty box drawing rendering quality (Issue #7067)](https://github.com/alacritty/alacritty/issues/7067)
- [Warp: Adventures in Text Rendering](https://www.warp.dev/blog/adventures-text-rendering-kerning-glyph-atlases)
- [How Zutty works: GPU terminal rendering](https://tomscii.sig7.se/2020/11/How-Zutty-works)
- [Contour terminal text stack internals](https://contour-terminal.org/internals/text-stack/)
- [CJK ambiguous width in Windows Terminal (Issue #370)](https://github.com/microsoft/terminal/issues/370)
- [Ghostty CJK height constraint (Issue #8709)](https://github.com/ghostty-org/ghostty/issues/8709)
- [winit ScaleFactorChanged race conditions (Issue #2921)](https://github.com/rust-windowing/winit/issues/2921)
- [wgpu surface resize on scale change (Issue #1872)](https://github.com/gfx-rs/wgpu/issues/1872)
- [cosmic-text font fallback discussion (#14)](https://github.com/pop-os/cosmic-text/discussions/14)
- [cosmic-text configurable fallback (Issue #126)](https://github.com/pop-os/cosmic-text/issues/126)
- Glass codebase: `grid_renderer.rs` (lines 48-49: 1.2x multiplier, lines 239-248: WIDE_CHAR_SPACER skip)
- Glass codebase: `main.rs` (lines 1052-1054: log-only ScaleFactorChanged)
- Glass codebase: `frame.rs` (line 130: update_font method)

---
*Pitfalls research for: GPU terminal emulator rendering correctness*
*Researched: 2026-03-09*

# Project Research Summary

**Project:** Glass v2.4 -- Rendering Correctness
**Domain:** GPU terminal emulator text rendering
**Researched:** 2026-03-09
**Confidence:** HIGH

## Executive Summary

Glass v2.4 is a rendering correctness milestone for a GPU-accelerated terminal emulator. The core problem is that the current rendering pipeline uses per-line text buffers with a hardcoded 1.2x line height multiplier, causing two fundamental visual bugs: horizontal glyph drift (characters shift off-grid on longer lines, breaking TUI borders) and vertical gaps between lines (box-drawing characters don't connect). Every mature GPU terminal (Alacritty, Ghostty, Kitty, WezTerm) has solved these problems the same way: per-cell glyph positioning locked to grid coordinates and line height derived from font metrics rather than arbitrary multipliers.

The recommended approach requires zero new dependencies. The existing stack (glyphon 0.10.0 / cosmic-text 0.15.0 / wgpu 28.0 / alacritty_terminal 0.25.1) already exposes every API needed. The work is entirely architectural -- changing how `GridRenderer` builds GPU primitives. The central change is rewriting `build_text_buffers()` from one Buffer per line to one Buffer per cell, with each cell's TextArea positioned at exact grid coordinates (`column * cell_width, line * cell_height`). All six features (per-cell positioning, line height fix, CJK wide chars, underline/strikethrough, font fallback, dynamic DPI) share this foundation.

The primary risk is performance regression from the per-cell Buffer approach: going from ~50 Buffers per frame to ~2000-4000. Mitigation is straightforward (skip empty cells, reuse Vec capacity, glyphon's atlas caches glyphs regardless of Buffer count) but must be benchmarked immediately after implementation. A secondary risk is the confirmed glyphon bug where `TextArea.scale` breaks alignment (issue #117) -- DPI handling must scale font Metrics, never TextArea.scale.

## Key Findings

### Recommended Stack

No dependency changes required. The entire v2.4 milestone is implemented by changing how existing APIs are called.

**Core technologies (all unchanged):**
- **glyphon 0.10.0**: Text rendering via Buffer/TextArea/TextRenderer -- per-cell Buffers use the same API
- **cosmic-text 0.15.0**: Text shaping with built-in font fallback via fontdb -- provides LayoutRun.line_height for correct metrics
- **wgpu 28.0.0**: Existing instanced RectRenderer handles underline/strikethrough as thin rect quads
- **alacritty_terminal =0.25.1**: Already exposes all needed Flags (UNDERLINE, STRIKEOUT, WIDE_CHAR, DOUBLE_UNDERLINE, UNDERCURL, etc.)

**What NOT to add:** harfbuzz-rs (cosmic-text already handles shaping), fontdue/ab_glyph (redundant with cosmic-text), custom WGSL shaders for decorations (RectInstance quads suffice).

### Expected Features

**Must have (table stakes):**
- Per-cell glyph positioning -- every glyph at `column * cell_width`; without this, TUI apps (vim, htop, tmux) have misaligned borders
- Correct line height from font metrics -- `ascent + descent` instead of `font_size * 1.2`; fixes box-drawing gaps
- Wide character / CJK support -- WIDE_CHAR cells rendered at 2x cell_width with proper background rects
- Underline rendering (SGR 4) -- universally used by grep, compilers, TUI highlights
- Strikethrough rendering (SGR 9) -- used by diff tools and TUI frameworks
- Dynamic DPI handling -- ScaleFactorChanged already logged but ignored; required for multi-monitor setups

**Should have (differentiators):**
- Multiple underline styles (double, curly, dotted, dashed) -- alacritty_terminal already provides all five flags
- Colored underlines (SGR 58) -- used by LSP error highlighting in Neovim
- Font fallback configuration -- user-specified fallback font order in config.toml
- Built-in box-drawing character rendering -- custom GPU geometry for pixel-perfect borders

**Defer (v2+):**
- Font ligatures -- requires fundamentally different cell model, explicitly out of scope
- Image protocol support (Kitty, Sixel) -- orthogonal to text rendering
- Custom glyph atlas rendering -- only if profiling shows glyphon is a bottleneck
- Sub-pixel anti-aliasing -- let OS/GPU driver handle this
- Configurable line height / cell padding -- get correct defaults first

### Architecture Approach

The rendering pipeline has three layers: GridSnapshot (data), GridRenderer (GPU primitive generation), and FrameRenderer (orchestration). Only GridRenderer gets a major rewrite. The key change is replacing `build_text_buffers()` (one Buffer per line) with `build_cell_buffers()` (one Buffer per non-empty cell), and adding `build_decoration_rects()` for underline/strikethrough. FrameRenderer's draw order becomes: bg rects -> decoration rects -> text. All downstream consumers (BlockRenderer, StatusBarRenderer, TabBarRenderer) cascade automatically via `update_font()`.

**Major components affected:**
1. **GridRenderer** -- MAJOR CHANGE: per-cell positioning, line height, wide char rects, decoration rects
2. **FrameRenderer** -- MODERATE CHANGE: integrate decoration rects, update draw order
3. **main.rs** -- MINOR CHANGE: wire ScaleFactorChanged to update_font + PTY resize
4. **GridSnapshot / RectRenderer / GlyphCache** -- NO CHANGE: data layer and GPU pipeline untouched

### Critical Pitfalls

1. **Per-line Buffer horizontal drift** -- The current approach lets cosmic-text shape entire lines, causing cumulative glyph drift. Fix: per-cell Buffers with grid-locked TextArea positions. This is the foundational fix.

2. **1.2x line height multiplier** -- Hardcoded in `GridRenderer::new()`. Creates gaps between box-drawing rows. Fix: derive from font ascent + descent via cosmic-text metrics.

3. **glyphon TextArea.scale bug (issue #117)** -- Setting `scale` to anything other than 1.0 breaks horizontal alignment. NEVER use it for DPI. Fix: apply scale factor to `Metrics::new(font_size * scale, line_height * scale)` instead.

4. **Wide char background rect single-width** -- WIDE_CHAR cells need 2x cell_width backgrounds; spacer cells must be skipped entirely. Easy to miss because text rendering handles spacers correctly but background rendering doesn't.

5. **Incomplete DPI rebuild** -- ScaleFactorChanged requires rebuilding fonts, cell metrics, surface, PTY size, AND clearing the glyph atlas. Missing any step causes subtle rendering corruption. Must be atomic.

## Implications for Roadmap

Based on research, suggested phase structure:

### Phase 1: Line Height Fix
**Rationale:** Smallest code change with highest visual impact. Changes one calculation in GridRenderer::new(). Cascades correctly through update_font() to all sub-renderers. Must come first because it sets cell_height used by all subsequent phases.
**Delivers:** Box-drawing characters connect vertically; TUI borders render without gaps.
**Addresses:** Correct line height (table stakes), partial box-drawing fix.
**Avoids:** Pitfall 2 (1.2x multiplier breaks box-drawing).

### Phase 2: Per-Cell Glyph Positioning
**Rationale:** Core architectural change that all other features depend on. Biggest code change but well-understood pattern (all GPU terminals do this). Depends on Phase 1 for correct cell_height.
**Delivers:** Every glyph at exact grid position; TUI apps render correctly.
**Addresses:** Per-cell positioning (table stakes), fixes horizontal drift in all TUI apps.
**Avoids:** Pitfall 1 (horizontal drift), Pitfall 7 (fallback font width mismatch -- per-cell positioning eliminates this class of bug).

### Phase 3: Wide Character / CJK Support
**Rationale:** Builds directly on per-cell positioning (Phase 2). Without per-cell Buffers, wide chars cannot be correctly positioned. Includes background rect changes.
**Delivers:** CJK text renders at correct double width with proper backgrounds.
**Addresses:** Wide char / CJK support (table stakes).
**Avoids:** Pitfall 3 (single-width background rects for wide chars).

### Phase 4: Underline and Strikethrough
**Rationale:** Independent feature but benefits from correct cell positioning (Phase 2) for pixel-accurate decoration placement. Uses existing RectInstance pipeline -- zero new GPU code.
**Delivers:** UNDERLINE, STRIKEOUT rendering. Optionally DOUBLE_UNDERLINE.
**Addresses:** Underline rendering (table stakes), strikethrough rendering (table stakes).
**Avoids:** Anti-pattern of custom WGSL shaders for decorations.

### Phase 5: Font Fallback Configuration
**Rationale:** cosmic-text already does automatic fallback. This phase verifies it works and adds user configuration. Must come after per-cell positioning (Phase 2) because per-cell rendering eliminates fallback font width mismatch issues.
**Delivers:** CJK/emoji/symbol characters render via system font fallback; optional config.toml `font.fallback` array.
**Addresses:** Font fallback cascade (differentiator).
**Avoids:** Pitfall 7 (fallback font inconsistent cell width).

### Phase 6: Dynamic DPI Handling
**Rationale:** Isolated to main.rs event handler. Depends on update_font() working correctly (validated by earlier phases). Smallest change, least risky.
**Delivers:** Window moved between monitors with different DPI renders correctly.
**Addresses:** Dynamic DPI handling (table stakes).
**Avoids:** Pitfall 4 (TextArea.scale bug), Pitfall 5 (incomplete DPI rebuild).

### Phase 7: Polish and Deferred Decorations
**Rationale:** Extensions that build on the core work. Multiple underline styles, colored underlines, optional built-in box-drawing geometry.
**Delivers:** UNDERCURL, DOTTED, DASHED underlines; colored underlines (SGR 58); optional custom box-drawing rendering.
**Addresses:** Multiple underline styles (differentiator), colored underlines (differentiator), built-in box-drawing (differentiator).

### Phase Ordering Rationale

- **Phases 1-2 are strictly ordered:** Line height fix sets cell_height, per-cell positioning uses cell_height. Both must precede all other rendering work.
- **Phase 3 depends on Phase 2:** Wide chars need per-cell Buffers to set double-width sizing.
- **Phase 4 is semi-independent:** Could theoretically run in parallel with Phase 3, but sequential is safer since both modify GridRenderer.
- **Phase 5 depends on Phase 2:** Per-cell positioning eliminates the fallback font width mismatch problem, making fallback safe to enable.
- **Phase 6 is independent:** Only touches main.rs event handling, but validates the full pipeline rebuilt by earlier phases.
- **Phase 7 is optional polish:** All table-stakes features are complete after Phase 6.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 2 (Per-cell positioning):** Performance impact needs benchmarking. If >5ms per frame, batching strategies (consecutive identical-attribute cells into single Buffers) may be needed.
- **Phase 5 (Font fallback):** cosmic-text's automatic fallback quality/ordering is MEDIUM confidence. Needs testing with CJK, emoji, and Nerd Font symbols across platforms.
- **Phase 7 (Box-drawing GPU rendering):** ~100 characters to implement as procedural geometry. Scope may be too large for a single phase.

Phases with standard patterns (skip research-phase):
- **Phase 1 (Line height):** One-line change, well-documented font metric APIs.
- **Phase 4 (Underline/strikethrough):** Straightforward rect rendering, alacritty_terminal flags already verified.
- **Phase 6 (Dynamic DPI):** Pattern already proven by config hot-reload path.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | Verified from Cargo.lock; all APIs confirmed via docs.rs; zero new dependencies |
| Features | HIGH | Feature flags verified in alacritty_terminal =0.25.1; reference implementations studied (Alacritty, Ghostty, WezTerm) |
| Architecture | HIGH | Based on direct source code analysis; data flow and component boundaries verified |
| Pitfalls | HIGH | Confirmed against codebase (1.2x multiplier at grid_renderer.rs:49, log-only DPI at main.rs:1052); glyphon scale bug verified via GitHub issue |

**Overall confidence:** HIGH

### Gaps to Address

- **Per-cell Buffer performance:** No benchmarks yet. Must measure after Phase 2 implementation. If frame time exceeds 5ms, consider batching consecutive cells with identical attributes.
- **cosmic-text fallback quality:** Automatic fallback works in theory but quality/ordering on Windows vs macOS vs Linux is untested. Validate during Phase 5.
- **Underline color field:** alacritty_terminal 0.25.1's `Cell` may or may not expose `underline_color` directly. Needs verification at implementation time.
- **UNDERCURL rendering:** Rect approximation vs custom shader is a design decision deferred to Phase 7. Rect approximation is sufficient for MVP.
- **winit ScaleFactorChanged + inner_size_writer:** The event may provide a new PhysicalSize that must be applied. Exact winit 0.30 API needs verification during Phase 6.

## Sources

### Primary (HIGH confidence)
- [glyphon 0.10.0 docs](https://docs.rs/glyphon/0.10.0/glyphon/) -- TextArea, Buffer, CustomGlyph APIs
- [cosmic-text FontSystem](https://docs.rs/cosmic-text/latest/cosmic_text/struct.FontSystem.html) -- font fallback, Metrics, LayoutRun
- [alacritty_terminal 0.25.1 cell Flags](https://docs.rs/alacritty_terminal/0.25.1/alacritty_terminal/term/cell/struct.Flags.html) -- all rendering flags
- Glass codebase direct analysis -- grid_renderer.rs, frame.rs, main.rs, grid_snapshot.rs

### Secondary (MEDIUM confidence)
- [Ghostty font system](https://deepwiki.com/ghostty-org/ghostty/5.5-font-system) -- per-cell positioning and fallback patterns
- [COSMIC Terminal (cosmic-term)](https://github.com/pop-os/cosmic-term) -- reference implementation using same stack
- [glyphon TextArea.scale bug (Issue #117)](https://github.com/grovesNL/glyphon/issues/117) -- confirmed DPI scaling pitfall
- [Alacritty builtin box drawing (Issue #5809)](https://github.com/alacritty/alacritty/issues/5809) -- custom box-drawing approach

### Tertiary (LOW confidence)
- [Warp: Adventures in Text Rendering](https://www.warp.dev/blog/adventures-text-rendering-kerning-glyph-atlases) -- general GPU text rendering context
- [Bevy cosmic-text fallback issue](https://github.com/bevyengine/bevy/issues/16354) -- fallback sizing inconsistencies (different context but same library)

---
*Research completed: 2026-03-09*
*Ready for roadmap: yes*

# Phase 43: Font Fallback - Research

**Researched:** 2026-03-10
**Domain:** Font fallback via cosmic-text/glyphon for GPU terminal rendering
**Confidence:** HIGH

## Summary

Font fallback in Glass is largely already wired up by the existing stack. The project uses `glyphon 0.10` which wraps `cosmic-text`, and cosmic-text provides automatic font fallback when `Shaping::Advanced` is used (which Glass already uses everywhere). `FontSystem::new()` discovers all system fonts automatically. The `FontFallbackIter` in cosmic-text searches: (1) default font for requested attributes, (2) script-specific fallbacks, (3) common fallbacks, (4) all system fonts as final resort.

The main work for this phase is **validation and fixing edge cases**, not building fallback from scratch. The likely issues are: (a) fallback glyphs from proportional system fonts may not align to the monospace grid, (b) `set_monospace_width` may or may not correctly constrain fallback glyph widths, and (c) CJK fallback glyphs need double-width handling. The STATE.md explicitly notes "cosmic-text fallback quality on Windows untested -- validate during Phase 43."

**Primary recommendation:** Validate that the existing pipeline renders CJK and other non-Latin glyphs correctly, then fix any grid alignment issues found. No new dependencies needed.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| FONT-01 | Missing glyphs fall back to system fonts automatically via cosmic-text | Already enabled: `FontSystem::new()` loads system fonts, `Shaping::Advanced` triggers fallback. Validation needed to confirm it works on all platforms. |
| FONT-02 | Fallback glyphs render at correct size within the cell grid | `set_monospace_width` already constrains glyph width per cell. Need to verify fallback glyphs respect this, especially CJK double-width characters. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| glyphon | 0.10.0 | Text rendering on wgpu | Already in use, wraps cosmic-text |
| cosmic-text (transitive) | via glyphon 0.10 | Font discovery, shaping, fallback | Provides `FontSystem`, `Shaping::Advanced`, `FontFallbackIter` |

### Supporting
No new dependencies needed. The existing stack already has all font fallback machinery.

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| cosmic-text built-in fallback | Manual fontdb queries + custom fallback | Unnecessary complexity; cosmic-text handles this well |
| FontSystem::new() | FontSystem::new_with_locale_and_db_and_fallback() | Only needed if default fallback is insufficient; try default first |

**Installation:**
No new packages. Zero new dependencies (consistent with v2.4 decision).

## Architecture Patterns

### Current Font Pipeline (already in place)
```
FontSystem::new()           -- discovers all system fonts
  |
  v
Buffer::set_text(           -- per cell
  font_system,
  text,
  Attrs::new().family(Family::Name(&font_family)),
  Shaping::Advanced,        -- ENABLES font fallback
  None,
)
  |
  v
buffer.set_monospace_width(font_system, Some(buf_width))  -- grid alignment
buffer.shape_until_scroll(font_system, false)              -- triggers shaping + fallback
```

### Pattern 1: Fallback Resolution Order (cosmic-text internal)
**What:** When a glyph is missing from the primary font, cosmic-text's `FontFallbackIter` searches in order:
1. Default font with requested attributes (weight, style)
2. Script-specific fallbacks via `PlatformFallback` (e.g., "MS Gothic" for CJK on Windows)
3. Common fallbacks (platform-specific lists derived from Chromium/Firefox)
4. All system fonts as final resort

**When to use:** Automatic -- no code changes needed for basic fallback.

### Pattern 2: Monospace Width Constraint for Grid Alignment
**What:** `buffer.set_monospace_width(Some(cell_width))` forces glyphs to fit cell width. For wide chars, `buf_width = cell_width * 2.0`.
**When to use:** Already applied in `build_cell_buffers`. This is the key mechanism ensuring fallback glyphs align to the grid.

### Anti-Patterns to Avoid
- **Manually querying fontdb for fallback fonts:** cosmic-text handles this internally. Don't bypass.
- **Using Shaping::Basic:** Disables font fallback entirely. Never use for terminal content.
- **Measuring cell dimensions from fallback font:** Cell metrics MUST come from the primary font only. Fallback glyphs are squeezed/stretched to fit via `set_monospace_width`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Font fallback resolution | Custom font matching loop | `Shaping::Advanced` + `FontSystem::new()` | cosmic-text has platform-specific fallback lists from Chromium/Firefox |
| Glyph width normalization | Manual glyph scaling | `buffer.set_monospace_width()` | Handles proportional-to-monospace conversion |
| System font discovery | Manual font file scanning | `FontSystem::new()` | Uses fontdb, handles all platforms |

**Key insight:** The existing code path already has all the pieces. The phase is about validation and edge-case fixes, not new architecture.

## Common Pitfalls

### Pitfall 1: Fallback Glyph Vertical Misalignment
**What goes wrong:** A fallback font has different ascent/descent metrics than the primary font, causing glyphs to render too high or too low within the cell.
**Why it happens:** `Metrics::new(physical_font_size, cell_height)` sets the line height but individual glyph baselines come from the fallback font's metrics.
**How to avoid:** The per-cell Buffer approach already isolates each glyph. Verify that `cell_height` (from primary font) is used as the Buffer's line_height, which constrains vertical positioning.
**Warning signs:** CJK characters appear shifted up or down relative to Latin text on the same line.

### Pitfall 2: CJK Glyphs Rendering at Single Width
**What goes wrong:** A CJK character falls back to a font that treats it as single-width, causing it to render in one cell instead of two.
**Why it happens:** The `WIDE_CHAR` flag from alacritty_terminal correctly marks the character as double-width, but the fallback font might not have a double-width glyph.
**How to avoid:** The code already uses `buf_width = cell_width * 2.0` for wide chars and `set_monospace_width(Some(buf_width))`. This should force the glyph to span 2 cells regardless of fallback font metrics. Verify this works.
**Warning signs:** CJK text appears compressed or overlapping.

### Pitfall 3: Performance Regression from Fallback Font Loading
**What goes wrong:** First render of a missing glyph triggers synchronous font loading and shaping, causing a frame stutter.
**Why it happens:** cosmic-text loads fonts lazily on first use. System font files can be large (CJK fonts are 10-20MB).
**How to avoid:** `FontSystem::new()` indexes fonts at startup (already done in `GlyphCache::new`). Individual glyph shaping is cached by cosmic-text after first use. The existing approach should be fine for terminal use.
**Warning signs:** Visible delay when first CJK character appears.

### Pitfall 4: Font Weight Mismatch Blocking Fallback
**What goes wrong:** cosmic-text rejects fallback fonts because they don't match the requested weight (e.g., Bold).
**Why it happens:** Historical bug in cosmic-text (fixed via PR #224). If using an older version, strict weight matching could prevent fallback.
**How to avoid:** glyphon 0.10 should include the fix. Verify by testing bold CJK text.
**Warning signs:** Bold text shows tofu but regular weight text falls back correctly.

### Pitfall 5: Windows-Specific Font Discovery
**What goes wrong:** Some system fonts on Windows are not discovered by fontdb.
**Why it happens:** Windows stores fonts in `C:\Windows\Fonts` and per-user font directories. fontdb may not scan all locations.
**How to avoid:** `FontSystem::new()` should handle standard Windows font directories. Validate CJK fonts like "MS Gothic", "Yu Gothic", "Microsoft YaHei" are discoverable.
**Warning signs:** Fallback works on Linux/macOS but shows tofu on Windows.

## Code Examples

### Current build_cell_buffers (already supports fallback)
```rust
// Source: crates/glass_renderer/src/grid_renderer.rs:340-410
// Key lines that enable fallback:

let mut buffer = Buffer::new(font_system, metrics);
buffer.set_size(font_system, Some(buf_width), Some(self.cell_height));
buffer.set_monospace_width(font_system, Some(buf_width));

let mut attrs = Attrs::new()
    .family(Family::Name(&self.font_family))  // primary font
    .color(GlyphonColor::rgba(cell.fg.r, cell.fg.g, cell.fg.b, 255));

// Shaping::Advanced enables font fallback
buffer.set_text(font_system, s, &attrs, Shaping::Advanced, None);
buffer.shape_until_scroll(font_system, false);
```

### Test Pattern: Verify Fallback Glyph Has Non-Zero Width
```rust
// Verify that a CJK character produces a shaped glyph (not tofu)
#[test]
fn fallback_renders_cjk_glyph() {
    let mut font_system = FontSystem::new();
    let renderer = GridRenderer::new(&mut font_system, "Consolas", 14.0, 1.0);
    let physical = 14.0;
    let metrics = Metrics::new(physical, renderer.cell_height);
    let buf_width = renderer.cell_width * 2.0; // CJK = double width

    let mut buffer = Buffer::new(&mut font_system, metrics);
    buffer.set_size(&mut font_system, Some(buf_width), Some(renderer.cell_height));
    buffer.set_monospace_width(&mut font_system, Some(buf_width));
    buffer.set_text(
        &mut font_system,
        "\u{4E16}", // CJK character: "world"
        &Attrs::new().family(Family::Name("Consolas")),
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut font_system, false);

    // Should have at least one layout run with a glyph
    let run = buffer.layout_runs().next();
    assert!(run.is_some(), "CJK char should produce a layout run");
    let glyphs = &run.unwrap().glyphs;
    assert!(!glyphs.is_empty(), "CJK char should have at least one glyph (via fallback)");
}
```

### Test Pattern: Verify Fallback Glyph Grid Alignment
```rust
// Verify fallback glyph width is constrained to buf_width by set_monospace_width
#[test]
fn fallback_glyph_respects_monospace_width() {
    let mut font_system = FontSystem::new();
    let renderer = GridRenderer::new(&mut font_system, "Consolas", 14.0, 1.0);
    let physical = 14.0;
    let metrics = Metrics::new(physical, renderer.cell_height);
    let buf_width = renderer.cell_width * 2.0;

    let mut buffer = Buffer::new(&mut font_system, metrics);
    buffer.set_size(&mut font_system, Some(buf_width), Some(renderer.cell_height));
    buffer.set_monospace_width(&mut font_system, Some(buf_width));
    buffer.set_text(
        &mut font_system,
        "\u{4E16}",
        &Attrs::new().family(Family::Name("Consolas")),
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut font_system, false);

    if let Some(run) = buffer.layout_runs().next() {
        if let Some(glyph) = run.glyphs.first() {
            // Glyph width should be approximately buf_width (set_monospace_width constraint)
            assert!(
                (glyph.w - buf_width).abs() < 1.0,
                "Glyph width ({}) should be close to buf_width ({})",
                glyph.w, buf_width
            );
        }
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Manual font fallback lists | cosmic-text PlatformFallback trait | cosmic-text 0.11+ | Automatic per-platform fallback |
| Strict weight matching | Closest weight matching | cosmic-text PR #224 | Prevents fallback rejection due to weight mismatch |
| No configurable fallback | FontSystem::new_with_locale_and_db_and_fallback() | cosmic-text PR #369 (Mar 2025) | Custom fallback chains possible if needed |

**Deprecated/outdated:**
- `Shaping::Basic`: Does NOT trigger fallback. Must always use `Shaping::Advanced` for terminal content.

## Open Questions

1. **Does `set_monospace_width` correctly constrain fallback glyphs?**
   - What we know: It forces monospace glyph resizing. Used for all cells.
   - What's unclear: Whether fallback glyphs from proportional fonts respect this constraint.
   - Recommendation: Write a test that shapes a CJK char with Consolas as primary font and checks glyph.w matches buf_width. If not, may need post-shaping width adjustment.

2. **Which CJK fallback fonts are available on each platform?**
   - What we know: Windows has MS Gothic, Yu Gothic, Microsoft YaHei. macOS has Hiragino, PingFang. Linux has Noto CJK if installed.
   - What's unclear: Whether fontdb discovers all of them via `FontSystem::new()`.
   - Recommendation: Log available fonts during debug builds to verify discovery.

3. **Is there a visible baseline shift for fallback glyphs?**
   - What we know: Per-cell Buffer isolates each character. Metrics use primary font's cell_height.
   - What's unclear: Whether cosmic-text adjusts baseline for fallback font metrics within the constrained cell.
   - Recommendation: Visual testing with mixed Latin + CJK text on the same line.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in Rust test framework) |
| Config file | Cargo.toml workspace |
| Quick run command | `cargo test -p glass_renderer --lib` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| FONT-01 | CJK char produces layout run with glyph (not empty/tofu) | unit | `cargo test -p glass_renderer fallback_renders_cjk_glyph -- --exact` | Wave 0 |
| FONT-01 | Multiple script chars (Arabic, Cyrillic, CJK) all produce glyphs | unit | `cargo test -p glass_renderer fallback_renders_multi_script -- --exact` | Wave 0 |
| FONT-02 | Fallback glyph width matches buf_width via set_monospace_width | unit | `cargo test -p glass_renderer fallback_glyph_respects_monospace_width -- --exact` | Wave 0 |
| FONT-02 | build_cell_buffers produces correct buffer count with CJK cells | unit | `cargo test -p glass_renderer build_cell_buffers_handles_cjk_fallback -- --exact` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_renderer --lib`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `fallback_renders_cjk_glyph` test in grid_renderer.rs -- covers FONT-01
- [ ] `fallback_renders_multi_script` test in grid_renderer.rs -- covers FONT-01
- [ ] `fallback_glyph_respects_monospace_width` test in grid_renderer.rs -- covers FONT-02
- [ ] `build_cell_buffers_handles_cjk_fallback` test in grid_renderer.rs -- covers FONT-02

## Sources

### Primary (HIGH confidence)
- [cosmic-text FontSystem docs](https://docs.rs/cosmic-text/latest/cosmic_text/struct.FontSystem.html) - FontSystem::new() auto-discovers system fonts
- [cosmic-text Shaping enum](https://docs.rs/cosmic-text/latest/cosmic_text/enum.Shaping.html) - Shaping::Advanced enables font fallback
- [cosmic-text Buffer docs](https://docs.rs/cosmic-text/latest/cosmic_text/struct.Buffer.html) - set_monospace_width behavior
- [pop-os/cosmic-text DeepWiki](https://deepwiki.com/pop-os/cosmic-text) - FontFallbackIter resolution order, PlatformFallback trait

### Secondary (MEDIUM confidence)
- [cosmic-term issue #104](https://github.com/pop-os/cosmic-term/issues/104) - Fallback font weight matching fix (PR #224)
- [cosmic-text issue #126](https://github.com/pop-os/cosmic-text/issues/126) - Configurable fallback fonts (PR #369, Mar 2025)

### Tertiary (LOW confidence)
- `set_monospace_width` interaction with fallback glyphs -- not documented, needs empirical validation

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - using existing glyphon/cosmic-text, no new deps
- Architecture: HIGH - existing pipeline already has all mechanisms, just needs validation
- Pitfalls: MEDIUM - some edge cases around grid alignment with fallback fonts need empirical testing

**Research date:** 2026-03-10
**Valid until:** 2026-04-10 (stable libraries, unlikely to change)

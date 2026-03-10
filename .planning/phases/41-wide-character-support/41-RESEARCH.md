# Phase 41: Wide Character Support - Research

**Researched:** 2026-03-10
**Domain:** Terminal wide/CJK character rendering -- double-width glyph positioning, background rects, cursor, and selection
**Confidence:** HIGH

## Summary

Phase 41 adds correct rendering for CJK and other double-width Unicode characters. The terminal emulation layer (alacritty_terminal) already handles wide character semantics correctly: it marks the primary cell with `Flags::WIDE_CHAR` and the trailing cell with `Flags::WIDE_CHAR_SPACER`. The current renderer (Phase 40's per-cell Buffer approach) already skips `WIDE_CHAR_SPACER` cells, which is correct. However, it does NOT detect `WIDE_CHAR` cells, meaning wide characters are rendered into a single-cell-width Buffer with `set_monospace_width(Some(cell_width))`, which squeezes the CJK glyph to half its intended width.

The fix touches three areas of `grid_renderer.rs`: (1) text rendering -- detect `WIDE_CHAR` flag and create a Buffer sized to `2 * cell_width` without monospace width constraint (or with `set_monospace_width(Some(2 * cell_width))`), (2) background rects -- emit a double-width rect for cells with `WIDE_CHAR` flag, and (3) cursor/selection -- use double-width rects when the cursor or selection start lands on a wide character. There is also a `LEADING_WIDE_CHAR_SPACER` flag used for line-wrapping of wide chars (placeholder at end of line), which should be treated like `WIDE_CHAR_SPACER` and skipped in rendering.

**Primary recommendation:** In `build_cell_buffers`, check `Flags::WIDE_CHAR` to set buffer width to `2 * cell_width`. In `build_rects`, emit double-width background rects for `WIDE_CHAR` cells. In cursor and selection rendering, use double-width spans when on wide characters.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| WIDE-01 | CJK and other double-width characters render spanning 2 cell widths | Detect `Flags::WIDE_CHAR` in `build_cell_buffers()`, set Buffer size to `2 * cell_width`, use `set_monospace_width(Some(2 * cell_width))` to ensure glyph fills double-width space. TextArea positioned at primary cell's column, spanning 2 cells. |
| WIDE-02 | Cell backgrounds, cursor, and selection correctly span 2 cells for wide characters | In `build_rects()`, emit `2 * cell_width` background rects for `WIDE_CHAR` cells. In cursor rect logic, double the cursor width when on a `WIDE_CHAR`. In `build_selection_rects()`, account for wide chars spanning 2 columns. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| glyphon | 0.10.0 | wgpu text rendering (Buffer, TextArea) | Already in use; per-cell Buffer approach from Phase 40 |
| cosmic-text | 0.15.0 | Text shaping, layout, `set_monospace_width` | Already in use; `set_monospace_width` supports arbitrary widths including `2 * cell_width` |
| alacritty_terminal | =0.25.1 | VTE parsing, cell Flags (WIDE_CHAR, WIDE_CHAR_SPACER, LEADING_WIDE_CHAR_SPACER) | Already provides all wide character metadata |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| unicode-width | 0.2.2 | Unicode character width detection | Already in dependency tree (via alacritty_terminal). NOT needed for rendering -- alacritty_terminal already sets WIDE_CHAR flags |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Flag-based width detection | `unicode_width::UnicodeWidthChar` at render time | Redundant -- alacritty_terminal already computes widths and sets flags. Using flags is faster and consistent with terminal state. |
| `set_monospace_width(2*cw)` for wide chars | No monospace width, rely on natural glyph advance | Risky -- some CJK fonts may have glyphs slightly wider or narrower than exactly `2 * cell_width`. Using set_monospace_width guarantees exact grid fit. |

**Installation:**
```bash
# No new dependencies needed. All required APIs exist in current stack.
```

## Architecture Patterns

### Data Flow for Wide Characters

**alacritty_terminal grid representation:**
```
Column:  0    1    2    3    4    5    6
Char:    'H'  'e'  '漢'  ' '  '字'  ' '  '!'
Flags:   -    -    WIDE  SPACER WIDE SPACER -
```

Where:
- `WIDE_CHAR` (0x20): Primary cell of a wide character. Contains the actual char.
- `WIDE_CHAR_SPACER` (0x40): Trailing cell. Contains space char, exists for column accounting.
- `LEADING_WIDE_CHAR_SPACER` (0x400): Placed at end of line when wide char wraps to next line.

**Rendering flow (new):**
```
For each cell in snapshot.cells:
  if WIDE_CHAR_SPACER or LEADING_WIDE_CHAR_SPACER -> skip (no buffer, no rect)
  if WIDE_CHAR:
    Buffer width = 2 * cell_width
    set_monospace_width(Some(2 * cell_width))  // glyph fills double-width
    Background rect width = 2 * cell_width
    Cursor width = 2 * cell_width (if cursor on this cell)
  else (normal char):
    Buffer width = cell_width  (existing behavior)
    set_monospace_width(Some(cell_width))
```

### Files Changed
```
crates/glass_renderer/src/
  grid_renderer.rs    # PRIMARY: build_cell_buffers, build_rects, build_selection_rects, cursor
```

### Pattern 1: Wide Character Buffer Creation
**What:** Create double-width Buffer for WIDE_CHAR cells
**When to use:** In `build_cell_buffers()` when cell has `Flags::WIDE_CHAR`
**Example:**
```rust
// Source: alacritty_terminal Flags + cosmic-text Buffer API
for cell in &snapshot.cells {
    // Skip spacer cells (right half of wide chars, or line-wrap placeholders)
    if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
        || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
    {
        continue;
    }
    if cell.c == ' ' && cell.zerowidth.is_empty() {
        continue;
    }

    let is_wide = cell.flags.contains(Flags::WIDE_CHAR);
    let buf_width = if is_wide { self.cell_width * 2.0 } else { self.cell_width };

    let mut buffer = Buffer::new(font_system, metrics);
    buffer.set_size(font_system, Some(buf_width), Some(self.cell_height));
    buffer.set_monospace_width(font_system, Some(buf_width));

    // ... set_text, shape_until_scroll as before ...

    buffers.push(buffer);
    positions.push((col, line));
}
```

### Pattern 2: Wide Character Background Rects
**What:** Emit double-width background rects for WIDE_CHAR cells
**When to use:** In `build_rects()` for cell backgrounds
**Example:**
```rust
for cell in &snapshot.cells {
    // Skip spacer cells -- their bg is handled by the primary WIDE_CHAR cell
    if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
        || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
    {
        continue;
    }

    let is_wide = cell.flags.contains(Flags::WIDE_CHAR);
    let rect_width = if is_wide { self.cell_width * 2.0 } else { self.cell_width };

    if cell.bg != default_bg {
        let x = cell.point.column.0 as f32 * self.cell_width;
        let y = (cell.point.line.0 + line_offset) as f32 * self.cell_height;
        rects.push(RectInstance {
            pos: [x, y, rect_width, self.cell_height],
            color: rgb_to_color(cell.bg, 1.0),
        });
    }
}
```

### Pattern 3: Wide Character Cursor
**What:** Double-width cursor when on a WIDE_CHAR cell
**When to use:** In `build_rects()` cursor section
**Example:**
```rust
// Determine if cursor is on a wide char
let cursor_is_wide = snapshot.cells.iter().any(|c| {
    c.point == cursor.point && c.flags.contains(Flags::WIDE_CHAR)
});
let cursor_cell_width = if cursor_is_wide { self.cell_width * 2.0 } else { self.cell_width };

match cursor.shape {
    CursorShape::Block => {
        rects.push(RectInstance {
            pos: [cursor_x, cursor_y, cursor_cell_width, self.cell_height],
            color: cursor_color,
        });
    }
    // Beam: stays 2px wide (independent of character width)
    CursorShape::Beam => { /* unchanged */ }
    CursorShape::Underline => {
        rects.push(RectInstance {
            pos: [cursor_x, cursor_y + self.cell_height - 2.0, cursor_cell_width, 2.0],
            color: cursor_color,
        });
    }
    CursorShape::HollowBlock => {
        // Use cursor_cell_width for all four edges
    }
    CursorShape::Hidden => {}
}
```

### Anti-Patterns to Avoid
- **Checking unicode_width at render time:** Do NOT call `UnicodeWidthChar::width()` in the renderer. alacritty_terminal already computed widths and set WIDE_CHAR flags. Using flags is O(1) bitwise check vs unicode table lookup.
- **Rendering WIDE_CHAR_SPACER backgrounds separately:** The spacer cell may have a different bg color due to how alacritty_terminal resolves attributes. However, in practice, both the primary and spacer cells share the same background. Rendering the primary cell's bg at double-width and skipping the spacer's bg is correct and avoids overdraw.
- **Forgetting LEADING_WIDE_CHAR_SPACER:** This flag appears on the last column of a line when a wide char wraps to the next line. It must also be skipped in buffer creation AND rect rendering, not just WIDE_CHAR_SPACER.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Wide character detection | Custom unicode width table | `Flags::WIDE_CHAR` from alacritty_terminal | Terminal already computed widths correctly; flags are authoritative |
| Double-width glyph sizing | Custom glyph scaling | `set_monospace_width(Some(2 * cell_width))` | cosmic-text handles glyph resizing internally |
| Wide char background spanning | Two separate rect draws | Single double-width rect | Fewer draw calls, no overdraw seam |

**Key insight:** alacritty_terminal does the hard work of determining character widths according to Unicode standards. The renderer's job is simply to read the WIDE_CHAR flag and use double-width for everything: buffer, rect, cursor, selection.

## Common Pitfalls

### Pitfall 1: WIDE_CHAR_SPACER Background Color Mismatch
**What goes wrong:** The spacer cell might have a different background color than the primary cell (e.g., if an application sets background per-column).
**Why it happens:** alacritty_terminal assigns attributes independently to each cell column.
**How to avoid:** For the double-width background rect, check if the spacer cell has a different bg. If so, render TWO cell-width rects with different colors instead of one double-width rect. This is an edge case -- start with double-width from primary cell's bg, address mismatches only if observed.
**Warning signs:** CJK characters showing split background colors, or wrong background on the right half.

### Pitfall 2: Cursor on WIDE_CHAR_SPACER Column
**What goes wrong:** If the cursor's column index points to the SPACER cell (column N+1), the cursor renders at the wrong position.
**Why it happens:** In some terminal states (e.g., vi mode navigation), the cursor can land on the spacer cell.
**How to avoid:** alacritty_terminal's cursor handling should always normalize cursor position to the primary wide char cell. But defensively: if cursor.point matches a WIDE_CHAR_SPACER, shift cursor_x back by one cell_width and render double-width.
**Warning signs:** Cursor appearing offset by one cell when on a CJK character.

### Pitfall 3: Selection Range Not Accounting for Wide Chars
**What goes wrong:** Selection highlight covers only 1 cell width for wide characters.
**Why it happens:** `build_selection_rects()` iterates by column index, treating each column as cell_width wide.
**How to avoid:** The current selection logic works by column range, which naturally handles wide chars because the selection range includes both the primary and spacer columns. A wide char at column 2 means columns 2 and 3 are selected, producing `3 - 2 + 1 = 2` cells of width, which equals `2 * cell_width`. This should work correctly WITHOUT changes. Verify with tests.
**Warning signs:** Selection highlight not covering the full width of CJK characters.

### Pitfall 4: set_monospace_width Squeezing Wide Glyphs
**What goes wrong:** If `set_monospace_width(Some(cell_width))` is applied to a wide character's Buffer, cosmic-text will rescale the glyph to fit within one cell width, making it tiny and unreadable.
**Why it happens:** `set_monospace_width` treats the provided width as the target for ALL glyphs in the buffer. For CJK, this must be `2 * cell_width`.
**How to avoid:** Always check `Flags::WIDE_CHAR` before calling `set_monospace_width` and use `2 * cell_width` for wide chars. This is the core change for WIDE-01.
**Warning signs:** CJK characters appearing half-width, squished, or overlapping.

### Pitfall 5: LEADING_WIDE_CHAR_SPACER Not Handled
**What goes wrong:** A phantom space character renders at the last column of lines where a wide char wraps.
**Why it happens:** alacritty_terminal inserts a placeholder cell with `LEADING_WIDE_CHAR_SPACER` flag at the end of a line when a wide char doesn't fit. Current code doesn't check this flag.
**How to avoid:** Add `LEADING_WIDE_CHAR_SPACER` to the skip conditions in both `build_cell_buffers()` and `build_rects()`.
**Warning signs:** Extra characters or artifacts at the end of lines containing wrapped wide characters.

## Code Examples

### Example 1: Detecting Wide Characters in build_cell_buffers
```rust
// Source: alacritty_terminal cell.rs Flags, grid_renderer.rs build_cell_buffers
let is_wide = cell.flags.contains(Flags::WIDE_CHAR);
let buf_width = if is_wide { self.cell_width * 2.0 } else { self.cell_width };

let mut buffer = Buffer::new(font_system, metrics);
buffer.set_size(font_system, Some(buf_width), Some(self.cell_height));
// Critical: monospace width must match buffer width
buffer.set_monospace_width(font_system, Some(buf_width));
```

### Example 2: How set_monospace_width Works Internally
```rust
// Source: cosmic-text 0.15.0 shape.rs lines 1597-1617
// cosmic-text computes: match_mono_em_width = match_mono_width / font_size
// Then for each glyph: if glyph's font monospace em_width differs from target,
// it scales the glyph_font_size to make the glyph fit the target width.
// This means set_monospace_width(Some(2 * cell_width)) will resize CJK glyphs
// to exactly fill 2 * cell_width -- which is correct for double-width rendering.
```

### Example 3: Full Skip Conditions for Spacer Cells
```rust
// Skip ALL spacer types -- they have no content to render
if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
    || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
{
    continue;
}
// Can also use intersects for a single check:
if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
    continue;
}
```

### Example 4: TextArea Width for Wide Characters
```rust
// In build_cell_text_areas_offset, the TextArea bounds handle double-width
// automatically because the Buffer itself is sized to 2*cell_width.
// The TextArea.left is positioned at the primary cell's column:
//   left = x_offset + col * cell_width
// The Buffer's internal layout handles the glyph within 2*cell_width.
// No special TextArea changes needed beyond what the Buffer provides.
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Ignore wide char flags | Per-cell Buffer with flag-based width detection | Standard practice in all modern terminals | CJK characters display correctly |
| Single-width everything | Double-width buffer + double-width background rects for WIDE_CHAR | Standard since unicode-width standardization | Correct column alignment with mixed ASCII/CJK |

**Related terminals:**
- **Alacritty:** Uses custom OpenGL renderer with explicit double-width handling based on same WIDE_CHAR flags
- **cosmic-term:** Uses cosmic-text with `set_monospace_width` for terminal rendering (issue #369 fixed in Nov 2025 via cosmic-text update)
- **WezTerm:** Custom rendering with explicit width-2 handling

## Open Questions

1. **set_monospace_width behavior with CJK glyphs in cosmic-text 0.15.0**
   - What we know: cosmic-text rescales glyphs to match target monospace width via font size adjustment (shape.rs lines 1597-1617). Setting `2 * cell_width` should make CJK glyphs fill the double-width space.
   - What's unclear: Whether CJK fonts' `font_monospace_em_width` metric is correctly set in cosmic-text 0.15.0. If the font doesn't report a monospace em width, the rescaling is skipped and natural glyph advance is used instead.
   - Recommendation: Implement with `set_monospace_width(Some(2 * cell_width))`. If CJK glyphs appear incorrectly sized, fall back to `set_monospace_width(None)` for wide chars and rely on the Buffer's `set_size` to clip/contain the glyph.

2. **WIDE_CHAR_SPACER background color divergence**
   - What we know: In typical terminal usage, both the primary and spacer cells share the same bg.
   - What's unclear: Whether any real-world application sets different bg on primary vs spacer cells.
   - Recommendation: Start with double-width rect using primary cell's bg. If bug reports surface, add spacer bg comparison.

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
| WIDE-01 | Wide char Buffer uses 2*cell_width | unit | `cargo test -p glass_renderer wide_char_buffer` | No -- Wave 0 |
| WIDE-01 | WIDE_CHAR_SPACER and LEADING_WIDE_CHAR_SPACER skipped | unit | `cargo test -p glass_renderer spacer_skipped` | Partial -- existing test checks WIDE_CHAR_SPACER but not LEADING |
| WIDE-02 | Wide char background rect is double-width | unit | `cargo test -p glass_renderer wide_char_bg_rect` | No -- Wave 0 |
| WIDE-02 | Cursor on wide char is double-width | unit | `cargo test -p glass_renderer wide_char_cursor` | No -- Wave 0 |
| WIDE-02 | Selection across wide chars has correct width | unit | `cargo test -p glass_renderer wide_char_selection` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_renderer`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before /gsd:verify-work

### Wave 0 Gaps
- [ ] `crates/glass_renderer/src/grid_renderer.rs` -- add tests:
  - Wide char Buffer is created with `2 * cell_width` size
  - LEADING_WIDE_CHAR_SPACER cells are skipped in build_cell_buffers
  - Wide char background rect is `2 * cell_width` wide
  - Cursor rect is double-width when cursor is on WIDE_CHAR cell
  - Position tracking for wide chars uses primary cell column (not spacer)
- [ ] Update existing `build_cell_buffers_skips_spaces_and_spacers` test to include LEADING_WIDE_CHAR_SPACER

## Sources

### Primary (HIGH confidence)
- alacritty_terminal 0.25.1 source: `term/cell.rs` -- Flags::WIDE_CHAR (0x20), WIDE_CHAR_SPACER (0x40), LEADING_WIDE_CHAR_SPACER (0x400)
- alacritty_terminal 0.25.1 source: `term/mod.rs` lines 1107-1128 -- wide char write logic (sets WIDE_CHAR on primary, WIDE_CHAR_SPACER on following cell)
- alacritty_terminal 0.25.1 source: `vi_mode.rs` tests -- confirms pattern: WIDE_CHAR on primary cell, WIDE_CHAR_SPACER on next
- cosmic-text 0.15.0 source: `shape.rs` lines 1597-1617 -- `match_mono_width` rescaling logic
- cosmic-text 0.15.0 source: `buffer.rs` lines 577-593 -- `set_monospace_width` API
- Glass source: `grid_renderer.rs` -- current per-cell Buffer approach (Phase 40)

### Secondary (MEDIUM confidence)
- [cosmic-term issue #369](https://github.com/pop-os/cosmic-term/issues/369) -- wide character rendering fix via cosmic-text update, resolved Nov 2025
- [cosmic-text Buffer docs](https://docs.rs/cosmic-text/latest/cosmic_text/struct.Buffer.html) -- set_monospace_width documentation

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - no new dependencies, all APIs verified in source
- Architecture: HIGH - alacritty_terminal flag semantics verified in source, rendering changes are straightforward
- Pitfalls: HIGH - verified by reading cosmic-text layout engine source code; LEADING_WIDE_CHAR_SPACER documented in alacritty source

**Research date:** 2026-03-10
**Valid until:** 2026-04-10 (stable stack, pinned versions)

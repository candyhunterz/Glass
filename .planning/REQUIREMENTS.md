# Requirements: Glass

**Defined:** 2026-03-09
**Core Value:** A terminal that looks and feels normal but passively watches, indexes, and snapshots everything -- surfacing intelligence only when you need it.

## v2.4 Requirements

Requirements for rendering correctness milestone. Each maps to roadmap phases.

### Grid Alignment

- [x] **GRID-01**: Terminal renders each glyph at exactly column * cell_width, eliminating horizontal drift
- [x] **GRID-02**: Line height derived from font ascent+descent metrics, box-drawing characters connect seamlessly vertically

### Wide Characters

- [ ] **WIDE-01**: CJK and other double-width characters render spanning 2 cell widths
- [ ] **WIDE-02**: Cell backgrounds, cursor, and selection correctly span 2 cells for wide characters

### Text Decorations

- [ ] **DECO-01**: Underlined text renders with a 1px line below the baseline
- [ ] **DECO-02**: Strikethrough text renders with a 1px line through the middle

### Font Handling

- [ ] **FONT-01**: Missing glyphs fall back to system fonts automatically via cosmic-text
- [ ] **FONT-02**: Fallback glyphs render at correct size within the cell grid

### DPI Handling

- [ ] **DPI-01**: ScaleFactorChanged event triggers full font metric recalculation and surface rebuild
- [ ] **DPI-02**: Terminal remains correctly rendered after moving window between displays with different DPI

## Future Requirements

Deferred to future release. Tracked but not in current roadmap.

### Advanced Decorations

- **DECO-03**: Double underline rendering
- **DECO-04**: Dashed underline rendering
- **DECO-05**: Dotted underline rendering
- **DECO-06**: Undercurl (wavy) underline rendering via custom WGSL shader
- **DECO-07**: Colored underlines (separate underline color from fg color)

### Custom Box Drawing

- **BOXD-01**: Custom GPU geometry for box-drawing characters (U+2500-U+257F) instead of font glyphs
- **BOXD-02**: Custom GPU geometry for block elements (U+2580-U+259F)

## Out of Scope

| Feature | Reason |
|---------|--------|
| Font ligatures | Requires HarfBuzz shaping pipeline, separate milestone |
| Image protocols (Kitty, Sixel) | Separate rendering layer, not needed yet |
| Custom color schemes/themes | Not rendering correctness |
| Emoji rendering | Requires color font support (COLR/CBDT), separate milestone |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| GRID-01 | Phase 40 | Complete |
| GRID-02 | Phase 40 | Complete |
| WIDE-01 | Phase 41 | Pending |
| WIDE-02 | Phase 41 | Pending |
| DECO-01 | Phase 42 | Pending |
| DECO-02 | Phase 42 | Pending |
| FONT-01 | Phase 43 | Pending |
| FONT-02 | Phase 43 | Pending |
| DPI-01 | Phase 44 | Pending |
| DPI-02 | Phase 44 | Pending |

**Coverage:**
- v2.4 requirements: 10 total
- Mapped to phases: 10
- Unmapped: 0

---
*Requirements defined: 2026-03-09*
*Last updated: 2026-03-10 after roadmap creation*

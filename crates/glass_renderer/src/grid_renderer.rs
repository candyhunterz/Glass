//! GridRenderer: converts GridSnapshot cells into glyphon TextAreas and RectInstances.
//!
//! This bridges the terminal grid data (RenderedCell with resolved RGB colors)
//! to the GPU rendering primitives: colored rectangles for backgrounds/cursor,
//! and glyphon TextAreas for text content.

use alacritty_terminal::selection::SelectionRange;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{CursorShape, Rgb};
use glyphon::{
    Attrs, Buffer, Color as GlyphonColor, Family, FontSystem, Metrics, Shaping, Style, TextArea,
    TextBounds, Weight,
};

use glass_terminal::GridSnapshot;

use crate::rect_renderer::RectInstance;

/// Converts terminal grid data to GPU rendering primitives.
///
/// Computes cell dimensions from font metrics and produces both
/// colored rectangles (backgrounds, cursor) and text content (via glyphon).
pub struct GridRenderer {
    /// Cell width in physical pixels
    pub cell_width: f32,
    /// Cell height in physical pixels
    pub cell_height: f32,
    /// Font size in logical pixels
    pub font_size: f32,
    /// Scale factor (DPI)
    pub scale_factor: f32,
    /// Font family name (stored for text area creation)
    pub font_family: String,
}

impl GridRenderer {
    /// Create a GridRenderer by measuring cell dimensions from font metrics.
    ///
    /// Uses cosmic-text to shape a reference character ("M") and derive:
    /// - `cell_width` from the glyph advance width
    /// - `cell_height` from `LayoutRun.line_height` (font ascent+descent), NOT a hardcoded 1.2x multiplier
    ///
    /// The font-metric cell height ensures box-drawing characters connect seamlessly
    /// between adjacent lines (no inter-line gaps).
    pub fn new(
        font_system: &mut FontSystem,
        font_family: &str,
        font_size: f32,
        scale_factor: f32,
    ) -> Self {
        let physical_font_size = font_size * scale_factor;
        // Use font_size as initial line_height to measure natural font metrics
        let metrics = Metrics::new(physical_font_size, physical_font_size);

        // Measure cell dimensions by shaping "M" and reading font metrics
        let mut measure_buf = Buffer::new(font_system, metrics);
        measure_buf.set_size(font_system, Some(1000.0), Some(physical_font_size * 2.0));
        measure_buf.set_text(
            font_system,
            "M",
            &Attrs::new().family(Family::Name(font_family)),
            Shaping::Advanced,
            None,
        );
        measure_buf.shape_until_scroll(font_system, false);

        let (cell_width, cell_height) = measure_buf
            .layout_runs()
            .next()
            .map(|run| {
                let w = run
                    .glyphs
                    .first()
                    .map(|g| g.w)
                    .unwrap_or(physical_font_size * 0.6);
                // Derive cell_height from font metrics with safety floor
                let h = run.line_height.max(physical_font_size).ceil();
                (w, h)
            })
            .unwrap_or((physical_font_size * 0.6, physical_font_size.ceil()));

        GridRenderer {
            cell_width,
            cell_height,
            font_size,
            scale_factor,
            font_family: font_family.to_string(),
        }
    }

    /// Returns (cell_width, cell_height) in physical pixels.
    pub fn cell_size(&self) -> (f32, f32) {
        (self.cell_width, self.cell_height)
    }

    /// Build colored rectangle instances for cell backgrounds and cursor.
    ///
    /// Creates a RectInstance for each cell whose background differs from the
    /// default background, plus a cursor rectangle based on cursor shape.
    #[cfg_attr(feature = "perf", tracing::instrument(skip_all))]
    pub fn build_rects(&self, snapshot: &GridSnapshot, default_bg: Rgb) -> Vec<RectInstance> {
        let mut rects = Vec::with_capacity(snapshot.cells.len() / 4); // estimate ~25% non-default bg
        let line_offset = snapshot.display_offset as i32;

        // Cell background rects
        for cell in &snapshot.cells {
            // Skip spacer cells -- covered by the primary wide char's double-width rect
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            if cell.bg != default_bg {
                let is_wide = cell.flags.contains(Flags::WIDE_CHAR);
                let rect_width = if is_wide {
                    self.cell_width * 2.0
                } else {
                    self.cell_width
                };
                let x = cell.point.column.0 as f32 * self.cell_width;
                let y = (cell.point.line.0 + line_offset) as f32 * self.cell_height;
                rects.push(RectInstance {
                    pos: [x, y, rect_width, self.cell_height],
                    color: rgb_to_color(cell.bg, 1.0),
                });
            }
        }

        // Cursor rect
        let cursor = &snapshot.cursor;
        let cursor_x = cursor.point.column.0 as f32 * self.cell_width;
        let cursor_y = (cursor.point.line.0 + line_offset) as f32 * self.cell_height;
        let cursor_color = [0.8, 0.8, 0.8, 0.7]; // semi-transparent light gray

        // Determine if cursor is on a wide char cell for double-width cursor
        let cursor_is_wide = snapshot
            .cells
            .iter()
            .any(|c| c.point == cursor.point && c.flags.contains(Flags::WIDE_CHAR));
        let cursor_cell_width = if cursor_is_wide {
            self.cell_width * 2.0
        } else {
            self.cell_width
        };

        match cursor.shape {
            CursorShape::Block => {
                rects.push(RectInstance {
                    pos: [cursor_x, cursor_y, cursor_cell_width, self.cell_height],
                    color: cursor_color,
                });
            }
            CursorShape::Beam => {
                // Beam cursor stays 2px wide regardless of wide char
                rects.push(RectInstance {
                    pos: [cursor_x, cursor_y, 2.0, self.cell_height],
                    color: cursor_color,
                });
            }
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
            CursorShape::HollowBlock => {
                // Draw 4 edges of the block
                let t = 1.0; // border thickness
                             // Top
                rects.push(RectInstance {
                    pos: [cursor_x, cursor_y, cursor_cell_width, t],
                    color: cursor_color,
                });
                // Bottom
                rects.push(RectInstance {
                    pos: [
                        cursor_x,
                        cursor_y + self.cell_height - t,
                        cursor_cell_width,
                        t,
                    ],
                    color: cursor_color,
                });
                // Left
                rects.push(RectInstance {
                    pos: [cursor_x, cursor_y, t, self.cell_height],
                    color: cursor_color,
                });
                // Right
                rects.push(RectInstance {
                    pos: [
                        cursor_x + cursor_cell_width - t,
                        cursor_y,
                        t,
                        self.cell_height,
                    ],
                    color: cursor_color,
                });
            }
            CursorShape::Hidden => {
                // No cursor rect
            }
        }

        rects
    }

    /// Build selection highlight rectangles.
    ///
    /// Creates semi-transparent rectangles over selected cells based on the
    /// selection range from the terminal.
    pub fn build_selection_rects(
        &self,
        selection: &SelectionRange,
        display_offset: usize,
        columns: usize,
    ) -> Vec<RectInstance> {
        let mut rects = Vec::new();
        let line_offset = display_offset as i32;
        let selection_color = [0.26, 0.52, 0.96, 0.35]; // blue highlight

        let start = selection.start;
        let end = selection.end;

        for line_val in start.line.0..=end.line.0 {
            let col_start = if line_val == start.line.0 || selection.is_block {
                start.column.0
            } else {
                0
            };
            let col_end = if line_val == end.line.0 || selection.is_block {
                end.column.0
            } else {
                columns.saturating_sub(1)
            };

            let x = col_start as f32 * self.cell_width;
            let y = (line_val + line_offset) as f32 * self.cell_height;
            let w = (col_end - col_start + 1) as f32 * self.cell_width;

            rects.push(RectInstance {
                pos: [x, y, w, self.cell_height],
                color: selection_color,
            });
        }

        rects
    }

    /// Build rects with a pixel offset applied to all positions.
    ///
    /// Used for split pane rendering where each pane's content is offset
    /// to its viewport position within the window.
    pub fn build_rects_offset(
        &self,
        snapshot: &GridSnapshot,
        default_bg: Rgb,
        x_offset: f32,
        y_offset: f32,
    ) -> Vec<RectInstance> {
        let mut rects = self.build_rects(snapshot, default_bg);
        for rect in &mut rects {
            rect.pos[0] += x_offset;
            rect.pos[1] += y_offset;
        }
        rects
    }

    /// Build per-cell glyphon Buffers for grid-locked rendering.
    ///
    /// Creates one Buffer per non-empty terminal cell, skipping spaces and
    /// WIDE_CHAR_SPACER cells. Each Buffer uses `set_monospace_width` to ensure
    /// all glyphs are exactly cell_width wide, preventing horizontal drift.
    ///
    /// Cell positions are tracked in a parallel `positions` vec to guarantee
    /// correct TextArea placement (avoids buffer-TextArea index mismatch).
    #[cfg_attr(feature = "perf", tracing::instrument(skip_all))]
    pub fn build_cell_buffers(
        &self,
        font_system: &mut FontSystem,
        snapshot: &GridSnapshot,
        buffers: &mut Vec<Buffer>,
        positions: &mut Vec<(usize, i32)>,
    ) {
        let physical_font_size = self.font_size * self.scale_factor;
        let metrics = Metrics::new(physical_font_size, self.cell_height);
        let line_offset = snapshot.display_offset as i32;
        let mut char_buf = [0u8; 4]; // stack buffer for zero-alloc char encoding

        for cell in &snapshot.cells {
            // Skip spacer cells (right half of wide chars & leading spacer at line end)
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            // Skip empty/space-only cells (no Buffer needed)
            if cell.c == ' ' && cell.zerowidth.is_empty() {
                continue;
            }

            // Wide chars get double-width buffers for proper CJK rendering
            let is_wide = cell.flags.contains(Flags::WIDE_CHAR);
            let buf_width = if is_wide {
                self.cell_width * 2.0
            } else {
                self.cell_width
            };

            let mut buffer = Buffer::new(font_system, metrics);
            buffer.set_size(font_system, Some(buf_width), Some(self.cell_height));
            // Force all glyphs to buf_width for grid snapping
            buffer.set_monospace_width(font_system, Some(buf_width));

            // Build text attributes
            let mut attrs = Attrs::new()
                .family(Family::Name(&self.font_family))
                .color(GlyphonColor::rgba(cell.fg.r, cell.fg.g, cell.fg.b, 255));
            if cell.flags.contains(Flags::BOLD) {
                attrs = attrs.weight(Weight::BOLD);
            }
            if cell.flags.contains(Flags::ITALIC) {
                attrs = attrs.style(Style::Italic);
            }

            // Zero-alloc path for single chars, String only for zero-width combiners
            if cell.zerowidth.is_empty() {
                let s = cell.c.encode_utf8(&mut char_buf);
                buffer.set_text(font_system, s, &attrs, Shaping::Advanced, None);
            } else {
                let mut text = String::with_capacity(4 + cell.zerowidth.len() * 4);
                text.push(cell.c);
                for &zw in &cell.zerowidth {
                    text.push(zw);
                }
                buffer.set_text(font_system, &text, &attrs, Shaping::Advanced, None);
            }

            buffer.shape_until_scroll(font_system, false);
            buffers.push(buffer);

            // Track grid position for this buffer
            let col = cell.point.column.0;
            let line = cell.point.line.0 + line_offset;
            positions.push((col, line));
        }
    }

    /// Create TextAreas from per-cell Buffers positioned at exact grid coordinates.
    ///
    /// Each TextArea is placed at `(x_offset + col * cell_width, y_offset + line * cell_height)`
    /// using the positions from `build_cell_buffers`. This eliminates horizontal drift
    /// since each cell is independently grid-locked.
    ///
    /// Uses `scale: 1.0` (never TextArea.scale for DPI -- see glyphon issue #117).
    pub fn build_cell_text_areas_offset<'a>(
        &self,
        buffers: &'a [Buffer],
        positions: &[(usize, i32)],
        viewport_width: u32,
        viewport_height: u32,
        x_offset: f32,
        y_offset: f32,
    ) -> Vec<TextArea<'a>> {
        let bounds = TextBounds {
            left: x_offset as i32,
            top: y_offset as i32,
            right: (x_offset as u32 + viewport_width) as i32,
            bottom: (y_offset as u32 + viewport_height) as i32,
        };

        buffers
            .iter()
            .zip(positions.iter())
            .map(|(buffer, &(col, line))| TextArea {
                buffer,
                left: x_offset + col as f32 * self.cell_width,
                top: y_offset + line as f32 * self.cell_height,
                scale: 1.0,
                bounds,
                default_color: GlyphonColor::rgba(204, 204, 204, 255),
                custom_glyphs: &[],
            })
            .collect()
    }
}

/// Convert an RGB color to a normalized [r, g, b, a] array.
fn rgb_to_color(rgb: Rgb, alpha: f32) -> [f32; 4] {
    [
        rgb.r as f32 / 255.0,
        rgb.g as f32 / 255.0,
        rgb.b as f32 / 255.0,
        alpha,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyphon::FontSystem;

    /// Test 1: cell_height is derived from font metrics, NOT from (font_size * scale * 1.2).ceil()
    #[test]
    fn cell_height_from_font_metrics_not_hardcoded() {
        let mut font_system = FontSystem::new();
        let renderer = GridRenderer::new(&mut font_system, "monospace", 14.0, 1.0);

        let hardcoded_height = (14.0_f32 * 1.0 * 1.2).ceil();
        // The font-metric derived height should differ from the old 1.2x multiplier
        assert_ne!(
            renderer.cell_height, hardcoded_height,
            "cell_height should be derived from font metrics, not hardcoded 1.2x multiplier"
        );
    }

    /// Test 2: cell_height is at least physical_font_size (safety floor)
    #[test]
    fn cell_height_at_least_physical_font_size() {
        let mut font_system = FontSystem::new();
        let renderer = GridRenderer::new(&mut font_system, "monospace", 14.0, 2.0);

        let physical_font_size = 14.0 * 2.0;
        assert!(
            renderer.cell_height >= physical_font_size,
            "cell_height ({}) should be >= physical_font_size ({})",
            renderer.cell_height,
            physical_font_size
        );
    }

    /// Test 3: cell_width matches "M" glyph advance width (existing behavior preserved)
    #[test]
    fn cell_width_matches_m_glyph_advance() {
        let mut font_system = FontSystem::new();
        let renderer = GridRenderer::new(&mut font_system, "monospace", 14.0, 1.0);

        // cell_width should be positive and reasonable (not a fallback of 0.6*font_size exactly
        // unless no font is found -- on CI with system fonts it should be the actual M width)
        assert!(renderer.cell_width > 0.0, "cell_width should be positive");
        // Should be roughly in the range of 0.5x to 1.0x font_size for a monospace font
        let physical = 14.0_f32;
        assert!(
            renderer.cell_width < physical * 1.5,
            "cell_width ({}) should be less than 1.5x physical font size ({})",
            renderer.cell_width,
            physical * 1.5
        );
    }

    /// Test 4: build_cell_buffers creates correct number of buffers (skips spaces and spacers)
    #[test]
    fn build_cell_buffers_skips_spaces_and_spacers() {
        use alacritty_terminal::index::{Column, Line, Point};

        let mut font_system = FontSystem::new();
        let renderer = GridRenderer::new(&mut font_system, "monospace", 14.0, 1.0);

        // Create a minimal snapshot with 3 cells: 'A', ' ' (space), 'B'
        let cells = vec![
            glass_terminal::RenderedCell {
                point: Point {
                    line: Line(0),
                    column: Column(0),
                },
                c: 'A',
                fg: Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                },
                bg: Rgb { r: 0, g: 0, b: 0 },
                flags: Flags::empty(),
                zerowidth: vec![],
            },
            glass_terminal::RenderedCell {
                point: Point {
                    line: Line(0),
                    column: Column(1),
                },
                c: ' ',
                fg: Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                },
                bg: Rgb { r: 0, g: 0, b: 0 },
                flags: Flags::empty(),
                zerowidth: vec![],
            },
            glass_terminal::RenderedCell {
                point: Point {
                    line: Line(0),
                    column: Column(2),
                },
                c: 'B',
                fg: Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                },
                bg: Rgb { r: 0, g: 0, b: 0 },
                flags: Flags::empty(),
                zerowidth: vec![],
            },
            // WIDE_CHAR_SPACER cell -- should be skipped
            glass_terminal::RenderedCell {
                point: Point {
                    line: Line(0),
                    column: Column(3),
                },
                c: ' ',
                fg: Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                },
                bg: Rgb { r: 0, g: 0, b: 0 },
                flags: Flags::WIDE_CHAR_SPACER,
                zerowidth: vec![],
            },
            // LEADING_WIDE_CHAR_SPACER cell -- should also be skipped
            glass_terminal::RenderedCell {
                point: Point {
                    line: Line(0),
                    column: Column(4),
                },
                c: ' ',
                fg: Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                },
                bg: Rgb { r: 0, g: 0, b: 0 },
                flags: Flags::LEADING_WIDE_CHAR_SPACER,
                zerowidth: vec![],
            },
        ];

        let snapshot = GridSnapshot {
            cells,
            cursor: alacritty_terminal::term::RenderableCursor {
                point: Point {
                    line: Line(0),
                    column: Column(0),
                },
                shape: CursorShape::Block,
            },
            display_offset: 0,
            history_size: 0,
            mode: alacritty_terminal::term::TermMode::empty(),
            columns: 5,
            screen_lines: 1,
            selection: None,
        };

        let mut buffers = Vec::new();
        let mut positions = Vec::new();
        renderer.build_cell_buffers(&mut font_system, &snapshot, &mut buffers, &mut positions);

        // Should have 2 buffers: 'A' and 'B' (space skipped, spacer skipped)
        assert_eq!(
            buffers.len(),
            2,
            "Should create 2 buffers (skip space + spacer)"
        );
        assert_eq!(
            positions.len(),
            2,
            "Should have 2 positions matching buffers"
        );

        // Verify positions
        assert_eq!(positions[0], (0, 0), "First buffer at column 0, line 0");
        assert_eq!(positions[1], (2, 0), "Second buffer at column 2, line 0");
    }

    /// Test 5: build_cell_text_areas_offset positions cells at exact grid coordinates
    #[test]
    fn cell_text_areas_at_exact_grid_coordinates() {
        let mut font_system = FontSystem::new();
        let renderer = GridRenderer::new(&mut font_system, "monospace", 14.0, 1.0);

        let positions = vec![(0_usize, 0_i32), (5, 0), (0, 1), (3, 2)];

        // Create minimal buffers (we just need them for the TextArea reference)
        let physical_font_size = 14.0_f32;
        let metrics = Metrics::new(physical_font_size, renderer.cell_height);
        let mut buffers = Vec::new();
        for _ in &positions {
            let mut buf = Buffer::new(&mut font_system, metrics);
            buf.set_size(
                &mut font_system,
                Some(renderer.cell_width),
                Some(renderer.cell_height),
            );
            buf.set_text(
                &mut font_system,
                "X",
                &Attrs::new(),
                Shaping::Advanced,
                None,
            );
            buf.shape_until_scroll(&mut font_system, false);
            buffers.push(buf);
        }

        let areas =
            renderer.build_cell_text_areas_offset(&buffers, &positions, 800, 600, 10.0, 20.0);

        assert_eq!(areas.len(), 4);

        // Check exact grid positioning
        let cw = renderer.cell_width;
        let ch = renderer.cell_height;

        // (col=0, line=0) -> left=10.0, top=20.0
        assert!((areas[0].left - 10.0).abs() < 0.001);
        assert!((areas[0].top - 20.0).abs() < 0.001);

        // (col=5, line=0) -> left=10.0 + 5*cw, top=20.0
        assert!((areas[1].left - (10.0 + 5.0 * cw)).abs() < 0.001);
        assert!((areas[1].top - 20.0).abs() < 0.001);

        // (col=0, line=1) -> left=10.0, top=20.0 + 1*ch
        assert!((areas[2].left - 10.0).abs() < 0.001);
        assert!((areas[2].top - (20.0 + ch)).abs() < 0.001);

        // (col=3, line=2) -> left=10.0 + 3*cw, top=20.0 + 2*ch
        assert!((areas[3].left - (10.0 + 3.0 * cw)).abs() < 0.001);
        assert!((areas[3].top - (20.0 + 2.0 * ch)).abs() < 0.001);

        // All should have scale=1.0
        for area in &areas {
            assert_eq!(
                area.scale, 1.0,
                "TextArea.scale must be 1.0 (never use for DPI)"
            );
        }
    }

    /// Helper to create a RenderedCell for tests
    fn make_cell(c: char, col: usize, line: i32, flags: Flags) -> glass_terminal::RenderedCell {
        use alacritty_terminal::index::{Column, Line, Point};
        glass_terminal::RenderedCell {
            point: Point {
                line: Line(line),
                column: Column(col),
            },
            c,
            fg: Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
            bg: Rgb { r: 0, g: 0, b: 0 },
            flags,
            zerowidth: vec![],
        }
    }

    /// Helper to create a GridSnapshot from cells
    fn make_snapshot(cells: Vec<glass_terminal::RenderedCell>, columns: usize) -> GridSnapshot {
        use alacritty_terminal::index::{Column, Line, Point};
        GridSnapshot {
            cells,
            cursor: alacritty_terminal::term::RenderableCursor {
                point: Point {
                    line: Line(0),
                    column: Column(0),
                },
                shape: CursorShape::Block,
            },
            display_offset: 0,
            history_size: 0,
            mode: alacritty_terminal::term::TermMode::empty(),
            columns,
            screen_lines: 1,
            selection: None,
        }
    }

    /// Helper to create a GridSnapshot with custom cursor
    fn make_snapshot_with_cursor(
        cells: Vec<glass_terminal::RenderedCell>,
        columns: usize,
        cursor_col: usize,
        cursor_shape: CursorShape,
    ) -> GridSnapshot {
        use alacritty_terminal::index::{Column, Line, Point};
        GridSnapshot {
            cells,
            cursor: alacritty_terminal::term::RenderableCursor {
                point: Point {
                    line: Line(0),
                    column: Column(cursor_col),
                },
                shape: cursor_shape,
            },
            display_offset: 0,
            history_size: 0,
            mode: alacritty_terminal::term::TermMode::empty(),
            columns,
            screen_lines: 1,
            selection: None,
        }
    }

    /// Helper to create a cell with custom background color
    fn make_cell_with_bg(
        c: char,
        col: usize,
        line: i32,
        flags: Flags,
        bg: Rgb,
    ) -> glass_terminal::RenderedCell {
        use alacritty_terminal::index::{Column, Line, Point};
        glass_terminal::RenderedCell {
            point: Point {
                line: Line(line),
                column: Column(col),
            },
            c,
            fg: Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
            bg,
            flags,
            zerowidth: vec![],
        }
    }

    /// Test: wide char cell produces a Buffer with double width, spacer is skipped
    #[test]
    fn wide_char_buffer_double_width() {
        let mut font_system = FontSystem::new();
        let renderer = GridRenderer::new(&mut font_system, "monospace", 14.0, 1.0);

        // 'A' at col 0 (normal), CJK at col 1 (WIDE_CHAR), spacer at col 2, 'B' at col 3
        let cells = vec![
            make_cell('A', 0, 0, Flags::empty()),
            make_cell('\u{4e16}', 1, 0, Flags::WIDE_CHAR), // CJK '世'
            make_cell(' ', 2, 0, Flags::WIDE_CHAR_SPACER),
            make_cell('B', 3, 0, Flags::empty()),
        ];
        let snapshot = make_snapshot(cells, 4);

        let mut buffers = Vec::new();
        let mut positions = Vec::new();
        renderer.build_cell_buffers(&mut font_system, &snapshot, &mut buffers, &mut positions);

        // Should have 3 buffers: A, CJK, B (spacer skipped)
        assert_eq!(buffers.len(), 3, "Should create 3 buffers (spacer skipped)");
        assert_eq!(positions.len(), 3, "Should have 3 positions");
        assert_eq!(positions[0], (0, 0), "A at column 0");
        assert_eq!(positions[1], (1, 0), "CJK at column 1");
        assert_eq!(positions[2], (3, 0), "B at column 3");
    }

    /// Test: LEADING_WIDE_CHAR_SPACER cells are skipped (no buffer created)
    #[test]
    fn leading_wide_char_spacer_skipped() {
        let mut font_system = FontSystem::new();
        let renderer = GridRenderer::new(&mut font_system, "monospace", 14.0, 1.0);

        // A cell with LEADING_WIDE_CHAR_SPACER (appears at end of line when wide char wraps)
        let cells = vec![
            make_cell('A', 0, 0, Flags::empty()),
            make_cell(' ', 1, 0, Flags::LEADING_WIDE_CHAR_SPACER),
        ];
        let snapshot = make_snapshot(cells, 2);

        let mut buffers = Vec::new();
        let mut positions = Vec::new();
        renderer.build_cell_buffers(&mut font_system, &snapshot, &mut buffers, &mut positions);

        // Only 'A' should produce a buffer; LEADING spacer should be skipped
        assert_eq!(
            buffers.len(),
            1,
            "LEADING_WIDE_CHAR_SPACER should be skipped"
        );
        assert_eq!(positions[0], (0, 0), "Only A at column 0");
    }

    /// Test: wide char at column 2 produces position (2, line), spacer at column 3 produces nothing
    #[test]
    fn wide_char_buffer_position_correct() {
        let mut font_system = FontSystem::new();
        let renderer = GridRenderer::new(&mut font_system, "monospace", 14.0, 1.0);

        let cells = vec![
            make_cell('X', 0, 0, Flags::empty()),
            make_cell('Y', 1, 0, Flags::empty()),
            make_cell('\u{4e16}', 2, 0, Flags::WIDE_CHAR),
            make_cell(' ', 3, 0, Flags::WIDE_CHAR_SPACER),
            make_cell('Z', 4, 0, Flags::empty()),
        ];
        let snapshot = make_snapshot(cells, 5);

        let mut buffers = Vec::new();
        let mut positions = Vec::new();
        renderer.build_cell_buffers(&mut font_system, &snapshot, &mut buffers, &mut positions);

        assert_eq!(buffers.len(), 4, "X, Y, CJK, Z = 4 buffers");
        assert_eq!(positions[0], (0, 0), "X at col 0");
        assert_eq!(positions[1], (1, 0), "Y at col 1");
        assert_eq!(positions[2], (2, 0), "CJK wide char at col 2, not col 3");
        assert_eq!(positions[3], (4, 0), "Z at col 4");
    }

    /// Test: WIDE_CHAR cell background rect is double-width, spacer produces no rect
    #[test]
    fn wide_char_bg_rect_double_width() {
        let mut font_system = FontSystem::new();
        let renderer = GridRenderer::new(&mut font_system, "monospace", 14.0, 1.0);
        let default_bg = Rgb { r: 0, g: 0, b: 0 };
        let red = Rgb { r: 255, g: 0, b: 0 };
        let blue = Rgb { r: 0, g: 0, b: 255 };

        // col 0: normal cell with red bg, col 1: WIDE_CHAR with blue bg,
        // col 2: WIDE_CHAR_SPACER with blue bg, col 3: normal cell with default bg
        let cells = vec![
            make_cell_with_bg('A', 0, 0, Flags::empty(), red),
            make_cell_with_bg('\u{4e16}', 1, 0, Flags::WIDE_CHAR, blue),
            make_cell_with_bg(' ', 2, 0, Flags::WIDE_CHAR_SPACER, blue),
            make_cell_with_bg('B', 3, 0, Flags::empty(), default_bg),
        ];

        let snapshot = make_snapshot(cells, 4);
        let rects = renderer.build_rects(&snapshot, default_bg);

        // Filter out cursor rects -- cursor is at col 0 by default, so last rect(s) are cursor
        // Background rects come first in the vec
        let cw = renderer.cell_width;

        // Find bg rect at col 1 (wide char)
        let wide_bg = rects
            .iter()
            .find(|r| (r.pos[0] - 1.0 * cw).abs() < 0.001)
            .expect("Should have bg rect at col 1");
        assert!(
            (wide_bg.pos[2] - 2.0 * cw).abs() < 0.001,
            "Wide char bg rect width ({}) should be 2*cell_width ({})",
            wide_bg.pos[2],
            2.0 * cw
        );

        // No rect should exist at x = 2*cell_width (spacer should be skipped)
        let spacer_rect = rects
            .iter()
            .find(|r| (r.pos[0] - 2.0 * cw).abs() < 0.001 && r.color != [0.8, 0.8, 0.8, 0.7]);
        assert!(
            spacer_rect.is_none(),
            "Spacer cell should not produce a background rect"
        );

        // Total bg rects should be 2 (col 0 red + col 1 blue)
        let bg_rects: Vec<_> = rects
            .iter()
            .filter(|r| r.color != [0.8, 0.8, 0.8, 0.7]) // exclude cursor rects
            .collect();
        assert_eq!(bg_rects.len(), 2, "Should have exactly 2 bg rects");
    }

    /// Test: Block cursor on WIDE_CHAR cell has double-width
    #[test]
    fn wide_char_cursor_block_double_width() {
        let mut font_system = FontSystem::new();
        let renderer = GridRenderer::new(&mut font_system, "monospace", 14.0, 1.0);
        let default_bg = Rgb { r: 0, g: 0, b: 0 };

        let cells = vec![
            make_cell('A', 0, 0, Flags::empty()),
            make_cell('\u{4e16}', 1, 0, Flags::WIDE_CHAR),
            make_cell(' ', 2, 0, Flags::WIDE_CHAR_SPACER),
            make_cell('B', 3, 0, Flags::empty()),
        ];

        // Cursor at col 1 (wide char), Block shape
        let snapshot = make_snapshot_with_cursor(cells, 4, 1, CursorShape::Block);
        let rects = renderer.build_rects(&snapshot, default_bg);
        let cw = renderer.cell_width;

        // Find the cursor rect (Block at col 1)
        let cursor_rect = rects
            .iter()
            .find(|r| (r.pos[0] - 1.0 * cw).abs() < 0.001 && r.color == [0.8, 0.8, 0.8, 0.7])
            .expect("Should have cursor rect at col 1");

        assert!(
            (cursor_rect.pos[2] - 2.0 * cw).abs() < 0.001,
            "Block cursor width ({}) should be 2*cell_width ({})",
            cursor_rect.pos[2],
            2.0 * cw
        );
    }

    /// Test: Underline cursor on WIDE_CHAR cell has double-width
    #[test]
    fn wide_char_cursor_underline_double_width() {
        let mut font_system = FontSystem::new();
        let renderer = GridRenderer::new(&mut font_system, "monospace", 14.0, 1.0);
        let default_bg = Rgb { r: 0, g: 0, b: 0 };

        let cells = vec![
            make_cell('A', 0, 0, Flags::empty()),
            make_cell('\u{4e16}', 1, 0, Flags::WIDE_CHAR),
            make_cell(' ', 2, 0, Flags::WIDE_CHAR_SPACER),
        ];

        let snapshot = make_snapshot_with_cursor(cells, 3, 1, CursorShape::Underline);
        let rects = renderer.build_rects(&snapshot, default_bg);
        let cw = renderer.cell_width;

        // Find cursor rect (underline at col 1)
        let cursor_rect = rects
            .iter()
            .find(|r| {
                (r.pos[0] - 1.0 * cw).abs() < 0.001
                    && r.color == [0.8, 0.8, 0.8, 0.7]
                    && (r.pos[3] - 2.0).abs() < 0.001 // underline height = 2.0
            })
            .expect("Should have underline cursor rect at col 1");

        assert!(
            (cursor_rect.pos[2] - 2.0 * cw).abs() < 0.001,
            "Underline cursor width ({}) should be 2*cell_width ({})",
            cursor_rect.pos[2],
            2.0 * cw
        );
    }

    /// Test: HollowBlock cursor on WIDE_CHAR cell has double-width edges
    #[test]
    fn wide_char_cursor_hollow_block_double_width() {
        let mut font_system = FontSystem::new();
        let renderer = GridRenderer::new(&mut font_system, "monospace", 14.0, 1.0);
        let default_bg = Rgb { r: 0, g: 0, b: 0 };

        let cells = vec![
            make_cell('A', 0, 0, Flags::empty()),
            make_cell('\u{4e16}', 1, 0, Flags::WIDE_CHAR),
            make_cell(' ', 2, 0, Flags::WIDE_CHAR_SPACER),
        ];

        let snapshot = make_snapshot_with_cursor(cells, 3, 1, CursorShape::HollowBlock);
        let rects = renderer.build_rects(&snapshot, default_bg);
        let cw = renderer.cell_width;
        let ch = renderer.cell_height;
        let cursor_x = 1.0 * cw;

        // Filter cursor rects (color matches cursor_color)
        let cursor_rects: Vec<_> = rects
            .iter()
            .filter(|r| r.color == [0.8, 0.8, 0.8, 0.7])
            .collect();

        // HollowBlock has 4 edges
        assert_eq!(
            cursor_rects.len(),
            4,
            "HollowBlock should produce 4 edge rects"
        );

        // Top edge: width should be 2*cell_width
        let top = cursor_rects
            .iter()
            .find(|r| (r.pos[1] - 0.0).abs() < 0.001 && (r.pos[3] - 1.0).abs() < 0.001)
            .expect("Should have top edge rect");
        assert!(
            (top.pos[2] - 2.0 * cw).abs() < 0.001,
            "Top edge width ({}) should be 2*cell_width ({})",
            top.pos[2],
            2.0 * cw
        );

        // Bottom edge: width should be 2*cell_width
        let bottom = cursor_rects
            .iter()
            .find(|r| (r.pos[1] - (ch - 1.0)).abs() < 0.001 && (r.pos[3] - 1.0).abs() < 0.001)
            .expect("Should have bottom edge rect");
        assert!(
            (bottom.pos[2] - 2.0 * cw).abs() < 0.001,
            "Bottom edge width ({}) should be 2*cell_width ({})",
            bottom.pos[2],
            2.0 * cw
        );

        // Right edge: x should be cursor_x + 2*cell_width - 1.0
        let right = cursor_rects
            .iter()
            .find(|r| {
                (r.pos[0] - (cursor_x + 2.0 * cw - 1.0)).abs() < 0.001
                    && (r.pos[3] - ch).abs() < 0.001
            })
            .expect("Should have right edge rect");
        assert!(
            (right.pos[0] - (cursor_x + 2.0 * cw - 1.0)).abs() < 0.001,
            "Right edge x ({}) should be cursor_x + 2*cell_width - 1.0 ({})",
            right.pos[0],
            cursor_x + 2.0 * cw - 1.0
        );
    }
}

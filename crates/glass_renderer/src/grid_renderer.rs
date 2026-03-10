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
    /// Uses cosmic-text to shape a reference character ("M") and measure its advance
    /// width, establishing the monospace cell grid dimensions.
    pub fn new(
        font_system: &mut FontSystem,
        font_family: &str,
        font_size: f32,
        scale_factor: f32,
    ) -> Self {
        let physical_font_size = font_size * scale_factor;
        // Line height = 1.2x font size (standard terminal line spacing)
        let line_height = (physical_font_size * 1.2).ceil();
        let metrics = Metrics::new(physical_font_size, line_height);

        // Measure cell width by shaping "M" and reading glyph advance
        let mut measure_buf = Buffer::new(font_system, metrics);
        measure_buf.set_size(font_system, Some(1000.0), Some(line_height));
        measure_buf.set_text(
            font_system,
            "M",
            &Attrs::new().family(Family::Name(font_family)),
            Shaping::Advanced,
            None,
        );
        measure_buf.shape_until_scroll(font_system, false);

        let cell_width = measure_buf
            .layout_runs()
            .next()
            .and_then(|run| run.glyphs.first().map(|g| g.w))
            .unwrap_or(physical_font_size * 0.6);

        GridRenderer {
            cell_width,
            cell_height: line_height,
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
            if cell.bg != default_bg {
                let x = cell.point.column.0 as f32 * self.cell_width;
                let y = (cell.point.line.0 + line_offset) as f32 * self.cell_height;
                rects.push(RectInstance {
                    pos: [x, y, self.cell_width, self.cell_height],
                    color: rgb_to_color(cell.bg, 1.0),
                });
            }
        }

        // Cursor rect
        let cursor = &snapshot.cursor;
        let cursor_x = cursor.point.column.0 as f32 * self.cell_width;
        let cursor_y = (cursor.point.line.0 + line_offset) as f32 * self.cell_height;
        let cursor_color = [0.8, 0.8, 0.8, 0.7]; // semi-transparent light gray

        match cursor.shape {
            CursorShape::Block => {
                rects.push(RectInstance {
                    pos: [cursor_x, cursor_y, self.cell_width, self.cell_height],
                    color: cursor_color,
                });
            }
            CursorShape::Beam => {
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
                        self.cell_width,
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
                    pos: [cursor_x, cursor_y, self.cell_width, t],
                    color: cursor_color,
                });
                // Bottom
                rects.push(RectInstance {
                    pos: [
                        cursor_x,
                        cursor_y + self.cell_height - t,
                        self.cell_width,
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
                        cursor_x + self.cell_width - t,
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

    /// Build glyphon text Buffers for each terminal line.
    ///
    /// The caller must pass a `&mut Vec<Buffer>` that will own the Buffers.
    /// Returns a vector of TextAreas that borrow from those Buffers.
    /// The Buffers Vec must outlive the returned TextAreas.
    #[cfg_attr(feature = "perf", tracing::instrument(skip_all))]
    pub fn build_text_buffers(
        &self,
        font_system: &mut FontSystem,
        snapshot: &GridSnapshot,
        buffers: &mut Vec<Buffer>,
    ) {
        let physical_font_size = self.font_size * self.scale_factor;
        let metrics = Metrics::new(physical_font_size, self.cell_height);
        let viewport_width = snapshot.columns as f32 * self.cell_width;

        buffers.reserve(buffers.len() + snapshot.screen_lines);
        let line_offset = snapshot.display_offset as i32;

        for line_idx in 0..snapshot.screen_lines {
            // Collect cells for this line, skip WIDE_CHAR_SPACER
            // display_iter yields line values starting at -(display_offset),
            // so add line_offset to convert to viewport-relative index
            let line_cells: Vec<_> = snapshot
                .cells
                .iter()
                .filter(|cell| {
                    (cell.point.line.0 + line_offset) as usize == line_idx
                        && !cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                })
                .collect();

            let mut buffer = Buffer::new(font_system, metrics);
            buffer.set_size(font_system, Some(viewport_width), Some(self.cell_height));

            if line_cells.is_empty() {
                // Empty line — set space as placeholder
                buffer.set_text(
                    font_system,
                    " ",
                    &Attrs::new().family(Family::Name(&self.font_family)),
                    Shaping::Advanced,
                    None,
                );
            } else {
                // Build combined string and per-span attributes
                let mut text = String::with_capacity(line_cells.len());
                let mut span_ranges: Vec<(usize, usize, u8, u8, u8, bool, bool)> =
                    Vec::with_capacity(line_cells.len());

                for cell in &line_cells {
                    let start = text.len();
                    text.push(cell.c);
                    for &zw in &cell.zerowidth {
                        text.push(zw);
                    }
                    let end = text.len();
                    span_ranges.push((
                        start,
                        end,
                        cell.fg.r,
                        cell.fg.g,
                        cell.fg.b,
                        cell.flags.contains(Flags::BOLD),
                        cell.flags.contains(Flags::ITALIC),
                    ));
                }

                // Build rich text spans from the collected data
                let rich_spans: Vec<(&str, Attrs<'_>)> = span_ranges
                    .iter()
                    .map(|&(start, end, r, g, b, bold, italic)| {
                        let mut attrs = Attrs::new()
                            .family(Family::Name(&self.font_family))
                            .color(GlyphonColor::rgba(r, g, b, 255));
                        if bold {
                            attrs = attrs.weight(Weight::BOLD);
                        }
                        if italic {
                            attrs = attrs.style(Style::Italic);
                        }
                        (&text[start..end], attrs)
                    })
                    .collect();

                buffer.set_rich_text(
                    font_system,
                    rich_spans,
                    &Attrs::new().family(Family::Name(&self.font_family)),
                    Shaping::Advanced,
                    None,
                );
            }

            buffer.shape_until_scroll(font_system, false);
            buffers.push(buffer);
        }
    }

    /// Create TextAreas from pre-built Buffers.
    ///
    /// Call this after `build_text_buffers` with the same Buffers vec.
    pub fn build_text_areas<'a>(
        &self,
        buffers: &'a [Buffer],
        viewport_width: u32,
        viewport_height: u32,
    ) -> Vec<TextArea<'a>> {
        self.build_text_areas_offset(buffers, viewport_width, viewport_height, 0.0, 0.0)
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

    /// Create TextAreas from pre-built Buffers with a pixel offset.
    ///
    /// Used for split pane rendering where text must be positioned within
    /// a viewport sub-region. TextBounds are set to the viewport rect.
    pub fn build_text_areas_offset<'a>(
        &self,
        buffers: &'a [Buffer],
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
            .enumerate()
            .map(|(line_idx, buffer)| TextArea {
                buffer,
                left: x_offset,
                top: y_offset + line_idx as f32 * self.cell_height,
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
        assert!(
            renderer.cell_width > 0.0,
            "cell_width should be positive"
        );
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
                point: Point { line: Line(0), column: Column(0) },
                c: 'A',
                fg: Rgb { r: 255, g: 255, b: 255 },
                bg: Rgb { r: 0, g: 0, b: 0 },
                flags: Flags::empty(),
                zerowidth: vec![],
            },
            glass_terminal::RenderedCell {
                point: Point { line: Line(0), column: Column(1) },
                c: ' ',
                fg: Rgb { r: 255, g: 255, b: 255 },
                bg: Rgb { r: 0, g: 0, b: 0 },
                flags: Flags::empty(),
                zerowidth: vec![],
            },
            glass_terminal::RenderedCell {
                point: Point { line: Line(0), column: Column(2) },
                c: 'B',
                fg: Rgb { r: 255, g: 255, b: 255 },
                bg: Rgb { r: 0, g: 0, b: 0 },
                flags: Flags::empty(),
                zerowidth: vec![],
            },
            // WIDE_CHAR_SPACER cell -- should be skipped
            glass_terminal::RenderedCell {
                point: Point { line: Line(0), column: Column(3) },
                c: ' ',
                fg: Rgb { r: 255, g: 255, b: 255 },
                bg: Rgb { r: 0, g: 0, b: 0 },
                flags: Flags::WIDE_CHAR_SPACER,
                zerowidth: vec![],
            },
        ];

        let snapshot = GridSnapshot {
            cells,
            cursor: alacritty_terminal::term::RenderableCursor {
                point: Point { line: Line(0), column: Column(0) },
                shape: CursorShape::Block,
            },
            display_offset: 0,
            history_size: 0,
            mode: alacritty_terminal::term::TermMode::empty(),
            columns: 4,
            screen_lines: 1,
            selection: None,
        };

        let mut buffers = Vec::new();
        let mut positions = Vec::new();
        renderer.build_cell_buffers(&mut font_system, &snapshot, &mut buffers, &mut positions);

        // Should have 2 buffers: 'A' and 'B' (space skipped, spacer skipped)
        assert_eq!(buffers.len(), 2, "Should create 2 buffers (skip space + spacer)");
        assert_eq!(positions.len(), 2, "Should have 2 positions matching buffers");

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
            buf.set_size(&mut font_system, Some(renderer.cell_width), Some(renderer.cell_height));
            buf.set_text(&mut font_system, "X", &Attrs::new(), Shaping::Advanced, None);
            buf.shape_until_scroll(&mut font_system, false);
            buffers.push(buf);
        }

        let areas = renderer.build_cell_text_areas_offset(
            &buffers, &positions, 800, 600, 10.0, 20.0,
        );

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
            assert_eq!(area.scale, 1.0, "TextArea.scale must be 1.0 (never use for DPI)");
        }
    }
}

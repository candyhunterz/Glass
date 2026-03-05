//! BlockRenderer: generates visual decorations for command blocks.
//!
//! Produces colored rectangles for separator lines and exit code badges,
//! plus text labels for badge symbols and duration display.

use alacritty_terminal::vte::ansi::Rgb;

use glass_terminal::Block;

use crate::rect_renderer::RectInstance;

/// A text label to be rendered on a block separator.
#[derive(Debug, Clone)]
pub struct BlockLabel {
    /// X position in pixels
    pub x: f32,
    /// Y position in pixels
    pub y: f32,
    /// Text content (badge symbol or duration string)
    pub text: String,
    /// Text color
    pub color: Rgb,
}

/// Renders block decorations (separators, exit code badges, duration labels).
///
/// Stateless helper that converts Block data into RectInstances and BlockLabels
/// for the GPU rendering pipeline.
pub struct BlockRenderer {
    cell_width: f32,
    cell_height: f32,
}

impl BlockRenderer {
    /// Create a new BlockRenderer with the given cell dimensions.
    pub fn new(cell_width: f32, cell_height: f32) -> Self {
        Self {
            cell_width,
            cell_height,
        }
    }

    /// Build colored rectangles for block separators and exit code badges.
    ///
    /// For each visible block, generates:
    /// - A horizontal separator line (1px tall, full width) at the block's prompt_start_line
    /// - If block has exit_code: a badge rect (3 cells wide, 1 cell tall) at the right edge
    pub fn build_block_rects(
        &self,
        blocks: &[&Block],
        display_offset: usize,
        screen_lines: usize,
        viewport_width: f32,
    ) -> Vec<RectInstance> {
        let mut rects = Vec::with_capacity(blocks.len() * 2);

        for block in blocks {
            let line = block.prompt_start_line;

            // Skip if separator would be off-screen
            if line < display_offset || line >= display_offset + screen_lines {
                continue;
            }

            let y = (line - display_offset) as f32 * self.cell_height;

            // Horizontal separator line (1px tall, full width, subtle gray)
            rects.push(RectInstance {
                pos: [0.0, y, viewport_width, 1.0],
                color: [60.0 / 255.0, 60.0 / 255.0, 60.0 / 255.0, 1.0],
            });

            // Exit code badge (if available)
            if let Some(exit_code) = block.exit_code {
                let badge_width = self.cell_width * 3.0;
                let badge_x = viewport_width - badge_width;
                let badge_color = if exit_code == 0 {
                    // Green for success
                    [40.0 / 255.0, 160.0 / 255.0, 40.0 / 255.0, 1.0]
                } else {
                    // Red for failure
                    [200.0 / 255.0, 50.0 / 255.0, 50.0 / 255.0, 1.0]
                };
                rects.push(RectInstance {
                    pos: [badge_x, y, badge_width, self.cell_height],
                    color: badge_color,
                });
            }
        }

        rects
    }

    /// Build text labels for exit code badges and duration display.
    ///
    /// For each complete block:
    /// - Badge text: checkmark for exit 0, X for non-zero
    /// - Duration text: right-aligned, subtle gray
    pub fn build_block_text(
        &self,
        blocks: &[&Block],
        display_offset: usize,
        screen_lines: usize,
        viewport_width: f32,
    ) -> Vec<BlockLabel> {
        let mut labels = Vec::new();

        for block in blocks {
            let line = block.prompt_start_line;

            // Skip if off-screen
            if line < display_offset || line >= display_offset + screen_lines {
                continue;
            }

            let y = (line - display_offset) as f32 * self.cell_height;

            // Exit code badge text
            if let Some(exit_code) = block.exit_code {
                let badge_width = self.cell_width * 3.0;
                let badge_x = viewport_width - badge_width + self.cell_width;
                let (text, color) = if exit_code == 0 {
                    ("OK".to_string(), Rgb { r: 255, g: 255, b: 255 })
                } else {
                    ("X".to_string(), Rgb { r: 255, g: 255, b: 255 })
                };
                labels.push(BlockLabel {
                    x: badge_x,
                    y,
                    text,
                    color,
                });
            }

            // Duration text (right-aligned, next to badge area)
            if let Some(duration) = block.duration() {
                let duration_text = glass_terminal::format_duration(duration);
                let duration_width = duration_text.len() as f32 * self.cell_width;
                let badge_width = self.cell_width * 3.0;
                let x = viewport_width - badge_width - duration_width - self.cell_width;
                labels.push(BlockLabel {
                    x,
                    y,
                    text: duration_text,
                    color: Rgb {
                        r: 140,
                        g: 140,
                        b: 140,
                    },
                });
            }
        }

        labels
    }
}

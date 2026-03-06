//! BlockRenderer: generates visual decorations for command blocks.
//!
//! Produces colored rectangles for separator lines and exit code badges,
//! plus text labels for badge symbols and duration display.

use alacritty_terminal::vte::ansi::Rgb;

use glass_pipes::FinalizedBuffer;
use glass_terminal::{Block, BlockState};

use crate::rect_renderer::RectInstance;

/// Count lines in a FinalizedBuffer.
fn line_count(data: &FinalizedBuffer) -> usize {
    match data {
        FinalizedBuffer::Complete(bytes) => {
            let count = bytes.iter().filter(|&&b| b == b'\n').count();
            count.max(if bytes.is_empty() { 0 } else { 1 })
        }
        FinalizedBuffer::Sampled { head, tail, .. } => {
            let head_lines = head.iter().filter(|&&b| b == b'\n').count();
            let tail_lines = tail.iter().filter(|&&b| b == b'\n').count();
            head_lines + tail_lines
        }
        FinalizedBuffer::Binary { .. } => 0,
    }
}

/// Format a byte count as a human-readable string.
fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

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
            let mut duration_x = None;
            if let Some(duration) = block.duration() {
                let duration_text = glass_terminal::format_duration(duration);
                let duration_width = duration_text.len() as f32 * self.cell_width;
                let badge_width = self.cell_width * 3.0;
                let x = viewport_width - badge_width - duration_width - self.cell_width;
                duration_x = Some(x);
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

            // [undo] label for blocks with snapshots (UI-01)
            if block.has_snapshot && block.state == BlockState::Complete {
                let undo_text = "[undo]";
                let undo_width = undo_text.len() as f32 * self.cell_width;
                let badge_width = self.cell_width * 3.0;
                let undo_x = if let Some(dx) = duration_x {
                    // Position to the left of the duration text
                    dx - undo_width - self.cell_width
                } else {
                    // No duration text, position relative to badge
                    viewport_width - badge_width - undo_width - self.cell_width
                };
                labels.push(BlockLabel {
                    x: undo_x,
                    y,
                    text: undo_text.to_string(),
                    color: Rgb {
                        r: 100,
                        g: 160,
                        b: 220,
                    },
                });
            }
        }

        labels
    }

    /// Build colored rectangles for pipeline stage panel at the bottom of viewport.
    ///
    /// Renders a fixed panel above the status bar showing stages for the most
    /// recently expanded pipeline block. Panel grows upward when a stage is expanded.
    pub fn build_pipeline_rects(
        &self,
        blocks: &[&Block],
        viewport_width: f32,
        viewport_height: f32,
        status_bar_height: f32,
    ) -> Vec<RectInstance> {
        let mut rects = Vec::new();

        let block = match blocks.iter().rev().find(|b| b.pipeline_expanded) {
            Some(b) => b,
            None => return rects,
        };

        let stage_count = Self::panel_stage_count(block);
        if stage_count == 0 { return rects; }

        let total_rows = Self::panel_total_rows(block, stage_count);
        let panel_top = viewport_height - status_bar_height - total_rows as f32 * self.cell_height;
        let mut row = 0;

        for i in 0..stage_count {
            let row_y = panel_top + row as f32 * self.cell_height;
            let selected = block.expanded_stage_index == Some(i);
            let color = if selected {
                [50.0 / 255.0, 50.0 / 255.0, 70.0 / 255.0, 0.95]
            } else {
                [30.0 / 255.0, 30.0 / 255.0, 40.0 / 255.0, 0.95]
            };
            rects.push(RectInstance {
                pos: [0.0, row_y, viewport_width, self.cell_height],
                color,
            });
            row += 1;

            // Expanded stage output rows
            if selected {
                let output_rows = Self::expanded_output_row_count(block, i);
                for j in 0..output_rows {
                    let out_y = panel_top + (row + j) as f32 * self.cell_height;
                    rects.push(RectInstance {
                        pos: [0.0, out_y, viewport_width, self.cell_height],
                        color: [25.0 / 255.0, 25.0 / 255.0, 35.0 / 255.0, 0.9],
                    });
                }
                row += output_rows;
            }
        }

        rects
    }

    /// Build text labels for pipeline stage panel at the bottom of viewport.
    pub fn build_pipeline_text(
        &self,
        blocks: &[&Block],
        viewport_width: f32,
        viewport_height: f32,
        status_bar_height: f32,
    ) -> Vec<BlockLabel> {
        let mut labels = Vec::new();

        let block = match blocks.iter().rev().find(|b| b.pipeline_expanded) {
            Some(b) => b,
            None => return labels,
        };

        let stage_count = Self::panel_stage_count(block);
        if stage_count == 0 { return labels; }

        let total_rows = Self::panel_total_rows(block, stage_count);
        let panel_top = viewport_height - status_bar_height - total_rows as f32 * self.cell_height;
        let mut row = 0;

        for i in 0..stage_count {
            let row_y = panel_top + row as f32 * self.cell_height;
            let captured = block.pipeline_stages.get(i);
            let selected = block.expanded_stage_index == Some(i);

            // Command text
            let cmd_text = if i < block.pipeline_stage_commands.len() {
                format!("  stage {}: {}", i, block.pipeline_stage_commands[i])
            } else {
                format!("  stage {}", i)
            };
            labels.push(BlockLabel {
                x: self.cell_width * 2.0,
                y: row_y,
                text: cmd_text,
                color: Rgb { r: 180, g: 180, b: 220 },
            });

            // Line count and byte count
            let (line_text, byte_text) = if let Some(stage) = captured {
                let lines = line_count(&stage.data);
                let lt = if lines == 1 {
                    "1 line".to_string()
                } else {
                    format!("{} lines", lines)
                };
                (lt, format_bytes(stage.total_bytes))
            } else {
                ("".to_string(), "".to_string())
            };

            // Expand/collapse indicator
            let indicator = if selected { "[-]" } else { "[+]" };

            let indicator_width = indicator.len() as f32 * self.cell_width;
            let indicator_x = viewport_width - indicator_width - self.cell_width;
            labels.push(BlockLabel {
                x: indicator_x,
                y: row_y,
                text: indicator.to_string(),
                color: Rgb { r: 100, g: 160, b: 220 },
            });

            let byte_width = byte_text.len() as f32 * self.cell_width;
            let byte_x = indicator_x - byte_width - self.cell_width;
            if !byte_text.is_empty() {
                labels.push(BlockLabel {
                    x: byte_x,
                    y: row_y,
                    text: byte_text,
                    color: Rgb { r: 140, g: 140, b: 140 },
                });
            }

            let line_width = line_text.len() as f32 * self.cell_width;
            let line_x = byte_x - line_width - self.cell_width;
            if !line_text.is_empty() {
                labels.push(BlockLabel {
                    x: line_x,
                    y: row_y,
                    text: line_text,
                    color: Rgb { r: 140, g: 140, b: 140 },
                });
            }

            row += 1;

            // Expanded stage output
            if selected {
                let output_labels = self.build_expanded_output(block, i, panel_top, &mut row);
                labels.extend(output_labels);
            }
        }

        labels
    }

    /// Helper: stage count for panel.
    fn panel_stage_count(block: &Block) -> usize {
        if !block.pipeline_stage_commands.is_empty() {
            block.pipeline_stage_commands.len()
        } else {
            block.pipeline_stages.len()
        }
    }

    /// Helper: total panel rows including expanded output.
    fn panel_total_rows(block: &Block, stage_count: usize) -> usize {
        let mut rows = stage_count;
        if let Some(idx) = block.expanded_stage_index {
            rows += Self::expanded_output_row_count(block, idx);
        }
        rows
    }

    /// Helper: number of output rows for an expanded stage.
    fn expanded_output_row_count(block: &Block, stage_idx: usize) -> usize {
        if let Some(stage) = block.pipeline_stages.get(stage_idx) {
            let lines = line_count(&stage.data);
            if lines == 0 { 1 } else { lines.min(30) } // at least 1 for "empty" message
        } else {
            1 // "no data captured" message
        }
    }

    /// Build text labels for expanded stage output in the panel.
    fn build_expanded_output(
        &self,
        block: &Block,
        stage_idx: usize,
        panel_top: f32,
        row: &mut usize,
    ) -> Vec<BlockLabel> {
        let mut labels = Vec::new();
        let content_color = Rgb { r: 160, g: 160, b: 160 };
        let x = self.cell_width * 4.0;

        if let Some(stage) = block.pipeline_stages.get(stage_idx) {
            match &stage.data {
                FinalizedBuffer::Complete(bytes) if bytes.is_empty() => {
                    let y = panel_top + *row as f32 * self.cell_height;
                    labels.push(BlockLabel { x, y, text: "  (empty)".to_string(), color: content_color });
                    *row += 1;
                }
                FinalizedBuffer::Complete(bytes) => {
                    let text = String::from_utf8_lossy(bytes);
                    for line in text.lines().take(30) {
                        let y = panel_top + *row as f32 * self.cell_height;
                        labels.push(BlockLabel { x, y, text: format!("  | {}", line), color: content_color });
                        *row += 1;
                    }
                }
                FinalizedBuffer::Sampled { head, tail, total_bytes } => {
                    let head_text = String::from_utf8_lossy(head);
                    for line in head_text.lines().take(15) {
                        let y = panel_top + *row as f32 * self.cell_height;
                        labels.push(BlockLabel { x, y, text: format!("  | {}", line), color: content_color });
                        *row += 1;
                    }
                    let omitted = total_bytes - head.len() - tail.len();
                    let y = panel_top + *row as f32 * self.cell_height;
                    labels.push(BlockLabel { x, y, text: format!("  | ... {} bytes omitted ...", omitted), color: content_color });
                    *row += 1;
                    let tail_text = String::from_utf8_lossy(tail);
                    for line in tail_text.lines().rev().take(15).collect::<Vec<_>>().into_iter().rev() {
                        let y = panel_top + *row as f32 * self.cell_height;
                        labels.push(BlockLabel { x, y, text: format!("  | {}", line), color: content_color });
                        *row += 1;
                    }
                }
                FinalizedBuffer::Binary { size } => {
                    let y = panel_top + *row as f32 * self.cell_height;
                    labels.push(BlockLabel { x, y, text: format!("  [binary: {}]", format_bytes(*size)), color: content_color });
                    *row += 1;
                }
            }
        } else {
            let y = panel_top + *row as f32 * self.cell_height;
            labels.push(BlockLabel { x, y, text: "  (no captured data)".to_string(), color: Rgb { r: 120, g: 120, b: 120 } });
            *row += 1;
        }

        labels
    }
}

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

    /// Build colored rectangles for pipeline stage rows.
    ///
    /// For each expanded pipeline block, generates a subtle background rect
    /// per stage row, positioned below the block separator.
    pub fn build_pipeline_rects(
        &self,
        blocks: &[&Block],
        display_offset: usize,
        screen_lines: usize,
        viewport_width: f32,
    ) -> Vec<RectInstance> {
        let mut rects = Vec::new();

        for block in blocks {
            if !block.pipeline_expanded || block.pipeline_stages.is_empty() {
                continue;
            }

            let line = block.prompt_start_line;
            if line < display_offset || line >= display_offset + screen_lines {
                continue;
            }

            let block_y = (line - display_offset) as f32 * self.cell_height;

            for (i, _stage) in block.pipeline_stages.iter().enumerate() {
                let row_y = block_y + self.cell_height * (i as f32 + 1.0);

                // Subtle background for pipeline stage row
                rects.push(RectInstance {
                    pos: [0.0, row_y, viewport_width, self.cell_height],
                    color: [30.0 / 255.0, 30.0 / 255.0, 40.0 / 255.0, 0.8],
                });

                // If this stage is expanded, add background rects for output lines
                if block.expanded_stage_index == Some(i) {
                    let output_lines = line_count(&_stage.data).min(50);
                    for line_idx in 0..output_lines {
                        let output_y = row_y + self.cell_height * (line_idx as f32 + 1.0);
                        rects.push(RectInstance {
                            pos: [0.0, output_y, viewport_width, self.cell_height],
                            color: [25.0 / 255.0, 25.0 / 255.0, 35.0 / 255.0, 0.8],
                        });
                    }
                }
            }
        }

        rects
    }

    /// Build text labels for pipeline stage rows.
    ///
    /// For each expanded pipeline block, generates labels showing:
    /// - Stage command text
    /// - Line count (right-aligned)
    /// - Byte count (right-aligned)
    /// - Expand/collapse indicator
    pub fn build_pipeline_text(
        &self,
        blocks: &[&Block],
        display_offset: usize,
        screen_lines: usize,
        viewport_width: f32,
    ) -> Vec<BlockLabel> {
        let mut labels = Vec::new();

        for block in blocks {
            if !block.pipeline_expanded || block.pipeline_stages.is_empty() {
                continue;
            }

            let line = block.prompt_start_line;
            if line < display_offset || line >= display_offset + screen_lines {
                continue;
            }

            let block_y = (line - display_offset) as f32 * self.cell_height;

            for (i, stage) in block.pipeline_stages.iter().enumerate() {
                let row_y = block_y + self.cell_height * (i as f32 + 1.0);

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

                // Line count
                let lines = line_count(&stage.data);
                let line_text = if lines == 1 {
                    "1 line".to_string()
                } else {
                    format!("{} lines", lines)
                };

                // Byte count
                let byte_text = format_bytes(stage.total_bytes);

                // Expand indicator
                let indicator = if block.expanded_stage_index == Some(i) {
                    "[^]"
                } else {
                    "[v]"
                };

                // Position right-aligned: indicator at far right, then byte count, then line count
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
                labels.push(BlockLabel {
                    x: byte_x,
                    y: row_y,
                    text: byte_text,
                    color: Rgb { r: 140, g: 140, b: 140 },
                });

                let line_width = line_text.len() as f32 * self.cell_width;
                let line_x = byte_x - line_width - self.cell_width;
                labels.push(BlockLabel {
                    x: line_x,
                    y: row_y,
                    text: line_text,
                    color: Rgb { r: 140, g: 140, b: 140 },
                });

                // If this stage is expanded, render captured output lines
                if block.expanded_stage_index == Some(i) {
                    let output_labels = self.build_stage_output_labels(
                        &stage.data,
                        row_y,
                    );
                    labels.extend(output_labels);
                }
            }
        }

        labels
    }

    /// Build text labels for expanded stage output content.
    fn build_stage_output_labels(
        &self,
        data: &FinalizedBuffer,
        stage_row_y: f32,
    ) -> Vec<BlockLabel> {
        let mut labels = Vec::new();
        let content_color = Rgb { r: 160, g: 160, b: 160 };
        let x = self.cell_width * 4.0;

        match data {
            FinalizedBuffer::Complete(bytes) => {
                let text = String::from_utf8_lossy(bytes);
                for (idx, line) in text.lines().take(50).enumerate() {
                    let y = stage_row_y + self.cell_height * (idx as f32 + 1.0);
                    labels.push(BlockLabel {
                        x,
                        y,
                        text: format!("  | {}", line),
                        color: content_color,
                    });
                }
            }
            FinalizedBuffer::Sampled { head, tail, total_bytes } => {
                let head_text = String::from_utf8_lossy(head);
                let head_lines: Vec<&str> = head_text.lines().collect();
                let tail_text = String::from_utf8_lossy(tail);
                let tail_lines: Vec<&str> = tail_text.lines().collect();

                let max_head = 25.min(head_lines.len());
                let max_tail = 25.min(tail_lines.len());
                let mut line_idx = 0;

                for line in head_lines.iter().take(max_head) {
                    let y = stage_row_y + self.cell_height * (line_idx as f32 + 1.0);
                    labels.push(BlockLabel {
                        x,
                        y,
                        text: format!("  | {}", line),
                        color: content_color,
                    });
                    line_idx += 1;
                }

                // Omission indicator
                let omitted = total_bytes - head.len() - tail.len();
                let y = stage_row_y + self.cell_height * (line_idx as f32 + 1.0);
                labels.push(BlockLabel {
                    x,
                    y,
                    text: format!("  | ... {} bytes omitted ...", omitted),
                    color: content_color,
                });
                line_idx += 1;

                // Tail lines (from the end)
                let tail_start = tail_lines.len().saturating_sub(max_tail);
                for line in tail_lines.iter().skip(tail_start) {
                    let y = stage_row_y + self.cell_height * (line_idx as f32 + 1.0);
                    labels.push(BlockLabel {
                        x,
                        y,
                        text: format!("  | {}", line),
                        color: content_color,
                    });
                    line_idx += 1;
                }
            }
            FinalizedBuffer::Binary { size } => {
                let y = stage_row_y + self.cell_height;
                labels.push(BlockLabel {
                    x,
                    y,
                    text: format!("  | [binary: {}]", format_bytes(*size)),
                    color: content_color,
                });
            }
        }

        labels
    }
}

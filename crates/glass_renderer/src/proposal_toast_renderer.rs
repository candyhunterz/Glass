//! ProposalToastRenderer: generates visual elements for the agent proposal toast notification.
//!
//! Produces a dark teal toast rect at the bottom-right of the viewport (above the
//! status bar) and two text labels: a truncated proposal description and a keyboard
//! hint with auto-dismiss countdown. Display-only; does not intercept keyboard input.

use alacritty_terminal::vte::ansi::Rgb;

use crate::rect_renderer::RectInstance;

/// Data transferred from Processor to the renderer for the proposal toast.
#[derive(Debug, Clone)]
pub struct ProposalToastRenderData {
    /// Short description of the pending proposal.
    pub description: String,
    /// Seconds remaining before the toast auto-dismisses.
    pub remaining_secs: u64,
}

/// A text label to be rendered in the proposal toast.
#[derive(Debug, Clone)]
pub struct ProposalToastTextLabel {
    /// Text content
    pub text: String,
    /// X position in pixels
    pub x: f32,
    /// Y position in pixels
    pub y: f32,
    /// Text color
    pub color: Rgb,
}

/// Renders proposal toast visual elements (dark teal rect + description + hint text).
///
/// Stateless helper that converts ProposalToastRenderData into RectInstances and text
/// labels for the GPU rendering pipeline. Follows the ConflictOverlay pattern.
pub struct ProposalToastRenderer {
    cell_width: f32,
    cell_height: f32,
}

impl ProposalToastRenderer {
    /// Create a new ProposalToastRenderer with the given cell dimensions.
    pub fn new(cell_width: f32, cell_height: f32) -> Self {
        Self {
            cell_width,
            cell_height,
        }
    }

    /// Build the toast background rectangle.
    ///
    /// Returns a single rect: right-aligned, positioned just above the status bar.
    /// Toast width = 60% viewport, height = 2.5 * cell_height.
    /// Color: dark teal [0.05, 0.25, 0.35, 0.92].
    pub fn build_toast_rects(
        &self,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Vec<RectInstance> {
        let toast_w = viewport_w * 0.6;
        let toast_h = self.cell_height * 2.5;
        let x = viewport_w - toast_w - self.cell_width;
        // Above status bar (cell_height) with a 0.5 cell gap
        let y = viewport_h - self.cell_height - toast_h - self.cell_height * 0.5;
        vec![RectInstance {
            pos: [x, y, toast_w, toast_h],
            color: [0.05, 0.25, 0.35, 0.92],
        }]
    }

    /// Build the text labels for the proposal toast.
    ///
    /// Returns two ProposalToastTextLabel structs:
    /// - Line 1: proposal description (truncated to 60 chars)
    /// - Line 2: keyboard hint + remaining seconds
    pub fn build_toast_text(
        &self,
        data: &ProposalToastRenderData,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Vec<ProposalToastTextLabel> {
        let toast_w = viewport_w * 0.6;
        let toast_h = self.cell_height * 2.5;
        let x = viewport_w - toast_w - self.cell_width + self.cell_width * 0.5;
        let y_base = viewport_h - self.cell_height - toast_h - self.cell_height * 0.5;

        // Truncate description to 60 chars
        let description = if data.description.len() > 60 {
            format!("{}...", &data.description[..57])
        } else {
            data.description.clone()
        };

        let line1 = ProposalToastTextLabel {
            text: description,
            x,
            y: y_base + self.cell_height * 0.25,
            color: Rgb {
                r: 220,
                g: 220,
                b: 220,
            },
        };

        let line2 = ProposalToastTextLabel {
            text: format!(
                "[Ctrl+Shift+A: review] [auto-dismiss in {}s]",
                data.remaining_secs
            ),
            x,
            y: y_base + self.cell_height * 1.25,
            color: Rgb {
                r: 160,
                g: 180,
                b: 190,
            },
        };

        vec![line1, line2]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer() -> ProposalToastRenderer {
        ProposalToastRenderer::new(10.0, 20.0)
    }

    fn sample_data() -> ProposalToastRenderData {
        ProposalToastRenderData {
            description: "Create a new authentication module".to_string(),
            remaining_secs: 30,
        }
    }

    #[test]
    fn test_build_toast_rects_count() {
        let r = renderer();
        let rects = r.build_toast_rects(800.0, 600.0);
        assert_eq!(rects.len(), 1, "Toast should produce exactly 1 rect");
    }

    #[test]
    fn test_build_toast_rects_color() {
        let r = renderer();
        let rects = r.build_toast_rects(800.0, 600.0);
        let c = rects[0].color;
        assert!((c[0] - 0.05).abs() < 0.01, "R channel should be 0.05");
        assert!((c[1] - 0.25).abs() < 0.01, "G channel should be 0.25");
        assert!((c[2] - 0.35).abs() < 0.01, "B channel should be 0.35");
        assert!((c[3] - 0.92).abs() < 0.01, "Alpha should be 0.92");
    }

    #[test]
    fn test_build_toast_rects_position_right_aligned() {
        let r = renderer();
        // cell_width=10, viewport_w=800
        // toast_w = 800 * 0.6 = 480
        // x = 800 - 480 - 10 = 310
        let rects = r.build_toast_rects(800.0, 600.0);
        let pos = rects[0].pos;
        assert!((pos[0] - 310.0).abs() < 0.01, "Toast x should be right-aligned, got {}", pos[0]);
        assert!((pos[2] - 480.0).abs() < 0.01, "Toast width should be 60% of viewport, got {}", pos[2]);
    }

    #[test]
    fn test_build_toast_rects_above_status_bar() {
        let r = renderer();
        // cell_height=20, toast_h = 20*2.5=50
        // y = 600 - 20 - 50 - 20*0.5 = 600 - 80 = 520
        let rects = r.build_toast_rects(800.0, 600.0);
        let pos = rects[0].pos;
        assert_eq!(pos[1], 520.0, "Toast y should be above status bar");
        assert_eq!(pos[3], 50.0, "Toast height should be 2.5 * cell_height");
    }

    #[test]
    fn test_build_toast_text_count() {
        let r = renderer();
        let data = sample_data();
        let labels = r.build_toast_text(&data, 800.0, 600.0);
        assert_eq!(labels.len(), 2, "Toast should produce exactly 2 text labels");
    }

    #[test]
    fn test_build_toast_text_line1_description() {
        let r = renderer();
        let data = sample_data();
        let labels = r.build_toast_text(&data, 800.0, 600.0);
        assert_eq!(
            labels[0].text, "Create a new authentication module",
            "Line 1 should be the description"
        );
    }

    #[test]
    fn test_build_toast_text_line2_hint() {
        let r = renderer();
        let data = sample_data();
        let labels = r.build_toast_text(&data, 800.0, 600.0);
        assert!(
            labels[1].text.contains("Ctrl+Shift+A: review"),
            "Line 2 should contain keyboard hint"
        );
        assert!(
            labels[1].text.contains("30s"),
            "Line 2 should show remaining seconds"
        );
    }

    #[test]
    fn test_build_toast_text_description_truncation() {
        let r = renderer();
        let long_desc = "A".repeat(80);
        let data = ProposalToastRenderData {
            description: long_desc,
            remaining_secs: 10,
        };
        let labels = r.build_toast_text(&data, 800.0, 600.0);
        // 57 chars + "..." = 60 chars
        assert_eq!(
            labels[0].text.len(),
            60,
            "Description should be truncated to 60 chars"
        );
        assert!(
            labels[0].text.ends_with("..."),
            "Truncated description should end with ..."
        );
    }

    #[test]
    fn test_build_toast_text_no_truncation_under_60() {
        let r = renderer();
        let desc = "Short description".to_string();
        let data = ProposalToastRenderData {
            description: desc.clone(),
            remaining_secs: 5,
        };
        let labels = r.build_toast_text(&data, 800.0, 600.0);
        assert_eq!(labels[0].text, desc, "Short descriptions should not be truncated");
    }
}

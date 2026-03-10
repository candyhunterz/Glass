//! ConflictOverlay: generates visual elements for the conflict warning banner.
//!
//! Produces a dark amber banner rect at the bottom of the viewport (above the
//! status bar) and a text label showing agent/lock count when multiple agents
//! are active with locks. Display-only; does not intercept keyboard input.

use alacritty_terminal::vte::ansi::Rgb;

use crate::rect_renderer::RectInstance;

/// A text label to be rendered in the conflict warning overlay.
#[derive(Debug, Clone)]
pub struct ConflictTextLabel {
    /// Text content
    pub text: String,
    /// X position in pixels
    pub x: f32,
    /// Y position in pixels
    pub y: f32,
    /// Text color
    pub color: Rgb,
}

/// Renders conflict warning overlay visual elements (amber banner rect + warning text).
///
/// Stateless helper that converts agent/lock counts into RectInstances and text
/// labels for the GPU rendering pipeline. Follows the ConfigErrorOverlay pattern.
pub struct ConflictOverlay {
    cell_width: f32,
    cell_height: f32,
}

impl ConflictOverlay {
    /// Create a new ConflictOverlay with the given cell dimensions.
    pub fn new(cell_width: f32, cell_height: f32) -> Self {
        Self {
            cell_width,
            cell_height,
        }
    }

    /// Build the warning banner rectangle at the bottom of the viewport.
    ///
    /// Returns a single rect: full viewport width, positioned above the status bar,
    /// dark amber background (220, 160, 0) at 90% opacity.
    pub fn build_warning_rects(
        &self,
        viewport_width: f32,
        viewport_height: f32,
        line_count: usize,
    ) -> Vec<RectInstance> {
        let y = viewport_height - self.cell_height * (line_count + 1) as f32;
        vec![RectInstance {
            pos: [0.0, y, viewport_width, self.cell_height * line_count as f32],
            color: [220.0 / 255.0, 160.0 / 255.0, 0.0 / 255.0, 0.9],
        }]
    }

    /// Build the warning text label for display inside the banner.
    ///
    /// Formats as "Warning: {agent_count} agents active, {lock_count} locks held".
    /// White text (220, 220, 220), positioned with cell_width/2 padding.
    pub fn build_warning_text(
        &self,
        agent_count: usize,
        lock_count: usize,
        viewport_height: f32,
    ) -> Vec<ConflictTextLabel> {
        vec![ConflictTextLabel {
            text: format!(
                "Warning: {} agents active, {} locks held",
                agent_count, lock_count
            ),
            x: self.cell_width * 0.5,
            y: viewport_height - self.cell_height * 2.0,
            color: Rgb {
                r: 220,
                g: 220,
                b: 220,
            },
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlay() -> ConflictOverlay {
        ConflictOverlay::new(10.0, 20.0)
    }

    #[test]
    fn test_warning_rects_amber_color() {
        let o = overlay();
        let rects = o.build_warning_rects(800.0, 600.0, 1);
        assert_eq!(rects.len(), 1);
        // Amber color: 220/255, 160/255, 0/255, 0.9
        assert!((rects[0].color[0] - 220.0 / 255.0).abs() < 0.01);
        assert!((rects[0].color[1] - 160.0 / 255.0).abs() < 0.01);
        assert!((rects[0].color[2] - 0.0).abs() < 0.01);
        assert!((rects[0].color[3] - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_warning_rects_position_above_status() {
        let o = overlay();
        // viewport_height=600, cell_height=20, line_count=1
        // y = 600 - 20 * (1+1) = 600 - 40 = 560
        let rects = o.build_warning_rects(800.0, 600.0, 1);
        assert_eq!(rects[0].pos[0], 0.0); // x
        assert_eq!(rects[0].pos[1], 560.0); // y = viewport_height - cell_height * 2
        assert_eq!(rects[0].pos[2], 800.0); // width = viewport width
        assert_eq!(rects[0].pos[3], 20.0); // height = cell_height * line_count
    }

    #[test]
    fn test_warning_text_content() {
        let o = overlay();
        let labels = o.build_warning_text(3, 5, 600.0);
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].text, "Warning: 3 agents active, 5 locks held");
        assert_eq!(
            labels[0].color,
            Rgb {
                r: 220,
                g: 220,
                b: 220
            }
        );
    }

    #[test]
    fn test_warning_text_position() {
        let o = overlay();
        let labels = o.build_warning_text(2, 1, 600.0);
        // x = cell_width * 0.5 = 5.0
        assert_eq!(labels[0].x, 5.0);
        // y = viewport_height - cell_height * 2 = 600 - 40 = 560
        assert_eq!(labels[0].y, 560.0);
    }
}

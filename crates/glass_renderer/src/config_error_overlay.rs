//! ConfigErrorOverlay: generates visual elements for the config error banner.
//!
//! Produces a dark red banner rect at the top of the viewport and a text label
//! showing the error message with line/column info. Display-only; does not
//! intercept keyboard input.

use alacritty_terminal::vte::ansi::Rgb;

use crate::rect_renderer::RectInstance;

/// A text label to be rendered in the config error overlay.
#[derive(Debug, Clone)]
pub struct ConfigErrorTextLabel {
    /// Text content
    pub text: String,
    /// X position in pixels
    pub x: f32,
    /// Y position in pixels
    pub y: f32,
    /// Text color
    pub color: Rgb,
}

/// Renders config error overlay visual elements (banner rect + error text).
///
/// Stateless helper that converts a ConfigError into RectInstances and text labels
/// for the GPU rendering pipeline. Follows the SearchOverlayRenderer pattern.
pub struct ConfigErrorOverlay {
    cell_width: f32,
    cell_height: f32,
}

impl ConfigErrorOverlay {
    /// Create a new ConfigErrorOverlay with the given cell dimensions.
    pub fn new(cell_width: f32, cell_height: f32) -> Self {
        Self {
            cell_width,
            cell_height,
        }
    }

    /// Build the error banner rectangle at the top of the viewport.
    ///
    /// Returns a single rect: full viewport width, 1 cell_height tall,
    /// dark red background (180, 40, 40) at 90% opacity.
    pub fn build_error_rects(&self, viewport_width: f32) -> Vec<RectInstance> {
        vec![RectInstance {
            pos: [0.0, 0.0, viewport_width, self.cell_height],
            color: [180.0 / 255.0, 40.0 / 255.0, 40.0 / 255.0, 0.9],
        }]
    }

    /// Build the error text label for display inside the banner.
    ///
    /// Formats as "Config error (line X, col Y): message" using the ConfigError's
    /// Display impl. White text (220, 220, 220), positioned with cell_width/2 padding.
    pub fn build_error_text(
        &self,
        error: &glass_core::config::ConfigError,
        _viewport_width: f32,
    ) -> Vec<ConfigErrorTextLabel> {
        vec![ConfigErrorTextLabel {
            text: format!("{}", error),
            x: self.cell_width * 0.5,
            y: 0.0,
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

    fn overlay() -> ConfigErrorOverlay {
        ConfigErrorOverlay::new(10.0, 20.0)
    }

    #[test]
    fn test_error_rects_single_rect() {
        let o = overlay();
        let rects = o.build_error_rects(800.0);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].pos[0], 0.0); // x
        assert_eq!(rects[0].pos[1], 0.0); // y
        assert_eq!(rects[0].pos[2], 800.0); // width = viewport width
        assert_eq!(rects[0].pos[3], 20.0); // height = cell_height
    }

    #[test]
    fn test_error_rects_color() {
        let o = overlay();
        let rects = o.build_error_rects(800.0);
        // Dark red at 90% opacity
        assert!((rects[0].color[0] - 180.0 / 255.0).abs() < 0.01);
        assert!((rects[0].color[3] - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_error_text_with_line_col() {
        let o = overlay();
        let err = glass_core::config::ConfigError {
            message: "expected string".to_string(),
            line: Some(3),
            column: Some(5),
            snippet: None,
        };
        let labels = o.build_error_text(&err, 800.0);
        assert_eq!(labels.len(), 1);
        assert_eq!(
            labels[0].text,
            "~/.glass/config.toml (line 3, col 5): expected string"
        );
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
    fn test_error_text_without_line_col() {
        let o = overlay();
        let err = glass_core::config::ConfigError {
            message: "something failed".to_string(),
            line: None,
            column: None,
            snippet: None,
        };
        let labels = o.build_error_text(&err, 800.0);
        assert_eq!(labels[0].text, "~/.glass/config.toml: something failed");
    }

    #[test]
    fn test_error_text_position() {
        let o = overlay();
        let err = glass_core::config::ConfigError {
            message: "test".to_string(),
            line: None,
            column: None,
            snippet: None,
        };
        let labels = o.build_error_text(&err, 800.0);
        // x should be cell_width * 0.5 = 5.0
        assert_eq!(labels[0].x, 5.0);
        assert_eq!(labels[0].y, 0.0);
    }
}

//! StatusBarRenderer: bottom-pinned status bar with CWD and git info.
//!
//! Produces a background rectangle and text labels for the status bar
//! that sits at the bottom of the terminal viewport.

use alacritty_terminal::vte::ansi::Rgb;

use glass_terminal::GitInfo;

use crate::rect_renderer::RectInstance;

/// Text content for the status bar.
#[derive(Debug, Clone)]
pub struct StatusLabel {
    /// Left-aligned text (CWD path)
    pub left_text: String,
    /// Right-aligned text (git branch + dirty count)
    pub right_text: Option<String>,
    /// Center-aligned text (update notification)
    pub center_text: Option<String>,
    /// Y position in pixels
    pub y: f32,
    /// Color for left text (CWD)
    pub left_color: Rgb,
    /// Color for right text (git info)
    pub right_color: Rgb,
    /// Color for center text (update notification)
    pub center_color: Rgb,
}

/// Renders the bottom-pinned status bar.
///
/// Produces a background rectangle and text labels showing the current
/// working directory and optional git branch/dirty information.
pub struct StatusBarRenderer {
    cell_height: f32,
}

impl StatusBarRenderer {
    /// Create a new StatusBarRenderer with the given cell height.
    pub fn new(cell_height: f32) -> Self {
        Self { cell_height }
    }

    /// Build the status bar background rectangle.
    ///
    /// Returns a single full-width rect at the bottom of the viewport,
    /// 1 cell_height tall, slightly lighter than terminal background.
    pub fn build_status_rects(
        &self,
        viewport_width: f32,
        viewport_height: f32,
    ) -> Vec<RectInstance> {
        let y = viewport_height - self.cell_height;
        vec![RectInstance {
            pos: [0.0, y, viewport_width, self.cell_height],
            // Slightly lighter than terminal bg (26,26,26) -> (38,38,38)
            color: [38.0 / 255.0, 38.0 / 255.0, 38.0 / 255.0, 1.0],
        }]
    }

    /// Build text content for the status bar.
    ///
    /// Left side: CWD path (truncated if needed).
    /// Center: update notification (if available).
    /// Right side: git branch name + dirty count if available.
    pub fn build_status_text(
        &self,
        cwd: &str,
        git_info: Option<&GitInfo>,
        update_text: Option<&str>,
        viewport_height: f32,
    ) -> StatusLabel {
        let y = viewport_height - self.cell_height;

        // Truncate CWD if too long (keep last 60 chars with leading ...)
        let left_text = if cwd.len() > 60 {
            format!("...{}", &cwd[cwd.len() - 57..])
        } else {
            cwd.to_string()
        };

        let right_text = git_info.map(|info| {
            if info.dirty_count > 0 {
                format!("{} +{}", info.branch, info.dirty_count)
            } else {
                info.branch.clone()
            }
        });

        let center_text = update_text.map(|t| t.to_string());

        // Git branch color: cyan if clean, with yellow dirty count appended
        // For simplicity, use cyan as the base right_color
        let right_color = Rgb {
            r: 80,
            g: 200,
            b: 200,
        };

        // Bright yellow-gold for update notification visibility
        let center_color = Rgb {
            r: 255,
            g: 200,
            b: 50,
        };

        StatusLabel {
            left_text,
            right_text,
            center_text,
            y,
            left_color: Rgb {
                r: 204,
                g: 204,
                b: 204,
            },
            right_color,
            center_color,
        }
    }
}

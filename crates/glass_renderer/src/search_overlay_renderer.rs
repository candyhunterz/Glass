//! SearchOverlayRenderer: generates visual elements for the search overlay.
//!
//! Produces colored rectangles for the backdrop, search input box, and result rows,
//! plus text labels for the query string and result details.

use alacritty_terminal::vte::ansi::Rgb;

use crate::rect_renderer::RectInstance;

/// Maximum number of visible result rows in the overlay.
const MAX_VISIBLE_RESULTS: usize = 10;

/// A text label to be rendered in the search overlay.
#[derive(Debug, Clone)]
pub struct SearchOverlayTextLabel {
    /// Text content
    pub text: String,
    /// X position in pixels
    pub x: f32,
    /// Y position in pixels
    pub y: f32,
    /// Text color
    pub color: Rgb,
}

/// Renders search overlay visual elements (backdrop, input box, result rows).
///
/// Stateless helper that converts overlay data into RectInstances and text labels
/// for the GPU rendering pipeline.
pub struct SearchOverlayRenderer {
    cell_width: f32,
    cell_height: f32,
}

impl SearchOverlayRenderer {
    /// Create a new SearchOverlayRenderer with the given cell dimensions.
    pub fn new(cell_width: f32, cell_height: f32) -> Self {
        Self {
            cell_width,
            cell_height,
        }
    }

    /// Build colored rectangles for the overlay backdrop, search box, and result rows.
    ///
    /// Returns:
    /// - 1 semi-transparent backdrop covering the full viewport
    /// - 1 search input box rect
    /// - Up to MAX_VISIBLE_RESULTS result row rects (selected row has highlight color)
    pub fn build_overlay_rects(
        &self,
        results_len: usize,
        selected: usize,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Vec<RectInstance> {
        let visible_count = results_len.min(MAX_VISIBLE_RESULTS);
        let mut rects = Vec::with_capacity(2 + visible_count);

        // Semi-transparent backdrop
        rects.push(RectInstance {
            pos: [0.0, 0.0, viewport_w, viewport_h],
            color: [0.05, 0.05, 0.05, 0.85],
        });

        // Search input box: centered with margin, near top
        let margin = 4.0 * self.cell_width;
        let input_x = margin;
        let input_y = 2.0 * self.cell_height;
        let input_w = viewport_w - 2.0 * margin;
        let input_h = 1.5 * self.cell_height;
        rects.push(RectInstance {
            pos: [input_x, input_y, input_w, input_h],
            color: [0.22, 0.22, 0.22, 1.0],
        });

        // Result rows
        let row_start_y = input_y + input_h + 0.5 * self.cell_height;
        let row_height = 2.2 * self.cell_height;
        let row_gap = 0.3 * self.cell_height;

        for i in 0..visible_count {
            let row_y = row_start_y + i as f32 * (row_height + row_gap);
            let color = if i == selected {
                [0.15, 0.30, 0.50, 1.0] // Highlight color
            } else {
                [0.12, 0.12, 0.12, 1.0] // Normal color
            };
            rects.push(RectInstance {
                pos: [input_x, row_y, input_w, row_height],
                color,
            });
        }

        rects
    }

    /// Build text labels for the search query and result details.
    ///
    /// Returns labels for:
    /// - Query text (prefixed with "Search: ") inside the input box
    /// - For each visible result: command text (line 1) and metadata (line 2)
    pub fn build_overlay_text(
        &self,
        query: &str,
        results: &[(String, Option<i32>, String, String)], // (command, exit_code, timestamp, preview)
        selected: usize,
        viewport_w: f32,
        _viewport_h: f32,
    ) -> Vec<SearchOverlayTextLabel> {
        let visible_count = results.len().min(MAX_VISIBLE_RESULTS);
        let mut labels = Vec::with_capacity(1 + visible_count * 2);

        let margin = 4.0 * self.cell_width;
        let padding = 0.5 * self.cell_width;
        let input_x = margin;
        let input_y = 2.0 * self.cell_height;
        let input_h = 1.5 * self.cell_height;
        let _ = viewport_w; // used for layout constraints if needed

        // Query text label
        labels.push(SearchOverlayTextLabel {
            text: format!("Search: {}", query),
            x: input_x + padding,
            y: input_y + (input_h - self.cell_height) * 0.5,
            color: Rgb { r: 230, g: 230, b: 230 },
        });

        // Result rows
        let row_start_y = input_y + input_h + 0.5 * self.cell_height;
        let row_height = 2.2 * self.cell_height;
        let row_gap = 0.3 * self.cell_height;

        for i in 0..visible_count {
            let (ref command, exit_code, ref timestamp, ref preview) = results[i];
            let row_y = row_start_y + i as f32 * (row_height + row_gap);

            // Line 1: command text
            let cmd_color = if i == selected {
                Rgb { r: 240, g: 240, b: 240 } // Brighter when selected
            } else {
                Rgb { r: 204, g: 204, b: 204 }
            };
            labels.push(SearchOverlayTextLabel {
                text: command.clone(),
                x: input_x + padding,
                y: row_y + 0.2 * self.cell_height,
                color: cmd_color,
            });

            // Line 2: metadata
            let exit_badge = match exit_code {
                Some(0) | None => "OK".to_string(),
                Some(code) => format!("X:{}", code),
            };
            let meta_text = format!("{}  {}  {}", exit_badge, timestamp, preview);
            labels.push(SearchOverlayTextLabel {
                text: meta_text,
                x: input_x + padding,
                y: row_y + 0.2 * self.cell_height + self.cell_height,
                color: Rgb { r: 120, g: 120, b: 120 },
            });
        }

        labels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer() -> SearchOverlayRenderer {
        SearchOverlayRenderer::new(10.0, 20.0)
    }

    #[test]
    fn test_rects_with_3_results_selected_1() {
        let r = renderer();
        let rects = r.build_overlay_rects(3, 1, 800.0, 600.0);
        // 1 backdrop + 1 search box + 3 result rows = 5
        assert_eq!(rects.len(), 5);

        // Backdrop is full viewport
        assert_eq!(rects[0].pos[2], 800.0); // width
        assert_eq!(rects[0].pos[3], 600.0); // height

        // Selected row (index 1 in results -> rects[3]) has highlight color
        assert_eq!(rects[3].color, [0.15, 0.30, 0.50, 1.0]);
        // Non-selected rows have normal color
        assert_eq!(rects[2].color, [0.12, 0.12, 0.12, 1.0]);
        assert_eq!(rects[4].color, [0.12, 0.12, 0.12, 1.0]);
    }

    #[test]
    fn test_rects_with_0_results() {
        let r = renderer();
        let rects = r.build_overlay_rects(0, 0, 800.0, 600.0);
        // 1 backdrop + 1 search box = 2
        assert_eq!(rects.len(), 2);
    }

    #[test]
    fn test_rects_caps_at_10_visible() {
        let r = renderer();
        let rects = r.build_overlay_rects(15, 0, 800.0, 600.0);
        // 1 backdrop + 1 search box + 10 visible rows = 12
        assert_eq!(rects.len(), 12);
    }

    #[test]
    fn test_text_label_counts() {
        let r = renderer();
        let results = vec![
            ("cmd1".to_string(), Some(0), "1m ago".to_string(), "output1".to_string()),
            ("cmd2".to_string(), Some(1), "2m ago".to_string(), "output2".to_string()),
            ("cmd3".to_string(), None, "3m ago".to_string(), "output3".to_string()),
        ];
        let labels = r.build_overlay_text("test", &results, 1, 800.0, 600.0);
        // 1 query + 3 results * 2 lines = 7
        assert_eq!(labels.len(), 7);
    }

    #[test]
    fn test_text_query_label_content() {
        let r = renderer();
        let labels = r.build_overlay_text("hello", &[], 0, 800.0, 600.0);
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].text, "Search: hello");
        assert_eq!(labels[0].color, Rgb { r: 230, g: 230, b: 230 });
    }

    #[test]
    fn test_text_selected_brighter() {
        let r = renderer();
        let results = vec![
            ("cmd1".to_string(), Some(0), "1m ago".to_string(), "out".to_string()),
            ("cmd2".to_string(), Some(0), "2m ago".to_string(), "out".to_string()),
        ];
        let labels = r.build_overlay_text("q", &results, 0, 800.0, 600.0);
        // labels[1] = cmd1 (selected), labels[3] = cmd2 (not selected)
        assert_eq!(labels[1].color, Rgb { r: 240, g: 240, b: 240 });
        assert_eq!(labels[3].color, Rgb { r: 204, g: 204, b: 204 });
    }

    #[test]
    fn test_text_exit_badge_format() {
        let r = renderer();
        let results = vec![
            ("cmd1".to_string(), Some(0), "1m ago".to_string(), "out".to_string()),
            ("cmd2".to_string(), Some(127), "2m ago".to_string(), "out".to_string()),
            ("cmd3".to_string(), None, "3m ago".to_string(), "out".to_string()),
        ];
        let labels = r.build_overlay_text("q", &results, 0, 800.0, 600.0);
        // Metadata lines are at indices 2, 4, 6
        assert!(labels[2].text.starts_with("OK"));
        assert!(labels[4].text.starts_with("X:127"));
        assert!(labels[6].text.starts_with("OK")); // None exit code -> OK
    }

    #[test]
    fn test_text_caps_at_10_visible() {
        let r = renderer();
        let results: Vec<_> = (0..15)
            .map(|i| (format!("cmd{}", i), Some(0), "1m ago".to_string(), "out".to_string()))
            .collect();
        let labels = r.build_overlay_text("q", &results, 0, 800.0, 600.0);
        // 1 query + 10 * 2 = 21
        assert_eq!(labels.len(), 21);
    }

    #[test]
    fn test_positions_computed_from_viewport() {
        let r = renderer();
        let rects_small = r.build_overlay_rects(1, 0, 400.0, 300.0);
        let rects_large = r.build_overlay_rects(1, 0, 1200.0, 900.0);

        // Backdrop sizes differ
        assert_eq!(rects_small[0].pos[2], 400.0);
        assert_eq!(rects_large[0].pos[2], 1200.0);

        // Input box widths differ (viewport - 2*margin)
        assert!(rects_small[1].pos[2] < rects_large[1].pos[2]);
    }
}

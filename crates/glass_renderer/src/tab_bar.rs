//! TabBarRenderer: top-pinned tab bar with per-tab rects and text labels.
//!
//! Produces background rectangles and text labels for the tab bar
//! that sits at the top of the terminal viewport.

use alacritty_terminal::vte::ansi::Rgb;

use crate::rect_renderer::RectInstance;

/// Information about a tab for rendering purposes.
#[derive(Debug, Clone)]
pub struct TabDisplayInfo {
    /// Title text shown in the tab.
    pub title: String,
    /// Whether this tab is currently active/focused.
    pub is_active: bool,
}

/// Text label for a single tab.
#[derive(Debug, Clone)]
pub struct TabLabel {
    /// Display text (possibly truncated).
    pub text: String,
    /// X position in pixels.
    pub x: f32,
    /// Y position in pixels.
    pub y: f32,
    /// Text color.
    pub color: Rgb,
}

/// Tab bar background color: slightly lighter than terminal bg (26/255).
const BAR_BG_COLOR: [f32; 4] = [30.0 / 255.0, 30.0 / 255.0, 30.0 / 255.0, 1.0];
/// Active tab background color.
const ACTIVE_TAB_COLOR: [f32; 4] = [50.0 / 255.0, 50.0 / 255.0, 50.0 / 255.0, 1.0];
/// Inactive tab background color.
const INACTIVE_TAB_COLOR: [f32; 4] = [35.0 / 255.0, 35.0 / 255.0, 35.0 / 255.0, 1.0];

/// Maximum title length before truncation.
const MAX_TITLE_LEN: usize = 20;

/// Left padding for text within a tab (in pixels).
const TAB_TEXT_PADDING: f32 = 8.0;

/// Gap between tab rects (in pixels).
const TAB_GAP: f32 = 1.0;

/// Renders the top-pinned tab bar.
///
/// Produces background rectangles and text labels for each tab.
/// Follows the same pattern as StatusBarRenderer.
pub struct TabBarRenderer {
    #[allow(dead_code)] // Used in future plans for text centering calculations.
    cell_width: f32,
    cell_height: f32,
}

impl TabBarRenderer {
    /// Create a new TabBarRenderer with the given cell dimensions.
    pub fn new(cell_width: f32, cell_height: f32) -> Self {
        Self { cell_width, cell_height }
    }

    /// Build rectangles for the tab bar.
    ///
    /// Returns:
    /// - First rect: full-width bar background at y=0
    /// - Per-tab rects: equally sized, positioned sequentially with 1px gaps
    pub fn build_tab_rects(&self, tabs: &[TabDisplayInfo], viewport_width: f32) -> Vec<RectInstance> {
        let mut rects = Vec::new();

        // Bar background rect (always present).
        rects.push(RectInstance {
            pos: [0.0, 0.0, viewport_width, self.cell_height],
            color: BAR_BG_COLOR,
        });

        if tabs.is_empty() {
            return rects;
        }

        let tab_count = tabs.len() as f32;
        let tab_width = (viewport_width - TAB_GAP * (tab_count - 1.0).max(0.0)) / tab_count;

        for (i, tab) in tabs.iter().enumerate() {
            let x = i as f32 * (tab_width + TAB_GAP);
            let color = if tab.is_active {
                ACTIVE_TAB_COLOR
            } else {
                INACTIVE_TAB_COLOR
            };
            rects.push(RectInstance {
                pos: [x, 0.0, tab_width, self.cell_height],
                color,
            });
        }

        rects
    }

    /// Build text labels for each tab.
    ///
    /// One TabLabel per tab, left-aligned with padding within the tab rect.
    /// Titles longer than 20 chars are truncated with "..." suffix.
    pub fn build_tab_text(&self, tabs: &[TabDisplayInfo], viewport_width: f32) -> Vec<TabLabel> {
        if tabs.is_empty() {
            return Vec::new();
        }

        let tab_count = tabs.len() as f32;
        let tab_width = (viewport_width - TAB_GAP * (tab_count - 1.0).max(0.0)) / tab_count;

        tabs.iter()
            .enumerate()
            .map(|(i, tab)| {
                let text = truncate_title(&tab.title);
                let x = i as f32 * (tab_width + TAB_GAP) + TAB_TEXT_PADDING;
                let color = if tab.is_active {
                    Rgb { r: 204, g: 204, b: 204 }
                } else {
                    Rgb { r: 140, g: 140, b: 140 }
                };
                TabLabel { text, x, y: 0.0, color }
            })
            .collect()
    }

    /// Returns the height of the tab bar in pixels (one row tall).
    pub fn tab_bar_height(&self) -> f32 {
        self.cell_height
    }

    /// Hit-test: given an x coordinate, return which tab index was clicked.
    ///
    /// Returns `None` if `tab_count` is 0 or x is out of range.
    pub fn hit_test(&self, x: f32, tab_count: usize, viewport_width: f32) -> Option<usize> {
        if tab_count == 0 {
            return None;
        }

        let tab_count_f = tab_count as f32;
        let tab_width = (viewport_width - TAB_GAP * (tab_count_f - 1.0).max(0.0)) / tab_count_f;

        for i in 0..tab_count {
            let tab_x = i as f32 * (tab_width + TAB_GAP);
            if x >= tab_x && x < tab_x + tab_width {
                return Some(i);
            }
        }

        None
    }
}

/// Truncate a title to MAX_TITLE_LEN chars, appending "..." if truncated.
fn truncate_title(title: &str) -> String {
    if title.len() > MAX_TITLE_LEN {
        format!("{}...", &title[..MAX_TITLE_LEN - 3])
    } else {
        title.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tabs(names: &[(&str, bool)]) -> Vec<TabDisplayInfo> {
        names
            .iter()
            .map(|(title, active)| TabDisplayInfo {
                title: title.to_string(),
                is_active: *active,
            })
            .collect()
    }

    #[test]
    fn test_build_tab_rects_single_tab() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = make_tabs(&[("Tab 1", true)]);
        let rects = renderer.build_tab_rects(&tabs, 800.0);
        // Bar background + 1 active tab = 2 rects
        assert_eq!(rects.len(), 2);
        // First rect is bar background
        assert_eq!(rects[0].color, BAR_BG_COLOR);
        assert_eq!(rects[0].pos[2], 800.0); // full width
        // Second rect is active tab
        assert_eq!(rects[1].color, ACTIVE_TAB_COLOR);
    }

    #[test]
    fn test_build_tab_rects_three_tabs_active_distinct() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = make_tabs(&[("Tab 1", false), ("Tab 2", true), ("Tab 3", false)]);
        let rects = renderer.build_tab_rects(&tabs, 800.0);
        // Bar background + 3 tab rects = 4
        assert_eq!(rects.len(), 4);
        // Active tab (index 1 -> rects[2]) has distinct color
        assert_eq!(rects[1].color, INACTIVE_TAB_COLOR);
        assert_eq!(rects[2].color, ACTIVE_TAB_COLOR);
        assert_eq!(rects[3].color, INACTIVE_TAB_COLOR);
    }

    #[test]
    fn test_tab_rects_equal_width() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = make_tabs(&[("A", true), ("B", false), ("C", false)]);
        let rects = renderer.build_tab_rects(&tabs, 800.0);
        // With 3 tabs and 2 gaps of 1px each, tab_width = (800 - 2) / 3
        let expected_width = (800.0 - 2.0) / 3.0;
        for rect in &rects[1..] {
            assert!((rect.pos[2] - expected_width).abs() < 0.01);
        }
    }

    #[test]
    fn test_tab_rects_at_top() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = make_tabs(&[("Tab", true)]);
        let rects = renderer.build_tab_rects(&tabs, 800.0);
        // All rects at y=0 with height=cell_height
        for rect in &rects {
            assert_eq!(rect.pos[1], 0.0);
            assert_eq!(rect.pos[3], 16.0);
        }
    }

    #[test]
    fn test_build_tab_text_positions() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = make_tabs(&[("Tab 1", true), ("Tab 2", false), ("Tab 3", false)]);
        let labels = renderer.build_tab_text(&tabs, 800.0);
        assert_eq!(labels.len(), 3);
        // First tab x starts at TAB_TEXT_PADDING
        assert!((labels[0].x - TAB_TEXT_PADDING).abs() < 0.01);
        // Each subsequent label has increasing x
        assert!(labels[1].x > labels[0].x);
        assert!(labels[2].x > labels[1].x);
    }

    #[test]
    fn test_long_title_truncated() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = make_tabs(&[("This is a very long tab title that exceeds limit", true)]);
        let labels = renderer.build_tab_text(&tabs, 800.0);
        assert_eq!(labels.len(), 1);
        assert!(labels[0].text.len() <= MAX_TITLE_LEN);
        assert!(labels[0].text.ends_with("..."));
    }

    #[test]
    fn test_zero_tabs_only_background() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs: Vec<TabDisplayInfo> = vec![];
        let rects = renderer.build_tab_rects(&tabs, 800.0);
        assert_eq!(rects.len(), 1); // only bar background
        assert_eq!(rects[0].color, BAR_BG_COLOR);
    }

    #[test]
    fn test_hit_test_correct_index() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        // 3 tabs, viewport 800px, tab_width = (800-2)/3 ~= 266
        assert_eq!(renderer.hit_test(10.0, 3, 800.0), Some(0));
        assert_eq!(renderer.hit_test(300.0, 3, 800.0), Some(1));
        assert_eq!(renderer.hit_test(600.0, 3, 800.0), Some(2));
    }

    #[test]
    fn test_hit_test_zero_tabs() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        assert_eq!(renderer.hit_test(10.0, 0, 800.0), None);
    }

    #[test]
    fn test_active_tab_text_color() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = make_tabs(&[("Active", true), ("Inactive", false)]);
        let labels = renderer.build_tab_text(&tabs, 800.0);
        assert_eq!(labels[0].color, Rgb { r: 204, g: 204, b: 204 });
        assert_eq!(labels[1].color, Rgb { r: 140, g: 140, b: 140 });
    }

    #[test]
    fn test_tab_bar_height() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        assert_eq!(renderer.tab_bar_height(), 16.0);
    }
}

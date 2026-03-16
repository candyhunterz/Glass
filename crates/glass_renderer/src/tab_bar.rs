//! TabBarRenderer: top-pinned tab bar with per-tab rects and text labels.
//!
//! Produces background rectangles and text labels for the tab bar
//! that sits at the top of the terminal viewport. Supports variable-width
//! tabs, a "+" new tab button, hover-only "x" close buttons, and
//! comprehensive hit-testing via [`TabHitResult`].

use alacritty_terminal::vte::ansi::Rgb;

use crate::rect_renderer::RectInstance;

/// Information about a tab for rendering purposes.
#[derive(Debug, Clone)]
pub struct TabDisplayInfo {
    /// Title text shown in the tab.
    pub title: String,
    /// Whether this tab is currently active/focused.
    pub is_active: bool,
    /// Whether agents hold file locks in the current project.
    pub has_locks: bool,
    /// Whether this tab was created by the orchestrator agent.
    pub agent_created: bool,
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

/// Result of a tab bar hit-test, distinguishing between different click targets.
#[derive(Debug, Clone, PartialEq)]
pub enum TabHitResult {
    /// Clicked on a tab body (not the close button).
    Tab(usize),
    /// Clicked the close button on the tab at the given index.
    CloseButton(usize),
    /// Clicked the "+" new tab button.
    NewTabButton,
}

/// Tab bar background color: slightly lighter than terminal bg (26/255).
const BAR_BG_COLOR: [f32; 4] = [30.0 / 255.0, 30.0 / 255.0, 30.0 / 255.0, 1.0];
/// Active tab background color.
const ACTIVE_TAB_COLOR: [f32; 4] = [50.0 / 255.0, 50.0 / 255.0, 50.0 / 255.0, 1.0];
/// Inactive tab background color.
const INACTIVE_TAB_COLOR: [f32; 4] = [35.0 / 255.0, 35.0 / 255.0, 35.0 / 255.0, 1.0];

/// Maximum title length before truncation (when close button is not shown).
const MAX_TITLE_LEN: usize = 20;

/// Left padding for text within a tab (in pixels).
const TAB_TEXT_PADDING: f32 = 8.0;

/// Width of the drag-and-drop insertion indicator line.
const DRAG_INDICATOR_WIDTH: f32 = 2.0;

/// Color for the drag-and-drop insertion indicator (blue accent).
const DRAG_INDICATOR_COLOR: [f32; 4] = [0.4, 0.6, 1.0, 1.0];

/// Gap between tab rects (in pixels).
const TAB_GAP: f32 = 1.0;

/// Minimum tab width before overflow (tabs stop shrinking below this).
const MIN_TAB_WIDTH: f32 = 60.0;

/// Width of the "+" new tab button area.
const NEW_TAB_BUTTON_WIDTH: f32 = 32.0;

/// Size of the close button highlight square.
const CLOSE_BUTTON_SIZE: f32 = 16.0;

/// Right padding from tab edge for close button positioning.
const CLOSE_BUTTON_PADDING: f32 = 6.0;

/// Close button hover background color.
const HOVER_HIGHLIGHT_COLOR: [f32; 4] = [70.0 / 255.0, 70.0 / 255.0, 70.0 / 255.0, 1.0];

/// Renders the top-pinned tab bar.
///
/// Produces background rectangles and text labels for each tab,
/// including a "+" new tab button and per-tab "x" close buttons (hover-only).
/// Follows the same pattern as StatusBarRenderer.
pub struct TabBarRenderer {
    cell_width: f32,
    cell_height: f32,
}

impl TabBarRenderer {
    /// Create a new TabBarRenderer with the given cell dimensions.
    pub fn new(cell_width: f32, cell_height: f32) -> Self {
        Self {
            cell_width,
            cell_height,
        }
    }

    /// Compute per-tab width and total tabs width.
    ///
    /// Returns `(tab_width, total_tabs_width)` where:
    /// - `tab_width` is the width of each individual tab (clamped to `MIN_TAB_WIDTH`)
    /// - `total_tabs_width` is `tab_count * tab_width + gaps`
    fn compute_tab_width(&self, tab_count: usize, viewport_width: f32) -> (f32, f32) {
        if tab_count == 0 {
            return (0.0, 0.0);
        }

        let tab_count_f = tab_count as f32;
        let gaps = TAB_GAP * (tab_count_f - 1.0).max(0.0);
        let available = viewport_width - NEW_TAB_BUTTON_WIDTH - gaps;
        let tab_width = (available / tab_count_f).max(MIN_TAB_WIDTH);
        let total_tabs_width = tab_count_f * tab_width + gaps;

        (tab_width, total_tabs_width)
    }

    /// Build rectangles for the tab bar.
    ///
    /// Returns:
    /// - First rect: full-width bar background at y=0
    /// - Per-tab rects: variable-width, positioned sequentially with 1px gaps
    /// - Close button highlight rect for the hovered tab (if any)
    /// - "+" new tab button background rect
    pub fn build_tab_rects(
        &self,
        tabs: &[TabDisplayInfo],
        viewport_width: f32,
        hovered_tab: Option<usize>,
        drop_index: Option<usize>,
    ) -> Vec<RectInstance> {
        let mut rects = Vec::new();

        // Bar background rect (always present).
        rects.push(RectInstance {
            pos: [0.0, 0.0, viewport_width, self.cell_height],
            color: BAR_BG_COLOR,
        });

        if tabs.is_empty() {
            return rects;
        }

        let (tab_width, total_tabs_width) = self.compute_tab_width(tabs.len(), viewport_width);

        // Per-tab rects
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

        // Close button highlight rect (only for hovered tab)
        if let Some(hover_idx) = hovered_tab {
            if hover_idx < tabs.len() {
                let tab_x = hover_idx as f32 * (tab_width + TAB_GAP);
                let close_x = tab_x + tab_width - CLOSE_BUTTON_PADDING - CLOSE_BUTTON_SIZE;
                let close_y = (self.cell_height - CLOSE_BUTTON_SIZE) / 2.0;
                rects.push(RectInstance {
                    pos: [close_x, close_y, CLOSE_BUTTON_SIZE, CLOSE_BUTTON_SIZE],
                    color: HOVER_HIGHLIGHT_COLOR,
                });
            }
        }

        // "+" new tab button background rect (same as bar bg, invisible until hover)
        let plus_x = total_tabs_width;
        rects.push(RectInstance {
            pos: [plus_x, 0.0, NEW_TAB_BUTTON_WIDTH, self.cell_height],
            color: BAR_BG_COLOR,
        });

        // Drag-and-drop insertion indicator
        if let Some(idx) = drop_index {
            let indicator_x =
                idx as f32 * (tab_width + TAB_GAP) - TAB_GAP / 2.0 - DRAG_INDICATOR_WIDTH / 2.0;
            rects.push(RectInstance {
                pos: [
                    indicator_x.max(0.0),
                    0.0,
                    DRAG_INDICATOR_WIDTH,
                    self.cell_height,
                ],
                color: DRAG_INDICATOR_COLOR,
            });
        }

        rects
    }

    /// Build text labels for each tab, plus "+" and optional "x" labels.
    ///
    /// When `hovered_tab` is `Some(i)`, the title for tab `i` is shortened
    /// to make room for an "x" close button glyph.
    pub fn build_tab_text(
        &self,
        tabs: &[TabDisplayInfo],
        viewport_width: f32,
        hovered_tab: Option<usize>,
    ) -> Vec<TabLabel> {
        if tabs.is_empty() {
            return Vec::new();
        }

        let (tab_width, total_tabs_width) = self.compute_tab_width(tabs.len(), viewport_width);
        let mut labels = Vec::new();

        // Compute how many characters the close button takes away
        let close_chars =
            ((CLOSE_BUTTON_SIZE + CLOSE_BUTTON_PADDING) / self.cell_width).ceil() as usize;

        for (i, tab) in tabs.iter().enumerate() {
            let is_hovered = hovered_tab == Some(i);
            let max_len = if is_hovered {
                MAX_TITLE_LEN.saturating_sub(close_chars)
            } else {
                MAX_TITLE_LEN
            };

            let base_title = truncate_title(&tab.title, max_len);
            let text = if tab.has_locks {
                format!("* {}", base_title)
            } else {
                base_title
            };
            let x = i as f32 * (tab_width + TAB_GAP) + TAB_TEXT_PADDING;
            let color = if tab.is_active {
                Rgb {
                    r: 204,
                    g: 204,
                    b: 204,
                }
            } else {
                Rgb {
                    r: 140,
                    g: 140,
                    b: 140,
                }
            };
            labels.push(TabLabel {
                text,
                x,
                y: 0.0,
                color,
            });

            // "x" close button text for hovered tab
            if is_hovered {
                let tab_x = i as f32 * (tab_width + TAB_GAP);
                let close_center_x =
                    tab_x + tab_width - CLOSE_BUTTON_PADDING - CLOSE_BUTTON_SIZE / 2.0;
                // Center the "x" glyph in the close button area
                let glyph_x = close_center_x - self.cell_width / 2.0;
                labels.push(TabLabel {
                    text: "x".to_string(),
                    x: glyph_x,
                    y: 0.0,
                    color: Rgb {
                        r: 180,
                        g: 180,
                        b: 180,
                    },
                });
            }
        }

        // "+" new tab button text label
        let plus_center_x = total_tabs_width + NEW_TAB_BUTTON_WIDTH / 2.0;
        let plus_glyph_x = plus_center_x - self.cell_width / 2.0;
        labels.push(TabLabel {
            text: "+".to_string(),
            x: plus_glyph_x,
            y: 0.0,
            color: Rgb {
                r: 140,
                g: 140,
                b: 140,
            },
        });

        labels
    }

    /// Returns the height of the tab bar in pixels (one row tall).
    pub fn tab_bar_height(&self) -> f32 {
        self.cell_height
    }

    /// Compute the drop slot index for a drag-and-drop operation.
    ///
    /// Given the mouse X position, returns the insertion slot (0..=tab_count)
    /// where a dragged tab should be dropped. The slot is computed by finding
    /// which tab boundary (midpoint) the cursor is closest to.
    pub fn drag_drop_index(&self, x: f32, tab_count: usize, viewport_width: f32) -> usize {
        if tab_count == 0 {
            return 0;
        }
        let (tab_width, _) = self.compute_tab_width(tab_count, viewport_width);
        let slot = ((x / (tab_width + TAB_GAP)) + 0.5) as usize;
        slot.min(tab_count)
    }

    /// Hit-test: given an x coordinate, return what was clicked.
    ///
    /// Checks in order: "+" new tab button, close button sub-rects, tab bodies.
    /// Close button is checked before tab body (critical: close button is a sub-region of the tab).
    pub fn hit_test(&self, x: f32, tab_count: usize, viewport_width: f32) -> Option<TabHitResult> {
        if tab_count == 0 {
            return None;
        }

        let (tab_width, total_tabs_width) = self.compute_tab_width(tab_count, viewport_width);

        // Check "+" new tab button region
        let plus_x = total_tabs_width;
        if x >= plus_x && x < plus_x + NEW_TAB_BUTTON_WIDTH {
            return Some(TabHitResult::NewTabButton);
        }

        // Check each tab (close button first, then body)
        for i in 0..tab_count {
            let tab_x = i as f32 * (tab_width + TAB_GAP);
            let tab_right = tab_x + tab_width;

            if x >= tab_x && x < tab_right {
                // Check close button sub-rect first
                let close_x = tab_right - CLOSE_BUTTON_PADDING - CLOSE_BUTTON_SIZE;
                if x >= close_x && x < close_x + CLOSE_BUTTON_SIZE {
                    return Some(TabHitResult::CloseButton(i));
                }
                return Some(TabHitResult::Tab(i));
            }
        }

        None
    }

    /// Convenience hit-test that returns only the tab index (ignoring close button distinction).
    ///
    /// Used for hover tracking in CursorMoved, where we only need to know which tab
    /// the mouse is over (to show/hide the close button).
    pub fn hit_test_tab_index(
        &self,
        x: f32,
        tab_count: usize,
        viewport_width: f32,
    ) -> Option<usize> {
        if tab_count == 0 {
            return None;
        }

        let (tab_width, _) = self.compute_tab_width(tab_count, viewport_width);

        for i in 0..tab_count {
            let tab_x = i as f32 * (tab_width + TAB_GAP);
            if x >= tab_x && x < tab_x + tab_width {
                return Some(i);
            }
        }

        None
    }
}

/// Truncate a title to `max_len` chars, appending "..." if truncated.
fn truncate_title(title: &str, max_len: usize) -> String {
    if max_len < 4 {
        // Not enough room for any text + "..."
        return title.chars().take(max_len).collect();
    }
    if title.len() > max_len {
        format!("{}...", &title[..max_len - 3])
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
                has_locks: false,
                agent_created: false,
            })
            .collect()
    }

    #[test]
    fn test_build_tab_rects_single_tab() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = make_tabs(&[("Tab 1", true)]);
        let rects = renderer.build_tab_rects(&tabs, 800.0, None, None);
        // Bar background + 1 active tab + "+" button = 3 rects
        assert_eq!(rects.len(), 3);
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
        let rects = renderer.build_tab_rects(&tabs, 800.0, None, None);
        // Bar background + 3 tab rects + "+" button = 5
        assert_eq!(rects.len(), 5);
        // Active tab (index 1 -> rects[2]) has distinct color
        assert_eq!(rects[1].color, INACTIVE_TAB_COLOR);
        assert_eq!(rects[2].color, ACTIVE_TAB_COLOR);
        assert_eq!(rects[3].color, INACTIVE_TAB_COLOR);
    }

    #[test]
    fn test_tab_rects_variable_width() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = make_tabs(&[("A", true), ("B", false), ("C", false)]);
        let rects = renderer.build_tab_rects(&tabs, 800.0, None, None);
        // Tab width = (800 - 32 - 2) / 3 = 255.33...
        let expected_width = (800.0 - NEW_TAB_BUTTON_WIDTH - 2.0) / 3.0;
        for rect in &rects[1..4] {
            assert!((rect.pos[2] - expected_width).abs() < 0.01);
        }
    }

    #[test]
    fn test_tab_rects_at_top() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = make_tabs(&[("Tab", true)]);
        let rects = renderer.build_tab_rects(&tabs, 800.0, None, None);
        // All rects at y=0 with height=cell_height (except close button)
        for rect in &rects {
            assert_eq!(rect.pos[3], 16.0);
        }
    }

    #[test]
    fn test_build_tab_text_positions() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = make_tabs(&[("Tab 1", true), ("Tab 2", false), ("Tab 3", false)]);
        let labels = renderer.build_tab_text(&tabs, 800.0, None);
        // 3 tab labels + 1 "+" label = 4
        assert_eq!(labels.len(), 4);
        // First tab x starts at TAB_TEXT_PADDING
        assert!((labels[0].x - TAB_TEXT_PADDING).abs() < 0.01);
        // Each subsequent tab label has increasing x
        assert!(labels[1].x > labels[0].x);
        assert!(labels[2].x > labels[1].x);
    }

    #[test]
    fn test_long_title_truncated() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = make_tabs(&[("This is a very long tab title that exceeds limit", true)]);
        let labels = renderer.build_tab_text(&tabs, 800.0, None);
        // Tab label + "+" label = 2
        assert_eq!(labels.len(), 2);
        assert!(labels[0].text.len() <= MAX_TITLE_LEN);
        assert!(labels[0].text.ends_with("..."));
    }

    #[test]
    fn test_zero_tabs_only_background() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs: Vec<TabDisplayInfo> = vec![];
        let rects = renderer.build_tab_rects(&tabs, 800.0, None, None);
        assert_eq!(rects.len(), 1); // only bar background
        assert_eq!(rects[0].color, BAR_BG_COLOR);
    }

    #[test]
    fn test_hit_test_correct_index() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        // 3 tabs, viewport 800px
        // tab_width = (800 - 32 - 2) / 3 = 255.33
        let result = renderer.hit_test(10.0, 3, 800.0);
        assert_eq!(result, Some(TabHitResult::Tab(0)));
        let result = renderer.hit_test(300.0, 3, 800.0);
        assert_eq!(result, Some(TabHitResult::Tab(1)));
        let result = renderer.hit_test(600.0, 3, 800.0);
        assert_eq!(result, Some(TabHitResult::Tab(2)));
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
        let labels = renderer.build_tab_text(&tabs, 800.0, None);
        assert_eq!(
            labels[0].color,
            Rgb {
                r: 204,
                g: 204,
                b: 204
            }
        );
        assert_eq!(
            labels[1].color,
            Rgb {
                r: 140,
                g: 140,
                b: 140
            }
        );
    }

    #[test]
    fn test_tab_bar_height() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        assert_eq!(renderer.tab_bar_height(), 16.0);
    }

    #[test]
    fn test_tab_with_locks_shows_indicator() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = vec![TabDisplayInfo {
            title: "Tab 1".to_string(),
            is_active: true,
            has_locks: true,
            agent_created: false,
        }];
        let labels = renderer.build_tab_text(&tabs, 800.0, None);
        // Tab label + "+" label = 2
        assert_eq!(labels.len(), 2);
        assert!(labels[0].text.starts_with("* "));
    }

    #[test]
    fn test_tab_without_locks_no_indicator() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = vec![TabDisplayInfo {
            title: "Tab 1".to_string(),
            is_active: true,
            has_locks: false,
            agent_created: false,
        }];
        let labels = renderer.build_tab_text(&tabs, 800.0, None);
        assert_eq!(labels.len(), 2); // Tab label + "+" label
        assert!(!labels[0].text.starts_with("* "));
        assert_eq!(labels[0].text, "Tab 1");
    }

    // --- New tests for TabHitResult, buttons, and layout ---

    #[test]
    fn test_new_tab_button_position() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = make_tabs(&[("Tab 1", true), ("Tab 2", false)]);
        let rects = renderer.build_tab_rects(&tabs, 800.0, None, None);
        // Last rect should be the "+" button
        let plus_rect = rects.last().unwrap();
        assert_eq!(plus_rect.pos[2], NEW_TAB_BUTTON_WIDTH);
        // "+" button should start right after the last tab
        let (tab_width, total_tabs_width) = renderer.compute_tab_width(2, 800.0);
        let _ = tab_width;
        assert!((plus_rect.pos[0] - total_tabs_width).abs() < 0.01);
    }

    #[test]
    fn test_close_button_hovered_only() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = make_tabs(&[("Tab 1", true), ("Tab 2", false), ("Tab 3", false)]);

        // No hover -> no close button rect
        let rects_none = renderer.build_tab_rects(&tabs, 800.0, None, None);
        // bg + 3 tabs + "+" button = 5
        assert_eq!(rects_none.len(), 5);

        // Hover on tab 1 -> close button rect added
        let rects_hover = renderer.build_tab_rects(&tabs, 800.0, Some(1), None);
        // bg + 3 tabs + close button + "+" button = 6
        assert_eq!(rects_hover.len(), 6);

        // The close button rect should have HOVER_HIGHLIGHT_COLOR
        let close_rect = &rects_hover[4]; // After 3 tab rects + bg
        assert_eq!(close_rect.color, HOVER_HIGHLIGHT_COLOR);
        assert_eq!(close_rect.pos[2], CLOSE_BUTTON_SIZE);
        assert_eq!(close_rect.pos[3], CLOSE_BUTTON_SIZE);
    }

    #[test]
    fn test_hit_new_tab_button() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        // 2 tabs, viewport 800px
        let (_, total_tabs_width) = renderer.compute_tab_width(2, 800.0);
        // Click in the middle of the "+" button area
        let click_x = total_tabs_width + NEW_TAB_BUTTON_WIDTH / 2.0;
        let result = renderer.hit_test(click_x, 2, 800.0);
        assert_eq!(result, Some(TabHitResult::NewTabButton));
    }

    #[test]
    fn test_hit_close_button() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        // 3 tabs, viewport 800px
        let (tab_width, _) = renderer.compute_tab_width(3, 800.0);
        // Close button of tab 1 is at: tab_x + tab_width - CLOSE_BUTTON_PADDING - CLOSE_BUTTON_SIZE
        let tab_x = 1.0 * (tab_width + TAB_GAP);
        let close_x = tab_x + tab_width - CLOSE_BUTTON_PADDING - CLOSE_BUTTON_SIZE;
        let click_x = close_x + CLOSE_BUTTON_SIZE / 2.0; // middle of close button
        let result = renderer.hit_test(click_x, 3, 800.0);
        assert_eq!(result, Some(TabHitResult::CloseButton(1)));
    }

    #[test]
    fn test_hit_tab_body() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        // Click on the left side of tab 1 (not close button area)
        let (tab_width, _) = renderer.compute_tab_width(3, 800.0);
        let tab_x = 1.0 * (tab_width + TAB_GAP);
        let click_x = tab_x + TAB_TEXT_PADDING; // near the start of the tab
        let result = renderer.hit_test(click_x, 3, 800.0);
        assert_eq!(result, Some(TabHitResult::Tab(1)));
    }

    #[test]
    fn test_min_tab_width() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        // With many tabs in a small viewport, tab_width should never go below MIN_TAB_WIDTH
        // viewport=400, 20 tabs: (400 - 32 - 19) / 20 = 17.45 -> should clamp to 60
        let (tab_width, _) = renderer.compute_tab_width(20, 400.0);
        assert!(
            tab_width >= MIN_TAB_WIDTH,
            "tab_width {} should be >= {}",
            tab_width,
            MIN_TAB_WIDTH
        );
        assert!((tab_width - MIN_TAB_WIDTH).abs() < 0.01);
    }

    #[test]
    fn test_title_truncation_with_close() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let long_title = "A medium-length title";
        let tabs = make_tabs(&[(long_title, true)]);

        // Without hover: full truncation limit
        let labels_no_hover = renderer.build_tab_text(&tabs, 800.0, None);
        // With hover: shorter truncation limit
        let labels_hover = renderer.build_tab_text(&tabs, 800.0, Some(0));

        // The hovered title should be shorter (or equal if already short enough)
        assert!(
            labels_hover[0].text.len() <= labels_no_hover[0].text.len(),
            "Hovered title '{}' should be <= non-hovered title '{}'",
            labels_hover[0].text,
            labels_no_hover[0].text
        );
    }

    #[test]
    fn test_existing_hit_test_still_works() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        // Basic tab click still returns correct indices wrapped in TabHitResult::Tab
        let (tab_width, _) = renderer.compute_tab_width(3, 800.0);

        // Click in middle of tab 0
        let result = renderer.hit_test(tab_width / 4.0, 3, 800.0);
        assert_eq!(result, Some(TabHitResult::Tab(0)));

        // Click in middle of tab 2
        let tab2_x = 2.0 * (tab_width + TAB_GAP) + tab_width / 4.0;
        let result = renderer.hit_test(tab2_x, 3, 800.0);
        assert_eq!(result, Some(TabHitResult::Tab(2)));
    }

    #[test]
    fn test_plus_button_text() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = make_tabs(&[("Tab 1", true)]);
        let labels = renderer.build_tab_text(&tabs, 800.0, None);
        // Last label should be the "+" button text
        let plus_label = labels.last().unwrap();
        assert_eq!(plus_label.text, "+");
        // Positioned in the new tab button area (after the tab)
        let (_, total_tabs_width) = renderer.compute_tab_width(1, 800.0);
        assert!(plus_label.x >= total_tabs_width);
        assert!(plus_label.x < total_tabs_width + NEW_TAB_BUTTON_WIDTH);
    }

    #[test]
    fn test_close_button_text() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = make_tabs(&[("Tab 1", true), ("Tab 2", false)]);
        let labels = renderer.build_tab_text(&tabs, 800.0, Some(0));
        // Should have: Tab 1 label, "x" label, Tab 2 label, "+" label = 4
        assert_eq!(labels.len(), 4);
        // The "x" label should be the second one (after tab 0's title)
        assert_eq!(labels[1].text, "x");
    }

    #[test]
    fn test_hit_test_tab_index_convenience() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        // hit_test_tab_index should return just the tab index regardless of close button
        let (tab_width, _) = renderer.compute_tab_width(3, 800.0);

        // Click on close button area of tab 1 should still return Some(1)
        let tab_x = 1.0 * (tab_width + TAB_GAP);
        let close_x = tab_x + tab_width - CLOSE_BUTTON_PADDING - CLOSE_BUTTON_SIZE / 2.0;
        let result = renderer.hit_test_tab_index(close_x, 3, 800.0);
        assert_eq!(result, Some(1));

        // Click on "+" button area should return None
        let (_, total_tabs_width) = renderer.compute_tab_width(3, 800.0);
        let result = renderer.hit_test_tab_index(total_tabs_width + 5.0, 3, 800.0);
        assert_eq!(result, None);
    }

    // ---- Drag-and-drop tests ----

    #[test]
    fn drag_drop_index_at_start() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        // x=0 should return slot 0
        assert_eq!(renderer.drag_drop_index(0.0, 3, 800.0), 0);
    }

    #[test]
    fn drag_drop_index_before_midpoint_tab0() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let (tab_width, _) = renderer.compute_tab_width(3, 800.0);
        // Just before the midpoint of tab 0 -> slot 0
        let x = (tab_width + TAB_GAP) * 0.5 - 1.0;
        assert_eq!(renderer.drag_drop_index(x, 3, 800.0), 0);
    }

    #[test]
    fn drag_drop_index_after_midpoint_tab0() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let (tab_width, _) = renderer.compute_tab_width(3, 800.0);
        // Just past the midpoint of tab 0 -> slot 1
        let x = (tab_width + TAB_GAP) * 0.5 + 1.0;
        assert_eq!(renderer.drag_drop_index(x, 3, 800.0), 1);
    }

    #[test]
    fn drag_drop_index_past_all_tabs() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        // x way past all tabs -> clamp to tab_count
        assert_eq!(renderer.drag_drop_index(9999.0, 3, 800.0), 3);
    }

    #[test]
    fn drag_indicator_present_when_drop_index_some() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = make_tabs(&[("A", true), ("B", false), ("C", false)]);
        let rects = renderer.build_tab_rects(&tabs, 800.0, None, Some(1));
        // Should have: bg + 3 tabs + "+" button + 1 indicator = 6
        assert_eq!(rects.len(), 6);
        // Last rect is the indicator (added after "+" button)
        let indicator = rects.last().unwrap();
        assert_eq!(indicator.color, DRAG_INDICATOR_COLOR);
        assert!((indicator.pos[2] - DRAG_INDICATOR_WIDTH).abs() < 0.01);
    }

    #[test]
    fn drag_indicator_absent_when_drop_index_none() {
        let renderer = TabBarRenderer::new(8.0, 16.0);
        let tabs = make_tabs(&[("A", true), ("B", false), ("C", false)]);
        let rects_none = renderer.build_tab_rects(&tabs, 800.0, None, None);
        // bg + 3 tabs + "+" button = 5 (same as before)
        assert_eq!(rects_none.len(), 5);
    }
}

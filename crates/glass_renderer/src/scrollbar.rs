//! ScrollbarRenderer: produces track + thumb RectInstance quads for terminal pane scrollbars.
//!
//! Each pane gets a vertical scrollbar on its right edge showing the current
//! scroll position within the scrollback buffer. The thumb height is proportional
//! to the visible/total line ratio, and its position maps from `display_offset`.

use crate::rect_renderer::RectInstance;

/// Scrollbar width in pixels (reserved gutter on right edge of each pane).
pub const SCROLLBAR_WIDTH: f32 = 8.0;

/// Minimum thumb height to ensure it's always grabbable.
const MIN_THUMB_HEIGHT: f32 = 20.0;

/// Track background color: barely visible stripe.
const TRACK_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.03];

/// Thumb resting color: subtle dim gray.
const THUMB_COLOR_REST: [f32; 4] = [100.0 / 255.0, 100.0 / 255.0, 100.0 / 255.0, 0.4];

/// Thumb hover/drag color: brighter.
const THUMB_COLOR_ACTIVE: [f32; 4] = [150.0 / 255.0, 150.0 / 255.0, 150.0 / 255.0, 0.7];

/// Terminal scroll state needed for scrollbar rendering and hit-testing.
pub struct ScrollState {
    pub display_offset: usize,
    pub history_size: usize,
    pub screen_lines: usize,
}

/// Result of a scrollbar hit-test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollbarHit {
    /// Mouse is on the thumb (start drag).
    Thumb,
    /// Mouse is above the thumb in the track (page up / scroll toward older history).
    TrackAbove,
    /// Mouse is below the thumb in the track (page down / scroll toward newer content).
    TrackBelow,
}

/// Renders a vertical scrollbar for a terminal pane using RectInstance quads.
///
/// Follows the same pattern as TabBarRenderer: a pure-data renderer that
/// produces GPU rect instances and provides hit-testing methods.
pub struct ScrollbarRenderer {
    width: f32,
}

impl Default for ScrollbarRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollbarRenderer {
    /// Create a new ScrollbarRenderer with default width.
    pub fn new() -> Self {
        Self {
            width: SCROLLBAR_WIDTH,
        }
    }

    /// Returns the scrollbar width in pixels.
    pub fn width(&self) -> f32 {
        self.width
    }

    /// Compute thumb geometry within a track of given height.
    ///
    /// Returns `(thumb_y_offset_within_track, thumb_height)`.
    /// Public so Plan 02 can reuse for drag math.
    pub fn compute_thumb_geometry(
        &self,
        track_height: f32,
        history_size: usize,
        screen_lines: usize,
        display_offset: usize,
    ) -> (f32, f32) {
        let total_lines = history_size + screen_lines;
        if total_lines == 0 {
            return (0.0, track_height);
        }

        let thumb_ratio = screen_lines as f32 / total_lines as f32;
        let thumb_height = (track_height * thumb_ratio)
            .max(MIN_THUMB_HEIGHT)
            .min(track_height);
        let scrollable_track = track_height - thumb_height;

        // display_offset=0 means at bottom (newest), display_offset=history_size means at top (oldest)
        // Visual: top of track = oldest, bottom of track = newest
        // scroll_ratio 1.0 = bottom (offset=0), scroll_ratio 0.0 = top (offset=history_size)
        let scroll_ratio = if history_size > 0 {
            1.0 - (display_offset as f32 / history_size as f32)
        } else {
            1.0
        };
        let thumb_y_offset = scrollable_track * scroll_ratio;

        (thumb_y_offset, thumb_height)
    }

    /// Build track + thumb RectInstance quads for a single pane's scrollbar.
    ///
    /// - `pane_right_x`: right edge of the pane viewport in pixels
    /// - `pane_y`: top edge of the pane viewport in pixels
    /// - `pane_height`: height of the pane viewport in pixels
    /// - `display_offset`: current scroll offset (0=bottom, history_size=top)
    /// - `history_size`: total scrollback lines above the visible screen
    /// - `screen_lines`: number of visible terminal lines
    /// - `is_hovered`: whether the mouse is over this scrollbar
    /// - `is_dragging`: whether the thumb is being dragged
    pub fn build_scrollbar_rects(
        &self,
        pane_right_x: f32,
        pane_y: f32,
        pane_height: f32,
        scroll: &ScrollState,
        is_hovered: bool,
        is_dragging: bool,
    ) -> Vec<RectInstance> {
        let scrollbar_x = pane_right_x - self.width;
        let mut rects = Vec::with_capacity(2);

        // Track background
        rects.push(RectInstance {
            pos: [scrollbar_x, pane_y, self.width, pane_height],
            color: TRACK_COLOR,
        });

        // Thumb
        let (thumb_y_offset, thumb_height) = self.compute_thumb_geometry(
            pane_height,
            scroll.history_size,
            scroll.screen_lines,
            scroll.display_offset,
        );

        let thumb_color = if is_dragging || is_hovered {
            THUMB_COLOR_ACTIVE
        } else {
            THUMB_COLOR_REST
        };

        rects.push(RectInstance {
            pos: [
                scrollbar_x,
                pane_y + thumb_y_offset,
                self.width,
                thumb_height,
            ],
            color: thumb_color,
        });

        rects
    }

    /// Hit-test: determine what part of the scrollbar (if any) the mouse is over.
    ///
    /// Returns `None` if the mouse is outside the scrollbar region.
    pub fn hit_test(
        &self,
        mouse_x: f32,
        mouse_y: f32,
        scrollbar_x: f32,
        viewport_y: f32,
        viewport_height: f32,
        scroll: &ScrollState,
    ) -> Option<ScrollbarHit> {
        // Check x-range
        if mouse_x < scrollbar_x || mouse_x >= scrollbar_x + self.width {
            return None;
        }

        // Check y-range (within track)
        if mouse_y < viewport_y || mouse_y >= viewport_y + viewport_height {
            return None;
        }

        // Determine thumb position
        let (thumb_y_offset, thumb_height) = self.compute_thumb_geometry(
            viewport_height,
            scroll.history_size,
            scroll.screen_lines,
            scroll.display_offset,
        );

        let thumb_top = viewport_y + thumb_y_offset;
        let thumb_bottom = thumb_top + thumb_height;

        if mouse_y >= thumb_top && mouse_y < thumb_bottom {
            Some(ScrollbarHit::Thumb)
        } else if mouse_y < thumb_top {
            Some(ScrollbarHit::TrackAbove)
        } else {
            Some(ScrollbarHit::TrackBelow)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    fn ss(display_offset: usize, history_size: usize, screen_lines: usize) -> ScrollState {
        ScrollState {
            display_offset,
            history_size,
            screen_lines,
        }
    }

    #[test]
    fn build_rects_returns_track_and_thumb() {
        let sb = ScrollbarRenderer::new();
        let rects = sb.build_scrollbar_rects(200.0, 0.0, 480.0, &ss(0, 100, 24), false, false);
        assert_eq!(rects.len(), 2, "should return track + thumb");
    }

    #[test]
    fn track_rect_has_correct_color_and_position() {
        let sb = ScrollbarRenderer::new();
        let rects = sb.build_scrollbar_rects(200.0, 10.0, 480.0, &ss(0, 100, 24), false, false);
        let track = &rects[0];
        assert_eq!(track.color, TRACK_COLOR);
        assert!(approx_eq(track.pos[0], 192.0)); // 200 - 8
        assert!(approx_eq(track.pos[1], 10.0));
        assert!(approx_eq(track.pos[2], SCROLLBAR_WIDTH));
        assert!(approx_eq(track.pos[3], 480.0));
    }

    #[test]
    fn thumb_at_bottom_when_display_offset_zero() {
        let sb = ScrollbarRenderer::new();
        // display_offset=0 means at bottom (newest content), thumb should be near bottom
        let rects = sb.build_scrollbar_rects(200.0, 0.0, 480.0, &ss(0, 100, 24), false, false);
        let thumb = &rects[1];
        let (thumb_y, _thumb_h) = sb.compute_thumb_geometry(480.0, 100, 24, 0);
        // scroll_ratio = 1.0, so thumb_y = scrollable_track * 1.0
        assert!(approx_eq(thumb.pos[1], thumb_y));
        assert!(
            approx_eq(thumb.pos[1] + thumb.pos[3], 480.0),
            "thumb should reach bottom of track"
        );
    }

    #[test]
    fn thumb_at_top_when_display_offset_equals_history_size() {
        let sb = ScrollbarRenderer::new();
        // display_offset=100 (= history_size) means at top (oldest history)
        let rects = sb.build_scrollbar_rects(200.0, 0.0, 480.0, &ss(100, 100, 24), false, false);
        let thumb = &rects[1];
        // scroll_ratio = 0.0, thumb_y_offset = 0
        assert!(
            approx_eq(thumb.pos[1], 0.0),
            "thumb should be at top of track"
        );
    }

    #[test]
    fn thumb_at_middle_when_display_offset_is_half() {
        let sb = ScrollbarRenderer::new();
        let rects = sb.build_scrollbar_rects(200.0, 0.0, 480.0, &ss(50, 100, 24), false, false);
        let thumb = &rects[1];
        let (_thumb_y, thumb_h) = sb.compute_thumb_geometry(480.0, 100, 24, 50);
        let expected_middle = (480.0 - thumb_h) / 2.0;
        assert!(
            approx_eq(thumb.pos[1], expected_middle),
            "thumb should be near middle"
        );
    }

    #[test]
    fn thumb_height_proportional_to_visible_ratio() {
        let sb = ScrollbarRenderer::new();
        let pane_height = 480.0;
        let (_, thumb_h) = sb.compute_thumb_geometry(pane_height, 100, 24, 0);
        // Expected: 480 * 24 / (100 + 24) = 480 * 0.1935 = 92.9
        let expected = pane_height * 24.0 / 124.0;
        assert!(approx_eq(thumb_h, expected));
    }

    #[test]
    fn empty_history_fills_entire_track() {
        let sb = ScrollbarRenderer::new();
        let rects = sb.build_scrollbar_rects(200.0, 0.0, 480.0, &ss(0, 0, 24), false, false);
        let thumb = &rects[1];
        assert!(
            approx_eq(thumb.pos[3], 480.0),
            "thumb should fill entire track when no history"
        );
    }

    #[test]
    fn min_thumb_height_enforced() {
        let sb = ScrollbarRenderer::new();
        let pane_height = 480.0;
        // Very large history: 10000 lines with 24 screen lines
        let (_, thumb_h) = sb.compute_thumb_geometry(pane_height, 10000, 24, 0);
        // 480 * 24/10024 = ~1.15 pixels, should clamp to MIN_THUMB_HEIGHT
        assert!(
            approx_eq(thumb_h, MIN_THUMB_HEIGHT),
            "thumb should be clamped to minimum height"
        );
    }

    #[test]
    fn hover_produces_active_color() {
        let sb = ScrollbarRenderer::new();
        let rects = sb.build_scrollbar_rects(200.0, 0.0, 480.0, &ss(0, 100, 24), true, false);
        let thumb = &rects[1];
        assert_eq!(thumb.color, THUMB_COLOR_ACTIVE);
    }

    #[test]
    fn drag_produces_active_color() {
        let sb = ScrollbarRenderer::new();
        let rects = sb.build_scrollbar_rects(200.0, 0.0, 480.0, &ss(0, 100, 24), false, true);
        let thumb = &rects[1];
        assert_eq!(thumb.color, THUMB_COLOR_ACTIVE);
    }

    #[test]
    fn rest_produces_rest_color() {
        let sb = ScrollbarRenderer::new();
        let rects = sb.build_scrollbar_rects(200.0, 0.0, 480.0, &ss(0, 100, 24), false, false);
        let thumb = &rects[1];
        assert_eq!(thumb.color, THUMB_COLOR_REST);
    }

    #[test]
    fn hit_test_returns_none_outside_x_range() {
        let sb = ScrollbarRenderer::new();
        // scrollbar_x = 192 (200-8), mouse at x=100 is outside
        let result = sb.hit_test(100.0, 240.0, 192.0, 0.0, 480.0, &ss(0, 100, 24));
        assert_eq!(result, None);
    }

    #[test]
    fn hit_test_returns_none_outside_y_range() {
        let sb = ScrollbarRenderer::new();
        let result = sb.hit_test(195.0, 500.0, 192.0, 0.0, 480.0, &ss(0, 100, 24));
        assert_eq!(result, None);
    }

    #[test]
    fn hit_test_returns_thumb_when_on_thumb() {
        let sb = ScrollbarRenderer::new();
        // display_offset=0, thumb is at bottom
        let (thumb_y, thumb_h) = sb.compute_thumb_geometry(480.0, 100, 24, 0);
        let mouse_y = thumb_y + thumb_h / 2.0; // middle of thumb
        let result = sb.hit_test(195.0, mouse_y, 192.0, 0.0, 480.0, &ss(0, 100, 24));
        assert_eq!(result, Some(ScrollbarHit::Thumb));
    }

    #[test]
    fn hit_test_returns_track_above_when_above_thumb() {
        let sb = ScrollbarRenderer::new();
        // display_offset=0, thumb is at bottom. Click near top of track.
        let result = sb.hit_test(195.0, 5.0, 192.0, 0.0, 480.0, &ss(0, 100, 24));
        assert_eq!(result, Some(ScrollbarHit::TrackAbove));
    }

    #[test]
    fn hit_test_returns_track_below_when_below_thumb() {
        let sb = ScrollbarRenderer::new();
        // display_offset=100 (at top), thumb is at top. Click near bottom of track.
        let result = sb.hit_test(195.0, 470.0, 192.0, 0.0, 480.0, &ss(100, 100, 24));
        assert_eq!(result, Some(ScrollbarHit::TrackBelow));
    }

    #[test]
    fn scrollbar_width_constant() {
        assert!(approx_eq(SCROLLBAR_WIDTH, 8.0));
    }

    #[test]
    fn compute_thumb_geometry_zero_total_lines() {
        let sb = ScrollbarRenderer::new();
        let (y, h) = sb.compute_thumb_geometry(480.0, 0, 0, 0);
        assert!(approx_eq(y, 0.0));
        assert!(approx_eq(h, 480.0));
    }
}

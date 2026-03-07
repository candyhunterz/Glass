//! Viewport layout for split pane rendering.

use crate::types::SplitDirection;

/// Rectangle describing a pane's position and size within the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewportLayout {
    /// X offset in pixels.
    pub x: u32,
    /// Y offset in pixels.
    pub y: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Divider gap between split panes in pixels.
pub const DIVIDER_GAP: u32 = 2;

impl ViewportLayout {
    /// Split this rect into two sub-rects along the given direction.
    /// Accounts for a 2px divider gap between the two halves.
    pub fn split(&self, direction: SplitDirection, ratio: f32) -> (ViewportLayout, ViewportLayout) {
        match direction {
            SplitDirection::Horizontal => {
                let usable = self.width.saturating_sub(DIVIDER_GAP);
                let left_w = (usable as f32 * ratio) as u32;
                let right_w = usable - left_w;
                let right_x = self.x + left_w + DIVIDER_GAP;
                (
                    ViewportLayout { x: self.x, y: self.y, width: left_w, height: self.height },
                    ViewportLayout { x: right_x, y: self.y, width: right_w, height: self.height },
                )
            }
            SplitDirection::Vertical => {
                let usable = self.height.saturating_sub(DIVIDER_GAP);
                let top_h = (usable as f32 * ratio) as u32;
                let bottom_h = usable - top_h;
                let bottom_y = self.y + top_h + DIVIDER_GAP;
                (
                    ViewportLayout { x: self.x, y: self.y, width: self.width, height: top_h },
                    ViewportLayout { x: self.x, y: bottom_y, width: self.width, height: bottom_h },
                )
            }
        }
    }

    /// Return the center point of this rect.
    pub fn center(&self) -> (u32, u32) {
        (self.x + self.width / 2, self.y + self.height / 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_horizontal_even() {
        let vp = ViewportLayout { x: 0, y: 0, width: 1000, height: 600 };
        let (left, right) = vp.split(SplitDirection::Horizontal, 0.5);
        assert_eq!(left.width + right.width + DIVIDER_GAP, 1000);
        assert_eq!(left.x, 0);
        assert_eq!(right.x, left.width + DIVIDER_GAP);
        assert_eq!(left.height, 600);
        assert_eq!(right.height, 600);
    }

    #[test]
    fn split_vertical_even() {
        let vp = ViewportLayout { x: 0, y: 0, width: 800, height: 800 };
        let (top, bottom) = vp.split(SplitDirection::Vertical, 0.5);
        assert_eq!(top.height + bottom.height + DIVIDER_GAP, 800);
        assert_eq!(top.y, 0);
        assert_eq!(bottom.y, top.height + DIVIDER_GAP);
        assert_eq!(top.width, 800);
        assert_eq!(bottom.width, 800);
    }

    #[test]
    fn center_calculation() {
        let vp = ViewportLayout { x: 100, y: 200, width: 400, height: 300 };
        assert_eq!(vp.center(), (300, 350));
    }
}

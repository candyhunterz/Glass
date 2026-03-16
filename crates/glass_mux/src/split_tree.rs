//! Split pane binary tree layout engine.

use crate::layout::ViewportLayout;
use crate::types::{FocusDirection, SessionId, SplitDirection};

/// Maximum allowed split tree depth. At depth 8, panes become too small
/// to be usable on typical displays (< 10px). Also prevents unbounded
/// recursion in layout computation.
pub const MAX_SPLIT_DEPTH: u32 = 8;

/// A node in the split pane tree.
///
/// Leaf nodes hold a single session. Split nodes divide space between
/// two children with a configurable ratio.
pub enum SplitNode {
    /// A terminal session occupying the full pane.
    Leaf(SessionId),
    /// A split dividing space between two sub-panes.
    Split {
        /// Direction of the split.
        direction: SplitDirection,
        /// Left (or top) child.
        left: Box<SplitNode>,
        /// Right (or bottom) child.
        right: Box<SplitNode>,
        /// Fraction of space allocated to the left/top child (0.0..1.0).
        ratio: f32,
    },
}

impl SplitNode {
    /// Compute pixel rects for all leaf panes given a container rect.
    pub fn compute_layout(&self, container: &ViewportLayout) -> Vec<(SessionId, ViewportLayout)> {
        match self {
            SplitNode::Leaf(id) => vec![(*id, container.clone())],
            SplitNode::Split {
                direction,
                left,
                right,
                ratio,
            } => {
                let (left_rect, right_rect) = container.split(*direction, *ratio);
                let mut result = left.compute_layout(&left_rect);
                result.extend(right.compute_layout(&right_rect));
                result
            }
        }
    }

    /// Remove a leaf by session_id. Returns the modified tree, or None if the
    /// removed leaf was the only node (i.e., the tree is now empty).
    pub fn remove_leaf(self, target: SessionId) -> Option<SplitNode> {
        match self {
            SplitNode::Leaf(id) if id == target => None,
            SplitNode::Leaf(_) => Some(self),
            SplitNode::Split {
                direction,
                left,
                right,
                ratio,
            } => {
                let new_left = left.remove_leaf(target);
                let new_right = right.remove_leaf(target);
                match (new_left, new_right) {
                    (None, Some(surviving)) | (Some(surviving), None) => Some(surviving),
                    (Some(l), Some(r)) => Some(SplitNode::Split {
                        direction,
                        left: Box::new(l),
                        right: Box::new(r),
                        ratio,
                    }),
                    (None, None) => None,
                }
            }
        }
    }

    /// Find the neighbor of `current` in the given `direction`.
    /// Uses layout computation to determine spatial relationships.
    /// Returns None if no neighbor exists in that direction.
    pub fn find_neighbor(
        &self,
        current: SessionId,
        direction: FocusDirection,
        container: &ViewportLayout,
    ) -> Option<SessionId> {
        let layouts = self.compute_layout(container);
        let current_rect = layouts
            .iter()
            .find(|(id, _)| *id == current)
            .map(|(_, vp)| vp)?;

        let (cx, cy) = current_rect.center();
        layouts
            .iter()
            .filter(|(id, _)| *id != current)
            .filter(|(_, vp)| {
                let (nx, ny) = vp.center();
                match direction {
                    FocusDirection::Left => nx < cx,
                    FocusDirection::Right => nx > cx,
                    FocusDirection::Up => ny < cy,
                    FocusDirection::Down => ny > cy,
                }
            })
            .min_by_key(|(_, vp)| {
                let (nx, ny) = vp.center();
                let dx = (nx as i32 - cx as i32).unsigned_abs();
                let dy = (ny as i32 - cy as i32).unsigned_abs();
                dx + dy
            })
            .map(|(id, _)| *id)
    }

    /// Adjust the ratio of the nearest ancestor Split of `focused` in the given
    /// `direction`. Clamps ratio to 0.1..=0.9. No-op on Leaf nodes.
    pub fn resize_ratio(&mut self, focused: SessionId, direction: SplitDirection, delta: f32) {
        match self {
            SplitNode::Leaf(_) => {} // no-op
            SplitNode::Split {
                direction: d,
                left,
                right,
                ratio,
            } => {
                if *d == direction {
                    // Check if focused is in this split's subtree
                    if left.contains(focused) || right.contains(focused) {
                        *ratio = (*ratio + delta).clamp(0.1, 0.9);
                        return;
                    }
                }
                // Recurse into children
                left.resize_ratio(focused, direction, delta);
                right.resize_ratio(focused, direction, delta);
            }
        }
    }

    /// Count the number of leaf panes in this tree.
    pub fn leaf_count(&self) -> usize {
        match self {
            SplitNode::Leaf(_) => 1,
            SplitNode::Split { left, right, .. } => left.leaf_count() + right.leaf_count(),
        }
    }

    /// Collect all leaf session IDs in this tree (left-to-right order).
    pub fn session_ids(&self) -> Vec<SessionId> {
        match self {
            SplitNode::Leaf(id) => vec![*id],
            SplitNode::Split { left, right, .. } => {
                let mut ids = left.session_ids();
                ids.extend(right.session_ids());
                ids
            }
        }
    }

    /// Return the first (leftmost/topmost) leaf session ID.
    pub fn first_leaf(&self) -> SessionId {
        match self {
            SplitNode::Leaf(id) => *id,
            SplitNode::Split { left, .. } => left.first_leaf(),
        }
    }

    /// Replace the leaf matching `target` with a Split node containing the
    /// old leaf on the left and a new leaf (`new_id`) on the right.
    /// Returns true if the replacement was performed.
    pub fn split_leaf(
        &mut self,
        target: SessionId,
        direction: SplitDirection,
        new_id: SessionId,
    ) -> bool {
        match self {
            SplitNode::Leaf(id) if *id == target => {
                let old_id = *id;
                *self = SplitNode::Split {
                    direction,
                    left: Box::new(SplitNode::Leaf(old_id)),
                    right: Box::new(SplitNode::Leaf(new_id)),
                    ratio: 0.5,
                };
                true
            }
            SplitNode::Leaf(_) => false,
            SplitNode::Split { left, right, .. } => {
                left.split_leaf(target, direction, new_id)
                    || right.split_leaf(target, direction, new_id)
            }
        }
    }

    /// Return the maximum depth of this tree (leaf = 0, one split = 1, etc.).
    pub fn depth(&self) -> u32 {
        match self {
            SplitNode::Leaf(_) => 0,
            SplitNode::Split { left, right, .. } => 1 + left.depth().max(right.depth()),
        }
    }

    /// Check if this tree contains a leaf with the given session ID.
    pub fn contains(&self, id: SessionId) -> bool {
        match self {
            SplitNode::Leaf(leaf_id) => *leaf_id == id,
            SplitNode::Split { left, right, .. } => left.contains(id) || right.contains(id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::DIVIDER_GAP;

    fn container() -> ViewportLayout {
        ViewportLayout {
            x: 0,
            y: 0,
            width: 1000,
            height: 800,
        }
    }

    fn sid(n: u64) -> SessionId {
        SessionId::new(n)
    }

    // ---- SPLIT-01: Tree construction ----

    #[test]
    fn leaf_construction() {
        let node = SplitNode::Leaf(sid(1));
        assert_eq!(node.leaf_count(), 1);
        assert!(node.contains(sid(1)));
        assert!(!node.contains(sid(2)));
    }

    #[test]
    fn split_construction() {
        let node = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Leaf(sid(2))),
            ratio: 0.5,
        };
        assert_eq!(node.leaf_count(), 2);
        assert!(node.contains(sid(1)));
        assert!(node.contains(sid(2)));
    }

    #[test]
    fn nested_split_construction() {
        // 3 levels deep
        let inner = SplitNode::Split {
            direction: SplitDirection::Vertical,
            left: Box::new(SplitNode::Leaf(sid(2))),
            right: Box::new(SplitNode::Leaf(sid(3))),
            ratio: 0.5,
        };
        let root = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(inner),
            ratio: 0.5,
        };
        assert_eq!(root.leaf_count(), 3);
        assert!(root.contains(sid(1)));
        assert!(root.contains(sid(2)));
        assert!(root.contains(sid(3)));
    }

    // ---- SPLIT-02: compute_layout ----

    #[test]
    fn layout_leaf_returns_full_container() {
        let node = SplitNode::Leaf(sid(1));
        let layouts = node.compute_layout(&container());
        assert_eq!(layouts.len(), 1);
        assert_eq!(layouts[0].0, sid(1));
        assert_eq!(layouts[0].1, container());
    }

    #[test]
    fn layout_horizontal_split_two_rects() {
        let node = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Leaf(sid(2))),
            ratio: 0.5,
        };
        let layouts = node.compute_layout(&container());
        assert_eq!(layouts.len(), 2);
        let (_, left) = &layouts[0];
        let (_, right) = &layouts[1];
        // Widths + gap = container width
        assert_eq!(left.width + right.width + DIVIDER_GAP, 1000);
        // Both have full height
        assert_eq!(left.height, 800);
        assert_eq!(right.height, 800);
    }

    #[test]
    fn layout_vertical_split_two_rects() {
        let node = SplitNode::Split {
            direction: SplitDirection::Vertical,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Leaf(sid(2))),
            ratio: 0.5,
        };
        let layouts = node.compute_layout(&container());
        assert_eq!(layouts.len(), 2);
        let (_, top) = &layouts[0];
        let (_, bottom) = &layouts[1];
        // Heights + gap = container height
        assert_eq!(top.height + bottom.height + DIVIDER_GAP, 800);
        // Both have full width
        assert_eq!(top.width, 1000);
        assert_eq!(bottom.width, 1000);
    }

    #[test]
    fn layout_nested_correct_rects() {
        // Horizontal split: left leaf, right is vertical split
        let inner = SplitNode::Split {
            direction: SplitDirection::Vertical,
            left: Box::new(SplitNode::Leaf(sid(2))),
            right: Box::new(SplitNode::Leaf(sid(3))),
            ratio: 0.5,
        };
        let root = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(inner),
            ratio: 0.5,
        };
        let layouts = root.compute_layout(&container());
        assert_eq!(layouts.len(), 3);
        // All rects have non-zero dimensions
        for (_, vp) in &layouts {
            assert!(vp.width > 0, "width must be > 0");
            assert!(vp.height > 0, "height must be > 0");
        }
    }

    #[test]
    fn layout_all_rects_nonzero() {
        // 4-pane grid
        let left_split = SplitNode::Split {
            direction: SplitDirection::Vertical,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Leaf(sid(2))),
            ratio: 0.5,
        };
        let right_split = SplitNode::Split {
            direction: SplitDirection::Vertical,
            left: Box::new(SplitNode::Leaf(sid(3))),
            right: Box::new(SplitNode::Leaf(sid(4))),
            ratio: 0.5,
        };
        let root = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(left_split),
            right: Box::new(right_split),
            ratio: 0.5,
        };
        let layouts = root.compute_layout(&container());
        assert_eq!(layouts.len(), 4);
        for (_, vp) in &layouts {
            assert!(vp.width > 0);
            assert!(vp.height > 0);
        }
    }

    // ---- SPLIT-03: Horizontal split gap accounting ----

    #[test]
    fn horizontal_gap_1000_half() {
        let c = ViewportLayout {
            x: 0,
            y: 0,
            width: 1000,
            height: 600,
        };
        let node = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Leaf(sid(2))),
            ratio: 0.5,
        };
        let layouts = node.compute_layout(&c);
        let (_, left) = &layouts[0];
        let (_, right) = &layouts[1];
        assert_eq!(left.x, 0);
        assert_eq!(right.x, left.width + DIVIDER_GAP);
        assert_eq!(left.width + right.width + DIVIDER_GAP, 1000);
        // Each side should be 499px for 1000px container at 0.5
        assert_eq!(left.width, 499);
        assert_eq!(right.width, 499);
    }

    #[test]
    fn horizontal_gap_100_ratio_03() {
        let c = ViewportLayout {
            x: 0,
            y: 0,
            width: 100,
            height: 50,
        };
        let node = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Leaf(sid(2))),
            ratio: 0.3,
        };
        let layouts = node.compute_layout(&c);
        let (_, left) = &layouts[0];
        let (_, right) = &layouts[1];
        assert_eq!(left.x, 0);
        assert_eq!(right.x, left.width + DIVIDER_GAP);
        assert_eq!(left.width + right.width + DIVIDER_GAP, 100);
        assert!(left.width > 0);
        assert!(right.width > 0);
    }

    // ---- SPLIT-04: Vertical split gap accounting ----

    #[test]
    fn vertical_gap_800_half() {
        let c = ViewportLayout {
            x: 0,
            y: 0,
            width: 600,
            height: 800,
        };
        let node = SplitNode::Split {
            direction: SplitDirection::Vertical,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Leaf(sid(2))),
            ratio: 0.5,
        };
        let layouts = node.compute_layout(&c);
        let (_, top) = &layouts[0];
        let (_, bottom) = &layouts[1];
        assert_eq!(top.y, 0);
        assert_eq!(bottom.y, top.height + DIVIDER_GAP);
        assert_eq!(top.height + bottom.height + DIVIDER_GAP, 800);
        assert_eq!(top.height, 399);
        assert_eq!(bottom.height, 399);
    }

    // ---- SPLIT-05: remove_leaf ----

    #[test]
    fn remove_only_leaf_returns_none() {
        let node = SplitNode::Leaf(sid(1));
        assert!(node.remove_leaf(sid(1)).is_none());
    }

    #[test]
    fn remove_left_child_collapses_to_right() {
        let node = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Leaf(sid(2))),
            ratio: 0.5,
        };
        let result = node.remove_leaf(sid(1)).unwrap();
        assert_eq!(result.leaf_count(), 1);
        assert!(result.contains(sid(2)));
        assert!(!result.contains(sid(1)));
    }

    #[test]
    fn remove_right_child_collapses_to_left() {
        let node = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Leaf(sid(2))),
            ratio: 0.5,
        };
        let result = node.remove_leaf(sid(2)).unwrap();
        assert_eq!(result.leaf_count(), 1);
        assert!(result.contains(sid(1)));
    }

    #[test]
    fn remove_nested_leaf_collapses_immediate_parent() {
        let inner = SplitNode::Split {
            direction: SplitDirection::Vertical,
            left: Box::new(SplitNode::Leaf(sid(2))),
            right: Box::new(SplitNode::Leaf(sid(3))),
            ratio: 0.5,
        };
        let root = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(inner),
            ratio: 0.5,
        };
        // Remove sid(2) -- inner split collapses to sid(3)
        let result = root.remove_leaf(sid(2)).unwrap();
        assert_eq!(result.leaf_count(), 2);
        assert!(result.contains(sid(1)));
        assert!(result.contains(sid(3)));
        assert!(!result.contains(sid(2)));
    }

    #[test]
    fn remove_nonexistent_returns_unchanged() {
        let node = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Leaf(sid(2))),
            ratio: 0.5,
        };
        let result = node.remove_leaf(sid(99)).unwrap();
        assert_eq!(result.leaf_count(), 2);
        assert!(result.contains(sid(1)));
        assert!(result.contains(sid(2)));
    }

    // ---- SPLIT-06: find_neighbor ----

    #[test]
    fn horizontal_neighbor_left_right() {
        let node = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Leaf(sid(2))),
            ratio: 0.5,
        };
        let c = container();
        // Left pane has Right neighbor
        assert_eq!(
            node.find_neighbor(sid(1), FocusDirection::Right, &c),
            Some(sid(2))
        );
        // Right pane has Left neighbor
        assert_eq!(
            node.find_neighbor(sid(2), FocusDirection::Left, &c),
            Some(sid(1))
        );
    }

    #[test]
    fn vertical_neighbor_up_down() {
        let node = SplitNode::Split {
            direction: SplitDirection::Vertical,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Leaf(sid(2))),
            ratio: 0.5,
        };
        let c = container();
        // Top pane has Down neighbor
        assert_eq!(
            node.find_neighbor(sid(1), FocusDirection::Down, &c),
            Some(sid(2))
        );
        // Bottom pane has Up neighbor
        assert_eq!(
            node.find_neighbor(sid(2), FocusDirection::Up, &c),
            Some(sid(1))
        );
    }

    #[test]
    fn no_neighbor_in_direction() {
        let node = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Leaf(sid(2))),
            ratio: 0.5,
        };
        let c = container();
        // Left pane has no Left neighbor
        assert_eq!(node.find_neighbor(sid(1), FocusDirection::Left, &c), None);
        // Right pane has no Right neighbor
        assert_eq!(node.find_neighbor(sid(2), FocusDirection::Right, &c), None);
    }

    #[test]
    fn nested_find_neighbor_across_splits() {
        // Layout: [sid(1)] | [sid(2) / sid(3)]
        let inner = SplitNode::Split {
            direction: SplitDirection::Vertical,
            left: Box::new(SplitNode::Leaf(sid(2))),
            right: Box::new(SplitNode::Leaf(sid(3))),
            ratio: 0.5,
        };
        let root = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(inner),
            ratio: 0.5,
        };
        let c = container();
        // sid(1) going Right should find either sid(2) or sid(3) (closest by Manhattan)
        let right_neighbor = root.find_neighbor(sid(1), FocusDirection::Right, &c);
        assert!(right_neighbor == Some(sid(2)) || right_neighbor == Some(sid(3)));
        // sid(2) going Left should find sid(1)
        assert_eq!(
            root.find_neighbor(sid(2), FocusDirection::Left, &c),
            Some(sid(1))
        );
        // sid(3) going Left should find sid(1)
        assert_eq!(
            root.find_neighbor(sid(3), FocusDirection::Left, &c),
            Some(sid(1))
        );
        // sid(2) going Down should find sid(3)
        assert_eq!(
            root.find_neighbor(sid(2), FocusDirection::Down, &c),
            Some(sid(3))
        );
        // sid(3) going Up should find sid(2)
        assert_eq!(
            root.find_neighbor(sid(3), FocusDirection::Up, &c),
            Some(sid(2))
        );
    }

    // ---- SPLIT-07: resize_ratio ----

    #[test]
    fn resize_ratio_adjusts() {
        let mut node = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Leaf(sid(2))),
            ratio: 0.5,
        };
        node.resize_ratio(sid(1), SplitDirection::Horizontal, 0.05);
        if let SplitNode::Split { ratio, .. } = &node {
            assert!((ratio - 0.55).abs() < 0.001);
        } else {
            panic!("expected Split");
        }
    }

    #[test]
    fn resize_ratio_clamps_max() {
        let mut node = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Leaf(sid(2))),
            ratio: 0.88,
        };
        node.resize_ratio(sid(1), SplitDirection::Horizontal, 0.05);
        if let SplitNode::Split { ratio, .. } = &node {
            assert!(*ratio <= 0.9);
        } else {
            panic!("expected Split");
        }
    }

    #[test]
    fn resize_ratio_clamps_min() {
        let mut node = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Leaf(sid(2))),
            ratio: 0.12,
        };
        node.resize_ratio(sid(1), SplitDirection::Horizontal, -0.05);
        if let SplitNode::Split { ratio, .. } = &node {
            assert!(*ratio >= 0.1);
        } else {
            panic!("expected Split");
        }
    }

    #[test]
    fn resize_ratio_on_leaf_is_noop() {
        let mut node = SplitNode::Leaf(sid(1));
        node.resize_ratio(sid(1), SplitDirection::Horizontal, 0.1);
        // Should still be a leaf
        assert!(node.contains(sid(1)));
        assert_eq!(node.leaf_count(), 1);
    }

    #[test]
    fn resize_ratio_wrong_direction_noop() {
        let mut node = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Leaf(sid(2))),
            ratio: 0.5,
        };
        // Resize in Vertical direction on a Horizontal split -- no matching ancestor
        node.resize_ratio(sid(1), SplitDirection::Vertical, 0.1);
        if let SplitNode::Split { ratio, .. } = &node {
            assert!((ratio - 0.5).abs() < 0.001, "ratio should be unchanged");
        }
    }

    // ---- SPLIT-08: depth and deep split trees ----

    #[test]
    fn leaf_depth_is_zero() {
        let node = SplitNode::Leaf(sid(1));
        assert_eq!(node.depth(), 0);
    }

    #[test]
    fn single_split_depth_is_one() {
        let node = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Leaf(sid(2))),
            ratio: 0.5,
        };
        assert_eq!(node.depth(), 1);
    }

    #[test]
    fn deep_split_tree_depth() {
        // Build a chain of 10 levels deep (always split the rightmost leaf)
        let mut root = SplitNode::Leaf(sid(1));
        for i in 2..=11u64 {
            root = SplitNode::Split {
                direction: SplitDirection::Horizontal,
                left: Box::new(root),
                right: Box::new(SplitNode::Leaf(sid(i))),
                ratio: 0.5,
            };
        }
        assert_eq!(root.depth(), 10);
        assert_eq!(root.leaf_count(), 11);
    }

    /// Deep split trees produce non-negative dimensions for all panes.
    /// Some panes will have 0-width at extreme depths — this verifies no panics.
    #[test]
    fn deep_split_tree_layout_no_panic() {
        // Build a 10-level deep split tree (all horizontal)
        let mut root = SplitNode::Leaf(sid(1));
        for i in 2..=11u64 {
            root = SplitNode::Split {
                direction: SplitDirection::Horizontal,
                left: Box::new(root),
                right: Box::new(SplitNode::Leaf(sid(i))),
                ratio: 0.5,
            };
        }
        let c = container(); // 1000x800
        let layouts = root.compute_layout(&c);
        assert_eq!(layouts.len(), 11);
        // All dimensions must be non-negative (no underflow)
        for (_, vp) in &layouts {
            // u32 can't be negative, so we just verify they exist
            let _ = vp.width;
            let _ = vp.height;
        }
    }

    /// Alternating H/V splits should produce non-zero dimensions for reasonable depths.
    #[test]
    fn alternating_split_directions_layout() {
        let mut root = SplitNode::Leaf(sid(1));
        for i in 2..=9u64 {
            let dir = if i % 2 == 0 {
                SplitDirection::Horizontal
            } else {
                SplitDirection::Vertical
            };
            root = SplitNode::Split {
                direction: dir,
                left: Box::new(root),
                right: Box::new(SplitNode::Leaf(sid(i))),
                ratio: 0.5,
            };
        }
        assert_eq!(root.depth(), 8);
        let c = container(); // 1000x800
        let layouts = root.compute_layout(&c);
        assert_eq!(layouts.len(), 9);
    }

    #[test]
    fn resize_specific_nested_split() {
        // Outer horizontal, inner vertical on right side
        let mut root = SplitNode::Split {
            direction: SplitDirection::Horizontal,
            left: Box::new(SplitNode::Leaf(sid(1))),
            right: Box::new(SplitNode::Split {
                direction: SplitDirection::Vertical,
                left: Box::new(SplitNode::Leaf(sid(2))),
                right: Box::new(SplitNode::Leaf(sid(3))),
                ratio: 0.5,
            }),
            ratio: 0.5,
        };
        // Resize vertical split by focusing sid(2) in vertical direction
        root.resize_ratio(sid(2), SplitDirection::Vertical, 0.1);
        // Inner ratio should have changed
        if let SplitNode::Split {
            right,
            ratio: outer_ratio,
            ..
        } = &root
        {
            assert!((outer_ratio - 0.5).abs() < 0.001, "outer ratio unchanged");
            if let SplitNode::Split {
                ratio: inner_ratio, ..
            } = right.as_ref()
            {
                assert!(
                    (inner_ratio - 0.6).abs() < 0.001,
                    "inner ratio should be 0.6"
                );
            } else {
                panic!("expected inner Split");
            }
        } else {
            panic!("expected outer Split");
        }
    }
}

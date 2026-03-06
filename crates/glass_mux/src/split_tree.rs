//! Split pane tree (stub for Phase 24).

use crate::types::{SessionId, SplitDirection};

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

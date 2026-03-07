# Phase 24: Split Panes - Research

**Researched:** 2026-03-06
**Domain:** Binary tree layout, wgpu scissor-rect clipping, glyphon TextBounds, PTY multiplexing
**Confidence:** HIGH

## Summary

Phase 24 adds split pane support within tabs. The existing codebase already has stub types (`SplitNode`, `SplitDirection`, `FocusDirection`, `ViewportLayout`) in `glass_mux` ready for implementation. The core challenge is threefold: (1) a binary tree layout engine that computes pixel rectangles from a split hierarchy and a container rect, (2) a per-pane rendering pipeline using wgpu `set_scissor_rect` and glyphon `TextBounds` to clip each pane's terminal output to its viewport sub-region, and (3) routing input (keyboard, mouse, PTY resize) to the correct pane based on focus state.

The existing `Tab` struct holds a single `session_id`. This must be replaced with (or augmented by) a `SplitNode` tree root, where leaf nodes hold `SessionId` values. The `SessionMux` already stores sessions in a flat `HashMap<SessionId, Session>`, so adding multiple sessions per tab requires no structural change to session storage -- only the tab-to-session mapping changes from 1:1 to 1:N via the tree.

**Primary recommendation:** Implement in three waves: (1) SplitTree data structure + layout engine with extensive unit tests, (2) viewport rendering with scissor clipping + pane dividers, (3) input routing (keyboard focus, mouse click, resize propagation) and pane lifecycle (split, close, collapse).

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| wgpu | 28.0.0 | GPU rendering, scissor rect clipping | Already in use; `set_scissor_rect` is the standard approach for sub-viewport rendering |
| glyphon | 0.10.0 | Text rendering with TextBounds clipping | Already in use; TextBounds provides pixel-level text clipping per pane |
| alacritty_terminal | workspace | Terminal grid state per pane | Already in use; each pane gets its own Term instance |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| bytemuck | workspace | RectInstance for divider/border rects | Already in use for rect rendering |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Binary tree (Box<SplitNode>) | Arena-allocated tree | Arena is overkill for typical split depth (3-4 levels); Box is simpler and sufficient |
| wgpu scissor rect | Multiple render passes per pane | Scissor is simpler, single-pass for rects; text needs per-pane TextBounds anyway |

## Architecture Patterns

### Recommended Project Structure
```
crates/glass_mux/src/
  split_tree.rs        # SplitNode enum + SplitTree methods (layout, navigate, resize, split, close)
  tab.rs               # Tab struct updated: session_id replaced with split_root + focused_pane_id
  session_mux.rs       # Updated: focused_session uses tab's focused pane, split/close pane methods
  types.rs             # Already has SplitDirection, FocusDirection (no changes needed)
  layout.rs            # ViewportLayout already defined (no changes needed)

crates/glass_renderer/src/
  frame.rs             # New: draw_pane_frame() or draw_frame_viewport() for per-pane rendering

src/main.rs            # Updated: render loop iterates panes, input routing via focus
```

### Pattern 1: Binary Tree Layout Engine
**What:** `SplitNode` is already defined as `Leaf(SessionId) | Split { direction, left, right, ratio }`. The layout engine takes a root `SplitNode` and a container `ViewportLayout` and recursively computes pixel rects for every leaf.
**When to use:** Every resize, every split/close operation, and on initial tab creation.
**Example:**
```rust
impl SplitNode {
    /// Compute pixel rects for all leaf panes given a container rect.
    /// Returns Vec<(SessionId, ViewportLayout)>.
    pub fn compute_layout(&self, container: &ViewportLayout) -> Vec<(SessionId, ViewportLayout)> {
        match self {
            SplitNode::Leaf(id) => vec![(*id, container.clone())],
            SplitNode::Split { direction, left, right, ratio } => {
                let (left_rect, right_rect) = container.split(*direction, *ratio);
                let mut result = left.compute_layout(&left_rect);
                result.extend(right.compute_layout(&right_rect));
                result
            }
        }
    }
}

impl ViewportLayout {
    /// Split this rect into two sub-rects along the given direction.
    /// Accounts for a divider gap (e.g., 2px).
    pub fn split(&self, direction: SplitDirection, ratio: f32) -> (ViewportLayout, ViewportLayout) {
        let gap = 2; // divider width in pixels
        match direction {
            SplitDirection::Horizontal => {
                let left_w = ((self.width as f32 * ratio) as u32).saturating_sub(gap / 2);
                let right_x = self.x + left_w + gap;
                let right_w = self.width.saturating_sub(left_w + gap);
                (
                    ViewportLayout { x: self.x, y: self.y, width: left_w, height: self.height },
                    ViewportLayout { x: right_x, y: self.y, width: right_w, height: self.height },
                )
            }
            SplitDirection::Vertical => {
                let top_h = ((self.height as f32 * ratio) as u32).saturating_sub(gap / 2);
                let bottom_y = self.y + top_h + gap;
                let bottom_h = self.height.saturating_sub(top_h + gap);
                (
                    ViewportLayout { x: self.x, y: self.y, width: self.width, height: top_h },
                    ViewportLayout { x: self.x, y: bottom_y, width: self.width, height: bottom_h },
                )
            }
        }
    }
}
```

### Pattern 2: Scissor-Clipped Pane Rendering
**What:** For each leaf pane, set a scissor rect on the render pass and configure TextBounds on glyphon TextAreas to clip text to the pane's pixel region. The GridRenderer must be parameterized with a viewport offset so cell positions are computed relative to the pane's origin.
**When to use:** Every frame, for every visible pane in the active tab.
**Example:**
```rust
// In FrameRenderer -- new method for split pane rendering
pub fn draw_pane(
    &mut self,
    pass: &mut wgpu::RenderPass,
    viewport: &ViewportLayout,
    snapshot: &GridSnapshot,
    // ... other per-pane data
) {
    // Clip all rendering to this pane's rect
    pass.set_scissor_rect(viewport.x, viewport.y, viewport.width, viewport.height);

    // Grid renderer builds rects with pane-local offsets
    // TextAreas use TextBounds matching the pane rect
    let text_bounds = TextBounds {
        left: viewport.x as i32,
        top: viewport.y as i32,
        right: (viewport.x + viewport.width) as i32,
        bottom: (viewport.y + viewport.height) as i32,
    };
    // ... render grid content within these bounds
}
```

### Pattern 3: Focus-Based Input Routing
**What:** Each tab tracks which pane (leaf SessionId) has focus. Keyboard input goes to the focused pane's PTY. Mouse clicks change focus based on which pane's rect contains the click position.
**When to use:** Every keyboard event, every mouse click.
**Example:**
```rust
// Tab now holds the split tree root and focused pane
pub struct Tab {
    pub id: TabId,
    pub root: SplitNode,
    pub focused_pane: SessionId,
    pub title: String,
}

// Mouse click routes to pane by position
fn pane_at_position(layouts: &[(SessionId, ViewportLayout)], x: f32, y: f32) -> Option<SessionId> {
    layouts.iter().find(|(_, vp)| {
        x >= vp.x as f32 && x < (vp.x + vp.width) as f32
        && y >= vp.y as f32 && y < (vp.y + vp.height) as f32
    }).map(|(id, _)| *id)
}
```

### Pattern 4: Pane Close with Parent Collapse
**What:** When a pane is closed, its leaf is removed from the tree. If the parent was a Split node, the sibling becomes the parent's replacement (the Split node collapses to its surviving child).
**When to use:** On pane close (Ctrl+Shift+W closes focused pane).
**Example:**
```rust
impl SplitNode {
    /// Remove a leaf by session_id. Returns the modified tree, or None if this was the removed leaf.
    pub fn remove_leaf(self, target: SessionId) -> Option<SplitNode> {
        match self {
            SplitNode::Leaf(id) if id == target => None,
            SplitNode::Leaf(_) => Some(self),
            SplitNode::Split { direction, left, right, ratio } => {
                let new_left = left.remove_leaf(target);
                let new_right = right.remove_leaf(target);
                match (new_left, new_right) {
                    (None, Some(surviving)) | (Some(surviving), None) => Some(surviving),
                    (Some(l), Some(r)) => Some(SplitNode::Split {
                        direction, left: Box::new(l), right: Box::new(r), ratio
                    }),
                    (None, None) => None, // shouldn't happen
                }
            }
        }
    }
}
```

### Anti-Patterns to Avoid
- **Shared render state across panes:** Each pane must have its own GridSnapshot and block state. Do NOT try to render multiple panes from a single snapshot.
- **Global cursor position for all panes:** `cursor_position` is currently stored per-session. Split panes need to track which pane the cursor is over separately from keyboard focus.
- **Resizing only active pane:** When the window resizes, ALL panes in ALL tabs must be resized (like current tab resize logic). Compute per-pane cell dimensions from each pane's pixel rect.
- **Modifying draw_frame in-place:** The current `draw_frame` is monolithic. Create a new `draw_pane` helper that renders a single pane, then loop over panes in the main render path. Do NOT try to make the existing draw_frame handle splits -- it would become unmanageably complex.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Text clipping per pane | Custom glyph-level clipping | glyphon `TextBounds` | TextBounds already clips text to pixel rect; combining with wgpu scissor rect handles all clipping |
| Rect clipping per pane | Manual rect intersection | wgpu `set_scissor_rect` | Hardware-accelerated, zero CPU cost, standard GPU approach |
| Unique IDs for panes | Custom ID generator | Existing `SessionMux::next_session_id()` | Each pane leaf is a Session; reuse session ID infrastructure |
| Tree traversal/navigation | Manual parent tracking | Recursive `SplitNode` methods | Binary tree is naturally recursive; methods like `find_neighbor` walk the tree |

**Key insight:** The rendering pipeline already has all the clipping primitives needed (scissor rects for geometry, TextBounds for text). The main work is structural: tree layout, input routing, and lifecycle management.

## Common Pitfalls

### Pitfall 1: Forgetting the Divider Gap in Layout Computation
**What goes wrong:** Pane rects overlap or leave gaps because the divider line width is not accounted for.
**Why it happens:** The ratio split gives exact pixel fractions, but dividers occupy 1-2px.
**How to avoid:** Always subtract divider width from the container before splitting. The gap pixels belong to neither child.
**Warning signs:** Visual artifacts at split boundaries, overlapping content.

### Pitfall 2: PTY Resize with Wrong Cell Dimensions
**What goes wrong:** Terminal content wraps incorrectly because the PTY was resized with full-window cell counts instead of per-pane cell counts.
**Why it happens:** Current resize code computes `num_cols = window_width / cell_w`. With splits, each pane has different pixel dimensions.
**How to avoid:** After computing pane layouts, resize each session's PTY with `pane_width / cell_w` and `pane_height / cell_h`.
**Warning signs:** Text wrapping at wrong column, content spilling into adjacent pane.

### Pitfall 3: Tab Title Regression
**What goes wrong:** Tab.title and Tab.session_id fields are removed when restructuring Tab to hold SplitNode, breaking tab bar rendering.
**Why it happens:** The Tab struct currently has `session_id` for single-session tabs. The temptation is to remove it entirely.
**How to avoid:** Keep `title` on Tab. Replace `session_id` with `root: SplitNode` and `focused_pane: SessionId`. Tab bar still renders from `tab.title`.
**Warning signs:** Tab bar shows no text, compile errors in tab_bar renderer.

### Pitfall 4: Borrow Checker Conflicts in Multi-Pane Render Loop
**What goes wrong:** Cannot borrow `session_mux` immutably for pane iteration while also borrowing `frame_renderer` mutably for drawing.
**Why it happens:** The render loop currently extracts data from one session. With splits, you iterate over multiple sessions.
**How to avoid:** Pre-extract all pane data (snapshots, blocks, status) into owned values before entering the render phase, exactly as done now for the single session. Collect `Vec<PaneRenderData>` first, then iterate to render.
**Warning signs:** Borrow checker errors like "cannot borrow as mutable because it is also borrowed as immutable."

### Pitfall 5: Glyphon TextBounds vs Scissor Rect Mismatch
**What goes wrong:** Text renders outside pane boundaries because TextBounds and scissor rect use different coordinate systems or values.
**Why it happens:** TextBounds uses `i32` pixel coordinates while scissor rect uses `u32`. Text positions (left, top) in TextArea are absolute pixel positions within the full window.
**How to avoid:** Ensure TextBounds and scissor rect use identical pixel regions. Text `left` and `top` must be the pane's absolute pixel offset. TextBounds clips the visible region.
**Warning signs:** Text bleeding into adjacent panes, especially near pane edges.

### Pitfall 6: Status Bar and Tab Bar Positioning with Splits
**What goes wrong:** Status bar renders inside pane area, or tab bar overlaps first pane.
**Why it happens:** Pane layout must account for the tab bar (1 cell height at top) and status bar (1 cell height at bottom) being outside the splittable area.
**How to avoid:** The container rect for the split tree should be `(0, tab_bar_h, window_w, window_h - tab_bar_h - status_bar_h)`. Tab bar and status bar render globally, not per-pane.
**Warning signs:** Status bar appears inside a pane, pane content obscured by tab bar.

## Code Examples

### wgpu set_scissor_rect
```rust
// Source: https://docs.rs/wgpu/28.0.0/wgpu/struct.RenderPass.html
// Clips all subsequent draw calls to the specified pixel rectangle.
// Must be within render target bounds.
render_pass.set_scissor_rect(x: u32, y: u32, width: u32, height: u32);
```

### glyphon TextBounds for Per-Pane Clipping
```rust
// Source: existing grid_renderer.rs pattern
// TextBounds clips text rendering to the pane's pixel region.
let text_area = TextArea {
    buffer: &line_buffer,
    left: pane_x as f32 + col_offset,     // absolute pixel position
    top: pane_y as f32 + line_offset,      // absolute pixel position
    scale: 1.0,
    bounds: TextBounds {
        left: pane_x as i32,               // clip left edge
        top: pane_y as i32,                // clip top edge
        right: (pane_x + pane_width) as i32,
        bottom: (pane_y + pane_height) as i32,
    },
    default_color: GlyphonColor::rgba(204, 204, 204, 255),
    custom_glyphs: &[],
};
```

### Direction-Aware Focus Navigation
```rust
// Navigate focus between panes using Alt+Arrow
impl SplitNode {
    /// Find the neighbor of `current` in `direction`.
    /// Returns None if no neighbor exists in that direction.
    pub fn find_neighbor(
        &self,
        current: SessionId,
        direction: FocusDirection,
        container: &ViewportLayout,
    ) -> Option<SessionId> {
        let layouts = self.compute_layout(container);
        let current_rect = layouts.iter()
            .find(|(id, _)| *id == current)
            .map(|(_, vp)| vp)?;

        // Find the pane whose center is closest in the given direction
        let (cx, cy) = current_rect.center();
        layouts.iter()
            .filter(|(id, _)| *id != current)
            .filter(|(_, vp)| match direction {
                FocusDirection::Left => vp.center().0 < cx,
                FocusDirection::Right => vp.center().0 > cx,
                FocusDirection::Up => vp.center().1 < cy,
                FocusDirection::Down => vp.center().1 > cy,
            })
            .min_by_key(|(_, vp)| {
                let (nx, ny) = vp.center();
                let dx = (nx as i32 - cx as i32).abs();
                let dy = (ny as i32 - cy as i32).abs();
                dx + dy // Manhattan distance
            })
            .map(|(id, _)| *id)
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Tab = 1 session | Tab = SplitNode tree of sessions | Phase 24 | Tab struct changes, render loop changes |
| draw_frame (single viewport) | draw_pane (per-viewport scissor clip) | Phase 24 | FrameRenderer gains viewport-aware method |
| Resize all tabs uniformly | Resize all panes per tab with per-pane dimensions | Phase 24 | PTY resize becomes pane-aware |

## Open Questions

1. **Should pane dividers be drawn as rects in the global pass or per-pane?**
   - What we know: Dividers are 1-2px lines between panes. They exist in the gap between pane rects.
   - What's unclear: Drawing them in the gap means they are NOT inside any pane's scissor rect.
   - Recommendation: Draw dividers in a global pass (before or after pane rendering), not clipped to any pane. This is simpler and avoids scissor rect issues.

2. **Should each pane have its own status bar, or should there be one global status bar?**
   - What we know: Currently there is one global status bar showing the focused session's CWD/git info.
   - What's unclear: Per-pane status bars would eat into pane height significantly.
   - Recommendation: Keep ONE global status bar showing the focused pane's info. This matches VS Code, iTerm2, and other split-pane terminals.

3. **Multi-pass vs single-pass rendering for panes**
   - What we know: glyphon `text_renderer.prepare()` + `text_renderer.render()` can only be called in matched pairs. The current frame already uses two passes (main + pipeline overlay).
   - What's unclear: Whether glyphon supports multiple prepare/render cycles within a single frame for different scissor regions, or whether we need one render pass per pane.
   - Recommendation: Use one render pass per pane (prepare text for pane, render pane with scissor rect, repeat). This is clean and avoids atlas contention. Performance is fine for <10 panes.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + `cargo test` |
| Config file | Cargo.toml workspace |
| Quick run command | `cargo test -p glass_mux` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SPLIT-01 | SplitNode tree construction (leaf, split, nested) | unit | `cargo test -p glass_mux split_tree` | Stub only, needs tests |
| SPLIT-02 | compute_layout returns correct pixel rects | unit | `cargo test -p glass_mux split_tree::tests::layout` | Needs creation |
| SPLIT-03 | Horizontal split divides width by ratio with gap | unit | `cargo test -p glass_mux split_tree::tests::horizontal` | Needs creation |
| SPLIT-04 | Vertical split divides height by ratio with gap | unit | `cargo test -p glass_mux split_tree::tests::vertical` | Needs creation |
| SPLIT-05 | remove_leaf collapses parent Split to surviving sibling | unit | `cargo test -p glass_mux split_tree::tests::remove` | Needs creation |
| SPLIT-06 | find_neighbor returns correct pane for each direction | unit | `cargo test -p glass_mux split_tree::tests::neighbor` | Needs creation |
| SPLIT-07 | Resize ratio adjustment clamps to valid range | unit | `cargo test -p glass_mux split_tree::tests::resize_ratio` | Needs creation |
| SPLIT-08 | Tab with SplitNode tracks focused_pane correctly | unit | `cargo test -p glass_mux session_mux::tests` | Existing tests need extension |
| SPLIT-09 | PTY resize sends correct per-pane cell dimensions | integration | Manual -- requires PTY | Manual only |
| SPLIT-10 | Scissor rect clipping renders correctly | integration | Manual -- requires GPU | Manual only |
| SPLIT-11 | Pane close on last pane closes tab | unit | `cargo test -p glass_mux session_mux::tests` | Needs creation |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_mux`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_mux/src/split_tree.rs` -- needs full test module (currently no tests, just stub type)
- [ ] Tests for layout computation, tree manipulation, focus navigation, ratio resize

## Sources

### Primary (HIGH confidence)
- Codebase inspection: `split_tree.rs`, `session_mux.rs`, `tab.rs`, `types.rs`, `frame.rs`, `grid_renderer.rs`, `rect_renderer.rs`, `main.rs`
- [wgpu RenderPass docs](https://docs.rs/wgpu/latest/wgpu/struct.RenderPass.html) -- set_scissor_rect API
- [glyphon TextArea docs](https://docs.rs/glyphon/latest/glyphon/struct.TextArea.html) -- TextBounds clipping

### Secondary (MEDIUM confidence)
- [wgpu scissor rect discussion](https://github.com/gfx-rs/wgpu/discussions/5403) -- community patterns for sub-viewport rendering
- [MDN setScissorRect](https://developer.mozilla.org/en-US/docs/Web/API/GPURenderPassEncoder/setScissorRect) -- WebGPU specification reference

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - all libraries already in use, only new API is set_scissor_rect which is well-documented
- Architecture: HIGH - binary tree layout is a well-understood pattern; existing stubs match the design
- Pitfalls: HIGH - identified from direct codebase analysis of current rendering and resize paths
- Rendering approach: MEDIUM - glyphon multi-prepare/render per frame needs validation (Open Question 3)

**Research date:** 2026-03-06
**Valid until:** 2026-04-06 (stable domain, no fast-moving dependencies)

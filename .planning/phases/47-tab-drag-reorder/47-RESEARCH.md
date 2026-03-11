# Phase 47: Tab Drag Reorder - Research

**Researched:** 2026-03-10
**Domain:** GUI drag-and-drop interaction / tab bar reordering
**Confidence:** HIGH

## Summary

Tab drag reorder is a self-contained UI interaction feature that requires no external libraries. The existing codebase already has all the primitives: `TabBarRenderer` with per-tab rect computation and hit-testing, `SessionMux` with a `Vec<Tab>` that supports index-based operations, and `WindowContext` with mouse state tracking patterns (see `scrollbar_dragging`, `mouse_left_pressed`).

The implementation follows a classic press-move-release state machine: on mouse down in a tab (not close button/new-tab), record the source tab index and initial X; on mouse move, compute which drop slot the cursor is over and render an insertion indicator; on mouse release, perform the Vec reorder and clear drag state. The `ScrollbarDragInfo` pattern in `main.rs` provides an exact template for how to track drag state across events.

**Primary recommendation:** Add a `TabDragState` struct to `WindowContext`, populate it on left-click in tab body when mouse moves beyond a 5px threshold, render a vertical insertion line indicator during drag, and call `session_mux.reorder_tab(from, to)` on release.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- Click and drag a tab to change its position
- Visual indicator shows drop location during drag
- Standard browser-like tab reorder behavior

### Claude's Discretion
- Drag threshold before reorder initiates (avoid accidental drags)
- Visual indicator style (insertion line, ghost tab, etc.)
- Whether tab index keyboard shortcuts (Ctrl+1-9) update to match new order
- Animation during reorder (smooth slide vs instant swap)

### Deferred Ideas (OUT OF SCOPE)
None
</user_constraints>

## Standard Stack

### Core
No external libraries needed. This is pure application logic using existing primitives.

| Component | Location | Purpose | Status |
|-----------|----------|---------|--------|
| `TabBarRenderer` | `crates/glass_renderer/src/tab_bar.rs` | Tab rect layout, hit-testing | Exists, needs drag indicator rendering |
| `SessionMux` | `crates/glass_mux/src/session_mux.rs` | Tab list management | Exists, needs `reorder_tab()` method |
| `WindowContext` | `src/main.rs:151` | Mouse event state machine | Exists, needs `TabDragState` field |
| `RectInstance` | `crates/glass_renderer/src/rect_renderer.rs` | GPU rect rendering | Exists, used for insertion indicator |

## Architecture Patterns

### Pattern 1: Drag State Machine (follows ScrollbarDragInfo pattern)

**What:** A struct tracking drag state, stored as `Option<TabDragState>` in `WindowContext`.
**When to use:** For all stateful drag interactions in the event loop.
**Example:**

```rust
/// Tab drag reorder tracking state.
struct TabDragState {
    /// Index of the tab being dragged.
    source_index: usize,
    /// X coordinate where the drag started (for threshold check).
    start_x: f32,
    /// Whether the drag threshold has been exceeded (drag is "active").
    active: bool,
    /// Current drop target slot (insertion point index, 0..=tab_count).
    drop_index: Option<usize>,
}
```

**Source:** Pattern derived from `ScrollbarDragInfo` at `main.rs:132-145`.

### Pattern 2: Event Flow (press/move/release)

**What:** The three-phase interaction model already used for scrollbar drag and text selection.

```
MouseInput::Pressed + Tab body hit
  -> Create TabDragState { source_index, start_x, active: false }
  -> Do NOT activate_tab yet (wait for threshold or release-without-drag)

CursorMoved + TabDragState exists
  -> If !active && |current_x - start_x| > DRAG_THRESHOLD: set active = true
  -> If active: compute drop_index from mouse X position, request_redraw()

MouseInput::Released + TabDragState exists
  -> If active && drop_index differs from source: reorder_tab(source, drop)
  -> If !active: activate_tab(source_index) (was a click, not a drag)
  -> Clear TabDragState
```

### Pattern 3: Drop Index Computation

**What:** Convert mouse X position to an insertion slot index (0 through tab_count).

```rust
// In TabBarRenderer:
pub fn drag_drop_index(&self, x: f32, tab_count: usize, viewport_width: f32) -> usize {
    let (tab_width, _) = self.compute_tab_width(tab_count, viewport_width);
    let slot = (x / (tab_width + TAB_GAP) + 0.5) as usize;
    slot.min(tab_count)
}
```

The +0.5 offset means the insertion point switches when the cursor crosses the midpoint of a tab, matching browser behavior.

### Pattern 4: Vec Reorder

**What:** Moving an element in a `Vec<Tab>` from one position to another.

```rust
// In SessionMux:
pub fn reorder_tab(&mut self, from: usize, to: usize) {
    if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
        return;
    }
    let tab = self.tabs.remove(from);
    self.tabs.insert(to, tab);
    // Adjust active_tab to follow the moved tab if it was active
    if self.active_tab == from {
        self.active_tab = to;
    } else if from < self.active_tab && to >= self.active_tab {
        self.active_tab -= 1;
    } else if from > self.active_tab && to <= self.active_tab {
        self.active_tab += 1;
    }
}
```

**Critical:** The `active_tab` index must be adjusted when tabs shift, or the wrong tab will be active after reorder.

### Pattern 5: Insertion Line Indicator

**What:** A thin vertical line rendered at the drop position during active drag.

```rust
// In TabBarRenderer::build_tab_rects, add when drag is active:
const DRAG_INDICATOR_WIDTH: f32 = 2.0;
const DRAG_INDICATOR_COLOR: [f32; 4] = [0.4, 0.6, 1.0, 1.0]; // Blue accent

let indicator_x = drop_index as f32 * (tab_width + TAB_GAP) - TAB_GAP / 2.0 - DRAG_INDICATOR_WIDTH / 2.0;
rects.push(RectInstance {
    pos: [indicator_x.max(0.0), 0.0, DRAG_INDICATOR_WIDTH, self.cell_height],
    color: DRAG_INDICATOR_COLOR,
});
```

### Recommended Project Structure

No new files needed. Changes go into existing files:

```
src/main.rs                              # TabDragState struct, event handling
crates/glass_mux/src/session_mux.rs      # reorder_tab() method
crates/glass_renderer/src/tab_bar.rs     # drag_drop_index(), indicator rendering
```

### Anti-Patterns to Avoid
- **Starting drag immediately on mousedown:** Users frequently click tabs without intending to drag. Always use a threshold (5px recommended).
- **Reordering on every CursorMoved:** Only reorder on mouse release. During drag, only update the visual indicator position.
- **Forgetting active_tab adjustment:** The most common bug. When `tabs.remove(from)` shifts elements, `active_tab` must be recalculated.
- **Activating tab on mousedown:** If the user starts dragging, the tab should not switch focus. Only `activate_tab` on a non-drag click (mouseup without exceeding threshold).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Vec element move | Manual swap loops | `Vec::remove` + `Vec::insert` | Standard Rust idiom, handles index shifting correctly |
| Hit testing | New coordinate system | Existing `compute_tab_width()` | Already computes exact tab positions and widths |

## Common Pitfalls

### Pitfall 1: Click vs Drag Ambiguity
**What goes wrong:** Tab clicks stop working because all mousedown events start a drag.
**Why it happens:** No threshold between "click" and "drag" states.
**How to avoid:** Only set `active = true` in CursorMoved when `|dx| > 5px`. On mouseup with `!active`, treat as a normal tab click.
**Warning signs:** Tabs require precise clicking with no mouse movement.

### Pitfall 2: active_tab Index Corruption
**What goes wrong:** After reorder, the wrong tab is shown or the app panics on out-of-bounds.
**Why it happens:** `remove(from)` shifts all indices after `from`, but `active_tab` isn't adjusted before `insert(to)`.
**How to avoid:** Compute the effective `to` index accounting for the removal, then set `active_tab` to the final position of the dragged tab if it was active.
**Warning signs:** Switching tabs after reorder goes to unexpected tab.

### Pitfall 3: Drag Visual Not Cleared
**What goes wrong:** Insertion indicator persists after drop.
**Why it happens:** `tab_drag_state` not set to `None` on mouseup, or redraw not requested.
**How to avoid:** Always clear drag state on any mouseup (left button), regardless of whether drag was active.
**Warning signs:** Blue line stays visible between tabs after releasing mouse.

### Pitfall 4: Close Button Interaction During Drag
**What goes wrong:** Dragging over a close button triggers tab close.
**Why it happens:** The close button hit-test fires during drag.
**How to avoid:** When `tab_drag_state.is_some()`, skip close button hit-testing entirely.
**Warning signs:** Tabs disappear when dragging over the close button area.

### Pitfall 5: Drop at Same Position
**What goes wrong:** Unnecessary reorder when dropping tab at its original position.
**Why it happens:** No guard checking `from == to`.
**How to avoid:** Early return in `reorder_tab` when `from == to` or `from + 1 == to` (insertion after self is no-op).

## Code Examples

### Drag Threshold Check (CursorMoved)
```rust
// In CursorMoved handler, after scrollbar drag check and before tab hover:
if let Some(ref mut drag) = ctx.tab_drag_state {
    if !drag.active {
        if (mouse_x - drag.start_x).abs() > 5.0 {
            drag.active = true;
        }
    }
    if drag.active {
        let viewport_w = ctx.window.inner_size().width as f32;
        let drop_idx = ctx.frame_renderer.tab_bar().drag_drop_index(
            mouse_x,
            ctx.session_mux.tab_count(),
            viewport_w,
        );
        drag.drop_index = Some(drop_idx);
        ctx.window.request_redraw();
    }
    return; // Consume event during drag (don't update hover, selection, etc.)
}
```

### Modified MouseInput::Pressed (Tab body click)
```rust
Some(TabHitResult::Tab(tab_idx)) => {
    // Don't activate tab immediately -- start potential drag
    ctx.tab_drag_state = Some(TabDragState {
        source_index: tab_idx,
        start_x: x as f32,
        active: false,
        drop_index: None,
    });
    // Don't set mouse_left_pressed (tab bar consumes this)
    ctx.mouse_left_pressed = false;
}
```

### Modified MouseInput::Released (Complete or Cancel Drag)
```rust
// Before existing scrollbar release check:
if let Some(drag) = ctx.tab_drag_state.take() {
    if drag.active {
        if let Some(drop_idx) = drag.drop_index {
            ctx.session_mux.reorder_tab(drag.source_index, drop_idx);
        }
    } else {
        // Was a click, not a drag -- activate the tab
        ctx.session_mux.activate_tab(drag.source_index);
    }
    ctx.window.request_redraw();
    return;
}
```

## State of the Art

This is a standard GUI interaction pattern. No evolving ecosystem considerations.

| Approach | Notes |
|----------|-------|
| Threshold-based drag | Industry standard (5px typical) |
| Insertion line indicator | Used by Chrome, Firefox, VS Code, most tab-based UIs |
| Instant reorder (no animation) | Simplest correct implementation; animation can be added later |

## Discretion Recommendations

Based on research, here are recommendations for the areas left to Claude's discretion:

| Area | Recommendation | Rationale |
|------|---------------|-----------|
| Drag threshold | 5px horizontal movement | Industry standard; prevents accidental drags on imprecise clicks |
| Visual indicator | 2px vertical insertion line, blue accent (#6699FF) | Minimal, clear, matches browser conventions. Ghost tab adds complexity for little UX gain. |
| Ctrl+1-9 shortcuts | Yes, update to match new order | Tab indices should always reflect visual order. No code change needed -- shortcuts already use `activate_tab(index)` which indexes into the Vec. |
| Animation | None (instant) | Matches the codebase's current approach (no animations anywhere). Can be added in a future phase if desired. |

## Open Questions

None. This is a well-understood UI pattern with clear implementation path using existing codebase primitives.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[cfg(test)]` + cargo test |
| Config file | None needed |
| Quick run command | `cargo test -p glass_mux session_mux::tests --  reorder` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map
| Behavior | Test Type | Automated Command | File Exists? |
|----------|-----------|-------------------|-------------|
| `reorder_tab` moves tab correctly | unit | `cargo test -p glass_mux -- reorder` | Will create |
| `reorder_tab` adjusts `active_tab` | unit | `cargo test -p glass_mux -- reorder` | Will create |
| `reorder_tab` no-op when from==to | unit | `cargo test -p glass_mux -- reorder` | Will create |
| `drag_drop_index` returns correct slot | unit | `cargo test -p glass_renderer -- drag_drop` | Will create |
| Drop indicator renders at correct position | unit | `cargo test -p glass_renderer -- drag_indicator` | Will create |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_mux -p glass_renderer`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green + `cargo clippy --workspace -- -D warnings`

### Wave 0 Gaps
- [ ] `reorder_tab` tests in `session_mux.rs` -- covers reorder logic and active_tab adjustment
- [ ] `drag_drop_index` test in `tab_bar.rs` -- covers drop position calculation
- [ ] `build_tab_rects` with drag indicator test in `tab_bar.rs` -- covers visual indicator

## Sources

### Primary (HIGH confidence)
- Direct code inspection of `tab_bar.rs` (700 lines) -- tab layout, hit-testing, rect building
- Direct code inspection of `session_mux.rs` (617 lines) -- tab Vec management, active_tab tracking
- Direct code inspection of `main.rs` (lines 131-209, 1654-2200) -- mouse event handling, WindowContext state, ScrollbarDragInfo pattern

### Secondary (MEDIUM confidence)
- Browser tab drag UX conventions (Chrome, Firefox, VS Code) -- well-established patterns

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - all components are existing codebase code, no external deps
- Architecture: HIGH - follows exact patterns already in use (ScrollbarDragInfo, hit-testing)
- Pitfalls: HIGH - well-known GUI interaction pitfalls, verified against codebase specifics

**Research date:** 2026-03-10
**Valid until:** Indefinite (pure application logic, no external dependencies)

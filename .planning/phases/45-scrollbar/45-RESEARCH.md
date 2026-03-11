# Phase 45: Scrollbar - Research

**Researched:** 2026-03-10
**Domain:** GPU-rendered scrollbar UI component (wgpu + winit mouse events)
**Confidence:** HIGH

## Summary

This phase adds a visible, interactive scrollbar to every terminal pane. The scrollbar is a pure rendering + input-handling feature with no new external dependencies. It follows the exact same architecture pattern used by TabBarRenderer and StatusBarRenderer: a dedicated `ScrollbarRenderer` struct that produces `RectInstance` quads for the track and thumb, plus hit-testing methods for mouse interactions.

The critical integration points are: (1) shrinking the terminal grid by 8px on the right to reserve scrollbar space, which affects PTY column calculations and must flow through both single-pane and multi-pane resize paths; (2) intercepting mouse events in main.rs to detect scrollbar clicks/drags before they reach text selection or other handlers; (3) reading `display_offset` and `history_size` from `GridSnapshot` (already captured per frame) to compute thumb position and size.

**Primary recommendation:** Create a `ScrollbarRenderer` in `crates/glass_renderer/src/scrollbar.rs` following the TabBarRenderer pattern exactly. Integrate it into `FrameRenderer` alongside the existing renderers. Handle mouse input in main.rs with a priority check before text selection logic.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- Always visible -- permanent thin scrollbar on right edge of every pane
- 8px wide narrow gutter
- Reserves its own space (terminal grid shrinks by 8px on the right) -- never overlays text
- Drag thumb to scroll smoothly through history
- Click above/below thumb to jump by one page
- Resting: subtle dim gray (~rgba(100,100,100,0.4))
- On hover/drag: brighter (~rgba(150,150,150,0.7))
- Slightly visible track background (~rgba(255,255,255,0.03))
- Every pane gets its own scrollbar on its right edge
- Scrollbar drawn inside pane viewport bounds (doesn't touch pane dividers)
- Clicking/dragging a pane's scrollbar also focuses that pane

### Claude's Discretion
- Minimum thumb height (ensure thumb is always grabbable)
- Exact scroll-to-position mapping math
- Animation/transition timing for hover effects
- Behavior when scrollback buffer is empty (thumb fills entire track)

### Deferred Ideas (OUT OF SCOPE)
None
</user_constraints>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| wgpu | 28.0 | GPU rect rendering for track + thumb | Already used for all UI rects |
| winit | 0.30 | Mouse events (click, drag, hover) | Already used for all input handling |
| alacritty_terminal | =0.25.1 | scroll_display(Scroll::Delta), display_offset, history_size | Already provides all scroll state |

### Supporting
No new dependencies needed. Everything builds on existing `RectInstance`, `RectRenderer`, and `GridSnapshot` infrastructure.

## Architecture Patterns

### Recommended Project Structure
```
crates/glass_renderer/src/
    scrollbar.rs          # NEW: ScrollbarRenderer (rect builder + hit-test)
    frame.rs              # MODIFIED: integrate scrollbar rects into draw pipelines
    lib.rs                # MODIFIED: pub mod scrollbar, re-exports
src/
    main.rs               # MODIFIED: mouse event handling, grid width adjustment
```

### Pattern 1: ScrollbarRenderer (follows TabBarRenderer pattern)
**What:** A pure-data renderer struct that takes scroll state and viewport dimensions, returns `Vec<RectInstance>` for GPU rendering plus methods for hit-testing.
**When to use:** Always -- this is the only pattern used for UI chrome in Glass.
**Example:**
```rust
// Source: Existing pattern from crates/glass_renderer/src/tab_bar.rs
pub struct ScrollbarRenderer {
    width: f32,  // 8.0px
}

impl ScrollbarRenderer {
    pub fn new() -> Self {
        Self { width: SCROLLBAR_WIDTH }
    }

    /// Build track + thumb rects for a single pane's scrollbar.
    /// Returns empty vec if history_size == 0.
    pub fn build_scrollbar_rects(
        &self,
        viewport_x: f32,      // pane right edge minus scrollbar width
        viewport_y: f32,      // pane top
        viewport_height: f32,  // pane height
        display_offset: usize,
        history_size: usize,
        screen_lines: usize,
        is_hovered: bool,
        is_dragging: bool,
    ) -> Vec<RectInstance> { ... }

    /// Hit-test: is the given (x,y) within this scrollbar's track?
    pub fn hit_test(
        &self,
        mouse_x: f32,
        mouse_y: f32,
        scrollbar_x: f32,
        viewport_y: f32,
        viewport_height: f32,
    ) -> Option<ScrollbarHit> { ... }
}

pub enum ScrollbarHit {
    Thumb,              // Mouse is on the thumb (for drag)
    TrackAbove,         // Mouse is above thumb (page up)
    TrackBelow,         // Mouse is below thumb (page down)
}
```

### Pattern 2: Scroll Position Math
**What:** Map between pixel position in the scrollbar track and terminal scroll offset.
**When to use:** Both for rendering (offset -> thumb position) and for dragging (thumb position -> offset).
**Example:**
```rust
// Total scrollable content = history_size + screen_lines
// Visible portion = screen_lines
// Thumb height ratio = screen_lines / total_lines (clamped to minimum)
// Thumb position = (history_size - display_offset) / history_size * (track_height - thumb_height)

fn compute_thumb_geometry(
    track_height: f32,
    history_size: usize,
    screen_lines: usize,
    display_offset: usize,
) -> (f32, f32) {  // (thumb_y_offset, thumb_height)
    let total_lines = history_size + screen_lines;
    let thumb_ratio = (screen_lines as f32) / (total_lines as f32);
    let thumb_height = (track_height * thumb_ratio).max(MIN_THUMB_HEIGHT);
    let scrollable_track = track_height - thumb_height;

    // display_offset=0 means at bottom, display_offset=history_size means at top
    let scroll_ratio = if history_size > 0 {
        1.0 - (display_offset as f32 / history_size as f32)
    } else {
        1.0
    };
    let thumb_y = scrollable_track * scroll_ratio;
    (thumb_y, thumb_height)
}
```

### Pattern 3: Mouse Drag State Tracking
**What:** Track scrollbar drag state in the window context, similar to how `mouse_left_pressed` tracks selection drag.
**When to use:** For thumb drag scrolling.
**Example:**
```rust
// In WindowContext (main.rs):
struct ScrollbarDragState {
    /// Which pane's scrollbar is being dragged (None if no drag active)
    pane_id: Option<SessionId>,
    /// Y offset within the thumb where drag started (for smooth dragging)
    thumb_grab_offset: f32,
    /// The scrollbar's track region for mapping
    track_y: f32,
    track_height: f32,
}
```

### Pattern 4: Grid Width Reduction for Scrollbar Gutter
**What:** Subtract 8px from the available grid width when computing terminal columns.
**When to use:** In all resize calculations (single-pane and multi-pane).
**Key insight:** This affects PTY column count. The scrollbar reserves space from the pane's width, reducing the number of terminal columns.
**Example:**
```rust
// Single-pane resize (main.rs ~line 999):
// BEFORE: let num_cols = (size.width as f32 / cell_w).floor().max(1.0) as u16;
// AFTER:  let num_cols = ((size.width as f32 - SCROLLBAR_WIDTH) / cell_w).floor().max(1.0) as u16;

// Multi-pane resize (resize_all_panes ~line 310):
// BEFORE: let pane_cols = (vp.width as f32 / cell_w).floor().max(1.0) as u16;
// AFTER:  let pane_cols = ((vp.width as f32 - SCROLLBAR_WIDTH) / cell_w).floor().max(1.0) as u16;
```

### Anti-Patterns to Avoid
- **Overlaying scrollbar on terminal text:** The decision is explicit -- scrollbar reserves its own gutter space. Grid rendering must clip to `viewport_width - 8px`.
- **Coupling scrollbar state to terminal state:** The scrollbar is a rendering concern only. It reads `display_offset` and `history_size` from `GridSnapshot` (already captured) and calls `scroll_display()` on the terminal. No new state stored in the terminal.
- **Handling scrollbar drag in CursorMoved without priority:** Scrollbar drag MUST be checked before text selection drag in the `CursorMoved` handler to prevent conflicts.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Scroll position tracking | Custom scroll offset variable | `GridSnapshot.display_offset` + `GridSnapshot.history_size` | Already captured every frame in snapshot_term() |
| Scroll commands | Custom scroll math | `term.scroll_display(Scroll::Delta(n))` | alacritty_terminal handles clamping, alternate screen, etc. |
| Rect rendering | New GPU pipeline | Existing `RectInstance` + `RectRenderer` | Instanced pipeline already handles arbitrary colored rects |
| Hit-testing | Complex geometry system | Simple coordinate range checks | TabBarRenderer::hit_test() proves simple arithmetic is sufficient |

**Key insight:** Every building block already exists. The scrollbar is purely a composition of existing `RectInstance` rendering + existing `Scroll::Delta` API + existing mouse event patterns.

## Common Pitfalls

### Pitfall 1: Off-by-one in scroll position mapping
**What goes wrong:** Thumb position doesn't match actual scroll state -- at "bottom" the thumb isn't at the bottom of the track, or dragging to the very top doesn't reach the oldest history.
**Why it happens:** `display_offset` is 0 at the bottom (most recent) and equals `history_size` at the top (oldest). This is inverted from visual top-to-bottom ordering.
**How to avoid:** Use `scroll_ratio = 1.0 - (display_offset / history_size)` to map offset to visual position. Test with: offset=0 gives thumb at bottom, offset=history_size gives thumb at top.
**Warning signs:** Scrollbar thumb moves in wrong direction, or doesn't reach track extremes.

### Pitfall 2: Grid width not reduced everywhere
**What goes wrong:** Terminal text renders under the scrollbar, or columns are wrong in some pane configurations.
**Why it happens:** Column count is computed in multiple places: single-pane resize, multi-pane resize, initial session creation, and font change. Missing any one creates inconsistency.
**How to avoid:** Search for all places that compute `num_cols` or `pane_cols` and subtract `SCROLLBAR_WIDTH` from the width. There are at least 5 locations in main.rs.
**Warning signs:** Text visible under scrollbar track, column count differs between resize and initial creation.

### Pitfall 3: Scrollbar drag conflicts with text selection
**What goes wrong:** Clicking on the scrollbar starts a text selection, or dragging the thumb selects text.
**Why it happens:** Both use left mouse button. The `MouseInput::Pressed` handler must check scrollbar hit BEFORE starting text selection.
**How to avoid:** In the left-click handler, check scrollbar hit first. If hit, set `scrollbar_dragging = true` and `mouse_left_pressed = false` (or skip selection start). In `CursorMoved`, check `scrollbar_dragging` before selection update.
**Warning signs:** Selection highlight appears when dragging scrollbar.

### Pitfall 4: Jittery thumb during drag
**What goes wrong:** Thumb jumps around or doesn't follow mouse smoothly.
**Why it happens:** Not tracking the initial grab offset within the thumb. If you map mouse_y directly to scroll position, the thumb snaps its top edge to the cursor.
**How to avoid:** When drag starts on the thumb, record the offset from the thumb's top edge to the mouse position (`thumb_grab_offset`). During drag, subtract this offset from mouse_y before computing scroll position.
**Warning signs:** Thumb "jumps" when you first start dragging.

### Pitfall 5: Scrollbar in empty buffer
**What goes wrong:** Division by zero or NaN when history_size is 0.
**Why it happens:** `scroll_ratio = display_offset / history_size` with `history_size = 0`.
**How to avoid:** When `history_size == 0`, thumb fills the entire track (ratio = 1.0). Guard all divisions by history_size with zero checks.
**Warning signs:** Missing scrollbar, panic, or NaN rendering artifacts.

### Pitfall 6: Multi-pane scrollbar hover state
**What goes wrong:** Hovering over one pane's scrollbar highlights a different pane's scrollbar, or all scrollbar thumbs brighten.
**Why it happens:** Hover state tracked globally instead of per-pane.
**How to avoid:** Track which pane's scrollbar (if any) the mouse is currently over. Use `Option<SessionId>` for hover state. Clear it when mouse leaves all scrollbar regions.
**Warning signs:** Wrong scrollbar brightens on hover.

## Code Examples

### Example 1: ScrollbarRenderer struct and constants
```rust
// crates/glass_renderer/src/scrollbar.rs
use crate::rect_renderer::RectInstance;

/// Scrollbar width in pixels.
pub const SCROLLBAR_WIDTH: f32 = 8.0;

/// Minimum thumb height to ensure it's always grabbable.
const MIN_THUMB_HEIGHT: f32 = 20.0;

/// Track background color: barely visible stripe.
const TRACK_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 0.03];

/// Thumb resting color: subtle dim gray.
const THUMB_COLOR_REST: [f32; 4] = [100.0 / 255.0, 100.0 / 255.0, 100.0 / 255.0, 0.4];

/// Thumb hover/drag color: brighter.
const THUMB_COLOR_ACTIVE: [f32; 4] = [150.0 / 255.0, 150.0 / 255.0, 150.0 / 255.0, 0.7];
```

### Example 2: build_scrollbar_rects producing RectInstances
```rust
pub fn build_scrollbar_rects(
    &self,
    pane_right_x: f32,   // right edge of pane viewport
    pane_y: f32,          // top of pane
    pane_height: f32,     // height of pane
    display_offset: usize,
    history_size: usize,
    screen_lines: usize,
    is_hovered: bool,
    is_dragging: bool,
) -> Vec<RectInstance> {
    let scrollbar_x = pane_right_x - SCROLLBAR_WIDTH;
    let mut rects = Vec::with_capacity(2);

    // Track background
    rects.push(RectInstance {
        pos: [scrollbar_x, pane_y, SCROLLBAR_WIDTH, pane_height],
        color: TRACK_COLOR,
    });

    // Thumb
    let total_lines = history_size + screen_lines;
    if total_lines == 0 { return rects; }

    let thumb_ratio = (screen_lines as f32) / (total_lines as f32);
    let thumb_height = (pane_height * thumb_ratio).max(MIN_THUMB_HEIGHT).min(pane_height);
    let scrollable_track = pane_height - thumb_height;

    let scroll_ratio = if history_size > 0 {
        1.0 - (display_offset as f32 / history_size as f32)
    } else {
        1.0 // At bottom when no history
    };
    let thumb_y = pane_y + scrollable_track * scroll_ratio;

    let thumb_color = if is_dragging || is_hovered {
        THUMB_COLOR_ACTIVE
    } else {
        THUMB_COLOR_REST
    };

    rects.push(RectInstance {
        pos: [scrollbar_x, thumb_y, SCROLLBAR_WIDTH, thumb_height],
        color: thumb_color,
    });

    rects
}
```

### Example 3: Integration into draw_frame (single-pane)
```rust
// In frame.rs draw_frame(), after tab bar rects, before search overlay:
// Build scrollbar rects for the single pane
{
    let scrollbar_rects = self.scrollbar.build_scrollbar_rects(
        w,              // pane right edge = full viewport width
        grid_y_offset,  // below tab bar
        h - grid_y_offset - status_bar_h,  // between tab bar and status bar
        snapshot.display_offset,
        snapshot.history_size,
        snapshot.screen_lines,
        scrollbar_hover,
        scrollbar_dragging,
    );
    rect_instances.extend(scrollbar_rects);
}
```

### Example 4: Mouse drag-to-scroll conversion
```rust
// In CursorMoved handler, when scrollbar_drag is active:
fn scroll_to_ratio(
    mouse_y: f32,
    thumb_grab_offset: f32,
    track_y: f32,
    track_height: f32,
    thumb_height: f32,
    history_size: usize,
) -> i32 {
    let effective_y = mouse_y - thumb_grab_offset;
    let scrollable_track = track_height - thumb_height;
    if scrollable_track <= 0.0 { return 0; }
    let ratio = ((effective_y - track_y) / scrollable_track).clamp(0.0, 1.0);
    // ratio 0.0 = top (oldest history), ratio 1.0 = bottom (newest)
    let target_offset = ((1.0 - ratio) * history_size as f32) as usize;
    // Return delta from current to target
    target_offset as i32  // caller computes delta with current display_offset
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| No scrollbar | Always-visible scrollbar | This phase | Users see position in scrollback |
| Mouse wheel only | Mouse wheel + click + drag | This phase | Full scrollbar interaction model |
| Grid uses full pane width | Grid reserves 8px for scrollbar | This phase | PTY column count reduced by ~1 column |

## Open Questions

1. **Hover detection without winit hover events**
   - What we know: winit sends `CursorMoved` events with position. We can check if the cursor is within scrollbar bounds on each move.
   - What's unclear: Performance impact of checking scrollbar bounds on every CursorMoved event.
   - Recommendation: The check is trivial arithmetic (4 comparisons). No performance concern. Track `scrollbar_hovered_pane: Option<SessionId>` in WindowContext and request redraw only when hover state changes.

2. **Smooth transition timing for hover effect**
   - What we know: The context specifies "smooth visual transition between states" for thumb color.
   - What's unclear: Whether to implement true animation (interpolating color over time) or instant snap (which is simpler and still feels responsive).
   - Recommendation: Use instant snap for v1. The color change from 0.4 to 0.7 alpha is subtle enough that instant transition feels natural. True animation would require a timer/animation system that doesn't exist yet. Can be added later if desired.

3. **Scrollbar during alternate screen mode**
   - What we know: When `TermMode` includes `ALT_SCREEN`, apps like vim take over the full screen. `history_size` is typically 0 in alternate screen.
   - What's unclear: Whether the scrollbar should still be visible (as an empty track) or hidden.
   - Recommendation: Keep the scrollbar always visible per the user decision. When `history_size == 0`, the thumb fills the entire track, indicating "you're looking at everything." The gutter space is always reserved regardless.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (Rust built-in + criterion for benchmarks) |
| Config file | Cargo.toml workspace |
| Quick run command | `cargo test -p glass_renderer --lib` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SB-01 | ScrollbarRenderer produces track + thumb rects | unit | `cargo test -p glass_renderer scrollbar` | No - Wave 0 |
| SB-02 | Thumb height proportional to visible/total ratio | unit | `cargo test -p glass_renderer scrollbar::tests::thumb_height` | No - Wave 0 |
| SB-03 | Thumb position maps correctly from display_offset | unit | `cargo test -p glass_renderer scrollbar::tests::thumb_position` | No - Wave 0 |
| SB-04 | Minimum thumb height enforced | unit | `cargo test -p glass_renderer scrollbar::tests::min_thumb` | No - Wave 0 |
| SB-05 | Empty history produces full-track thumb | unit | `cargo test -p glass_renderer scrollbar::tests::empty_history` | No - Wave 0 |
| SB-06 | Hit-test correctly identifies Thumb/TrackAbove/TrackBelow | unit | `cargo test -p glass_renderer scrollbar::tests::hit_test` | No - Wave 0 |
| SB-07 | Hit-test returns None for coordinates outside scrollbar | unit | `cargo test -p glass_renderer scrollbar::tests::hit_test_miss` | No - Wave 0 |
| SB-08 | Hover state changes thumb color | unit | `cargo test -p glass_renderer scrollbar::tests::hover_color` | No - Wave 0 |
| SB-09 | Grid width subtracted in resize calculations | integration | Manual - verify with `cargo run` | Manual-only |
| SB-10 | Scrollbar drag updates display_offset correctly | integration | Manual - verify with `cargo run` | Manual-only |
| SB-11 | Multi-pane: each pane has independent scrollbar | integration | Manual - verify with `cargo run` | Manual-only |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_renderer --lib`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_renderer/src/scrollbar.rs` -- new file with ScrollbarRenderer + tests (SB-01 through SB-08)
- [ ] No framework install needed -- cargo test already configured

## Sources

### Primary (HIGH confidence)
- **Codebase inspection:** `crates/glass_renderer/src/tab_bar.rs` -- complete TabBarRenderer pattern (struct, build_rects, build_text, hit_test, tests)
- **Codebase inspection:** `crates/glass_renderer/src/status_bar.rs` -- StatusBarRenderer pattern (rects + labels)
- **Codebase inspection:** `crates/glass_renderer/src/rect_renderer.rs` -- RectInstance struct, instanced GPU pipeline
- **Codebase inspection:** `crates/glass_renderer/src/frame.rs` -- draw_frame() and draw_multi_pane_frame() rendering pipelines
- **Codebase inspection:** `crates/glass_terminal/src/grid_snapshot.rs` -- GridSnapshot with display_offset, history_size fields
- **Codebase inspection:** `src/main.rs` -- Mouse event handling (lines 1606-1834), resize logic (lines 286-329, 981-1016), multi-pane layout (lines 824-905)
- **Codebase inspection:** `crates/glass_mux/src/layout.rs` -- ViewportLayout struct, split(), DIVIDER_GAP

### Secondary (MEDIUM confidence)
- **Pattern analysis:** alacritty_terminal `Scroll::Delta(n)` API -- used in 4 places in main.rs, confirmed working

### Tertiary (LOW confidence)
None -- all findings based on direct codebase inspection.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all existing infrastructure
- Architecture: HIGH -- directly follows established TabBarRenderer pattern verified in codebase
- Pitfalls: HIGH -- identified from actual code paths (6 integration points in main.rs that compute columns)
- Scroll math: HIGH -- display_offset/history_size semantics confirmed from GridSnapshot and existing scroll_display() usage

**Research date:** 2026-03-10
**Valid until:** 2026-04-10 (stable -- no external dependencies)

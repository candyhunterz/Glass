# Phase 46: Tab Bar Controls - Research

**Researched:** 2026-03-10
**Domain:** GPU-rendered tab bar UI controls (buttons, hover states, hit-testing)
**Confidence:** HIGH

## Summary

This phase adds clickable "+" (new tab) and "x" (close tab) buttons to the existing `TabBarRenderer`, plus tab overflow handling with minimum-width compression. The existing code in `tab_bar.rs` (360 lines) provides a clean foundation: equal-width tab rects, text labels, and a simple hit-test that returns a tab index. The main work is extending the layout algorithm to reserve space for a "+" button after the last tab, adding per-tab close button rects that appear on hover, and upgrading `hit_test()` to distinguish between tab body clicks, close button clicks, and new-tab button clicks.

The scrollbar implementation from Phase 45 establishes the exact pattern for hover state: a field on `WindowContext` tracks which element is hovered, `CursorMoved` updates it via hit-testing, and the render path passes the hover state to the renderer which changes colors accordingly. Tab hover follows this identical pattern.

**Primary recommendation:** Extend `TabBarRenderer` with a new layout engine that computes tab widths respecting a minimum width, reserves "+" button space, and returns a `TabHitResult` enum from `hit_test()` instead of `Option<usize>`.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- "+" button appears immediately after the last tab, moves rightward as tabs are added (Chrome/VS Code pattern)
- "x" close button appears on the right side of a tab only on hover, replaces some title space when visible
- Use "+" and "x" text glyphs (not custom icons)
- On hover: show a subtle circular/rounded background highlight behind the glyph
- Minimal style consistent with VS Code approach
- Tabs compress to a minimum width as more are added, titles truncate with ellipsis
- No scroll arrows or dropdown menus for overflow

### Claude's Discretion
- Minimum tab width before truncation kicks in
- Exact hover highlight size and opacity
- Close button glyph positioning within tab rect
- "+" button sizing relative to tab height

### Deferred Ideas (OUT OF SCOPE)
None
</user_constraints>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| wgpu | 28.0 | GPU rect rendering for button backgrounds | Already used for all UI rects |
| glyphon | 0.10 | Text rendering for "+" and "x" glyphs | Already used for all tab text |
| winit | 0.30 | Mouse events (CursorMoved, MouseInput) | Already handles all input |

### Supporting
No new dependencies needed. All rendering uses existing `RectInstance` pipeline and glyphon text pipeline.

## Architecture Patterns

### Modified File Structure
```
crates/glass_renderer/src/tab_bar.rs   # Main changes: layout, rendering, hit-testing
src/main.rs                             # Hover state tracking, click handling updates
crates/glass_renderer/src/frame.rs      # Pass hover state to tab bar rendering
```

### Pattern 1: Extended Hit-Test Result Enum
**What:** Replace `Option<usize>` return from `hit_test()` with a `TabHitResult` enum.
**When to use:** Tab bar click handling must distinguish three outcomes.
**Example:**
```rust
pub enum TabHitResult {
    /// Clicked on a tab body (not the close button).
    Tab(usize),
    /// Clicked the close button on tab at index.
    CloseButton(usize),
    /// Clicked the "+" new tab button.
    NewTabButton,
}
```

### Pattern 2: Hover State on WindowContext (established by Phase 45 scrollbar)
**What:** Add `tab_bar_hovered_tab: Option<usize>` to `WindowContext` to track which tab the mouse is over. Updated in `CursorMoved`, consumed during rendering to show/hide close buttons.
**When to use:** The close button only appears on hover, so the renderer needs to know which tab is hovered.
**Example:**
```rust
// In WindowContext struct:
tab_bar_hovered_tab: Option<usize>,

// In CursorMoved handler, when mouse is in tab bar region:
let new_hover = renderer.tab_bar().hit_test_tab_index(mouse_x, tab_count, viewport_w);
if new_hover != ctx.tab_bar_hovered_tab {
    ctx.tab_bar_hovered_tab = new_hover;
    ctx.window.request_redraw();
}
```

### Pattern 3: Variable-Width Tab Layout with Minimum Width
**What:** Replace equal-width layout with clamped-minimum-width layout. Tabs share available space (minus "+" button width) equally, but never shrink below a minimum.
**When to use:** Always -- replaces `build_tab_rects()` logic.
**Example:**
```rust
const MIN_TAB_WIDTH: f32 = 60.0;   // Minimum before tabs stop shrinking
const NEW_TAB_BUTTON_WIDTH: f32 = 32.0;  // "+" button width

// Available space for tabs = viewport_width - NEW_TAB_BUTTON_WIDTH
// tab_width = max(MIN_TAB_WIDTH, (available - gaps) / tab_count)
// If tabs at min width exceed available space, they overflow (clip at right edge)
```

### Pattern 4: Close Button Rect Within Tab
**What:** When a tab is hovered, render a small circular background rect and "x" glyph at the right side of the tab.
**When to use:** Only for the hovered tab (per user decision).
**Example:**
```rust
const CLOSE_BUTTON_SIZE: f32 = 16.0;  // Diameter of circular highlight
const CLOSE_BUTTON_PADDING: f32 = 6.0; // Right padding from tab edge

// Close button center: (tab_right - CLOSE_BUTTON_PADDING - CLOSE_BUTTON_SIZE/2, tab_center_y)
// Highlight rect: centered on glyph, size CLOSE_BUTTON_SIZE x CLOSE_BUTTON_SIZE
// Highlight color: slightly lighter than tab bg, e.g., [60/255, 60/255, 60/255, 1.0]
```

### Anti-Patterns to Avoid
- **Separate "+" button renderer:** Keep all tab bar layout in `TabBarRenderer` -- the "+" button is part of the tab bar layout, not a separate component.
- **Hover state in renderer:** Do NOT store hover state inside `TabBarRenderer`. Keep it in `WindowContext` and pass it as a parameter (matches scrollbar pattern).
- **Pixel-perfect circle rendering:** wgpu rects are axis-aligned rectangles. The "circular" highlight is actually a small square rect -- this is fine for the VS Code aesthetic at small sizes. Do not attempt actual circle rendering.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Text truncation with ellipsis | Custom char-by-char truncation | Existing `truncate_title()` in tab_bar.rs | Already handles "..." suffix, just adjust max length based on available width |
| Rect rendering | Custom shader for buttons | Existing `RectInstance` pipeline | All UI rects use the same pipeline |
| Text rendering | Custom glyph pipeline for "+" / "x" | Existing glyphon `TabLabel` pipeline | Same font, same rendering path |

## Common Pitfalls

### Pitfall 1: Close Button Hit-Test Must Be Checked Before Tab Hit-Test
**What goes wrong:** Click on close button activates the tab instead of closing it.
**Why it happens:** If you check tab body hit first (which encompasses the close button area), close button never triggers.
**How to avoid:** In `hit_test()`, check close button rect first (inner rect), then tab body (outer rect). Close button is a sub-region of the tab.
**Warning signs:** Clicking "x" switches tabs instead of closing them.

### Pitfall 2: Close Button on Active Tab vs Inactive Tab
**What goes wrong:** Closing the active tab while hovering causes stale hover state.
**Why it happens:** After closing a tab, tab indices shift. The `tab_bar_hovered_tab` index may point to a different tab or be out of bounds.
**How to avoid:** Clear `tab_bar_hovered_tab` to `None` after any tab close operation. The next `CursorMoved` event will recalculate.
**Warning signs:** Wrong tab highlighted after closing a tab.

### Pitfall 3: Title Truncation Must Account for Close Button Space
**What goes wrong:** Title text overlaps with the close button glyph when both are visible.
**Why it happens:** The title truncation length is fixed at `MAX_TITLE_LEN` chars regardless of available space.
**How to avoid:** When a tab is hovered (showing close button), reduce the available text width by `CLOSE_BUTTON_SIZE + CLOSE_BUTTON_PADDING`. Use character-width estimation (cell_width * chars) to determine max chars.
**Warning signs:** Text visually collides with "x" glyph on hover.

### Pitfall 4: "+" Button Must Not Appear When No Tabs Exist
**What goes wrong:** Rendering a "+" button when tab_count is 0 (e.g., during shutdown sequence).
**Why it happens:** Tab bar is only shown when tab_count > 0 (checked in main.rs), but guard should also be in renderer.
**How to avoid:** Keep existing guard: `if tabs.is_empty() { return rects; }`.

### Pitfall 5: Middle-Click Close Must Also Understand New Layout
**What goes wrong:** Middle-click closes wrong tab after layout changes.
**Why it happens:** Middle-click uses `hit_test()` which now returns `TabHitResult` instead of `Option<usize>`.
**How to avoid:** Update middle-click handler to use new `TabHitResult` enum. Middle-click on "+" button should be ignored.

## Code Examples

### Current Click Handler That Must Be Updated
```rust
// main.rs:1817-1829 - Current left-click on tab bar
if (y as f32) < cell_h {
    ctx.mouse_left_pressed = false;
    let viewport_w = ctx.window.inner_size().width as f32;
    if let Some(tab_idx) = ctx.frame_renderer.tab_bar().hit_test(
        x as f32,
        ctx.session_mux.tab_count(),
        viewport_w,
    ) {
        ctx.session_mux.activate_tab(tab_idx);
        ctx.window.request_redraw();
    }
    return;
}
```

### New Tab Creation Pattern (from Ctrl+Shift+T, main.rs:1266-1286)
```rust
// This exact pattern must be reused for "+" button click
let cwd = ctx.session().status.cwd().to_string();
let session_id = ctx.session_mux.next_session_id();
let (cell_w, cell_h) = ctx.frame_renderer.cell_size();
let size = ctx.window.inner_size();
let session = create_session(
    &self.proxy,
    window_id,
    session_id,
    &self.config,
    Some(std::path::Path::new(&cwd)),
    cell_w, cell_h,
    size.width, size.height,
    1,
);
ctx.session_mux.add_tab(session);
ctx.window.request_redraw();
```

### Hover State Pattern (from scrollbar, main.rs:1679-1767)
```rust
// Scrollbar hover tracking in CursorMoved handler:
// 1. Compute which pane's scrollbar the mouse is over
// 2. Compare with stored state
// 3. If changed, update state and request redraw
if new_hovered != ctx.scrollbar_hovered_pane {
    ctx.scrollbar_hovered_pane = new_hovered;
    ctx.window.request_redraw();
}
```

### Build Methods Signature Changes Needed
```rust
// build_tab_rects needs hovered_tab to render close button background
pub fn build_tab_rects(
    &self,
    tabs: &[TabDisplayInfo],
    viewport_width: f32,
    hovered_tab: Option<usize>,
) -> Vec<RectInstance>

// build_tab_text needs hovered_tab to add "x" glyph and adjust title width
pub fn build_tab_text(
    &self,
    tabs: &[TabDisplayInfo],
    viewport_width: f32,
    hovered_tab: Option<usize>,
) -> Vec<TabLabel>

// hit_test returns enum instead of Option<usize>
pub fn hit_test(
    &self,
    x: f32,
    tab_count: usize,
    viewport_width: f32,
) -> Option<TabHitResult>
```

## Sizing Recommendations (Claude's Discretion)

| Parameter | Recommended Value | Rationale |
|-----------|------------------|-----------|
| `MIN_TAB_WIDTH` | 60.0 px | ~7-8 chars visible minimum, enough for truncated title |
| `NEW_TAB_BUTTON_WIDTH` | 32.0 px | Slightly wider than cell_height for comfortable click target |
| `CLOSE_BUTTON_SIZE` | 16.0 px | Half of typical cell_height (16-20px), comfortable click target |
| `CLOSE_BUTTON_PADDING` | 6.0 px | Right margin from tab edge |
| Hover highlight color | [70/255, 70/255, 70/255, 1.0] | Slightly lighter than active tab (50,50,50), visible but subtle |
| Close hover highlight | [80/255, 80/255, 80/255, 1.0] | Even lighter when mouse is directly over close button |
| "+" button highlight | [50/255, 50/255, 50/255, 1.0] | Same as active tab color, on hover |

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Equal-width tabs, no buttons | Variable-width with controls | This phase | Tab bar becomes fully mouse-interactive |
| `hit_test() -> Option<usize>` | `hit_test() -> Option<TabHitResult>` | This phase | All callers must handle enum |
| Fixed MAX_TITLE_LEN=20 | Dynamic based on available width | This phase | Titles fill available space |

## Open Questions

1. **"+" button hover highlight shape**
   - What we know: User wants "subtle circular/rounded background highlight" for both "+" and "x"
   - What's unclear: wgpu `RectInstance` renders axis-aligned rectangles, not circles
   - Recommendation: Use a small square rect (e.g., 20x20px). At this size, the difference between a square and circle is barely perceptible, especially with subtle coloring. This matches VS Code which also uses rects.

2. **Dynamic title truncation vs fixed**
   - What we know: Current truncation is char-count based (MAX_TITLE_LEN=20). With variable tab widths, pixel-based truncation would be more accurate.
   - What's unclear: glyphon measures text width after the fact; pre-truncating by pixel width requires knowing glyph advances.
   - Recommendation: Use character-based truncation with `available_width / cell_width` as the max chars. This is approximate but consistent with the existing approach and avoids complex text measurement.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in (#[cfg(test)] mod tests) + Criterion benches |
| Config file | Cargo.toml workspace |
| Quick run command | `cargo test -p glass_renderer tab_bar` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| TAB-01 | "+" button layout at correct position | unit | `cargo test -p glass_renderer tab_bar::tests::test_new_tab_button` | No - Wave 0 |
| TAB-02 | "x" close button rect on hovered tab only | unit | `cargo test -p glass_renderer tab_bar::tests::test_close_button_hovered` | No - Wave 0 |
| TAB-03 | hit_test returns TabHitResult::NewTabButton | unit | `cargo test -p glass_renderer tab_bar::tests::test_hit_new_tab_button` | No - Wave 0 |
| TAB-04 | hit_test returns TabHitResult::CloseButton | unit | `cargo test -p glass_renderer tab_bar::tests::test_hit_close_button` | No - Wave 0 |
| TAB-05 | Tab width compression with minimum | unit | `cargo test -p glass_renderer tab_bar::tests::test_min_tab_width` | No - Wave 0 |
| TAB-06 | Title truncation adjusts for close button | unit | `cargo test -p glass_renderer tab_bar::tests::test_title_truncation_with_close` | No - Wave 0 |
| TAB-07 | Existing tab click still works | unit | `cargo test -p glass_renderer tab_bar::tests::test_hit_test_correct_index` | Yes - existing |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_renderer tab_bar`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green + `cargo clippy --workspace -- -D warnings`

### Wave 0 Gaps
- [ ] New test cases for `TabHitResult` enum variants
- [ ] New test cases for variable-width tab layout
- [ ] New test cases for close button rect positioning
- [ ] Update existing tests to match new `hit_test()` return type

## Sources

### Primary (HIGH confidence)
- `crates/glass_renderer/src/tab_bar.rs` - Full source read, 360 lines
- `crates/glass_renderer/src/scrollbar.rs` - Hover state pattern reference
- `src/main.rs` lines 1260-1290, 1648-1770, 1807-1830, 2087-2113 - All tab/mouse handlers
- `crates/glass_renderer/src/frame.rs` - Tab bar rendering integration

### Secondary (MEDIUM confidence)
- VS Code tab bar behavior (general UI pattern knowledge from training data)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - No new dependencies, all existing crates
- Architecture: HIGH - Extends well-understood existing code with established patterns (scrollbar hover)
- Pitfalls: HIGH - Derived from direct code analysis of existing handlers

**Research date:** 2026-03-10
**Valid until:** 2026-04-10 (stable codebase, no external dependency changes)

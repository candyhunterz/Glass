# Phase 58: Agent Approval UI - Research

**Researched:** 2026-03-13
**Domain:** wgpu overlay rendering, winit keyboard event handling, Rust std::time for auto-dismiss, non-blocking UI overlays
**Confidence:** HIGH

## Summary

Phase 58 adds the user-facing approval layer for agent proposals. The backend (Phase 57 WorktreeManager) already exists: proposals live in `Processor.agent_proposal_worktrees: Vec<(AgentProposalData, Option<WorktreeHandle>)>` with the worktree fully created when `AgentProposal` arrives. The TODO comment at line 3614 of `src/main.rs` marks exactly where Phase 58 hooks in.

The rendering architecture is established. Every display element — status bar segments, search overlay, conflict banner, config error banner — uses the same pattern: a stateless renderer struct in `glass_renderer` that converts data into `Vec<RectInstance>` and `Vec<TextLabel>`, consumed by `draw_frame`. Phase 58 adds two new UI components following this exact pattern: a toast renderer and a proposal overlay renderer. Both must be display-only overlays that do NOT swallow keyboard input unconditionally — the terminal must remain interactive.

The keyboard handling architecture for non-swallowing overlays is well-understood: the search overlay (`Ctrl+Shift+F`) swallows all keystrokes while open because it needs text input. The approval overlay (`Ctrl+Shift+A`) does NOT need text input — it only needs two action keys (accept, reject). The correct pattern is to check for the action keys and pass everything else through to the PTY. This is a different modal contract than the search overlay.

**Primary recommendation:** Add `ProposalToastRenderer` and `ProposalOverlayRenderer` to `glass_renderer`, add `ProposalToast` state to `Processor`, add `agent_review_open: bool` to `Processor`, wire `Ctrl+Shift+A` toggle, wire `Ctrl+Shift+Y` / `Ctrl+Shift+N` accept/reject keys (which pass through when overlay is closed), extend `StatusLabel` with `agent_mode_text` and `proposal_count_text`, and update `draw_frame` to accept and render proposal UI data.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| AGTU-01 | Status bar shows agent mode indicator and pending proposal count | `StatusLabel` struct already has pattern for extra segments; add `agent_mode_text: Option<String>` and `proposal_count_text: Option<String>` fields; `build_status_text` in `status_bar.rs` assembles the label; caller in `main.rs` constructs values from `agent_runtime.config.mode` and `agent_proposal_worktrees.len()` |
| AGTU-02 | Toast notification appears for new proposals with auto-dismiss and keyboard shortcut hint | Add `ProposalToast { description: String, created_at: Instant }` to `Processor`; display as bottom-anchored 2-line rect + text; auto-dismiss by checking `toast.created_at.elapsed() >= Duration::from_secs(30)` during `RedrawRequested`; schedule continued redraws while toast is live by calling `request_redraw()` after checking elapsed |
| AGTU-03 | Review overlay (Ctrl+Shift+A) shows scrollable proposal list with diff preview | Add `agent_review_open: bool` and `proposal_review_selected: usize` to `Processor`; `ProposalOverlayRenderer` in `glass_renderer` renders backdrop + header + list rows + diff preview pane; diff text fetched from `WorktreeManager::generate_diff` on demand and cached in `proposal_diff_cache: Option<(usize, String)>` |
| AGTU-04 | Keyboard-driven approval: accept, reject, and dismiss actions on proposals | `Ctrl+Shift+Y` (accept) / `Ctrl+Shift+N` (reject) in the `Ctrl+Shift` match arm; when overlay is open, `ArrowUp`/`ArrowDown` navigate list; all other keys pass through to PTY (not swallowed); accept calls `wm.apply(handle)`, reject calls `wm.dismiss(handle)`, removes element from `agent_proposal_worktrees` |
| AGTU-05 | Terminal remains fully interactive while proposals are pending | Toast uses bottom-corner positioned rect that does NOT cover terminal input area significantly; overlay key handler passes unrecognized keys to PTY rather than returning early; MUST NOT set the same `OverlayAction::Handled` catch-all that the search overlay uses |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `std::time::Instant` | stdlib | Toast auto-dismiss timer, created when proposal arrives | Zero deps; already used for `CooldownTracker`, `key_start` perf timing throughout the codebase |
| `glass_renderer` (internal) | workspace | New renderer structs for toast and overlay | Existing pattern: every UI element is a stateless renderer struct; `SearchOverlayRenderer`, `ConflictOverlay`, `ConfigErrorOverlay` all prove the pattern |
| `glass_agent::WorktreeManager` | workspace | `generate_diff()` for diff preview, `apply()` / `dismiss()` for actions | Already instantiated in `Processor.worktree_manager`; Phase 57 implemented full API |
| `winit::keyboard` | 0.30 | Key matching for `Ctrl+Shift+A`, `Ctrl+Shift+Y`, `Ctrl+Shift+N` | Already imported; existing `modifiers.control_key() && modifiers.shift_key()` match pattern |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `alacritty_terminal::vte::ansi::Rgb` | 0.25.1 | Color type for text labels | Same as every other renderer in `glass_renderer` |
| `glyphon` | 0.10 | Text buffer rendering | Already in `frame.rs` rendering pipeline; new overlays follow same `Buffer::new` + `set_text` + `shape_until_scroll` flow |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Bottom-corner toast rect | Full-width banner (ConflictOverlay style) | Full-width banner is more intrusive; corner toast is less disruptive per AGTU-05 non-blocking requirement |
| Pass-through key handler for overlay | Swallow-all handler (search overlay style) | Swallow-all would block PTY input; approval overlay only needs two action keys so pass-through is correct |
| On-demand diff generation | Pre-generate diff on proposal arrival | Pre-generation wastes I/O if user never opens overlay; on-demand with a simple `Option<(selected_idx, String)>` cache is simpler |
| Dedicated `ProposalToast` struct in glass_mux | Toast state directly on `Processor` | No session scope needed; proposals are global (not per-session); keeping on Processor matches existing `agent_proposal_worktrees` placement |

**No new dependencies required.** All needed libraries are already in the workspace.

## Architecture Patterns

### Recommended Project Structure

New files:
```
crates/glass_renderer/src/proposal_toast_renderer.rs    # Toast rect + text generation
crates/glass_renderer/src/proposal_overlay_renderer.rs  # Full overlay rect + text generation
```

Modified files:
```
crates/glass_renderer/src/lib.rs           # pub mod + pub use new renderers
crates/glass_renderer/src/frame.rs         # draw_frame: add toast + overlay params + rendering
crates/glass_renderer/src/status_bar.rs    # StatusLabel: add agent_mode_text + proposal_count_text
src/main.rs                                # Processor state, key handlers, AgentProposal handler update
```

### Pattern 1: Stateless Renderer Struct (Established)

**What:** A struct holding only `cell_width` and `cell_height` with pure methods that take data and return `Vec<RectInstance>` + `Vec<TextLabel>`.

**When to use:** All overlay UI in Glass. `SearchOverlayRenderer`, `ConflictOverlay`, `ConfigErrorOverlay`, `StatusBarRenderer` all follow this exact pattern.

**Example (from existing `ConflictOverlay`):**
```rust
// Source: crates/glass_renderer/src/conflict_overlay.rs
pub struct ConflictOverlay {
    cell_width: f32,
    cell_height: f32,
}

impl ConflictOverlay {
    pub fn new(cell_width: f32, cell_height: f32) -> Self { ... }
    pub fn build_warning_rects(&self, viewport_w: f32, viewport_h: f32, ...) -> Vec<RectInstance> { ... }
    pub fn build_warning_text(&self, ...) -> Vec<ConflictTextLabel> { ... }
}
```

The proposal renderers follow this pattern exactly.

### Pattern 2: Proposal Toast State on Processor

**What:** A lightweight struct tracking the toast description, creation time, and which proposal index it refers to.

**When to use:** When a new `AgentProposal` arrives and `agent_proposal_worktrees` goes from 0 to N, or whenever a new proposal is added.

**Implementation:**
```rust
// In src/main.rs

struct ProposalToast {
    /// Short description from AgentProposalData.description
    description: String,
    /// Index into agent_proposal_worktrees for this toast
    proposal_idx: usize,
    /// When the toast was created (for 30-second auto-dismiss)
    created_at: std::time::Instant,
}

// On Processor:
active_toast: Option<ProposalToast>,
agent_review_open: bool,
proposal_review_selected: usize,
/// Cached diff for the currently selected proposal to avoid regenerating on every frame
proposal_diff_cache: Option<(usize, String)>,  // (selected_idx, diff_text)
```

### Pattern 3: Toast Auto-Dismiss via RedrawRequested Polling

**What:** During `RedrawRequested`, check if the active toast has expired; if so, clear it; if not, schedule another redraw so the toast eventually expires even with no user input.

**When to use:** Whenever a timed UI element needs auto-dismissal without a background thread.

**Implementation:**
```rust
// In RedrawRequested handler, BEFORE drawing:
if let Some(ref toast) = self.active_toast {
    if toast.created_at.elapsed() >= std::time::Duration::from_secs(30) {
        self.active_toast = None;
    } else {
        // Request another redraw to eventually expire the toast
        ctx.window.request_redraw();
    }
}
```

This is the same pattern used for the search debounce (`should_search` + `request_redraw()` at line 1115 of `main.rs`).

### Pattern 4: Non-Swallowing Overlay Key Handler

**What:** Check for specific overlay action keys; pass everything else to the PTY. Do NOT use the search overlay's catch-all `_ => OverlayAction::Handled`.

**When to use:** Review overlay that requires terminal interactivity (AGTU-05).

**Implementation:**
```rust
// In KeyboardInput handler, BEFORE the PTY forward:
if self.agent_review_open && modifiers.control_key() && modifiers.shift_key() {
    match &event.logical_key {
        Key::Character(c) if c.as_str().eq_ignore_ascii_case("a") => {
            // Toggle overlay closed
            self.agent_review_open = false;
            ctx.window.request_redraw();
            return;
        }
        Key::Character(c) if c.as_str().eq_ignore_ascii_case("y") => {
            // Accept selected proposal
            self.accept_selected_proposal(ctx);
            ctx.window.request_redraw();
            return;
        }
        Key::Character(c) if c.as_str().eq_ignore_ascii_case("n") => {
            // Reject selected proposal
            self.reject_selected_proposal(ctx);
            ctx.window.request_redraw();
            return;
        }
        Key::Named(NamedKey::ArrowUp) => {
            self.proposal_review_selected = self.proposal_review_selected.saturating_sub(1);
            self.proposal_diff_cache = None;  // invalidate cache
            ctx.window.request_redraw();
            return;
        }
        Key::Named(NamedKey::ArrowDown) => {
            let max = self.agent_proposal_worktrees.len().saturating_sub(1);
            self.proposal_review_selected = (self.proposal_review_selected + 1).min(max);
            self.proposal_diff_cache = None;  // invalidate cache
            ctx.window.request_redraw();
            return;
        }
        _ => {} // Fall through -- do NOT swallow; let PTY get the key
    }
}

// Ctrl+Shift+A toggle (works whether overlay is open or closed)
// Already in the existing `modifiers.control_key() && modifiers.shift_key()` block:
Key::Character(c) if c.as_str().eq_ignore_ascii_case("a") => {
    if self.agent_runtime.is_some() {
        self.agent_review_open = !self.agent_review_open;
        ctx.window.request_redraw();
        return;
    }
}
```

### Pattern 5: Status Bar Extension

**What:** Add two new `Option<String>` fields to `StatusLabel` for agent mode indicator and proposal count.

**When to use:** Status bar already has `coordination_text`, `agent_cost_text` — this extends the same right-side segment chain.

**Implementation:**
```rust
// In crates/glass_renderer/src/status_bar.rs StatusLabel:
pub agent_mode_text: Option<String>,       // e.g. "[agent: watch]"
pub proposal_count_text: Option<String>,   // e.g. "1 proposal"

// In build_status_text() signature:
pub fn build_status_text(
    &self,
    ...
    agent_mode_text: Option<&str>,
    proposal_count_text: Option<&str>,
    ...
) -> StatusLabel { ... }

// In main.rs, before draw_frame:
let agent_mode_text = self.agent_runtime.as_ref().map(|r| {
    format!("[agent: {}]", format!("{:?}", r.config.mode).to_lowercase())
});
let proposal_count_text = if !self.agent_proposal_worktrees.is_empty() {
    Some(format!("{} proposal(s)", self.agent_proposal_worktrees.len()))
} else {
    None
};
```

### Pattern 6: draw_frame Extension

**What:** Add `proposal_toast` and `proposal_overlay` parameters to `draw_frame`, following the established pattern of how `search_overlay`, `coordination_text`, and `agent_cost_text` were added.

**When to use:** Any time new UI data must flow from `Processor` into the rendering pipeline.

The `draw_frame` function already has 18 parameters — adding 2-3 more is acceptable given the established precedent. The alternative (a struct parameter) would require a larger refactor not warranted for this phase.

### Toast Layout

Toast is positioned at the bottom of the active pane, just above the status bar, right-aligned (less intrusive than full-width):

```
[status bar                                      ]
[       agent: "description text" [Ctrl+A: review]  <- toast, 2 cells above status bar
```

Concrete layout:
- Y: `viewport_height - status_bar_height - toast_height - padding`
- Width: ~60% of viewport width, right-aligned
- Height: 2 cell heights
- Background: dark teal/blue (0.05, 0.25, 0.35, 0.92)
- Line 1: proposal description (truncated)
- Line 2: "[Ctrl+Shift+A: review] [auto-dismiss in Xs]"

### Proposal Overlay Layout

Follows the `SearchOverlayRenderer` pattern — backdrop + inner panel:

```
+------------------------------------------+
| Agent Proposals (N pending)              |
|                                          |
| > [SELECTED] description 1               |
|   [        ] description 2               |
|                                          |
| Diff preview:                            |
| --- a/src/main.rs                        |
| +++ b/src/main.rs                        |
| -old line                                |
| +new line                                |
|                                          |
| Ctrl+Shift+Y: Accept  Ctrl+Shift+N: Reject  Ctrl+Shift+A: Close |
+------------------------------------------+
```

Concrete layout:
- Backdrop: full viewport, semi-transparent (0.03, 0.03, 0.03, 0.88)
- Panel: 80% width centered, full height minus tab/status bars
- Proposal list: top 30% of panel
- Diff preview: bottom 65% of panel (scrollable text)
- Footer hint: 5% (1 cell height)

### Anti-Patterns to Avoid

- **Swallowing all keys in proposal overlay:** Unlike the search overlay, the proposal overlay must NOT use `_ => OverlayAction::Handled`. Keys that aren't overlay actions must fall through to PTY forward. This is the core of AGTU-05.
- **Blocking the PTY thread for diff generation:** `generate_diff()` reads files from disk. Call it once and cache the result in `proposal_diff_cache`. Invalidate cache only when selection changes or a proposal is applied/dismissed.
- **Drawing the diff as one giant text buffer:** glyphon `Buffer::set_text` with thousands of characters will stall. Truncate diff preview to at most 50 lines before passing to the renderer.
- **Not clearing `proposal_diff_cache` on apply/dismiss:** After removing an element from `agent_proposal_worktrees`, the index shifts. Always `None` the cache after any mutation.
- **Not clamping `proposal_review_selected`:** When the last proposal is accepted/rejected, `proposal_review_selected` could be out of bounds. Clamp to `agent_proposal_worktrees.len().saturating_sub(1)` after removal.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Diff text generation | Custom line differ | `WorktreeManager::generate_diff()` (already wraps `diffy`) | Already implemented in Phase 57; produces standard unified diff |
| Timer for auto-dismiss | Background thread with sleep | `Instant::elapsed()` checked in `RedrawRequested` + `request_redraw()` | Background thread adds complexity; the winit render loop already re-enters on every dirty event |
| Key matching for `Ctrl+Shift` | Custom key code tables | `modifiers.control_key() && modifiers.shift_key()` + `Key::Character(c)` | Established pattern for all Glass shortcuts; same code handles Ctrl+Shift+F, Ctrl+Shift+Z, etc. |

**Key insight:** The hard parts (diff generation, worktree apply/dismiss, keyboard routing) are already solved. Phase 58 is almost purely additive UI work following well-worn Glass rendering patterns.

## Common Pitfalls

### Pitfall 1: Swallowing Keys in Review Overlay
**What goes wrong:** Terminal stops accepting input while review overlay is visible; user cannot type or run commands.
**Why it happens:** Copying the search overlay's `_ => OverlayAction::Handled` catch-all without understanding it was designed for text input capture.
**How to avoid:** The proposal overlay key handler must NOT have a catch-all `Handled` arm. Only named action keys (`A`, `Y`, `N`, `ArrowUp`, `ArrowDown`) return early; everything else falls through to the PTY forward path.
**Warning signs:** Test by opening the overlay and typing a character — it should appear in the terminal.

### Pitfall 2: Off-by-One After Proposal Removal
**What goes wrong:** After accepting/rejecting a proposal, `proposal_review_selected` points to wrong entry or out of bounds.
**Why it happens:** Removing element at index `i` from `agent_proposal_worktrees` shifts all subsequent elements.
**How to avoid:** After `agent_proposal_worktrees.remove(i)`, clamp `proposal_review_selected` and always `None` the `proposal_diff_cache`.

### Pitfall 3: Status Bar Text Width Collision
**What goes wrong:** Agent mode text and proposal count text collide with existing right-side segments (git info, coordination text, agent cost text).
**Why it happens:** The status bar positions text segments using char-width calculations. Adding more segments without adjusting offsets causes overlap.
**How to avoid:** Follow the established offset chain in `frame.rs` (lines 488-598): each new segment is positioned to the left of the previous one using `right_text_chars + gap + coord_text_chars + coord_gap + ...`. Add `agent_mode_text` and `proposal_count_text` to this chain.

### Pitfall 4: Toast Visible When No Agent Runtime
**What goes wrong:** A stale toast is shown after the agent is disabled or crashes.
**Why it happens:** `active_toast` is set when a proposal arrives but never cleared when `agent_runtime` is None.
**How to avoid:** Gate toast rendering on `self.agent_runtime.is_some()`, or clear `active_toast` in the `AgentCrashed` handler.

### Pitfall 5: Diff Preview Performance
**What goes wrong:** Frame rate drops when diff preview is visible for large files.
**Why it happens:** glyphon text rendering has non-trivial cost for large text; calling `generate_diff()` every frame reads files from disk.
**How to avoid:** Cache diff text in `proposal_diff_cache: Option<(usize, String)>`. Truncate to 50 lines max before passing to renderer. Only regenerate when `proposal_review_selected` changes.

### Pitfall 6: Toast Not Dismissing Without Input
**What goes wrong:** Toast stays visible indefinitely when user is idle (no keystrokes or terminal output).
**Why it happens:** The `Instant::elapsed()` check only runs in `RedrawRequested`, which only fires when something triggers a redraw.
**How to avoid:** When a toast is active and not expired, call `ctx.window.request_redraw()` unconditionally during `RedrawRequested`. This keeps the render loop spinning at ~60fps until the toast expires. (This is the same mechanism used by `should_search` + `request_redraw()` at line 1113-1115 of `main.rs`.)

## Code Examples

Verified patterns from existing codebase:

### Status Bar Extension Pattern
```rust
// Source: crates/glass_renderer/src/status_bar.rs (existing agent_cost_text field)
pub struct StatusLabel {
    pub left_text: String,
    pub right_text: Option<String>,
    pub center_text: Option<String>,
    pub coordination_text: Option<String>,
    pub agent_cost_text: Option<String>,
    // NEW for Phase 58:
    pub agent_mode_text: Option<String>,      // "[agent: watch]"
    pub proposal_count_text: Option<String>,  // "2 proposals"
    // ...colors...
}
```

### Keyboard Handler Non-Swallow Pattern
```rust
// Source: adapted from src/main.rs lines 1840-1960 (Ctrl+Shift block)
// The overlay check comes BEFORE the Ctrl+Shift block:
if self.agent_review_open {
    if modifiers.control_key() && modifiers.shift_key() {
        match &event.logical_key {
            Key::Character(c) if c.as_str().eq_ignore_ascii_case("y") => {
                // accept -- call apply, remove from vec, clear cache, request_redraw, return
            }
            Key::Character(c) if c.as_str().eq_ignore_ascii_case("n") => {
                // reject -- call dismiss, remove from vec, clear cache, request_redraw, return
            }
            _ => {} // CRITICAL: no return here -- fall through to PTY forward
        }
    }
    // ArrowUp/Down for list navigation:
    if !modifiers.control_key() {
        match &event.logical_key {
            Key::Named(NamedKey::ArrowUp) => { /* navigate */ return; }
            Key::Named(NamedKey::ArrowDown) => { /* navigate */ return; }
            _ => {} // fall through
        }
    }
}
```

### Toast Auto-Dismiss in RedrawRequested
```rust
// Source: pattern from lines 1112-1115 of src/main.rs (search debounce keepalive)
// In RedrawRequested handler, before computing render data:
if let Some(ref toast) = self.active_toast {
    if toast.created_at.elapsed() >= std::time::Duration::from_secs(30) {
        self.active_toast = None;
    } else {
        ctx.window.request_redraw(); // keep spinning until toast expires
    }
}
```

### RectInstance Toast Rect
```rust
// Source: pattern from crates/glass_renderer/src/conflict_overlay.rs
// In ProposalToastRenderer:
pub fn build_toast_rects(&self, viewport_w: f32, viewport_h: f32) -> Vec<RectInstance> {
    let toast_w = viewport_w * 0.6;
    let toast_h = self.cell_height * 2.5;
    let status_bar_h = self.cell_height;
    let x = viewport_w - toast_w - self.cell_width;
    let y = viewport_h - status_bar_h - toast_h - self.cell_height * 0.5;
    vec![RectInstance {
        pos: [x, y, toast_w, toast_h],
        color: [0.05, 0.25, 0.35, 0.92],
    }]
}
```

### draw_frame Signature Extension
```rust
// Source: src/main.rs lines 169-188 (existing draw_frame call site)
// After adding proposal params, draw_frame gains:
//   proposal_toast: Option<&ProposalToastRenderData>,
//   proposal_overlay: Option<&ProposalOverlayRenderData>,
// where the data structs are analogous to SearchOverlayRenderData
pub struct ProposalToastRenderData {
    pub description: String,
    pub remaining_secs: u64,
}

pub struct ProposalOverlayRenderData {
    pub proposals: Vec<(String, String)>,   // (description, action)
    pub selected: usize,
    pub diff_preview: String,               // truncated to 50 lines max
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Modal overlays capturing all input | Non-modal overlays with pass-through keys | Phase 58 design decision | Terminal stays interactive; aligns with "non-blocking" requirement |
| Diff generated per-frame | Diff cached in `proposal_diff_cache` | Phase 58 | Avoids file I/O on every render tick |

**Existing precedent:**
- `SearchOverlay` (glass_mux): modal, captures all keys — NOT the right model for Phase 58
- `ConflictOverlay`, `ConfigErrorOverlay` (glass_renderer): display-only, no key capture — closer to what Phase 58 needs, but Phase 58 also needs to react to specific keys

## Open Questions

1. **Toast position when multi-pane is active**
   - What we know: The toast should appear in the active pane; multi-pane rendering uses `draw_frame_offset` calls per pane
   - What's unclear: Whether the toast should be per-pane or window-global (above all panes)
   - Recommendation: Window-global (above all panes) is simpler and clearer — position relative to full window height/width, rendered in the single-pane path and once in multi-pane. Proposals are not per-session; they are global.

2. **Diff preview truncation threshold**
   - What we know: glyphon text rendering is not free; very large diffs will cause frame drops
   - What's unclear: Exact threshold before performance degrades
   - Recommendation: Hard cap at 50 lines. If diff exceeds 50 lines, append "... (N more lines)" hint. This is safe for common cases and avoids measurement.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in (`#[test]`) |
| Config file | none — inline `#[cfg(test)] mod tests` per crate |
| Quick run command | `cargo test --package glass_renderer 2>&1` |
| Full suite command | `cargo test --workspace 2>&1` |

### Phase Requirements -> Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| AGTU-01 | `build_status_text` includes agent_mode_text and proposal_count_text segments | unit | `cargo test --package glass_renderer status_bar 2>&1` | Wave 0 |
| AGTU-02 | `ProposalToastRenderer::build_toast_rects` returns correct position/color; `build_toast_text` returns 2 labels | unit | `cargo test --package glass_renderer proposal_toast 2>&1` | Wave 0 |
| AGTU-03 | `ProposalOverlayRenderer::build_overlay_rects` returns backdrop + panel + N rows; `build_overlay_text` returns header + diff labels | unit | `cargo test --package glass_renderer proposal_overlay 2>&1` | Wave 0 |
| AGTU-04 | `wm.apply()` + `wm.dismiss()` already tested in Phase 57; integration: accept removes from `agent_proposal_worktrees` | unit | `cargo test --workspace agent 2>&1` | Exists (Phase 57) |
| AGTU-05 | Non-swallow: overlay open + non-action key falls through to PTY | manual | Run Glass, open overlay, type character — must appear in terminal | N/A |

### Sampling Rate
- **Per task commit:** `cargo test --package glass_renderer 2>&1`
- **Per wave merge:** `cargo test --workspace 2>&1`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_renderer/src/proposal_toast_renderer.rs` — covers AGTU-02 (new file with `#[cfg(test)] mod tests`)
- [ ] `crates/glass_renderer/src/proposal_overlay_renderer.rs` — covers AGTU-03 (new file with `#[cfg(test)] mod tests`)
- [ ] Status bar unit tests extended for new fields — covers AGTU-01

## Sources

### Primary (HIGH confidence)
- Direct code reading: `crates/glass_renderer/src/status_bar.rs` — established `StatusLabel` pattern
- Direct code reading: `crates/glass_renderer/src/search_overlay_renderer.rs` — overlay rect+text pattern
- Direct code reading: `crates/glass_renderer/src/conflict_overlay.rs` — banner overlay pattern
- Direct code reading: `crates/glass_renderer/src/config_error_overlay.rs` — banner overlay pattern
- Direct code reading: `crates/glass_renderer/src/frame.rs` — `draw_frame` rendering pipeline
- Direct code reading: `src/main.rs` lines 1620-1704 — search overlay key handler (swallow pattern to avoid)
- Direct code reading: `src/main.rs` lines 1840-1960 — `Ctrl+Shift` key dispatch block
- Direct code reading: `src/main.rs` lines 1089-1115 — `RedrawRequested` + search debounce keepalive
- Direct code reading: `src/main.rs` lines 220-271 — `Processor` struct fields
- Direct code reading: `src/main.rs` lines 3566-3617 — `AgentProposal` event handler with TODO comment
- Direct code reading: `crates/glass_agent/src/worktree_manager.rs` — `generate_diff`, `apply`, `dismiss` API
- Direct code reading: `.planning/STATE.md` lines 60, 106-107 — locked decisions (non-modal, `agent_proposal_worktrees`)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all libraries already in use; no new deps needed
- Architecture: HIGH — all patterns observed directly in existing codebase; research is reading, not guessing
- Pitfalls: HIGH — key-swallow and off-by-one pitfalls identified from directly reading the search overlay implementation

**Research date:** 2026-03-13
**Valid until:** 2026-04-13 (stable codebase; patterns won't change)

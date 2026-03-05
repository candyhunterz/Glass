# Phase 8: Search Overlay - Research

**Researched:** 2026-03-05
**Domain:** GPU-rendered modal overlay with live search, text input, and scrollback navigation
**Confidence:** HIGH

## Summary

Phase 8 adds a search overlay to the running Glass terminal. The overlay is a modal UI element rendered on top of the terminal content, activated by Ctrl+Shift+F and dismissed by Escape. It includes a text input field, a scrollable result list rendered via the existing GPU pipeline, and keyboard navigation (arrow keys + Enter) to jump to command blocks in scrollback.

The key architectural insight is that this overlay must be built entirely within the existing wgpu + glyphon rendering pipeline -- there is no DOM, no egui, no retained-mode UI framework. The overlay is composed from the same primitives already used for block decorations and the status bar: `RectInstance` for backgrounds and `glyphon::Buffer` for text. Input handling is already centralized in `Processor::window_event()` with modifier detection via `self.modifiers`, and the `HistoryDb` is already available on `WindowContext`. The `QueryFilter` + `filtered_query()` from glass_history provides the search backend. The primary engineering challenge is managing overlay state (open/closed, input text, selected result, debounce timer) and compositing overlay rendering on top of the terminal frame without modifying the existing rendering pipeline's structure.

**Primary recommendation:** Add an `OverlayState` enum to `WindowContext` that tracks search mode. When active, intercept all keyboard input in main.rs before PTY forwarding, render overlay rects + text in `draw_frame()` as a new overlay layer after all existing content, and query `HistoryDb` via `filtered_query()` with debounced text input.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SRCH-01 | User can open search overlay with Ctrl+Shift+F | Ctrl+Shift key combo detection already exists in main.rs (lines 390-406). Add 'f' to the match. OverlayState toggle. |
| SRCH-02 | Incremental/live search results as user types | `filtered_query()` accepts `QueryFilter` with text field. Debounce via `Instant::now()` comparison (no async needed). |
| SRCH-03 | Arrow key navigation through results with enter to select | Arrow key detection exists in input.rs. When overlay active, intercept before PTY forwarding. Enter selects and scrolls via `Term::scroll_display()`. |
| SRCH-04 | Results displayed as structured blocks (command text, exit code, timestamp, preview) | `CommandRecord` has all fields. Render with glyphon `Buffer` + `RectInstance` using same pattern as block_renderer and status_bar. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| wgpu | 28.0.0 | GPU rendering | Already in project, all rendering goes through it |
| glyphon | 0.10.0 | Text rendering | Already in project, used for all text (grid, blocks, status) |
| winit | 0.30.13 | Window/input events | Already in project, keyboard events come through here |
| rusqlite | 0.38.0 | SQLite queries | Already in project, HistoryDb uses it |
| chrono | 0.4 | Timestamp formatting | Already in project, used in Phase 7 for relative time |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| alacritty_terminal | 0.25.1 | Terminal state + scroll | For `Term::scroll_display()` when navigating to a result |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Hand-rolled overlay | egui | Would add ~5 crate deps, requires wgpu integration layer, overkill for a single search panel |
| Debounce via timer | tokio channel | Already have tokio but main loop is sync winit; Instant-based check is simpler |
| Custom text input widget | iced/druid | Massive deps for one text field; raw character accumulation is sufficient |

## Architecture Patterns

### Recommended Project Structure
```
src/
  main.rs              # Add Ctrl+Shift+F handling + overlay state on WindowContext
  search_overlay.rs    # NEW: OverlayState, SearchOverlay struct, input handling logic

crates/glass_renderer/src/
  search_overlay_renderer.rs  # NEW: builds rects + text buffers for overlay
  frame.rs                    # Modify draw_frame() to accept optional overlay data
```

### Pattern 1: OverlayState on WindowContext
**What:** Add a `SearchOverlay` struct to `WindowContext` that holds all search modal state.
**When to use:** Always -- the overlay is per-window state.
**Example:**
```rust
/// Search overlay state, None when closed.
pub struct SearchOverlay {
    /// Current search text typed by the user.
    pub query: String,
    /// Cursor position within the query string (for future editing).
    pub cursor_pos: usize,
    /// Current search results from the database.
    pub results: Vec<CommandRecord>,
    /// Index of the currently highlighted result (0-based).
    pub selected: usize,
    /// Timestamp of last keystroke for debounce.
    pub last_keystroke: Instant,
    /// Whether a search is pending (debounce not yet elapsed).
    pub search_pending: bool,
}

// In WindowContext:
struct WindowContext {
    // ... existing fields ...
    search_overlay: Option<SearchOverlay>,
}
```

### Pattern 2: Input Interception
**What:** When `search_overlay.is_some()`, intercept ALL keyboard events before PTY forwarding.
**When to use:** Every key press while overlay is active.
**Example:**
```rust
// In window_event KeyboardInput handler, BEFORE existing Ctrl+Shift check:
if let Some(ref mut overlay) = ctx.search_overlay {
    match &event.logical_key {
        Key::Named(NamedKey::Escape) => {
            ctx.search_overlay = None;
            ctx.window.request_redraw();
            return;
        }
        Key::Named(NamedKey::ArrowUp) => {
            overlay.selected = overlay.selected.saturating_sub(1);
            ctx.window.request_redraw();
            return;
        }
        Key::Named(NamedKey::ArrowDown) => {
            if overlay.selected + 1 < overlay.results.len() {
                overlay.selected += 1;
            }
            ctx.window.request_redraw();
            return;
        }
        Key::Named(NamedKey::Enter) => {
            // Jump to selected result in scrollback
            // ... scroll logic ...
            ctx.search_overlay = None;
            ctx.window.request_redraw();
            return;
        }
        Key::Named(NamedKey::Backspace) => {
            overlay.query.pop();
            overlay.last_keystroke = Instant::now();
            overlay.search_pending = true;
            ctx.window.request_redraw();
            return;
        }
        Key::Character(c) => {
            overlay.query.push_str(c.as_str());
            overlay.last_keystroke = Instant::now();
            overlay.search_pending = true;
            ctx.window.request_redraw();
            return;
        }
        _ => { return; } // Swallow all other keys
    }
}
```

### Pattern 3: Debounced Search Execution
**What:** Execute the database query only after a debounce period (e.g., 150ms) since last keystroke, checked during `RedrawRequested`.
**When to use:** During `RedrawRequested` when `search_pending` is true.
**Example:**
```rust
// In RedrawRequested, before rendering:
if let Some(ref mut overlay) = ctx.search_overlay {
    if overlay.search_pending
        && overlay.last_keystroke.elapsed() >= Duration::from_millis(150)
    {
        overlay.search_pending = false;
        if !overlay.query.is_empty() {
            let filter = QueryFilter {
                text: Some(overlay.query.clone()),
                limit: 20,
                ..QueryFilter::default()
            };
            if let Some(ref db) = ctx.history_db {
                overlay.results = db.filtered_query(&filter).unwrap_or_default();
            }
        } else {
            overlay.results.clear();
        }
        overlay.selected = 0;
    }
    // If search is still pending (debounce not elapsed), schedule another redraw
    if overlay.search_pending {
        ctx.window.request_redraw();
    }
}
```

### Pattern 4: Overlay Rendering as Additional Layer
**What:** Render the overlay after all existing content (grid + blocks + status bar) in the same render pass, using semi-transparent background rect + text buffers.
**When to use:** In `draw_frame()` when overlay data is provided.
**Example:**
```rust
// In draw_frame(), after status bar rendering, before Phase B text areas:
// Add semi-transparent full-screen overlay backdrop
if let Some(ref overlay_data) = search_overlay {
    // Dimming backdrop
    rects.push(RectInstance {
        pos: [0.0, 0.0, w, h],
        color: [0.0, 0.0, 0.0, 0.7], // Semi-transparent black
    });
    // Search box background
    let box_y = cell_height; // One line from top
    let box_h = cell_height * 1.5;
    rects.push(RectInstance {
        pos: [cell_width * 2.0, box_y, w - cell_width * 4.0, box_h],
        color: [50.0/255.0, 50.0/255.0, 50.0/255.0, 1.0],
    });
    // Result rows...
}
```

### Pattern 5: Scroll-to-Block Navigation
**What:** When user selects a result, find the matching block by timestamp/command and scroll the terminal to show it.
**When to use:** On Enter key with a selected result.
**Example:**
```rust
// Find block matching the selected CommandRecord
// Option A: Use BlockManager to find block by approximate line
// Option B: Scroll to absolute position using display_offset
// The simplest approach: compute display_offset from block's prompt_start_line
fn scroll_to_block(term: &Arc<FairMutex<Term<EventProxy>>>, block: &Block) {
    let mut term = term.lock();
    let target_line = block.prompt_start_line;
    let history_size = term.grid().history_size();
    // display_offset = distance from bottom of history
    // target absolute line is `target_line`
    // display_offset = history_size - target_line (clamped)
    let offset = history_size.saturating_sub(target_line) as i32;
    term.scroll_display(Scroll::Delta(offset));
}
```

### Anti-Patterns to Avoid
- **Spawning async tasks for search:** The winit event loop is synchronous. SQLite FTS5 queries on local data are sub-millisecond. Keep everything synchronous -- no channels, no threads, no async for the search itself.
- **Modifying `encode_key()` for overlay state:** The input module is stateless by design. Overlay input interception belongs in `main.rs` where state is available, not in the encoder.
- **Using alpha blending for dimming backdrop:** The existing RectRenderer shader uses `alpha` in the color but the blend state may be set to opaque. Verify the wgpu blend state supports alpha. If not, use a nearly-opaque dark color (e.g., `rgba(10, 10, 10, 0.95)`) or add alpha blending to the pipeline.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Full-text search | Custom string matching | `filtered_query()` + FTS5 | Already built, handles escaping, ranking, filtering |
| Text rendering | Manual glyph layout | glyphon `Buffer` + `TextArea` | Already integrated, handles Unicode, font fallback |
| Rectangle rendering | Custom quad pipeline | `RectInstance` + `RectRenderer` | Already exists with instanced rendering |
| Scroll position calculation | Manual offset math | `Term::scroll_display(Scroll::Delta)` | Handles clamping, history bounds |
| Timestamp formatting | Manual epoch conversion | `chrono` + existing relative time pattern from Phase 7 | Already in codebase |

**Key insight:** Every rendering primitive needed for the overlay already exists. The work is composing them into a new overlay layout, not building new rendering infrastructure.

## Common Pitfalls

### Pitfall 1: Alpha Blending Not Enabled
**What goes wrong:** The dimming backdrop renders as fully opaque black, hiding terminal content entirely.
**Why it happens:** The existing `RectRenderer` pipeline may have `BlendState::REPLACE` (opaque), not `BlendState::ALPHA_BLENDING`.
**How to avoid:** Check the wgpu `BlendState` in `RectRenderer::new()`. If it's opaque, either (a) add alpha blending to the blend state, or (b) use a very dark but fully opaque color for the backdrop (simpler but less polished).
**Warning signs:** Overlay appears as solid black rectangle hiding everything.

### Pitfall 2: Input Forwarded to PTY While Overlay Open
**What goes wrong:** Typing in the search box sends characters to the shell, executing commands or corrupting the terminal.
**Why it happens:** The overlay input interception doesn't return early, falling through to the PTY forwarding code.
**How to avoid:** The overlay handler MUST `return` after processing every key event. Place the overlay check at the very top of the `KeyboardInput` handler, before any other key processing.
**Warning signs:** Shell prompt echoes search characters.

### Pitfall 3: Debounce Scheduling Without Redraw Request
**What goes wrong:** After typing, the search never executes because no redraw is requested to check the debounce timer.
**Why it happens:** winit only calls `RedrawRequested` when explicitly asked (or on OS-driven events). If the debounce timer expires without a redraw request, the search won't fire.
**How to avoid:** When `search_pending` is true, always call `ctx.window.request_redraw()` at the end of `RedrawRequested` to schedule another check.
**Warning signs:** Search results appear only after moving the mouse or pressing another key.

### Pitfall 4: Borrowing Conflicts in Overlay Rendering
**What goes wrong:** Can't borrow `WindowContext` mutably for overlay state while also borrowing `FrameRenderer` for rendering.
**Why it happens:** Overlay data needs to be extracted before passing to `draw_frame()`.
**How to avoid:** Extract overlay rendering data (query text, results, selected index) into a standalone struct before calling `draw_frame()`. Pass it as `Option<SearchOverlayData>` parameter.
**Warning signs:** Compilation errors about multiple mutable borrows.

### Pitfall 5: Scroll Target Calculation Off-By-One
**What goes wrong:** Pressing Enter on a search result scrolls to the wrong position (slightly above or below the command block).
**Why it happens:** `display_offset` is the distance from the bottom of scrollback, but block `prompt_start_line` is absolute. The conversion requires `history_size` which changes as more content is added.
**How to avoid:** Lock the term briefly to get current `history_size`, compute `display_offset = history_size - target_line`, then call `scroll_display(Scroll::Top)` followed by `scroll_display(Scroll::Delta(-offset))`. Alternatively, use the simpler approach of setting display_offset directly if the alacritty_terminal API supports it.
**Warning signs:** Search result navigation jumps to wrong part of scrollback.

### Pitfall 6: Search Overlay Persists After Window Resize
**What goes wrong:** The overlay layout uses stale dimensions after the window is resized.
**Why it happens:** Overlay positions are computed from surface dimensions which change on resize.
**How to avoid:** Overlay positions should be computed fresh each frame from current `width`/`height` in `draw_frame()`, not cached.
**Warning signs:** Overlay appears clipped or misaligned after resizing.

## Code Examples

### Opening the Overlay (Ctrl+Shift+F Interception)
```rust
// In main.rs, inside the Ctrl+Shift match block (line ~391):
Key::Character(c) if c.as_str().eq_ignore_ascii_case("f") => {
    // Toggle search overlay
    if ctx.search_overlay.is_some() {
        ctx.search_overlay = None;
    } else {
        ctx.search_overlay = Some(SearchOverlay::new());
    }
    ctx.window.request_redraw();
    return;
}
```

### Overlay Rendering Data Extraction
```rust
// Data struct passed to draw_frame (avoids borrow conflicts)
pub struct SearchOverlayData {
    pub query: String,
    pub results: Vec<SearchResultDisplay>,
    pub selected: usize,
}

pub struct SearchResultDisplay {
    pub command: String,
    pub exit_code: Option<i32>,
    pub timestamp: String,   // Pre-formatted
    pub output_preview: String, // First ~80 chars of output
}
```

### Overlay Rect + Text Layout
```rust
// SearchOverlayRenderer (in glass_renderer crate)
pub fn build_overlay_rects(
    overlay: &SearchOverlayData,
    cell_width: f32,
    cell_height: f32,
    viewport_width: f32,
    viewport_height: f32,
) -> Vec<RectInstance> {
    let mut rects = Vec::new();

    // Dimming backdrop
    rects.push(RectInstance {
        pos: [0.0, 0.0, viewport_width, viewport_height],
        color: [0.05, 0.05, 0.05, 0.85],
    });

    let margin = cell_width * 4.0;
    let panel_x = margin;
    let panel_w = viewport_width - margin * 2.0;
    let panel_y = cell_height * 2.0;

    // Search input background
    rects.push(RectInstance {
        pos: [panel_x, panel_y, panel_w, cell_height * 1.5],
        color: [0.22, 0.22, 0.22, 1.0],
    });

    // Result rows
    let results_y = panel_y + cell_height * 2.0;
    for (i, _result) in overlay.results.iter().enumerate().take(10) {
        let row_y = results_y + (i as f32) * cell_height * 2.5;
        let bg_color = if i == overlay.selected {
            [0.15, 0.30, 0.50, 1.0] // Highlighted
        } else {
            [0.12, 0.12, 0.12, 1.0] // Normal
        };
        rects.push(RectInstance {
            pos: [panel_x, row_y, panel_w, cell_height * 2.2],
            color: bg_color,
        });
    }

    rects
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Separate overlay window | In-window modal rendering | Standard practice | Simpler, no window management issues |
| Blocking search on keystroke | Debounced incremental search | Standard UX pattern | Responsive feel, no UI lag |
| Full re-render for overlay | Overlay as additional rects/text in same pass | Project convention | Consistent with block_renderer, status_bar patterns |

**Deprecated/outdated:**
- None relevant. The approach uses the existing project patterns consistently.

## Open Questions

1. **Alpha blending in RectRenderer**
   - What we know: The shader uses alpha in `color[3]`, but the blend state needs verification.
   - What's unclear: Whether `BlendState` is set to `ALPHA_BLENDING` or `REPLACE` in the existing pipeline.
   - Recommendation: Check `rect_renderer.rs` blend state during implementation. If opaque, add alpha blending support (one-line change in pipeline descriptor). Fallback: use opaque dark rects.

2. **Block-to-ScrollPosition mapping**
   - What we know: `BlockManager` stores `prompt_start_line` (absolute). `Term::scroll_display()` uses `Scroll::Delta(i32)`.
   - What's unclear: Exact formula to convert a target absolute line to the correct `Scroll::Delta` value given current display_offset and history_size.
   - Recommendation: Lock term, read current `display_offset` and `history_size`, compute delta = `(history_size - target_line) - display_offset`, then `scroll_display(Scroll::Delta(delta))`. Test empirically.

3. **Matching search results to BlockManager blocks**
   - What we know: `CommandRecord` has `started_at` (epoch). `Block` has `started_at` (Instant, not epoch -- monotonic clock).
   - What's unclear: There is no direct mapping between a `CommandRecord` and a `Block`. The `command_started_wall` was used for CommandRecord but not stored on Block.
   - Recommendation: For v1.1, match by iterating blocks and finding the closest by prompt line position or simply scroll to the approximate line. Exact record-to-block mapping can be deferred. Alternative: store the database row ID on Block when the CommandRecord is inserted.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (Rust built-in) |
| Config file | Cargo.toml (workspace) |
| Quick run command | `cargo test -p glass --lib -- search_overlay` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SRCH-01 | Ctrl+Shift+F toggles overlay state | unit | `cargo test -p glass --lib -- search_overlay` | No -- Wave 0 |
| SRCH-02 | Query text change triggers filtered_query with debounce | unit | `cargo test -p glass --lib -- search_overlay` | No -- Wave 0 |
| SRCH-03 | Arrow keys change selected index, Enter triggers scroll | unit | `cargo test -p glass --lib -- search_overlay` | No -- Wave 0 |
| SRCH-04 | SearchResultDisplay contains command, exit_code, timestamp, preview | unit | `cargo test -p glass --lib -- search_overlay` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass --lib`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `src/search_overlay.rs` -- SearchOverlay struct + state management unit tests
- [ ] `crates/glass_renderer/src/search_overlay_renderer.rs` -- layout computation tests
- [ ] Verify RectRenderer blend state supports alpha (manual inspection)

## Sources

### Primary (HIGH confidence)
- Project codebase: `src/main.rs`, `crates/glass_renderer/src/frame.rs`, `crates/glass_renderer/src/rect_renderer.rs`, `crates/glass_renderer/src/status_bar.rs`, `crates/glass_renderer/src/block_renderer.rs`
- Project codebase: `crates/glass_history/src/query.rs` (QueryFilter + filtered_query API)
- Project codebase: `crates/glass_history/src/db.rs` (CommandRecord, HistoryDb)
- Project codebase: `crates/glass_terminal/src/block_manager.rs` (Block, BlockManager, visible_blocks)
- Project codebase: `crates/glass_terminal/src/input.rs` (encode_key, keyboard handling patterns)
- Project codebase: `crates/glass_terminal/src/grid_snapshot.rs` (GridSnapshot, display_offset, history_size)

### Secondary (MEDIUM confidence)
- winit 0.30.x keyboard event model (Key::Character, Key::Named, ModifiersState) -- verified in project code
- wgpu BlendState API -- well-known, but specific project configuration needs verification

### Tertiary (LOW confidence)
- Exact scroll-to-position formula needs empirical validation during implementation

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- everything is already in the project
- Architecture: HIGH -- follows exact patterns from block_renderer and status_bar
- Pitfalls: HIGH -- identified from code reading, especially the input forwarding and borrow patterns
- Scroll navigation: MEDIUM -- the concept is clear but exact delta math needs implementation validation

**Research date:** 2026-03-05
**Valid until:** 2026-04-05 (stable -- no external dependencies changing)

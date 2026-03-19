# Performance Audit Implementation Plan (Branch 5 of 8)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate per-frame GPU buffer thrashing, add dirty-flag redraw skipping and frame rate throttling, batch database operations, and add benchmarks for the hot render path.

**Architecture:** Work impact-first: dirty flag + throttling first (biggest win, least risk), then glyph atlas trim (one-liner), then buffer caching + row-level dirty tracking (hardest), then quick wins (cursor lookup, batch DELETE, clone removal, SmallVec), then benchmarks last to measure improvements.

**Tech Stack:** Rust, wgpu, glyphon, smallvec (new dep), criterion (existing), rusqlite

**Branch:** `audit/performance` off `master`

**Spec:** `docs/superpowers/specs/2026-03-18-prelaunch-audit-fixes-design.md` Branch 5

---

### Task 1: Branch setup + dirty flag + frame throttling (PERF-R02, PERF-L01)

**Files:**
- Modify: `src/main.rs` (add dirty flag, last_redraw tracking, skip render when clean)
- Modify: `crates/glass_terminal/src/pty.rs` (set dirty flag after PTY reads)

**Rationale:** Currently every `RedrawRequested` event runs the full snapshot + build_cell_buffers + draw pipeline even when nothing has changed. The PTY thread sends `Event::Wakeup` after every read, which triggers `request_redraw()`. Adding a dirty flag lets the render path skip entirely when the terminal content has not changed, and frame throttling caps redraws during output floods.

- [ ] **Step 1: Create branch**

```bash
git checkout -b audit/performance master
```

- [ ] **Step 2: Add dirty flag to WindowContext**

In `src/main.rs`, find the `WindowContext` struct and add a field:

```rust
/// Set by PTY thread on new output, cleared after render.
dirty: std::sync::Arc<std::sync::atomic::AtomicBool>,
/// Timestamp of last completed redraw for frame throttling.
last_redraw: std::time::Instant,
```

Initialize in the `WindowContext` constructor:

```rust
dirty: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
last_redraw: std::time::Instant::now(),
```

- [ ] **Step 3: Set dirty flag from PTY thread**

In `crates/glass_terminal/src/pty.rs`, the `glass_pty_loop` function needs access to the dirty flag. Add a `dirty: Arc<AtomicBool>` parameter to `glass_pty_loop` (or to `spawn_pty` which creates it).

In `pty_read_with_scan` (around line 572), after the `event_proxy.send_event(Event::Wakeup)` call, set the dirty flag:

```rust
if parser.sync_bytes_count() < processed && processed > 0 {
    event_proxy.send_event(Event::Wakeup);
    dirty.store(true, std::sync::atomic::Ordering::Release);
}
```

Also set dirty on `PtyMsg::Resize` handling — resize always needs a redraw.

- [ ] **Step 4: Pass dirty flag from main.rs to PTY**

In `src/main.rs`, where `spawn_pty` is called, pass `ctx.dirty.clone()` as the new parameter. Update the `spawn_pty` signature and forward it to `glass_pty_loop`.

- [ ] **Step 5: Skip render when not dirty**

In the `WindowEvent::RedrawRequested` handler (`src/main.rs:2539`), add an early-out after the toast/search checks but before the snapshot:

```rust
// Frame throttling: cap at ~500fps during output floods
let now = std::time::Instant::now();
if now.duration_since(ctx.last_redraw) < std::time::Duration::from_millis(1) {
    // Too soon — schedule another redraw and skip this one
    ctx.window.request_redraw();
    return;
}

// Skip full render pipeline when nothing has changed
let is_dirty = ctx.dirty.swap(false, std::sync::atomic::Ordering::AcqRel);
if !is_dirty && !force_redraw {
    return;
}
```

Where `force_redraw` is true when:
- A toast is active (countdown needs updating)
- Search overlay is pending
- Orchestrator is active (activity line updates)
- Window was just resized
- Cursor blink timer fired

Compute `force_redraw` from the existing conditions already checked above this point in the handler.

- [ ] **Step 6: Update last_redraw after frame present**

After `frame.present()` (line ~3660):

```rust
ctx.last_redraw = std::time::Instant::now();
```

- [ ] **Step 7: Ensure non-PTY events still trigger redraw**

Several events already call `ctx.window.request_redraw()`. For these to work with the dirty flag, they must also set dirty. Add a helper:

```rust
impl WindowContext {
    fn mark_dirty(&self) {
        self.dirty.store(true, std::sync::atomic::Ordering::Release);
    }
}
```

Call `ctx.mark_dirty()` from:
- Resize handlers
- Focus change handlers
- Config reload handler
- Keyboard input handlers (cursor movement, scrolling)
- Tab switch / pane split handlers
- Search overlay open/close

This is conservative — we mark dirty on any user interaction that could change the display.

- [ ] **Step 8: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 9: Commit**

```bash
git add src/main.rs crates/glass_terminal/src/pty.rs
git commit -m "perf(PERF-R02/L01): add dirty flag and frame throttling to render loop

PTY thread sets AtomicBool on new output. RedrawRequested skips
the full snapshot+draw pipeline when nothing has changed. Frame
throttling caps effective rate at ~500fps during output floods."
```

---

### Task 2: Glyph atlas trim (PERF-G02)

**Files:**
- Modify: `src/main.rs:3660` (after `frame.present()`)

**Rationale:** `GlyphCache::trim()` and `FrameRenderer::trim()` exist but are never called. Glyphs accumulate in the atlas forever. One line fix.

- [ ] **Step 1: Add trim call after frame present**

In `src/main.rs`, after `frame.present()` (line ~3660), add:

```rust
frame.present();
ctx.frame_renderer.trim();
```

This calls `self.glyph_cache.trim()` which calls `self.atlas.trim()`, freeing GPU memory for glyphs no longer on screen.

- [ ] **Step 2: Verify trim is also called in multi-pane path**

Check the multi-pane render path (after `draw_multi_pane_frame`). If there is a separate `frame.present()` there, add `ctx.frame_renderer.trim()` after it as well.

- [ ] **Step 3: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "perf(PERF-G02): trim glyph atlas after each frame present

Calls FrameRenderer::trim() to free unused atlas GPU memory.
Prevents unbounded atlas growth during long sessions."
```

---

### Task 3: Generation counter + buffer caching + row-level dirty tracking (PERF-R01)

**Files:**
- Modify: `crates/glass_terminal/src/grid_snapshot.rs:65-74` (add generation counter to GridSnapshot)
- Modify: `crates/glass_terminal/src/grid_snapshot.rs:289` (increment generation in snapshot_term)
- Modify: `crates/glass_renderer/src/grid_renderer.rs:340-410` (cache buffers, row-level dirty)
- Modify: `crates/glass_renderer/src/frame.rs:37-55` (add cached buffer state to FrameRenderer)

**Rationale:** `build_cell_buffers` is the hottest function in the render loop. It creates a new `glyphon::Buffer` for every non-empty cell every frame. With a 200x50 terminal, that is ~10,000 Buffer allocations per frame. Row-level dirty tracking lets us skip reshaping rows whose content has not changed.

- [ ] **Step 1: Add generation counter to GridSnapshot**

In `crates/glass_terminal/src/grid_snapshot.rs`, add to the `GridSnapshot` struct:

```rust
pub struct GridSnapshot {
    pub cells: Vec<RenderedCell>,
    pub cursor: RenderableCursor,
    pub display_offset: usize,
    pub history_size: usize,
    pub mode: TermMode,
    pub columns: usize,
    pub screen_lines: usize,
    pub selection: Option<SelectionRange>,
    /// Monotonically increasing counter, bumped each time content changes.
    pub generation: u64,
}
```

- [ ] **Step 2: Add static generation counter and increment in snapshot_term**

In `crates/glass_terminal/src/grid_snapshot.rs`, add a static counter:

```rust
use std::sync::atomic::{AtomicU64, Ordering};

static SNAPSHOT_GENERATION: AtomicU64 = AtomicU64::new(0);
```

In `snapshot_term`, when constructing `GridSnapshot`:

```rust
GridSnapshot {
    cells,
    cursor: content.cursor,
    display_offset: content.display_offset,
    history_size: term.grid().history_size(),
    mode: content.mode,
    columns: term.columns(),
    screen_lines: term.screen_lines(),
    selection: content.selection,
    generation: SNAPSHOT_GENERATION.fetch_add(1, Ordering::Relaxed),
}
```

- [ ] **Step 3: Add row hash computation to GridSnapshot**

Add a method to compute a lightweight hash per row for dirty detection:

```rust
impl GridSnapshot {
    /// Compute a hash for each row's content. Used by the renderer to detect
    /// which rows changed between frames without full cell comparison.
    pub fn row_hashes(&self) -> Vec<u64> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hashes = vec![0u64; self.screen_lines];
        for cell in &self.cells {
            let row_idx = (cell.point.line.0 + self.display_offset as i32) as usize;
            if row_idx < self.screen_lines {
                let mut hasher = DefaultHasher::new();
                cell.c.hash(&mut hasher);
                cell.fg.hash(&mut hasher);
                cell.bg.hash(&mut hasher);
                cell.flags.bits().hash(&mut hasher);
                cell.point.column.0.hash(&mut hasher);
                hashes[row_idx] ^= hasher.finish();
            }
        }
        hashes
    }
}
```

Note: `Rgb` from alacritty_terminal may not implement `Hash`. If not, hash the individual r/g/b bytes manually: `cell.fg.r.hash(&mut hasher); cell.fg.g.hash(&mut hasher); cell.fg.b.hash(&mut hasher);`

- [ ] **Step 4: Add buffer cache to FrameRenderer**

In `crates/glass_renderer/src/frame.rs`, add cached state to `FrameRenderer`:

```rust
pub struct FrameRenderer {
    // ... existing fields ...

    /// Cached cell buffers from previous frame (keyed by row index)
    cached_row_buffers: Vec<Vec<Buffer>>,
    /// Cached cell positions from previous frame (keyed by row index)
    cached_row_positions: Vec<Vec<(usize, i32)>>,
    /// Row hashes from previous frame for dirty detection
    cached_row_hashes: Vec<u64>,
    /// Generation of the snapshot used for cached buffers
    cached_generation: u64,
}
```

Initialize all as empty vecs / 0 in `new()`.

- [ ] **Step 5: Add incremental build method to GridRenderer**

In `crates/glass_renderer/src/grid_renderer.rs`, add a new method alongside `build_cell_buffers`:

```rust
/// Incrementally rebuild cell buffers, only reshaping rows whose content changed.
///
/// Compares `new_hashes` against `old_hashes` to identify dirty rows.
/// Clean rows reuse their existing buffers. Dirty rows get new buffers.
/// Returns the flattened buffer and position vecs for the entire grid.
pub fn build_cell_buffers_incremental(
    &self,
    font_system: &mut FontSystem,
    snapshot: &GridSnapshot,
    new_hashes: &[u64],
    old_hashes: &[u64],
    cached_row_buffers: &mut Vec<Vec<Buffer>>,
    cached_row_positions: &mut Vec<Vec<(usize, i32)>>,
    out_buffers: &mut Vec<Buffer>,
    out_positions: &mut Vec<(usize, i32)>,
) {
    let num_rows = snapshot.screen_lines;

    // Resize caches if terminal dimensions changed
    if cached_row_buffers.len() != num_rows {
        cached_row_buffers.clear();
        cached_row_buffers.resize_with(num_rows, Vec::new);
        cached_row_positions.clear();
        cached_row_positions.resize_with(num_rows, Vec::new);
    }

    let physical_font_size = self.font_size * self.scale_factor;
    let metrics = Metrics::new(physical_font_size, self.cell_height);
    let line_offset = snapshot.display_offset as i32;
    let mut char_buf = [0u8; 4];

    // Group cells by row for dirty-row rebuilding
    // (Only rebuild rows where hash differs)
    for row_idx in 0..num_rows {
        let row_dirty = row_idx >= old_hashes.len()
            || row_idx >= new_hashes.len()
            || old_hashes[row_idx] != new_hashes[row_idx];

        if !row_dirty {
            // Reuse cached buffers for this row
            out_buffers.extend(cached_row_buffers[row_idx].drain(..));
            out_positions.extend(cached_row_positions[row_idx].drain(..));
            // Note: drain moves ownership; we will re-populate the cache
            // from out_buffers after the frame is rendered.
            continue;
        }

        // Rebuild this row: collect cells, build buffers
        cached_row_buffers[row_idx].clear();
        cached_row_positions[row_idx].clear();

        for cell in &snapshot.cells {
            let cell_row = (cell.point.line.0 + line_offset) as usize;
            if cell_row != row_idx {
                continue;
            }
            if cell.flags.intersects(
                Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER,
            ) {
                continue;
            }
            if cell.c == ' ' && cell.zerowidth.is_empty() {
                continue;
            }

            let is_wide = cell.flags.contains(Flags::WIDE_CHAR);
            let buf_width = if is_wide {
                self.cell_width * 2.0
            } else {
                self.cell_width
            };

            let mut buffer = Buffer::new(font_system, metrics);
            buffer.set_size(font_system, Some(buf_width), Some(self.cell_height));
            buffer.set_monospace_width(font_system, Some(buf_width));

            let mut attrs = Attrs::new()
                .family(Family::Name(&self.font_family))
                .color(GlyphonColor::rgba(cell.fg.r, cell.fg.g, cell.fg.b, 255));
            if cell.flags.contains(Flags::BOLD) {
                attrs = attrs.weight(Weight::BOLD);
            }
            if cell.flags.contains(Flags::ITALIC) {
                attrs = attrs.style(Style::Italic);
            }

            if cell.zerowidth.is_empty() {
                let s = cell.c.encode_utf8(&mut char_buf);
                buffer.set_text(font_system, s, &attrs, Shaping::Advanced, None);
            } else {
                let mut text = String::with_capacity(4 + cell.zerowidth.len() * 4);
                text.push(cell.c);
                for &zw in &cell.zerowidth {
                    text.push(zw);
                }
                buffer.set_text(font_system, &text, &attrs, Shaping::Advanced, None);
            }

            buffer.shape_until_scroll(font_system, false);

            let col = cell.point.column.0;
            let line = cell.point.line.0 + line_offset;

            cached_row_buffers[row_idx].push(buffer);
            cached_row_positions[row_idx].push((col, line));
        }

        // Copy into output
        // (We keep a copy in cache and move another into output)
        // Actually, glyphon Buffers are not Clone. We need to rebuild
        // from cache each frame. Alternative: keep buffers in cache,
        // borrow for TextArea construction.
    }
}
```

**Important design note:** `glyphon::Buffer` does not implement `Clone`, so we cannot keep a copy in the cache and also pass one to the output vec. The practical approach is:

1. Store buffers directly in the row cache (not in `text_buffers`)
2. Build `TextArea` references directly from the row cache
3. Only rebuild rows that are dirty

This requires modifying `build_cell_text_areas_offset` to accept `&[Vec<Buffer>]` (row-grouped) instead of `&[Buffer]` (flat). Alternatively, keep the flat layout but track row boundaries.

The simplest correct approach:
- Keep `text_buffers` and `cell_positions` as the canonical storage (flat vecs, as today)
- On each frame, only clear and rebuild dirty rows in-place
- Track per-row start/end indices into the flat vecs

```rust
/// Row boundary tracking for incremental updates
struct RowRange {
    buf_start: usize,
    buf_end: usize,
}
```

This is complex. The implementer should:
1. Start with the generation-skip optimization (skip `build_cell_buffers` entirely when generation matches and display_offset unchanged)
2. Then add row-level dirty tracking as a second pass if time permits

- [ ] **Step 6: Wire generation-skip into draw_frame**

In `crates/glass_renderer/src/frame.rs`, in the `draw_frame` method, before the `build_cell_buffers` call (line ~353):

```rust
// Skip rebuilding cell buffers if snapshot has not changed
let snap_changed = snapshot.generation != self.cached_generation
    || snapshot.display_offset != self.cached_display_offset;

if snap_changed {
    self.text_buffers.clear();
    self.cell_positions.clear();
    self.grid_renderer.build_cell_buffers(
        &mut self.glyph_cache.font_system,
        snapshot,
        &mut self.text_buffers,
        &mut self.cell_positions,
    );
    self.cached_generation = snapshot.generation;
    self.cached_display_offset = snapshot.display_offset;
}
// Else: reuse self.text_buffers and self.cell_positions from previous frame
```

Add `cached_display_offset: usize` field to `FrameRenderer`, initialized to `usize::MAX`.

Apply the same pattern in `draw_multi_pane_frame` — but since multi-pane has multiple snapshots, track per-pane generations (or simply always rebuild for multi-pane in this pass).

- [ ] **Step 7: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 8: Commit**

```bash
git add crates/glass_terminal/src/grid_snapshot.rs crates/glass_renderer/src/grid_renderer.rs crates/glass_renderer/src/frame.rs
git commit -m "perf(PERF-R01): add generation counter and skip buffer rebuild on unchanged frames

GridSnapshot gets a monotonic generation counter. FrameRenderer
tracks the last-rendered generation and skips build_cell_buffers
entirely when the snapshot has not changed. Eliminates ~10K Buffer
allocations per idle frame."
```

---

### Task 4: Quick wins — cursor lookup, batch DELETE, clone removal, SmallVec (PERF-R03, PERF-S02, PERF-A01, PERF-M02)

**Files:**
- Modify: `crates/glass_renderer/src/grid_renderer.rs:137-140` (PERF-R03 cursor wide-char)
- Modify: `crates/glass_history/src/retention.rs:29-107` (PERF-S02 batch DELETE)
- Modify: `src/main.rs:2628-2633` (PERF-A01 visible blocks clone)
- Modify: `crates/glass_terminal/src/grid_snapshot.rs:61,310` (PERF-M02 SmallVec zerowidth)
- Modify: `Cargo.toml` (add smallvec dependency)
- Modify: `crates/glass_terminal/Cargo.toml` (add smallvec dependency)

- [ ] **Step 1: Fix PERF-R03 — direct cursor wide-char lookup**

In `crates/glass_renderer/src/grid_renderer.rs:137-140`, replace the linear scan:

```rust
// BEFORE:
let cursor_is_wide = snapshot
    .cells
    .iter()
    .any(|c| c.point == cursor.point && c.flags.contains(Flags::WIDE_CHAR));

// AFTER:
// Direct index lookup: cursor row/column maps to a predictable cell index.
// The cells vec is ordered by (line, column) from display_iter, so the cursor
// cell is at approximately (cursor_line * columns + cursor_col).
let cursor_line = cursor.point.line.0 + line_offset;
let cursor_col = cursor.point.column.0;
let cursor_is_wide = if cursor_line >= 0 {
    let idx = cursor_line as usize * snapshot.columns + cursor_col;
    snapshot.cells.get(idx).map_or(false, |c| {
        c.point == cursor.point && c.flags.contains(Flags::WIDE_CHAR)
    })
} else {
    false
};
```

**Caution:** The cells vec may not be perfectly indexed by `line * columns + col` if the display_iter skips cells or if there's scrollback involved. Test with CJK characters to verify. If direct indexing is unreliable, use `binary_search_by` on the sorted cells vec instead — still O(log n) vs O(n):

```rust
let cursor_is_wide = snapshot
    .cells
    .binary_search_by(|c| {
        c.point.line.cmp(&cursor.point.line)
            .then(c.point.column.cmp(&cursor.point.column))
    })
    .ok()
    .map_or(false, |idx| snapshot.cells[idx].flags.contains(Flags::WIDE_CHAR));
```

- [ ] **Step 2: Fix PERF-S02 — batch DELETE in retention.rs**

In `crates/glass_history/src/retention.rs`, replace the 5 nested for-loops (lines 29-55 and 81-107) with batch DELETE using `WHERE command_id IN (...)`:

```rust
// BEFORE (5 loops, N statements each):
for &id in &ids_to_delete {
    tx.execute("DELETE FROM pipe_stages WHERE command_id = ?1", params![id])?;
}
for &id in &ids_to_delete {
    tx.execute("DELETE FROM output_records WHERE command_id = ?1", params![id])?;
}
// ... 3 more loops

// AFTER (5 statements total):
if !ids_to_delete.is_empty() {
    let tx = conn.unchecked_transaction()?;
    let placeholders: String = ids_to_delete
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");

    let delete_from = |table: &str, col: &str| -> String {
        format!("DELETE FROM {table} WHERE {col} IN ({placeholders})")
    };

    let id_params: Vec<&dyn rusqlite::types::ToSql> = ids_to_delete
        .iter()
        .map(|id| id as &dyn rusqlite::types::ToSql)
        .collect();

    tx.execute(&delete_from("pipe_stages", "command_id"), &*id_params)?;
    tx.execute(&delete_from("output_records", "command_id"), &*id_params)?;
    tx.execute(&delete_from("command_output_records", "command_id"), &*id_params)?;
    tx.execute(&delete_from("commands_fts", "rowid"), &*id_params)?;
    tx.execute(&delete_from("commands", "id"), &*id_params)?;
    tx.commit()?;
    total_deleted += ids_to_delete.len() as u64;
}
```

Apply the same refactor to the second deletion block (size-based pruning, lines 81-107).

**Note on rusqlite parameter binding:** `rusqlite` does not support passing a `Vec` directly as `IN (?)`. The approach above builds the placeholder string dynamically. An alternative is to use a temporary table or `rusqlite::params_from_iter`. Check which approach compiles cleanly:

```rust
use rusqlite::params_from_iter;
tx.execute(
    &format!("DELETE FROM pipe_stages WHERE command_id IN ({})", placeholders),
    params_from_iter(ids_to_delete.iter()),
)?;
```

- [ ] **Step 3: Fix PERF-A01 — avoid deep clone of visible blocks**

In `src/main.rs:2628-2633`, the current code clones all visible Block structs:

```rust
// BEFORE:
let vb: Vec<_> = session
    .block_manager
    .visible_blocks(viewport_abs_start, snapshot.screen_lines)
    .into_iter()
    .cloned()
    .collect();
```

The Block struct contains `Vec<CapturedStage>`, `Vec<String>`, and `Option<String>` fields that are expensive to clone. The `draw_frame` method takes `&[&Block]`, so references should suffice.

The issue is borrow lifetime: `session` is borrowed from `ctx.session_mux` which is also needed for other things. The fix is to restructure the borrow scope so the block references outlive the draw call:

```rust
// AFTER: collect references without cloning
let vb: Vec<&Block> = session
    .block_manager
    .visible_blocks(viewport_abs_start, snapshot.screen_lines);
```

If the borrow checker prevents this due to conflicting borrows on `ctx`, extract the needed data (snapshot, visible blocks, search overlay, status) in a single borrow scope and then pass to draw_frame. The snapshot is already extracted separately. The key change is to keep `vb` as `Vec<&Block>` and ensure the session borrow does not conflict with the `draw_frame` call.

If borrow restructuring proves too invasive, a lighter alternative is to only clone the fields that `draw_frame` actually reads from Block (prompt_start_line, output_start_line, output_end_line, exit_code, state, has_snapshot, pipeline_stages, soi_summary, soi_severity) into a lighter struct. But try the zero-clone approach first.

- [ ] **Step 4: Fix PERF-M02 — SmallVec for zerowidth chars**

Add `smallvec` dependency:

In root `Cargo.toml`:
```toml
smallvec = "1.13"
```

In `crates/glass_terminal/Cargo.toml`:
```toml
smallvec = { workspace = true }
```

In `crates/glass_terminal/src/grid_snapshot.rs`, change `RenderedCell`:

```rust
// BEFORE (line 61):
pub zerowidth: Vec<char>,

// AFTER:
pub zerowidth: smallvec::SmallVec<[char; 0]>,
```

The `[char; 0]` type parameter means SmallVec uses zero inline capacity — it behaves like Vec but avoids heap allocation for the empty case (which is >99% of cells). When empty, SmallVec<[char; 0]> is just a pointer-sized value with no heap allocation, vs Vec which always has 3 words (ptr, len, cap).

In `snapshot_term` (line 310), update the construction:

```rust
// BEFORE:
zerowidth: cell.zerowidth().map(|z| z.to_vec()).unwrap_or_default(),

// AFTER:
zerowidth: cell.zerowidth().map(|z| z.iter().copied().collect()).unwrap_or_default(),
```

Update all code that references `.zerowidth` — the SmallVec API is compatible with Vec for iteration and `.is_empty()` checks, so most call sites need no change.

- [ ] **Step 5: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/glass_terminal/Cargo.toml crates/glass_terminal/src/grid_snapshot.rs crates/glass_renderer/src/grid_renderer.rs crates/glass_history/src/retention.rs src/main.rs
git commit -m "perf: quick wins — cursor lookup, batch DELETE, clone removal, SmallVec

PERF-R03: direct index or binary search for cursor wide-char check
PERF-S02: batch DELETE in retention pruning (5N statements -> 5)
PERF-A01: avoid deep clone of visible blocks in render path
PERF-M02: SmallVec<[char; 0]> for zerowidth (zero-alloc for 99% of cells)"
```

---

### Task 5: Overlay buffer caching (PERF-R04)

**Files:**
- Modify: `crates/glass_renderer/src/frame.rs:373+` (overlay buffer construction in draw_frame)

**Rationale:** Overlay buffers (status bar, tab bar labels, block labels, search overlay text) are rebuilt with `Buffer::new()` every frame even when their text content has not changed. There are 15+ Buffer::new calls in the overlay section. Caching with text comparison avoids reshaping unchanged overlays.

- [ ] **Step 1: Add cached overlay text tracking**

In `crates/glass_renderer/src/frame.rs`, add fields to `FrameRenderer`:

```rust
/// Cached status bar text for overlay buffer reuse
cached_status_text: String,
/// Cached tab titles for overlay buffer reuse
cached_tab_titles: Vec<String>,
/// Whether overlay buffers need rebuilding
overlay_dirty: bool,
```

- [ ] **Step 2: Compare overlay text before rebuilding**

In `draw_frame`, before the overlay buffer section (line ~373), compare the current status text and tab titles against cached values. If identical, skip the overlay buffer rebuild and reuse `self.overlay_buffers` from the previous frame.

```rust
let current_status_text = /* extract from status_state */;
let current_tab_titles: Vec<String> = tabs.iter().map(|t| t.title.clone()).collect();

let overlay_changed = current_status_text != self.cached_status_text
    || current_tab_titles != self.cached_tab_titles
    || /* search overlay changed */;

if overlay_changed {
    self.overlay_buffers.clear();
    // ... existing overlay buffer construction ...
    self.cached_status_text = current_status_text;
    self.cached_tab_titles = current_tab_titles;
} else {
    // Reuse self.overlay_buffers from previous frame
}
```

**Note:** This is a partial optimization. The block label overlays depend on visible blocks which change frequently. The status bar and tab bar are the best candidates for caching since they change rarely. The implementer should start with status bar + tab bar caching and leave block label overlays for a future pass.

- [ ] **Step 3: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add crates/glass_renderer/src/frame.rs
git commit -m "perf(PERF-R04): cache overlay buffers when status/tab text unchanged

Skip reshaping status bar and tab bar overlay buffers when their
text content matches the previous frame."
```

---

### Task 6: Block memory management (PERF-M01)

**Files:**
- Modify: `crates/glass_terminal/src/block_manager.rs` (evict pipeline_stages from distant blocks)

**Rationale:** Blocks accumulate `pipeline_stages: Vec<CapturedStage>` and `pipeline_stage_commands: Vec<String>` forever. For long-running sessions with thousands of commands, this is unbounded memory growth. Blocks far from the viewport do not need this data.

- [ ] **Step 1: Add eviction method to BlockManager**

In `crates/glass_terminal/src/block_manager.rs`, add:

```rust
/// Evict heavy data from blocks far from the viewport to limit memory usage.
///
/// Blocks more than `distance_threshold` lines from the viewport have their
/// pipeline_stages and pipeline_stage_commands cleared. The metadata
/// (line numbers, exit code, state) is retained.
pub fn evict_distant_blocks(&mut self, viewport_start: usize, viewport_lines: usize, distance_threshold: usize) {
    let viewport_end = viewport_start.saturating_add(viewport_lines);

    for block in &mut self.blocks {
        let block_start = block.prompt_start_line;
        let block_end = block
            .output_end_line
            .or(block.output_start_line)
            .unwrap_or(block.command_start_line);

        // Check if block is far from viewport
        let distance = if block_end < viewport_start {
            viewport_start - block_end
        } else if block_start > viewport_end {
            block_start - viewport_end
        } else {
            0 // Block overlaps viewport
        };

        if distance > distance_threshold {
            if !block.pipeline_stages.is_empty() {
                block.pipeline_stages.clear();
                block.pipeline_stages.shrink_to_fit();
            }
            if !block.pipeline_stage_commands.is_empty() {
                block.pipeline_stage_commands.clear();
                block.pipeline_stage_commands.shrink_to_fit();
            }
        }
    }
}
```

- [ ] **Step 2: Call eviction from the render loop**

In `src/main.rs`, in the `RedrawRequested` handler, after computing `viewport_abs_start`, call:

```rust
session.block_manager.evict_distant_blocks(viewport_abs_start, snapshot.screen_lines, 1000);
```

Use 1000 lines as the threshold — blocks more than ~1000 lines away from the viewport have their pipeline data cleared.

- [ ] **Step 3: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add crates/glass_terminal/src/block_manager.rs src/main.rs
git commit -m "perf(PERF-M01): evict pipeline data from blocks far from viewport

Blocks >1000 lines from the viewport have pipeline_stages and
pipeline_stage_commands cleared to bound memory growth in long sessions."
```

---

### Task 7: Snapshot watcher debounce (PERF-F01)

**Files:**
- Modify: `crates/glass_snapshot/src/watcher.rs:39` (add debounce or lazy watching)

**Rationale:** `RecursiveMode::Recursive` on the project root generates a flood of events for large directory trees (node_modules, target/, .git/). The existing ignore filter helps, but the OS watcher still fires for every event. Adding coalescing in the drain method reduces processing overhead.

- [ ] **Step 1: Add debounce coalescing to drain_events**

In `crates/glass_snapshot/src/watcher.rs`, the `drain_events` method already deduplicates paths. Add a 50ms coalescing window:

```rust
/// Drain all pending events with 50ms coalescing window.
///
/// After receiving the first event, waits up to 50ms for additional
/// events to arrive, then processes them all in one batch.
pub fn drain_events(&self) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Drain all immediately available events
    while let Ok(event) = self.rx.try_recv() {
        if let Ok(event) = event {
            for path in event.paths {
                if !self.ignore.is_ignored(&path) && seen.insert(path.clone()) {
                    paths.push(path);
                }
            }
        }
    }

    // If we got events, wait briefly for more to coalesce
    if !paths.is_empty() {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(50);
        while std::time::Instant::now() < deadline {
            match self.rx.try_recv() {
                Ok(Ok(event)) => {
                    for path in event.paths {
                        if !self.ignore.is_ignored(&path) && seen.insert(path.clone()) {
                            paths.push(path);
                        }
                    }
                }
                _ => break,
            }
        }
    }

    paths
}
```

If `drain_events` is called from a hot loop, the 50ms wait could add latency. Check the call site to ensure it is called on a background thread or timer, not on the render thread.

- [ ] **Step 2: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add crates/glass_snapshot/src/watcher.rs
git commit -m "perf(PERF-F01): add 50ms debounce coalescing for snapshot watcher events

Batches rapid filesystem events into single processing passes.
Reduces overhead in large project directories."
```

---

### Task 8: Configurable scrollback (PERF-M03)

**Files:**
- Modify: `crates/glass_core/src/config.rs` (add terminal section with scrollback field)
- Modify: `crates/glass_terminal/src/pty.rs` (read scrollback from config)

**Rationale:** The terminal scrollback buffer size is currently hardcoded (alacritty_terminal default of 10,000 lines). For large output sessions, this can consume significant memory. A config field lets users tune the tradeoff.

- [ ] **Step 1: Add TerminalSection to config**

In `crates/glass_core/src/config.rs`, add:

```rust
/// Terminal-related configuration in the `[terminal]` TOML section.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TerminalSection {
    /// Maximum scrollback buffer size in lines. Default 10000.
    #[serde(default = "default_scrollback")]
    pub scrollback: u32,
}

fn default_scrollback() -> u32 {
    10_000
}
```

Add to `GlassConfig`:

```rust
pub terminal: Option<TerminalSection>,
```

- [ ] **Step 2: Wire config value to PTY spawn**

In `crates/glass_terminal/src/pty.rs`, where the terminal is created (in `spawn_pty`), read the scrollback value from config and pass it to `alacritty_terminal::Term::new()` via `Config::scrollback_lines`.

Check how `Term::new` accepts scrollback configuration — it is part of `alacritty_terminal::term::Config`. Set `config.scrolling.history` to the configured value.

- [ ] **Step 3: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add crates/glass_core/src/config.rs crates/glass_terminal/src/pty.rs
git commit -m "perf(PERF-M03): add configurable scrollback via [terminal] config section

New [terminal] scrollback field (default 10000) controls the
alacritty_terminal scrollback buffer size."
```

---

### Task 9: Benchmarks (PERF-B01)

**Files:**
- Modify: `benches/perf_benchmarks.rs` (add new Criterion benchmarks)
- Modify: `crates/glass_terminal/src/grid_snapshot.rs` (ensure snapshot_term is benchmarkable)

**Rationale:** The existing benchmarks cover `resolve_color`, `osc_scan`, cold start, and output processing. The hot render path (`build_cell_buffers`, `snapshot_term`, `build_rects`) has no benchmarks. Adding them lets us measure the impact of the optimizations in this branch and catch regressions.

- [ ] **Step 1: Add snapshot_term benchmark**

In `benches/perf_benchmarks.rs`, add a benchmark that creates a mock terminal and snapshots it. Since `alacritty_terminal::Term` requires an event proxy, create a minimal one:

```rust
fn bench_snapshot_term(c: &mut Criterion) {
    use glass_terminal::{snapshot_term, DefaultColors, EventProxy};
    use alacritty_terminal::term::Config as TermConfig;
    use alacritty_terminal::term::Term;

    // Create a minimal terminal for benchmarking
    let size = alacritty_terminal::term::SizeInfo::new(80.0, 24.0, 8.0, 16.0, 0.0, 0.0, false);
    let proxy = EventProxy::new(/* ... */);
    let term = Term::new(TermConfig::default(), &size, proxy);
    let defaults = DefaultColors::default();

    c.bench_function("snapshot_term_80x24", |b| {
        b.iter(|| {
            snapshot_term(black_box(&term), black_box(&defaults))
        })
    });
}
```

**Note:** EventProxy requires a window_id and event_loop_proxy. The benchmarkability depends on whether these can be constructed outside a running event loop. If not, the benchmark may need to use a mock proxy or skip this specific bench.

- [ ] **Step 2: Add build_cell_buffers benchmark**

```rust
fn bench_build_cell_buffers(c: &mut Criterion) {
    use glass_renderer::grid_renderer::GridRenderer;
    use glass_terminal::{GridSnapshot, RenderedCell};
    use glyphon::FontSystem;

    let mut font_system = FontSystem::new();
    let renderer = GridRenderer::new(&mut font_system, "Consolas", 14.0, 1.0);

    // Create a realistic 80x24 snapshot with mixed content
    let mut cells = Vec::new();
    for line in 0..24i32 {
        for col in 0..80usize {
            cells.push(RenderedCell {
                point: alacritty_terminal::grid::Point {
                    line: alacritty_terminal::index::Line(line),
                    column: alacritty_terminal::index::Column(col),
                },
                c: if (line + col as i32) % 3 == 0 { ' ' } else { 'a' },
                fg: alacritty_terminal::vte::ansi::Rgb { r: 204, g: 204, b: 204 },
                bg: alacritty_terminal::vte::ansi::Rgb { r: 26, g: 26, b: 26 },
                flags: Flags::empty(),
                zerowidth: Default::default(),
            });
        }
    }

    let snapshot = GridSnapshot {
        cells,
        cursor: /* default cursor */,
        display_offset: 0,
        history_size: 0,
        mode: alacritty_terminal::term::TermMode::empty(),
        columns: 80,
        screen_lines: 24,
        selection: None,
        generation: 0,
    };

    let mut group = c.benchmark_group("render_pipeline");
    group.bench_function("build_cell_buffers_80x24", |b| {
        let mut buffers = Vec::new();
        let mut positions = Vec::new();
        b.iter(|| {
            buffers.clear();
            positions.clear();
            renderer.build_cell_buffers(
                &mut font_system,
                black_box(&snapshot),
                &mut buffers,
                &mut positions,
            );
        })
    });

    group.finish();
}
```

**Note:** The exact construction depends on which types are publicly exported. The implementer may need to add `pub` visibility to some types or use builder patterns. If `GridSnapshot` construction requires private types, add a `GridSnapshot::for_benchmark()` constructor gated behind `#[cfg(test)]` or a `bench` feature flag.

- [ ] **Step 3: Add build_rects benchmark**

Create a benchmark for `build_rects` in the `GridRenderer` if it is publicly accessible. This covers the rect instance buffer construction which is the other hot path.

- [ ] **Step 4: Update criterion_group**

```rust
criterion_group!(
    benches,
    bench_resolve_color,
    bench_osc_scanner,
    bench_cold_start,
    bench_input_processing,
    bench_snapshot_term,
    bench_build_cell_buffers,
);
criterion_main!(benches);
```

- [ ] **Step 5: Build and run benchmarks**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
cargo bench 2>&1
```

Record baseline numbers for the new benchmarks.

- [ ] **Step 6: Commit**

```bash
git add benches/perf_benchmarks.rs
git commit -m "perf(PERF-B01): add Criterion benchmarks for render hot path

Add benchmarks for snapshot_term and build_cell_buffers.
Provides baseline measurements for render pipeline optimizations."
```

---

### Task 10: Final verification and clippy

- [ ] **Step 1: Run clippy**

```bash
cargo clippy --workspace -- -D warnings 2>&1
```

Fix any warnings.

- [ ] **Step 2: Run fmt**

```bash
cargo fmt --all -- --check 2>&1
```

Fix any formatting issues.

- [ ] **Step 3: Run full test suite**

```bash
cargo test --workspace 2>&1
```

- [ ] **Step 4: Run benchmarks to verify improvements**

```bash
cargo bench 2>&1
```

Compare `build_cell_buffers` and `snapshot_term` numbers against the pre-optimization baseline (if available from Task 9).

- [ ] **Step 5: Commit any cleanup**

```bash
git add -A
git commit -m "chore: clippy and fmt cleanup for audit/performance branch"
```

- [ ] **Step 6: Summary — verify all items addressed**

Check off against the spec:
- [x] PERF-R01: Generation counter + buffer cache skip (Task 3)
- [x] PERF-R02: Dirty flag redraw tracking (Task 1)
- [x] PERF-L01: Frame rate throttling (Task 1)
- [x] PERF-M01: Block memory eviction (Task 6)
- [x] PERF-G02: Glyph atlas trim (Task 2)
- [x] PERF-M02: SmallVec zerowidth (Task 4)
- [x] PERF-A01: Visible blocks clone removal (Task 4)
- [x] PERF-R03: Cursor wide-char direct lookup (Task 4)
- [x] PERF-R04: Overlay buffer caching (Task 5)
- [x] PERF-S02: Batch DELETE in retention (Task 4)
- [x] PERF-B01: Hot-path benchmarks (Task 9)
- [x] PERF-F01: Snapshot watcher debounce (Task 7)
- [x] PERF-M03: Configurable scrollback (Task 8)

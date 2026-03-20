# Glass Performance Audit

**Date:** 2026-03-18
**Scope:** Prelaunch readiness -- render pipeline, memory, GPU resources, SQLite, PTY I/O, startup, threading, allocations
**Auditor:** Claude Opus 4.6

## Summary

Glass has a well-structured architecture with several good performance practices already in place: event-driven redraw (no constant polling), parallel font/GPU init, WAL-mode SQLite, content-addressed blob deduplication, and reusable buffer storage. However, there are significant performance issues that would be noticeable to users, particularly the per-cell Buffer allocation on every frame, lack of dirty-flag tracking for redraws, and unbounded block history in memory. The most critical items are in the render hot path.

---

## 1. Render Pipeline

### PERF-R01: Per-Cell glyphon Buffer Allocation Every Frame [Critical]

**Files:** `crates/glass_renderer/src/grid_renderer.rs` lines 340-409, `crates/glass_renderer/src/frame.rs` lines 352-360

**Description:** `build_cell_buffers()` creates a new `glyphon::Buffer` for every non-empty terminal cell on every frame. For a typical 120x40 terminal with ~50% non-space cells, that is ~2,400 Buffer allocations per frame. Each Buffer involves heap allocation, font shaping (`shape_until_scroll`), and layout computation. The buffers in `self.text_buffers` are cleared and rebuilt every frame via `.clear()` -- the Vec capacity is reused but the Buffer objects themselves are recreated.

**Impact:** This is the single largest performance bottleneck. Font shaping is CPU-intensive. At 60fps on a 120x40 terminal, that is ~144,000 Buffer allocs/shapes per second. Users will notice frame drops during rapid scrolling or large output.

**Recommendation:** Implement a dirty-flag system. Cache the shaped buffers between frames and only rebuild when the grid content actually changes. Track a generation counter on the GridSnapshot -- if it matches the previous frame, skip `build_cell_buffers` entirely. Alternatively, use a row-level dirty tracker so only changed rows are reshaped.

### PERF-R02: No Dirty-Flag Redraw Tracking [High]

**Files:** `src/main.rs` lines 2539-2635 (RedrawRequested handler)

**Description:** Every `RedrawRequested` event performs the full rendering pipeline: snapshot the terminal grid, build all rect instances, build all text buffers, upload to GPU, and present. There is no check for whether the terminal content has actually changed since the last frame. The PTY thread sends `Event::Wakeup` on every read, which triggers `request_redraw()`. During rapid output (e.g. `cat huge_file`), this means the full pipeline runs on every PTY read chunk.

**Impact:** Unnecessary GPU work and CPU utilization when content hasn't changed. During output floods, the render pipeline becomes the bottleneck rather than gracefully dropping frames.

**Recommendation:** Add a dirty flag set by the PTY thread when content changes and cleared after rendering. When the flag is not set, skip the render pipeline entirely. Consider frame-rate limiting (e.g., cap redraws at 60fps) to avoid overwhelming the GPU during output floods.

### PERF-R03: Linear Scan for Cursor Wide-Char Detection [Medium]

**File:** `crates/glass_renderer/src/grid_renderer.rs` lines 137-140

**Description:** `build_rects()` uses `snapshot.cells.iter().any(|c| c.point == cursor.point && ...)` to check if the cursor is on a wide character. This is an O(n) scan over all visible cells (typically 4,800 cells for 120x40) on every frame.

**Impact:** Small but unnecessary per-frame cost. The cursor position is known, so a direct index lookup would be O(1).

**Recommendation:** Use `snapshot.cells.get(cursor_line * columns + cursor_col)` for direct access, or store a `cursor_is_wide` flag in GridSnapshot during snapshot creation.

### PERF-R04: Overlay Buffer Rebuilds Every Frame [Medium]

**File:** `crates/glass_renderer/src/frame.rs` lines 373-1050+

**Description:** Status bar, tab bar, block labels, and other overlay text buffers are rebuilt via `Buffer::new()` and `shape_until_scroll()` on every frame, even when their text content hasn't changed. The status bar CWD, git info, and agent cost text are typically static between frames.

**Impact:** Each overlay buffer requires font shaping. With 5-10 overlay buffers per frame, this adds ~10-20 unnecessary shape operations per frame.

**Recommendation:** Cache overlay buffers and only rebuild when their input text changes. Store the previous text content and compare before reshaping.

### PERF-R05: Duplicate draw_frame / draw_multi_pane_frame Code [Low]

**File:** `crates/glass_renderer/src/frame.rs` -- `draw_frame` (~1000 lines) and `draw_multi_pane_frame` (~800 lines)

**Description:** The single-pane and multi-pane rendering paths are largely duplicated, including the entire overlay buffer building logic. Both create the same status bar, tab bar, search overlay, and proposal overlay buffers independently.

**Impact:** No direct runtime impact, but increases maintenance burden and risk of divergent behavior. More code to compile also increases build times.

**Recommendation:** Refactor to share overlay buffer construction between the two paths. The single-pane path could be a special case of multi-pane with one pane.

---

## 2. Memory Usage

### PERF-M01: Unbounded Block History in Memory [High]

**File:** `crates/glass_terminal/src/block_manager.rs`

**Description:** The `BlockManager` stores all `Block` objects in a `Vec<Block>`. Each Block contains a `Vec<CapturedStage>` for pipeline data and multiple `Vec<String>` for stage commands. While there is a `MAX_BLOCKS` constant for pruning, each Block's pipeline data (captured stage output) can hold significant amounts of data. The `visible_blocks()` method filters by viewport, but all blocks remain in memory.

**Impact:** For long-running sessions with many commands (especially pipeline-heavy workflows), memory usage grows without bound. Each Block with pipeline stages can hold kilobytes of captured output.

**Recommendation:** Evict pipeline stage data from completed blocks that are far from the viewport. Keep the Block metadata but drop the `pipeline_stages` Vec contents for blocks more than N screens away.

### PERF-M02: GridSnapshot Allocates Vec per Cell for zerowidth [Medium]

**File:** `crates/glass_terminal/src/grid_snapshot.rs` line 310

**Description:** `snapshot_term()` calls `cell.zerowidth().map(|z| z.to_vec()).unwrap_or_default()` for every cell. Since >99% of cells have no zero-width combining characters, this creates an empty `Vec<char>` (24 bytes on 64-bit) for almost every cell. For a 120x40 grid, that is 4,800 empty Vec allocations per snapshot.

**Impact:** ~115KB of unnecessary heap allocations per frame (4,800 x 24 bytes) for the zero-width fields alone, all immediately discarded on the next frame.

**Recommendation:** Use `SmallVec<[char; 0]>` or a custom enum (`None` / `One(char)` / `Many(Vec<char>)`) to avoid heap allocation for the common empty case. Alternatively, only store `zerowidth` for cells that actually have combining characters.

### PERF-M03: Scrollback Buffer Fixed at 10,000 Lines [Low]

**File:** `crates/glass_terminal/src/pty.rs` line 210

**Description:** `scrolling_history: 10_000` is hardcoded in the Term config. This is reasonable, but not configurable. For users who run commands with massive output, 10K lines may be too little; for memory-constrained environments, it may be too much.

**Impact:** Low -- 10K lines is a reasonable default. The alacritty_terminal grid manages this efficiently.

**Recommendation:** Make scrollback configurable via `config.toml` (e.g., `[terminal] scrollback = 10000`).

---

## 3. Large Output Handling

### PERF-L01: No Frame Rate Throttling During Output Floods [High]

**Files:** `crates/glass_terminal/src/pty.rs` lines 302-444, `src/main.rs` RedrawRequested handler

**Description:** The PTY read loop sends `Event::Wakeup` after every processed chunk (line 573: `event_proxy.send_event(Event::Wakeup)`). Each Wakeup triggers `request_redraw()` in main.rs. During `cat huge_file` or `cargo build` with verbose output, the PTY can produce thousands of Wakeup events per second, each causing a full frame render.

The PTY reader has `MAX_LOCKED_READ` (64KB) to limit how long it holds the terminal lock, but there is no rate limiting on the Wakeup -> redraw path. The VSync present mode (`AutoVsync`) provides some backpressure, but the CPU work of building the frame still happens even if the GPU drops it.

**Impact:** During output floods, CPU usage spikes to 100% on the render thread. The UI feels sluggish because each frame does full Buffer rebuilds (PERF-R01) for content that changes faster than the display can show.

**Recommendation:** Coalesce Wakeup events: after processing a Wakeup, set a "redraw pending" flag and delay the actual redraw by 1-2ms. If another Wakeup arrives before the delay expires, reset the timer. This caps frame rate at ~500fps while still being responsive.

### PERF-L02: Output Capture Buffer Scales Linearly [Low]

**File:** `crates/glass_terminal/src/output_capture.rs` lines 30-36

**Description:** The `OutputBuffer` pre-allocates `min(max_bytes, 65536)` and then grows up to `max_bytes` via `extend_from_slice`. With the default `max_output_capture_kb` config, this is bounded. The `check_alt_screen` method scans every PTY read with `data.windows()`, which is O(n) per read.

**Impact:** Low -- the buffer is bounded and the alt-screen scan is efficient for typical read sizes (usually <64KB).

**Recommendation:** No immediate action needed. The current design is sound.

---

## 4. GPU Resource Management

### PERF-G01: Instance Buffer Growth Without Shrink [Low]

**File:** `crates/glass_renderer/src/rect_renderer.rs` lines 224-233

**Description:** The instance buffer grows via `next_power_of_two()` when needed but never shrinks. If a frame with an unusually large number of rects (e.g., many pipeline stages visible) causes growth to 8192 instances, that memory remains allocated even when subsequent frames only need 100 instances.

**Impact:** Low -- GPU buffer memory is small (8192 instances x 32 bytes = 256KB). Not a practical concern.

**Recommendation:** No action needed. The current approach avoids buffer churn.

### PERF-G02: Glyph Atlas Never Trimmed [Medium]

**Files:** `crates/glass_renderer/src/glyph_cache.rs` lines 81-83, `crates/glass_renderer/src/frame.rs` lines 2599-2602, `src/main.rs`

**Description:** `GlyphCache::trim()` exists and `FrameRenderer::trim()` wraps it, but **neither is ever called from main.rs**. The glyph atlas texture grows as new glyphs are rasterized but stale glyphs are never reclaimed. Over a long session with diverse terminal output (especially CJK, emoji, or special characters), the atlas texture will grow monotonically.

**Impact:** GPU memory leak proportional to the number of unique glyphs encountered over the session lifetime. For typical usage with ASCII-heavy output this is small, but for sessions involving multiple scripts/languages, the atlas could grow to tens of megabytes.

**Recommendation:** Call `ctx.frame_renderer.trim()` after `frame.present()` in the RedrawRequested handler.

### PERF-G03: Shader Compiled at Init Time [Low -- Already Handled]

**File:** `crates/glass_renderer/src/rect_renderer.rs` lines 40-95

**Description:** The WGSL shader is compiled during `RectRenderer::new()`, which happens during startup. Good -- no runtime shader compilation.

**Impact:** N/A -- properly implemented.

---

## 5. SQLite Performance

### PERF-S01: WAL Mode and Indexes [Low -- Already Handled]

**Files:** `crates/glass_history/src/db.rs` lines 57-61, `crates/glass_snapshot/src/db.rs` lines 24-29

**Description:** Both databases use WAL mode, `PRAGMA synchronous = NORMAL`, `busy_timeout = 5000`, and foreign keys enabled. Proper indexes exist on `started_at`, `cwd`, `command_id`, and FTS5 for full-text search.

**Impact:** N/A -- well configured.

### PERF-S02: Retention Pruning Does Individual DELETE Per Record [Medium]

**File:** `crates/glass_history/src/retention.rs` lines 29-56, 81-107

**Description:** Both age-based and size-based pruning iterate over IDs and execute individual DELETE statements per record across 5 tables (pipe_stages, output_records, command_output_records, commands_fts, commands). For N records to prune, this is 5N DELETE statements. While wrapped in a transaction, this is O(N) round-trips to the SQLite engine.

**Impact:** For routine pruning of a few records, this is fine. But if a user has accumulated thousands of records past the age limit, the first prune could be slow (seconds).

**Recommendation:** Use batch DELETE with `WHERE command_id IN (...)` instead of per-row deletes. This would reduce 5N statements to 5 statements.

### PERF-S03: No Connection Pooling for History DB [Low]

**File:** `crates/glass_history/src/db.rs` line 44

**Description:** `HistoryDb` holds a single `Connection`. All operations are single-threaded through this connection. There is no connection pool.

**Impact:** Low -- the history DB is only accessed from the main thread (insert on command finish, search on overlay open). WAL mode allows concurrent reads, but with a single connection this doesn't matter.

**Recommendation:** No action needed for the current single-window architecture. If multi-window support is added, consider using a connection pool.

---

## 6. File Watching

### PERF-F01: Recursive Directory Watch for Snapshots [Medium]

**File:** `crates/glass_snapshot/src/watcher.rs` line 39

**Description:** `FsWatcher::new()` watches the entire CWD recursively (`RecursiveMode::Recursive`). On Windows this uses ReadDirectoryChangesW, on Linux inotify. For large project trees (e.g., a monorepo with 100K+ files), the initial watch setup can be slow and consume significant OS resources.

**Impact:** On large projects, the initial watcher setup during command execution may cause a noticeable delay. The `IgnoreRules` filter helps reduce event volume but doesn't reduce the watch count.

**Recommendation:** Consider lazy watching: only set up watchers on directories that the command parser identifies as potentially affected. Or use a debounced polling approach for large trees.

### PERF-F02: Config Watcher Thread Blocks Forever [Low -- Correct Design]

**File:** `crates/glass_core/src/config_watcher.rs` lines 93-96

**Description:** The config watcher thread uses `std::thread::park()` in an infinite loop to keep the watcher alive. The notify event handler runs in a callback. Non-recursive watch on the parent directory only.

**Impact:** N/A -- this is the correct pattern for keeping a notify watcher alive. One thread for config watching is acceptable.

---

## 7. PTY I/O

### PERF-P01: PTY Read Buffer is 1MB [Low -- Correct Design]

**File:** `crates/glass_terminal/src/pty.rs` line 24

**Description:** `READ_BUFFER_SIZE = 0x10_0000` (1MB) is the stack-allocated read buffer. This matches alacritty's design.

**Impact:** N/A -- 1MB stack allocation is fine for a dedicated PTY reader thread.

### PERF-P02: OscScanner Re-scans Entire Buffer [Low]

**File:** `crates/glass_terminal/src/pty.rs` lines 505-524

**Description:** `scanner.scan(data)` processes the entire PTY read buffer for OSC sequences before the VTE parser sees it. This is a byte-by-byte scan.

**Impact:** Low -- the scan is simple (looking for `\x1b]133;`) and runs on the dedicated PTY thread. Benchmarked at nanosecond scale (see `bench_osc_scanner`).

---

## 8. Startup Time

### PERF-T01: Parallel Font/GPU Init [Low -- Already Optimized]

**File:** `src/main.rs` lines 2327-2335

**Description:** `FontSystem::new()` (system font enumeration, ~35ms) is spawned on a separate thread while `GlassRenderer::new()` (wgpu adapter/device creation) runs async. The font thread is joined after GPU init completes.

**Impact:** N/A -- good optimization, saves ~35ms on cold start.

### PERF-T02: Snapshot Pruning Runs on Startup Background Thread [Low -- Already Optimized]

**File:** `src/main.rs` lines 2384-2415

**Description:** Snapshot pruning is spawned on a background thread during `resumed()`, so it doesn't block the first frame.

**Impact:** N/A -- correct approach.

### PERF-T03: DB Migrations Run Synchronously on Open [Low]

**Files:** `crates/glass_history/src/db.rs` lines 101-165, `crates/glass_snapshot/src/db.rs` lines 60-66

**Description:** Schema migrations run during `HistoryDb::open()` and `SnapshotDb::open()`. These are called during session creation, which is on the main thread path. Migrations check `PRAGMA user_version` and conditionally run `CREATE TABLE IF NOT EXISTS` / `ALTER TABLE` statements.

**Impact:** Low -- migrations are idempotent and very fast when the schema is already up to date (just a version check). Only on first-ever-run or schema upgrade would there be noticeable work.

---

## 9. Benchmark Coverage

### PERF-B01: Critical Paths Not Benchmarked [Medium]

**File:** `benches/perf_benchmarks.rs`

**Description:** Current benchmarks cover:
- `resolve_color` (color resolution)
- `osc_scan` (OSC scanner)
- `process_startup` (cold start via `--help`)
- `process_output_50kb` (output processing)

Missing benchmarks for the hot path:
- `build_cell_buffers` -- the most expensive per-frame operation (PERF-R01)
- `snapshot_term` -- terminal grid snapshot extraction
- `build_rects` -- rectangle instance generation
- `draw_frame` end-to-end (would need headless GPU)
- History DB insert/search latency
- FTS5 query performance at scale

**Impact:** Without benchmarks on the render hot path, it is impossible to measure the impact of optimizations or detect regressions.

**Recommendation:** Add Criterion benchmarks for `build_cell_buffers`, `snapshot_term`, and `build_rects` using synthetic GridSnapshot data. These don't require a GPU context.

---

## 10. Threading Model

### PERF-TH01: No Tokio Runtime for GUI Path [Low -- Correct Design]

**File:** `src/main.rs`

**Description:** The GUI path does not use Tokio. The main thread runs the winit event loop. PTY I/O runs on a dedicated `std::thread`. Config watching, snapshot pruning, update checking, and coordination polling each run on dedicated `std::thread`s. The only async usage is `pollster::block_on(GlassRenderer::new())` at startup for wgpu initialization.

**Impact:** N/A -- this is the correct design. Tokio would add unnecessary overhead for a GUI application.

### PERF-TH02: FairMutex Lock Contention on Terminal Grid [Medium]

**Files:** `crates/glass_terminal/src/pty.rs` lines 493-500, `src/main.rs` line 2617

**Description:** The terminal grid is protected by `FairMutex<Term<EventProxy>>`. The PTY reader thread locks it for parsing, and the main thread locks it for `snapshot_term()` during rendering. The PTY reader uses `try_lock_unfair()` to avoid blocking, falling back to `lock_unfair()` only when the buffer is full. The main thread calls `term.lock()` (blocking) during `RedrawRequested`.

During output floods, both threads compete for this lock. The PTY reader holds it for up to `MAX_LOCKED_READ` (64KB) of parsing, during which the render thread blocks.

**Impact:** During rapid output, the render thread may block for milliseconds waiting for the PTY reader to release the lock, causing frame drops.

**Recommendation:** The current design (borrowed from alacritty) is the standard approach. The main optimization opportunity is reducing how often the render thread needs the lock (see PERF-R02 -- dirty-flag tracking to skip unnecessary redraws).

---

## 11. String/Allocation Patterns

### PERF-A01: RenderedCell Clone in visible_blocks Path [Medium]

**File:** `src/main.rs` lines 2628-2633

**Description:** `visible_blocks()` returns `Vec<&Block>`, but the call site in main.rs clones all visible blocks: `.into_iter().cloned().collect()`. Each Block clone copies its `pipeline_stages: Vec<CapturedStage>`, `pipeline_stage_commands: Vec<String>`, and `soi_summary: Option<String>`.

**Impact:** For blocks with pipeline data, this is a deep clone of potentially kilobytes of data per frame.

**Recommendation:** Restructure the borrow to avoid cloning. The blocks are only needed for the duration of the `draw_frame` call. Use a reference-based approach or `Rc`/`Arc` for block data.

### PERF-A02: Tab Title String Clone per Frame [Low]

**File:** `src/main.rs` lines 2597-2611

**Description:** `tab.title.clone()` is called for every tab on every frame to build `TabDisplayInfo`. Tab titles change rarely (only on directory change).

**Impact:** Low -- typically 1-5 tabs, each with a short title string.

**Recommendation:** Use `Cow<str>` or cache the display info.

---

## Priority Fix List

Sorted by user-visible impact, highest first:

| Priority | ID | Severity | Summary | Estimated Effort |
|----------|---------|----------|---------|-----------------|
| 1 | PERF-R01 | Critical | Per-cell Buffer allocation every frame | Large -- requires dirty tracking or buffer caching |
| 2 | PERF-R02 | High | No dirty-flag redraw tracking | Medium -- add generation counter to GridSnapshot |
| 3 | PERF-L01 | High | No frame rate throttling during output floods | Small -- coalesce Wakeup events with 1-2ms debounce |
| 4 | PERF-M01 | High | Unbounded block history in memory | Medium -- evict pipeline data from old blocks |
| 5 | PERF-M02 | Medium | Vec<char> allocation for every cell's zerowidth | Medium -- use SmallVec or enum |
| 6 | PERF-TH02 | Medium | FairMutex contention during output floods | Small -- mitigated by fixing PERF-R02 |
| 7 | PERF-A01 | Medium | Deep clone of visible blocks every frame | Small -- restructure borrows |
| 8 | PERF-B01 | Medium | Missing hot-path benchmarks | Medium -- add Criterion benchmarks |
| 9 | PERF-R03 | Medium | Linear scan for cursor wide-char detection | Small -- direct index lookup |
| 10 | PERF-R04 | Medium | Overlay buffers rebuilt every frame | Medium -- cache with text comparison |
| 11 | PERF-G02 | Medium | Glyph atlas never trimmed (GPU memory leak) | Trivial -- add one call |
| 12 | PERF-S02 | Medium | Individual DELETE per record in pruning | Small -- batch DELETE |
| 13 | PERF-F01 | Medium | Recursive watch on entire project tree | Medium -- lazy/scoped watching |
| 14 | PERF-M03 | Low | Non-configurable scrollback | Small -- add config field |
| 15 | PERF-R05 | Low | Duplicate single/multi-pane render code | Large -- refactor, no perf impact |

### Quick Wins (< 1 day each):
1. **PERF-G02**: Add `ctx.frame_renderer.trim()` call after `frame.present()` -- one line fix for GPU memory leak
2. **PERF-L01**: Add a `last_redraw: Instant` field and skip redraws if less than 1ms since last frame
3. **PERF-R03**: Replace `.iter().any()` with direct index lookup for cursor wide-char check
4. **PERF-A01**: Restructure `visible_blocks` borrow to avoid cloning Block data
5. **PERF-S02**: Change retention pruning to use batch `WHERE id IN (...)` DELETE

### Medium-Term (1-3 days):
1. **PERF-R02 + PERF-R01**: Add dirty tracking to GridSnapshot. Only rebuild cell buffers when content changes.
2. **PERF-M02**: Replace `Vec<char>` with `SmallVec<[char; 0]>` in RenderedCell
3. **PERF-B01**: Add Criterion benchmarks for `build_cell_buffers`, `snapshot_term`, `build_rects`

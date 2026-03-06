# Phase 17: Pipeline UI - Research

**Researched:** 2026-03-05
**Domain:** Terminal UI rendering for pipeline blocks (Rust, wgpu, glyphon)
**Confidence:** HIGH

## Summary

Phase 17 adds visual rendering of piped commands as multi-row pipeline blocks with expandable stage inspection. The existing rendering pipeline (wgpu + glyphon + custom rect/text renderers) is well-established across 16 prior phases. The Block data structure already stores `pipeline_stages: Vec<CapturedStage>` with captured output data and byte counts. The BlockRenderer currently generates separator lines, exit code badges, duration labels, and [undo] labels -- Phase 17 extends this to render additional rows for pipeline stage metadata and expandable content.

A key data gap exists: `CapturedStage` stores `index`, `total_bytes`, and `data` (the captured bytes) but NOT the per-stage command text (e.g., "grep foo"). The command text is available by parsing the full command from the terminal grid using `glass_pipes::parse_pipeline()`, but this parsing must happen at command execution time and the results stored on the Block. This is the primary architectural addition needed.

**Primary recommendation:** Extend CapturedStage (or Block) to store per-stage command text, add expand/collapse state to Block, then extend BlockRenderer to emit additional rects and labels for pipeline stage rows. Handle mouse clicks and keyboard shortcuts in main.rs to toggle expansion state.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| UI-01 | Piped commands render as multi-row pipeline blocks showing each stage with line/byte count | BlockRenderer already generates per-block rects/labels; extend with pipeline stage rows. CapturedStage has total_bytes and data (for line counting). Must add per-stage command text storage. |
| UI-02 | Pipeline blocks auto-expand on failure or >2 stages, collapse for simple success | Block already has exit_code and pipeline_stage_count. Add `pipeline_expanded: bool` field with auto-expand logic in handle_event on CommandFinished. |
| UI-03 | User can expand any stage to view its full intermediate output | CapturedStage.data contains FinalizedBuffer with Complete/Sampled/Binary variants. Need per-stage expand state and text rendering of captured output within BlockRenderer. |
| UI-04 | User can collapse/expand pipeline blocks with click or keyboard | No mouse click handling exists in main.rs yet. Need CursorMoved tracking + MouseInput handling + hit testing against pipeline block rects. Keyboard: a keybinding to toggle focused pipeline block. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| wgpu | 28.0.0 | GPU rendering | Already the rendering backend for Glass |
| glyphon | 0.10.0 | Text rendering | Already used for all text in Glass (grid, overlays, labels) |
| winit | 0.30.13 | Window events (keyboard, mouse) | Already the event loop for Glass |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| glass_pipes | 0.1.0 | `parse_pipeline()` for extracting per-stage command text | Called once at CommandExecuted time to populate stage command names |
| glass_terminal | 0.1.0 | Block, BlockManager, CapturedStage types | Extended with expand/collapse state and stage command text |
| glass_renderer | 0.1.0 | BlockRenderer, FrameRenderer | Extended with pipeline row rendering |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Custom pipeline row rendering in BlockRenderer | Separate PipelineRenderer module | Separate module is cleaner if pipeline rendering is complex, but BlockRenderer pattern is established and keeps block decorations co-located |
| Storing parsed Pipeline on Block | Re-parsing from command text at render time | Re-parsing is wasteful; store once at execution time |

## Architecture Patterns

### Data Flow for Pipeline UI

```
Shell Integration (bash/ps1)
  -> OSC 133;S (pipeline start, stage_count)
  -> OSC 133;P (per-stage: index, total_bytes, temp_path)
  -> OscEvent -> BlockManager.handle_event()
  -> Block.pipeline_stages populated with CapturedStage
  -> main.rs reads temp files, processes through StageBuffer, stores FinalizedBuffer

At CommandExecuted time:
  -> Extract command text from terminal grid
  -> parse_pipeline(command_text) -> Pipeline with PipeStage.command per stage
  -> Store stage command texts on Block (new field)

At CommandFinished time:
  -> Auto-expand logic: if exit_code != 0 || stage_count > 2 -> expanded = true

At render time:
  -> BlockRenderer checks Block.pipeline_stages and expansion state
  -> Generates additional rect rows + text labels for each visible stage
  -> If a stage is expanded, renders its captured output as text lines
```

### Key Data Structure Changes

```rust
// In glass_terminal::block_manager::Block, add:
pub pipeline_expanded: bool,           // Overall block expand/collapse state
pub pipeline_stage_commands: Vec<String>, // Per-stage command text (parallel to pipeline_stages)

// In glass_terminal::block_manager::Block, or separate UI state:
pub expanded_stage_index: Option<usize>,  // Which single stage is showing full output (UI-03)
```

### Rendering Pattern: Pipeline Stage Rows

Each pipeline block renders as:
```
[separator line] ---- [cmd: cat file | grep foo | wc -l] [duration] [exit badge]
                     stage 0: cat file          42 lines  1.2KB   [v]
                     stage 1: grep foo          12 lines    384B   [v]
                     stage 2: wc -l              1 line      4B   [v]
```

When a stage is expanded (UI-03):
```
                     stage 1: grep foo          12 lines    384B   [^]
                       | line 1 of captured output
                       | line 2 of captured output
                       | ...
```

### Rendering Implementation Pattern

Follow the established BlockRenderer pattern:
1. `build_pipeline_rects()` -> Vec<RectInstance> for stage row backgrounds
2. `build_pipeline_text()` -> Vec<BlockLabel> for stage command, line count, byte count
3. FrameRenderer calls these alongside existing block_rects and block_text
4. Overlay text buffers handle pipeline labels via the same Phase B approach

### Mouse Click Hit Testing Pattern

```rust
// In main.rs, add WindowEvent::CursorMoved tracking:
struct WindowContext {
    cursor_position: Option<(f64, f64)>,  // Track mouse position
    // ...
}

// In WindowEvent::MouseInput handler:
// 1. Convert cursor_position to cell coordinates using cell_width/cell_height
// 2. Check if click is within a pipeline stage row
// 3. If so, toggle that stage's expanded state or toggle overall pipeline expansion
// 4. Request redraw
```

### Anti-Patterns to Avoid
- **Rendering captured output as raw terminal grid rows:** The captured data is raw bytes, not terminal-parsed content. Render it as plain text lines, not as cells with ANSI attributes.
- **Storing expansion state outside Block:** Keeping UI state separate from Block creates synchronization issues. The Block is the single source of truth.
- **Re-parsing pipeline on every frame:** Parse once at CommandExecuted time, store results.
- **Allocating text buffers per frame for pipeline content:** Reuse the overlay_buffers Vec pattern already in FrameRenderer.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Pipeline command parsing | Custom string splitting | `glass_pipes::parse_pipeline()` | Already handles quoting, escaping, PowerShell backticks |
| Byte count formatting | Manual formatting | Simple helper function (KB, MB) | Consistent formatting, avoid off-by-one in units |
| Line count from captured data | Custom counting | `data.iter().filter(\|&&b\| b == b'\n').count()` | Standard pattern, handles edge cases |
| Text rendering | Custom GPU text pipeline | glyphon via BlockLabel pattern | Already proven across 5+ overlay text use cases |

## Common Pitfalls

### Pitfall 1: Pipeline Stage Commands Not Available at Render Time
**What goes wrong:** CapturedStage only has index/bytes/data. No command text per stage.
**Why it happens:** Shell integration emits only structural data (index, size, path), not semantic data (what command ran).
**How to avoid:** At CommandExecuted time, extract command text from terminal grid, call `parse_pipeline()`, store resulting `PipeStage.command` strings on the Block in a parallel Vec.
**Warning signs:** Empty or missing command labels in pipeline UI rows.

### Pitfall 2: Vertical Space Accounting for Pipeline Rows
**What goes wrong:** Pipeline rows push content below them down, but the terminal grid has fixed line positions. Pipeline UI rows are overlay content, not terminal grid lines.
**Why it happens:** Confusion between terminal grid coordinates (absolute line numbers) and rendered pixel positions.
**How to avoid:** Pipeline stage rows are rendered as overlay rects/labels at pixel positions BELOW the block separator line. They overlay the terminal grid content. The expanded content may occlude terminal output -- this is intentional and consistent with how block decorations work. Alternatively, pipeline rows can be rendered within the block's existing vertical space as additional overlays.
**Warning signs:** Content jumping, overlapping text, incorrect scroll offsets.

### Pitfall 3: Large Captured Output in Expanded Stage View
**What goes wrong:** A stage with 10MB of captured data would create thousands of text buffers if rendered naively.
**Why it happens:** FinalizedBuffer::Complete can hold up to 10MB of text data.
**How to avoid:** Limit the rendered output to a fixed number of lines (e.g., first 50 lines). For Sampled buffers, show head and tail with a gap indicator. For Binary, show `[binary: <size>]`.
**Warning signs:** Frame rate drops when expanding a stage, excessive memory allocation.
**Note from STATE.md:** "Research flag: Expanded stage output for long captures may need virtual scrolling (Phase 17)" -- confirmed this is a known concern. Recommend capping at ~50 visible lines initially, deferring virtual scrolling to a future phase.

### Pitfall 4: Mouse Click Hit Testing Coordinate Systems
**What goes wrong:** Click position doesn't match pipeline row position.
**Why it happens:** Multiple coordinate systems: physical pixels (cursor), logical pixels (rendering), cell coordinates (terminal grid). Scale factor adds complexity.
**How to avoid:** Use the same coordinate system as BlockRenderer. CursorMoved gives physical pixels; cell_size() returns physical pixel dimensions. Divide to get cell coordinates, then match against known pipeline row positions.
**Warning signs:** Clicks on one row toggle a different row.

### Pitfall 5: Borrow Checker Conflicts in FrameRenderer
**What goes wrong:** Adding pipeline stage data to draw_frame creates borrow conflicts.
**Why it happens:** FrameRenderer already has a complex two-phase (Phase A/B) pattern for overlay buffers to satisfy borrow checker. Adding more overlay sources complicates it.
**How to avoid:** Follow the exact same pattern: build all buffers in Phase A (mutable borrows), then create TextAreas in Phase B (immutable borrows). Don't try to mix pipeline label creation with grid text area creation.
**Warning signs:** Compile errors about conflicting borrows on self.glyph_cache.font_system.

## Code Examples

### Line Count from FinalizedBuffer
```rust
// Source: glass_pipes::types::FinalizedBuffer
fn line_count(data: &FinalizedBuffer) -> usize {
    match data {
        FinalizedBuffer::Complete(bytes) => {
            let count = bytes.iter().filter(|&&b| b == b'\n').count();
            count.max(if bytes.is_empty() { 0 } else { 1 })
        }
        FinalizedBuffer::Sampled { head, tail, total_bytes } => {
            // Approximate: count in head + tail, note this is sampled
            let head_lines = head.iter().filter(|&&b| b == b'\n').count();
            let tail_lines = tail.iter().filter(|&&b| b == b'\n').count();
            head_lines + tail_lines // approximate
        }
        FinalizedBuffer::Binary { size } => 0,
    }
}
```

### Byte Count Formatting
```rust
fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
```

### Auto-Expand Logic (UI-02)
```rust
// In BlockManager::handle_event, after CommandFinished:
if block.pipeline_stage_count.unwrap_or(0) > 0 {
    let stage_count = block.pipeline_stage_count.unwrap_or(0);
    let failed = block.exit_code.map_or(false, |c| c != 0);
    block.pipeline_expanded = failed || stage_count > 2;
}
```

### Pipeline Rect Generation Pattern
```rust
// Source: follows block_renderer.rs pattern
fn build_pipeline_rects(
    &self,
    block: &Block,
    block_y: f32,  // Y position of block separator
    viewport_width: f32,
) -> Vec<RectInstance> {
    if !block.pipeline_expanded || block.pipeline_stages.is_empty() {
        return Vec::new();
    }
    let mut rects = Vec::new();
    for (i, _stage) in block.pipeline_stages.iter().enumerate() {
        let row_y = block_y + self.cell_height * (i as f32 + 1.0);
        // Subtle background for pipeline row
        rects.push(RectInstance {
            pos: [0.0, row_y, viewport_width, self.cell_height],
            color: [30.0 / 255.0, 30.0 / 255.0, 40.0 / 255.0, 0.8],
        });
    }
    rects
}
```

### Mouse Click Handling Pattern
```rust
// In main.rs WindowEvent handler:
WindowEvent::CursorMoved { position, .. } => {
    ctx.cursor_position = Some((position.x, position.y));
}
WindowEvent::MouseInput { state: ElementState::Pressed, button: winit::event::MouseButton::Left, .. } => {
    if let Some((x, y)) = ctx.cursor_position {
        let (cell_w, cell_h) = ctx.frame_renderer.cell_size();
        // Hit test pipeline rows...
        // Toggle expansion state, request redraw
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| No pipeline visualization | Block separator + exit badge only | Phase 16 (current) | Pipeline stages captured but not displayed |
| No mouse interaction | Keyboard-only (scroll, search, undo) | All prior phases | Phase 17 adds first mouse click handling |

**Deprecated/outdated:**
- None -- this is new functionality being added to an established rendering pipeline.

## Open Questions

1. **Pipeline row rendering strategy: overlay vs. inserted rows?**
   - What we know: Block decorations (separator, badge, duration) are rendered as overlays at pixel positions. They don't shift terminal grid content.
   - What's unclear: Should pipeline rows push terminal content down (inserted rows) or overlay on top of it?
   - Recommendation: Overlay approach. Pipeline rows render on top of the terminal grid content below the separator. This is consistent with existing block decoration behavior, avoids complex grid content shifting, and is simpler to implement. The overlaid terminal content is the command's own output, which the user can still scroll to see.

2. **Keyboard shortcut for pipeline expand/collapse**
   - What we know: Ctrl+Shift+letter is the Glass keybinding pattern. No shortcut for "current block" interaction exists.
   - What's unclear: Which key? How to determine "current" pipeline block?
   - Recommendation: Use Ctrl+Shift+P to toggle expansion of the most recent pipeline block. For stage-level expansion, arrow keys could navigate stages when a pipeline block is focused. Alternatively, mouse-only for stage expansion keeps keyboard handling simple.

3. **Virtual scrolling for large expanded stage output**
   - What we know: STATE.md flags this as a research concern. Captures can be up to 10MB.
   - What's unclear: Whether initial implementation needs virtual scrolling.
   - Recommendation: Cap rendered output at 50 lines initially. Show "[... N more lines]" truncation indicator. Defer virtual scrolling to a future enhancement.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test (cargo test) |
| Config file | Cargo.toml workspace |
| Quick run command | `cargo test -p glass_terminal --lib block_manager` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| UI-01 | Pipeline blocks render with stage command, line count, byte count | unit | `cargo test -p glass_terminal --lib block_manager::tests::pipeline_stage_command_text -x` | Wave 0 |
| UI-01 | Line count and byte formatting helpers | unit | `cargo test -p glass_renderer --lib block_renderer -x` | Wave 0 |
| UI-02 | Auto-expand on failure or >2 stages | unit | `cargo test -p glass_terminal --lib block_manager::tests::pipeline_auto_expand -x` | Wave 0 |
| UI-02 | Auto-collapse for simple success with <=2 stages | unit | `cargo test -p glass_terminal --lib block_manager::tests::pipeline_auto_collapse -x` | Wave 0 |
| UI-03 | Stage expansion stores/toggles per-stage state | unit | `cargo test -p glass_terminal --lib block_manager::tests::pipeline_stage_expand_toggle -x` | Wave 0 |
| UI-04 | Mouse hit test correctly identifies pipeline rows | unit | `cargo test -p glass_renderer --lib block_renderer::tests::pipeline_hit_test -x` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_terminal -p glass_renderer --lib`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `block_manager::tests::pipeline_auto_expand` -- covers UI-02 auto-expand logic
- [ ] `block_manager::tests::pipeline_stage_command_text` -- covers UI-01 command text storage
- [ ] `block_renderer` pipeline rendering tests -- covers UI-01 rect/label generation
- [ ] Hit test helper tests -- covers UI-04 click detection

## Sources

### Primary (HIGH confidence)
- Project source code: `crates/glass_renderer/src/block_renderer.rs` - existing block rendering pattern
- Project source code: `crates/glass_terminal/src/block_manager.rs` - Block data structure with pipeline_stages
- Project source code: `crates/glass_pipes/src/types.rs` - CapturedStage, FinalizedBuffer, PipeStage types
- Project source code: `src/main.rs` - event loop, rendering pipeline, input handling
- Project source code: `crates/glass_renderer/src/frame.rs` - FrameRenderer two-phase overlay pattern
- Project source code: `shell-integration/glass.bash` - OSC 133;S/P emission (no per-stage command text)

### Secondary (MEDIUM confidence)
- `.planning/STATE.md` - research flag about virtual scrolling for expanded stages
- `.planning/REQUIREMENTS.md` - UI-01 through UI-04 requirement definitions

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - all libraries already in use, no new dependencies needed
- Architecture: HIGH - follows established BlockRenderer/FrameRenderer patterns from 16 prior phases
- Pitfalls: HIGH - identified from direct code inspection of existing data structures and rendering pipeline
- Mouse handling: MEDIUM - first mouse click handling in Glass; winit 0.30.13 API for MouseInput is straightforward but untested in this codebase

**Research date:** 2026-03-05
**Valid until:** 2026-04-05 (stable stack, no external dependency changes)

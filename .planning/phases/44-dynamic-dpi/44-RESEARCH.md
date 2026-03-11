# Phase 44: Dynamic DPI - Research

**Researched:** 2026-03-10
**Domain:** winit ScaleFactorChanged handling, font metric recalculation, wgpu surface rebuild
**Confidence:** HIGH

## Summary

Dynamic DPI support requires handling the `WindowEvent::ScaleFactorChanged` event in winit 0.30 to trigger a full font metric recalculation and surface rebuild. The existing codebase already has all the building blocks: `FrameRenderer::update_font()` rebuilds GridRenderer with a new scale factor, `GlassRenderer::resize()` reconfigures the wgpu surface, and `resize_all_panes()` resizes all PTYs. The current `ScaleFactorChanged` handler in `main.rs` (line 1052) is a stub that logs a warning -- it needs to call the same sequence used by the config hot-reload font change path (lines 2364-2381), but with the new scale factor instead of a new font family/size.

The key constraint from STATE.md is: **never use glyphon TextArea.scale for DPI -- scale Metrics instead** (glyphon issue #117). The existing GridRenderer already follows this pattern correctly, computing `physical_font_size = font_size * scale_factor` and passing it to `Metrics::new()`. The DPI handler just needs to pass the new scale factor through the same path.

**Primary recommendation:** Implement ScaleFactorChanged by calling `update_font()` with the new scale factor, then `resize()` the wgpu surface, then `resize_all_panes()` for PTY notification -- mirroring the existing config hot-reload path exactly.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| DPI-01 | ScaleFactorChanged event triggers full font metric recalculation and surface rebuild | `update_font()` already accepts scale_factor; stub handler at main.rs:1052 needs real implementation |
| DPI-02 | Terminal remains correctly rendered after moving window between displays with different DPI | Correct Metrics scaling (not TextArea.scale), PTY resize via resize_all_panes, surface reconfigure via GlassRenderer::resize |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| winit | 0.30.13 | Window events including ScaleFactorChanged | Already in use, provides DPI change events |
| glyphon | 0.10 | GPU text rendering with Metrics-based DPI | Already in use, Metrics.font_size carries physical px |
| wgpu | 28.0 | GPU surface that needs reconfiguration on DPI change | Already in use |

### Supporting
No new dependencies needed. Zero new dependencies constraint from STATE.md.

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Manual scale tracking | winit `window.scale_factor()` query | Event-driven is correct; querying on every frame wastes cycles |

## Architecture Patterns

### Existing Code to Modify

```
src/main.rs
  L1052  WindowEvent::ScaleFactorChanged handler (STUB -> real implementation)
  L2364  Config hot-reload font change path (REFERENCE pattern to follow)

crates/glass_renderer/src/frame.rs
  L133   update_font() (already accepts scale_factor, no changes needed)

crates/glass_renderer/src/grid_renderer.rs
  L46    GridRenderer::new() (already computes physical_font_size = font_size * scale_factor)

crates/glass_renderer/src/surface.rs
  L184   GlassRenderer::resize() (already handles surface reconfiguration)
```

### Pattern: DPI Change Handler (to implement)

**What:** Handle `WindowEvent::ScaleFactorChanged` by rebuilding fonts, surface, and PTY dimensions.
**When to use:** When winit delivers a scale factor change (monitor switch, DPI settings change).
**Example:**

```rust
// Source: Mirrors existing config hot-reload pattern at main.rs:2364-2381
WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
    let scale = scale_factor as f32;
    tracing::info!("Scale factor changed to {}", scale);

    // 1. Rebuild font metrics with new scale factor (same font family/size)
    ctx.frame_renderer.update_font(
        &self.config.font_family,
        self.config.font_size,
        scale,
    );

    // 2. Resize wgpu surface (window physical size may have changed)
    let size = ctx.window.inner_size();
    ctx.renderer.resize(size.width, size.height);

    // 3. Resize all PTYs with new cell dimensions
    resize_all_panes(
        &mut ctx.session_mux,
        &ctx.frame_renderer,
        size.width,
        size.height,
    );

    // 4. Request redraw
    ctx.window.request_redraw();
}
```

### Pattern: Scale Factor in Metrics (existing, preserve)

**What:** DPI scaling via `Metrics::new(font_size * scale_factor, cell_height)`, never via `TextArea.scale`.
**Why critical:** glyphon issue #117 -- TextArea.scale causes incorrect glyph positioning.
**Already implemented in:** GridRenderer::new() line 51, frame.rs lines 309, 963, 1284, 1403.

### Anti-Patterns to Avoid
- **Using TextArea.scale for DPI:** Causes glyph positioning bugs (glyphon #117). Always scale Metrics font_size instead.
- **Skipping PTY resize after DPI change:** Running programs won't reflow, causing garbled output.
- **Not reconfiguring wgpu surface:** Surface dimensions are in physical pixels; DPI change may alter physical size.
- **Querying window.inner_size() before OS processes the DPI change:** On Windows, the `ScaleFactorChanged` event arrives before the window has been physically resized by the OS. A subsequent `Resized` event follows with the correct physical dimensions.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Font metric recalculation | Custom DPI math | `FrameRenderer::update_font()` | Already handles GridRenderer rebuild + sub-renderer updates |
| Surface reconfiguration | Custom wgpu reconfigure | `GlassRenderer::resize()` | Already handles zero-size guard + surface.configure |
| PTY dimension computation | Per-pane math | `resize_all_panes()` | Already handles split panes, background tabs |

**Key insight:** The DPI handler is literally the same 3-step sequence as the config hot-reload font change path. No new infrastructure needed.

## Common Pitfalls

### Pitfall 1: ScaleFactorChanged and Resized Event Ordering
**What goes wrong:** On Windows, `ScaleFactorChanged` fires first, then the OS resizes the window and `Resized` fires. If you resize the surface in `ScaleFactorChanged` using the pre-change `inner_size()`, you get the old physical size, then `Resized` fixes it. But if you DON'T resize in `ScaleFactorChanged`, there's a brief frame with wrong cell metrics and old surface size.
**Why it happens:** winit on Windows maps `WM_DPICHANGED` to `ScaleFactorChanged`; the OS then applies the suggested window rect which triggers `Resized`.
**How to avoid:** In `ScaleFactorChanged`: rebuild fonts (metrics change immediately). Let the subsequent `Resized` event handle the surface resize and PTY re-dimension. This avoids double-resize and is simpler. Alternatively, handle both in ScaleFactorChanged by querying `inner_size()` -- the Resized handler is already idempotent.
**Warning signs:** Double PTY resize messages in logs, brief rendering glitch during transition.

### Pitfall 2: Glyph Atlas Stale After Scale Change
**What goes wrong:** The glyph atlas (TextAtlas in GlyphCache) contains glyphs rasterized at the old DPI. After a scale change, old glyphs render at wrong size/resolution until re-rasterized.
**Why it happens:** glyphon's TextAtlas caches rasterized glyphs by their Metrics. New Metrics with different font_size produce new cache entries; old entries age out via `atlas.trim()`.
**How to avoid:** After `update_font()`, the next `prepare()` call re-rasterizes all visible glyphs with new Metrics. The existing `trim()` call after each frame cleans up. No explicit atlas clear needed.
**Warning signs:** Blurry text on first frame after DPI change (should self-resolve on next frame).

### Pitfall 3: Background Tab Scale Mismatch
**What goes wrong:** Background tabs don't get their PTYs resized with new cell dimensions.
**Why it happens:** Only the active tab or focused session gets resized.
**How to avoid:** The existing Resized handler (main.rs:1018-1047) already resizes background tabs. Ensure the DPI handler follows the same pattern or relies on the subsequent Resized event.

### Pitfall 4: Platform Differences in ScaleFactorChanged Delivery
**What goes wrong:** X11 may not deliver ScaleFactorChanged at all; Wayland delivers it; Windows delivers it.
**Why it happens:** X11 doesn't have per-monitor DPI support in most window managers; winit falls back to scale_factor=1.0.
**How to avoid:** The handler is a no-op if scale factor hasn't actually changed. No special platform handling needed -- just implement the event handler and it works where supported.

## Code Examples

### Complete DPI Handler (recommended implementation)

```rust
// Source: Pattern from config hot-reload at main.rs:2364-2381
WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
    let scale = scale_factor as f32;
    tracing::info!("DPI scale factor changed to {scale}");

    // Rebuild font metrics + all sub-renderers with new scale
    ctx.frame_renderer.update_font(
        &self.config.font_family,
        self.config.font_size,
        scale,
    );

    // Get new physical window size (may already reflect DPI change on some platforms)
    let size = ctx.window.inner_size();
    if size.width > 0 && size.height > 0 {
        ctx.renderer.resize(size.width, size.height);

        // Resize all panes in active tab + background tabs
        // (mirrors Resized handler pattern at main.rs:990-1047)
        let (cell_w, cell_h) = ctx.frame_renderer.cell_size();

        if ctx.session_mux.active_tab_pane_count() > 1 {
            resize_all_panes(
                &mut ctx.session_mux,
                &ctx.frame_renderer,
                size.width,
                size.height,
            );
        } else {
            let num_cols = (size.width as f32 / cell_w).floor().max(1.0) as u16;
            let num_lines =
                ((size.height as f32 / cell_h).floor().max(2.0) as u16).saturating_sub(2);
            let full_size = WindowSize {
                num_lines,
                num_cols,
                cell_width: cell_w as u16,
                cell_height: cell_h as u16,
            };
            if let Some(session) = ctx.session_mux.focused_session_mut() {
                let _ = session.pty_sender.send(PtyMsg::Resize(full_size));
                session.term.lock().resize(TermDimensions {
                    columns: num_cols as usize,
                    screen_lines: num_lines as usize,
                });
                session.block_manager.notify_resize(num_cols as usize);
            }
        }

        // Background tabs
        let num_cols = (size.width as f32 / cell_w).floor().max(1.0) as u16;
        let num_lines =
            ((size.height as f32 / cell_h).floor().max(2.0) as u16).saturating_sub(2);
        let full_size = WindowSize {
            num_lines,
            num_cols,
            cell_width: cell_w as u16,
            cell_height: cell_h as u16,
        };
        let active_idx = ctx.session_mux.active_tab_index();
        let bg_session_ids: Vec<_> = ctx
            .session_mux
            .tabs()
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != active_idx)
            .flat_map(|(_, t)| t.session_ids())
            .collect();
        for sid in bg_session_ids {
            if let Some(session) = ctx.session_mux.session_mut(sid) {
                let _ = session.pty_sender.send(PtyMsg::Resize(full_size));
                session.term.lock().resize(TermDimensions {
                    columns: num_cols as usize,
                    screen_lines: num_lines as usize,
                });
                session.block_manager.notify_resize(num_cols as usize);
            }
        }
    }

    ctx.window.request_redraw();
}
```

### Alternative: Lean Handler (delegate to Resized)

```rust
// Simpler approach: only rebuild fonts in ScaleFactorChanged,
// let the subsequent Resized event handle surface + PTY resize
WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
    let scale = scale_factor as f32;
    tracing::info!("DPI scale factor changed to {scale}");
    ctx.frame_renderer.update_font(
        &self.config.font_family,
        self.config.font_size,
        scale,
    );
    ctx.window.request_redraw();
}
```

**Tradeoff:** The lean approach is simpler but relies on the OS sending a Resized event after ScaleFactorChanged. On Windows this is reliable (WM_DPICHANGED causes a window resize). On other platforms, verify behavior. The lean approach means there's one frame where fonts are rebuilt but surface/PTY dimensions are stale -- the Resized handler fixes this immediately after.

### Test: Scale Factor Changes Cell Dimensions

```rust
// Source: Existing test pattern in grid_renderer.rs
#[test]
fn scale_factor_changes_cell_dimensions() {
    let mut font_system = FontSystem::new();
    let gr_1x = GridRenderer::new(&mut font_system, "Cascadia Mono", 14.0, 1.0);
    let gr_2x = GridRenderer::new(&mut font_system, "Cascadia Mono", 14.0, 2.0);
    // At 2x scale, cell dimensions should be approximately double
    let ratio_w = gr_2x.cell_width / gr_1x.cell_width;
    let ratio_h = gr_2x.cell_height / gr_1x.cell_height;
    assert!((ratio_w - 2.0).abs() < 0.15, "width ratio: {ratio_w}");
    assert!((ratio_h - 2.0).abs() < 0.15, "height ratio: {ratio_h}");
}

#[test]
fn scale_factor_preserves_grid_alignment() {
    let mut font_system = FontSystem::new();
    let gr = GridRenderer::new(&mut font_system, "Cascadia Mono", 14.0, 1.5);
    // cell_height must be at least physical_font_size
    let physical = 14.0 * 1.5;
    assert!(gr.cell_height >= physical, "cell_height {} < physical {}", gr.cell_height, physical);
    // cell_height must be ceil'd (integer pixel boundary)
    assert_eq!(gr.cell_height, gr.cell_height.ceil());
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| TextArea.scale for DPI | Metrics font_size scaling | glyphon #117 discovery | Correct glyph positioning at all DPI levels |
| Hardcoded 1.2x line height | Font metric (ascent+descent) line height | Phase 40 | Box-drawing connects seamlessly |
| Log-and-ignore DPI changes | Full rebuild on ScaleFactorChanged | Phase 44 (this phase) | Multi-monitor HiDPI support |

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test + cargo test |
| Config file | Cargo.toml workspace |
| Quick run command | `cargo test -p glass_renderer -- grid_renderer` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DPI-01 | scale_factor change produces different cell dimensions | unit | `cargo test -p glass_renderer -- scale_factor_changes_cell_dimensions -x` | Wave 0 |
| DPI-01 | scale_factor change preserves grid alignment invariants | unit | `cargo test -p glass_renderer -- scale_factor_preserves_grid_alignment -x` | Wave 0 |
| DPI-02 | After DPI change, PTY receives correct new dimensions | manual-only | Manual: drag window between monitors, verify reflow | N/A |
| DPI-02 | No rendering artifacts after DPI change | manual-only | Manual: visual inspection after monitor switch | N/A |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_renderer -- grid_renderer -x`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `scale_factor_changes_cell_dimensions` test in grid_renderer.rs -- covers DPI-01
- [ ] `scale_factor_preserves_grid_alignment` test in grid_renderer.rs -- covers DPI-01

## Open Questions

1. **Does winit 0.30.13 reliably send Resized after ScaleFactorChanged on Windows?**
   - What we know: Windows WM_DPICHANGED typically causes the OS to resize the window, which should trigger a Resized event.
   - What's unclear: Whether the Resized event is guaranteed in all cases (e.g., if the new size happens to equal the old physical size).
   - Recommendation: Implement the full handler in ScaleFactorChanged (not the lean version) to be safe. The Resized handler is idempotent so double-processing is harmless.

2. **Is there a visible glitch frame during the transition?**
   - What we know: Font metrics rebuild is synchronous; glyph rasterization happens on the next prepare() call.
   - What's unclear: Whether there's a visible frame with old-DPI glyphs and new-DPI metrics.
   - Recommendation: Accept one potential glitch frame -- this is the standard behavior for terminal emulators.

## Sources

### Primary (HIGH confidence)
- Codebase analysis: `src/main.rs` ScaleFactorChanged stub (L1052-1063), config hot-reload (L2364-2381), Resized handler (L981-1051)
- Codebase analysis: `grid_renderer.rs` GridRenderer::new() with scale_factor (L46-89)
- Codebase analysis: `frame.rs` FrameRenderer::update_font() (L133-145)
- Codebase analysis: `surface.rs` GlassRenderer::resize() (L184-191)
- STATE.md: "Never use glyphon TextArea.scale for DPI -- scale Metrics instead (glyphon issue #117)"

### Secondary (MEDIUM confidence)
- [winit WindowEvent docs](https://docs.rs/winit/0.30.8/winit/event/enum.WindowEvent.html) -- ScaleFactorChanged variant with scale_factor and inner_size_writer fields
- [winit issue #3704](https://github.com/rust-windowing/winit/issues/3704) -- InnerSizeWriter panic fixed in 0.30+
- [WM_DPICHANGED](https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-dpichanged) -- Windows DPI change message behavior

### Tertiary (LOW confidence)
- [winit issue #3192](https://github.com/rust-windowing/winit/issues/3192) -- Resized event delivery guarantees (platform-dependent)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - zero new dependencies, all building blocks exist in codebase
- Architecture: HIGH - mirrors existing config hot-reload pattern exactly
- Pitfalls: MEDIUM - platform-specific event ordering not fully verified on all OSes

**Research date:** 2026-03-10
**Valid until:** 2026-04-10 (stable domain, winit 0.30 API unlikely to change)

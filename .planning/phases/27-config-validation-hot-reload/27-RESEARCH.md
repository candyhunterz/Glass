# Phase 27: Config Validation & Hot-Reload - Research

**Researched:** 2026-03-07
**Domain:** Config file watching, validation, live reload in a winit/wgpu terminal emulator
**Confidence:** HIGH

## Summary

Phase 27 transforms Glass's config system from "load once at startup, silently fallback on errors" to "validate with actionable errors, watch for changes, apply live." The existing `GlassConfig` in `glass_core::config` uses `serde + toml` for deserialization with `#[serde(default)]` on all fields, silently falling back to defaults on any error. This needs to change: malformed TOML must produce specific error messages with line/column info, and a file watcher must detect changes and propagate them through the winit event loop.

The architecture is straightforward: `notify 8.2` is already in the workspace (used by `glass_snapshot`), `toml 1.0.4` already provides rich `toml::de::Error` with span information (line, column, message), and the winit `EventLoopProxy<AppEvent>` pattern is well-established for cross-thread communication. The main complexity is in the font rebuild path -- `FrameRenderer` currently has no method to update font family/size after construction, and `GridRenderer` stores font metrics as plain fields that need recalculation.

**Primary recommendation:** Add a `ConfigReloaded(GlassConfig)` variant to `AppEvent`, watch `~/.glass/config.toml` with `notify`, diff old vs new config to determine if font rebuild is needed, and add an `update_font()` method to `FrameRenderer`/`GridRenderer`. For error display, reuse the existing overlay rendering pattern (like `SearchOverlayRenderer`) to show a semi-transparent error banner.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CONF-01 | Config validation with actionable error messages on malformed config.toml | `toml::de::Error` provides `span()`, `line_col()`, and `message()` for precise error location. Change `load_from_str` to return `Result<GlassConfig, ConfigError>` instead of silently defaulting. |
| CONF-02 | Config hot-reload watching config.toml for changes and applying without restart | `notify 8.2` already in workspace. Watch `~/.glass/config.toml` with `NonRecursive` mode. Send `AppEvent::ConfigReloaded` through `EventLoopProxy`. Diff old/new config to determine rebuild scope. |
| CONF-03 | In-terminal error overlay displaying config parse errors instead of silent failure | Reuse overlay rendering pattern from `SearchOverlayRenderer`. Draw semi-transparent background rect + error text via `FrameRenderer`. Auto-dismiss on next successful reload. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| notify | 8.2 | Filesystem watching for config.toml changes | Already in workspace (glass_snapshot). Cross-platform (ReadDirectoryChangesW on Windows, FSEvents on macOS, inotify on Linux). |
| toml | 1.0.4 | TOML parsing with error spans | Already in workspace. `toml::de::Error` provides `span()` returning byte range, `message()` for description -- sufficient for actionable errors. |
| serde | 1.0.228 | Deserialization with validation | Already in workspace. `#[serde(default)]` already used on GlassConfig. |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| dirs | 6 | Home directory resolution | Already used for config path (`~/.glass/config.toml`). |
| winit | 0.30.13 | EventLoopProxy for cross-thread config events | Already the event loop. `EventLoopProxy<AppEvent>` is Clone + Send. |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| notify for file watching | Polling with timer | notify is already a dependency and more efficient; polling adds latency |
| toml error spans | Custom TOML parser | toml 1.0 already provides excellent error messages with line/column |

**Installation:**
```bash
# No new dependencies needed -- notify 8.2 already in workspace via glass_snapshot
# Just add notify = { workspace = true } to glass_core/Cargo.toml (or root Cargo.toml)
```

Note: `notify` is pinned at 8.2 in glass_snapshot but NOT declared in `[workspace.dependencies]`. Either promote it to workspace or add it directly to the crate that owns the watcher.

## Architecture Patterns

### Recommended Project Structure
```
crates/glass_core/src/
  config.rs          # GlassConfig + ConfigError + validation + load_validated()
  config_watcher.rs  # NEW: ConfigWatcher using notify, sends AppEvent::ConfigReloaded
  event.rs           # Add ConfigReloaded variant to AppEvent
  error.rs           # Existing error types

crates/glass_renderer/src/
  frame.rs           # Add update_font() method to FrameRenderer
  grid_renderer.rs   # Add update_font() method to GridRenderer
  config_overlay.rs  # NEW: ConfigErrorOverlay renderer (similar to SearchOverlayRenderer)

src/main.rs          # Handle AppEvent::ConfigReloaded in Processor
```

### Pattern 1: Config Validation with Structured Errors
**What:** Replace silent fallback with `Result<GlassConfig, ConfigError>` that captures TOML parse errors with location info.
**When to use:** At startup and on every hot-reload attempt.
**Example:**
```rust
// In glass_core/src/config.rs
#[derive(Debug, Clone)]
pub struct ConfigError {
    pub message: String,
    pub line: Option<usize>,    // 1-based line number
    pub column: Option<usize>,  // 1-based column number
    pub snippet: Option<String>, // The offending line from the TOML
}

impl GlassConfig {
    /// Parse config, returning structured error on failure.
    pub fn load_validated(s: &str) -> Result<Self, ConfigError> {
        toml::from_str(s).map_err(|e| {
            let (line, col) = e.span()
                .and_then(|span| {
                    // Convert byte offset to line/col
                    let prefix = &s[..span.start.min(s.len())];
                    let line = prefix.matches('\n').count() + 1;
                    let col = prefix.rsplit('\n').next().map(|l| l.len() + 1).unwrap_or(1);
                    Some((line, col))
                })
                .unwrap_or((0, 0));
            let snippet = if line > 0 {
                s.lines().nth(line - 1).map(|l| l.to_string())
            } else { None };
            ConfigError {
                message: e.message().to_string(),
                line: if line > 0 { Some(line) } else { None },
                column: if col > 0 { Some(col) } else { None },
                snippet,
            }
        })
    }
}
```

### Pattern 2: Config Watcher with EventLoopProxy
**What:** Background thread watches config.toml and sends parsed config (or error) via the winit event loop.
**When to use:** Spawned once during window creation, lives for app lifetime.
**Example:**
```rust
// In glass_core/src/config_watcher.rs
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc;
use std::path::PathBuf;

pub fn spawn_config_watcher(
    config_path: PathBuf,
    proxy: winit::event_loop::EventLoopProxy<AppEvent>,
) {
    std::thread::spawn(move || {
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res| {
            tx.send(res).ok();
        }).expect("Failed to create config watcher");

        // Watch the PARENT directory (not the file) -- some editors
        // do atomic save (write tmp + rename) which removes the watch.
        let parent = config_path.parent().unwrap();
        watcher.watch(parent, RecursiveMode::NonRecursive)
            .expect("Failed to watch config directory");

        for result in rx {
            if let Ok(event) = result {
                // Only react to events affecting our config file
                if event.paths.iter().any(|p| p.ends_with("config.toml")) {
                    match std::fs::read_to_string(&config_path) {
                        Ok(contents) => {
                            match GlassConfig::load_validated(&contents) {
                                Ok(config) => {
                                    proxy.send_event(AppEvent::ConfigReloaded {
                                        config,
                                        error: None,
                                    }).ok();
                                }
                                Err(e) => {
                                    proxy.send_event(AppEvent::ConfigReloaded {
                                        config: GlassConfig::default(),
                                        error: Some(e),
                                    }).ok();
                                }
                            }
                        }
                        Err(_) => {} // File might be mid-write; ignore
                    }
                }
            }
        }
    });
}
```

### Pattern 3: Selective Config Application (Font vs Non-Visual)
**What:** Diff old and new config to determine what changed. Only rebuild fonts when font_family or font_size changes.
**When to use:** In the `AppEvent::ConfigReloaded` handler in `Processor`.
**Example:**
```rust
// In main.rs Processor::handle ConfigReloaded
fn apply_config(&mut self, new_config: GlassConfig) {
    let font_changed = new_config.font_family != self.config.font_family
        || new_config.font_size != self.config.font_size;

    if font_changed {
        // Rebuild font metrics in FrameRenderer for ALL windows
        for ctx in self.windows.values_mut() {
            let scale = ctx.window.scale_factor() as f32;
            ctx.frame_renderer.update_font(
                &new_config.font_family,
                new_config.font_size,
                scale,
            );
            // Recompute terminal grid size and resize all PTYs
            let (cell_w, cell_h) = ctx.frame_renderer.cell_size();
            let size = ctx.window.inner_size();
            // ... resize PTYs for all sessions
            ctx.window.request_redraw();
        }
    }

    // Non-visual changes: just swap the config reference
    // history thresholds, snapshot settings, pipe settings
    // are read from self.config at point-of-use, so swapping is sufficient.
    self.config = new_config;
}
```

### Pattern 4: Error Overlay Rendering
**What:** Draw a semi-transparent overlay with config error text, similar to SearchOverlayRenderer.
**When to use:** When config parse error occurs during hot-reload.
**Example:**
```rust
// Store in WindowContext or Processor
pub struct ConfigErrorOverlay {
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}
// Render as: background rect (dark red, 80% opacity) at top of viewport
// Text: "Config Error (line X, col Y): message"
// Auto-dismiss when next successful reload clears the error
```

### Anti-Patterns to Avoid
- **Watching the file directly:** Some editors (vim, VSCode) do atomic saves (write to temp, rename). Watching the file directly loses the watch after rename. Watch the PARENT DIRECTORY with `NonRecursive` and filter events by filename.
- **Blocking the event loop on config read:** Config file I/O must happen on the watcher thread, not the main thread. Only send the parsed result via `EventLoopProxy`.
- **Rebuilding fonts on every config change:** Only rebuild when `font_family` or `font_size` actually changed. History/snapshot/pipe config changes should NOT trigger any rendering work.
- **Debounce overkill:** The notify crate on some platforms fires multiple events per save. A simple "last config wins" approach (overwrite pending config) is sufficient; no need for timer-based debouncing since config reloads are fast.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| File watching | Custom polling loop | `notify 8.2` (already in workspace) | Cross-platform, handles OS-specific APIs, well-tested |
| TOML error formatting | Custom parser for error positions | `toml::de::Error::span()` + `message()` | Already provides byte span, just convert to line/col |
| Cross-thread event dispatch | Custom channels/mutex for config events | `EventLoopProxy<AppEvent>` | Already the established pattern in Glass for PTY events |
| Debouncing file events | Timer-based debounce system | Simple "last event wins" (config parse is <1ms) | Config file is tiny, parsing is instant, complexity not warranted |

**Key insight:** Every building block already exists in the Glass codebase. The watcher pattern is in glass_snapshot, the event dispatch pattern is `EventLoopProxy<AppEvent>`, the overlay rendering pattern is in `SearchOverlayRenderer`. This phase is integration work, not greenfield.

## Common Pitfalls

### Pitfall 1: Atomic Save / Editor Rename
**What goes wrong:** vim and VSCode write to a temp file then rename. This can remove the inotify watch on the original file, causing the watcher to stop receiving events.
**Why it happens:** `notify` watches inodes, not filenames. Rename replaces the inode.
**How to avoid:** Watch the PARENT DIRECTORY (`~/.glass/`) with `NonRecursive` mode. Filter events to only process those where the path ends with `config.toml`.
**Warning signs:** Config reload works once but stops working after the second save.

### Pitfall 2: Font Rebuild Triggers PTY Resize
**What goes wrong:** Changing font size changes cell dimensions, which changes the terminal grid size (cols x rows). If the PTY is not resized, text wrapping breaks.
**Why it happens:** PTY expects `TIOCSWINSZ` to match the actual grid.
**How to avoid:** After font rebuild, recalculate grid dimensions and call `term.resize()` + send PTY resize for ALL sessions in ALL windows.
**Warning signs:** Text wraps at wrong column after font size change.

### Pitfall 3: Race Between Config Write and Read
**What goes wrong:** The watcher fires mid-write, reads a partial/truncated file, and shows a spurious error.
**Why it happens:** Some editors write in multiple steps, and the watcher fires on the first write.
**How to avoid:** On parse failure during hot-reload, add a small retry (50ms delay, re-read). If still failing, show the error. Alternatively, just attempt parse and if it fails, wait for the next event (editors typically fire multiple events).
**Warning signs:** Intermittent "config error" flashes that immediately resolve.

### Pitfall 4: Config Error Overlay Blocking Terminal Use
**What goes wrong:** Error overlay prevents user from interacting with the terminal to fix the config.
**Why it happens:** If overlay intercepts keyboard input like SearchOverlay does.
**How to avoid:** Error overlay is DISPLAY ONLY -- a small banner at the top of the viewport. It does NOT intercept any keyboard input. User can continue using the terminal normally.
**Warning signs:** User can't type while error overlay is showing.

### Pitfall 5: ScaleFactorChanged Interaction
**What goes wrong:** Font update method doesn't account for current scale factor.
**Why it happens:** `GridRenderer` stores `scale_factor` separately from font metrics. `update_font()` must accept or read the current scale factor.
**How to avoid:** Pass `scale_factor` to `update_font()`. STATE.md notes: "ScaleFactorChanged is log-only (tech debt) -- Phase 27 config hot-reload should address font recalculation."
**Warning signs:** Fonts appear wrong size on HiDPI displays after config reload.

## Code Examples

### toml Error Span Extraction
```rust
// toml 1.0.4 -- toml::de::Error provides span() and message()
let input = "font_size = \"not a number\"";
let err: toml::de::Error = toml::from_str::<GlassConfig>(input).unwrap_err();
// err.span() returns Option<Range<usize>> -- byte offsets into input
// err.message() returns &str -- human-readable description
// Convert byte offset to line/col by counting newlines in input[..offset]
```

### notify 8.2 Watcher Setup (from glass_snapshot pattern)
```rust
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc;

let (tx, rx) = mpsc::channel();
let mut watcher = notify::recommended_watcher(move |res| {
    tx.send(res).ok();
})?;
// Watch parent directory, not the file itself
watcher.watch(parent_dir, RecursiveMode::NonRecursive)?;
// rx.recv() blocks; rx.try_recv() is non-blocking
```

### FrameRenderer Font Update (new method to add)
```rust
// In frame.rs -- add this method to FrameRenderer
pub fn update_font(
    &mut self,
    font_family: &str,
    font_size: f32,
    scale_factor: f32,
) {
    // Rebuild GridRenderer with new metrics
    self.grid_renderer = GridRenderer::new(
        &mut self.glyph_cache.font_system,
        font_family,
        font_size,
        scale_factor,
    );
    // Update sub-renderers that depend on cell size
    let (cell_width, cell_height) = self.grid_renderer.cell_size();
    self.block_renderer = BlockRenderer::new(cell_width, cell_height);
    self.search_overlay_renderer = SearchOverlayRenderer::new(cell_width, cell_height);
    self.status_bar = StatusBarRenderer::new(cell_height);
    self.tab_bar = TabBarRenderer::new(cell_width, cell_height);
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Silent fallback to defaults | Structured errors with line/col | This phase | Users get actionable feedback |
| Load once at startup | File watcher + live reload | This phase | No restart needed for config changes |
| No error display | In-terminal overlay | This phase | Config errors visible without log diving |

**Current state of Glass config:**
- `GlassConfig::load()` silently falls back to defaults on any error
- `GlassConfig::load_from_str()` returns `Self` (infallible), hiding errors
- Config is loaded once in `main()` and stored in `Processor.config` as an owned value
- No file watching exists for config (only for snapshot directory changes)
- `FrameRenderer` has no `update_font()` method -- fonts are set at construction time
- `GridRenderer` stores `font_family`, `font_size`, `scale_factor` as public fields but has no update method

## Open Questions

1. **Debounce strategy for rapid saves**
   - What we know: Some editors fire multiple events per save. Config parsing is fast (<1ms).
   - What's unclear: Whether a simple "parse on every event" approach causes visible flicker on the error overlay.
   - Recommendation: Start without debouncing. If flicker is observed, add a 100ms debounce timer. The simplest approach is to just let every event trigger a re-parse; the last successful parse wins.

2. **Scale factor update during font rebuild**
   - What we know: STATE.md explicitly says "ScaleFactorChanged is log-only (tech debt) -- Phase 27 should address font recalculation."
   - What's unclear: Whether full ScaleFactorChanged support is in scope or just ensuring font reload uses the current scale factor.
   - Recommendation: Ensure `update_font()` takes the current `window.scale_factor()`. Full ScaleFactorChanged support (dynamic DPI switching) can remain future work, but the infrastructure will be in place.

3. **Config watcher placement (which crate?)**
   - What we know: `glass_core` owns `GlassConfig` and `AppEvent`. `glass_snapshot` has the notify dependency.
   - What's unclear: Whether to add `notify` to `glass_core` or create the watcher in the root crate.
   - Recommendation: Add `notify` to `glass_core` since it owns config and events. The watcher module logically belongs with the config module.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test + cargo test |
| Config file | N/A (uses `#[cfg(test)]` modules) |
| Quick run command | `cargo test -p glass_core` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CONF-01 | Malformed TOML produces structured error with line/col | unit | `cargo test -p glass_core config::tests::validation -- -x` | No - Wave 0 |
| CONF-01 | Unknown keys produce warning (not hard error) | unit | `cargo test -p glass_core config::tests::unknown_keys -- -x` | No - Wave 0 |
| CONF-01 | Type mismatch (string for float) shows field name | unit | `cargo test -p glass_core config::tests::type_mismatch -- -x` | No - Wave 0 |
| CONF-02 | Config diff detects font_family change | unit | `cargo test -p glass_core config::tests::diff_font -- -x` | No - Wave 0 |
| CONF-02 | Config diff detects non-visual change (no font rebuild) | unit | `cargo test -p glass_core config::tests::diff_nonvisual -- -x` | No - Wave 0 |
| CONF-02 | ConfigWatcher sends event on file change | integration | `cargo test -p glass_core config_watcher::tests -- -x` | No - Wave 0 |
| CONF-03 | ConfigError formats as user-friendly message | unit | `cargo test -p glass_core config::tests::error_display -- -x` | No - Wave 0 |
| CONF-02 | Font rebuild updates cell dimensions | unit | `cargo test -p glass_renderer grid_renderer::tests::update_font -- -x` | No - Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_core`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `glass_core/src/config.rs` -- new validation tests (CONF-01 error messages)
- [ ] `glass_core/src/config.rs` -- config diff tests (CONF-02 change detection)
- [ ] `glass_core/src/config_watcher.rs` -- watcher integration tests (CONF-02)
- [ ] `glass_renderer/src/frame.rs` or `grid_renderer.rs` -- font update tests (CONF-02)
- [ ] Add `notify` dependency to `glass_core/Cargo.toml` (or promote to workspace)

## Sources

### Primary (HIGH confidence)
- Glass codebase: `crates/glass_core/src/config.rs` -- current GlassConfig implementation
- Glass codebase: `crates/glass_snapshot/src/watcher.rs` -- existing notify 8.2 usage pattern
- Glass codebase: `src/main.rs` -- Processor struct, event loop, font init, AppEvent handling
- Glass codebase: `crates/glass_renderer/src/frame.rs` -- FrameRenderer construction and font usage
- Glass codebase: `crates/glass_renderer/src/grid_renderer.rs` -- GridRenderer font metrics
- Glass codebase: `.planning/STATE.md` -- ScaleFactorChanged tech debt note

### Secondary (MEDIUM confidence)
- toml 1.0 crate: `toml::de::Error` provides `span()` (byte range) and `message()` for parse errors
- notify 8.2 crate: `RecommendedWatcher` with `mpsc` channel pattern (verified from glass_snapshot usage)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - all libraries already in workspace, patterns established
- Architecture: HIGH - follows existing EventLoopProxy + overlay rendering patterns
- Pitfalls: HIGH - atomic save issue is well-documented; PTY resize requirement clear from codebase

**Research date:** 2026-03-07
**Valid until:** 2026-04-07 (stable domain, no fast-moving dependencies)

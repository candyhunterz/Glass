# Phase 23: Tabs - Research

**Researched:** 2026-03-06
**Domain:** Tab bar UI, session lifecycle, multi-PTY management
**Confidence:** HIGH

## Summary

Phase 23 adds a tab bar and full tab lifecycle to Glass. The existing codebase is exceptionally well-prepared: `SessionMux` already has `tabs: Vec<Tab>`, `active_tab: usize`, `sessions: HashMap<SessionId, Session>`, and a `next_session_id()` counter. The `Session` struct already holds all per-session state (PTY, Term, BlockManager, HistoryDb, SnapshotStore). All `AppEvent` variants already carry `session_id` for routing. The work is primarily: (1) adding tab CRUD methods to `SessionMux`, (2) rendering a tab bar strip, (3) wiring keyboard/mouse shortcuts in main.rs, (4) adding a `working_directory` parameter to `spawn_pty`, and (5) handling session cleanup on tab close.

The rendering approach follows the established pattern: `RectInstance` colored rectangles for tab backgrounds via the existing `RectRenderer`, and glyphon `Buffer` text for tab titles via the existing `GlyphCache`. No new GPU pipelines or shaders are needed. The tab bar consumes one row of screen height (one `cell_height`), same as the existing status bar pattern.

**Primary recommendation:** Extend `SessionMux` with `add_tab`, `close_tab`, `activate_tab`, `next_tab`, `prev_tab` methods. Add a `TabBarRenderer` in glass_renderer following the `StatusBarRenderer` pattern. Wire shortcuts in main.rs keyboard handler. Add `working_directory` parameter to `spawn_pty`.

## Standard Stack

### Core (already in workspace)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| wgpu | 28.0.0 | Tab bar rect rendering | Already used for all GPU rendering |
| glyphon | 0.10.0 | Tab title text rendering | Already used for all text rendering |
| winit | 0.30.13 | Keyboard/mouse event handling | Already used for window management |
| alacritty_terminal | =0.25.1 | Per-tab terminal emulation | Already used, one Term per session |

### Supporting (already in workspace)
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| glass_mux | local | SessionMux with tab/session management | Core of tab lifecycle |
| glass_terminal | local | PTY spawn, EventProxy per session | New tab creation |
| glass_renderer | local | RectRenderer + GlyphCache for tab bar | Tab bar rendering |
| glass_core | local | AppEvent with session_id routing | Event routing to correct tab |

### No New Dependencies
No new crates are needed. Everything required is already in the workspace.

## Architecture Patterns

### Recommended Changes by Crate

```
glass_mux/src/
  session_mux.rs   # ADD: add_tab, close_tab, activate_tab, next_tab, prev_tab, tab_count, tabs()
  tab.rs           # ADD: title field, is_active computed property
  types.rs         # No changes needed

glass_renderer/src/
  tab_bar.rs       # NEW: TabBarRenderer (follows StatusBarRenderer pattern)
  frame.rs         # MODIFY: add tab bar rendering pass before grid content

glass_terminal/src/
  pty.rs           # MODIFY: add working_directory parameter to spawn_pty

src/main.rs        # MODIFY: keyboard shortcuts, tab bar mouse clicks, session creation/teardown
```

### Pattern 1: SessionMux Tab CRUD
**What:** Methods on SessionMux for the full tab lifecycle
**When to use:** Every tab operation

The existing SessionMux already has:
- `sessions: HashMap<SessionId, Session>` -- owns all sessions
- `tabs: Vec<Tab>` -- ordered tab list
- `active_tab: usize` -- index of focused tab
- `next_session_id()` -- generates unique IDs

Add these methods:
```rust
impl SessionMux {
    /// Add a new tab with its session. Returns the tab's ID.
    /// Inserts the tab after the current active tab.
    pub fn add_tab(&mut self, session: Session) -> TabId {
        let tab_id = TabId::new(self.next_id);
        self.next_id += 1;
        let session_id = session.id;
        self.sessions.insert(session_id, session);
        let insert_pos = self.active_tab + 1;
        self.tabs.insert(insert_pos, Tab {
            id: tab_id,
            session_id,
            title: String::new(),
        });
        self.active_tab = insert_pos;
        tab_id
    }

    /// Close a tab by index. Returns the removed Session for cleanup.
    /// Adjusts active_tab to stay valid.
    pub fn close_tab(&mut self, index: usize) -> Option<Session> {
        if index >= self.tabs.len() { return None; }
        let tab = self.tabs.remove(index);
        let session = self.sessions.remove(&tab.session_id);
        if self.active_tab >= self.tabs.len() && self.active_tab > 0 {
            self.active_tab -= 1;
        }
        session
    }

    /// Switch to tab at given index.
    pub fn activate_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = index;
        }
    }

    /// Cycle to next tab (wraps around).
    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab = (self.active_tab + 1) % self.tabs.len();
        }
    }

    /// Cycle to previous tab (wraps around).
    pub fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab = if self.active_tab == 0 {
                self.tabs.len() - 1
            } else {
                self.active_tab - 1
            };
        }
    }

    pub fn tab_count(&self) -> usize { self.tabs.len() }
    pub fn active_tab_index(&self) -> usize { self.active_tab }
    pub fn tabs(&self) -> &[Tab] { &self.tabs }
}
```

### Pattern 2: TabBarRenderer (follows StatusBarRenderer)
**What:** Builds RectInstance and text labels for the tab bar strip
**When to use:** Every frame

The existing StatusBarRenderer pattern:
- `build_status_rects(w, h) -> Vec<RectInstance>` for background
- `build_status_text(...) -> StatusLabel` for text content

TabBarRenderer follows the same pattern:
```rust
pub struct TabBarRenderer {
    cell_width: f32,
    cell_height: f32,
}

pub struct TabLabel {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub color: Rgb,
}

impl TabBarRenderer {
    pub fn new(cell_width: f32, cell_height: f32) -> Self { ... }

    /// Build background rects for each tab + the tab bar background.
    pub fn build_tab_rects(
        &self,
        tabs: &[TabDisplayInfo],  // (title, is_active)
        width: f32,
    ) -> Vec<RectInstance> { ... }

    /// Build text labels for each tab title.
    pub fn build_tab_text(
        &self,
        tabs: &[TabDisplayInfo],
    ) -> Vec<TabLabel> { ... }
}
```

### Pattern 3: spawn_pty with Working Directory
**What:** Add `working_directory: Option<&str>` parameter to `spawn_pty`
**When to use:** New tab creation inheriting CWD from current tab

Currently `spawn_pty` hardcodes `working_directory: None` in TtyOptions. Add an optional parameter:
```rust
pub fn spawn_pty(
    event_proxy: EventProxy,
    proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    window_id: WindowId,
    shell_override: Option<&str>,
    working_directory: Option<&std::path::Path>,  // NEW
    max_output_capture_kb: u32,
    pipes_enabled: bool,
) -> (PtySender, Arc<FairMutex<Term<EventProxy>>>) {
    let options = TtyOptions {
        working_directory: working_directory.map(|p| p.to_path_buf()),
        // ... rest unchanged
    };
}
```

### Pattern 4: Session Cleanup on Tab Close
**What:** Proper teardown of PTY and resources when a tab is closed
**When to use:** Tab close via shortcut, middle-click, X button, or shell exit

```rust
fn cleanup_session(session: Session) {
    // 1. Send Shutdown to PTY to trigger clean exit
    let _ = session.pty_sender.send(PtyMsg::Shutdown);
    // 2. Drop the session -- this drops:
    //    - Arc<FairMutex<Term>> (refcount decreases)
    //    - BlockManager (in-memory, freed)
    //    - HistoryDb (Option, closes SQLite on Drop)
    //    - SnapshotStore (Option, closes on Drop)
    //    - active_watcher (Option, stops watcher on Drop)
    // PtySender channel close signals reader thread to exit
    drop(session);
}
```

### Pattern 5: Tab Bar Layout in Frame
**What:** Reserve tab bar height at top of window, shift grid content down
**When to use:** When rendering with multiple tabs

The tab bar occupies one `cell_height` row at the top of the window. The terminal grid content area starts at y = cell_height (tab bar) and ends at y = height - cell_height (status bar). When there's only one tab, the tab bar can optionally be hidden (user preference, defer to Claude's discretion).

Key integration points in `draw_frame`:
1. Tab bar background rects: added before grid rects
2. Tab bar text: added to overlay_buffers
3. Grid rendering: y-offset shifted down by cell_height
4. Terminal size: subtract 1 more line for the tab bar (num_lines - 2 total: 1 status + 1 tab bar)

### Anti-Patterns to Avoid
- **Separate render pass for tab bar:** The existing RectRenderer supports drawing ranges of instances in a single pass. Do NOT create a separate GPU pipeline for the tab bar -- just add rect instances to the existing batch.
- **Tab state outside SessionMux:** All tab state must live in SessionMux. Do NOT store tab-related state in WindowContext or Processor -- that defeats the extraction done in Phase 21.
- **Spawning PTY on main thread blocking the event loop:** `spawn_pty` is fast (ConPTY creation is ~1ms) so blocking is acceptable, but the initial shell integration injection and HistoryDb/SnapshotStore opening should remain non-fatal as they already are.
- **Forgetting to resize new tab's Term:** After spawning a new PTY, the Term must be resized to match the current window dimensions (minus tab bar and status bar lines). The existing pattern in `resumed()` shows exactly how.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Colored rectangles | Custom tab rendering | RectRenderer (existing) | Already handles instanced quads with alpha blending |
| Text labels | Custom text pipeline | GlyphCache + glyphon Buffer (existing) | Already handles font shaping and glyph atlas |
| Session state management | Ad-hoc HashMap | SessionMux (existing) | Already has the right data structures |
| PTY lifecycle | Manual process management | spawn_pty + PtyMsg::Shutdown (existing) | Already handles reader thread, polling, cleanup |
| Cross-platform shortcuts | Hardcoded key combos | is_glass_shortcut + is_action_modifier (existing) | Already handles Cmd vs Ctrl+Shift per platform |

**Key insight:** Nearly everything needed for tabs already exists as infrastructure. The work is connecting existing pieces, not building new ones.

## Common Pitfalls

### Pitfall 1: Borrow Conflicts with SessionMux
**What goes wrong:** Attempting to borrow `session_mux` mutably for the active session while also reading tab list for rendering.
**Why it happens:** The render path needs tab titles (immutable) AND the focused session's data (immutable from different part of the struct).
**How to avoid:** Clone tab display info (titles, active index) into owned data before rendering, same pattern used for `visible_blocks` and `status_clone` in the existing `RedrawRequested` handler.
**Warning signs:** Compile errors about conflicting borrows on `session_mux`.

### Pitfall 2: TabId vs Tab Index Confusion
**What goes wrong:** Using TabId (stable identifier) where tab index (position in Vec) is needed, or vice versa.
**Why it happens:** Tabs can be reordered or closed, changing indices but not IDs.
**How to avoid:** Use index for rendering and keyboard navigation (Ctrl+1-9), use TabId for stable references. close_tab takes index, not TabId.
**Warning signs:** Wrong tab activated after closing a middle tab.

### Pitfall 3: Zombie PTY Processes
**What goes wrong:** PTY reader thread keeps running after tab close, or child shell process not killed.
**Why it happens:** PtyMsg::Shutdown not sent, or sender channel not dropped cleanly.
**How to avoid:** Always send PtyMsg::Shutdown before dropping Session. The PTY reader thread checks for Shutdown and exits its loop. Dropping PtySender closes the mpsc channel, which also signals the reader thread.
**Warning signs:** Process count grows with each tab open/close cycle.

### Pitfall 4: Terminal Resize Mismatch on New Tab
**What goes wrong:** New tab's terminal grid has wrong dimensions (default 80x24 instead of current window size).
**Why it happens:** spawn_pty creates Term at 80x24 default, and resize is forgotten.
**How to avoid:** Immediately after spawn_pty, send PtyMsg::Resize with current window dimensions and call term.lock().resize() -- exactly as done in the existing `resumed()` flow.
**Warning signs:** Terminal content wraps incorrectly in new tabs.

### Pitfall 5: TerminalExit Handling with Multiple Tabs
**What goes wrong:** Shell exit in one tab closes the entire window.
**Why it happens:** Current TerminalExit handler calls `event_loop.exit()`.
**How to avoid:** On TerminalExit, close only the specific tab (by session_id). Only exit the event loop when the last tab is closed.
**Warning signs:** Typing `exit` in one tab kills all tabs.

### Pitfall 6: Tab Bar Click Hit-Testing Off by One
**What goes wrong:** Clicking on a tab activates the wrong one.
**Why it happens:** Tab widths calculated differently during rendering vs hit-testing.
**How to avoid:** Store tab x-ranges in a structure during render, use the same structure for hit-testing. Or compute tab widths deterministically from tab count and window width.
**Warning signs:** Clicking the rightmost tab activates the one to its left.

## Code Examples

### Session Creation for New Tab (from existing `resumed()` pattern)
```rust
// In main.rs, extracted as a helper function:
fn create_session(
    proxy: &EventLoopProxy<AppEvent>,
    window_id: WindowId,
    session_id: SessionId,
    config: &GlassConfig,
    working_directory: Option<&std::path::Path>,
    cell_w: f32,
    cell_h: f32,
    window_width: u32,
    window_height: u32,
    tab_bar_lines: u16,  // 1 when tab bar visible, 0 when hidden
) -> Session {
    let event_proxy = EventProxy::new(proxy.clone(), window_id, session_id);
    let max_output_kb = config.history.as_ref()
        .map(|h| h.max_output_capture_kb)
        .unwrap_or(50);
    let pipes_enabled = config.pipes.as_ref()
        .map(|p| p.enabled)
        .unwrap_or(true);

    let (pty_sender, term) = spawn_pty(
        event_proxy,
        proxy.clone(),
        window_id,
        config.shell.as_deref(),
        working_directory,
        max_output_kb,
        pipes_enabled,
    );

    // Resize to current window dimensions (subtract status bar + tab bar)
    let num_cols = (window_width as f32 / cell_w).floor().max(1.0) as u16;
    let num_lines = ((window_height as f32 / cell_h).floor().max(2.0) as u16)
        .saturating_sub(1)  // status bar
        .saturating_sub(tab_bar_lines);  // tab bar
    let size = WindowSize { num_lines, num_cols, cell_width: cell_w as u16, cell_height: cell_h as u16 };
    let _ = pty_sender.send(PtyMsg::Resize(size));
    term.lock().resize(TermDimensions { columns: num_cols as usize, screen_lines: num_lines as usize });

    // Open history/snapshot stores
    let cwd = working_directory.unwrap_or(&std::env::current_dir().unwrap_or_default());
    let history_db = HistoryDb::open(&resolve_db_path(cwd)).ok();
    let snapshot_store = {
        let glass_dir = glass_snapshot::resolve_glass_dir(cwd);
        glass_snapshot::SnapshotStore::open(&glass_dir).ok()
    };

    Session {
        id: session_id,
        pty_sender,
        term,
        default_colors: DefaultColors::default(),
        block_manager: BlockManager::new(),
        status: StatusState::default(),
        history_db,
        last_command_id: None,
        command_started_wall: None,
        search_overlay: None,
        snapshot_store,
        pending_command_text: None,
        active_watcher: None,
        pending_snapshot_id: None,
        pending_parse_confidence: None,
        cursor_position: None,
        title: String::from("Glass"),
    }
}
```

### Keyboard Shortcut Wiring
```rust
// In the KeyboardInput handler, after search overlay handling:
// Tab shortcuts use is_glass_shortcut (Ctrl+Shift on Win/Linux, Cmd on macOS)
if glass_mux::is_glass_shortcut(modifiers) {
    match &event.logical_key {
        Key::Character(c) if c.as_str().eq_ignore_ascii_case("t") => {
            // New tab: inherit CWD from current session
            let cwd = ctx.session().status.cwd().to_string();
            let session_id = ctx.session_mux.next_session_id();
            let session = create_session(/* ... */, Some(Path::new(&cwd)));
            ctx.session_mux.add_tab(session);
            // Inject shell integration for new tab
            inject_shell_integration(&ctx.session().pty_sender, &config);
            ctx.window.request_redraw();
            return;
        }
        Key::Character(c) if c.as_str().eq_ignore_ascii_case("w") => {
            // Close current tab
            let idx = ctx.session_mux.active_tab_index();
            if let Some(session) = ctx.session_mux.close_tab(idx) {
                cleanup_session(session);
            }
            if ctx.session_mux.tab_count() == 0 {
                // Last tab closed -- exit or keep empty window (configurable)
                self.windows.remove(&window_id);
                event_loop.exit();
                return;
            }
            ctx.window.request_redraw();
            return;
        }
        _ => {}
    }
}

// Ctrl+Tab / Ctrl+Shift+Tab: cycle tabs
if modifiers.control_key() {
    match &event.logical_key {
        Key::Named(NamedKey::Tab) => {
            if modifiers.shift_key() {
                ctx.session_mux.prev_tab();
            } else {
                ctx.session_mux.next_tab();
            }
            ctx.window.request_redraw();
            return;
        }
        _ => {}
    }
}

// Ctrl+1-9 / Cmd+1-9: jump to tab by index
if glass_mux::is_action_modifier(modifiers) {
    if let Key::Character(c) = &event.logical_key {
        if let Some(digit) = c.as_str().chars().next().and_then(|c| c.to_digit(10)) {
            if digit >= 1 && digit <= 9 {
                let target = (digit as usize) - 1;
                ctx.session_mux.activate_tab(target);
                ctx.window.request_redraw();
                return;
            }
        }
    }
}
```

### Tab Title from OSC 7 / Process Name
```rust
// In Shell event handler, update tab title when CWD changes:
if let ShellEvent::CurrentDirectory(ref path) = shell_event {
    // Derive title from last path component
    let title = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    session.title = title;
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Single session in WindowContext | SessionMux with Session extraction | Phase 21 | Enables multi-tab without refactoring |
| Hardcoded working_directory: None | Parameterized working_directory | Phase 23 (this) | Enables CWD inheritance |
| TerminalExit closes window | TerminalExit closes tab | Phase 23 (this) | Multi-tab lifecycle |

## Open Questions

1. **Tab bar visibility with single tab**
   - What we know: Most terminals hide the tab bar when only one tab exists
   - Recommendation: Always show tab bar for consistency during Phase 23. Single-tab hiding can be a config option later.

2. **Last-tab-closed behavior**
   - What we know: GOAL.md lists "close window or keep empty" as options
   - Recommendation: Close the window (exit event loop). This matches most terminal emulators. Can be made configurable later.

3. **Tab bar position (top vs bottom)**
   - What we know: Most terminals use top. Status bar is at bottom.
   - Recommendation: Top of window. Tab bar at top, terminal content in middle, status bar at bottom.

4. **Tab width strategy**
   - What we know: Options are fixed width, proportional, or min/max clamped
   - Recommendation: Equal width tabs that shrink to fit, with a minimum width. Simple and predictable. Max ~20 chars per tab title, truncate with ellipsis.

5. **Resize propagation to non-active tabs**
   - What we know: Window resize currently only resizes the focused session
   - Recommendation: Resize ALL sessions on window resize, not just the active one. Background tabs should be ready to display at the correct size when activated.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in Rust testing) |
| Config file | Cargo.toml workspace |
| Quick run command | `cargo test -p glass_mux` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| TAB-01 | SessionMux add_tab/close_tab/activate_tab | unit | `cargo test -p glass_mux -- session_mux` | Partial (existing tests cover ID generation) |
| TAB-02 | Tab cycling wraps around correctly | unit | `cargo test -p glass_mux -- tab_cycle` | Wave 0 |
| TAB-03 | Close middle tab adjusts active_tab | unit | `cargo test -p glass_mux -- close_tab` | Wave 0 |
| TAB-04 | Tab bar rect rendering | unit | `cargo test -p glass_renderer -- tab_bar` | Wave 0 |
| TAB-05 | 50-tab rapid create/close no panics | integration | Manual stress test | Manual-only: requires PTY |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_mux && cargo test -p glass_renderer`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `glass_mux/src/session_mux.rs` -- add unit tests for add_tab, close_tab, activate_tab, next_tab, prev_tab
- [ ] `glass_renderer/src/tab_bar.rs` -- add unit tests for rect/text generation

## Sources

### Primary (HIGH confidence)
- Project source code: glass_mux/src/session_mux.rs, tab.rs, types.rs -- existing SessionMux infrastructure
- Project source code: glass_renderer/src/frame.rs, rect_renderer.rs, status_bar.rs -- rendering patterns
- Project source code: glass_terminal/src/pty.rs -- spawn_pty signature and TtyOptions
- Project source code: src/main.rs -- event loop, keyboard handling, session creation pattern
- Project source code: glass_core/src/event.rs -- AppEvent with session_id routing

### Secondary (MEDIUM confidence)
- Project GOAL.md -- phase deliverables and test gate

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - everything is already in the workspace, no new dependencies
- Architecture: HIGH - patterns directly follow existing StatusBarRenderer, BlockRenderer, SearchOverlayRenderer
- Pitfalls: HIGH - identified from direct code analysis of borrow patterns, event handling, and PTY lifecycle
- Session lifecycle: HIGH - spawn_pty, EventProxy, PtyMsg::Shutdown all inspected directly

**Research date:** 2026-03-06
**Valid until:** 2026-04-06 (stable -- internal project patterns, no external API dependencies)

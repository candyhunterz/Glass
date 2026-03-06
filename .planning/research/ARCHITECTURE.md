# Architecture Patterns

**Domain:** Cross-platform terminal emulator with tabs/split panes
**Researched:** 2026-03-06

## Current Architecture (v1.3)

Before describing changes, here is what exists today:

```
                    winit EventLoop<AppEvent>
                           |
                    Processor (ApplicationHandler)
                           |
               HashMap<WindowId, WindowContext>
                           |
         +-----------+-----+------+-----------+
         |           |            |            |
    GlassRenderer  FrameRenderer  PtySender   Term (FairMutex)
    (wgpu surface) (render pipeline) |         |
         |           |            PTY reader   BlockManager
         |           |            thread       StatusState
         |           |            (std::thread) HistoryDb
         |           |                         SnapshotStore
         +-----+-----+
               |
        Single render pass:
        clear -> rects -> text -> present
```

Key characteristics:
- **One WindowContext per window** (currently always one window)
- **One PTY per WindowContext** (ConPTY, dedicated std::thread reader)
- **One Term grid per PTY** (alacritty_terminal Term<EventProxy>)
- **One FrameRenderer per window** (owns GlyphCache, GridRenderer, RectRenderer)
- **AppEvent routed by WindowId** to the correct WindowContext
- **GridSnapshot** extracted under brief FairMutex lock, rendered without lock held

## Target Architecture (v2.0)

### High-Level Changes

```
                    winit EventLoop<AppEvent>
                           |
                    Processor (ApplicationHandler)
                           |
               HashMap<WindowId, WindowContext>
                           |
         +----------+------+-------+-----------+
         |          |              |            |
    GlassRenderer  FrameRenderer  SessionMux   TabBar (NEW)
    (wgpu surface) (render pipeline)  (NEW)
                        |              |
                   Viewport layout     +-- Tab 0
                   calculator (NEW)    |     +-- SplitTree (NEW)
                        |              |     |     +-- Leaf: Session { pty, term, blocks, status, ... }
                        |              |     |     +-- Leaf: Session { pty, term, blocks, status, ... }
                        |              +-- Tab 1
                        |                    +-- SplitTree
                        |                          +-- Leaf: Session { pty, term, blocks, status, ... }
                        |
                   Renders each visible Session
                   into its viewport rect
```

### Component Boundaries

| Component | Responsibility | Communicates With | New/Modified |
|-----------|---------------|-------------------|--------------|
| `Processor` | winit event loop, keyboard dispatch, window lifecycle | WindowContext, SessionMux | **Modified** -- routes keys/events to focused session via SessionMux |
| `WindowContext` | Per-window GPU state, compositor | GlassRenderer, FrameRenderer, SessionMux | **Modified** -- replaces single-PTY fields with SessionMux |
| `SessionMux` | Tab/pane tree, focus tracking, session lifecycle | Session, TabBar | **NEW crate: glass_mux** |
| `Session` | Single terminal session (PTY + Term + blocks + status + history + snapshot) | PtySender, Term, BlockManager, HistoryDb, SnapshotStore | **NEW struct** (extracted from WindowContext fields) |
| `SplitTree` | Binary tree of horizontal/vertical splits with size ratios | Session (leaves) | **NEW** in glass_mux |
| `TabBar` | Tab strip rendering (titles, close buttons, active indicator) | SessionMux (reads tab list) | **NEW** in glass_renderer |
| `ViewportLayout` | Computes pixel rects for each visible session from SplitTree | SplitTree, FrameRenderer | **NEW** in glass_mux |
| `GlassRenderer` | wgpu surface, device, queue | FrameRenderer | **Modified** -- backend selection via cfg, no API change |
| `FrameRenderer` | Render pipeline (rects, text, overlays) | GridSnapshot, Block, StatusState | **Modified** -- called per-session with viewport rect (scissor) |
| `spawn_pty` | PTY creation and reader thread | EventProxy, AppEvent | **Modified** -- platform-conditional shell detection |
| `GlassConfig` | TOML configuration | All consumers | **Modified** -- platform-aware defaults (font, shell) |
| `AppEvent` | Event enum routed through winit proxy | PTY threads -> Processor | **Modified** -- adds SessionId to variants |

### New Crate: glass_mux

This is the only new crate needed. It owns the session multiplexer logic.

```
glass_mux/
  src/
    lib.rs          -- pub exports
    session.rs      -- Session struct (extracted from WindowContext)
    session_mux.rs  -- SessionMux: tabs vec, active tab index, focus tracking
    split_tree.rs   -- SplitTree: binary tree of splits with ratio
    layout.rs       -- ViewportLayout: compute pixel rects from split tree
    tab.rs          -- Tab: wraps SplitTree + tab metadata (title, id)
    types.rs        -- SessionId, TabId, SplitDirection, FocusDirection
```

Dependencies: `glass_core` (for AppEvent, GlassConfig), `glass_terminal` (for Session internals).

## Detailed Design: Five Integration Points

### 1. PTY Abstraction Layer

**Current state:** `pty.rs` calls `alacritty_terminal::tty::new()` directly. alacritty_terminal already provides cross-platform PTY support internally -- ConPTY on Windows, forkpty on Unix. The `polling` crate (v3) already abstracts epoll/kqueue/IOCP.

**What needs to change:** Almost nothing in the PTY read loop. The alacritty_terminal tty module handles platform differences. Changes are:

1. **Shell detection** in `spawn_pty()` -- currently Windows-only (pwsh/powershell). Add:
   - macOS: detect zsh (default), bash, fish
   - Linux: read `$SHELL` or fall back to bash

2. **Shell integration injection** in `main.rs` -- currently PowerShell only. Add:
   - bash: `. ~/.glass/shell-integration/glass.bash`
   - zsh: `. ~/.glass/shell-integration/glass.zsh`
   - fish: `source ~/.glass/shell-integration/glass.fish`

3. **Environment variables** -- TERM already set to `xterm-256color`, which is correct cross-platform.

**No new PTY abstraction trait needed.** The alacritty_terminal tty module IS the abstraction. Glass's custom read loop (`glass_pty_loop`) works identically because it only uses `pty.reader().read()`, `pty.writer().write()`, `pty.on_resize()`, and `pty.register()/deregister()` -- all of which are trait methods on `EventedPty`/`EventedReadWrite` that alacritty_terminal implements per-platform.

The one platform-specific concern: `tty::PTY_CHILD_EVENT_TOKEN` and `tty::PTY_READ_WRITE_TOKEN` -- these are constants from alacritty_terminal that differ per platform. The current code already uses them correctly.

```rust
// spawn_pty changes (pseudocode):
pub fn spawn_pty(
    // ... existing params ...
    session_id: SessionId,  // NEW: identify which session this PTY belongs to
) -> (PtySender, Arc<FairMutex<Term<EventProxy>>>) {
    let shell_program = if let Some(shell) = shell_override {
        shell.to_owned()
    } else {
        #[cfg(target_os = "windows")]
        { detect_windows_shell() }  // existing pwsh/powershell logic
        #[cfg(target_os = "macos")]
        { std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into()) }
        #[cfg(target_os = "linux")]
        { std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into()) }
    };
    // ... rest unchanged ...
}
```

### 2. wgpu Backend Selection

**Current state:** `surface.rs` uses `#[cfg(target_os = "windows")] backends: wgpu::Backends::DX12` and `#[cfg(not(target_os = "windows"))] backends: wgpu::Backends::all()`.

**This is already correct for cross-platform.** wgpu 28 auto-selects the best backend when `Backends::all()` is specified:
- macOS: Metal (only option)
- Linux: Vulkan (preferred), GL (fallback)
- Windows: DX12 (forced, already configured)

The existing `#[cfg]` pattern in `surface.rs` is the recommended approach. No changes needed beyond what is already there.

**One consideration:** On Linux with older hardware, Vulkan may not be available. The `Backends::all()` fallback to GL handles this. The `WGPU_BACKEND` environment variable can override (built into wgpu).

### 3. Session Multiplexer Architecture

This is the largest new component. Design inspired by WezTerm's Mux but much simpler (no remote sessions, no client-server).

```rust
// glass_mux/src/types.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub u64);

#[derive(Debug, Clone, Copy)]
pub enum SplitDirection { Horizontal, Vertical }

#[derive(Debug, Clone, Copy)]
pub enum FocusDirection { Up, Down, Left, Right }
```

```rust
// glass_mux/src/session.rs
// Extracted from WindowContext -- one per terminal pane
pub struct Session {
    pub id: SessionId,
    pub pty_sender: PtySender,
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    pub default_colors: DefaultColors,
    pub block_manager: BlockManager,
    pub status: StatusState,
    pub history_db: Option<HistoryDb>,
    pub snapshot_store: Option<SnapshotStore>,
    pub last_command_id: Option<i64>,
    pub command_started_wall: Option<std::time::SystemTime>,
    pub search_overlay: Option<SearchOverlay>,
    pub pending_command_text: Option<String>,
    pub active_watcher: Option<FsWatcher>,
    pub pending_snapshot_id: Option<i64>,
    pub pending_parse_confidence: Option<Confidence>,
    pub cursor_position: Option<(f64, f64)>,
    // NEW: tab title derived from shell CWD or process name
    pub title: String,
}
```

```rust
// glass_mux/src/split_tree.rs
pub enum SplitNode {
    Leaf(SessionId),
    Split {
        direction: SplitDirection,
        ratio: f32,  // 0.0..1.0, fraction allocated to first child
        first: Box<SplitNode>,
        second: Box<SplitNode>,
    },
}

impl SplitNode {
    /// Compute viewport rects for all leaf sessions given a bounding rect.
    pub fn layout(&self, bounds: Rect) -> Vec<(SessionId, Rect)> { ... }

    /// Find the session in a given direction from the focused session.
    pub fn navigate(&self, from: SessionId, dir: FocusDirection) -> Option<SessionId> { ... }

    /// Split the leaf containing `target` in the given direction.
    /// Returns the new SessionId for the created pane.
    pub fn split(&mut self, target: SessionId, dir: SplitDirection, new_id: SessionId) { ... }

    /// Remove a session, collapsing its parent split.
    pub fn remove(&mut self, target: SessionId) -> bool { ... }
}
```

```rust
// glass_mux/src/tab.rs
pub struct Tab {
    pub id: TabId,
    pub tree: SplitNode,
    pub focused_session: SessionId,
    pub title: String,  // derived from focused session
}
```

```rust
// glass_mux/src/session_mux.rs
pub struct SessionMux {
    tabs: Vec<Tab>,
    active_tab: usize,
    sessions: HashMap<SessionId, Session>,
    next_id: u64,
}

impl SessionMux {
    pub fn new_tab(&mut self, /* PTY spawn params */) -> TabId { ... }
    pub fn close_tab(&mut self, id: TabId) { ... }
    pub fn switch_tab(&mut self, index: usize) { ... }
    pub fn next_tab(&mut self) { ... }
    pub fn prev_tab(&mut self) { ... }

    pub fn split(&mut self, dir: SplitDirection) -> SessionId { ... }
    pub fn close_pane(&mut self, id: SessionId) { ... }
    pub fn focus_direction(&mut self, dir: FocusDirection) { ... }

    pub fn focused_session(&self) -> Option<&Session> { ... }
    pub fn focused_session_mut(&mut self) -> Option<&mut Session> { ... }

    pub fn active_tab(&self) -> Option<&Tab> { ... }

    /// Get all visible sessions with their viewport rects for rendering.
    pub fn visible_sessions(&self, window_rect: Rect) -> Vec<(SessionId, Rect, &Session)> { ... }

    /// Route an AppEvent to the correct session by SessionId.
    pub fn route_event(&mut self, session_id: SessionId) -> Option<&mut Session> { ... }
}
```

### 4. Renderer Changes for Multiple Terminal Views

**Current state:** `FrameRenderer::draw_frame()` renders one GridSnapshot to the full window.

**What needs to change:** The FrameRenderer draws each visible session into a scissor-clipped viewport rect. The key insight: the existing `draw_frame` already takes `width` and `height` parameters. The change is to call it multiple times with different viewport offsets.

**Approach: Scissor rect per session, shared render pass.**

```rust
// Modified draw flow in WindowContext (pseudocode):
fn render_frame(&mut self) {
    let frame = self.renderer.get_current_texture()?;
    let view = frame.texture.create_view(&Default::default());

    // 1. Draw tab bar at top (if >1 tab)
    if self.session_mux.tab_count() > 1 {
        self.tab_bar_renderer.draw(&view, &self.session_mux);
    }

    // 2. Compute available area (subtract tab bar height)
    let tab_bar_height = if self.session_mux.tab_count() > 1 { TAB_BAR_HEIGHT } else { 0 };
    let content_rect = Rect { x: 0, y: tab_bar_height, w: width, h: height - tab_bar_height };

    // 3. For each visible session in the active tab's split tree:
    for (session_id, viewport_rect, session) in self.session_mux.visible_sessions(content_rect) {
        let snapshot = {
            let term = session.term.lock();
            snapshot_term(&term, &session.default_colors)
        };
        let visible_blocks = session.block_manager.visible_blocks(...);

        // Draw with viewport offset and scissor
        self.frame_renderer.draw_frame_viewport(
            device, queue, &view,
            viewport_rect,      // NEW: position and size of this pane
            &snapshot,
            &visible_blocks,
            Some(&session.status),
            search_overlay,
        );
    }

    // 4. Draw split dividers (1px lines between panes)
    self.draw_split_dividers(&view, &self.session_mux);

    frame.present();
}
```

**FrameRenderer changes:**
- New method `draw_frame_viewport()` that takes a `Rect` instead of full `(width, height)`
- Uses wgpu scissor rect to clip rendering to the pane's area
- Offsets all positions by the viewport's (x, y) origin
- The existing `draw_frame()` becomes a convenience wrapper calling `draw_frame_viewport` with full window rect

**GlyphCache/FontSystem sharing:** All sessions in a window share the same `FrameRenderer` (and thus the same `GlyphCache` and `FontSystem`). This is critical -- FontSystem is expensive to create and holds the font atlas. Do NOT create a FrameRenderer per session.

### 5. Event Routing with Multiple Sessions

**Current state:** `AppEvent` variants carry `WindowId`. The Processor looks up `WindowContext` by WindowId.

**What needs to change:** AppEvent needs `SessionId` so events from PTY threads route to the correct session.

```rust
// Modified AppEvent:
#[derive(Debug, Clone)]
pub enum AppEvent {
    TerminalDirty { window_id: WindowId },  // unchanged -- any dirty triggers redraw
    SetTitle { window_id: WindowId, session_id: SessionId, title: String },
    TerminalExit { window_id: WindowId, session_id: SessionId },
    Shell { window_id: WindowId, session_id: SessionId, event: ShellEvent, line: usize },
    GitInfo { window_id: WindowId, session_id: SessionId, info: Option<GitStatus> },
    CommandOutput { window_id: WindowId, session_id: SessionId, raw_output: Vec<u8> },
}
```

**Keyboard routing:**
- Global shortcuts (Ctrl+Shift+T new tab, Ctrl+Shift+W close tab, Ctrl+Tab switch tab, Ctrl+Shift+D split, etc.) handled by Processor before reaching any session
- All other keyboard input forwarded to `session_mux.focused_session().pty_sender`
- Mouse events: hit-test against viewport rects to determine which session receives the event; click also changes focus

```rust
// In Processor::window_event, keyboard handling:
fn handle_key(&mut self, ctx: &mut WindowContext, key: Key, mods: ModifiersState) {
    // Global shortcuts first
    match (mods, &key) {
        (CTRL_SHIFT, Key::Named(NamedKey::T)) => { ctx.session_mux.new_tab(...); return; }
        (CTRL_SHIFT, Key::Named(NamedKey::W)) => { ctx.session_mux.close_focused_pane(); return; }
        (CTRL, Key::Named(NamedKey::Tab)) => { ctx.session_mux.next_tab(); return; }
        (CTRL_SHIFT, Key::Character("d")) => { ctx.session_mux.split(Horizontal); return; }
        (CTRL_SHIFT, Key::Character("e")) => { ctx.session_mux.split(Vertical); return; }
        (ALT, Key::Named(NamedKey::ArrowLeft)) => { ctx.session_mux.focus_direction(Left); return; }
        // ... etc
        _ => {}
    }

    // Forward to focused session
    if let Some(session) = ctx.session_mux.focused_session() {
        let bytes = encode_key(key, mods, session.term.lock().mode());
        session.pty_sender.send(PtyMsg::Input(bytes));
    }
}
```

## Crate Modification Map

### No changes needed
- `glass_protocol` -- protocol types are session-agnostic
- `glass_pipes` -- pipe parsing is session-agnostic

### Minor changes (add SessionId parameter)
- `glass_terminal` -- `spawn_pty()` takes SessionId, passes it in AppEvent
- `glass_core` -- AppEvent variants get SessionId field
- `glass_history` -- no API change, but each Session opens its own HistoryDb
- `glass_snapshot` -- no API change, each Session opens its own SnapshotStore
- `glass_mcp` -- no change (separate process, reads DB directly)

### Moderate changes
- `glass_config` -- platform-aware defaults (font: "SF Mono"/"Menlo" on macOS, "Consolas" on Windows, "Monospace" on Linux; shell detection per-platform)
- `glass_renderer` -- new `draw_frame_viewport()` method, `TabBarRenderer` module, split divider rendering

### Major changes
- `glass_mux` -- **NEW CRATE** (session, split tree, tab, mux, layout)
- Root `src/main.rs` -- WindowContext restructured to use SessionMux, keyboard routing rewritten, render loop iterates sessions

## Data Flow: New Tab Creation

```
User presses Ctrl+Shift+T
  -> Processor::handle_key()
  -> ctx.session_mux.new_tab()
    -> SessionMux generates new SessionId, TabId
    -> spawn_pty(event_proxy, proxy, window_id, session_id, shell, ...)
      -> Returns (PtySender, Arc<FairMutex<Term>>)
    -> Open HistoryDb for CWD
    -> Open SnapshotStore for CWD
    -> Create Session { id, pty_sender, term, block_manager, status, history_db, ... }
    -> Create Tab { id, tree: SplitNode::Leaf(session_id), focused: session_id }
    -> Insert into tabs vec, set active_tab
    -> Inject shell integration via PtyMsg::Input
  -> window.request_redraw()
```

## Data Flow: Split Pane Creation

```
User presses Ctrl+Shift+D (horizontal split)
  -> Processor::handle_key()
  -> ctx.session_mux.split(Horizontal)
    -> Find focused session in active tab's SplitTree
    -> Generate new SessionId
    -> spawn_pty(...) for new session
    -> Replace Leaf(focused_id) with Split { Horizontal, 0.5, Leaf(focused_id), Leaf(new_id) }
    -> Resize BOTH PTYs: compute new cell dimensions from split viewport rects
  -> window.request_redraw()
```

## Data Flow: Rendering Multiple Panes

```
RedrawRequested
  -> For each (session_id, rect) in active_tab.tree.layout(content_area):
    -> session = session_mux.sessions[session_id]
    -> snapshot = snapshot_term(&session.term.lock(), &session.default_colors)
    -> visible_blocks = session.block_manager.visible_blocks(...)
    -> frame_renderer.draw_frame_viewport(device, queue, view, rect, snapshot, blocks, status, overlay)
  -> Draw split dividers between pane rects
  -> Draw tab bar (if multiple tabs)
  -> frame.present()
```

## Patterns to Follow

### Pattern 1: Extract-Then-Render (existing, extend to multi-session)
**What:** Lock Term briefly to extract GridSnapshot, release lock, render from snapshot.
**When:** Always -- this is critical for input latency.
**Why for v2.0:** With multiple sessions, you must NOT hold multiple FairMutex locks simultaneously. Extract snapshots sequentially, then render all.

### Pattern 2: SharedRenderer, IndependentSessions
**What:** One FrameRenderer (and GlyphCache/FontSystem) per window, shared across all sessions. Each session owns its own PTY, Term, BlockManager, etc.
**When:** Always.
**Why:** FontSystem is expensive (~35ms to create, holds font atlas). Creating one per session would be wasteful and cause font atlas fragmentation.

### Pattern 3: SessionId in Events
**What:** Every AppEvent from a PTY thread includes both WindowId and SessionId.
**When:** All PTY-to-main-thread communication.
**Why:** With multiple PTY threads sending events, the main thread must know which session the event belongs to. WindowId alone is ambiguous when multiple sessions exist in one window.

### Pattern 4: cfg-gated Platform Code (not trait abstraction)
**What:** Use `#[cfg(target_os = "...")]` for platform-specific code rather than a trait-based abstraction layer.
**When:** Shell detection, shell integration injection, config defaults, keyboard shortcuts.
**Why:** The number of platform-specific points is small (~5 functions). A trait-based abstraction would be over-engineering. alacritty_terminal already abstracts the hard part (PTY). The `polling` crate already abstracts I/O polling. wgpu already abstracts GPU backends. Just use cfg for the remaining glue.

## Anti-Patterns to Avoid

### Anti-Pattern 1: One FrameRenderer per Session
**What:** Creating a separate FrameRenderer/GlyphCache for each terminal pane.
**Why bad:** Font atlas duplication, GPU memory waste, ~35ms overhead per new pane.
**Instead:** Share one FrameRenderer, call draw_frame_viewport() per session with scissor rect.

### Anti-Pattern 2: PTY Abstraction Trait
**What:** Creating a `trait Pty { fn read(); fn write(); }` wrapper over alacritty_terminal's tty.
**Why bad:** alacritty_terminal already IS the cross-platform PTY abstraction. Adding another layer adds complexity without value.
**Instead:** Use alacritty_terminal::tty directly, with cfg-gated shell detection in spawn_pty.

### Anti-Pattern 3: Global Session Registry (Arc<Mutex<SessionMux>>)
**What:** Making SessionMux a global singleton shared between threads.
**Why bad:** The winit event loop is single-threaded. SessionMux is only accessed from the main thread. Adding Arc<Mutex> creates unnecessary contention and deadlock risk.
**Instead:** SessionMux is owned by WindowContext, accessed only in ApplicationHandler callbacks.

### Anti-Pattern 4: Separate wgpu Surface per Pane
**What:** Creating a wgpu Surface for each terminal pane.
**Why bad:** Each surface requires its own swapchain. Multiple swapchains in one window cause tearing and synchronization issues.
**Instead:** One surface per window. Use scissor rects and viewport offsets to render panes into sub-regions.

## Suggested Build Order

The build order must respect dependency chains and allow incremental testing.

### Phase 1: Session Extraction (foundation, no user-visible change)
1. Create `glass_mux` crate with Session struct (move fields from WindowContext)
2. Create SessionMux with single-tab, single-session mode
3. Refactor WindowContext to use SessionMux instead of inline fields
4. Add SessionId to AppEvent variants
5. Update event routing in Processor to go through SessionMux
6. **Test:** Everything works exactly as before (regression test)

### Phase 2: Platform PTY + Backend (cross-platform, no UI change)
1. Add cfg-gated shell detection to spawn_pty()
2. Write shell integration scripts for bash/zsh/fish
3. Verify wgpu backend selection on macOS (Metal) and Linux (Vulkan/GL)
4. Platform-aware config defaults (font family, shell)
5. CI cross-compilation matrix
6. **Test:** Glass launches on macOS and Linux with correct shell, rendering works

### Phase 3: Tabs (user-visible feature, builds on Session extraction)
1. Implement Tab struct and tab management in SessionMux
2. Add TabBarRenderer to glass_renderer
3. Wire keyboard shortcuts (Ctrl+Shift+T/W, Ctrl+Tab, Ctrl+Shift+Tab)
4. Tab title from CWD or process name
5. Handle tab close (PTY shutdown, session cleanup)
6. Handle last-tab-closed (window close)
7. **Test:** Create/close/switch tabs, each has independent terminal session

### Phase 4: Split Panes (user-visible feature, builds on Tabs)
1. Implement SplitTree (binary tree with direction + ratio)
2. Implement ViewportLayout (compute pixel rects)
3. Add `draw_frame_viewport()` to FrameRenderer (scissor rect rendering)
4. Wire keyboard shortcuts (Ctrl+Shift+D/E for h/v split, Alt+arrows for focus)
5. Split divider rendering (1-2px lines between panes)
6. PTY resize on split (each pane gets correct cell dimensions)
7. Mouse click to focus pane
8. Pane close (collapse parent split)
9. **Test:** Split in both directions, focus navigation, resize, close

**Phase ordering rationale:**
- Phase 1 first because every subsequent phase depends on Session being extracted from WindowContext and SessionMux being the routing layer
- Phase 2 before 3/4 because cross-platform must work before adding UI complexity
- Phase 3 before 4 because tabs are simpler (no viewport subdivision) and validate the SessionMux design
- Phase 4 last because it requires all prior infrastructure (session extraction, multi-session rendering, event routing)

## Scalability Considerations

| Concern | 1-2 tabs | 10 tabs | 50+ tabs |
|---------|----------|---------|----------|
| Memory | ~120MB (current baseline + minimal overhead) | ~200MB (each inactive PTY + Term ~8MB) | ~500MB+ -- consider lazy PTY for background tabs |
| Render | One draw_frame call | One draw_frame call (only active tab renders) | Same -- only active tab's visible panes render |
| Font atlas | Shared, no scaling issue | Shared | Shared -- glyphon atlas grows with unique glyphs, not session count |
| PTY threads | 1 std::thread | 10 std::threads | Consider thread pool or async PTY at >20 sessions |

## Sources

- WezTerm Mux architecture: [DeepWiki - wezterm](https://deepwiki.com/wezterm/wezterm)
- alacritty_terminal cross-platform PTY: [GitHub - alacritty/alacritty](https://github.com/alacritty/alacritty)
- wgpu 28 backend selection: [wgpu docs](https://docs.rs/crate/wgpu/latest), [Backends documentation](https://docs.rs/wgpu/latest/wgpu/struct.Backends.html)
- polling crate cross-platform: [GitHub - smol-rs/polling](https://github.com/smol-rs/polling)
- Terminal multiplexer architecture in Rust: [implaustin - Terminal Multiplexer with Actors](https://implaustin.hashnode.dev/how-to-write-a-terminal-multiplexer-with-rust-async-and-actors-part-2)

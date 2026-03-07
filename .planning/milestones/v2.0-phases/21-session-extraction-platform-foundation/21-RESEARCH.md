# Phase 21: Session Extraction & Platform Foundation - Research

**Researched:** 2026-03-06
**Domain:** Rust refactoring -- extracting session state, platform cfg-gating, new crate creation
**Confidence:** HIGH

## Summary

Phase 21 is a pure refactoring phase with zero user-visible change. The goal is to extract the 15+ session-related fields from `WindowContext` (lines 119-157 of `src/main.rs`) into a `Session` struct in a new `glass_mux` crate, wrap it in a `SessionMux` that initially operates in single-session mode, add `SessionId` to all PTY-originated `AppEvent` variants, and add cfg-gated platform code for shell detection, config paths, and keyboard modifier mapping.

The existing v2.0 milestone research (`.planning/research/`) already produced detailed architecture, stack, and pitfall analysis. This phase research synthesizes those findings into actionable guidance specific to Phase 21's scope. The key risk is regression -- WindowContext currently has ~1200 lines of tightly coupled event handling. The extraction must preserve exact behavior while creating clean seams for Phase 23 (Tabs) and Phase 24 (Split Panes).

**Primary recommendation:** Create `glass_mux` crate with Session struct extracted from WindowContext fields, SessionMux in single-tab mode, and stub SplitTree/Tab types. Add SessionId to AppEvent. Add cfg-gated platform helpers. Verify zero regression on Windows.

<phase_requirements>
## Phase Requirements

Phase 21 has no explicit requirement IDs. Requirements are derived from the GOAL.md:

| ID | Description | Research Support |
|----|-------------|-----------------|
| P21-01 | New `glass_mux` crate with Session, SessionMux, SplitTree, Tab, ViewportLayout structs | Architecture patterns section -- struct layouts, crate boundaries |
| P21-02 | Session struct extracted from WindowContext fields | Code analysis of WindowContext (15 fields at lines 119-157) |
| P21-03 | SessionId added to all PTY-originated AppEvent variants | AppEvent enum analysis (5 variants at glass_core/src/event.rs:37-50) |
| P21-04 | SessionMux in single-tab/single-session mode wrapping existing behavior | Architecture patterns -- SessionMux API |
| P21-05 | WindowContext refactored to use SessionMux instead of inline terminal fields | main.rs analysis -- event routing must go through SessionMux |
| P21-06 | cfg-gated shell detection (zsh on macOS, $SHELL on Linux, pwsh on Windows) | Platform code patterns -- spawn_pty changes |
| P21-07 | Platform config/data paths via `dirs` crate | Config path analysis -- currently hardcoded to ~/.glass/ |
| P21-08 | Platform action modifier helper (Cmd on macOS, Ctrl+Shift elsewhere) | Keyboard mapping patterns -- winit ModifiersState |
| P21-09 | Shell integration scripts for zsh and bash on Linux/macOS | Shell integration analysis -- existing glass.bash, new glass.zsh needed |
| P21-10 | Zero regression -- Glass runs identically to v1.3 on Windows | Test gate from GOAL.md |
</phase_requirements>

## Standard Stack

### Core (No new dependencies)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| alacritty_terminal | =0.25.1 | PTY abstraction (already cross-platform) | Existing pinned dep, handles ConPTY/forkpty internally |
| wgpu | 28.0.0 | GPU rendering (already cross-platform via `Backends::all()`) | Existing dep, no changes needed |
| winit | 0.30.13 | Windowing (already cross-platform) | Existing dep, provides `ModifiersState::meta_key()` for macOS |
| dirs | 6 | Platform config/data directories | Existing dep, returns XDG on Linux, ~/Library on macOS |

### New Dependency

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| uuid | 1.x (features: v4) | SessionId generation | Unique session identification that survives DB persistence |

### New Crate (Internal)

| Crate | Purpose | Dependencies |
|-------|---------|-------------|
| `glass_mux` | Session extraction, SessionMux, Tab, SplitTree, ViewportLayout types | glass_core (AppEvent, GlassConfig), glass_terminal (BlockManager, StatusState, PtySender, etc.), glass_history, glass_snapshot |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| uuid v4 for SessionId | Incrementing u64 counter | u64 is simpler but doesn't survive process restart; uuid is safer for DB persistence. Use u64 if DB schema doesn't need cross-restart session continuity. |
| New glass_mux crate | Inline Session struct in main.rs | Defeats purpose -- glass_mux must be a separate crate so Phase 23/24 can depend on it without circular deps |

**Installation:**
```toml
# Workspace Cargo.toml
[workspace.dependencies]
uuid = { version = "1", features = ["v4"] }

# glass_mux/Cargo.toml (NEW)
[dependencies]
glass_core = { path = "../glass_core" }
glass_terminal = { path = "../glass_terminal" }
glass_history = { path = "../glass_history" }
glass_snapshot = { path = "../glass_snapshot" }
uuid = { workspace = true }
tracing = { workspace = true }
alacritty_terminal = { workspace = true }
winit = { workspace = true }

# Root binary Cargo.toml -- ADD:
glass_mux = { path = "crates/glass_mux" }
uuid = { workspace = true }
```

## Architecture Patterns

### New Crate Structure

```
crates/glass_mux/
  src/
    lib.rs          -- pub exports
    session.rs      -- Session struct (extracted from WindowContext)
    session_mux.rs  -- SessionMux: single-session wrapper (tabs/splits stubbed)
    split_tree.rs   -- SplitNode enum (stub for Phase 24)
    tab.rs          -- Tab struct (stub for Phase 23)
    layout.rs       -- ViewportLayout (stub for Phase 24)
    types.rs        -- SessionId, TabId, SplitDirection, FocusDirection
    platform.rs     -- cfg-gated helpers: shell detection, action modifier, config paths
```

### Pattern 1: Session Struct Extraction

**What:** Move 15 fields from WindowContext into a Session struct owned by SessionMux.

**Current WindowContext fields to extract (src/main.rs lines 125-157):**
```rust
// These fields move to Session:
pty_sender: PtySender,
term: Arc<FairMutex<Term<EventProxy>>>,
default_colors: DefaultColors,
block_manager: BlockManager,
status: StatusState,
history_db: Option<HistoryDb>,
last_command_id: Option<i64>,
command_started_wall: Option<std::time::SystemTime>,
search_overlay: Option<SearchOverlay>,
snapshot_store: Option<glass_snapshot::SnapshotStore>,
pending_command_text: Option<String>,
active_watcher: Option<glass_snapshot::FsWatcher>,
pending_snapshot_id: Option<i64>,
pending_parse_confidence: Option<glass_snapshot::Confidence>,
cursor_position: Option<(f64, f64)>,

// These fields STAY in WindowContext:
window: Arc<Window>,
renderer: GlassRenderer,
frame_renderer: FrameRenderer,
first_frame_logged: bool,
```

**Session struct:**
```rust
pub struct Session {
    pub id: SessionId,
    pub pty_sender: PtySender,
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    pub default_colors: DefaultColors,
    pub block_manager: BlockManager,
    pub status: StatusState,
    pub history_db: Option<HistoryDb>,
    pub last_command_id: Option<i64>,
    pub command_started_wall: Option<std::time::SystemTime>,
    pub search_overlay: Option<SearchOverlay>,
    pub snapshot_store: Option<glass_snapshot::SnapshotStore>,
    pub pending_command_text: Option<String>,
    pub active_watcher: Option<glass_snapshot::FsWatcher>,
    pub pending_snapshot_id: Option<i64>,
    pub pending_parse_confidence: Option<glass_snapshot::Confidence>,
    pub cursor_position: Option<(f64, f64)>,
    pub title: String,
}
```

**WindowContext becomes:**
```rust
struct WindowContext {
    window: Arc<Window>,
    renderer: GlassRenderer,
    frame_renderer: FrameRenderer,
    session_mux: SessionMux,
    first_frame_logged: bool,
}
```

### Pattern 2: SessionMux in Single-Session Mode

**What:** SessionMux wraps a single Session with the exact same API surface as direct field access.

```rust
pub struct SessionMux {
    tabs: Vec<Tab>,
    active_tab: usize,
    sessions: HashMap<SessionId, Session>,
    next_id: u64,
}

impl SessionMux {
    /// Create a SessionMux with one tab containing one session.
    pub fn new(session: Session) -> Self { ... }

    /// Get the currently focused session (always returns Some in single-session mode).
    pub fn focused_session(&self) -> Option<&Session> { ... }
    pub fn focused_session_mut(&mut self) -> Option<&mut Session> { ... }

    /// Route an event to a session by SessionId.
    pub fn session(&self, id: SessionId) -> Option<&Session> { ... }
    pub fn session_mut(&mut self, id: SessionId) -> Option<&mut Session> { ... }

    // Stub methods for Phase 23/24:
    // pub fn new_tab(...) -> TabId { ... }
    // pub fn close_tab(...) { ... }
    // pub fn split(...) -> SessionId { ... }
}
```

### Pattern 3: AppEvent SessionId Addition

**What:** Add `session_id: SessionId` to all PTY-originated AppEvent variants.

**Current AppEvent (glass_core/src/event.rs:37-50):**
```rust
pub enum AppEvent {
    TerminalDirty { window_id: WindowId },
    SetTitle { window_id: WindowId, title: String },
    TerminalExit { window_id: WindowId },
    Shell { window_id: WindowId, event: ShellEvent, line: usize },
    GitInfo { window_id: WindowId, info: Option<GitStatus> },
    CommandOutput { window_id: WindowId, raw_output: Vec<u8> },
}
```

**New AppEvent:**
```rust
pub enum AppEvent {
    TerminalDirty { window_id: WindowId },  // No SessionId -- any dirty triggers redraw
    SetTitle { window_id: WindowId, session_id: SessionId, title: String },
    TerminalExit { window_id: WindowId, session_id: SessionId },
    Shell { window_id: WindowId, session_id: SessionId, event: ShellEvent, line: usize },
    GitInfo { window_id: WindowId, session_id: SessionId, info: Option<GitStatus> },
    CommandOutput { window_id: WindowId, session_id: SessionId, raw_output: Vec<u8> },
}
```

**Note:** `TerminalDirty` does NOT need SessionId -- any PTY output triggers a window redraw regardless of which session produced it. This avoids unnecessary routing for the most frequent event.

### Pattern 4: cfg-gated Platform Code

**What:** Use `#[cfg(target_os = "...")]` for small platform differences. No trait abstraction.

```rust
// glass_mux/src/platform.rs

/// Returns the default shell program for the current platform.
pub fn default_shell() -> String {
    #[cfg(target_os = "windows")]
    {
        if std::process::Command::new("pwsh").arg("--version").output().is_ok() {
            "pwsh".to_owned()
        } else {
            "powershell".to_owned()
        }
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())
    }
    #[cfg(target_os = "linux")]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
    }
}

/// Returns true if the platform's "action" modifier is pressed.
/// Cmd on macOS, Ctrl on Windows/Linux.
pub fn is_action_modifier(mods: ModifiersState) -> bool {
    #[cfg(target_os = "macos")]
    { mods.meta_key() }
    #[cfg(not(target_os = "macos"))]
    { mods.control_key() }
}

/// Returns true if this is a Glass shortcut combo (action + shift).
/// Ctrl+Shift on Windows/Linux, Cmd on macOS (no Shift needed for most).
pub fn is_glass_shortcut(mods: ModifiersState) -> bool {
    #[cfg(target_os = "macos")]
    { mods.meta_key() }
    #[cfg(not(target_os = "macos"))]
    { mods.control_key() && mods.shift_key() }
}
```

### Pattern 5: EventProxy SessionId Propagation

**What:** EventProxy must carry SessionId so PTY reader threads include it in AppEvent.

**Current EventProxy (glass_terminal/src/event_proxy.rs):**
```rust
pub struct EventProxy {
    proxy: EventLoopProxy<AppEvent>,
    window_id: WindowId,
}
```

**New EventProxy:**
```rust
pub struct EventProxy {
    proxy: EventLoopProxy<AppEvent>,
    window_id: WindowId,
    session_id: SessionId,
}
```

All places that construct AppEvent variants in the PTY reader thread and OscScanner must include the session_id.

### Anti-Patterns to Avoid

- **Over-abstracting for Phase 21:** Do NOT build tab switching, split rendering, or viewport layout now. Phase 21 is extraction only. SessionMux should have stub methods that panic or return None for multi-tab operations.
- **Moving SearchOverlay to glass_mux:** SearchOverlay is defined in `src/search_overlay.rs` (root binary). It depends on glass_history types. For Phase 21, keep it as an `Option<Box<dyn Any>>` or move it to glass_mux with a glass_history dependency. Simplest: move SearchOverlay to glass_mux since glass_mux already depends on glass_history.
- **Global SessionMux:** SessionMux must be owned by WindowContext, NOT a global singleton. It is only accessed from the main thread (winit event loop).
- **Breaking find_shell_integration:** The existing `find_shell_integration()` function (main.rs:1193) is Windows-only (looks for glass.ps1). Phase 21 should make this platform-aware but keep the existing behavior on Windows.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Session IDs | Custom ID generator | uuid v4 | Globally unique, DB-safe, no collision on restart |
| Platform config paths | Custom path detection | `dirs::config_dir()`, `dirs::data_dir()` | Already a dependency, handles XDG/macOS/Windows correctly |
| Platform shell detection | Custom process probing on Unix | `std::env::var("SHELL")` | Standard Unix convention, set by login |
| Cross-platform PTY | Custom PTY wrapper trait | alacritty_terminal::tty | Already the PTY abstraction, handles ConPTY/forkpty |

## Common Pitfalls

### Pitfall 1: Borrow Checker Fights During Extraction

**What goes wrong:** WindowContext fields are accessed by `&mut self` in event handlers. When extracting to `ctx.session_mux.focused_session_mut()`, the borrow checker may reject patterns like `ctx.session_mux.focused_session_mut().history_db.as_ref()` combined with `ctx.window.request_redraw()` because both borrow `ctx`.
**Why it happens:** Rust's borrow checker cannot see through method calls to know that `session_mux` and `window` are disjoint fields.
**How to avoid:** Use temporary variables to break borrows. Extract session data into locals before using window/renderer methods. Consider `let session = &mut ctx.session_mux.focused_session_mut().unwrap();` then work with `session` exclusively.
**Warning signs:** Compile errors about "cannot borrow `ctx` as immutable because it is also borrowed as mutable."

### Pitfall 2: SessionId Not Available at PTY Spawn Time

**What goes wrong:** SessionId must be passed to `spawn_pty()` so the PTY reader thread can include it in AppEvent. But `spawn_pty()` is called during `resumed()` where the Session hasn't been fully constructed yet.
**Why it happens:** Chicken-and-egg: SessionMux creates the SessionId, but spawn_pty needs the SessionId to create the PTY, and the Session needs the PTY.
**How to avoid:** Generate SessionId first (from SessionMux's counter), pass it to spawn_pty, then construct Session with the returned PtySender and Term. The SessionMux.new() method should take a pre-spawned PTY, not spawn it internally.
**Warning signs:** SessionId is hardcoded or always 0.

### Pitfall 3: SearchOverlay Dependency Chain

**What goes wrong:** SearchOverlay is defined in `src/search_overlay.rs` and uses `glass_history::db::CommandRecord`. If Session owns `search_overlay: Option<SearchOverlay>`, then glass_mux needs glass_history as a dependency (which it should already have).
**Why it happens:** SearchOverlay was designed as a binary-only module, not a library type.
**How to avoid:** Move SearchOverlay into glass_mux (it logically belongs there as per-session state). glass_mux already depends on glass_history for HistoryDb, so CommandRecord is available.

### Pitfall 4: Platform cfg Compilation on Windows-Only Dev Machine

**What goes wrong:** cfg-gated code for macOS/Linux is written but never compiled on the developer's Windows machine. Syntax errors, missing imports, or wrong types go undetected.
**Why it happens:** `#[cfg(target_os = "macos")]` blocks are dead code on Windows.
**How to avoid:** Add `cargo check --target aarch64-apple-darwin` and `cargo check --target x86_64-unknown-linux-gnu` to CI or run locally via `rustup target add`. At minimum, test compilation for all three targets before merge.
**Warning signs:** CI failures on non-Windows platforms after merge.

### Pitfall 5: Event Handler Regression from Indirection

**What goes wrong:** The event handler in `user_event()` (main.rs:785-1191) accesses WindowContext fields ~50 times directly (e.g., `ctx.history_db`, `ctx.block_manager`, `ctx.pty_sender`). After extraction, every access becomes `ctx.session_mux.focused_session_mut().unwrap().field`. Missing even one causes a runtime panic (unwrap on None) or compile error.
**Why it happens:** 400+ lines of event handling code must be systematically updated.
**How to avoid:** Do the extraction mechanically: search-and-replace `ctx.field` with `ctx.session_mux.focused_session_mut().unwrap().field` (or better, a helper that returns &mut Session). Then fix borrow issues. Do NOT rewrite the event handling logic during extraction.

## Code Examples

### Session Construction (extracted from resumed())

```rust
// In SessionMux or a factory function:
pub fn create_session(
    id: SessionId,
    pty_sender: PtySender,
    term: Arc<FairMutex<Term<EventProxy>>>,
    history_db: Option<HistoryDb>,
    snapshot_store: Option<glass_snapshot::SnapshotStore>,
) -> Session {
    Session {
        id,
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
        title: String::new(),
    }
}
```

### Event Routing Through SessionMux

```rust
// In user_event() handler:
AppEvent::Shell { window_id, session_id, event: shell_event, line } => {
    if let Some(ctx) = self.windows.get_mut(&window_id) {
        // Route to correct session by SessionId
        if let Some(session) = ctx.session_mux.session_mut(session_id) {
            // ... existing shell event handling, using `session.field` instead of `ctx.field`
        }
    }
}
```

### Shell Integration for zsh (new file: shell-integration/glass.zsh)

```zsh
# Glass Shell Integration for Zsh
#
# Emits OSC 133 (command lifecycle) and OSC 7 (CWD) sequences.
# Compatible with Starship, Oh My Posh, and Powerlevel10k.

[[ -n "$__GLASS_INTEGRATION_LOADED" ]] && return
__GLASS_INTEGRATION_LOADED=1

__glass_osc7() {
    printf '\e]7;file://%s%s\e\\' "${HOST}" "${PWD}"
}

__glass_precmd() {
    local exit_code=$?
    printf '\e]133;D;%d\e\\' "$exit_code"
    __glass_osc7
    printf '\e]133;A\e\\'
}

__glass_preexec() {
    printf '\e]133;B\e\\'
    printf '\e]133;C\e\\'
}

autoload -Uz add-zsh-hook
add-zsh-hook precmd __glass_precmd
add-zsh-hook preexec __glass_preexec
```

### Platform-Aware find_shell_integration

```rust
fn find_shell_integration(shell_name: &str) -> Option<std::path::PathBuf> {
    let script_name = if shell_name.contains("pwsh") || shell_name.to_lowercase().contains("powershell") {
        "glass.ps1"
    } else if shell_name.contains("zsh") {
        "glass.zsh"
    } else if shell_name.contains("fish") {
        "glass.fish"
    } else {
        "glass.bash"
    };

    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;

    // Installed layout
    let candidate = exe_dir.join("shell-integration").join(script_name);
    if candidate.exists() {
        return Some(candidate);
    }

    // Development layout: exe in target/{debug,release}/
    if let Some(repo_root) = exe_dir.parent().and_then(|p| p.parent()) {
        let candidate = repo_root.join("shell-integration").join(script_name);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Single WindowContext with inline fields | SessionMux wrapping Session structs | Phase 21 (now) | Enables tabs and splits in Phase 23/24 |
| Hardcoded Windows shell detection | cfg-gated platform shell detection | Phase 21 (now) | Enables macOS/Linux PTY spawn |
| Hardcoded ~/.glass/ paths | dirs crate platform paths | Phase 21 (now) | XDG compliance on Linux, ~/Library on macOS |
| Ctrl+Shift only shortcuts | Platform action modifier (Cmd on macOS) | Phase 21 (now) | macOS keyboard convention support |

## Open Questions

1. **SearchOverlay location**
   - What we know: SearchOverlay is in `src/search_overlay.rs`, depends on glass_history types
   - What's unclear: Whether it should move to glass_mux or stay in the binary with Session holding `Option<Box<dyn Any>>`
   - Recommendation: Move to glass_mux. It is per-session state and glass_mux already depends on glass_history. Cleanest approach.

2. **SessionId type: uuid vs u64**
   - What we know: uuid is more robust for DB persistence; u64 is simpler
   - What's unclear: Whether sessions will ever be persisted across restarts
   - Recommendation: Use u64 for now (simpler, no new dep). If DB persistence needs arise in Phase 23, switch to uuid then. The ARCHITECTURE.md research suggests uuid but there is no current DB schema change planned for Phase 21.

3. **Shell integration injection location**
   - What we know: Currently in `resumed()` (main.rs:283-295), hardcoded to PowerShell
   - What's unclear: Whether injection should happen in SessionMux (on session create) or stay in main.rs
   - Recommendation: Keep injection in main.rs for Phase 21. Session creation should be a pure data operation. Shell integration injection requires the PtySender and platform knowledge.

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust built-in test (`#[cfg(test)]` + `cargo test`) |
| Config file | None (uses Cargo.toml test config) |
| Quick run command | `cargo test -p glass_mux` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| P21-01 | glass_mux crate compiles with correct exports | unit | `cargo check -p glass_mux` | Wave 0 |
| P21-02 | Session struct holds all extracted fields | unit | `cargo test -p glass_mux -- session` | Wave 0 |
| P21-03 | AppEvent variants include SessionId | unit | `cargo test -p glass_core -- app_event` | Partial (existing tests need update) |
| P21-04 | SessionMux single-session focused_session returns Some | unit | `cargo test -p glass_mux -- session_mux` | Wave 0 |
| P21-05 | WindowContext compiles with SessionMux field | compilation | `cargo check -p glass` | Existing |
| P21-06 | default_shell returns correct value per platform | unit | `cargo test -p glass_mux -- platform` | Wave 0 |
| P21-07 | Platform config paths use dirs crate | unit | `cargo test -p glass_mux -- platform::config_path` | Wave 0 |
| P21-08 | is_action_modifier returns correct value per platform | unit | `cargo test -p glass_mux -- platform::modifier` | Wave 0 |
| P21-09 | Shell integration scripts exist for zsh and bash | smoke | `test -f shell-integration/glass.zsh` | Wave 0 |
| P21-10 | Full workspace compiles and existing tests pass | integration | `cargo test --workspace` | Existing |

### Sampling Rate

- **Per task commit:** `cargo test -p glass_mux && cargo check -p glass`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full workspace test suite green + manual verification that Glass launches and runs identically to v1.3 on Windows

### Wave 0 Gaps

- [ ] `crates/glass_mux/src/lib.rs` -- new crate, no files exist yet
- [ ] `crates/glass_mux/Cargo.toml` -- new crate manifest
- [ ] `shell-integration/glass.zsh` -- zsh shell integration script
- [ ] Update `crates/glass_core/src/event.rs` tests for SessionId in AppEvent variants

## Sources

### Primary (HIGH confidence)

- Glass source code analysis: `src/main.rs` WindowContext struct (lines 119-157), event handling (lines 385-1191), find_shell_integration (lines 1193-1212)
- Glass source code analysis: `crates/glass_core/src/event.rs` AppEvent enum (lines 37-50)
- Glass source code analysis: `crates/glass_terminal/src/pty.rs` spawn_pty (lines 106-121)
- Glass source code analysis: `crates/glass_terminal/src/event_proxy.rs` EventProxy (lines 13-22)
- `.planning/research/ARCHITECTURE.md` -- v2.0 target architecture, component boundaries, data flows
- `.planning/research/STACK.md` -- dependency analysis, platform compatibility matrix
- `.planning/research/PITFALLS.md` -- 14 catalogued pitfalls with prevention strategies

### Secondary (MEDIUM confidence)

- WezTerm Mux architecture (deepwiki.com) -- validated binary tree split pattern
- winit 0.30.13 ModifiersState docs -- `meta_key()` for macOS Cmd key

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies except possibly uuid; all existing deps are cross-platform
- Architecture: HIGH -- extraction pattern is mechanical refactoring of known code
- Pitfalls: HIGH -- borrow checker fights and event routing regression are well-understood Rust patterns
- Platform code: MEDIUM -- cfg-gated code cannot be tested on Windows dev machine without cross-compilation

**Research date:** 2026-03-06
**Valid until:** 2026-04-06 (stable domain, no rapidly changing dependencies)

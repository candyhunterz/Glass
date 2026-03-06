# Stack Research: Cross-Platform & Tabs

**Project:** Glass v2.0 -- Cross-Platform (macOS/Linux) & Tabs/Split Panes
**Researched:** 2026-03-06
**Confidence:** HIGH (primary deps already cross-platform; changes are cfg-gated code, not new heavy crates)

## Scope

This document covers ONLY what is needed for v2.0: macOS/Linux PTY support, wgpu backend auto-selection, platform keyboard mapping, and tab/split pane session management. The existing validated stack (11 crates, 28,885 LOC) is unchanged.

---

## Key Finding: alacritty_terminal Already Handles Cross-Platform PTY

The most important discovery: **Glass does NOT need portable-pty or any new PTY crate.** The existing `alacritty_terminal 0.25.1` already provides cross-platform PTY support through its `tty` module:

- **Windows:** ConPTY via `windows-sys` + `miow` (current implementation)
- **macOS/Linux:** Unix PTY via `rustix-openpty` + `signal-hook` (same `tty::new()` API)

Glass's `pty.rs` calls `alacritty_terminal::tty::new()` which compiles to the correct platform implementation automatically. The `Pty`, `EventedPty`, `EventedReadWrite`, and `ChildEvent` types are all cross-platform abstractions within alacritty_terminal.

**What needs to change in pty.rs:** Only the shell detection logic. Currently hardcoded to detect `pwsh`/`powershell`. On Unix, detect `$SHELL` or fall back to `/bin/bash`.

---

## Existing Stack: Already Cross-Platform (No Changes Needed)

| Technology | Version | macOS | Linux | Notes |
|------------|---------|-------|-------|-------|
| alacritty_terminal | =0.25.1 | YES (rustix-openpty) | YES (rustix-openpty) | PTY abstraction is built-in |
| wgpu | 28.0.0 | YES (Metal) | YES (Vulkan/GL) | Backend auto-selection via `Backends::all()` |
| winit | 0.30.13 | YES (Cocoa) | YES (X11 + Wayland) | Both X11 and Wayland enabled by default |
| tokio | 1.50.0 | YES | YES | Fully cross-platform async runtime |
| rusqlite | 0.38.0 (bundled) | YES | YES | Bundled SQLite compiles everywhere |
| notify | 8.2 | YES (FSEvents) | YES (inotify) | Platform backends selected at compile time |
| blake3 | 1.8.3 | YES | YES | Pure Rust + optional SIMD |
| glyphon | 0.10.0 | YES | YES | wgpu-based text rendering, platform-agnostic |
| arboard | 3 | YES (NSPasteboard) | YES (X11 sel/Wayland) | Cross-platform clipboard |
| dirs | 6 | YES | YES | XDG on Linux, ~/Library on macOS |
| ignore | 0.4 | YES | YES | .gitignore parsing is platform-agnostic |

**Observation:** Every existing dependency is already cross-platform. The v1.0-v1.3 stack was well-chosen.

---

## Changes Required (Not New Dependencies)

### 1. wgpu Backend Selection

**Current code** (`surface.rs` line 22-28):
```rust
let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
    #[cfg(target_os = "windows")]
    backends: wgpu::Backends::DX12,
    #[cfg(not(target_os = "windows"))]
    backends: wgpu::Backends::all(),
    ..Default::default()
});
```

**This is already correct.** The `cfg` gate already exists. On macOS, `Backends::all()` will prefer Metal (the only available backend). On Linux, it will prefer Vulkan, falling back to GL if Vulkan drivers are unavailable. No code changes needed for backend selection.

**wgpu Cargo features to verify:** The default features of wgpu 28.0 include `metal`, `vulkan`, `dx12`, and `gles` -- all enabled by default. No feature flag changes needed in Cargo.toml.

**Confidence:** HIGH -- verified via wgpu docs and crates.io feature list.

### 2. Platform Keyboard Mapping (Cmd vs Ctrl)

**Current code** (`input.rs`) uses `modifiers.control_key()` for shortcuts like Ctrl+Shift+C/V.

**winit 0.30.13 `ModifiersState`** provides:
- `CONTROL` -- Ctrl key on all platforms
- `META` -- Windows key on PC, **Command key on macOS**
- `ALT` -- Alt key on PC, Option key on macOS
- `SHIFT` -- Shift on all platforms
- `SUPER` -- **Deprecated** in favor of `META`

Methods: `control_key()`, `meta_key()`, `alt_key()`, `shift_key()`

**Approach:** Create a `platform_action_modifier()` helper:
```rust
/// Returns true if the platform's "action" modifier is pressed.
/// Cmd on macOS, Ctrl on Windows/Linux.
fn action_modifier(mods: ModifiersState) -> bool {
    #[cfg(target_os = "macos")]
    { mods.meta_key() }
    #[cfg(not(target_os = "macos"))]
    { mods.control_key() }
}
```

This affects: copy/paste (Cmd+C/V on macOS vs Ctrl+Shift+C/V), tab shortcuts (Cmd+T/W), split pane shortcuts. The Ctrl+letter -> ASCII control character encoding remains unchanged (it is terminal protocol, not platform convention).

**Confidence:** HIGH -- verified `meta_key()` method exists in winit 0.30.13 ModifiersState via official docs.

### 3. Shell Detection on Unix

**Current code** (`pty.rs` line 115-121): Detects `pwsh` then falls back to `powershell`.

**Unix approach:**
```rust
#[cfg(unix)]
fn detect_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
}

#[cfg(windows)]
fn detect_shell() -> String {
    if Command::new("pwsh").arg("--version").output().is_ok() {
        "pwsh".to_owned()
    } else {
        "powershell".to_owned()
    }
}
```

No new dependencies. Standard `$SHELL` environment variable is universal on Unix.

### 4. Shell Integration Scripts

**Existing:** `glass.bash` and `glass.ps1`

**Needed for v2.0:**
- `glass.zsh` -- zsh shell integration (macOS default shell)
- `glass.fish` -- fish shell integration (popular on Linux)
- Extend `glass.bash` to work on Linux (already POSIX-compatible, mostly works)

**zsh differences from bash:** `precmd`/`preexec` hooks instead of `PROMPT_COMMAND`/`PS0`. The OSC 133 sequences are identical. This is shell script work, not Rust crate work.

**fish differences:** `fish_prompt`/`fish_preexec` functions. Fish syntax differs significantly from bash/zsh but the OSC protocol is identical.

### 5. windows-sys Dependency Gating

**Current:** `windows-sys` is a workspace dependency used for UTF-8 console code page.

**Change:** Gate it behind `cfg(windows)`:
```toml
[target.'cfg(windows)'.dependencies]
windows-sys = { workspace = true }
```

Already partially done in some crates. Needs consistent application across the workspace.

---

## New Code Required: Tab/Split Pane Session Management

### No External Crate -- Build It

After researching wezterm's mux architecture and evaluating available crates, the recommendation is to **build the tab/split system from scratch** (~500-800 LOC). Rationale:

1. **No suitable crate exists.** Ratatui is for TUI apps (renders to terminal, not GPU). wezterm's `mux` crate is deeply coupled to wezterm internals and not published as a standalone library.

2. **The data structures are simple.** Tabs are a `Vec<Tab>` with an active index. Split panes are a binary tree where leaves are terminal sessions and internal nodes are split direction + ratio.

3. **Glass already has the hard parts.** PTY spawning, terminal emulation, and GPU rendering are solved. Tab/split management is lightweight orchestration on top.

### Architecture (inspired by wezterm's mux)

```
SessionManager
  |-- tabs: Vec<Tab>           // ordered list
  |-- active_tab: usize        // index into tabs
  |
  Tab
  |-- tree: SplitTree          // binary tree of panes
  |-- active_pane: PaneId      // which pane has focus
  |
  SplitTree (enum)
  |-- Leaf(Pane)               // terminal session
  |-- Split { direction: H|V, ratio: f32, first: Box<SplitTree>, second: Box<SplitTree> }
  |
  Pane
  |-- id: PaneId
  |-- pty_sender: PtySender
  |-- term: Arc<FairMutex<Term<EventProxy>>>
  |-- block_manager: BlockManager
  |-- history_db: HistoryDb     // independent per-session
  |-- snapshot_store: SnapshotStore  // independent per-session
```

**Key design decisions:**

| Decision | Choice | Why |
|----------|--------|-----|
| Tab data structure | `Vec<Tab>` with active index | Simple, O(1) switch, matches UI tab bar order |
| Split data structure | Binary tree enum | Natural recursive splits. wezterm validated this approach at scale. |
| PTY per pane | Yes, independent | Each pane needs its own shell process, terminal grid, and I/O thread |
| History per pane | Shared DB, session-scoped queries | One SQLite database with a `session_id` column, not one DB per pane. Avoids file proliferation. |
| Snapshot store per pane | Shared blob store, session-scoped metadata | Same CAS store (BLAKE3 dedup works across sessions), metadata table gains `session_id` |
| Layout persistence | Defer to future milestone | Saving/restoring tab layouts is config hot-reload territory |

### Session ID Integration

Add `session_id: Uuid` to each Pane. Extend `commands` and `snapshots` tables with `session_id` column (nullable for backward compat with v1.x data). This allows:
- Per-pane history queries in search overlay
- Per-pane undo (don't undo commands from a different pane)
- MCP context scoped to active pane

**uuid crate:** Use `uuid = { version = "1", features = ["v4"] }` for random session IDs. Lightweight, widely used, no controversy.

---

## New Dependencies for v2.0

### Required

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| uuid | 1.x | Session IDs for tabs/panes | v4 random UUIDs for unique pane identification. Required to scope history/snapshots to individual sessions. 800M+ downloads, Rust ecosystem standard. |

### That's It

One new crate. Everything else is code changes to existing crates.

**Confidence:** HIGH -- uuid is trivial, battle-tested, and the only genuinely new dependency.

---

## What NOT to Add

| Temptation | Why Not |
|------------|---------|
| **portable-pty** | alacritty_terminal 0.25.1 already provides cross-platform PTY via `tty::new()`. Adding portable-pty would create a redundant abstraction layer, add wezterm coupling, and require rewriting the entire PTY loop. |
| **nix crate** | Provides Unix syscall wrappers. alacritty_terminal already handles forkpty internally via rustix-openpty. Direct nix usage is unnecessary. |
| **libc (direct)** | Already a transitive dependency of alacritty_terminal. No need to add as direct dependency for PTY work. |
| **mio** | polling crate (already used by alacritty_terminal) handles the event loop. mio would be redundant. |
| **crossterm** | For TUI apps writing to a terminal. Glass IS a terminal -- it renders via wgpu, not terminal escape sequences. |
| **signal-hook (direct)** | Already a dependency of alacritty_terminal on Unix. Child process signal handling is internal to the tty module. |
| **tauri/egui for tab bar** | The tab bar is a simple GPU-rendered strip. Glass already renders text with glyphon and quads with wgpu. No UI framework needed. |
| **slab / slotmap** | For pane ID allocation. A simple `u64` counter or `Uuid` is sufficient for the expected number of panes (< 100). |

---

## Alternatives Considered

| Category | Recommended | Alternative | Why Not |
|----------|-------------|-------------|---------|
| PTY abstraction | alacritty_terminal::tty (existing) | portable-pty 0.9 | Existing dep already cross-platform. portable-pty adds 15+ transitive deps and requires rewriting PTY loop. |
| PTY abstraction | alacritty_terminal::tty | pseudoterminal crate | Newer, less proven (< 10K downloads). alacritty_terminal has years of battle-testing. |
| wgpu backends | Default features (all backends) | Feature-gate per platform | Unnecessary complexity. wgpu auto-selects the best backend. Binary size cost of unused backends is negligible (they're compile-time selected). |
| macOS Cmd key | `ModifiersState::meta_key()` | Raw platform keycode matching | meta_key() is the winit-sanctioned approach. Raw keycodes break on keyboard layout changes. |
| Tab management | Custom Vec<Tab> + binary tree | ratatui Layout | ratatui renders to terminal (text mode). Glass renders via wgpu (GPU). Completely different rendering model. |
| Tab management | Custom code | Extract wezterm mux crate | wezterm's mux is tightly coupled to its own Terminal type, Domain system, and Lua config. Extraction effort exceeds writing from scratch. |
| Session IDs | uuid v4 | Incrementing u64 | UUIDs survive across process restarts without coordination. u64 counters reset on restart, risking ID collision in the database. |
| Split layout | Binary tree enum | Grid/tile system | Binary tree naturally models recursive H/V splits. Grid systems are more complex and not needed for terminal splits. |

---

## Platform-Specific Compilation Matrix

| Crate | Windows | macOS | Linux | Notes |
|-------|---------|-------|-------|-------|
| windows-sys | YES | no | no | Gate with `cfg(windows)` |
| Signal handling | ConPTY events | signal-hook (via alacritty_terminal) | signal-hook (via alacritty_terminal) | Transparent via tty module |
| FS watching | ReadDirectoryChangesW | FSEvents | inotify | notify 8.2 handles all three |
| Clipboard | WinAPI | NSPasteboard | X11 selections / Wayland | arboard 3 handles all |
| Config dir | %APPDATA% | ~/Library/Application Support | $XDG_CONFIG_HOME | dirs 6 handles all |
| Data dir | %LOCALAPPDATA% | ~/Library/Application Support | $XDG_DATA_HOME | dirs 6 handles all |

---

## Cargo.toml Changes

### Workspace Root

```toml
[workspace.dependencies]
# ADD:
uuid = { version = "1", features = ["v4"] }

# CHANGE windows-sys to target-specific in crates that use it:
# (Move from [dependencies] to [target.'cfg(windows)'.dependencies] in each crate)
```

### Per-Crate Changes

**glass_terminal/Cargo.toml:**
```toml
# No changes -- alacritty_terminal handles platform PTY internally
```

**glass_renderer/Cargo.toml:**
```toml
# No changes -- wgpu backend selection is already cfg-gated in code
```

**Root binary Cargo.toml:**
```toml
[dependencies]
uuid = { workspace = true }  # Session management

[target.'cfg(windows)'.dependencies]
windows-sys = { workspace = true }  # Move from unconditional
```

---

## Version Compatibility

| Package | Compatible With | Notes |
|---------|-----------------|-------|
| alacritty_terminal =0.25.1 | macOS (aarch64, x86_64), Linux (aarch64, x86_64) | Verified: builds listed on docs.rs for all targets |
| wgpu 28.0.0 | Metal (macOS), Vulkan (Linux), GL (Linux fallback) | All backend features enabled by default |
| winit 0.30.13 | Cocoa (macOS), X11 + Wayland (Linux) | Both Linux backends enabled by default |
| uuid 1.x | All platforms | Pure Rust, no platform dependencies |
| notify 8.2 | FSEvents (macOS), inotify (Linux) | Platform backend selected at compile time |

---

## Compile & CI Impact

| Change | Compile Impact | Binary Size | Notes |
|--------|---------------|-------------|-------|
| uuid crate | MINIMAL (~2s) | ~20 KB | Small, pure Rust |
| Tab/split code (~800 LOC) | MODERATE | ~40 KB | New module in glass_core or new glass_session crate |
| Shell integration scripts | NONE | N/A | Not compiled, shipped alongside binary |
| Platform cfg gates | MINIMAL | Slight reduction per-platform | Dead code elimination removes unused platform paths |
| **Total v2.0 addition** | **~60 KB code** | Minimal new deps. Primary work is cfg-gated code paths. |

---

## Sources

- [alacritty_terminal::tty docs (docs.rs)](https://docs.rs/alacritty_terminal/0.25.1/alacritty_terminal/tty/index.html) -- Cross-platform PTY module, verified macOS/Linux/Windows build targets (HIGH confidence)
- [alacritty_terminal Cargo.toml (GitHub)](https://github.com/alacritty/alacritty/blob/master/alacritty_terminal/Cargo.toml) -- Platform-specific deps: rustix-openpty for Unix, windows-sys for Windows (HIGH confidence)
- [wgpu cross-platform backends (blog.brightcoding.dev)](https://www.blog.brightcoding.dev/2025/09/30/cross-platform-rust-graphics-with-wgpu-one-api-to-rule-vulkan-metal-d3d12-opengl-webgpu/) -- Backend auto-selection documentation (MEDIUM confidence)
- [winit ModifiersState (rust-windowing.github.io)](https://rust-windowing.github.io/winit/winit/keyboard/struct.ModifiersState.html) -- META constant = Command on macOS, `meta_key()` method verified (HIGH confidence)
- [wezterm Multiplexer Architecture (deepwiki.com)](https://deepwiki.com/wezterm/wezterm/2.2-multiplexer-architecture) -- Tab/split binary tree pattern, PTY-per-pane model (HIGH confidence)
- [portable-pty (crates.io)](https://crates.io/crates/portable-pty) -- Evaluated and rejected; alacritty_terminal already provides equivalent (HIGH confidence)
- [notify crate (GitHub)](https://github.com/notify-rs/notify) -- FSEvents on macOS, inotify on Linux confirmed (HIGH confidence)

---
*Stack research for: Glass v2.0 Cross-Platform & Tabs*
*Researched: 2026-03-06*

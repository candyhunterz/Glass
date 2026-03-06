# Domain Pitfalls

**Domain:** Cross-platform terminal emulator (adding macOS/Linux + tabs/split panes to existing Windows-only Rust terminal)
**Researched:** 2026-03-06
**Confidence:** HIGH (verified against alacritty, wezterm, wgpu sources, and Glass codebase analysis)

## Critical Pitfalls

### Pitfall 1: PTY Abstraction Hides Fundamental Semantic Differences Between ConPTY and forkpty

**What goes wrong:**
Glass's `pty.rs` directly uses `alacritty_terminal::tty` which provides `tty::new()`, `tty::Pty`, and `EventedPty` -- these already abstract ConPTY on Windows vs forkpty on Unix. But the abstraction hides critical behavioral differences:
- **Signal propagation:** Unix PTYs use signal groups (`SIGWINCH` for resize, `SIGHUP` on disconnect, `SIGCHLD` on child exit). ConPTY uses none of these -- resize is an API call (`ResizePseudoConsole`), and child exit comes via `WaitForSingleObject`. Glass's current code handles `tty::ChildEvent::Exited` which maps differently per platform.
- **EOF semantics:** On Unix, the PTY master gets EOF when the child exits AND all file descriptors are closed. On Windows ConPTY, the pipe-based I/O can return EOF before the process has fully terminated, or vice versa.
- **Process group lifetime:** Unix forkpty creates a new session with `setsid()`. The child is the session leader. If Glass crashes, orphaned children get `SIGHUP`. On Windows, ConPTY children are not in a job object by default -- if Glass crashes, child processes become orphans with no cleanup signal.

**Why it happens:**
ConPTY and forkpty solve the same problem (give a process a fake terminal) but with completely different OS primitives. ConPTY wraps Windows Console API with pipes. forkpty wraps Unix pseudo-terminal devices with file descriptors. The `alacritty_terminal::tty` trait makes them look similar, but edge cases (process lifecycle, signal handling, error conditions) diverge.

**Consequences:**
- Zombie processes on macOS/Linux when Glass exits uncleanly (no `SIGHUP` sent, or sent but not handled correctly)
- PTY reader thread hangs on Unix waiting for EOF that never comes because a background child process holds the PTY fd open
- Different behavior between `Ctrl+C` (sends `SIGINT` to foreground process group on Unix, `CTRL_C_EVENT` via `GenerateConsoleCtrlEvent` on Windows)

**Prevention:**
1. Keep using `alacritty_terminal::tty` -- it handles the low-level differences. Do NOT write a custom PTY layer.
2. Add explicit signal handling on Unix: install `SIGCHLD` handler for reaping, and send `SIGHUP` to child process group on Glass exit.
3. On Windows, consider using Job Objects to ensure child cleanup on parent crash.
4. Test the PTY reader thread shutdown path on each platform separately. The drain-on-exit logic (line 148 `drain_on_exit: true`) may behave differently.
5. Add a watchdog: if child process exits but PTY reader thread hasn't received EOF within 5 seconds, force-close.

**Detection:**
- `ps aux | grep defunct` shows zombie processes after closing Glass on Linux/macOS
- PTY reader thread still running after shell exits (check thread list)
- `Ctrl+C` kills Glass instead of sending to child process

**Phase to address:**
Phase 1 (Platform PTY abstraction) -- must be validated with per-platform integration tests before any tabs/splits work begins.

---

### Pitfall 2: wgpu Backend-Specific Shader and Surface Behavior Differences

**What goes wrong:**
Glass currently forces `wgpu::Backends::DX12` on Windows (surface.rs line 24). The existing `#[cfg(not(target_os = "windows"))]` falls back to `Backends::all()`, which means Metal on macOS and Vulkan on Linux. But WGSL shaders that work on DX12 may exhibit subtle differences on Metal or Vulkan due to Naga's shader translation:
- **Texture format differences:** `caps.formats[0]` (surface.rs line 53) returns different preferred formats per backend. DX12 typically gives `Bgra8UnormSrgb`, Metal may give `Bgra8Unorm` (no sRGB), Vulkan may give `Bgra8Srgb`. If Glass's glyph rendering or rect rendering assumes a specific format, colors shift.
- **Surface lost/outdated frequency:** macOS aggressively invalidates surfaces on window resize/occlude. Linux Wayland invalidates on compositor changes. Glass handles `SurfaceError::Lost` and `Outdated` already (surface.rs lines 80-84), but on macOS these fire much more frequently than DX12 on Windows.
- **Metal shader compilation timeout:** Known wgpu issue (#4456) where certain WGSL patterns cause Metal shader compilation to hang. Glass's shaders are simple (instanced quads), so this is low risk but not zero.
- **Integer minimum value:** `i32::MIN` (-2147483648) causes Metal shader compilation errors (wgpu issue #4399). If Glass ever uses this in shader constants, it will work on DX12 but crash on Metal.

**Why it happens:**
wgpu abstracts graphics APIs through Naga shader translation. Naga translates WGSL to MSL (Metal Shading Language) for macOS, SPIR-V for Vulkan, and HLSL for DX12. Each translation has edge cases. Additionally, different backends have different surface lifecycle semantics.

**Consequences:**
- Application renders correctly on Windows but shows wrong colors, blank screen, or crashes on macOS/Linux
- Intermittent blank frames on macOS during window resize (surface invalidation race)
- "No compatible GPU adapter found" panic on Linux VMs without GPU passthrough

**Prevention:**
1. Test on all three platforms early -- do not develop the full renderer on Windows and port later.
2. Explicitly handle texture format negotiation: query `caps.formats` and select a format that works for your shaders, rather than blindly taking `[0]`.
3. On Linux, add fallback to GL backend (`wgpu::Backends::GL`) for environments without Vulkan (VMs, older GPUs, Wayland compositors with no Vulkan support).
4. Add a `GLASS_GPU_BACKEND` environment variable override for debugging.
5. Test surface lost/outdated handling on macOS by rapidly resizing the window and minimizing/restoring.

**Detection:**
- Colors look different between platforms (sRGB vs linear gamma)
- Blank frames during resize on macOS
- Crash on startup on Linux VM

**Phase to address:**
Phase 2 (Renderer abstraction) -- validate surface/format handling on each platform before building tab/split viewport splitting.

---

### Pitfall 3: macOS Keyboard Modifier Mapping -- Cmd vs Ctrl Confusion

**What goes wrong:**
Glass's current keyboard shortcuts use `Ctrl+Shift+C` (copy), `Ctrl+Shift+V` (paste), `Ctrl+Shift+F` (search), `Ctrl+Shift+Z` (undo). On macOS, users expect `Cmd+C` for copy, `Cmd+V` for paste, `Cmd+Z` for undo. But `Cmd+C` in a terminal must NOT send `SIGINT` (which is what `Ctrl+C` does). The mapping is:
- **macOS convention:** `Cmd` = application shortcuts, `Ctrl` = terminal control characters
- **Linux convention:** `Ctrl+Shift` = terminal app shortcuts (to avoid conflict with `Ctrl` terminal sequences)
- **Windows convention (Glass current):** `Ctrl+Shift` = terminal app shortcuts

If Glass naively maps `Cmd` to `Ctrl+Shift` on macOS, users lose the ability to use `Cmd+C` for copy (it would be `Ctrl+Shift+C` which is not intuitive on Mac). If Glass maps `Cmd+C` to copy but doesn't handle `Cmd+V` vs `Ctrl+V` consistently, muscle memory breaks.

**Why it happens:**
macOS has a dedicated `Cmd` modifier separate from `Ctrl`. Linux and Windows share `Ctrl` for both OS-level and terminal-level functions, requiring `Shift` as a disambiguator. winit exposes `ModifiersState::SUPER` for the Cmd/Win key, but Glass's `input.rs` likely matches on `ModifiersState::CONTROL | ModifiersState::SHIFT`.

**Consequences:**
- macOS users cannot use standard `Cmd+C`/`Cmd+V` for copy/paste
- `Cmd+Q` doesn't quit (macOS convention) -- users force-kill instead
- Tab shortcuts (`Cmd+T` new tab, `Cmd+W` close tab) conflict with terminal sequences if mapped wrong
- `Cmd+N` (new window) convention missed

**Prevention:**
1. Define a `Shortcut` abstraction that maps logical actions to platform-specific key combinations:
   - Copy: `Ctrl+Shift+C` (Win/Linux) / `Cmd+C` (macOS)
   - Paste: `Ctrl+Shift+V` (Win/Linux) / `Cmd+V` (macOS)
   - New Tab: `Ctrl+Shift+T` (Win/Linux) / `Cmd+T` (macOS)
   - Close Tab: `Ctrl+Shift+W` (Win/Linux) / `Cmd+W` (macOS)
2. On macOS, intercept `Cmd` modifier in `input.rs` before the terminal key encoder. `Cmd+<key>` should NEVER be sent to the PTY as a terminal escape sequence.
3. Handle `Cmd+Q` for graceful shutdown on macOS.
4. Make shortcuts configurable in `config.toml` for users who remap their keyboards.

**Detection:**
- macOS users report "copy/paste doesn't work"
- `Cmd+C` sends `^C` to the shell instead of copying
- `Cmd+W` sends escape sequence instead of closing tab

**Phase to address:**
Phase 1 (Platform abstraction layer) -- keyboard handling must be platform-aware before tabs/splits add more shortcuts.

---

### Pitfall 4: Tab/Split Session Lifecycle Creates Zombie PTY Processes and Resource Leaks

**What goes wrong:**
Moving from single-session to multi-session (tabs + splits) multiplies every resource by N:
- Each tab/split needs its own PTY process, reader thread, `Term` instance, `BlockManager`, `OutputBuffer`, history DB connection, and snapshot context
- Closing a tab must shut down ALL of these cleanly. If the PTY reader thread doesn't exit (because the child shell hasn't responded to close), it leaks a thread + PTY fd + child process.
- Splitting a pane and then unsplitting must deallocate the renderer viewport correctly. wgpu resources (textures, buffers) tied to a split viewport that isn't properly destroyed cause GPU memory leaks.
- Tab switching must pause rendering for inactive tabs but keep PTY reader threads running (commands continue executing in background tabs).

**Why it happens:**
Single-session cleanup is trivial (process exit = everything dies). Multi-session requires explicit lifecycle management for each session independently. The PTY reader thread in Glass uses blocking I/O in a `std::thread` -- you cannot just "cancel" it; you must signal it via `PtyMsg::Shutdown` and wait for the thread to exit. If the child process is stuck (e.g., `sleep infinity`), the shutdown sequence stalls.

**Consequences:**
- Each closed tab leaves a zombie shell process and orphaned thread
- After opening/closing many tabs, Glass consumes GBs of memory
- GPU memory grows monotonically (never freed for closed splits)
- On macOS/Linux, file descriptor exhaustion after ~1000 tab open/close cycles (each PTY uses 2-3 fds)

**Prevention:**
1. Implement a `Session` struct that owns all resources for one tab/split: `PtySender`, `JoinHandle<()>` for the reader thread, `Arc<FairMutex<Term>>`, `BlockManager`, DB connections. `Drop` implementation must clean up in order: send `PtyMsg::Shutdown`, kill child process if still alive after timeout, join reader thread with timeout.
2. Add a `SessionManager` that tracks all active sessions, enforces max session count, and handles graceful shutdown-all on app exit.
3. For splits: share the wgpu device/queue (they're thread-safe) but create separate render targets per pane. On split close, explicitly destroy the render target.
4. Set hard timeout on PTY reader thread join (2 seconds). If it doesn't exit, `kill -9` the child and `detach` the thread.
5. Track resource counts (threads, fds, GPU allocations) in debug mode and log warnings if they grow monotonically.

**Detection:**
- Task Manager/Activity Monitor shows increasing process count as tabs are opened and closed
- Memory usage grows over time even with few active tabs
- "Too many open files" error after extended use on macOS/Linux
- GPU driver warnings about resource exhaustion

**Phase to address:**
Phase 3 (Tabs/splits implementation) -- but the `Session` abstraction must be designed in Phase 1 as part of the architecture.

---

### Pitfall 5: Shell Integration Scripts Break on macOS/Linux Shell Differences

**What goes wrong:**
Glass currently ships shell integration for PowerShell (`glass.ps1`) and Bash (`glass.bash`). Cross-platform adds new requirements:
- **macOS default shell is zsh** (since Catalina), not bash. Glass needs `glass.zsh`.
- **zsh has different hook mechanisms:** `precmd` / `preexec` functions (not `PROMPT_COMMAND` and `DEBUG` trap like bash). The `preexec` function receives the command string as `$1`, while bash's `DEBUG` trap uses `$BASH_COMMAND`.
- **Fish shell** is popular on macOS/Linux. It uses `fish_prompt` / `fish_preexec` / `fish_postexec` event functions, completely unlike bash/zsh.
- **Bash version differences:** macOS ships bash 3.2 (GPLv2, from 2007). Bash 4+ features (`|&`, associative arrays, `PIPESTATUS` behavior changes) are not available on stock macOS. Glass's pipe capture uses `PIPESTATUS` which exists in bash 3.2 but `pipefail` behavior differs subtly.
- **OSC sequence emission in zsh:** zsh's `$COLUMNS` and `$LINES` are set differently than bash, affecting the shell integration's terminal size awareness.
- **Starship/Oh My Posh interaction:** Glass's current shell integration is "Oh My Posh/Starship compatible." Each shell has a different integration mechanism for these prompt managers.

**Why it happens:**
POSIX shell compatibility is a myth. Each shell (bash, zsh, fish, nushell) has distinct hook mechanisms, variable scoping, quoting rules, and built-in function APIs. Shell integration scripts that work in bash cannot be copy-pasted to zsh or fish.

**Consequences:**
- Block decorations don't appear on macOS (zsh hooks not installed)
- OSC 133 sequences emitted in wrong order causing block manager confusion
- Shell integration conflicts with user's existing zsh config (`.zshrc` ordering)
- Pipe capture breaks on macOS's ancient bash 3.2
- Fish users get no shell integration at all

**Prevention:**
1. Write separate shell integration files per shell: `glass.bash`, `glass.zsh`, `glass.fish`. Do NOT use a single POSIX-compatible script.
2. For zsh: use `add-zsh-hook precmd glass_precmd` and `add-zsh-hook preexec glass_preexec` (the `add-zsh-hook` function properly chains with other hooks).
3. For fish: use `function fish_preexec --on-event fish_preexec` and `function fish_postexec --on-event fish_postexec`.
4. Test against macOS bash 3.2 explicitly. Use `#!/usr/bin/env bash` and avoid bash 4+ features in `glass.bash`.
5. For pipe capture on macOS: if bash 3.2 lacks needed features, recommend users install bash 5 via Homebrew and configure Glass to use it.
6. Test integration with Starship AND Oh My Posh on each shell to verify hook ordering.

**Detection:**
- No block separators on macOS with default zsh
- "Command not found: add-zsh-hook" if loaded before zsh modules
- Fish users reporting "glass: not a valid event handler"
- Double-prompt or missing prompt when both Glass and Starship try to set the same hooks

**Phase to address:**
Phase 1 (Shell integration) -- must be one of the first things ported, since all other features (blocks, history, snapshots, pipes) depend on shell integration working.

---

### Pitfall 6: Linux Display Server Fragmentation -- Wayland vs X11 Clipboard and Window Management

**What goes wrong:**
winit handles Wayland vs X11 windowing, but clipboard and certain window behaviors differ:
- **Clipboard on Wayland:** Clipboard data is stored in the source client's memory. When Glass copies text, it must keep serving clipboard requests until another app copies something. If Glass is killed, clipboard content is lost (unlike X11 where the X server holds it). winit does not handle clipboard -- Glass likely uses a separate clipboard crate.
- **Clipboard on X11:** Uses `CLIPBOARD` and `PRIMARY` selections. Most terminal emulators support both (middle-click paste from PRIMARY). Glass probably only handles `CLIPBOARD` currently.
- **IME (Input Method Editor):** Wayland uses `text-input-v3` protocol for IME. X11 uses XIM or IBus. winit's IME support varies. CJK users cannot type in Glass if IME events are not forwarded.
- **Window decorations:** Wayland expects client-side decorations (CSD) by default. X11 uses server-side decorations. winit supports both, but the window title bar appearance differs.
- **DPI scaling:** Wayland handles per-monitor DPI natively. X11 has `Xft.dpi` or `GDK_SCALE` hacks. winit reports `scale_factor()` but Glass's font rendering must respond correctly.

**Why it happens:**
Linux has two competing display server protocols with fundamentally different security and IPC models. Wayland is sandboxed (apps cannot read other apps' windows or clipboard), while X11 is global (any app can access anything). Applications must handle both because ~40% of Linux desktop users are still on X11 (Arch, older Ubuntu, NVIDIA users).

**Consequences:**
- Copy/paste broken on Wayland (clipboard crate uses X11 API)
- Glass crashes on Wayland-only systems (no X11 fallback)
- CJK users cannot type Chinese/Japanese/Korean characters
- Window appears tiny on HiDPI Wayland monitors or huge on X11 with scaling
- Middle-click paste does nothing (no PRIMARY selection support)

**Prevention:**
1. Use the `arboard` crate or `copypasta` crate for clipboard -- they handle Wayland vs X11 automatically. Do NOT use platform-specific clipboard APIs.
2. Test on both Wayland (GNOME on Fedora/Ubuntu 24+) and X11 (XFCE, i3, or `GDK_BACKEND=x11`).
3. For IME: ensure winit's `Ime` events are forwarded to the PTY. Test with `fcitx5` or `ibus` on both Wayland and X11.
4. Handle `ScaleFactorChanged` events to adjust font size and terminal grid dimensions.
5. For Wayland clipboard persistence: consider running a background thread that keeps clipboard data alive, or integrate with `wl-clip-persist`.
6. Support PRIMARY selection (highlight-to-copy, middle-click-to-paste) -- terminal users expect this on Linux.

**Detection:**
- `Ctrl+Shift+C` copies but paste in another app shows nothing (Wayland clipboard lost)
- Glass window has no title bar decorations on Wayland
- Font appears 2x too large or small on HiDPI displays
- IME popup appears but selected characters don't reach the terminal

**Phase to address:**
Phase 2 (Platform windowing/input) -- must be validated before tab bar UI (which adds more clipboard/keyboard complexity).

---

## Moderate Pitfalls

### Pitfall 7: macOS Notarization and Code Signing Requirements

**What goes wrong:**
Distributing a Rust binary on macOS requires code signing and notarization, or users get "this app is damaged" / "cannot verify developer" Gatekeeper warnings. This is more than just signing:
- Notarization requires a DMG, PKG, or .app bundle -- a bare binary cannot be notarized
- Hardened runtime must be enabled (`codesign --options=runtime`)
- The Apple notarization service has experienced multi-hour delays and timeouts (reported as recently as January 2026)
- Requires paid Apple Developer Account ($99/year)
- All nested binaries must be individually signed before the outer bundle is signed

**Prevention:**
1. Build a proper `.app` bundle with `Info.plist`, icon, and the Glass binary inside `Contents/MacOS/`.
2. Sign with `codesign -f --options=runtime -s 'Developer ID Application: ...'`.
3. Submit via `xcrun notarytool submit` (not the deprecated `altool`).
4. Add notarization to CI -- do not rely on manual submission.
5. For Homebrew distribution (recommended for CLI-savvy users): Homebrew taps bypass Gatekeeper for formulae.
6. Budget 1-2 days for notarization pipeline setup -- it always takes longer than expected.

**Phase to address:**
Phase 4 (Packaging/distribution) -- but plan for it early because it requires Apple Developer account setup.

---

### Pitfall 8: Cross-Platform File Path and Config Directory Differences

**What goes wrong:**
Glass uses `dirs::home_dir()` and `~/.glass/` for data storage (history.db, snapshots, blob store). This works on all platforms but violates platform conventions:
- **macOS:** App data goes in `~/Library/Application Support/Glass/`, not `~/.glass/`
- **Linux:** XDG spec says `$XDG_DATA_HOME/glass/` (defaults to `~/.local/share/glass/`), config in `$XDG_CONFIG_HOME/glass/` (defaults to `~/.config/glass/`)
- **Windows:** `%APPDATA%\Glass\` (already somewhat handled)

Additionally, `config.toml` path resolution differs. macOS and Linux users expect different locations for config files.

Path separator differences (`\` vs `/`) can cause issues in CWD tracking (OSC 7 reports paths) and history DB queries that do `cwd.starts_with()` prefix matching.

**Prevention:**
1. Use `dirs::data_dir()` for databases/blobs and `dirs::config_dir()` for `config.toml`. Fall back to `~/.glass/` only if these return `None`.
2. Normalize path separators in CWD tracking and history queries.
3. The `.glass/` project-local directory convention (for per-project history) works cross-platform -- keep it.
4. Consider migrating existing Windows users' data from `~/.glass/` to `%APPDATA%\Glass\` with a one-time migration prompt.

**Phase to address:**
Phase 1 (Platform abstraction) -- affects where config, history, and snapshots are stored, so must be resolved before anything else.

---

### Pitfall 9: notify Crate (File Watcher) Behavioral Differences Across Platforms

**What goes wrong:**
Glass uses `notify 8.2` for filesystem watching in `glass_snapshot`. The notify crate uses different backends per platform:
- **Windows:** `ReadDirectoryChangesW` -- current implementation, works
- **macOS:** FSEvents -- batches events with ambiguous types. A single file save can produce coalesced events where you can't distinguish "create then write" from "modify."
- **Linux:** inotify -- generates 3-5 events for a single file save (editor-dependent: some editors truncate, others create-and-rename). Has system-wide limits (`fs.inotify.max_user_watches` defaults to 8192 on some distros, which can be exhausted when watching large directories).

FSEvents on macOS has a security model that prevents watching files owned by other users. inotify on Linux doesn't work on `/proc` or `/sys` filesystems.

**Prevention:**
1. Glass's snapshot watcher already has debouncing and deduplication logic -- verify it handles FSEvents' batched events correctly.
2. Add a note in documentation about `fs.inotify.max_user_watches` limits on Linux for users watching large project directories.
3. Test watcher behavior with common editors on each platform (VS Code, vim, nano do different things on save).
4. The existing `ignore 0.4` crate integration (`.glassignore`) reduces watch scope, mitigating inotify limits.

**Phase to address:**
Phase 2 (Feature porting) -- snapshot/undo feature must be tested per-platform after PTY and rendering work.

---

### Pitfall 10: Split Pane Rendering and Viewport Calculation Complexity

**What goes wrong:**
Adding split panes requires dividing the wgpu render surface into multiple viewports, each with:
- Independent terminal grid dimensions (columns x rows)
- Independent scroll positions
- Independent cursor positions
- Separate render passes or scissor rects

Common mistakes:
- **Off-by-one in viewport bounds:** A split at 50% of 1920px = 960px, but terminals need integer character cell widths. If cell width is 8px, 960/8 = 120 columns exact, but 961/8 = 120.125, causing 1px gaps or overlaps between panes.
- **Resize cascading:** Resizing the window must recalculate ALL split dimensions proportionally, then resize each PTY, which triggers `SIGWINCH` on Unix or `ResizePseudoConsole` on Windows. This must happen atomically or users see flash-of-wrong-size content.
- **Focus management:** Only one pane receives keyboard input at a time. Clicks must be translated to pane-local coordinates. Mouse events must account for split divider width.
- **Scroll independence:** Each pane has its own scrollback buffer. `Ctrl+Shift+Up/Down` (scroll) must apply to the focused pane only.

**Why it happens:**
Single-session terminals treat the entire window as one viewport. Glass's `grid_renderer.rs` and `rect_renderer.rs` assume a single render target spanning the full surface. Splitting requires parameterizing everything by viewport bounds.

**Prevention:**
1. Introduce a `Viewport` struct: `{ x, y, width, height, columns, rows }`. Every renderer takes a `Viewport` reference.
2. Use wgpu scissor rects (`render_pass.set_scissor_rect()`) to clip rendering per pane -- simpler than multiple render targets.
3. Calculate split dimensions in character cells first, then derive pixel bounds. Never go pixels-first.
4. Buffer resize events -- don't resize PTYs on every pixel of a window drag. Debounce to ~100ms or only resize when character dimensions actually change.
5. Implement splits as a binary tree: `SplitNode::Leaf(Session)` or `SplitNode::Split { direction, ratio, left, right }`. This naturally handles nested splits.

**Phase to address:**
Phase 3 (Split panes) -- but viewport abstraction should be introduced in Phase 2 when adding tab support (tabs are simpler: only one viewport visible at a time).

---

### Pitfall 11: Cross-Platform CI Test Matrix is Expensive and Slow

**What goes wrong:**
Running CI on Windows + macOS + Linux triples build time and cost:
- macOS GitHub Actions runners are 10x more expensive than Linux runners
- Rust compilation on macOS ARM (M-series) vs macOS x86 requires separate targets
- GPU-dependent tests (wgpu rendering) cannot run on CI without GPU -- headless rendering or software rasterization needed
- PTY tests need actual shell processes, which behave differently per OS
- Cross-compilation (e.g., building macOS binary on Linux) requires complex toolchains

**Prevention:**
1. Run most tests on Linux (cheapest). Only run platform-specific integration tests on their native OS.
2. For GPU tests: use wgpu's software backend (`wgpu::Backends::GL` with Mesa's `llvmpipe`) for CI, or skip render tests and rely on manual testing.
3. Use `#[cfg(target_os = "...")]` to gate platform-specific tests.
4. Cache Rust build artifacts aggressively (`sccache` or GitHub Actions cache with target/ directory).
5. Don't cross-compile -- build natively on each platform's runner. Cross-compilation for GUI apps with native dependencies is fragile.
6. Start with Linux x86_64 + Windows x86_64 + macOS ARM64. Add macOS x86_64 only if needed (Rosetta 2 handles it).

**Phase to address:**
Phase 1 (CI setup) -- must be running before any platform-specific code is merged, or regressions accumulate silently.

---

## Minor Pitfalls

### Pitfall 12: Default Shell Detection per Platform

**What goes wrong:**
Glass currently detects `pwsh` vs `powershell` on Windows. On macOS/Linux, detecting the user's preferred shell requires checking `$SHELL` env var, `/etc/passwd`, or defaulting. Common mistakes:
- Defaulting to `bash` on macOS when the user's login shell is `zsh`
- Not respecting `chsh` changes
- Spawning the shell as a login shell (`-l` flag) vs interactive shell (`-i` flag) -- affects which rc files are sourced

**Prevention:**
Use `$SHELL` environment variable on Unix (set by login). Fall back to `/bin/sh`. Spawn as login shell (`-l`) for the first session and interactive shell for subsequent tabs.

**Phase to address:** Phase 1.

---

### Pitfall 13: macOS-Specific Window Lifecycle Events

**What goes wrong:**
macOS has unique window events that winit exposes but Glass may not handle:
- `applicationShouldTerminateAfterLastWindowClosed` -- on macOS, closing all windows doesn't quit the app (dock icon remains). Glass must handle this or users think it's still running.
- Window tabbing: macOS has built-in window tabbing (`NSWindow.tabbingMode`). This can conflict with Glass's own tab implementation.
- Full-screen behavior: macOS full screen creates a new desktop space. Window resize events fire differently.
- App Nap: macOS puts background apps to sleep. If Glass has background tabs running commands, App Nap can throttle them.

**Prevention:**
1. Set `NSWindow.tabbingMode = .disallowed` to prevent macOS native tab merging with Glass's tabs.
2. Handle `applicationShouldTerminate` to gracefully shut down all sessions.
3. Disable App Nap for Glass (or at least for windows with active PTY I/O) via `ProcessInfo.processInfo.beginActivity()`.

**Phase to address:** Phase 2 (macOS platform layer).

---

### Pitfall 14: alacritty_terminal Version Pinning Across Platforms

**What goes wrong:**
Glass pins `alacritty_terminal 0.25.1`. This version supports cross-platform PTY via its `tty` module, but:
- Alacritty's `tty` module on macOS/Linux requires POSIX dependencies (`libc`, signal handling) that may not be tested in isolation
- The crate's internal `cfg` attributes may not cover all platforms uniformly at this exact version
- Updating the pin is risky (alacritty makes breaking API changes between minor versions)

**Prevention:**
1. Keep the pin at 0.25.1 -- it is known to work cross-platform (alacritty itself runs on all three platforms with this crate).
2. Review the crate's `Cargo.toml` for platform-specific dependencies that need to be in Glass's own `Cargo.toml`.
3. Run `cargo build --target` for all three platforms early to catch missing dependency issues.

**Phase to address:** Phase 1 (dependency audit).

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|---------------|------------|
| Platform PTY abstraction | Signal handling differences (SIGWINCH, SIGHUP, SIGCHLD) silently broken on Unix | Add Unix signal handler tests; test child cleanup on crash |
| wgpu renderer porting | Surface format mismatch causes color shift or crash | Query and log format; test on all backends before shipping |
| Shell integration (zsh/fish) | Wrong hook mechanism causes no block decorations | Write per-shell scripts; test on macOS default zsh |
| Keyboard shortcuts | Cmd vs Ctrl mapping breaks copy/paste on macOS | Platform-aware shortcut abstraction from day one |
| Tab lifecycle | Zombie PTY processes accumulate | Session struct with Drop-based cleanup; resource count monitoring |
| Split pane rendering | Off-by-one viewport gaps or overlaps | Character-cell-first dimension calculation |
| Clipboard | Wayland clipboard lost when Glass exits | Use arboard/copypasta; test on Wayland |
| Config/data paths | Wrong directory on macOS/Linux | Use dirs crate platform-aware directories |
| File watcher | inotify limits exceeded on Linux | Document limits; use .glassignore aggressively |
| CI matrix | macOS CI runners expensive and slow | Run most tests on Linux; native build only |
| macOS distribution | Gatekeeper blocks unsigned binary | Code signing + notarization pipeline in CI |
| macOS window lifecycle | App doesn't quit when all windows closed | Handle NSApp termination events |

## Integration Gotchas

Mistakes when integrating cross-platform + tabs/splits with existing Glass systems.

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| BlockManager per session | Sharing one BlockManager across tabs/panes | Each session gets its own BlockManager instance. Block IDs must be session-scoped. |
| History DB per session | All tabs writing to same DB concurrently without coordination | Use a single DB connection pool with WAL mode. Each session identifies itself with a session_id column. |
| Snapshot watcher per session | Running N file watchers for N sessions all watching similar directories | Share a single watcher instance across sessions in the same CWD. Demux events to relevant sessions. |
| OSC 7 CWD tracking | CWD updates from one tab affecting status bar of another | CWD is per-session state. Status bar must read from focused session only. |
| Search overlay | Global search searching only the focused tab | Decision: search focused tab (simple) or all tabs (complex). Decide upfront and be consistent. |
| Git status polling | N sessions polling git status independently for same repo | Share git status across sessions with same CWD. Single poller, multiple consumers. |
| MCP server | MCP context returns data from wrong session | MCP should expose all sessions or accept a session filter parameter. |

## "Looks Done But Isn't" Checklist

- [ ] **PTY cleanup:** Close 50 tabs rapidly. Zero zombie processes remain (`ps aux | grep defunct` on Linux/macOS, Task Manager on Windows).
- [ ] **Surface format:** Colors identical on DX12, Metal, and Vulkan. Screenshot comparison test.
- [ ] **Keyboard:** `Cmd+C` copies on macOS. `Ctrl+C` sends SIGINT to shell. Both work correctly.
- [ ] **Keyboard:** `Ctrl+Shift+C` copies on Linux. Does NOT conflict with terminal sequences.
- [ ] **Shell integration:** zsh on macOS shows block decorations. bash on Linux shows block decorations. fish on both shows block decorations.
- [ ] **Clipboard:** Copy in Glass, paste in another app -- works on Wayland, X11, macOS, Windows.
- [ ] **Resize:** Resize window with 4 split panes. No gaps between panes, no overlapping text, no flash of wrong content.
- [ ] **Config path:** `config.toml` loads from platform-correct location. `~/.glass/config.toml` works as fallback.
- [ ] **File watcher:** Snapshot undo works on all three platforms. FSEvents batching doesn't cause missed snapshots.
- [ ] **macOS:** App quits when last window closes (or docks correctly). No App Nap throttling of background tabs.
- [ ] **Linux:** Works on both Wayland (GNOME 46+) and X11 (i3, XFCE). No crash on either.
- [ ] **CI:** All platforms green. No platform-specific test skipped without documented reason.

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Zombie PTY processes | LOW | Fix Session Drop impl. Users kill orphans manually meanwhile. |
| Wrong surface format/colors | LOW | Fix format selection. No data corruption. |
| Cmd/Ctrl mapping broken on macOS | LOW | Fix shortcut mapping. Users can workaround via config. |
| Shell integration broken on zsh | MEDIUM | Write and test glass.zsh. Users lose block features until fixed. |
| Wayland clipboard broken | MEDIUM | Switch to arboard crate. Users use terminal selection meanwhile. |
| Split viewport off-by-one | LOW | Fix calculation. No data loss, just visual glitch. |
| inotify limit exceeded | LOW | Increase system limit. Document in README. |
| macOS notarization blocked | MEDIUM | Set up signing pipeline. Users bypass Gatekeeper via xattr meanwhile. |
| History DB corruption from concurrent tab writes | HIGH | Switch to WAL mode and connection pool. May lose recent history. |
| alacritty_terminal platform incompatibility | HIGH | Must fix or fork the crate. Blocks all cross-platform work. |

## Sources

- [WezTerm PTY and Process Management (DeepWiki)](https://deepwiki.com/wezterm/wezterm/4.5-pty-and-process-management) -- portable-pty architecture, cross-platform PTY abstraction patterns
- [portable-pty crate (docs.rs)](https://docs.rs/portable-pty) -- cross-platform PTY API reference
- [wgpu Backend Selection Issue #1416](https://github.com/gfx-rs/wgpu/issues/1416) -- backend priority and selection behavior
- [wgpu Metal Shader Timeout Issue #4456](https://github.com/gfx-rs/wgpu/issues/4456) -- WGSL to Metal compilation hangs
- [wgpu Metal Integer Min Issue #4399](https://github.com/gfx-rs/wgpu/issues/4399) -- i32::MIN breaks Metal shader compilation
- [Cross-Platform Rust Graphics with wgpu (BrightCoding)](https://www.blog.brightcoding.dev/2025/09/30/cross-platform-rust-graphics-with-wgpu-one-api-to-rule-vulkan-metal-d3d12-opengl-webgpu/) -- wgpu cross-platform patterns
- [macOS Code Signing and Notarization Guide (rsms gist)](https://gist.github.com/rsms/929c9c2fec231f0cf843a1a746a416f5) -- comprehensive macOS distribution guide
- [Notarizing CLI Apps for macOS (Random Errata)](https://www.randomerrata.com/articles/2024/notarize/) -- practical notarization walkthrough
- [Resolving Common Notarization Issues (Apple)](https://developer.apple.com/documentation/security/resolving-common-notarization-issues) -- official Apple troubleshooting
- [Signing and Notarizing on macOS (Armaan Aggarwal)](https://armaan.cc/blog/signing-and-notarizing-macos/) -- exhaustive signing guide
- [Wayland vs X11 in 2025 (dasroot.net)](https://dasroot.net/posts/2025/11/wayland-vs-x11/) -- current state of display server ecosystem
- [wl-clipboard (GitHub)](https://github.com/bugaevc/wl-clipboard) -- Wayland clipboard utilities
- [notify crate (GitHub)](https://github.com/notify-rs/notify) -- cross-platform file watcher, platform backend differences
- [Building a Cross-Platform Rust CI/CD Pipeline (Ahmed Jama)](https://ahmedjama.com/blog/2025/12/cross-platform-rust-pipeline-github-actions/) -- GitHub Actions CI matrix patterns
- [Zombie Process Accumulation in Terminal Apps (opencode #11225)](https://github.com/anomalyco/opencode/issues/11225) -- real-world zombie process leak
- [Auto-Claude Process Cleanup Bug (#1252)](https://github.com/AndyMik90/Auto-Claude/issues/1252) -- Windows zombie process accumulation
- [Alacritty Architecture (DeepWiki)](https://deepwiki.com/alacritty/alacritty) -- cross-platform terminal architecture reference
- Glass source: `crates/glass_terminal/src/pty.rs` -- current PTY reader implementation (ConPTY-only)
- Glass source: `crates/glass_renderer/src/surface.rs` -- current wgpu DX12-forced backend selection
- Glass source: `crates/glass_history/src/lib.rs` -- `resolve_db_path` using `dirs::home_dir()`
- Glass source: `crates/glass_snapshot/src/watcher.rs` -- notify-based file watcher

---
*Pitfalls research for: Glass v2.0 -- Cross-Platform & Tabs/Splits*
*Researched: 2026-03-06*

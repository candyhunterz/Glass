# Cross-Platform Compatibility Audit

**Project:** Glass -- GPU-accelerated terminal emulator (Rust)
**Date:** 2026-03-18
**Auditor:** Claude Code (automated)
**Scope:** Prelaunch readiness -- cross-platform support for Windows, macOS, and Linux

## Summary

Glass has **solid cross-platform foundations**: platform-specific code is gated with `#[cfg(target_os = ...)]` and `#[cfg(unix/windows)]` throughout, CI tests all three platforms, and critical subsystems (PTY, IPC, process control, font defaults, packaging) each have per-platform implementations. The codebase demonstrates awareness of platform differences (ConPTY vs forkpty, named pipes vs Unix sockets, DX12 vs Vulkan/Metal, dunce path canonicalization).

There are **11 findings** ranging from Medium to Low severity. No Critical issues were found. The most significant gap is the missing macOS orphan prevention for child processes and missing pipeline capture in the zsh/fish shell integration scripts.

---

## Findings by Category

### 1. PTY Layer

#### F-01: PTY implementation is fully delegated to alacritty_terminal -- correct and robust
- **Severity:** Info (positive finding)
- **Files:** `crates/glass_terminal/src/pty.rs` (lines 33-36, 120-139, 181-182)
- **Platforms:** All
- **Description:** PTY spawning delegates to `alacritty_terminal::tty::new()`, which uses ConPTY on Windows and forkpty on Unix. Platform-specific token values are correctly defined per platform (PTY_READ_WRITE_TOKEN: 2 on Windows, 0 on Unix). The `escape_args` field is correctly gated with `#[cfg(target_os = "windows")]`. Default shell detection probes for `pwsh` on Windows (with CREATE_NO_WINDOW to avoid console flash) and reads `$SHELL` on Unix. Error handling is present for all PTY operations.
- **Recommendation:** None needed.

---

### 2. Orphan Prevention (Child Process Cleanup)

#### F-02: macOS has no orphan prevention mechanism for agent child processes
- **Severity:** Medium
- **File:** `src/main.rs` (lines 793-840, 1128-1145), `src/ephemeral_agent.rs` (lines 129-145)
- **Platforms:** macOS
- **Description:** Windows uses a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` to ensure child processes (specifically the `claude` subprocess) are killed when Glass exits. Linux uses `PR_SET_PDEATHSIG(SIGKILL)` via `prctl` in a `pre_exec` hook. macOS has **neither** mechanism. The comments explicitly note: "prctl is Linux-specific; macOS does not have it." If Glass crashes or is force-killed on macOS, the `claude` subprocess will become an orphan.
- **Recommendation:** Implement macOS orphan prevention using one of:
  - `kqueue` with `EVFILT_PROC` + `NOTE_EXIT` on the parent PID
  - A watchdog thread that periodically checks `getppid()` and kills the child if reparented to init (PID 1)
  - `posix_spawn` attributes with `POSIX_SPAWN_SETSIGDEF` (less reliable)

#### F-03: Job Object handle stored as `Option<isize>` -- could use a stronger type
- **Severity:** Low
- **File:** `src/main.rs` (line 387-389)
- **Platforms:** Windows
- **Description:** The Windows Job Object handle is stored as `Option<isize>` with `#[allow(dead_code)]`. The field exists solely to prevent the handle from being dropped (which triggers kill-on-close). While functionally correct, using a raw `isize` for a Windows HANDLE is a code smell. An `OwnedHandle` wrapper or at minimum a comment explaining the lifetime semantics would improve clarity.
- **Recommendation:** Low priority. Consider wrapping in a newtype with a `Drop` impl that calls `CloseHandle`.

---

### 3. Shell Integration

#### F-04: Pipeline capture is missing from zsh and fish shell integration scripts
- **Severity:** Medium
- **File:** `shell-integration/glass.zsh`, `shell-integration/glass.fish`
- **Platforms:** macOS (zsh default), Linux (fish users)
- **Description:** The bash (`glass.bash`) and PowerShell (`glass.ps1`) scripts include full pipeline capture: `tee` rewriting, OSC 133;S/P emission, and temp file cleanup. The zsh and fish scripts only implement basic OSC 133 command lifecycle (A/B/C/D) and OSC 7 CWD reporting. Users on macOS (where zsh is the default shell) and fish users will not get pipeline visualization. Searching for `GLASS_PIPES_DISABLED` or `tee_rewrite` confirms only bash and ps1 contain these features.
- **Recommendation:** Implement pipeline capture in `glass.zsh` and `glass.fish`. Zsh supports `preexec` hooks and `READLINE_LINE` equivalents via `zle` widgets. Fish supports `fish_preexec` hooks and `commandline` for buffer manipulation.

#### F-05: Shell integration script discovery assumes specific directory layouts
- **Severity:** Low
- **File:** `src/main.rs` (lines 9499-9528)
- **Platforms:** All (but primarily affects installed deployments)
- **Description:** `find_shell_integration()` checks two paths: `<exe_dir>/shell-integration/<script>` (installed) and `<exe_dir>/../../shell-integration/<script>` (development). The macOS `.app` bundle places the binary in `Contents/MacOS/` but does **not** include the `shell-integration/` directory. The DMG build script (`packaging/macos/build-dmg.sh`) copies only the binary. Similarly, the Windows MSI (`wix/main.wxs`) only installs `glass.exe` and the license file.
- **Recommendation:** Update packaging to include shell integration scripts:
  - macOS: copy `shell-integration/` into `Contents/Resources/` and adjust `find_shell_integration()` to check `../Resources/shell-integration/`
  - Windows MSI: add shell integration files as additional WiX components in `main.wxs`
  - Linux deb: add to the `assets` list in `Cargo.toml` `[package.metadata.deb]`

---

### 4. GPU/Rendering

#### F-06: DX12-only backend on Windows may fail on older hardware
- **Severity:** Low
- **File:** `crates/glass_renderer/src/surface.rs` (lines 22-28)
- **Platforms:** Windows
- **Description:** On Windows, the wgpu backend is hardcoded to `wgpu::Backends::DX12`. On non-Windows, it uses `wgpu::Backends::all()` which allows Vulkan, Metal, or OpenGL fallback. Windows systems with very old GPUs or drivers that only support DX11 or Vulkan (but not DX12) will fail at adapter selection with the panic "No compatible GPU adapter found". The `.expect()` on line 41 will crash the application.
- **Recommendation:** Use `wgpu::Backends::DX12 | wgpu::Backends::VULKAN` on Windows to allow Vulkan fallback. Alternatively, convert the `.expect()` to a graceful error message suggesting a driver update.

#### F-07: Surface format selection uses sRGB preference -- correct cross-platform
- **Severity:** Info (positive finding)
- **File:** `crates/glass_renderer/src/surface.rs` (lines 64-69)
- **Platforms:** All
- **Description:** The renderer prefers sRGB formats but falls back to the first available format. This handles Metal (which often returns non-sRGB Bgra8Unorm as the first format) and Vulkan/DX12 correctly.
- **Recommendation:** None needed.

---

### 5. File Paths

#### F-08: Path handling is correctly cross-platform throughout
- **Severity:** Info (positive finding)
- **Files:** Multiple (glass_coordination, glass_history, glass_snapshot, glass_core)
- **Platforms:** All
- **Description:** The codebase consistently uses `std::path::Path` and `PathBuf::join()` rather than hardcoded separators. Windows-specific concerns are addressed:
  - `dunce::canonicalize()` avoids UNC `\\?\` prefix issues (`crates/glass_coordination/src/lib.rs:34`)
  - Windows paths are lowercased for case-insensitive comparison (`lib.rs:39`)
  - The orchestrator's PRD parser handles both `/` and `\\` in file detection (`src/orchestrator.rs:191`)
  - Config paths use `dirs::home_dir()` + `.join()` throughout
- **Recommendation:** None needed.

---

### 6. IPC Infrastructure

#### F-09: IPC path/name duplication between glass_core and glass_mcp
- **Severity:** Low
- **Files:** `crates/glass_core/src/ipc.rs` (lines 74-86), `crates/glass_mcp/src/ipc_client.rs` (lines 121-134)
- **Platforms:** All
- **Description:** The IPC socket path (Unix) and named pipe name (Windows) are duplicated across two crates with identical strings. The comment in `ipc_client.rs` explains this is intentional to avoid pulling in `glass_core` (which brings `winit`). While not a bug, any change to the IPC path requires coordinated updates in two files.
- **Recommendation:** Consider extracting the path constants into a tiny shared crate, or a `const` in a leaf crate both can depend on. Low priority since the values are stable.

---

### 7. CI Coverage

#### F-10: Clippy runs only on Windows; should run on all platforms
- **Severity:** Low
- **File:** `.github/workflows/ci.yml` (lines 46-54)
- **Platforms:** macOS, Linux (not linted)
- **Description:** The clippy job runs only on `windows-latest`. Platform-specific `#[cfg]` blocks for macOS and Linux are never lint-checked by CI. Dead code or lint violations inside `#[cfg(target_os = "macos")]` or `#[cfg(target_os = "linux")]` blocks would go undetected.
- **Recommendation:** Add clippy jobs for macOS and Linux, or convert clippy to a matrix job across all three platforms.

---

### 8. Conditional Compilation

#### F-11: `winresource` build dependency is unconditional but only used on Windows
- **Severity:** Low
- **File:** `Cargo.toml` (line 133)
- **Platforms:** macOS, Linux (unnecessary dependency)
- **Description:** `winresource = "0.1"` is listed under `[build-dependencies]` unconditionally. The `build.rs` gates its use with `#[cfg(target_os = "windows")]`, so on non-Windows platforms the crate is compiled but never called. `winresource` depends on the `toml` crate, adding unnecessary compile time on macOS/Linux.
- **Recommendation:** Move to `[target.'cfg(windows)'.build-dependencies]`. Note: this requires Cargo edition 2024+ or a `build.rs` that conditionally depends on it. Alternatively, use `#[cfg(windows)]` in build.rs with the unconditional dep (current approach works, just wastes compile time).

---

### 9. Font Handling

#### Fonts are correctly platform-aware
- **Severity:** Info (positive finding)
- **Files:** `crates/glass_core/src/config.rs` (lines 379-392), `crates/glass_renderer/src/settings_overlay.rs` (lines 112-123)
- **Platforms:** All
- **Description:** Default fonts are correctly set per platform:
  - Windows: "Consolas"
  - macOS: "Menlo"
  - Linux/Other: "Monospace"
  Font discovery uses `glyphon::FontSystem::new()` which delegates to `fontdb` for system font enumeration. This works correctly on all platforms (DirectWrite on Windows, Core Text on macOS, fontconfig on Linux). The font system is initialized on a separate thread to parallelize with GPU init.
- **Recommendation:** None needed.

---

### 10. Clipboard, Keyboard, Window Management

#### Clipboard is correctly cross-platform via arboard
- **Severity:** Info (positive finding)
- **Files:** `src/main.rs` (lines 9531-9548)
- **Platforms:** All
- **Description:** Clipboard access uses the `arboard` crate (v3), which handles:
  - Windows: Win32 clipboard API
  - macOS: NSPasteboard
  - Linux: X11 clipboard / Wayland (via wl-clipboard)
  Error handling uses `if let Ok(...)` pattern, gracefully ignoring clipboard failures.

#### Keyboard input encoding is platform-neutral
- **Severity:** Info (positive finding)
- **File:** `crates/glass_terminal/src/input.rs`
- **Platforms:** All
- **Description:** Key encoding translates winit `Key` events to terminal escape sequences. Platform-specific modifier mapping (Cmd vs Ctrl) is handled in `crates/glass_mux/src/platform.rs` via `is_action_modifier()` and `is_glass_shortcut()`. macOS uses Meta (Cmd), Windows/Linux use Ctrl+Shift for Glass shortcuts.

---

## Platform Support Matrix

| Feature | Windows | macOS | Linux | Notes |
|---------|---------|-------|-------|-------|
| PTY (ConPTY/forkpty) | Yes | Yes | Yes | Delegated to alacritty_terminal |
| Shell detection | pwsh/powershell | $SHELL/zsh | $SHELL/bash | All with fallbacks |
| Shell integration (basic) | PowerShell | zsh/bash/fish | bash/zsh/fish | OSC 133 A/B/C/D |
| Pipeline capture | PowerShell, bash | bash only | bash only | **Missing: zsh, fish** |
| GPU backend | DX12 only | Metal+Vulkan | Vulkan+OpenGL | **DX12-only may fail on old Windows** |
| Orphan prevention | Job Object | **None** | prctl PDEATHSIG | **macOS gap** |
| IPC | Named pipe | Unix socket | Unix socket | Correct per platform |
| Font defaults | Consolas | Menlo | Monospace | Correct per platform |
| Clipboard | Win32 API | NSPasteboard | X11/Wayland | Via arboard |
| Path canonicalization | dunce + lowercase | std | std | Correct |
| Console codepage | UTF-8 (65001) | N/A | N/A | Windows-specific |
| Build resource icon | .ico embedded | N/A | N/A | Windows-specific |
| Packaging | MSI (WiX) | DMG | deb | All three in release CI |
| CI build+test | Yes | Yes | Yes | All three |
| CI clippy | Yes | **No** | **No** | Windows-only |
| System deps | None | None | libwayland, libxkbcommon, libx11, libxi, libxtst | Installed in CI |

---

## Priority Fix List

### Pre-Launch (should fix before release)

1. **F-04 (Medium): Add pipeline capture to zsh and fish shell integration.** macOS defaults to zsh; pipeline visualization is a key product feature and will not work on macOS without this. Impact: macOS and fish users see no pipe stage data.

2. **F-02 (Medium): Add macOS orphan prevention.** If Glass crashes on macOS, the `claude` subprocess becomes an orphan consuming resources. A watchdog thread checking `getppid() == 1` is the simplest approach.

3. **F-05 (Low, but user-facing): Package shell integration scripts in installers.** Without this, installed users on all platforms cannot get shell integration unless they manually copy files. The MSI, DMG, and deb all need updates.

### Post-Launch (should fix but not blocking)

4. **F-10 (Low): Run clippy on all three platforms in CI.** Currently platform-specific macOS/Linux code is not lint-checked.

5. **F-06 (Low): Allow Vulkan fallback on Windows.** DX12-only may fail on edge-case hardware. Adding `DX12 | VULKAN` is a one-line change.

6. **F-11 (Low): Make winresource a Windows-only build dependency.** Saves compile time on macOS/Linux CI.

7. **F-09 (Low): Deduplicate IPC path constants.** Prevents drift between glass_core and glass_mcp.

8. **F-03 (Low): Wrap Windows Job Object HANDLE in a proper type.** Code hygiene improvement.

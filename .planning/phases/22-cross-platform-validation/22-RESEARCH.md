# Phase 22: Cross-Platform Validation - Research

**Researched:** 2026-03-06
**Domain:** Cross-platform compilation, platform-specific code validation, CI pipeline, shell integration
**Confidence:** HIGH

## Summary

Phase 22 validates that Glass compiles and runs correctly on macOS and Linux, building on Phase 21's platform cfg-gates and SessionMux extraction. The codebase is already well-positioned: all major dependencies (wgpu 28, winit 0.30.13, alacritty_terminal 0.25.1, notify 8.2, arboard 3, rusqlite with bundled SQLite, dirs 6) are cross-platform. Phase 21 added cfg-gated platform helpers in `glass_mux::platform` (shell detection, modifier keys, config/data dirs) and a `glass.zsh` shell integration script.

The primary work falls into five areas: (1) fixing compilation blockers -- the `windows-sys` dependency is unconditional in the root binary, spawn_pty has hardcoded Windows shell detection, and shell integration injection only handles PowerShell; (2) validating wgpu surface format negotiation across DX12/Metal/Vulkan; (3) making config defaults platform-aware (font family defaults to "Consolas" which doesn't exist on macOS/Linux); (4) ensuring shell integration works on zsh (macOS) and bash (Linux); and (5) establishing a CI cross-compilation pipeline.

**Primary recommendation:** Gate windows-sys behind `cfg(windows)`, make spawn_pty use `glass_mux::platform::default_shell()`, generalize shell integration injection for all shells, add platform-aware font defaults, validate cross-compilation for all three targets, and set up GitHub Actions CI matrix.

## Standard Stack

### Core (No new dependencies)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| alacritty_terminal | =0.25.1 | PTY abstraction (ConPTY/forkpty) | Already cross-platform internally |
| wgpu | 28.0.0 | GPU rendering (DX12/Metal/Vulkan/GL) | Backend auto-selection via `Backends::all()` already in code |
| winit | 0.30.13 | Windowing (Win32/Cocoa/X11+Wayland) | Already cross-platform |
| arboard | 3 | Clipboard (WinAPI/NSPasteboard/X11/Wayland) | Already a dependency, handles all platforms |
| notify | 8.2 | File watching (ReadDirectoryChangesW/FSEvents/inotify) | Already a dependency, backend auto-selected |
| dirs | 6 | Platform config/data directories | Already a dependency |
| rusqlite | 0.38.0 (bundled) | SQLite database | Bundled feature compiles SQLite from source on all platforms |

### No New Dependencies Needed

The existing stack is fully cross-platform. Phase 22 is about code changes, not dependency additions.

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Manual cross-check via `cargo check --target` | Full CI on native runners | `cargo check` catches compilation errors cheaply; native runners needed for runtime validation |
| Platform font detection crate | Hardcoded per-platform defaults | Hardcoded defaults are simpler and sufficient -- users can override via config.toml |

## Architecture Patterns

### Pattern 1: Conditional windows-sys Dependency

**What:** Gate `windows-sys` behind `cfg(windows)` in both Cargo.toml and source code.

**Current problem:** Root binary `Cargo.toml` has `windows-sys.workspace = true` unconditionally. The `use windows_sys::...` in `main.rs:1316` and `tests.rs:162` is already cfg-gated, but the dependency itself compiles on all platforms (wasted build time, potential link issues).

**Fix:**
```toml
# Root Cargo.toml -- change from:
# windows-sys.workspace = true
# To:
[target.'cfg(windows)'.dependencies]
windows-sys = { workspace = true }
```

### Pattern 2: Platform-Aware Shell Detection in spawn_pty

**What:** Replace hardcoded pwsh/powershell detection in `spawn_pty()` with `glass_mux::platform::default_shell()`.

**Current problem:** `pty.rs` lines 115-121 hardcode Windows shell detection. On macOS/Linux this would fall through to "powershell" which doesn't exist.

**Fix:** When `shell_override` is None, call `glass_mux::platform::default_shell()` instead of the inline detection. This function already exists and handles all three platforms correctly.

### Pattern 3: Platform-Aware Shell Integration Injection

**What:** Generalize the shell integration injection in `resumed()` to handle bash, zsh, and fish, not just PowerShell.

**Current problem:** `main.rs:269-281` only injects shell integration for PowerShell. The `is_powershell` check returns true when `shell_name.is_empty()` (default on Windows). On macOS/Linux, the default shell is zsh/bash, so `is_powershell` would be false and no integration is injected.

**Fix:** Determine the effective shell name (from config or platform default), find the matching integration script, and inject it with the correct source command for each shell type:
- PowerShell: `. 'path/glass.ps1'\r\n`
- Bash: `source 'path/glass.bash'\r\n`
- Zsh: `source 'path/glass.zsh'\r\n`
- Fish: `source path/glass.fish\r\n`

### Pattern 4: Platform-Aware Config Defaults

**What:** Make `GlassConfig::default()` return platform-appropriate font family.

**Current problem:** Default font is "Consolas" which only exists on Windows. macOS has "Menlo" or "SF Mono". Linux typically has "Monospace" or "DejaVu Sans Mono".

**Fix:**
```rust
impl Default for GlassConfig {
    fn default() -> Self {
        Self {
            font_family: default_font_family().into(),
            font_size: 14.0,
            shell: None,
            history: None,
            snapshot: None,
            pipes: None,
        }
    }
}

fn default_font_family() -> &'static str {
    #[cfg(target_os = "windows")]
    { "Consolas" }
    #[cfg(target_os = "macos")]
    { "Menlo" }
    #[cfg(target_os = "linux")]
    { "Monospace" }
}
```

### Pattern 5: Surface Format Logging and Validation

**What:** Log the selected surface format in `GlassRenderer::new()` and validate it handles both sRGB and non-sRGB formats.

**Current code:** `surface.rs:55` blindly takes `caps.formats[0]` which returns different formats per backend (DX12: Bgra8UnormSrgb, Metal: Bgra8Unorm, Vulkan: varies).

**Recommendation:** Log the format and prefer an sRGB format if available. If only non-sRGB is available, it still works but colors may appear slightly different. This is a validation concern, not necessarily a code change -- just log and document.

### Pattern 6: HiDPI/Scale Factor Awareness

**What:** Ensure scale factor is correctly plumbed through text rendering on all platforms.

**Current code:** `resumed()` at line 208 reads `window.scale_factor()` and passes it to `FrameRenderer`. This is correct. The risk is on macOS Retina displays (scale_factor = 2.0) and Linux HiDPI (variable scale factors on Wayland).

**Recommendation:** Verify that `WindowEvent::ScaleFactorChanged` is handled. Currently, the code only reads scale_factor at window creation. If the window moves between monitors with different DPIs, the font rendering would be wrong. Add handling for `ScaleFactorChanged` to reinitialize font metrics.

### Anti-Patterns to Avoid

- **Testing only with `cargo check`**: Cross-compilation check catches type errors but not runtime behavior. Must test on actual macOS/Linux for GPU, PTY, clipboard, and shell integration.
- **Skipping Wayland testing**: Many Linux developers use X11. Wayland has different clipboard, IME, and window decoration behavior. Test on both.
- **Assuming bash 4+ on macOS**: macOS ships bash 3.2. The glass.bash pipe capture uses bash 4.4+ features (PS0, bind -x) and correctly gates them. Verify this gate works on macOS bash 3.2.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Cross-platform PTY | Custom PTY wrapper | alacritty_terminal::tty | Already the abstraction layer; handles ConPTY/forkpty |
| Clipboard | Platform-specific clipboard code | arboard 3 | Handles Win/macOS/X11/Wayland automatically |
| Platform directories | Custom path detection | dirs 6 | XDG on Linux, ~/Library on macOS, %APPDATA% on Windows |
| GPU backend selection | Manual backend matching | wgpu `Backends::all()` | Already in code; auto-selects Metal/Vulkan/GL |
| File watching backends | Platform-specific inotify/FSEvents | notify 8.2 | Backend selection is compile-time automatic |

## Common Pitfalls

### Pitfall 1: windows-sys Compilation on Non-Windows

**What goes wrong:** The `windows-sys` crate is listed as an unconditional dependency in root `Cargo.toml`. While `windows-sys` does compile on non-Windows (it's stubs), the `use windows_sys::Win32::System::Console::...` imports in `main.rs` will fail to resolve on non-Windows if not properly cfg-gated.
**Why it happens:** The code uses are cfg-gated but the dependency itself is not platform-gated in Cargo.toml.
**How to avoid:** Move to `[target.'cfg(windows)'.dependencies]` in Cargo.toml.
**Warning signs:** Compilation error on macOS/Linux mentioning windows_sys.

### Pitfall 2: Shell Integration Not Injected on macOS/Linux

**What goes wrong:** Glass launches on macOS/Linux but shows no block decorations, no CWD tracking, no exit codes -- because shell integration is never sourced.
**Why it happens:** The current `is_powershell` guard in `resumed()` gates all shell integration injection. When the default shell is zsh or bash, no integration is injected.
**How to avoid:** Make shell integration injection unconditional (for all recognized shells). Determine shell type from config or platform default, find the matching script, inject the appropriate source command.
**Warning signs:** No block separators visible. OSC 133 sequences never received by OscScanner.

### Pitfall 3: Font "Consolas" Not Found on macOS/Linux

**What goes wrong:** Glass launches but renders with a fallback system font (possibly serif or unreadable) because "Consolas" doesn't exist on the platform.
**Why it happens:** `GlassConfig::default()` hardcodes "Consolas". glyphon falls back to a system default when the requested font isn't found.
**How to avoid:** Use platform-aware font defaults: "Consolas" on Windows, "Menlo" on macOS, "Monospace" on Linux.
**Warning signs:** Ugly or wrong font on first launch without config.toml.

### Pitfall 4: spawn_pty Falls Through to "powershell" on macOS/Linux

**What goes wrong:** Glass tries to spawn "powershell" (which doesn't exist) on macOS/Linux, causing a panic at `tty::new().expect("Failed to spawn ConPTY (pwsh)")`.
**Why it happens:** `spawn_pty()` in `pty.rs` has inline Windows-only shell detection. The else branch defaults to "powershell".
**How to avoid:** Use `glass_mux::platform::default_shell()` when no shell override is configured.
**Warning signs:** Panic on startup: "Failed to spawn ConPTY (pwsh)".

### Pitfall 5: wgpu Surface Format Color Differences

**What goes wrong:** Colors look washed out or too saturated on macOS/Linux compared to Windows.
**Why it happens:** DX12 typically returns `Bgra8UnormSrgb`, Metal may return `Bgra8Unorm`. sRGB vs linear gamma interpretation of the same color values produces visually different results.
**How to avoid:** Log the selected format. Consider selecting an sRGB format explicitly from `caps.formats` rather than taking `[0]`. The shader pipeline may need adjustment if formats differ.
**Warning signs:** Side-by-side color comparison shows differences between platforms.

### Pitfall 6: macOS Retina / Linux HiDPI Scale Factor

**What goes wrong:** Text appears tiny on Retina displays or blurry on HiDPI Linux monitors.
**Why it happens:** Scale factor is read at window creation but `ScaleFactorChanged` events are not handled.
**How to avoid:** Handle `WindowEvent::ScaleFactorChanged` to reinitialize font metrics and resize the terminal grid.
**Warning signs:** Text too small on 4K/Retina displays, or wrong size after moving window between monitors.

### Pitfall 7: Pipe Capture Breaks on macOS Default Bash 3.2

**What goes wrong:** Pipeline visualization doesn't work on macOS when using the system bash (3.2).
**Why it happens:** The `glass.bash` pipe capture uses PS0 and `bind -x` which require bash 4.4+. The script correctly gates these features behind a version check, so pipe capture is silently disabled.
**How to avoid:** This is expected behavior -- document it. Users who want pipe capture on macOS can install bash 5 via Homebrew or use zsh.
**Warning signs:** No pipe stages captured on macOS bash 3.2. Block decorations still work (those use PROMPT_COMMAND which works on bash 3.x).

### Pitfall 8: glass.zsh Missing Pipe Capture

**What goes wrong:** Zsh shell integration script (`glass.zsh`) has OSC 133 hooks but no pipe capture (unlike `glass.bash`).
**Why it happens:** Phase 21 created a basic `glass.zsh` with precmd/preexec hooks but didn't add tee-rewriting for pipe capture. Zsh has different hook mechanisms than bash (no `bind -x`, no `READLINE_LINE`).
**How to avoid:** Accept this as a known limitation for Phase 22. Pipe capture for zsh would require zsh-specific tee rewriting (using `preexec` hook to intercept and rewrite the command). Defer to a future enhancement.
**Warning signs:** Pipes work on bash but not zsh.

## Code Examples

### Platform-Gated windows-sys in Cargo.toml
```toml
# Root Cargo.toml
# Remove: windows-sys.workspace = true
# Add:
[target.'cfg(windows)'.dependencies]
windows-sys = { workspace = true }
```

### Platform-Aware Shell Detection in spawn_pty
```rust
// crates/glass_terminal/src/pty.rs
// Change shell detection from inline Windows-only to platform-aware:
let shell_program = if let Some(shell) = shell_override {
    shell.to_owned()
} else {
    glass_mux::platform::default_shell()
};
```

Note: This creates a dependency from glass_terminal to glass_mux. Alternative: duplicate the platform detection logic in glass_terminal, or extract it to glass_core. The simplest option is to pass the effective shell as a parameter from main.rs where both config and platform are available.

### Generalized Shell Integration Injection
```rust
// In main.rs resumed(), replace the PowerShell-only injection:
let effective_shell = self.config.shell.as_deref()
    .map(|s| s.to_owned())
    .unwrap_or_else(|| glass_mux::platform::default_shell());

if let Some(path) = find_shell_integration(&effective_shell) {
    let inject_cmd = if effective_shell.contains("fish") {
        format!("source {}\r\n", path.display())
    } else {
        // Works for bash, zsh, and powershell (all use `. 'path'` or `source 'path'`)
        format!("source '{}'\r\n", path.display())
    };
    let _ = pty_sender.send(PtyMsg::Input(Cow::Owned(inject_cmd.into_bytes())));
    tracing::info!("Auto-injecting shell integration: {}", path.display());
} else {
    tracing::warn!("Shell integration script not found for: {}", effective_shell);
}
```

### Platform-Aware Default Font
```rust
// crates/glass_core/src/config.rs
fn default_font_family() -> &'static str {
    #[cfg(target_os = "windows")]
    { "Consolas" }
    #[cfg(target_os = "macos")]
    { "Menlo" }
    #[cfg(target_os = "linux")]
    { "Monospace" }
}

impl Default for GlassConfig {
    fn default() -> Self {
        Self {
            font_family: default_font_family().into(),
            font_size: 14.0,
            shell: None,
            history: None,
            snapshot: None,
            pipes: None,
        }
    }
}
```

### GitHub Actions CI Matrix
```yaml
# .github/workflows/ci.yml
name: CI
on: [push, pull_request]

jobs:
  build:
    strategy:
      matrix:
        include:
          - os: windows-latest
            target: x86_64-pc-windows-msvc
          - os: macos-latest
            target: aarch64-apple-darwin
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Build
        run: cargo build --release
      - name: Test
        run: cargo test --workspace
```

### Cross-Compilation Check (Local Dev)
```bash
# Add cross-compilation targets for validation without native hardware
rustup target add aarch64-apple-darwin x86_64-unknown-linux-gnu

# Check compilation (no linking) for each target
cargo check --target aarch64-apple-darwin
cargo check --target x86_64-unknown-linux-gnu
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Hardcoded Windows shell (pwsh/powershell) | Platform-aware default_shell() in glass_mux::platform | Phase 21 (helper exists, not wired in) | Enables macOS/Linux PTY spawn |
| Hardcoded "Consolas" font default | Platform-aware defaults needed | Phase 22 (this phase) | Correct rendering on macOS/Linux without config |
| PowerShell-only shell injection | All-shell injection needed | Phase 22 (this phase) | Block decorations work on all platforms |
| Unconditional windows-sys dep | cfg(windows) gating needed | Phase 22 (this phase) | Clean cross-platform compilation |
| No CI pipeline | GitHub Actions cross-platform matrix | Phase 22 (this phase) | Prevents cross-platform regressions |

## Open Questions

1. **ScaleFactorChanged handling**
   - What we know: Scale factor is read at window creation in `resumed()`. No `WindowEvent::ScaleFactorChanged` handler exists.
   - What's unclear: Whether this causes real issues or if winit handles it transparently on resize.
   - Recommendation: Add ScaleFactorChanged handling if testing reveals issues. At minimum, log when it fires.

2. **glass.zsh pipe capture**
   - What we know: glass.bash has full pipe capture via tee-rewriting. glass.zsh only has basic OSC 133 hooks.
   - What's unclear: Whether pipe capture for zsh is expected in Phase 22 scope or deferred.
   - Recommendation: Defer pipe capture for zsh. Basic shell integration (blocks, CWD, exit codes) is sufficient for validation. Pipe capture is a feature enhancement.

3. **spawn_pty dependency on glass_mux**
   - What we know: `glass_terminal::spawn_pty()` has inline shell detection. `glass_mux::platform::default_shell()` exists.
   - What's unclear: Whether glass_terminal should depend on glass_mux (creates circular potential) or if the shell name should be passed from main.rs.
   - Recommendation: Pass the effective shell name from main.rs to spawn_pty via the existing `shell_override` parameter. Resolve the shell name in main.rs using `config.shell.unwrap_or(glass_mux::platform::default_shell())`. No new crate dependency needed.

4. **wgpu GL fallback on Linux VMs**
   - What we know: CI Linux runners don't have GPUs. wgpu Vulkan requires drivers.
   - What's unclear: Whether wgpu GL fallback with mesa llvmpipe works in CI.
   - Recommendation: CI should run `cargo build` and `cargo test` (unit tests don't need GPU). Rendering validation is manual on real hardware.

5. **config.toml path migration**
   - What we know: Config loads from `~/.glass/config.toml` (hardcoded). Platform convention is `~/Library/Application Support/glass/` (macOS) or `$XDG_CONFIG_HOME/glass/` (Linux).
   - What's unclear: Whether to migrate to platform paths now or keep `~/.glass/` for simplicity.
   - Recommendation: Keep `~/.glass/config.toml` as primary for now. The `glass_mux::platform::config_dir()` and `data_dir()` helpers exist but are not yet wired in. Defer path migration to a packaging/polish milestone to avoid breaking existing Windows users. Add platform paths as secondary lookup.

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust built-in test (`#[cfg(test)]` + `cargo test`) |
| Config file | None (uses Cargo.toml test config) |
| Quick run command | `cargo test --workspace` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map

Phase 22 has no explicit requirement IDs. Requirements are derived from GOAL.md:

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| P22-01 | Glass compiles on macOS (aarch64-apple-darwin) | compilation | `cargo check --target aarch64-apple-darwin` | N/A (cargo built-in) |
| P22-02 | Glass compiles on Linux (x86_64-unknown-linux-gnu) | compilation | `cargo check --target x86_64-unknown-linux-gnu` | N/A (cargo built-in) |
| P22-03 | spawn_pty uses platform default shell | unit | `cargo test -p glass_mux -- platform::default_shell` | Exists (Phase 21) |
| P22-04 | Shell integration injection works for all shells | smoke | Manual test -- source glass.zsh/glass.bash | N/A |
| P22-05 | Platform-aware font defaults | unit | `cargo test -p glass_core -- config` | Needs update |
| P22-06 | windows-sys gated behind cfg(windows) | compilation | `cargo check --target x86_64-unknown-linux-gnu` | N/A |
| P22-07 | wgpu surface format logged | smoke | Manual -- check log output on each platform | N/A |
| P22-08 | Shell integration scripts exist for zsh and bash | smoke | `test -f shell-integration/glass.zsh && test -f shell-integration/glass.bash` | Exists |
| P22-09 | CI cross-platform matrix passes | CI | GitHub Actions workflow | Wave 0 |
| P22-10 | notify/file watcher compiles on all platforms | compilation | Part of P22-01/P22-02 | N/A |

### Sampling Rate

- **Per task commit:** `cargo check --target aarch64-apple-darwin && cargo check --target x86_64-unknown-linux-gnu && cargo test --workspace`
- **Per wave merge:** `cargo test --workspace` + CI green on all platforms
- **Phase gate:** Cross-compilation check passes for all three targets. CI matrix green. Manual validation that Glass launches on macOS and Linux (if hardware available).

### Wave 0 Gaps

- [ ] `.github/workflows/ci.yml` -- CI workflow does not exist
- [ ] Cross-compilation targets may not be installed (`rustup target add` needed)
- [ ] `crates/glass_core/src/config.rs` tests assume "Consolas" default -- need cfg-gated assertions

## Sources

### Primary (HIGH confidence)

- Glass source code analysis: `src/main.rs` (shell integration injection at lines 268-281, find_shell_integration at lines 1251-1280)
- Glass source code analysis: `crates/glass_terminal/src/pty.rs` (spawn_pty shell detection at lines 115-121)
- Glass source code analysis: `crates/glass_core/src/config.rs` (hardcoded "Consolas" default at line 94)
- Glass source code analysis: `crates/glass_mux/src/platform.rs` (platform helpers already implemented)
- Glass source code analysis: `crates/glass_renderer/src/surface.rs` (wgpu backend selection, format at line 55)
- Glass source code analysis: `Cargo.toml` (unconditional windows-sys at line 74)
- Phase 21 research and summaries (architecture, SessionMux, platform patterns)
- `.planning/research/PITFALLS.md` -- 14 pitfalls catalogued for cross-platform domain
- `.planning/research/STACK.md` -- dependency cross-platform compatibility matrix

### Secondary (MEDIUM confidence)

- `.planning/research/ARCHITECTURE.md` -- target v2.0 architecture with platform patterns
- wgpu 28.0 backend auto-selection behavior (from stack research)
- winit 0.30.13 ModifiersState API (from Phase 21 research)

## Metadata

**Confidence breakdown:**
- Compilation fixes: HIGH -- directly inspected code, issues are mechanical
- Shell integration: HIGH -- existing scripts and injection logic fully analyzed
- Font defaults: HIGH -- trivial cfg-gated change
- Surface format: MEDIUM -- behavior differences across backends are documented but untested on real hardware
- CI pipeline: HIGH -- standard GitHub Actions patterns for Rust projects
- HiDPI/scale: MEDIUM -- scale_factor read at creation but ScaleFactorChanged handler not analyzed in full render pipeline

**Research date:** 2026-03-06
**Valid until:** 2026-04-06 (stable domain, no rapidly changing dependencies)

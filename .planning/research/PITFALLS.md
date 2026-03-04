# Pitfalls Research

**Domain:** Rust GPU-accelerated terminal emulator (Windows-first, wgpu + alacritty_terminal + ConPTY)
**Researched:** 2026-03-04
**Confidence:** HIGH (ConPTY, wgpu resize, winit migration), MEDIUM (font rendering, shell integration), LOW (alacritty_terminal API stability guarantees)

---

## Critical Pitfalls

### Pitfall 1: ConPTY Rewrites Escape Sequences In Transit

**What goes wrong:**
ConPTY is not a transparent byte pipe. It parses incoming VT sequences, applies them to a Win32 screen buffer, then re-serializes that buffer back as VT sequences. This round-trip is lossy. Known concrete behaviors:
- `ESC[49m` (default background) and `ESC[39m` (default foreground) get translated to `ESC[m` (full reset), blowing away other attributes.
- Curly-underline sequences are stripped entirely.
- Control sequences in window title OSC payloads are left unfiltered in older ConPTY versions, causing downstream parse failures.
- Keyboard input sequences are also rewritten: `ESC[5D` (Ctrl+Left in some encodings) becomes `ESC[D` (plain left-arrow), breaking word navigation until `ENABLE_VIRTUAL_TERMINAL_INPUT` mode is activated.
- OSC sequences from the hosted process can arrive out-of-order in third-party terminals because ConPTY injects its own OSC codes (title changes, mode changes) into the same stream.

**Why it happens:**
ConPTY was designed as a compatibility shim for legacy Win32 console applications, not as a transparent VT proxy. The intermediate Win32 screen buffer introduces an irreversible normalization step. The rewriting is intentional for legacy compatibility, not a bug that will be fixed.

**How to avoid:**
- Enable `ENABLE_VIRTUAL_TERMINAL_INPUT` on the ConPTY input side immediately after creation so keyboard sequences pass through unmodified.
- Never assume the byte stream from ConPTY's output side is identical to what the child process wrote. Treat it as semantically equivalent but syntactically re-encoded.
- Do not rely on distinguishing `ESC[49m` from `ESC[m` for background-color-only resets — ConPTY will collapse them.
- For curly underlines and other modern sequences, document the limitation or route around ConPTY using WSL for those sessions.
- Use `portable-pty` (from WezTerm) rather than hand-rolling ConPTY calls — it handles the `ENABLE_VIRTUAL_TERMINAL_INPUT` flag and process lifecycle correctly on Windows.

**Warning signs:**
- Colors reset unexpectedly when only background should change.
- Word-jump keyboard shortcuts (Ctrl+Arrow) move one character instead of one word.
- Underline styling works on Linux/WSL but not native Windows shells.
- Window title flickers to unrelated values mid-output.

**Phase to address:** Phase 0 (scaffold) — test ConPTY output passthrough with a known escape-sequence fixture before building any rendering on top of it.

---

### Pitfall 2: alacritty_terminal Has No Stable Embedding API

**What goes wrong:**
The `alacritty_terminal` crate is published on crates.io but is the internal library of the Alacritty binary, not a versioned public API. Breaking changes ship with every Alacritty release (the crate is at version 0.25+ and tracks Alacritty's version number). There is no semver stability promise, no deprecation cycle, and no migration guide aimed at embedders. Fields, traits, and module paths change without announcement.

**Why it happens:**
Alacritty maintainers have explicitly stated the crate exists for their own binary and not for third-party use. The project ships it on crates.io as a byproduct of workspace structure, not as a supported library.

**How to avoid:**
- Pin the exact version in `Cargo.toml` (`= "0.25.x"`) rather than using `^` or `~` range specifiers.
- Treat `alacritty_terminal` as vendored source: read the code, do not expect API stability, and budget time for version-bump migrations.
- Isolate all `alacritty_terminal` types behind a `glass_terminal` crate boundary. The rest of the workspace should never import `alacritty_terminal` directly — only `glass_terminal` does. This makes future migrations to a different VTE backend (e.g., raw `vte` crate) a single-crate change.
- Monitor the `alacritty/alacritty` GitHub releases page before upgrading dependencies.

**Warning signs:**
- Cargo fails to compile after a `cargo update` due to type mismatches in `alacritty_terminal`.
- Methods referenced in integration code disappear without explanation in changelogs.

**Phase to address:** Phase 0 (scaffold) — establish the `glass_terminal` isolation wrapper before writing any VTE integration logic.

---

### Pitfall 3: wgpu Surface Resize Causes Flickering and Hangs on Windows

**What goes wrong:**
Resizing a wgpu surface on Windows with Vulkan or DX12 backends exhibits two documented failure modes:
1. **Visual artifacts**: white rectangles appear underneath the window during drag-resize with both backends.
2. **Application hangs**: calling `surface.configure()` during resize can block for 100–150ms on Vulkan, making the window unresponsive during fast drag operations.

**Why it happens:**
The wgpu surface resize path calls into swapchain recreation, which synchronizes with the GPU pipeline. On Windows, the DWM compositing window manager creates an impedance mismatch: the window has already been resized by the OS before wgpu has reconfigured the swapchain, leaving a gap where the old swapchain is invalid but the new one isn't ready.

**How to avoid:**
- Handle `wgpu::SurfaceError::Outdated` and `wgpu::SurfaceError::Lost` gracefully — these are not fatal, just signals to reconfigure.
- Debounce resize events: do not call `surface.configure()` on every resize event during a drag; coalesce into the next frame render.
- On Windows, prefer DX12 over Vulkan as the primary backend. DX12 has fewer resize hang incidents than Vulkan in the wgpu issue tracker. Let wgpu's default auto-selection choose DX12 first on Windows — do not force Vulkan.
- Implement a minimum redraw interval (one frame at 60fps = ~16ms) so resize events that arrive faster than the GPU can reconfigure are dropped.
- Keep a "last known good size" and skip rendering frames where surface configuration fails, rendering nothing rather than crashing.

**Warning signs:**
- White flash during window drag-resize in development.
- Frame time spikes to >100ms when the user resizes the window.
- Panic at `surface.get_current_texture()` returning `SurfaceError::Lost`.

**Phase to address:** Phase 0 (scaffold) — implement resize handling correctly from the first wgpu surface creation, before any terminal content is rendered.

---

### Pitfall 4: winit 0.30 ApplicationHandler API Is a Complete Rewrite

**What goes wrong:**
winit 0.30 replaced the closure-based `EventLoop::run(|event, target| {...})` pattern with a trait-based `ApplicationHandler` that must be implemented on application state. All tutorials, examples, and Stack Overflow answers prior to 2024 use the old API, which no longer compiles. Additionally, `WindowBuilder` is deprecated. Window creation must now happen exclusively inside `can_create_surfaces()` / `resumed()` callbacks for portability with Android — creating a window before this callback is called panics on some platforms.

**Why it happens:**
The API redesign (tracking issue #3367) was driven by Android lifecycle requirements, where surfaces can be created and destroyed mid-session. The new model correctly represents this lifecycle, but the migration is non-trivial and all prior examples are broken.

**How to avoid:**
- Use the winit changelog (`docs.rs/winit/latest/winit/changelog/`) as the authoritative migration reference, not tutorials.
- Implement `ApplicationHandler` on a newtype that holds all application state (PTY handle, wgpu device, window handle, etc.).
- Create the `wgpu::Surface` only inside `can_create_surfaces()` — not in `main()`.
- Store `Arc<Window>` in application state to avoid lifetime fights with `wgpu::Surface<'static>`.

**Warning signs:**
- Compile errors mentioning `EventLoop::run` signature mismatch or `WindowBuilder` deprecation.
- Panic on startup with "window created outside of can_create_surfaces".
- Examples from 2023 docs refuse to compile.

**Phase to address:** Phase 0 (scaffold) — the winit initialization pattern must be correct before anything else can be built.

---

### Pitfall 5: PTY Reader Thread Blocking the Render Thread

**What goes wrong:**
Reading from a PTY (via `portable-pty` or raw ConPTY handles) is a blocking operation. If PTY reads happen on the main/render thread, the UI freezes whenever the shell produces no output (waiting for next output) or produces a burst (processing backlog). The frame rate drops to zero during blocking reads.

**Why it happens:**
Developers new to terminal architecture put PTY I/O in the event loop for simplicity. This works in demos (constant output) but fails in real usage where the shell is often idle.

**How to avoid:**
- Run PTY reads on a dedicated `std::thread::spawn`-ed thread (not a Tokio task — PTY reads are blocking and will stall the Tokio thread pool).
- The PTY reader thread sends parsed terminal state updates to the render thread via a `std::sync::mpsc` channel or a lock-free ring buffer.
- The render thread polls the channel during each frame tick and only redraws when new state is available.
- Keep the channel bounded (e.g., 16 entries) to apply back-pressure if the renderer falls behind.
- If using Tokio elsewhere in the codebase, use `tokio::task::spawn_blocking` for PTY reads, not `tokio::spawn`.

**Warning signs:**
- UI freezes when the shell prompt is idle.
- Frame rate drops to near zero when a command produces large output.
- Input feels sluggish because the event loop is blocked waiting for PTY bytes.

**Phase to address:** Phase 0 (scaffold) — establish the PTY-reader-thread + channel architecture from day one. Retrofitting threading after the render loop is built is painful.

---

### Pitfall 6: Shell Integration Sequences Fragile Under Prompt Customization

**What goes wrong:**
OSC 133 shell integration (FinalTerm marks: `A`=prompt start, `B`=command start, `C`=output start, `D`=command end + exit code) breaks when users have heavily customized prompts via Oh My Posh, Starship, or manual `$PROFILE` edits. Specific failure modes:
- A custom `prompt` function in PowerShell overwrites the hook if integration is injected via profile rather than wrapping the existing function.
- PSReadLine replaces the prompt machinery in PowerShell, so direct `$PROFILE` mutation races with PSReadLine's own prompt rendering.
- `OSC 133;D` with exit code must be sent before the prompt renders, not after — getting this order wrong causes incorrect exit-code attribution.
- Clearing the screen (`clear` / `cls`) causes marks to refer to stale positions, breaking command block detection.

**Why it happens:**
PowerShell's prompt system (unlike bash's `PROMPT_COMMAND`) requires wrapping the `prompt` function rather than appending to a pipeline. Developers who know bash patterns apply them incorrectly to PowerShell, and the integration appears to work in simple prompts but fails with third-party prompt frameworks.

**How to avoid:**
- In PowerShell, wrap the existing `prompt` function: save a reference to `$originalPrompt = Get-Item function:prompt`, then redefine `function prompt { <OSC 133;A>; & $originalPrompt; <OSC 133;B> }`.
- Send `OSC 133;D;<exitcode>` in a `$PSDefaultParameterValues` or PSReadLine `PreExecution` hook, not in the prompt function itself (which runs after execution).
- Document that Glass shell integration requires PowerShell 7+ (PSReadLine 2.x hooks are available).
- Treat screen-clear events (detecting `clear`/`cls` commands) as requiring a full prompt-block reset.
- Implement a fallback mode: if OSC 133 marks are not received within a timeout after shell startup, fall back to heuristic prompt detection (trailing `$`, `>`, `%` character patterns).

**Warning signs:**
- Command blocks don't appear in a session that uses Oh My Posh.
- Exit codes always show 0 regardless of actual command result.
- Prompt detection stops working after the user runs `clear`.
- Block boundaries are misaligned — one block contains output from multiple commands.

**Phase to address:** Phase 1 (shell integration) — test against both vanilla PowerShell and Oh My Posh before declaring shell integration complete.

---

### Pitfall 7: Font Rendering Atlas Overflow and Glyph Cache Thrashing

**What goes wrong:**
GPU text renderers (glyphon, wgpu-text) use texture atlases to cache rasterized glyphs. When the atlas fills up, renderers either:
1. Stop rendering new glyphs silently (glyphs disappear from output).
2. Flush and rebuild the entire atlas on every frame when too many unique glyphs are needed.

For terminals, this surfaces when:
- Users paste large amounts of text containing characters outside the ASCII range.
- The terminal scrollback contains many unique Unicode codepoints (CJK, combining marks, emoji).
- A command outputs colored emoji in large quantity.

**Why it happens:**
Default atlas sizes (e.g., 1024x1024) are sized for typical GUI text, not terminal scrollback that may contain thousands of unique glyphs. Terminals also render at high frequency compared to static UI, so atlas rebuilds are more costly.

**How to avoid:**
- Configure glyphon's `TextAtlas` with a larger initial size (2048x2048 or larger) for terminal use.
- Separate the atlas into two pools: one for monochrome glyphs (ASCII + common Unicode) and one for colored glyphs (emoji). This prevents colored glyphs from evicting monochrome text glyphs.
- For the on-screen grid (typically 80-220 columns), pre-warm the atlas with the full printable ASCII set at startup. This covers the common case with zero runtime allocation.
- Implement a least-recently-used eviction policy for the scrollback glyph set, not the on-screen set.

**Warning signs:**
- Characters disappear after heavy Unicode output.
- Frame time spikes during paste operations with non-ASCII content.
- Emoji render as blank squares after a long session.

**Phase to address:** Phase 1 (rendering pipeline) — size atlas correctly from the beginning. Growing it later requires rearchitecting the render pass.

---

### Pitfall 8: Wide Character (CJK/Emoji) Cell Width Misalignment

**What goes wrong:**
Unicode defines "wide" characters (East Asian full-width, CJK, most emoji) as occupying 2 terminal cells. If the terminal grid treats all characters as 1-cell wide:
- CJK text overwrites adjacent characters.
- The cursor position drifts right of the visible content.
- Line-wrap math is wrong, causing garbage output on lines with wide chars.
- The `unicode-width` crate (which alacritty_terminal uses internally) has its own opinionated table that disagrees with some shells' `wcwidth` implementation on "ambiguous-width" characters.

**Why it happens:**
The terminal grid allocation uses a `columns * rows` Vec of cells. If cell allocation doesn't account for wide characters spanning 2 slots with a "continuation" placeholder in the second slot, the grid corrupts.

**How to avoid:**
- Rely on `alacritty_terminal`'s built-in wide-char handling — it implements the placeholder cell model. Do not reimplement this in the renderer.
- In the renderer, when iterating grid cells, skip cells marked as `Flags::WIDE_CHAR_SPACER` (the second half of a wide character). Render wide chars at double the cell width.
- Test with `echo "日本語"` and `echo "🦀"` early and verify cursor position after each.
- Be aware that "ambiguous" Unicode characters (e.g., `★`, `©`) render as different widths in different terminal environments. Match the width used by the child shell's `wcwidth`.

**Warning signs:**
- Japanese/Chinese characters overwrite the character to their right.
- The cursor appears shifted right from where typing actually inserts characters.
- `vim` or other TUI apps display garbage when CJK characters are on screen.

**Phase to address:** Phase 1 (grid rendering) — add a CJK/emoji rendering test before declaring the VTE display working.

---

### Pitfall 9: Windows Code Page Encoding Corruption

**What goes wrong:**
On Windows, the console default code page is 437 (OEM US English) or locale-specific (e.g., 932 for Japanese Windows). If Glass spawns a PowerShell or bash process via ConPTY without setting code page 65001 (UTF-8), non-ASCII output is double-encoded: the shell outputs UTF-8, ConPTY interprets it as the system code page, and the terminal displays mojibake.

**Why it happens:**
ConPTY inherits the process code page of its parent. If the parent (Glass) hasn't set UTF-8 as the code page before calling ConPTY creation, the entire pipeline operates in the wrong encoding.

**How to avoid:**
- Call `SetConsoleCP(65001)` and `SetConsoleOutputCP(65001)` in Glass's Windows startup code before creating any ConPTY instance. In Rust, use the `windows` crate: `unsafe { windows::Win32::System::Console::SetConsoleCP(65001); SetConsoleOutputCP(65001); }`.
- Pass `chcp 65001` as a pre-command in the shell launch args, or set `PYTHONIOENCODING=utf-8` and similar env vars for child processes.
- Verify with `portable-pty`'s `CommandBuilder` that the spawned process inherits the UTF-8 code page.

**Warning signs:**
- Non-ASCII filenames display as `?` or garbled bytes.
- `git log` commit messages with accented characters show garbage.
- Japanese or emoji output from shell commands renders incorrectly.

**Phase to address:** Phase 0 (scaffold) — set code page as the first thing in `main()` before any window or PTY creation.

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Import `alacritty_terminal` directly from multiple crates in the workspace | Faster access to terminal state | Version bump breaks all crates simultaneously; cannot swap VTE backend | Never — always isolate behind `glass_terminal` |
| Perform PTY reads on the render thread | Simpler code, no channels needed | UI freezes on idle shell; unresponsive during output bursts | Never |
| Skip atlas size configuration and use glyphon defaults | Works in demos | Atlas overflow silently drops glyphs in production use | Never |
| Hard-code DX12 backend selection in wgpu `InstanceDescriptor` | Avoids Vulkan resize hangs | Fails on systems without DX12 (pre-Win10-1709) | Acceptable for Milestone 1, revisit for cross-platform |
| Implement shell integration only for vanilla prompts | Passes basic tests | Breaks for all users with Oh My Posh / Starship | Only acceptable as Phase 1 MVP, must fix before daily-driver |
| Pre-allocate full scrollback buffer at startup | Simple code | 191MB idle memory for 10k scrollback (documented Alacritty issue) | Never — use lazy/partial allocation |

---

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| ConPTY via `portable-pty` | Not enabling `ENABLE_VIRTUAL_TERMINAL_INPUT`, causing keyboard sequence rewrites | `portable-pty` handles this flag — verify it is set in WezTerm source before relying on it |
| ConPTY output stream | Assuming raw output matches what the child wrote | Treat ConPTY output as re-encoded VT; test with escape sequence fixtures |
| `alacritty_terminal` event handler | Implementing `EventListener` on a type that crosses thread boundaries without `Send` | Use `Arc<Mutex<...>>` for shared state or an `mpsc` sender in the event handler |
| glyphon `TextAtlas` | Creating one global atlas shared across multiple render passes | Create a dedicated atlas for terminal cell text, separate from any UI overlay text |
| winit `ApplicationHandler` | Calling `window.request_redraw()` from outside the event loop without a `EventLoopProxy` | Store an `EventLoopProxy` during `new()` and send a user event to trigger redraws from the PTY reader thread |
| PowerShell integration | Appending to `PROMPT_COMMAND` (a bash concept) | PowerShell requires wrapping the `prompt` function and using PSReadLine hooks |
| Windows DPI | Creating wgpu surface at logical pixels, rendering text at physical pixels | Use `window.scale_factor()` to convert logical to physical coordinates; pass physical size to `surface.configure()` |

---

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Re-shaping text on every frame | CPU pegged at high utilization during idle terminal | Only shape text when the grid cell content changes; cache shaped runs per grid line | Immediately visible with any animation or cursor blink |
| Uploading full glyph atlas to GPU every frame | GPU memory bandwidth saturated, frame times >16ms | Only upload atlas texture regions that changed (dirty rect tracking) | During any text output or paste |
| Scrollback grid as a `Vec<Vec<Cell>>` (row of rows) | Memory allocation on every new line; poor cache locality | Use alacritty_terminal's `Storage` type (ring buffer) — do not reimplement | At 10k+ lines of scrollback |
| Sending full terminal state snapshot across mpsc channel | Channel congestion during large output bursts | Send a "dirty" notification only; renderer reads from shared state under a lock | When a command produces >1MB output (e.g., `cat large_file`) |
| Re-rendering all cells every frame | Unnecessary GPU work during idle | Implement a "dirty" flag per grid row; only upload and render changed rows | At screen sizes above ~120 columns |

---

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| Executing shell integration script content received via OSC sequences without sanitization | A malicious program could inject PowerShell commands via crafted OSC output | Shell integration scripts are static, loaded from Glass's own files — never execute OSC sequence payloads as code |
| Passing unsanitized CWD from `OSC 9;9` sequence into filesystem APIs | Path traversal if the sequence contains `../` sequences | Validate CWD paths are absolute and within expected filesystem boundaries before using for display |
| Storing command history (Phase 2) with full environment variable values | Secrets in env vars (API keys, tokens) captured in history DB | In Phase 2, implement a blocklist for env var names matching `*_TOKEN`, `*_SECRET`, `*_KEY`, `*_PASSWORD` |

---

## UX Pitfalls

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| Cursor blink implemented as full-frame redraw | Excessive GPU work causing fan spin during idle | Blink only the cursor cell region; do not re-render the full grid |
| No visual feedback during ConPTY process spawn | User thinks app is frozen during 200-500ms startup | Show a loading cursor or splash during PTY initialization |
| Shell integration failure is silent | Block UI appears broken with no explanation | If OSC 133 marks aren't received within 5s, show a status bar indicator: "Shell integration inactive" |
| Font fallback silently renders tofu boxes | User sees blank squares for emoji without knowing why | Log a warning when font fallback fails; show the Unicode codepoint in the missing glyph box |
| DPI change (moving window between monitors) not handled | Text becomes blurry or oversized on the new monitor | Handle winit `ScaleFactorChanged` event; reconfigure surface and re-render at new DPI |

---

## "Looks Done But Isn't" Checklist

- [ ] **ConPTY output passthrough**: Verify `ENABLE_VIRTUAL_TERMINAL_INPUT` is set — test that Ctrl+Left sends `ESC[1;5D` not `ESC[D` to the shell.
- [ ] **wgpu resize**: Drag-resize the window rapidly for 5 seconds — verify no hangs, no white rectangles, and no panics.
- [ ] **PTY thread isolation**: Idle at the shell prompt for 10 seconds — verify frame rate stays at target FPS (cursor blink should be smooth, not stuttery).
- [ ] **Shell integration exit codes**: Run `false; true` and verify the `false` block shows a non-zero exit code and `true` shows zero.
- [ ] **Wide character rendering**: Run `echo "日本語🦀"` and verify cursor lands in the correct column after the string.
- [ ] **UTF-8 encoding**: Run a command that outputs a UTF-8 accented character (e.g., `Write-Output "café"` in PowerShell) — verify no mojibake.
- [ ] **Atlas overflow**: Paste a large block of CJK text (>500 unique characters) — verify no glyphs disappear.
- [ ] **Shell integration with Oh My Posh**: Install Oh My Posh in the test environment and verify command blocks still appear correctly.
- [ ] **DPI handling**: Move the Glass window between a 1x and 2x DPI monitor — verify text remains sharp.
- [ ] **Cold start time**: Measure from process launch to first rendered prompt — must be <200ms per requirements.

---

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| ConPTY escape sequence rewriting discovered after rendering is built | MEDIUM | Add a ConPTY output normalization layer in `glass_terminal`; re-test all color and keyboard scenarios |
| `alacritty_terminal` API breaks on version update | MEDIUM | All breakage is contained in `glass_terminal` crate; update the crate boundary adapter only |
| wgpu surface resize implemented incorrectly (crashing) | LOW | The resize handler is a small, isolated function; replace with debounce + error-recovery pattern |
| winit 0.30 `ApplicationHandler` not implemented correctly | HIGH | Requires restructuring all application state ownership; do it right in Phase 0 |
| Shell integration not working with Oh My Posh | LOW | The prompt wrapping logic is a standalone PowerShell script; iterate independently |
| Atlas overflow causing glyph drops | MEDIUM | Rebuild atlas with larger dimensions; requires a render pipeline change but not an architecture change |
| PTY reader on render thread (freeze bug) | HIGH | Requires introducing a dedicated thread + channel and restructuring the event loop; very disruptive post-scaffold |

---

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| ConPTY escape sequence rewriting | Phase 0: scaffold + PTY setup | Escape sequence fixture test: send known sequences, verify received bytes match expected |
| `alacritty_terminal` API instability | Phase 0: workspace structure | `glass_terminal` crate has zero public `alacritty_terminal` types in its API |
| wgpu surface resize flickering/hang | Phase 0: wgpu surface initialization | Drag-resize stress test: 5 seconds of continuous resize, no freeze or panic |
| winit 0.30 ApplicationHandler API | Phase 0: winit event loop | Compiles without deprecated API warnings; window creation inside `can_create_surfaces()` |
| PTY reader blocking render thread | Phase 0: threading architecture | Frame rate measurement at idle shell: target FPS maintained, no stuttering |
| Shell integration fragility | Phase 1: shell integration | Test with vanilla PowerShell 7 AND Oh My Posh; exit codes correct in both |
| Font atlas overflow | Phase 1: rendering pipeline | Paste 1000-char CJK block; all characters visible, no glyph drops |
| Wide character cell misalignment | Phase 1: grid rendering | `echo "日本語🦀"` cursor position verification test |
| Windows UTF-8 code page | Phase 0: process startup | Non-ASCII output test before any other PTY work |
| Glyph sub-pixel kerning (cosmetic) | Phase 1: font rendering | Visual inspection of rendered ASCII text at multiple font sizes |

---

## Sources

- [ConPTY modifies escape sequences passed to process input — microsoft/terminal #12166](https://github.com/microsoft/terminal/issues/12166)
- [ConPTY translating \[49m to \[m escape sequence — microsoft/terminal #362](https://github.com/microsoft/terminal/issues/362)
- [OSC escape sequences received out-of-order in 3rd-party terminals — microsoft/terminal #17314](https://github.com/microsoft/terminal/issues/17314)
- [Taming Windows Terminal's win32-input-mode in Go ConPTY Applications — DEV Community](https://dev.to/andylbrummer/taming-windows-terminals-win32-input-mode-in-go-conpty-applications-7gg)
- [Windows Command-Line: Introducing the Windows Pseudo Console (ConPTY)](https://devblogs.microsoft.com/commandline/windows-command-line-introducing-the-windows-pseudo-console-conpty/)
- [Shell integration in the Windows Terminal — Microsoft Learn](https://learn.microsoft.com/en-us/windows/terminal/tutorials/shell-integration)
- [Shell integration in the Windows Terminal — Microsoft DevBlogs](https://devblogs.microsoft.com/commandline/shell-integration-in-the-windows-terminal/)
- [Does portable-pty always utf-8? — wezterm/wezterm Discussion #2463](https://github.com/wezterm/wezterm/discussions/2463)
- [Using portable_pty causes terminal to be cleared while trying to stream to stdout — wezterm/wezterm #4784](https://github.com/wezterm/wezterm/issues/4784)
- [Backend selection is not always Vulkan > Metal > DX12 — gfx-rs/wgpu #1416](https://github.com/gfx-rs/wgpu/issues/1416)
- [Window resizing lags with white rectangles on Windows — gfx-rs/wgpu #5374](https://github.com/gfx-rs/wgpu/issues/5374)
- [Surface::configure is not in sync with window surface resizing — gfx-rs/wgpu #7447](https://github.com/gfx-rs/wgpu/issues/7447)
- [wgpu vulkan backend hangs on resize on windows — emilk/egui #7718](https://github.com/emilk/egui/issues/7718)
- [winit 0.30 changelog — docs.rs](https://docs.rs/winit/latest/winit/changelog/index.html)
- [EventLoop 3.0 Changes — rust-windowing/winit #2900](https://github.com/rust-windowing/winit/issues/2900)
- [Warp: Adventures in Text Rendering: Kerning and Glyph Atlases](https://www.warp.dev/blog/adventures-text-rendering-kerning-glyph-atlases)
- [Incorrect glyph information for emojis on Windows 11 — pop-os/cosmic-text #210](https://github.com/pop-os/cosmic-text/issues/210)
- [Certain double-width unicode emoji characters are treated as single-width — alacritty/alacritty #6144](https://github.com/alacritty/alacritty/issues/6144)
- [Scrollback memory pre-allocation and optimization — alacritty/alacritty #1236](https://github.com/alacritty/alacritty/issues/1236)
- [Ambiguous width character in CJK environment — microsoft/terminal #370](https://github.com/microsoft/terminal/issues/370)
- [Atlas performance — microsoft/terminal Discussion #12811](https://github.com/microsoft/terminal/discussions/12811)
- [Fixing Mojibake from UTF-8 Tools in PowerShell on Windows — hy2k.dev (2025)](https://hy2k.dev/en/blog/2025/11-20-fix-powershell-mojibake-on-windows/)

---
*Pitfalls research for: Rust GPU-accelerated terminal emulator (Glass), Windows-first, wgpu + alacritty_terminal + ConPTY*
*Researched: 2026-03-04*

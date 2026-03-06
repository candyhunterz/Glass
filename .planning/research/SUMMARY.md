# Project Research Summary

**Project:** Glass v2.0 -- Cross-Platform & Tabs/Split Panes
**Domain:** GPU-accelerated terminal emulator (Rust, wgpu) expanding from Windows-only to macOS/Linux with multiplexed sessions
**Researched:** 2026-03-06
**Confidence:** HIGH

## Executive Summary

Glass v2.0 extends an existing, well-architected Windows terminal emulator (11 crates, 28,885 LOC, validated through v1.3) to macOS and Linux while adding tabs and split panes. The most important discovery from research is that the existing stack is already cross-platform -- every major dependency (alacritty_terminal, wgpu, winit, rusqlite, notify, arboard) compiles and runs on all three platforms with zero new crate dependencies beyond `uuid` for session identification. The cross-platform work is primarily cfg-gated code changes (shell detection, keyboard modifier mapping, config paths), not architectural rewrites.

The recommended approach is a four-phase build: (1) extract the single-session assumption into a SessionMux abstraction and add platform cfg gates, (2) validate cross-platform PTY/rendering/input on macOS and Linux, (3) implement tabs with a wgpu-rendered tab bar, (4) add split panes with binary tree layout and scissor-rect viewport rendering. This ordering is dictated by strict dependency chains -- tabs require session extraction, splits require tabs, and all UI work requires cross-platform rendering to be validated first.

The primary risks are behavioral differences between ConPTY and Unix forkpty (signal handling, process lifecycle, EOF semantics), wgpu backend surface format mismatches across Metal/Vulkan/DX12, and macOS keyboard modifier confusion (Cmd vs Ctrl). All three are well-understood problems with documented solutions from Alacritty and WezTerm. The highest-impact risk is zombie PTY processes from improper session cleanup in the multi-session model -- this demands a Session struct with explicit Drop-based lifecycle management designed from Phase 1.

## Key Findings

### Recommended Stack

The existing Glass stack requires exactly one new dependency: `uuid 1.x` for session IDs. Every other crate already supports all target platforms. The key insight is that `alacritty_terminal 0.25.1` already provides cross-platform PTY via its `tty` module -- there is no need for `portable-pty` or any other PTY crate. Shell detection, keyboard mapping, and config path logic are the only platform-conditional code needed.

**Core technologies (unchanged):**
- **alacritty_terminal 0.25.1**: PTY abstraction -- already handles ConPTY (Windows), forkpty (macOS/Linux) behind `tty::new()`
- **wgpu 28.0.0**: GPU rendering -- Metal (macOS), Vulkan (Linux), DX12 (Windows) auto-selected via `Backends::all()`
- **winit 0.30.13**: Windowing -- Cocoa (macOS), X11+Wayland (Linux), Win32 (Windows)
- **glyphon 0.10.0**: Text rendering -- platform-agnostic, wgpu-based
- **rusqlite 0.38.0 (bundled)**: History/metadata storage -- bundled SQLite compiles everywhere

**New dependency:**
- **uuid 1.x**: Session IDs for tab/pane scoping of history, snapshots, and events (~20KB, pure Rust)

**Explicitly rejected:** portable-pty (redundant with alacritty_terminal), nix/libc (already transitive), crossterm (Glass renders via wgpu, not terminal), tauri/egui (tab bar is simple GPU quads+text), slab/slotmap (uuid sufficient for pane count).

### Expected Features

**Must have (table stakes):**
- macOS: Cmd key shortcuts (C/V/T/W/Q/N/1-9), Option-as-Meta, Retina/HiDPI, zsh shell integration, platform config paths
- Linux: Wayland + X11 support, Vulkan/GL backends, XDG directory compliance, Ctrl+Shift+C/V shortcuts
- Tabs: Tab bar with new/close/switch, keyboard shortcuts, per-tab independent PTY/state, tab titles from CWD
- Splits: Horizontal/vertical splits, keyboard create/navigate/resize, independent PTY per pane, focus indicator, dividers

**Should have (differentiators -- Glass's competitive advantage):**
- Per-pane block UI, command history, and undo (no competitor has this)
- Cross-pane/tab search (find a command across all sessions)
- New tab/pane inherits CWD from current pane
- Pane zoom toggle (tmux-style maximize)

**Defer (v2.x+):**
- Fish shell integration (P2), tab drag reorder (P2), mouse pane resize (P2), macOS .app bundle (P2)
- Detachable sessions, session save/restore, startup layout config, tab groups (v3+)

**Anti-features (explicitly avoid):** Tmux integration mode, native platform tab bars (NSTabView/GTK), per-pane shell picker GUI, unlimited layout nesting depth, session persistence across restarts.

### Architecture Approach

The architecture introduces one new crate (`glass_mux`) containing SessionMux, Session, SplitTree, Tab, and ViewportLayout. The core pattern is "shared renderer, independent sessions" -- one FrameRenderer/GlyphCache per window (FontSystem is expensive at ~35ms to create), but each pane owns its own PTY, Term, BlockManager, HistoryDb, and SnapshotStore. Rendering uses wgpu scissor rects to clip each session into its viewport sub-region within a single render pass. Events gain a SessionId field to route PTY thread output to the correct session. All platform-specific code uses `#[cfg(target_os)]` gates rather than trait abstractions -- the number of conditional points is small (~5 functions) and alacritty_terminal/wgpu/winit already abstract the hard parts.

**Major components:**
1. **SessionMux** (NEW) -- Tab/pane tree, focus tracking, session lifecycle, viewport layout computation
2. **Session** (NEW, extracted from WindowContext) -- Single terminal: PTY + Term + BlockManager + HistoryDb + SnapshotStore
3. **SplitTree** (NEW) -- Binary tree enum for recursive H/V splits with ratio, layout computation, directional navigation
4. **FrameRenderer** (MODIFIED) -- New `draw_frame_viewport()` method with scissor rect for pane-scoped rendering
5. **AppEvent** (MODIFIED) -- All PTY-originated variants gain SessionId for routing
6. **spawn_pty** (MODIFIED) -- cfg-gated shell detection (zsh on macOS, $SHELL on Linux, pwsh on Windows)

### Critical Pitfalls

1. **ConPTY vs forkpty behavioral differences** -- Signal handling (SIGWINCH/SIGHUP/SIGCHLD vs API calls), EOF semantics, and process group lifetime differ fundamentally. Prevention: keep using alacritty_terminal's abstraction, add Unix signal handlers for child reaping, add 5-second watchdog for PTY reader thread shutdown.

2. **wgpu surface format mismatches across backends** -- DX12 returns Bgra8UnormSrgb, Metal may return Bgra8Unorm (no sRGB), Vulkan varies. Prevention: explicitly negotiate texture format from `caps.formats` rather than blindly taking `[0]`. Test colors on all three platforms early.

3. **macOS Cmd vs Ctrl keyboard confusion** -- Cmd+C must copy (not SIGINT), Ctrl+C must send SIGINT. Prevention: build a `platform_action_modifier()` helper using winit's `meta_key()` on macOS, `control_key()` elsewhere. Cmd must never reach the PTY as a terminal escape.

4. **Zombie PTY processes from multi-session lifecycle** -- Each closed tab must clean up PTY process, reader thread, Term, DB connections. Prevention: Session struct with Drop impl that sends shutdown, kills child after timeout, joins thread with 2-second deadline.

5. **Shell integration script incompatibilities** -- zsh uses precmd/preexec (not PROMPT_COMMAND), fish uses entirely different event system, macOS ships bash 3.2 (no bash 4+ features). Prevention: write separate scripts per shell, use `add-zsh-hook` for zsh, test on macOS default zsh specifically.

## Implications for Roadmap

Based on research, suggested phase structure:

### Phase 1: Session Extraction and Platform Foundation
**Rationale:** Everything depends on Session being extracted from WindowContext and SessionMux being the routing layer. Cross-platform cfg gates must exist before any platform testing. This is the architectural foundation that unblocks all subsequent phases.
**Delivers:** glass_mux crate with Session/SessionMux/SplitTree structs, SessionId in AppEvent, refactored WindowContext using SessionMux in single-session mode, cfg-gated shell detection, platform config/data paths via dirs crate, shell integration scripts for zsh and bash (Linux).
**Addresses:** Platform PTY abstraction, Unix shell detection, platform config paths, independent PTY per pane (architecture only)
**Avoids:** Pitfall 1 (PTY semantic differences -- validate with per-platform tests), Pitfall 3 (Cmd/Ctrl mapping -- platform shortcut abstraction), Pitfall 5 (shell integration -- per-shell scripts), Pitfall 8 (config paths)
**Test gate:** Glass runs identically to v1.3 on Windows through the new SessionMux layer (zero user-visible change).

### Phase 2: Cross-Platform Validation
**Rationale:** Must validate rendering, input, and PTY on macOS/Linux before adding UI complexity (tabs/splits). Finding surface format bugs or keyboard issues after building a tab bar wastes effort.
**Delivers:** Glass launches and runs on macOS (Metal, Cmd shortcuts, zsh, Retina) and Linux (Vulkan/GL, Wayland+X11, XDG paths). Cross-platform CI pipeline.
**Addresses:** wgpu backend auto-selection, macOS Cmd key mappings, HiDPI/Retina rendering, Wayland+X11 support, Option-as-Meta, cross-platform CI
**Avoids:** Pitfall 2 (surface format mismatches -- test early), Pitfall 6 (Wayland vs X11 clipboard/window), Pitfall 9 (notify behavior differences), Pitfall 11 (CI matrix cost -- Linux-heavy strategy)
**Test gate:** Glass runs on all three platforms with correct rendering, keyboard, clipboard, shell integration, and file watching.

### Phase 3: Tabs
**Rationale:** Tabs are simpler than splits (no viewport subdivision) and validate the SessionMux design with real multi-session usage. Tab bar UI is a prerequisite for the split pane visual frame.
**Delivers:** wgpu-rendered tab bar, Ctrl+Shift+T/W new/close tab, Ctrl+Tab/Shift+Tab cycle, Ctrl+1-9 jump, per-tab independent PTY/Term/BlockManager/History, tab title from CWD/process.
**Addresses:** Tab bar UI, independent PTY per tab, per-pane block UI + history (differentiator), keyboard shortcuts for tab management
**Avoids:** Pitfall 4 (zombie PTY from tab close -- Session Drop impl), Pitfall integration gotcha (BlockManager per session, History DB session_id scoping)
**Test gate:** Create/close/switch 50 tabs rapidly with zero zombie processes, zero resource leaks, independent history per tab.

### Phase 4: Split Panes
**Rationale:** Splits require all prior infrastructure (session extraction, multi-session rendering, event routing, tab container). Binary tree layout builds on the SplitTree designed in Phase 1.
**Delivers:** Horizontal/vertical splits via keyboard, Alt+Arrow focus navigation, Alt+Shift+Arrow resize, pane dividers, focused pane border highlight, PTY resize on split, pane close with parent collapse.
**Addresses:** Split pane rendering, layout engine, pane focus switching, pane resize, independent PTY per pane
**Avoids:** Pitfall 10 (viewport off-by-one -- character-cell-first dimension calculation, wgpu scissor rects)
**Test gate:** Nested splits in both directions, correct resize cascading, no viewport gaps/overlaps, mouse click to focus.

### Phase Ordering Rationale

- Phase 1 before 2: Session extraction is a refactor on Windows (safe, testable) that creates the abstraction layer needed for multi-platform and multi-session work.
- Phase 2 before 3/4: Cross-platform must be validated before adding UI complexity. A surface format bug discovered during split pane development is 3x harder to debug than during single-session platform bring-up.
- Phase 3 before 4: Tabs validate SessionMux with real usage (spawn, route, cleanup) without the added complexity of viewport subdivision. Splits layer cleanly on top of working tabs.
- Shell integration scripts can be developed in parallel with any phase since they are standalone shell code.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 2 (Cross-Platform Validation):** Wayland-specific issues (clipboard persistence, CSD vs SSD, IME) are poorly documented and need hands-on testing. macOS App Nap, NSWindow tabbingMode suppression, and fullscreen behavior need investigation.
- **Phase 4 (Split Panes):** Viewport scissor-rect rendering with glyphon text is not well-documented. May need to prototype the draw_frame_viewport approach early to validate the scissor clipping works correctly with wgpu's text rendering pipeline.

Phases with standard patterns (skip research-phase):
- **Phase 1 (Session Extraction):** Pure refactoring of existing code into new structs. Well-understood Rust patterns (extract struct, add indirection layer). WezTerm's mux architecture provides a validated reference.
- **Phase 3 (Tabs):** Tab bar rendering is straightforward (colored rectangles + text with glyphon). Tab management is a Vec with an index. Standard patterns from every terminal emulator.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | All existing deps verified cross-platform on docs.rs. Only one new dep (uuid). alacritty_terminal cross-platform PTY verified against source. |
| Features | HIGH | Feature landscape derived from 5 competitor terminals (Alacritty, Ghostty, Kitty, WezTerm, Windows Terminal) with official documentation. |
| Architecture | HIGH | SessionMux pattern validated against WezTerm's Mux architecture. Binary tree splits are the industry standard (WezTerm, tmux). Renderer viewport approach uses standard wgpu scissor rects. |
| Pitfalls | HIGH | Pitfalls verified against Glass source code, wgpu issue tracker, and real-world bug reports from similar projects. Phase-specific warnings are concrete and actionable. |

**Overall confidence:** HIGH

### Gaps to Address

- **Wayland clipboard persistence:** arboard handles basic Wayland clipboard but data is lost when Glass exits. May need `wl-clip-persist` integration or a background clipboard thread. Validate during Phase 2.
- **IME support on Linux:** winit's IME event handling on Wayland (text-input-v3) is not well-documented. CJK input may not work without explicit event forwarding. Needs hands-on testing.
- **macOS .app bundle and notarization:** Required for real distribution but deferred to post-v2.0. Needs Apple Developer Account ($99/year) and CI pipeline setup. Budget 1-2 days when addressed.
- **Fish shell integration:** Deferred to v2.x. Fish syntax is completely unlike bash/zsh and requires a separate script. Not blocking for launch since zsh (macOS) and bash (Linux) cover the majority.
- **Thread scaling at 20+ sessions:** Each PTY gets a std::thread for its reader loop. At 20+ simultaneous sessions, consider async PTY I/O or a thread pool. Not a v2.0 concern but a future scalability consideration.
- **HiDPI scale factor plumbing:** winit provides `scale_factor()` and glyphon needs it for glyph sizing. The exact integration path through Glass's font/rendering pipeline needs validation during Phase 2.

## Sources

### Primary (HIGH confidence)
- [alacritty_terminal::tty docs (docs.rs)](https://docs.rs/alacritty_terminal/0.25.1/alacritty_terminal/tty/index.html) -- cross-platform PTY module
- [alacritty_terminal Cargo.toml (GitHub)](https://github.com/alacritty/alacritty/blob/master/alacritty_terminal/Cargo.toml) -- platform-specific deps
- [winit ModifiersState docs](https://rust-windowing.github.io/winit/winit/keyboard/struct.ModifiersState.html) -- META/meta_key() for Cmd on macOS
- [WezTerm Multiplexer Architecture (DeepWiki)](https://deepwiki.com/wezterm/wezterm/2.2-multiplexer-architecture) -- tab/split binary tree pattern
- [wgpu Backends documentation (docs.rs)](https://docs.rs/wgpu/latest/wgpu/struct.Backends.html) -- backend auto-selection
- Glass source code analysis -- pty.rs, surface.rs, input.rs, main.rs

### Secondary (MEDIUM confidence)
- [Cross-Platform Rust Graphics with wgpu (BrightCoding)](https://www.blog.brightcoding.dev/2025/09/30/cross-platform-rust-graphics-with-wgpu-one-api-to-rule-vulkan-metal-d3d12-opengl-webgpu/) -- wgpu cross-platform patterns
- [Ghostty features and keybind reference](https://ghostty.org/docs/features) -- competitor analysis
- [Kitty layouts and overview](https://sw.kovidgoyal.net/kitty/overview/) -- competitor analysis
- [macOS Code Signing guides (multiple sources)](https://gist.github.com/rsms/929c9c2fec231f0cf843a1a746a416f5) -- notarization pipeline
- [wgpu Metal shader issues #4456, #4399](https://github.com/gfx-rs/wgpu/issues/4456) -- known Metal compilation edge cases

### Tertiary (LOW confidence)
- [Wayland vs X11 in 2025 (dasroot.net)](https://dasroot.net/posts/2025/11/wayland-vs-x11/) -- display server ecosystem state (opinionated)
- Thread scaling recommendations for 20+ PTY sessions -- based on general async Rust patterns, not terminal-specific benchmarks

---
*Research completed: 2026-03-06*
*Ready for roadmap: yes*

# Glass

## What This Is

Glass is a GPU-accelerated terminal emulator built in Rust that understands command structure. It renders each command's output as a visually distinct block with exit code, duration, and a status bar showing CWD and git branch. Shell integration scripts for Bash, Zsh, Fish, and PowerShell emit OSC 133/7 sequences that Glass parses into structured blocks. Every command is logged to a local SQLite database with FTS5 full-text search, and AI assistants can query terminal history and context through an MCP server over stdio. File-modifying commands are automatically snapshotted with one-keystroke undo (Ctrl+Shift+Z). Piped commands are transparently captured and displayed as multi-row pipeline blocks with inspectable intermediate stage output. Multiple terminal sessions run in tabs with a GPU-rendered tab bar, and tabs can be split horizontally or vertically into independent panes with a binary tree layout engine.

## Core Value

A terminal that looks and feels normal but passively watches, indexes, and snapshots everything -- surfacing intelligence only when you need it.

## Requirements

### Validated

- ✓ Rust workspace with 7 crates (glass_core, glass_terminal, glass_renderer, + 4 stubs) -- v1.0
- ✓ wgpu DX12 GPU-accelerated rendering with instanced quad pipeline -- v1.0
- ✓ ConPTY PTY spawn with dedicated reader thread and keyboard round-trip -- v1.0
- ✓ Full VTE rendering: 24-bit color, cursor shapes, font-metrics resize -- v1.0
- ✓ Keyboard escape encoding: Ctrl/Alt/Shift modifiers, arrow/function keys -- v1.0
- ✓ Clipboard copy/paste (Ctrl+Shift+C/V), bracketed paste, scrollback -- v1.0
- ✓ Window resize with terminal reflow, UTF-8 rendering -- v1.0
- ✓ OSC 133 command lifecycle parsing and OSC 7 CWD tracking -- v1.0
- ✓ PowerShell and Bash shell integration scripts (Oh My Posh/Starship compatible) -- v1.0
- ✓ Block UI: separator lines, exit code badges, duration labels -- v1.0
- ✓ Status bar: CWD display, git branch + dirty count -- v1.0
- ✓ TOML configuration: font family, font size, shell override -- v1.0
- ✓ Performance: 360ms cold start, 3-7us key latency, 86MB idle memory -- v1.0
- ✓ SQLite history database with FTS5 full-text search and per-project storage -- v1.1
- ✓ Command metadata logging (cwd, exit code, duration, output capture up to 50KB) -- v1.1
- ✓ Retention policies with configurable max age and max size, automatic pruning -- v1.1
- ✓ PTY output capture with alt-screen detection, binary filtering, ANSI stripping -- v1.1
- ✓ Block decoration scrollback (display_offset fix) -- v1.1
- ✓ CLI query interface: `glass history search/list` with combined filters -- v1.1
- ✓ Search overlay (Ctrl+Shift+F) with live incremental search and scroll-to-block -- v1.1
- ✓ MCP server (`glass mcp serve`) with GlassHistory and GlassContext tools over stdio -- v1.1
- ✓ Clap subcommand routing preserving default terminal launch -- v1.1
- ✓ Content-addressed blob store (BLAKE3) with deduplication for file snapshots -- v1.2
- ✓ Command parser identifying file targets for pre-exec snapshot (POSIX + PowerShell) -- v1.2
- ✓ FS watcher monitoring CWD during command execution with .glassignore -- v1.2
- ✓ Auto pre-exec snapshot on file-modifying commands (OSC 133;C triggered) -- v1.2
- ✓ Ctrl+Shift+Z undo restoring files to pre-command state -- v1.2
- ✓ Conflict detection warning if file modified since tracked command -- v1.2
- ✓ Confidence level display (pre-exec snapshot vs watcher-only) -- v1.2
- ✓ [undo] label on command blocks with visual feedback after undo -- v1.2
- ✓ CLI undo: `glass undo <command-id>` -- v1.2
- ✓ MCP tools: GlassUndo and GlassFileDiff for AI integration -- v1.2
- ✓ Storage pruning with configurable age/count limits and startup cleanup -- v1.2
- ✓ Snapshot configuration section in config.toml -- v1.2
- ✓ Pipe parsing with TTY detection, opt-out flag, and buffer sampling -- v1.3
- ✓ Shell capture via tee rewriting (bash) and Tee-Object (PowerShell) with OSC transport -- v1.3
- ✓ Multi-row pipeline UI with auto-expand, click/keyboard stage expansion -- v1.3
- ✓ pipe_stages DB table with schema migration and retention cascade -- v1.3
- ✓ GlassPipeInspect MCP tool and GlassContext pipeline stats -- v1.3
- ✓ [pipes] config section with enabled gate, max_capture_mb, auto_expand -- v1.3
- ✓ SessionMux multiplexer with Session struct and platform cfg-gated helpers -- v2.0
- ✓ SessionId newtype routing through AppEvent/EventProxy for multi-session dispatch -- v2.0
- ✓ Cross-platform compilation (Windows/macOS/Linux) with 3-platform CI matrix -- v2.0
- ✓ Platform-aware shell detection and font defaults -- v2.0
- ✓ Shell integration for bash, zsh, fish, and PowerShell with auto-injection -- v2.0
- ✓ Tab bar with GPU-rendered rects/text, Ctrl+Shift+T/W shortcuts, mouse click, CWD inheritance -- v2.0
- ✓ Binary tree split pane layout engine (SplitTree) with TDD (26 tests) -- v2.0
- ✓ Per-pane scissor-clipped rendering with viewport offsets, focus borders, dividers -- v2.0
- ✓ Split pane keyboard shortcuts (Ctrl+Shift+D/E), focus navigation (Alt+Arrow), resize (Alt+Shift+Arrow) -- v2.0
- ✓ Independent PTY/history/snapshot per tab and pane -- v2.0
- ✓ Pane-aware TerminalExit handler (close_pane vs close_tab based on pane count) -- v2.0

### Active

<!-- Current scope: v2.1 Packaging & Polish -->

- [ ] Platform installers (MSI, DMG, deb/rpm/AppImage/Flatpak)
- [ ] Auto-update mechanism
- [ ] Performance profiling and optimization pass
- [ ] Public documentation site and README
- [ ] Config validation and error reporting
- [ ] Config hot-reload

## Current Milestone: v2.1 Packaging & Polish

**Goal:** Production-ready distribution, performance tuning, config polish, and public documentation across all three platforms.

### Deferred (Future Milestones)

- Block collapse/expand, URL detection, block keyboard navigation -- UI polish
- Config hot reload -- deferred to Packaging & Polish milestone
- Blob compression with zstd -- storage optimization, not critical yet
- Diff view before undo -- undo enhancement
- Per-file partial undo from multi-file commands -- undo enhancement
- Undo/redo chain navigation -- undo enhancement
- File modification timeline queries -- history enhancement
- Multi-command batch undo -- undo enhancement
- macOS runtime validation (Metal backend, FSEvents, Cmd shortcuts) -- compiles but not runtime-tested
- Linux runtime validation (Vulkan/GL, inotify, Wayland+X11) -- compiles but not runtime-tested

### Out of Scope

- Built-in AI chat -- Glass exposes data *to* AI assistants via MCP, not an AI itself
- IDE features -- no file explorer, editor, or LSP integration
- Plugin system -- core features must be solid first
- Cloud sync -- history and snapshots stay local, no telemetry
- Theme marketplace -- one dark theme, one light theme for now
- cmd.exe shell support -- no shell integration hooks available
- Font ligatures -- requires HarfBuzz shaping pipeline
- Image protocols (Kitty, Sixel) -- separate rendering layer not needed yet
- FTS5 on output content -- defer until storage impact measured in practice
- Custom FTS5 tokenizer -- unicode61 default sufficient; revisit if search quality is poor
- MCP over network transport -- stdio sufficient for local AI; network adds security concerns
- Full directory tree snapshots -- storage explosion (node_modules = 500MB+)
- Process state undo -- killed processes, env changes, network effects are irreversible
- Undo for sudo/elevated commands -- security implications of writing to system paths
- Full shell command parser -- shell syntax is Turing-complete; heuristic whitelist instead

## Context

Shipped v2.0 with 17,868 LOC Rust across 12 crates (glass_core, glass_terminal, glass_renderer, glass_protocol, glass_config, glass_snapshot, glass_history, glass_pipes, glass_mcp, glass_mux + root binary).
Tech stack: wgpu 28.0 (DX12), winit 0.30.13, alacritty_terminal 0.25.1, glyphon 0.10.0, tokio 1.50.0, rusqlite 0.35.0, rmcp 1.1.0, blake3, notify 8.2, ignore 0.4, shlex, chrono 0.4.
Windows 11 primary -- ConPTY for PTY, DX12 for GPU rendering. Cross-compiles for macOS and Linux via CI.
Built across 5 milestones (25 phases, 60 plans) in 4 days. 436 tests passing.

Known tech debt:
- pruner.rs max_size_mb not enforced (count and age pruning work)
- PipeStage.is_tty vestigial after classify.rs removal
- default_shell_program() duplicated in pty.rs and platform.rs
- config_dir() and data_dir() exported but never consumed
- ScaleFactorChanged is log-only (no dynamic font metric recalculation)
- Nyquist validation partial across most phases

## Constraints

- **Tech stack**: Rust -- non-negotiable (performance, memory safety, cross-platform compilation)
- **VTE layer**: alacritty_terminal 0.25.1 (exact pin) -- battle-tested terminal emulation
- **Rendering**: wgpu with DX12 on Windows (auto-select on other platforms)
- **Performance**: <500ms cold start, <5ms input latency, <120MB idle memory
- **Polish**: Daily-drivable -- good enough to use as primary terminal

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Windows first | Developer is on Windows, build where you test | ✓ Good -- DX12 + ConPTY works well |
| Fork alacritty_terminal for VTE | Battle-tested since 2017, embeddable, Apache 2.0 | ✓ Good -- 0.25.1 worked with custom PTY read loop |
| wgpu DX12 forced backend | 33% faster than Vulkan on Windows | ✓ Good -- 360ms cold start |
| Instanced WGSL quad rendering | Simple, fast cell backgrounds without index buffer | ✓ Good -- clean GPU pipeline |
| Per-line cosmic_text::Buffer | Per-character fg color and font weight/style | ✓ Good -- flexible text rendering |
| Custom PTY read loop | Replaced alacritty PtyEventLoop for OscScanner pre-scanning | ✓ Good -- enables shell integration |
| PSReadLine Enter handler for 133;C | More reliable than PreExecution hook across versions | ✓ Good -- works with pwsh 7+ |
| Dedicated PTY reader thread | std::thread not Tokio -- blocking PTY I/O must not block async executor | ✓ Good -- clean separation |
| GridSnapshot lock-minimizing pattern | Minimize lock contention between PTY reader and renderer | ✓ Good -- no visible lag |
| Revised cold start <500ms | DX12 hardware init floor ~290ms unavoidable | ✓ Good -- realistic target met |
| Revised memory <120MB | GPU driver allocations ~80MB baseline | ✓ Good -- realistic target met |
| Content FTS5 tables (not external content) | Simpler, safer -- external content tables require manual sync | ✓ Good -- no sync bugs |
| FTS5 on command text only for v1.1 | Defer output indexing until storage impact measured | ✓ Good -- keeps index small |
| Option<Commands> clap pattern | Default-to-terminal when no subcommand given | ✓ Good -- clean UX |
| PRAGMA user_version for migrations | Simple, built-in schema versioning without migration framework | ✓ Good -- v0->v1 worked cleanly |
| Raw bytes via AppEvent for output | Avoids glass_terminal -> glass_history dependency | ✓ Good -- clean crate boundaries |
| Alt-screen detection via raw bytes | Scanning ESC[?1049h/l avoids locking terminal TermMode | ✓ Good -- no contention |
| rmcp SDK for MCP | Official Rust MCP SDK, handles JSON-RPC framing | ✓ Good -- clean integration |
| MCP as separate process | `glass mcp serve` not embedded in terminal process | ✓ Good -- isolation, testability |
| Epoch timestamp matching for scroll-to-block | Wall-clock match between DB records and Block structs | ✓ Good -- reliable navigation |
| Separate snapshots.db from history.db | Independent pruning, avoids migration risk | ✓ Good -- clean separation |
| Content-addressed blobs on filesystem | >100KB threshold from SQLite guidance; shard dirs for scalability | ✓ Good -- dedup works well |
| Dual mechanism (pre-exec parser + FS watcher) | Watcher is safety net for parser gaps | ✓ Good -- honest limitations |
| shlex for POSIX, custom for PowerShell | shlex battle-tested; PS uses backtick escaping | ✓ Good -- correct tokenization |
| One-shot undo (snapshot deleted after restore) | Simple V1 semantics; undo chain deferred | ✓ Good -- clear behavior |
| Config gating pre-exec only | FS watcher and undo handler always available | ✓ Good -- can undo existing snapshots even when creation disabled |
| GlassServer stores glass_dir not open store | Per-request store opening in spawn_blocking for thread safety | ✓ Good -- SnapshotStore is !Send |
| Whitespace splitting for pipe program extraction | shlex treats backslash as escape, mangles Windows paths | ✓ Good -- correct on all platforms |
| Backtick escape in pipe parser | PowerShell uses backtick not backslash for escaping | ✓ Good -- cross-shell compatibility |
| OSC 133;S/P protocol for pipe transport | Reuses existing OSC infrastructure, no new IPC | ✓ Good -- clean integration |
| Tee rewriting for bash, Tee-Object for PowerShell | Native shell primitives, no external binaries | ✓ Good -- reliable capture |
| Pipeline overlays (not grid rows) for stage rendering | Consistent with existing overlay architecture | ✓ Good -- no grid disruption |
| GLASS_PIPES_DISABLED env var for shell IPC | Shells can't read TOML config; env var is universal | ✓ Good -- clean three-layer gate |
| Separate pipe_stages DB table with FK cascade | Independent lifecycle from commands, clean pruning | ✓ Good -- schema v2 migration works |
| SessionMux as separate glass_mux crate | Clean boundary: session state vs terminal rendering | ✓ Good -- enables multi-session without touching glass_terminal |
| SessionId newtype with Copy/Hash | Cheap routing key, pattern-matched in event dispatch | ✓ Good -- zero-cost abstraction |
| Platform cfg-gating (windows/macos/unix) | Compile-time elimination of platform code | ✓ Good -- all 3 platforms compile |
| Binary tree for split layout | Recursive splits, natural sibling collapse on close | ✓ Good -- 26 TDD tests, clean API |
| Scissor-clip per-pane rendering | Reuse single FrameRenderer with viewport offset | ✓ Good -- no per-pane GPU pipeline needed |
| Tab owns SplitNode tree | Each tab has independent pane layout | ✓ Good -- clean ownership, no shared state |
| find_shell_integration() auto-injection | Source shell script into PTY at spawn time | ✓ Good -- works for bash/zsh/fish/pwsh |
| fish event handlers (not precmd/preexec) | fish uses fish_prompt/fish_preexec events natively | ✓ Good -- cooperates with Starship/Tide |

---
*Last updated: 2026-03-07 after v2.1 milestone started*

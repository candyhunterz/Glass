# Glass

## What This Is

Glass is a GPU-accelerated terminal emulator built in Rust that understands command structure. It renders each command's output as a visually distinct block with exit code, duration, and a status bar showing CWD and git branch. Shell integration scripts for PowerShell and Bash emit OSC 133/7 sequences that Glass parses into structured blocks. Every command is logged to a local SQLite database with FTS5 full-text search, and AI assistants can query terminal history and context through an MCP server over stdio.

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

### Active

## Current Milestone: v1.2 Command-Level Undo

**Goal:** Automatic filesystem snapshots per command with one-keystroke revert via Ctrl+Shift+Z.

**Target features:**
- Filesystem monitoring engine (ReadDirectoryChangesW on Windows)
- Pre-exec command parsing to identify file targets for snapshot
- Snapshot storage with content-addressed deduplication
- Ctrl+Shift+Z undo and [undo] button on command blocks
- CLI interface: `glass undo <command-id>`
- MCP tools: GlassUndo, GlassFileDiff
- Storage management and pruning
- File modification tracking integrated into history DB

**Approach:** Targeted pre-exec snapshot (parse command text for file arguments, snapshot before execution) + FS watcher for recording all modifications. Honest limitations: commands with unpredictable file targets (scripts, build tools) get recorded but may not be fully undoable.

#### Future

- [ ] Pipe visualization with intermediate stage output
- [ ] Block collapse/expand, URL detection, block keyboard navigation
- [ ] Config hot reload
- [ ] macOS and Linux support
- [ ] Tabs and split panes

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

## Context

Shipped v1.1 with 8,473 LOC Rust across 9 crates (glass_core, glass_terminal, glass_renderer, glass_protocol, glass_config, glass_snapshot, glass_history, glass_mcp + root binary).
Tech stack: wgpu 28.0 (DX12), winit 0.30.13, alacritty_terminal 0.25.1, glyphon 0.10.0, tokio 1.50.0, rusqlite 0.35.0, rmcp 1.1.0, chrono 0.4.
Windows 11 first -- ConPTY for PTY, DX12 for GPU rendering.
Built across 2 milestones (9 phases, 24 plans) in 2 days.

Known tech debt:
- Command text stored as empty string in history (grid extraction deferred)
- prune() never auto-triggered (retention policies exist as library code only)
- PTY throughput not benchmarked quantitatively
- Nyquist validation partial across all phases

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

---
*Last updated: 2026-03-05 after v1.2 milestone start*

# Glass

## What This Is

Glass is a GPU-accelerated terminal emulator built in Rust that understands command structure. It renders each command's output as a visually distinct block with exit code, duration, and a status bar showing CWD and git branch. Shell integration scripts for PowerShell and Bash emit OSC 133/7 sequences that Glass parses into structured blocks. Future milestones will add command-level undo with filesystem snapshots, visual pipe debugging, and structured scrollback as a queryable database exposed to AI assistants via MCP.

## Core Value

A terminal that looks and feels normal but passively watches, indexes, and snapshots everything — surfacing intelligence only when you need it.

## Requirements

### Validated

- ✓ Rust workspace with 7 crates (glass_core, glass_terminal, glass_renderer, + 4 stubs) — v1.0
- ✓ wgpu DX12 GPU-accelerated rendering with instanced quad pipeline — v1.0
- ✓ ConPTY PTY spawn with dedicated reader thread and keyboard round-trip — v1.0
- ✓ Full VTE rendering: 24-bit color, cursor shapes, font-metrics resize — v1.0
- ✓ Keyboard escape encoding: Ctrl/Alt/Shift modifiers, arrow/function keys — v1.0
- ✓ Clipboard copy/paste (Ctrl+Shift+C/V), bracketed paste, scrollback — v1.0
- ✓ Window resize with terminal reflow, UTF-8 rendering — v1.0
- ✓ OSC 133 command lifecycle parsing and OSC 7 CWD tracking — v1.0
- ✓ PowerShell and Bash shell integration scripts (Oh My Posh/Starship compatible) — v1.0
- ✓ Block UI: separator lines, exit code badges, duration labels — v1.0
- ✓ Status bar: CWD display, git branch + dirty count — v1.0
- ✓ TOML configuration: font family, font size, shell override — v1.0
- ✓ Performance: 360ms cold start, 3-7us key latency, 86MB idle memory — v1.0

### Active

- [ ] History database with searchable scrollback (Ctrl+Shift+F overlay)
- [ ] MCP server for AI assistant integration (GlassHistory, GlassContext tools)
- [ ] Command-level undo with filesystem snapshots
- [ ] Pipe visualization with intermediate stage output
- [ ] Block collapse/expand, URL detection, block keyboard navigation
- [ ] Config hot reload
- [ ] macOS and Linux support
- [ ] Tabs and split panes

### Out of Scope

- Built-in AI chat — Glass exposes data *to* AI assistants via MCP, not an AI itself
- IDE features — no file explorer, editor, or LSP integration
- Plugin system — core features must be solid first
- Cloud sync — history and snapshots stay local, no telemetry
- Theme marketplace — one dark theme, one light theme for now
- cmd.exe shell support — no shell integration hooks available
- Font ligatures — requires HarfBuzz shaping pipeline
- Image protocols (Kitty, Sixel) — separate rendering layer not needed yet

## Context

Shipped v1.0 with 4,343 LOC Rust across 7 crates.
Tech stack: wgpu 28.0 (DX12), winit 0.30.13, alacritty_terminal 0.25.1, glyphon 0.10.0, tokio 1.50.0.
Windows 11 first — ConPTY for PTY, DX12 for GPU rendering.
Built in 1 day across 4 phases (12 plans).
Known tech debt: display_offset hardcoded to 0, Nyquist validation partial on phases 2-4.

## Constraints

- **Tech stack**: Rust — non-negotiable (performance, memory safety, cross-platform compilation)
- **VTE layer**: alacritty_terminal 0.25.1 (exact pin) — battle-tested terminal emulation
- **Rendering**: wgpu with DX12 on Windows (auto-select on other platforms)
- **Performance**: <500ms cold start, <5ms input latency, <120MB idle memory
- **Polish**: Daily-drivable — good enough to use as primary terminal

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Windows first | Developer is on Windows, build where you test | ✓ Good — DX12 + ConPTY works well |
| Fork alacritty_terminal for VTE | Battle-tested since 2017, embeddable, Apache 2.0 | ✓ Good — 0.25.1 worked with custom PTY read loop |
| wgpu DX12 forced backend | 33% faster than Vulkan on Windows | ✓ Good — 360ms cold start |
| Instanced WGSL quad rendering | Simple, fast cell backgrounds without index buffer | ✓ Good — clean GPU pipeline |
| Per-line cosmic_text::Buffer | Per-character fg color and font weight/style | ✓ Good — flexible text rendering |
| Custom PTY read loop | Replaced alacritty PtyEventLoop for OscScanner pre-scanning | ✓ Good — enables shell integration |
| PSReadLine Enter handler for 133;C | More reliable than PreExecution hook across versions | ✓ Good — works with pwsh 7+ |
| Dedicated PTY reader thread | std::thread not Tokio — blocking PTY I/O must not block async executor | ✓ Good — clean separation |
| GridSnapshot lock-minimizing pattern | Minimize lock contention between PTY reader and renderer | ✓ Good — no visible lag |
| Revised cold start <500ms | DX12 hardware init floor ~290ms unavoidable | ✓ Good — realistic target met |
| Revised memory <120MB | GPU driver allocations ~80MB baseline | ✓ Good — realistic target met |

---
*Last updated: 2026-03-05 after v1.0 milestone*

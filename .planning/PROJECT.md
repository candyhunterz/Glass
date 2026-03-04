# Glass

## What This Is

Glass is a cross-platform terminal emulator built in Rust that understands what commands *do*, not just what they print. It introduces three capabilities no existing terminal offers: command-level undo with filesystem snapshots, visual pipe debugging with inspectable intermediate stages, and structured scrollback that turns terminal history into a queryable database. It exposes this data to AI assistants via MCP.

## Core Value

A terminal that looks and feels normal but passively watches, indexes, and snapshots everything — surfacing intelligence only when you need it.

## Requirements

### Validated

(None yet — ship to validate)

### Active

- [ ] Rust workspace with crate structure (glass_core, glass_terminal, glass_renderer + stubs for glass_history, glass_snapshot, glass_pipes, glass_mcp)
- [ ] Window creation with wgpu GPU-accelerated rendering surface
- [ ] PTY child process spawning via alacritty_terminal (ConPTY on Windows)
- [ ] VTE output rendered as monospace grid with cursor and colors
- [ ] Keyboard input forwarded to PTY stdin
- [ ] Shell integration hooks for PowerShell and bash (command start/end, prompt detection, cwd tracking)
- [ ] Block-based command output (collapsible, shows exit code and duration)
- [ ] Status bar (cwd, git branch, dirty count)
- [ ] Minimal TOML configuration (font, font size, shell override)
- [ ] Daily-drivable polish — usable as a real terminal during development

### Out of Scope

- Built-in AI chat — Glass exposes data *to* AI assistants, not an AI itself
- IDE features — no file explorer, editor, or LSP integration
- Plugin system — core features must be solid first
- Cloud sync — history and snapshots stay local
- Theme marketplace — one dark theme, one light theme
- Tabs and split panes — deferred to later milestone
- Command-level undo (Phase 3 feature, not this milestone)
- Pipe visualization (Phase 4 feature, not this milestone)
- Structured scrollback / history DB (Phase 2 feature, not this milestone)
- MCP server (Phase 2 feature, not this milestone)
- macOS / Linux support — Windows first, cross-platform later
- cmd.exe shell support — PowerShell and bash only for now
- Ctrl+Z undo binding — deferred to Phase 3, pass through to shell normally

## Context

- Developer is on Windows 11. Glass is being built Windows-first, unlike the PRD which targeted macOS/Linux as P0.
- This is Milestone 1 covering PRD Phases 0-1: project scaffold through foundation terminal with block UI.
- The PRD specifies alacritty_terminal ~0.24, wgpu ~24, winit ~0.30 — these are estimates. Research should verify current versions and Windows compatibility.
- ConPTY is the Windows PTY mechanism. Need to verify alacritty_terminal's ConPTY support or identify a platform shim.
- Shell integration for PowerShell requires custom prompt functions (similar to starship/oh-my-posh hooks). Bash integration uses PROMPT_COMMAND / precmd patterns.
- Full PRD available at `PRD.md` in the project root for reference.

## Constraints

- **Tech stack**: Rust — non-negotiable (performance, memory safety, cross-platform compilation)
- **VTE layer**: Fork/embed alacritty_terminal — don't build terminal emulation from scratch
- **Rendering**: wgpu for GPU-accelerated rendering (auto-selects DX12/Vulkan/OpenGL backend)
- **Config**: Minimal for this milestone — font, font size, shell override only
- **Performance**: <200ms cold start, <5ms input latency, <50MB idle memory
- **Polish**: Daily-drivable — good enough to use as primary terminal during Glass development

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Windows first | Developer is on Windows, build where you test | — Pending |
| Fork alacritty_terminal for VTE | Battle-tested since 2017, embeddable, Apache 2.0 | — Pending |
| wgpu auto-select backend | Let wgpu pick DX12/Vulkan/OpenGL per system | — Pending |
| PowerShell + bash shells only | Primary shells on Windows dev environment | — Pending |
| Milestone 1 = Phase 0-1 only | Get to usable terminal first, add features in future milestones | — Pending |
| Defer Ctrl+Z to Phase 3 | Undo doesn't exist yet, pass through to shell normally | — Pending |
| Minimal config | Font, font size, shell override — no feature config until features exist | — Pending |

---
*Last updated: 2026-03-04 after initialization*

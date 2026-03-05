# Requirements: Glass

**Defined:** 2026-03-04
**Core Value:** A terminal that looks and feels normal but passively watches, indexes, and snapshots everything — surfacing intelligence only when you need it.

## v1 Requirements

Requirements for Milestone 1 (foundation terminal with block UI). Each maps to roadmap phases.

### Terminal Core

- [x] **CORE-01**: User can launch Glass and get a working PowerShell prompt via ConPTY
- [x] **CORE-02**: User can run any CLI tool with correct VT/ANSI escape sequence rendering (colors, formatting, cursor movement)
- [ ] **CORE-03**: User can use keyboard with Ctrl, Alt, Shift modifiers correctly (vim, fzf, tmux work)
- [ ] **CORE-04**: User can paste multi-line text safely via bracketed paste mode (no accidental execution)
- [x] **CORE-05**: User can scroll back through at least 10,000 lines of output (configurable)
- [ ] **CORE-06**: User can copy text with Ctrl+Shift+C and paste with Ctrl+Shift+V
- [ ] **CORE-07**: User can resize the Glass window and terminal content reflows correctly
- [x] **CORE-08**: UTF-8 text renders correctly (no mojibake from Windows code page issues)

### Rendering

- [x] **RNDR-01**: Terminal output renders via GPU acceleration (wgpu with DX12 on Windows)
- [x] **RNDR-02**: User sees truecolor (24-bit RGB) output from tools like bat, delta, neovim
- [ ] **RNDR-03**: Cursor renders correctly in block, beam, and underline shapes with optional blink
- [ ] **RNDR-04**: User can configure font family and font size, and text renders in chosen monospace font

### Shell Integration

- [ ] **SHEL-01**: Glass parses OSC 133 sequences to detect command lifecycle (prompt start, input start, command executed, command finished with exit code)
- [ ] **SHEL-02**: Glass parses OSC 7 sequences to track current working directory
- [ ] **SHEL-03**: PowerShell integration script installs and emits OSC 133/7 sequences (wraps existing prompt, compatible with Oh My Posh/Starship)
- [ ] **SHEL-04**: Bash integration script installs and emits OSC 133/7 sequences (via PROMPT_COMMAND/PS0)

### Block UI

- [ ] **BLOK-01**: Each command's prompt, input, and output renders as a visually distinct block
- [ ] **BLOK-02**: Each block displays an exit code badge (green checkmark for success, red X for failure)
- [ ] **BLOK-03**: Each block displays command wall-clock duration

### Status Bar

- [ ] **STAT-01**: Status bar displays current working directory (updated via OSC 7 events)
- [ ] **STAT-02**: Status bar displays git branch name and dirty file count (via async subprocess)

### Configuration

- [ ] **CONF-01**: User can configure Glass via TOML config file (~/.glass/config.toml)
- [ ] **CONF-02**: User can set font family and font size in config
- [ ] **CONF-03**: User can override default shell in config

### Performance

- [ ] **PERF-01**: Cold start time is under 200ms
- [ ] **PERF-02**: Input latency (keypress to screen) is under 5ms
- [ ] **PERF-03**: Idle memory usage is under 50MB

## v2 Requirements

Deferred to future milestones. Tracked but not in current roadmap.

### History & AI (PRD Phase 2)

- **HIST-01**: User can search terminal history by command text, exit code, timestamp, CWD, and files modified
- **HIST-02**: User can query history via Ctrl+Shift+F search overlay
- **HIST-03**: AI assistants can query Glass history via MCP server (GlassHistory, GlassContext tools)

### Undo (PRD Phase 3)

- **UNDO-01**: Every file-modifying command triggers automatic filesystem snapshot before modification
- **UNDO-02**: User can undo any file-modifying command's filesystem changes with Ctrl+Z or [undo] button

### Pipe Visualization (PRD Phase 4)

- **PIPE-01**: Piped commands show intermediate output at each pipeline stage
- **PIPE-02**: Each pipeline stage is expandable to show full intermediate output

### Polish

- **POLI-01**: User can collapse/expand command blocks to reclaim screen space
- **POLI-02**: URLs in terminal output are detected and Ctrl+clickable
- **POLI-03**: User can navigate between blocks with keyboard shortcuts
- **POLI-04**: Config file changes apply without restarting Glass (hot reload)

### Platform

- **PLAT-01**: Glass runs on macOS (Apple Silicon + Intel)
- **PLAT-02**: Glass runs on Linux (x86_64, aarch64)
- **PLAT-03**: Glass supports tabs and split panes

## Out of Scope

| Feature | Reason |
|---------|--------|
| Built-in AI chat | Glass exposes data *to* AI assistants via MCP, not an AI itself |
| IDE features | No file explorer, editor, or LSP integration — Glass is a terminal |
| Plugin/extension system | Core features must be solid before exposing extension API |
| Cloud sync | History and snapshots stay local — no accounts, no telemetry |
| Theme marketplace | Ship one dark theme, one light theme — add theming later |
| cmd.exe shell support | No shell integration hooks available for cmd.exe |
| Font ligatures | Requires HarfBuzz shaping pipeline — rendering enhancement for future milestone |
| Image protocols (Kitty, Sixel) | Requires separate rendering layer — not needed for M1 workflows |
| Searchable scrollback UI | Block structure mitigates this; full search deferred to history DB (Phase 2) |
| Right-click context menu | Keyboard shortcuts sufficient for developer audience |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| CORE-01 | Phase 1 | In progress (01-01 scaffold done) |
| CORE-02 | Phase 2 | Complete |
| CORE-03 | Phase 2 | Pending |
| CORE-04 | Phase 2 | Pending |
| CORE-05 | Phase 2 | Complete |
| CORE-06 | Phase 2 | Pending |
| CORE-07 | Phase 2 | Pending |
| CORE-08 | Phase 2 | Complete |
| RNDR-01 | Phase 1 | In progress (01-01 scaffold done) |
| RNDR-02 | Phase 2 | Complete |
| RNDR-03 | Phase 2 | Pending |
| RNDR-04 | Phase 2 | Pending |
| SHEL-01 | Phase 3 | Pending |
| SHEL-02 | Phase 3 | Pending |
| SHEL-03 | Phase 3 | Pending |
| SHEL-04 | Phase 3 | Pending |
| BLOK-01 | Phase 3 | Pending |
| BLOK-02 | Phase 3 | Pending |
| BLOK-03 | Phase 3 | Pending |
| STAT-01 | Phase 3 | Pending |
| STAT-02 | Phase 3 | Pending |
| CONF-01 | Phase 4 | Pending |
| CONF-02 | Phase 4 | Pending |
| CONF-03 | Phase 4 | Pending |
| PERF-01 | Phase 4 | Pending |
| PERF-02 | Phase 4 | Pending |
| PERF-03 | Phase 4 | Pending |

**Coverage:**
- v1 requirements: 27 total
- Mapped to phases: 27
- Unmapped: 0

---
*Requirements defined: 2026-03-04*
*Last updated: 2026-03-05 after plan 01-01 completion*

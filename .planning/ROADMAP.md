# Roadmap: Glass

## Overview

Glass is built in four phases that follow a strict dependency chain. Phase 1 establishes the structural scaffold — workspace, GPU surface, and PTY architecture — with all critical pitfalls addressed before any feature work. Phase 2 wires VTE parsing to the rendering pipeline to produce a fully functional baseline terminal. Phase 3 adds shell integration and block UI, which are the visible Glass differentiator. Phase 4 locks in configuration and confirms the daily-driver performance targets. Each phase delivers a coherent, independently verifiable capability.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [x] **Phase 1: Scaffold** - Cargo workspace, wgpu GPU surface, PTY spawn with keyboard round-trip, and all structural pitfalls addressed (completed 2026-03-05)
- [x] **Phase 2: Terminal Core** - Full VTE rendering pipeline producing a functional terminal (colors, keyboard, scrollback, copy/paste, resize, UTF-8) (completed 2026-03-05)
- [ ] **Phase 3: Shell Integration and Block UI** - OSC 133/7 parsing, PowerShell and bash integration scripts, block-based command output, status bar
- [ ] **Phase 4: Configuration and Performance** - TOML config file, font/shell override, cold start and latency targets confirmed

## Phase Details

### Phase 1: Scaffold
**Goal**: The project compiles, a window opens with a GPU-rendered surface, and PowerShell spawns in a PTY with keyboard input reaching the shell — all structural pitfalls resolved before feature work begins
**Depends on**: Nothing (first phase)
**Requirements**: CORE-01, RNDR-01
**Success Criteria** (what must be TRUE):
  1. `cargo build` succeeds for the full workspace including all stub crates (glass_core, glass_terminal, glass_renderer, glass_history, glass_snapshot, glass_pipes, glass_mcp)
  2. Glass launches and displays a wgpu-rendered window with DX12 backend; window can be dragged and resized without crash or visible flicker
  3. PowerShell spawns via ConPTY and the user can type a command and see output — keyboard input reaches the PTY stdin
  4. Escape sequence fixture tests pass (ConPTY ENABLE_VIRTUAL_TERMINAL_INPUT verified, UTF-8 code page 65001 set)
**Plans:** 3/3 plans complete

Plans:
- [x] 01-01-PLAN.md — Cargo workspace with all 7 crates, glass_core types, and compiling root binary
- [x] 01-02-PLAN.md — winit window with wgpu DX12 GPU surface (clear-to-color, resize-stable)
- [x] 01-03-PLAN.md — ConPTY PTY spawn with dedicated reader thread and keyboard round-trip

### Phase 2: Terminal Core
**Goal**: Glass is a functional terminal — any CLI tool works correctly with full color, keyboard modifiers, scrollback, copy/paste, and window resize
**Depends on**: Phase 1
**Requirements**: CORE-02, CORE-03, CORE-04, CORE-05, CORE-06, CORE-07, CORE-08, RNDR-02, RNDR-03, RNDR-04
**Success Criteria** (what must be TRUE):
  1. Running `bat`, `delta`, `neovim`, and `ls --color` all display correct truecolor (24-bit RGB) output with no color artifacts
  2. vim, fzf, and tmux work correctly — Ctrl, Alt, Shift modifier keys produce correct escape sequences
  3. Pasting multi-line text (e.g., a shell script) does not execute immediately; bracketed paste mode prevents accidental execution
  4. Scrolling back through 10,000 lines of output works without blank regions or performance degradation
  5. Copying with Ctrl+Shift+C and pasting with Ctrl+Shift+V works; window resize causes terminal content to reflow correctly; non-ASCII characters (emoji, CJK, accented chars) render without mojibake
**Plans:** 3/3 plans complete

Plans:
- [ ] 02-01-PLAN.md — GridSnapshot data pipeline, color resolution, glyphon initialization, workspace dependencies
- [ ] 02-02-PLAN.md — GPU text rendering pipeline (RectRenderer, GridRenderer, FrameRenderer, cursor, font-metrics resize)
- [ ] 02-03-PLAN.md — Keyboard escape encoding, clipboard copy/paste, scrollback interaction, bracketed paste

### Phase 3: Shell Integration and Block UI
**Goal**: Shell integration scripts emit OSC 133/7 sequences that Glass parses into a BlockManager, rendering each command's output as a visually distinct block with exit code, duration, and a status bar showing CWD and git branch
**Depends on**: Phase 2
**Requirements**: SHEL-01, SHEL-02, SHEL-03, SHEL-04, BLOK-01, BLOK-02, BLOK-03, STAT-01, STAT-02
**Success Criteria** (what must be TRUE):
  1. Running a command in PowerShell (with integration script installed) renders its prompt, input, and output as a visually distinct block separated from surrounding commands
  2. Each block displays a green checkmark badge for exit code 0 and a red X badge for non-zero exit codes
  3. Each block displays the wall-clock duration of the command (e.g., "1.2s")
  4. The status bar shows the current working directory, updating when `cd` is run; it shows the git branch name and dirty file count when inside a git repository
  5. Shell integration is compatible with Oh My Posh and Starship — their prompt styling is preserved when integration scripts are installed
**Plans:** 3/4 plans executed

Plans:
- [x] 03-01-PLAN.md — OscScanner byte parser, BlockManager state machine, StatusState with git queries (TDD)
- [ ] 03-02-PLAN.md — Block rendering (separators, exit code badges, duration labels) and status bar in FrameRenderer
- [x] 03-03-PLAN.md — PowerShell and Bash shell integration scripts emitting OSC 133/7
- [ ] 03-04-PLAN.md — Custom PTY read loop with OscScanner, full wiring into main.rs

### Phase 4: Configuration and Performance
**Goal**: Glass reads a TOML config file for font, font size, and shell override; and the application meets cold start, input latency, and idle memory targets that confirm it is daily-drivable
**Depends on**: Phase 3
**Requirements**: CONF-01, CONF-02, CONF-03, PERF-01, PERF-02, PERF-03
**Success Criteria** (what must be TRUE):
  1. A `~/.glass/config.toml` file with `font_family`, `font_size`, and `shell` fields is loaded at startup and the chosen font and shell are applied
  2. Cold start time (from launch to interactive prompt) is under 200ms measured on a clean launch
  3. Keypress-to-screen latency is under 5ms measured under normal shell load
  4. Idle memory usage (shell at prompt, no active process) is under 50MB
**Plans:** 2 plans

Plans:
- [ ] 04-01-PLAN.md — TOML config loading with serde/dirs, wiring into FrameRenderer and spawn_pty
- [ ] 04-02-PLAN.md — Performance instrumentation (cold start, input latency, idle memory) and target verification

## Progress

**Execution Order:**
Phases execute in numeric order: 1 → 2 → 3 → 4

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Scaffold | 3/3 | Complete   | 2026-03-05 |
| 2. Terminal Core | 3/3 | Complete   | 2026-03-05 |
| 3. Shell Integration and Block UI | 3/4 | In Progress|  |
| 4. Configuration and Performance | 0/2 | Not started | - |

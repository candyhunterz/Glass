# Glass

[![CI](https://github.com/candyhunterz/Glass/actions/workflows/ci.yml/badge.svg)](https://github.com/candyhunterz/Glass/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A GPU-accelerated terminal emulator built in Rust. Glass looks like a normal terminal but understands command structure — every command produces a structured record that humans can inspect and AI agents can query.

**For humans**: command blocks with exit codes, durations, and CWD badges; command-level undo; visual pipeline debugging; full-text history search.

**For AI agents**: 33 MCP tools, Structured Output Intelligence (SOI) with 19 format-specific parsers, token-budgeted context compression, multi-agent coordination, and an optional autonomous agent mode backed by Claude CLI.

- [Why Glass?](#why-glass)
- [What Makes Glass Different](#what-makes-glass-different)
- [Features](#features)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Keyboard Shortcuts](#keyboard-shortcuts)
- [Configuration](#configuration)
- [CLI Reference](#cli-reference)
- [Orchestrator Mode](#orchestrator-mode)
- [Multi-Agent Coordination](#multi-agent-coordination)
- [MCP Tools](#mcp-tools)
- [Architecture](#architecture)
- [Performance](#performance)
- [License](#license)

---

## Why Glass?

| Capability | Glass | Standard terminals |
|---|---|---|
| Undo | Pre-exec snapshots, Ctrl+Shift+Z restore | None |
| Pipe debugging | Multi-row pipeline UI, per-stage inspection | None |
| Search | SQLite + FTS5, CLI query interface, search overlay | grep ~/.bash_history |
| AI context | Compressed structured output, diff-aware, token-budgeted | Raw scrollback dump |
| Agent terminal control | glass_tab_create/send/output -- full tab orchestration | None |
| Token efficiency | glass_cache_check, glass_command_diff, glass_compressed_context | None |
| Multi-agent coordination | Shared SQLite, advisory locks (atomic all-or-nothing), messaging, activity stream | None |
| Structured output | 19 format parsers (cargo, npm, pytest, jest, git, docker, kubectl, tsc, Go, terraform, JSON, CSV, TAP, C++, ...) | None |
| Agent mode | Background Claude CLI runtime, approval UI, budget cap, worktree isolation | None |
| Orchestrator | Autonomous project builder from PRD, overnight runs, checkpoint/resume, metric guard (auto-revert on regression), bounded iterations | None |

---

## What Makes Glass Different

### For Humans

Glass turns every command into a first-class object. Completed blocks show exit code, duration, and working directory. Destructive commands (rm, mv, sed -i, etc.) are snapshotted before execution so you can restore with a single keystroke. Pipelines render as a multi-row UI so you can inspect each stage's output independently. History is stored in SQLite with full-text search -- queryable from the shell, search overlay, or CLI.

Tabs support drag-to-reorder and close buttons. Panes use a binary tree layout: split horizontally or vertically, resize with Alt+Shift+Arrow, navigate with Alt+Arrow. Shell integration is injected automatically for bash, zsh, fish, and PowerShell via OSC 133 sequences.

### For AI Agents

Glass exposes 33 MCP tools covering the full surface of a terminal session: history, undo, file diffs, pipe inspection, tab orchestration, live command awareness, error extraction, token-efficient context, structured output queries, and scripting automation.

Structured Output Intelligence (SOI) auto-parses every completed command using 19 format-specific parsers. Results are stored in SQLite (schema v3) and served through `glass_query`, `glass_query_trend`, and `glass_query_drill`. The `glass_compressed_context` tool delivers a 4-level token-budgeted summary (OneLine / Summary / Detailed / Full) with diff-aware change detection, so agents can maintain awareness of the terminal session without exhausting their context window. Block decorations show a one-line SOI summary on each completed command block. Shell hint lines are injected into PTY output to help agents discover available MCP tools at session start.

### Agent Mode

An optional background Claude CLI runtime that watches the activity stream and can propose or execute actions. Three modes: Watch (observe only), Assist (propose with approval), Autonomous (execute within permission matrix). Actions are gated by a configurable permission matrix -- edit_files, run_commands, and git_operations each set to `approve`, `auto`, or `never`. A $1.00 default budget cap and 30-second cooldown between proposals prevent runaway spend.

Code changes are isolated in git worktrees with SQLite crash recovery. A non-blocking approval UI (toast notifications + Ctrl+Shift+A review overlay) keeps humans in the loop. Session continuity is maintained via structured handoff summaries across context resets. The activity stream feeds compressed SOI data to the agent runtime with noise filtering and rate limiting.

### Orchestrator Mode

An autonomous project execution engine that pairs a Glass Agent (reviewer/guide) with Claude Code (implementer) to build entire projects from a PRD or continue work you've started. Press Ctrl+Shift+O to enable.

**Two primary workflows:**
- **Fresh project**: Write a PRD.md, open Glass, press Ctrl+Shift+O. The orchestrator drives Claude Code through the plan, checkpointing progress and refreshing context automatically.
- **Mid-work handoff**: Write `.glass/handoff.md` describing what needs finishing, press Ctrl+Shift+O, walk away. The orchestrator picks up from your terminal context and git history.

The orchestrator monitors PTY silence to detect when Claude Code finishes working, sends terminal context to the Glass Agent for review, and types the agent's instructions back into the terminal. A checkpoint cycle kills and respawns the agent with fresh context every few features (or every 15 iterations). Stuck detection, crash recovery, and OAuth usage tracking with auto-pause keep overnight runs safe.

---

## Features

**Core terminal**
- Command blocks with exit code, duration, and CWD badges
- Tabs with drag-to-reorder and close buttons
- Split panes (binary tree layout: horizontal and vertical)
- Scrollbar and full scrollback
- GPU rendering via wgpu (DX12 on Windows, Metal on macOS, Vulkan/OpenGL on Linux)
- Shell integration for bash, zsh, fish, and PowerShell (OSC 133)
- Clipboard (copy/paste)
- Mouse selection

**History**
- SQLite + FTS5 command history database
- Search overlay (Ctrl+Shift+F)
- CLI query interface (`glass history search`, `glass history list`)

**Undo**
- Content-addressed blob store (blake3 hashing)
- Pre-exec filesystem snapshots for destructive commands
- Ctrl+Shift+Z restore
- FS watcher for change detection

**Pipes**
- Pipeline parser and tee-based shell stage capture
- Multi-row pipeline UI with per-stage inspection (Ctrl+Shift+P)

**Structured Output Intelligence (SOI) -- new in v3.0**
- Output classifier with 19 format-specific parsers: cargo (build/test/misc), npm, jest, pytest, tsc, Go (build/test), git, docker, kubectl, terraform, JSON (lines/object), CSV, TAP, C++ compiler, generic compiler
- SQLite storage (schema v3) with auto-parse on CommandFinished via spawn_blocking
- 4-level token-budgeted compression: OneLine / Summary / Detailed / Full
- Diff-aware change detection across command runs
- Block decorations showing one-line SOI summary on completed command blocks
- Shell hint lines injected into PTY output for agent tool discovery
- 3 MCP tools: glass_query, glass_query_trend, glass_query_drill

**Agent Mode -- new in v3.0**
- Bounded activity stream feeding compressed SOI data to agent runtime
- Noise filtering and rate limiting on the activity stream
- Background Claude CLI runtime with Watch / Assist / Autonomous modes
- Platform-safe process lifecycle (Windows Job Objects, Unix prctl)
- $1.00 default budget cap, 30-second cooldown between proposals
- Git worktree isolation for agent code changes with SQLite crash recovery
- Non-blocking approval UI: toast notifications and review overlay (Ctrl+Shift+A)
- Status bar indicator for agent mode
- Session continuity with structured handoff summaries across context resets
- Full [agent] config: permission matrix, quiet rules, hot-reload, graceful degradation when Claude CLI is absent
- Coordination lock integration

**Orchestrator Mode -- new in v3.2**
- Autonomous project execution from PRD.md (Ctrl+Shift+O)
- Mid-work handoff via .glass/handoff.md
- Silence-triggered feedback loop between Glass Agent (reviewer) and Claude Code (implementer)
- Periodic checkpoint cycle with agent respawn for context refresh
- Stuck detection (3 identical responses triggers recovery)
- Crash recovery with automatic Claude Code restart and context injection
- OAuth usage tracking with auto-pause at 80% and hard stop at 95%
- Course correction via .glass/nudge.md while running
- Iteration logging to .glass/iterations.tsv
- Emergency checkpoint on usage hard stop
- Metric guard: auto-detect verify commands (Rust, Node, Python, Go, Make), background verification after each iteration, auto-revert via git on regression
- Artifact-based completion signal: file watcher triggers orchestrator instantly when agent writes to configurable path (default `.glass/done`)
- Bounded iteration mode: limit orchestration to N iterations, then checkpoint-stop with summary
- GLASS_VERIFY agent response for dynamic verification command discovery
- Self-improving feedback loop: rule-based analysis after each run (15 detectors), auto config tuning, Rust-level enforcement (auto-commit on drift, hot file isolation, instruction splitting, scope guard with auto-revert, dependency block), optional LLM qualitative analysis, regression guard with auto-rollback, rule lifecycle (proposed → provisional → confirmed), staleness detection, 6 default rules shipped — 8 of 9 actions enforced in code, no LLM compliance needed
- Tier 4 scripting: Rhai-based automation scripts generated from feedback findings, sandboxed execution, promotion/rejection lifecycle, exportable profiles

**Scripting Layer -- new in v3.2**
- Embedded Rhai scripting engine with event-driven hook system
- Hook points: SnapshotBefore, McpRequest, and orchestrator lifecycle events
- Sandboxed execution: CPU time limits, memory caps, array size bounds
- Script lifecycle: Provisional → Confirmed promotion with regression guard
- Script profiles: export/import bundles with metadata and tech stack tags
- 2 MCP tools: `glass_list_script_tools` (discover available scripts), `glass_script_tool` (execute a script)
- Scripts loaded from `~/.glass/scripts/` (global) and `<project>/.glass/scripts/` (project-local)

**Activity Stream -- new in v3.1**
- Real-time coordination event log (agent registrations, lock acquisitions, conflicts, messages)
- Two-line contextual status bar with agent activity summary and ticker
- Fullscreen overlay (Ctrl+Shift+G) with agent cards, event timeline, and category filters
- Agent Mode observation events (command_seen, output_parsed, error_noticed, proposing, dismissed)
- Command context events from OSC 133 boundaries (started, finished with exit code and duration)
- Scroll, filter by category (All/Agents/Locks/Observations/Messages), toggle verbose mode

**Settings Overlay -- new in v3.1**
- In-app settings editor (Ctrl+Shift+,) with three tabs: Settings, Shortcuts, About
- Settings tab: sidebar with 8 config sections (Font, Agent Mode, SOI, Snapshots, Pipes, History, Orchestrator, Scripting)
- Editable fields with Enter/Space to toggle booleans, +/- to adjust numeric values
- Changes write back to ~/.glass/config.toml (hot-reload picks up changes immediately)
- Shortcuts tab: two-column keyboard shortcut cheatsheet
- About tab: version info, platform details, license

**Multi-agent coordination**
- Shared SQLite database (~/.glass/agents.db) in WAL mode
- Agent registry scoped by project root
- Advisory file locks (atomic all-or-nothing)
- Inter-agent messaging (directed and broadcast)
- Agent status tracking
- Coordination event log for activity stream UI

**MCP server**
- 33 tools covering history, context, undo, diffs, pipes, tab orchestration, token saving, error extraction, live awareness, SOI query, coordination, scripting, and health
- stdio transport

---

## Installation

### Pre-built binaries

Download the latest release from [github.com/candyhunterz/Glass/releases](https://github.com/candyhunterz/Glass/releases).

> **macOS Gatekeeper:** If macOS blocks Glass with "cannot be opened because the developer cannot be verified", run:
> ```bash
> xattr -cr /Applications/Glass.app
> ```
> This removes the quarantine attribute from unsigned downloads. Code signing and notarization are planned for a future release.

### Build from source

Prerequisites: Rust stable toolchain (https://rustup.rs), Git.

On Linux, install system dependencies for your distribution:

**Debian / Ubuntu:**
```bash
sudo apt install libxkbcommon-dev libwayland-dev libx11-dev libxi-dev libxtst-dev
```

**Fedora:**
```bash
sudo dnf install libxkbcommon-devel wayland-devel libX11-devel libXi-devel libXtst-devel
```

**Arch Linux:**
```bash
sudo pacman -S libxkbcommon wayland libx11 libxi libxtst
```

```bash
git clone https://github.com/candyhunterz/Glass.git
cd Glass
cargo build --release
# Binary: target/release/glass  (target\release\glass.exe on Windows)
```

### Cargo install

```bash
cargo install --git https://github.com/candyhunterz/Glass.git glass
```

The binary is self-contained. Shell integration scripts are embedded and auto-injected at PTY spawn time.

---

## Quick Start

Launch Glass:

```bash
glass
```

Glass auto-injects shell integration into your running shell. Command blocks appear as you run commands -- each shows the command text, an exit code badge (green/red), duration, and working directory.

**Try undo**: run `touch testfile.txt`, then press Ctrl+Shift+Z. Glass restores the filesystem to the state before that command.

**Try history search**: press Ctrl+Shift+F to open the search overlay, or run `glass history search "cargo build"` in any shell.

**Try pipeline inspection**: run a piped command such as `cat file | grep pattern | sort`, then press Ctrl+Shift+P to toggle the pipeline visualization.

**Enable MCP** (for AI agent integration): run `glass mcp serve` or configure your AI client to connect to the Glass MCP server.

---

## Keyboard Shortcuts

### Core

| Action | Windows / Linux | macOS |
|---|---|---|
| Copy | Ctrl+Shift+C | Cmd+Shift+C |
| Paste | Ctrl+Shift+V | Cmd+Shift+V |
| Search history | Ctrl+Shift+F | Cmd+Shift+F |
| Undo last command | Ctrl+Shift+Z | Cmd+Shift+Z |
| Toggle pipeline view | Ctrl+Shift+P | Cmd+Shift+P |
| Check for updates | Ctrl+Shift+U | Cmd+Shift+U |

### Tabs

| Action | Windows / Linux | macOS |
|---|---|---|
| New tab | Ctrl+Shift+T | Cmd+Shift+T |
| Close tab | Ctrl+Shift+W | Cmd+Shift+W |
| Next tab | Ctrl+Tab | Ctrl+Tab |
| Previous tab | Ctrl+Shift+Tab | Ctrl+Shift+Tab |
| Jump to tab 1-9 | Ctrl+1 through Ctrl+9 | Cmd+1 through Cmd+9 |
| Close tab (mouse) | Middle-click tab | Middle-click tab |

### Panes

| Action | Windows / Linux | macOS |
|---|---|---|
| Split horizontally | Ctrl+Shift+D | Cmd+Shift+D |
| Split vertically | Ctrl+Shift+E | Cmd+Shift+E |
| Focus pane | Alt+Arrow keys | Opt+Arrow keys |
| Resize pane | Alt+Shift+Arrow keys | Opt+Shift+Arrow keys |

### Navigation

| Action | Windows / Linux | macOS |
|---|---|---|
| Scroll up | Shift+PageUp | Shift+PageUp |
| Scroll down | Shift+PageDown | Shift+PageDown |
| Select text | Mouse drag | Mouse drag |

### Overlays

| Action | Windows / Linux | macOS |
|---|---|---|
| Settings | Ctrl+Shift+, | Cmd+Shift+, |
| Review proposals | Ctrl+Shift+A | Cmd+Shift+A |
| Activity stream | Ctrl+Shift+G | Cmd+Shift+G |
| Toggle orchestrator | Ctrl+Shift+O | Cmd+Shift+O |

---

## Configuration

Glass reads `~/.glass/config.toml`. Changes are hot-reloaded without restarting.

```toml
# Font
font_family = "JetBrains Mono"
font_size = 14.0

# Shell override (auto-detected if omitted)
# shell = "/bin/zsh"

[history]
# Maximum number of commands to retain
max_entries = 50000
# Retain commands with non-zero exit codes
keep_failures = true

[snapshot]
# Enable pre-exec filesystem snapshots for undo
enabled = true
# Maximum blob store size in MB
max_blob_store_mb = 500
# Auto-prune snapshots older than N days
retention_days = 30

[pipes]
# Enable pipeline capture and visualization
enabled = true

[soi]
# Enable Structured Output Intelligence auto-parsing
enabled = true
# Inject one-line SOI summary as shell hint in PTY output
shell_summary = true
# Only parse commands whose output exceeds this line count
min_lines = 3

[agent]
# Enable the background agent runtime
enabled = false
# Watch | Assist | Autonomous
mode = "Assist"
# Maximum spend per session in USD
max_budget_usd = 1.00
# Minimum seconds between proposals
cooldown_secs = 30

[agent.permissions]
# approve | auto | never
edit_files = "approve"
run_commands = "approve"
git_operations = "approve"

[agent.quiet_rules]
# Suppress activity stream events matching these glob patterns
ignore_patterns = ["*.log", "node_modules/**"]
# Do not surface commands that exit zero and produce no output
ignore_exit_zero = false

[agent.orchestrator]
# Enable orchestrator mode (toggled at runtime with Ctrl+Shift+O)
enabled = false
# Seconds of PTY silence before sending context to the agent
silence_timeout_secs = 30
# Path to the project requirements document
prd_path = "PRD.md"
# Path to the checkpoint file (for context refresh)
checkpoint_path = ".glass/checkpoint.md"
# Identical responses before stuck detection triggers
max_retries_before_stuck = 3
# Verification mode: "floor" (auto-detect and guard) or "disabled"
verify_mode = "floor"
# Optional override for verification command (skips auto-detect)
# verify_command = "cargo test"
# File path that triggers orchestrator when created (empty to disable)
completion_artifact = ".glass/done"
# Maximum iterations before checkpoint-stop (omit or 0 for unlimited)
# max_iterations = 25
# Feedback loop
feedback_llm = false          # Enable LLM qualitative analysis after each run (opt-in)
# max_prompt_hints = 10       # Max Tier 3 prompt hints per project

[scripting]
# Enable Rhai scripting engine
enabled = true
# Maximum operations per script execution
max_operations = 10000
# Maximum script timeout in milliseconds
max_timeout_ms = 5000
# Maximum scripts per hook point
max_scripts_per_hook = 10
```

Default fonts: Consolas (Windows), Menlo (macOS), Monospace (Linux).

---

## CLI Reference

```
glass                           Launch Glass terminal
glass check                     Run system diagnostics (GPU, shell, config)
glass history search <query>    Full-text search command history
glass history list              List recent commands
glass undo <id>                 Restore filesystem snapshot by command ID
glass mcp serve                 Start MCP server (stdio transport)
```

**Examples:**

```bash
glass history search "cargo build"
glass history search "npm install" --limit 20
glass history list --cwd ~/projects/myapp
glass history list --exit 1

# Find a command ID, then restore
glass history list
glass undo 42
```

---

## Orchestrator Mode

The orchestrator drives autonomous project development by pairing two AI agents: Claude Code (the implementer, running in the PTY) and the Glass Agent (the reviewer/guide, running as a background subprocess). Glass manages the feedback loop between them.

### How It Works

The orchestrator has two phases: **kickoff** (interactive) and **autonomous loop**.

**Kickoff phase** — When you press Ctrl+Shift+O, the orchestrator activates but does not immediately take over. You interact directly with the AI agent in the terminal (answer its questions, describe your task, clarify requirements). Glass tracks your keyboard activity and suppresses the autonomous loop as long as you're actively typing. Once both you and the terminal have been idle for the silence threshold (default 30s), kickoff ends and the autonomous loop begins.

**Autonomous loop:**
1. **Silence detection**: Glass monitors the PTY for inactivity. When the terminal goes quiet, Glass captures the last 100 lines of output.
2. **Agent review**: The captured context is sent to the Glass Agent, which reviews what happened and decides the next step.
3. **Agent response**: The Glass Agent responds with one of five actions:
   - **Text** — typed into the terminal as instructions for the implementer
   - **GLASS_WAIT** — still working, check again later
   - **GLASS_CHECKPOINT** — feature complete, trigger a context refresh cycle
   - **GLASS_DONE** — all PRD items are complete, stop orchestration
   - **GLASS_VERIFY** — report additional verification commands for the metric guard
4. **Loop**: Steps 1-3 repeat until the project is complete or the orchestrator is paused.

### Workflows

**Fresh project from PRD:**
1. Write `PRD.md` in your project root with the full project plan
2. Open Glass in the project directory
3. Start your AI agent (e.g., `claude --dangerously-skip-permissions`)
4. Press Ctrl+Shift+O to enable orchestration
5. The agent may ask clarifying questions — answer them at your own pace
6. Once you stop typing and the terminal goes quiet, the Glass Agent takes over and drives the project autonomously

**Mid-work handoff:**
1. Write `.glass/handoff.md` with instructions for what to finish
2. Press Ctrl+Shift+O
3. The orchestrator captures your terminal context, git history, and handoff note, then starts the autonomous loop

**Course correction while running:**
- Write `.glass/nudge.md` with new instructions. The orchestrator picks it up on the next silence cycle and injects it as a `[USER_NUDGE]`.

### Checkpoint Cycle

Context refresh prevents the Glass Agent from hitting its context limit during long runs:

- After a `GLASS_CHECKPOINT` signal (or every 15 iterations automatically), Glass tells Claude Code to commit and write `.glass/checkpoint.md`
- Glass polls the checkpoint file for updates. Once written (or after a 180-second timeout), the Glass Agent subprocess is killed and respawned with a fresh system prompt containing the updated checkpoint
- The new agent picks up exactly where the previous one left off

### Safety Features

| Feature | Description |
|---|---|
| Metric guard | Auto-detects project test/build commands, runs verification after each iteration, auto-reverts via git if tests regress or build breaks |
| Stuck detection | After 3 identical responses, the orchestrator tells Claude Code to stash changes and try a different approach |
| Crash recovery | If Claude Code exits unexpectedly, Glass restarts it with `-p "Read .glass/checkpoint.md and continue"` |
| Usage tracking | Polls Anthropic OAuth usage API every 60 seconds. Auto-pause at 80%, hard stop at 95% with emergency checkpoint |
| Bounded iterations | Optional iteration limit with checkpoint-stop and summary (configurable via `max_iterations`) |
| Artifact completion | File watcher on configurable path (default `.glass/done`) triggers orchestrator instantly |
| Kickoff guard | Suppresses autonomous loop during kickoff while user is actively typing; transitions to autonomous mode once user and terminal are both idle |
| Grace period | 10-second window after orchestrator PTY writes prevents false crash recovery triggers |
| Backpressure | Context sends are gated on pending response to prevent overlapping messages |

### Files

| File | Purpose |
|---|---|
| `PRD.md` | Project requirements document (configurable path) |
| `.glass/checkpoint.md` | Current progress checkpoint (written by Claude Code, read by orchestrator) |
| `.glass/handoff.md` | User instructions for mid-work handoff (read on enable, deleted after) |
| `.glass/nudge.md` | Course correction while running (read on next silence, deleted after) |
| `.glass/iterations.tsv` | Iteration log (TSV: iteration, commit, feature, metric, status, description) |
| `.glass/done` | Artifact completion signal (configurable path, deleted after processing) |

---

## Multi-Agent Coordination

Glass provides shared coordination infrastructure for teams of AI agents working on the same project. All state lives in `~/.glass/agents.db` (SQLite in WAL mode). Agents are scoped by project root path so multiple projects do not interfere.

### Protocol

Agents should follow this protocol when operating in a Glass-managed project:

1. **On session start**: call `glass_agent_register` with name, type (e.g. `claude-code`), and project root path. Returns an agent ID for all subsequent calls.
2. **Before editing files**: call `glass_agent_lock` with the file paths you intend to edit. Locking is atomic and all-or-nothing -- if any file is held by another agent, the call returns a `Conflict` identifying the holder without acquiring any locks.
3. **After editing files**: call `glass_agent_unlock` to release locks so other agents can proceed.
4. **Periodically**: call `glass_agent_messages` to check for directed messages and broadcasts.
5. **On lock conflict**: call `glass_agent_send` with `msg_type` set to `request_unlock` to ask the holder to release.
6. **When changing tasks**: call `glass_agent_status` to update your status and current task description.
7. **On session end**: call `glass_agent_deregister` to clean up the registration and release all held locks.

`glass_agent_list` returns all active agents for the project, their status, current task, and held locks. Any agent can see what others are doing before starting work.

---

## MCP Tools

Start the MCP server with `glass mcp serve`. Connect using any MCP-compatible client (Claude Desktop, Cursor, etc.).

| Category | Tool | Description |
|---|---|---|
| History & Context | `glass_history` | Query command history with filters |
| History & Context | `glass_context` | Get recent terminal context |
| Undo & Diffs | `glass_undo` | Restore filesystem to pre-command state |
| Undo & Diffs | `glass_file_diff` | Show diff between current and snapshotted file |
| Pipes | `glass_pipe_inspect` | Inspect pipeline stage data |
| Tab Orchestration | `glass_tab_create` | Create a new terminal tab |
| Tab Orchestration | `glass_tab_list` | List open tabs |
| Tab Orchestration | `glass_tab_send` | Send input to a tab |
| Tab Orchestration | `glass_tab_output` | Read output from a tab |
| Tab Orchestration | `glass_tab_close` | Close a tab |
| Token Saving | `glass_cache_check` | Check if a command result is cached |
| Token Saving | `glass_command_diff` | Diff two command results |
| Token Saving | `glass_compressed_context` | Get token-budgeted session summary |
| Error Extraction | `glass_extract_errors` | Extract structured errors from command output |
| Live Awareness | `glass_has_running_command` | Check if a command is currently running |
| Live Awareness | `glass_cancel_command` | Cancel the running command |
| SOI Query | `glass_query` | Query structured output records |
| SOI Query | `glass_query_trend` | Query trends across multiple command executions |
| SOI Query | `glass_query_drill` | Drill into a specific structured output record |
| Coordination | `glass_agent_register` | Register an agent for this project |
| Coordination | `glass_agent_deregister` | Deregister an agent and release all locks |
| Coordination | `glass_agent_list` | List all active agents |
| Coordination | `glass_agent_status` | Update agent status and current task |
| Coordination | `glass_agent_heartbeat` | Send a heartbeat to maintain registration |
| Coordination | `glass_agent_lock` | Acquire advisory locks on file paths |
| Coordination | `glass_agent_unlock` | Release advisory locks |
| Coordination | `glass_agent_locks` | List all held locks for this project |
| Coordination | `glass_agent_broadcast` | Broadcast a message to all agents |
| Coordination | `glass_agent_send` | Send a directed message to a specific agent |
| Coordination | `glass_agent_messages` | Read pending messages for this agent |
| Scripting | `glass_list_script_tools` | List available Rhai scripts and their hook points |
| Scripting | `glass_script_tool` | Execute a registered Rhai script by name |
| Health | `glass_ping` | Verify MCP server connectivity |

**Total: 33 tools**

---

## Architecture

Glass is a Rust workspace with 16 crates plus the root binary.

```
glass (binary)
  src/main.rs              Event loop, window management, session wiring (~2200 lines)

crates/
  glass_core/              Config (TOML), events, update checker, hot-reload watcher
  glass_terminal/          PTY management, VT parsing (alacritty_terminal =0.25.1),
                           block manager (PromptActive -> InputActive -> Executing ->
                           Complete), OSC 133 scanner, shell integration injection
  glass_renderer/          wgpu GPU rendering: grid, blocks, tab bar, status bar,
                           search overlay, pipeline visualization, settings overlay
  glass_mux/               Session multiplexer: tabs, split panes (binary tree layout)
  glass_history/           SQLite + FTS5 command history, query engine, pruning
  glass_snapshot/          Filesystem snapshots: FS watcher, blake3 blob store,
                           undo engine, destructive command parser
  glass_pipes/             Pipeline parser and tee-based stage capture
  glass_mcp/               MCP server (33 tools, stdio transport)
  glass_errors/            Structured error extraction from command output
  glass_coordination/      Multi-agent coordination: agent registry, advisory locks,
                           inter-agent messaging (SQLite WAL)
  glass_soi/               Structured Output Intelligence: output classifier,
                           19 format parsers, SQLite storage (schema v3),
                           4-level compression engine
  glass_agent/             Activity stream, worktree manager, session DB,
                           approval pipeline, budget tracking
  glass_feedback/          Self-improving feedback loop: run analysis, rule engine,
                           config tuning, regression guard, LLM prompts
  glass_scripting/         Rhai scripting: engine, hook registry, lifecycle,
                           sandboxing, profiles, MCP integration
  glass_protocol/          Shared protocol types
  glass_config/            Shared config types
```

**Key design decisions:**

- VTE layer: embeds `alacritty_terminal` (pinned =0.25.1) -- terminal emulation is not reimplemented.
- Rendering: wgpu + glyphon for GPU text rendering. DX12 on Windows, Metal on macOS, Vulkan/OpenGL on Linux.
- PTY: ConPTY on Windows, forkpty on Unix, abstracted behind a common interface.
- Content addressing: blake3 hashing for the snapshot blob store.
- SOI parsing: runs on CommandFinished via `spawn_blocking` to avoid blocking the async runtime.
- Agent worktrees: code changes by the agent runtime are isolated in git worktrees with SQLite crash recovery.
- History: per-project SQLite DB with FTS5 full-text search.

---

## Performance

| Metric | Value |
|---|---|
| Cold start | ~520ms |
| Input latency (p50) | 3-7 microseconds |
| Idle memory | ~89MB |
| Rendering | GPU-accelerated via wgpu |
| History queries | SQLite FTS5, sub-millisecond |
| SOI parsing | Non-blocking via spawn_blocking |

Run `cargo bench` for Criterion benchmarks. Build with `--features perf` for tracing instrumentation (view in [Perfetto](https://ui.perfetto.dev)).

---

## Troubleshooting

### Windows Console Behavior

Glass uses `#![windows_subsystem = "windows"]` to suppress the console window when launched from the Start Menu or Explorer. This means:
- **No visible console output** when double-clicked -- this is intentional
- **CLI subcommands** (`glass history`, `glass check`, `glass mcp`) work normally when run from an existing terminal (PowerShell, cmd, Windows Terminal)
- **Error messages** during initialization use native Windows dialog boxes since stderr is hidden

---

## License

MIT. See [LICENSE](LICENSE).

---

[github.com/candyhunterz/Glass](https://github.com/candyhunterz/Glass)

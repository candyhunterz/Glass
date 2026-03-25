# Contributing to Glass

Thank you for considering a contribution to Glass! This document covers everything you need to get started, understand the architecture, and submit changes.

## Prerequisites

- **Rust toolchain** (stable) -- install from [rustup.rs](https://rustup.rs)
- **Git**

### Linux system dependencies

**Debian / Ubuntu:**
```bash
sudo apt install libxkbcommon-dev libwayland-dev libx11-dev libxi-dev libxtst-dev libfontconfig-dev
```

**Fedora:**
```bash
sudo dnf install libxkbcommon-devel wayland-devel libX11-devel libXi-devel libXtst-devel fontconfig-devel
```

**Arch Linux:**
```bash
sudo pacman -S libxkbcommon wayland libx11 libxi libxtst fontconfig
```

macOS and Windows require no additional system dependencies.

## Building

```bash
cargo build                  # Debug build
cargo build --release        # Optimized release build
cargo build --features perf  # Build with tracing instrumentation
```

## Testing

```bash
cargo test --workspace       # Run all tests (~1,700 tests)
```

Some tests use ConPTY and are gated with `#[cfg(target_os = "windows")]`. These only run on Windows.

## Linting

Both checks must pass before a PR will be merged:

```bash
cargo fmt --all -- --check                  # Check formatting
cargo clippy --workspace -- -D warnings     # Lint (all warnings are errors)
```

## Code Style

- Tests live in the same file as the code they test, inside `#[cfg(test)] mod tests`.
- The `alacritty_terminal` dependency is pinned to exact version `=0.25.1`. Do not change this pin without discussion.
- Use conventional commit messages: `feat:`, `fix:`, `docs:`, `chore:`, `perf:`, `ci:`, `refactor:`, `test:`.
- Prefer `tracing::warn!` over `let _ =` for file I/O errors. Silent error suppression is acceptable for channel sends and display operations.
- No `unwrap()` in production code paths. Use `?`, `unwrap_or_default()`, or explicit match/if-let with error logging.

## PR Process

1. Branch off `master` (the development branch).
2. Make your changes with clear, focused commits.
3. Ensure CI passes: `cargo fmt`, `cargo clippy`, `cargo test` on all three platforms (Linux, macOS, Windows).
4. Open a pull request targeting `main`.
5. A maintainer will review your PR. Address any feedback and push updates.

---

## Architecture Overview

Glass is a Rust workspace with 16 crates plus the root binary. This section explains how data flows through the system so you can orient yourself before diving into code.

### The Big Picture

```
 User types → PTY → alacritty_terminal (VT parse) → Block Manager → Renderer (wgpu)
                ↓
          OSC 133 events → Shell Integration → Command lifecycle
                ↓                                    ↓
          Silence Tracker ← ← ← ← ← ←    Snapshot Engine (blake3 blobs)
                ↓                                    ↓
        Orchestrator loop                     History DB (SQLite + FTS5)
                ↓                                    ↓
          Glass Agent ← ← ← ← ← ← ←    SOI Parsers (19 formats)
                ↓                                    ↓
        TypeText → PTY                     MCP Server (33 tools)
```

### How a Keystroke Becomes a Frame

1. **Input** (`src/main.rs`): winit delivers keyboard events. The event loop encodes them for the shell (handling modifiers, special keys, bracketed paste) and writes bytes to the PTY.

2. **PTY** (`glass_terminal/pty.rs`): The PTY reader thread reads shell output in a loop, feeds it to `alacritty_terminal` for VT parsing, and posts a `Wakeup` event to the winit event loop. On Windows this uses ConPTY; on Unix, forkpty. Wakeup events are throttled to ~60fps to prevent flooding the event loop.

3. **Terminal state** (`alacritty_terminal`): The embedded alacritty terminal crate handles all VT100/xterm escape sequence parsing, maintaining a grid of cells with attributes (color, bold, etc.). Glass does not reimplement terminal emulation.

4. **Rendering** (`glass_renderer/`): On each frame, the renderer reads the terminal grid and composites: terminal cells as GPU quads via wgpu + glyphon text shaping, block decorations (exit code badges, duration, CWD), tab bar, status bar, search overlay, pipe visualization, and settings overlay. All rendering is GPU-accelerated.

### How a Command Becomes a Record

1. **Shell integration** (`shell-integration/`): Glass auto-injects shell integration scripts (bash, zsh, fish, PowerShell) into the PTY at spawn time. These scripts emit OSC 133 escape sequences at prompt start, command start, command execution, and command finish.

2. **OSC scanner** (`glass_terminal/osc_scanner.rs`): Parses OSC 133 sequences from the VT stream. Each sequence triggers a state transition in the Block Manager.

3. **Block Manager** (`glass_terminal/block_manager.rs`): Tracks command lifecycle as a state machine: `PromptActive` → `InputActive` → `Executing` → `Complete`. Each completed block stores the command text, exit code, duration, CWD, and start/end line positions.

4. **Snapshot engine** (`glass_snapshot/`): Before a command executes, the command parser checks if it's destructive (rm, mv, sed -i, etc.). If so, the file watcher's pending changes are flushed to the content-addressed blob store (blake3 hashing) so the user can undo with Ctrl+Shift+Z.

5. **History** (`glass_history/`): On command completion, the command text, exit code, duration, CWD, and captured output are inserted into a per-project SQLite database with FTS5 full-text search. Queryable via `glass query` CLI, search overlay (Ctrl+Shift+F), or MCP tools.

6. **SOI** (`glass_soi/`): Structured Output Intelligence runs 19 format-specific parsers against the command output (cargo, npm, pytest, jest, git, docker, kubectl, tsc, Go, terraform, etc.). Results are stored in SQLite and displayed as one-line labels on command blocks.

### How the Orchestrator Works

The orchestrator is a silence-triggered feedback loop between two agents:

```
                    ┌──────────────────────┐
                    │    Glass Agent        │
                    │  (reviewer/guide)     │
                    │  Separate process     │
                    └──────┬───────────────┘
                           │ TypeText instruction
                           ▼
┌──────────────────────────────────────────────┐
│                    PTY                        │
│  Claude Code (or other implementer) running   │
│  in the terminal, writing code, running tests │
└──────────────────────────────────────────────┘
                           │ Output stops (silence)
                           ▼
                    ┌──────────────────────┐
                    │  Silence Tracker      │
                    │  Captures terminal    │
                    │  context, sends to    │
                    │  Glass Agent          │
                    └──────────────────────┘
```

1. **Activation** (Ctrl+Shift+O): Gathers terminal context, git status, PRD content, and project instructions. Spawns the Glass Agent with a system prompt describing its role. Sends the gathered context as the first message.

2. **Silence detection** (`glass_terminal/silence.rs`): A `SmartTrigger` monitors PTY output. When output stops for the configured timeout (default 5-6 seconds), it fires an `OrchestratorSilence` event.

3. **Context handoff** (`src/main.rs`): On silence, the orchestrator captures the last N terminal lines, git diff, and verification results. This context is sent to the Glass Agent as a user message.

4. **Agent response** (`src/orchestrator.rs`): The Glass Agent responds with structured text: `TypeText:` (instruction for Claude Code), `Wait:` (more time needed), `Checkpoint:` (context refresh), or `GLASS_DONE` (project complete). The orchestrator parses the response and acts accordingly.

5. **Checkpoint cycle**: Every N iterations (default 20), the orchestrator kills both agents and respawns them with fresh context plus a checkpoint summary. This prevents context exhaustion on long runs.

6. **Metric guard**: Before each checkpoint, the orchestrator runs the project's test suite. If test counts drop (regression), it reverts to the last known good commit. Test floor only goes up, never down.

7. **Stuck detection**: If the agent gives 3 identical responses in a row, the orchestrator declares it stuck and tells the agent to try a different approach.

### Crate Responsibilities

| Crate | What it owns | Key files |
|-------|-------------|-----------|
| `glass_core` | Config (TOML + hot-reload), event types, IPC protocol | `config.rs`, `event.rs` |
| `glass_terminal` | PTY spawn/IO, VT parsing, block manager, OSC scanner, silence detection | `pty.rs`, `block_manager.rs`, `osc_scanner.rs`, `silence.rs` |
| `glass_renderer` | GPU rendering, all UI overlays (settings, search, activity, pipes) | `frame.rs`, `grid_renderer.rs`, `activity_overlay.rs` |
| `glass_mux` | Tab/pane multiplexer, binary split tree layout | `session_mux.rs`, `split_tree.rs` |
| `glass_history` | SQLite + FTS5 history DB, retention/pruning | `db.rs` |
| `glass_snapshot` | Content-addressed blob store, undo engine, destructive command detection | `db.rs`, `undo.rs`, `command_parser.rs` |
| `glass_pipes` | Pipeline parsing, stage capture via tee | `lib.rs` |
| `glass_soi` | 19 format-specific output parsers, compression engine | `lib.rs`, `parsers/` |
| `glass_mcp` | MCP server with 33 tools, stdio transport | `tools.rs` |
| `glass_coordination` | Multi-agent registry, advisory file locks, messaging | `db.rs` |
| `glass_agent` | Agent runtime, activity stream, worktree manager | `lib.rs` |
| `glass_agent_backend` | LLM provider backends (Claude CLI, OpenAI, Anthropic, Ollama) | `claude_cli.rs`, `openai.rs`, `anthropic.rs`, `ollama.rs` |
| `glass_feedback` | Run analysis, rule engine, config auto-tuning, LLM qualitative review | `analyzer.rs`, `lib.rs` |
| `glass_scripting` | Rhai scripting engine, hook system, sandbox, profiles | `engine.rs`, `hooks.rs` |
| `glass_errors` | Structured error type definitions | `lib.rs` |

### Key Design Decisions

- **alacritty_terminal is embedded, not forked.** We pin to `=0.25.1` and use it as a library for VT parsing. We never reimplement terminal emulation.
- **The orchestrator communicates through the PTY.** The Glass Agent types instructions into the terminal where Claude Code is running. There's no side-channel — it's text in, text out. This means PTY writes must be split (text then Enter with a 150ms delay) to avoid triggering readline paste detection.
- **Metric guard uses git for rollback.** `git reset --hard` to the last known good commit. This is intentionally aggressive — a failed test means the change is reverted, no questions asked.
- **Config validation uses a field whitelist.** `update_config_field()` rejects unknown field names to prevent the feedback loop or agents from writing garbage to the config.
- **Generation-specific filenames** for agent system prompts and MCP configs prevent file-locking conflicts during checkpoint respawn on Windows.

### Where to Start

- **Adding a new MCP tool**: `glass_mcp/src/tools.rs` — add a handler function and register it in the tool list.
- **Adding a new SOI parser**: `glass_soi/src/parsers/` — implement the `Parser` trait and register in `lib.rs`.
- **Adding a new keyboard shortcut**: `src/main.rs` — find the `KeyEvent` handling match block.
- **Adding a config field**: `glass_core/src/config.rs` — add the field to the appropriate struct, add a default, and add it to the `is_valid_config_field` whitelist.
- **Fixing an orchestrator bug**: `src/orchestrator.rs` for state machine logic, `src/main.rs` for event handling. Read ORCHESTRATOR.md for the full protocol.

## Configuration

See [config.example.toml](config.example.toml) for all configuration options with their defaults and descriptions.

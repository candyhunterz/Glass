# Glass

**A terminal that understands what your commands do, not just what they print.**

Glass is a GPU-accelerated terminal emulator built with Rust and [wgpu](https://wgpu.rs/). It looks and feels like a normal terminal -- you type commands exactly as you always have -- but underneath, Glass watches, indexes, and snapshots everything, surfacing its power only when you need it.

## Why Glass?

Every terminal today treats output as a flat text stream. Glass treats it as structured data.

| Feature | Traditional terminals | Glass |
|---|---|---|
| **Undo destructive commands** | Hope you have a git stash | `Ctrl+Shift+Z` restores files automatically |
| **Debug pipelines** | Insert `tee` at each stage | Click to inspect intermediate output |
| **Search history** | `history \| grep ...` gives you command text | Full-text search across commands, exit codes, timestamps, and output |
| **AI assistant context** | Lost every time the context window resets | MCP server gives AI tools access to your full session history |
| **AI agent terminal control** | Manual copy-paste between agent and terminal | Agents create tabs, run commands, read output, and extract errors via MCP |
| **Token efficiency** | Agents read entire command output | Filtered output, cache checks, diffs, and budget-aware context compression |
| **Multi-agent coordination** | Agents overwrite each other's files | Advisory locks, conflict detection, and inter-agent messaging |
| **Command metadata** | None | Every command shows exit code, duration, and working directory |

### What makes Glass different

- **Command-level undo.** Every file-modifying command triggers an automatic snapshot. Made a mistake with `sed -i` or a rogue script? Undo it. Glass captures file state before modification and restores it on demand. No git archaeology required.

- **Visual pipe debugging.** Run `cat data.csv | grep error | sort | uniq -c` and Glass captures the output at each pipe stage. Click any stage to inspect the intermediate data. No more inserting `tee` everywhere.

- **Structured history.** Your terminal history becomes a queryable SQLite database. Search by command text, exit code, working directory, or time range -- across sessions. The `glass history` CLI gives you full access.

- **MCP server for AI assistants.** Glass exposes your terminal context (recent commands, outputs, working directory) over the Model Context Protocol. AI coding assistants like Claude Code can read what happened in your terminal and pick up where they left off, even after a context reset.

- **Agent terminal control.** AI agents can create tabs, run commands, read output, and close tabs -- all through MCP tools. Agents orchestrate multiple terminal sessions as parallel workspaces without human intervention.

- **Token-efficient agent tools.** Agents don't need to read entire command outputs. Glass provides filtered output (head/tail/regex), cache staleness checks, file diffs for modified files, and budget-aware compressed context that respects token limits.

- **Structured error extraction.** Glass parses compiler output into structured errors with file path, line, column, message, and severity. Supports Rust compiler output (human-readable and JSON format) and a generic parser for any `file:line:col: message` pattern.

- **Live command awareness.** Agents can check whether a command is still running and cancel it if needed -- no more blind waits or manual Ctrl+C.

- **Multi-agent coordination.** Run multiple AI agents in separate Glass tabs and they stay out of each other's way. Glass provides a shared coordination layer with agent registration, advisory file locks, and inter-agent messaging -- all through MCP tools backed by a shared SQLite database. Agents can claim files before editing, detect conflicts, and communicate without human intervention.

- **GPU-accelerated rendering.** Built on wgpu with sub-10us input latency and ~90MB idle memory. Cold start under 500ms.

## Features

- **Command blocks** -- every command is a distinct visual block with exit code, duration, and CWD
- **Tabs** and **split panes** for multitasking
- **Full-text history search** with FTS5/SQLite backend
- **File undo** with automatic filesystem snapshots
- **Pipeline inspection** with per-stage output capture
- **Mouse selection** -- drag to select text, auto-copies to clipboard
- **Agent terminal orchestration** -- AI agents create tabs, run commands, read output, extract errors, and cancel commands via MCP
- **Token-saving tools** -- filtered output, cache checks, file diffs, and budget-aware context compression for AI agents
- **Multi-agent coordination** -- advisory file locks, agent registry, and messaging across AI agents
- **Hot-reloadable configuration** -- edit `~/.glass/config.toml`, changes apply instantly
- **Shell integration** for Bash, Zsh, Fish, and PowerShell
- **Auto-update notifications**
- **Cross-platform:** Windows, macOS, Linux

## Installation

### From source

```bash
# Prerequisites: Rust toolchain (https://rustup.rs)
# Linux only: sudo apt install libwayland-dev libxkbcommon-dev libx11-dev libxi-dev libxtst-dev

git clone https://github.com/<GITHUB_USER>/glass.git
cd glass
cargo build --release
# Binary: target/release/glass (or target\release\glass.exe on Windows)
```

### Packages

| Platform | Method |
|---|---|
| Windows | `.msi` from [Releases](https://github.com/<GITHUB_USER>/glass/releases/latest), or `winget install Glass.Terminal` |
| macOS | `.dmg` from [Releases](https://github.com/<GITHUB_USER>/glass/releases/latest), or `brew install <GITHUB_USER>/glass/glass` |
| Linux | `.deb` from [Releases](https://github.com/<GITHUB_USER>/glass/releases/latest): `sudo dpkg -i glass_*.deb` |

> **macOS note:** The DMG is currently unsigned. If Gatekeeper blocks it, run `xattr -cr /Applications/Glass.app` or right-click and choose **Open**.

## Quick Start

1. Launch Glass -- it detects your default shell automatically.
2. Run commands as usual. Each command appears as a distinct block with metadata.
3. Edit `~/.glass/config.toml` to customize. Changes apply immediately.

## Keyboard Shortcuts

### Core

| Shortcut | macOS | Action |
|---|---|---|
| `Ctrl+Shift+C` | `Cmd+C` | Copy selection |
| `Ctrl+Shift+V` | `Cmd+V` | Paste from clipboard |
| `Ctrl+Shift+F` | `Cmd+F` | Search history |
| `Ctrl+Shift+Z` | `Cmd+Z` | Undo last file-modifying command |
| `Ctrl+Shift+P` | `Cmd+P` | Toggle pipeline expansion |
| `Ctrl+Shift+U` | `Cmd+U` | Apply available update |

### Tabs

| Shortcut | macOS | Action |
|---|---|---|
| `Ctrl+Shift+T` | `Cmd+T` | New tab |
| `Ctrl+Shift+W` | `Cmd+W` | Close pane / tab |
| `Ctrl+Tab` | `Ctrl+Tab` | Next tab |
| `Ctrl+Shift+Tab` | `Ctrl+Shift+Tab` | Previous tab |
| `Ctrl+1`..`Ctrl+9` | `Cmd+1`..`Cmd+9` | Jump to tab by index |

### Panes

| Shortcut | macOS | Action |
|---|---|---|
| `Ctrl+Shift+D` | `Cmd+D` | Split horizontally |
| `Ctrl+Shift+E` | `Cmd+E` | Split vertically |
| `Alt+Arrow` | `Alt+Arrow` | Move focus between panes |
| `Alt+Shift+Arrow` | `Alt+Shift+Arrow` | Resize pane split ratio |

### Navigation

| Shortcut | Action |
|---|---|
| `Shift+PageUp/Down` | Scroll through scrollback |
| Mouse drag | Select text (auto-copies to clipboard) |
| Middle-click tab | Close tab |

### Search overlay

| Key | Action |
|---|---|
| Type | Filter results |
| `Up/Down` | Navigate results |
| `Enter` | Jump to selected result |
| `Escape` | Close search |

## Configuration

Glass reads `~/.glass/config.toml` and hot-reloads on changes.

```toml
# Font
font_family = "JetBrains Mono"
font_size = 14.0

# Shell override (auto-detected if omitted)
# shell = "pwsh"

# History
[history]
max_output_capture_kb = 50   # max output stored per command

# File snapshots
[snapshot]
enabled = true
max_count = 1000             # max snapshots retained
max_size_mb = 500            # max total snapshot storage
retention_days = 30          # auto-prune after N days

# Pipe visualization
[pipes]
enabled = true
max_capture_mb = 10          # max capture per pipe stage
auto_expand = true           # auto-expand on failure
```

Default fonts: Consolas (Windows), Menlo (macOS), Monospace (Linux).

## CLI

```bash
glass                        # Launch terminal
glass history search "query" # Search command history
glass history list           # List recent commands
glass history list --exit 1  # Filter by exit code
glass history list --after 1h --cwd /project
glass undo <command-id>      # Undo a command's file changes
glass mcp serve              # Start MCP server (for AI assistants)
```

## Multi-Agent Coordination

When multiple AI agents (Claude Code, Cursor, etc.) work on the same project in separate Glass tabs, Glass coordinates them through a shared SQLite database (`~/.glass/agents.db`). Each agent's MCP server connects to the same database, enabling:

- **Agent registry** -- agents register on session start and deregister on exit, so every agent knows who else is active
- **Advisory file locks** -- before editing a file, an agent claims it atomically; if another agent holds the lock, the request returns a conflict identifying the holder
- **Inter-agent messaging** -- agents can broadcast to all peers or send directed messages (e.g., requesting another agent to release a lock)
- **Status tracking** -- agents publish their current task so others can see what's in progress

All coordination is exposed as MCP tools (`glass_agent_register`, `glass_agent_lock`, `glass_agent_send`, etc.) -- no special client integration required. Any MCP-compatible agent can participate.

## MCP Tools for AI Agents

Glass exposes 25 MCP tools through `glass mcp serve`. Any MCP-compatible AI agent can use them.

| Category | Tools | Description |
|---|---|---|
| **History & Context** | `glass_history`, `glass_context` | Search command history, get session context |
| **Undo & Diffs** | `glass_undo`, `glass_file_diff` | Undo file changes, inspect pre-command file state |
| **Pipes** | `glass_pipe_inspect` | Inspect pipeline stage output |
| **Tab Orchestration** | `glass_tab_create`, `glass_tab_list`, `glass_tab_send`, `glass_tab_output`, `glass_tab_close` | Create, list, command, read, and close tabs |
| **Token Saving** | `glass_cache_check`, `glass_command_diff`, `glass_compressed_context` | Check cache validity, get file diffs, budget-aware context |
| **Error Extraction** | `glass_extract_errors` | Structured errors (file, line, column, message, severity) |
| **Live Awareness** | `glass_has_running_command`, `glass_cancel_command` | Check command status, cancel running commands |
| **Coordination** | `glass_agent_register`, `glass_agent_deregister`, `glass_agent_list`, `glass_agent_status`, `glass_agent_heartbeat`, `glass_agent_lock`, `glass_agent_unlock`, `glass_agent_locks`, `glass_agent_broadcast`, `glass_agent_send`, `glass_agent_messages` | Multi-agent coordination |
| **Health** | `glass_ping` | Verify MCP-to-GUI connection |

## Architecture

```
glass (binary)
  +-- glass_core       Config, events, update checker
  +-- glass_terminal   PTY management, grid snapshots, input encoding
  +-- glass_renderer   GPU rendering (wgpu, glyphon, rect pipeline)
  +-- glass_mux        Session multiplexer, tabs, split panes
  +-- glass_history    SQLite command history with FTS5
  +-- glass_snapshot   Filesystem snapshots, blob store, undo
  +-- glass_pipes      Pipeline parser and stage capture
  +-- glass_mcp        MCP server for AI assistant integration (25 tools)
  +-- glass_errors     Structured error extraction (Rust, generic parsers)
  +-- glass_coordination  Multi-agent coordination (locks, messaging, registry)
```

Built on [alacritty_terminal](https://github.com/alacritty/alacritty) for VT parsing and [wgpu](https://wgpu.rs/) for cross-platform GPU rendering.

## Performance

| Metric | Target | Measured |
|---|---|---|
| Cold start (first frame) | <500ms | ~520ms |
| Input latency (key to render) | <5ms | 3-7us |
| Idle memory | <120MB | ~89MB |

Run `cargo bench` for criterion benchmarks. Run `cargo run --release --features perf` for tracing output (view in [Perfetto](https://ui.perfetto.dev)).

## License

MIT

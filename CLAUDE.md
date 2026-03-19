# Glass - Claude Code Context

## What Is This
Glass is a GPU-accelerated terminal emulator built in Rust. It looks like a normal terminal but understands command structure — providing command-level undo, visual pipe debugging, and queryable history. MIT licensed.

## Architecture
Rust workspace with 14 crates + main binary:

```
src/main.rs              - App entry point, winit event loop, wires everything together
crates/glass_core/       - Config (TOML), events, config hot-reload watcher, update checker
crates/glass_errors/     - Centralized error types and structured error handling
crates/glass_terminal/   - PTY management, VT parsing (alacritty_terminal), block manager, OSC scanner, input encoding, shell integration
crates/glass_renderer/   - wgpu GPU rendering: grid, blocks, tab bar, status bar, search overlay, pipe visualization
crates/glass_mux/        - Session multiplexer: tabs, split panes (binary split tree), search overlay state
crates/glass_history/    - SQLite + FTS5 command history DB, query engine, retention/pruning
crates/glass_snapshot/   - Filesystem snapshots: file watcher, blob store (blake3 content-addressed), undo engine, command parser
crates/glass_pipes/      - Pipeline parsing (pipe detection, stage capture)
crates/glass_soi/        - Structured Output Intelligence: 19 format-specific parsers for command output
crates/glass_mcp/        - MCP server for AI tool integration (history, context, undo, file diff, pipe inspect)
crates/glass_coordination/ - Multi-agent coordination: agent registry, advisory file locks, inter-agent messaging (SQLite)
crates/glass_agent/      - Agent runtime: event-driven AI agent with MCP tool access
crates/glass_feedback/   - Feedback loop: LLM-based qualitative analysis, rule extraction, prompt hints
crates/glass_scripting/  - Self-improvement scripting: Rhai engine, hook system, sandbox, profiles
```

## Important: Orchestrator Reference
**Read `ORCHESTRATOR.md` for complete orchestrator architecture** — state machine, event flow, kickoff, silence detection, checkpoint synthesis, metric guard, feedback loop, file locations, config reference. Do NOT ask the user to re-explain orchestrator concepts documented there.

## Key Design Decisions
- **Orchestrator**: Silence-triggered feedback loop between Glass Agent (reviewer) and Claude Code (implementer). State machine in `src/orchestrator.rs`, event handling in `src/main.rs`, silence detection via `SilenceTracker` in `crates/glass_terminal/src/silence.rs`. Checkpoint cycle kills/respawns agent with fresh context.
- **VTE layer**: Embeds `alacritty_terminal` crate (pinned =0.25.1) — we don't rewrite terminal emulation
- **Shell integration**: OSC 133 sequences injected into bash/zsh/fish/PowerShell for command boundary detection
- **Rendering**: wgpu + glyphon for GPU text rendering, custom block-based UI on top of terminal grid
- **Platform PTY**: ConPTY on Windows, forkpty on Unix — abstracted in glass_terminal/pty.rs
- **Snapshots**: Content-addressed blob store with blake3 hashing, SQLite metadata DB
- **History**: Per-project SQLite DB with FTS5 full-text search

## Key Files
- `src/main.rs` (~2200 lines) - Event loop, window management, session creation, keyboard handling
- `crates/glass_terminal/src/block_manager.rs` - Command block lifecycle (PromptActive -> InputActive -> Executing -> Complete)
- `crates/glass_terminal/src/pty.rs` - PTY spawning, shell integration injection, glass_pty_loop
- `crates/glass_terminal/src/osc_scanner.rs` - OSC 133 parsing for shell events + pipeline events
- `crates/glass_renderer/src/frame.rs` - Frame composition (single-pane and multi-pane)
- `crates/glass_snapshot/src/command_parser.rs` - Destructive command detection (rm, mv, sed -i, etc.)
- `crates/glass_snapshot/src/undo.rs` - File restoration from snapshots
- `crates/glass_mux/src/session_mux.rs` - Tab/pane management
- `crates/glass_mux/src/split_tree.rs` - Binary tree pane layout
- `src/orchestrator.rs` - Orchestrator state machine, response parsing, checkpoint cycle, stuck detection, iteration logging
- `src/usage_tracker.rs` - OAuth usage polling, auto-pause/hard-stop thresholds
- `crates/glass_terminal/src/silence.rs` - Periodic silence detection for orchestrator polling
- `crates/glass_soi/src/lib.rs` - SOI parser registry and format detection
- `crates/glass_agent/src/lib.rs` - Agent runtime event loop and MCP tool dispatch
- `crates/glass_feedback/src/lib.rs` - Feedback loop lifecycle, rule extraction, prompt hints
- `crates/glass_scripting/src/engine.rs` - Rhai script execution engine with sandbox
- `crates/glass_scripting/src/hooks.rs` - Hook registry mapping scripts to lifecycle events

## Tech Stack
- Rust 2021 edition, Tokio async runtime
- wgpu 28.0, winit 0.30, glyphon 0.10
- alacritty_terminal 0.25.1 (pinned exact)
- rusqlite 0.38 (bundled SQLite with FTS5)
- blake3 for content-addressed hashing
- notify for cross-platform filesystem watching
- rmcp for MCP server
- clap 4.5 for CLI

## Build & Test
```bash
cargo build --release          # Build
cargo test --workspace         # Run all tests (~420 tests)
cargo fmt --all -- --check     # Check formatting
cargo clippy --workspace -- -D warnings  # Lint
cargo bench                    # Criterion benchmarks
cargo build --features perf    # Build with tracing instrumentation
```

## CI
GitHub Actions workflow at `.github/workflows/ci.yml` (on remote, not in local repo):
- Format check (ubuntu)
- Clippy (windows — matches dev platform)
- Build + test matrix: Linux (x86_64), macOS (aarch64), Windows (x86_64)

## Platform Notes
- **Windows** (primary dev platform): ConPTY, `windows-sys` for UTF-8 console code page, `escape_args` field in PTY options is `#[cfg(target_os = "windows")]` only
- **macOS**: Metal backend, FSEvents, forkpty
- **Linux**: Vulkan/OpenGL, inotify, forkpty, needs system deps (libxkbcommon, etc.)

## Shell Integration
Scripts in `shell-integration/` auto-injected by PTY spawner:
- `glass.bash`, `glass.zsh`, `glass.fish`, `glass.ps1`
- Emit OSC 133 sequences for prompt start, command start, command executed, command finished
- Pipeline stages emit custom OSC 133;S (start) and OSC 133;P (stage data)

## Configuration
`~/.glass/config.toml` — hot-reloaded via notify watcher. Sections: font, shell, history, snapshot, pipes, soi, agent (with orchestrator, permissions, quiet_rules sub-sections), scripting, terminal, theme. See `config.example.toml` for all fields with defaults.

## Conventions
- Clippy with `-D warnings` — all warnings are errors
- `cargo fmt` enforced in CI
- Tests live in same file as code (`#[cfg(test)] mod tests`)
- ConPTY-specific tests gated with `#[cfg(target_os = "windows")]`
- Feature flag `perf` enables tracing instrumentation

## Multi-Agent Coordination
Glass provides agent coordination through MCP tools when multiple AI agents work on the same project. All coordination data lives in `~/.glass/agents.db` (shared SQLite in WAL mode). Agents are scoped by project root path.

### Protocol
Follow these steps when operating as an AI agent in a Glass-managed project:

- **On session start:** Call `glass_agent_register` with your name, type (e.g. `claude-code`, `cursor`), and the project root path. This returns your agent ID for all subsequent calls.
- **Before editing files:** Call `glass_agent_lock` with the file paths you intend to edit. Locking is atomic and all-or-nothing -- if any file is held by another agent, the call returns a `Conflict` identifying the holder instead of acquiring any locks.
- **After editing files:** Call `glass_agent_unlock` to release advisory locks on files you are done editing, so other agents can work on them.
- **Periodically:** Call `glass_agent_messages` to check for messages from other agents (directed messages and broadcasts).
- **On lock conflict:** Use `glass_agent_send` with `msg_type` set to `request_unlock` to ask the lock holder to release the file.
- **When changing tasks:** Call `glass_agent_status` to update your status and current task description, so other agents can see what you are working on.
- **On session end:** Call `glass_agent_deregister` to clean up your agent registration and release all held locks.

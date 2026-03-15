# Architecture

**Analysis Date:** 2026-03-15

## Pattern Overview

**Overall:** Modular Rust workspace with layered separation of concerns.

**Key Characteristics:**
- 13-crate workspace + main binary for isolated domain responsibilities
- Silence-triggered feedback loop for autonomous agent orchestration
- Command lifecycle tracking via shell integration (OSC 133 sequences)
- Content-addressed file snapshots with SQLite metadata
- GPU-accelerated rendering pipeline (wgpu + glyphon)
- Session multiplexer for tabs and split panes
- Multi-agent coordination via shared SQLite database

## Layers

**Terminal Emulation Layer:**
- Purpose: PTY management, VT sequence parsing, shell integration event detection
- Location: `crates/glass_terminal/`
- Contains: PTY spawning (ConPTY on Windows, forkpty on Unix), alacritty_terminal embedding, OSC scanner, block manager
- Depends on: alacritty_terminal (pinned 0.25.1), glass_core for events
- Used by: Main event loop, snapshot system

**Session & Multiplexing Layer:**
- Purpose: Multi-session state management (tabs, split panes, search overlays)
- Location: `crates/glass_mux/`
- Contains: Session struct (owns PTY, terminal grid, block manager, history DB), SessionMux (tab/pane routing), SearchOverlay
- Depends on: glass_terminal, glass_history
- Used by: WindowContext in main.rs

**Rendering Layer:**
- Purpose: GPU text rendering, UI overlays, frame composition
- Location: `crates/glass_renderer/`
- Contains: wgpu surface binding, glyphon glyph cache, frame rendering pipeline, block/search/status bar overlays, tab bar
- Depends on: wgpu 28.0, glyphon 0.10, glass_terminal for grid snapshots
- Used by: WindowContext for per-frame drawing

**History & Database Layer:**
- Purpose: SQLite-backed command history with FTS5 search and SOI output classification
- Location: `crates/glass_history/`
- Contains: CommandRecord schema, HistoryDb CRUD, search/query filtering, output compression, SOI record storage
- Depends on: rusqlite (bundled SQLite), glass_soi for output classification
- Used by: Session, MCP tools, history CLI subcommands

**Snapshot & Undo Layer:**
- Purpose: Pre-command file snapshots and destructive command detection
- Location: `crates/glass_snapshot/`
- Contains: Content-addressed blob store (blake3), SnapshotDb metadata, destructive command parser, undo engine
- Depends on: blake3, rusqlite, notify for filesystem watching
- Used by: Session for per-command file tracking

**Structured Output Intelligence (SOI):**
- Purpose: Output classification and parsing into token-efficient summaries
- Location: `crates/glass_soi/`
- Contains: Output type classifiers (cargo, git, docker, etc.), severity extraction, one-line summary generation
- Depends on: ANSI parser, language-specific parsers
- Used by: glass_history, block manager for SOI hint lines

**Configuration & Events:**
- Purpose: Config hot-reload, event type definitions, version checking
- Location: `crates/glass_core/`
- Contains: TOML config schema, AppEvent enum (terminal, shell, SOI, agent, orchestrator), config watcher, update checker
- Depends on: serde, notify, semver
- Used by: All layers for event passing and config access

**Coordination & Agent Integration:**
- Purpose: Multi-agent file locking, agent registration, inter-agent messaging
- Location: `crates/glass_coordination/`
- Contains: CoordinationDb (global ~/.glass/agents.db), file locking, PID checking, agent registry
- Depends on: rusqlite
- Used by: MCP server, agent lifecycle management

**Worktree & Session Persistence:**
- Purpose: Isolated worktrees for agent proposals, session handoff records
- Location: `crates/glass_agent/`
- Contains: WorktreeManager (git worktrees or plain copies), AgentSessionDb (session continuity across restarts)
- Depends on: git2, glass_coordination
- Used by: Agent approval flow, checkpoint cycles

**Pipeline & Pipe Detection:**
- Purpose: Command pipeline parsing, stage capture, pipe debugging
- Location: `crates/glass_pipes/`
- Contains: Pipeline parser, CapturedStage structs, stage split logic
- Depends on: regex
- Used by: OSC scanner, block manager, renderer

**MCP Server:**
- Purpose: Claude AI assistant integration via Model Context Protocol
- Location: `crates/glass_mcp/`
- Contains: MCP tools (history query, context summary, undo, file diff inspection), stdio transport
- Depends on: rmcp, glass_history, glass_snapshot, glass_coordination
- Used by: External AI assistants via stdio

**Error Extraction:**
- Purpose: Structured error parsing from compiler output
- Location: `crates/glass_errors/`
- Contains: Language-specific parsers (Rust JSON/human, C++ compiler, generic), error structs
- Depends on: serde
- Used by: SOI, proposal overlays

**Main Event Loop & Orchestrator:**
- Purpose: Window lifecycle, event routing, UI interaction, orchestrator state machine
- Location: `src/main.rs`, `src/orchestrator.rs`, `src/usage_tracker.rs`
- Contains: Processor impl of ApplicationHandler, session management, keyboard/mouse routing, orchestrator feedback loop
- Depends on: winit 0.30, all crates above
- Used by: Entry point only

## Data Flow

**Terminal Output Pipeline:**

1. PTY reader thread reads from ConPTY/forkpty
2. alacritty_terminal parses VT sequences into grid updates
3. OscScanner intercepts OSC 133 (shell events) and OSC 133;P (pipeline stages)
4. Shell/Pipeline events fire AppEvent variants
5. Main thread AppEvent handler routes to session block manager
6. Block manager updates block state, fires snapshot trigger if needed
7. Snapshot store creates pre-exec file snapshot on CommandExecuted
8. Output captured between CommandExecuted and CommandFinished
9. CommandFinished event + output → history DB insert
10. SOI worker thread parses output → AppEvent::SoiReady
11. SoiReady handler updates block's soi_summary field
12. Frame renderer reads grid + blocks + overlays → renders next frame

**Orchestrator Feedback Loop:**

1. SilenceTracker (in glass_terminal/silence.rs) detects silence > threshold
2. OrchestratorSilence event fires to main thread
3. Processor captures terminal context snapshot + shell history context
4. Glass Agent (subprocess) receives context, generates proposal or response
5. AgentResponse::TypeText → keyboard input to PTY
6. AgentResponse::Checkpoint → checkpoint cycle (kill/respawn agent with fresh context)
7. AgentResponse::Wait → reset silence timer, continue polling
8. Usage tracker polls 5h utilization → triggers pause/hard-stop/resume events

**State Management:**

- **Terminal Grid:** Owned by alacritty_terminal::Term in Session, modified by PTY reader thread
- **Block Lifecycle:** BlockManager in Session tracks PromptActive → InputActive → Executing → Complete
- **Command History:** HistoryDb persists to `.glass/history.db` (or `~/.glass/global-history.db`)
- **Snapshots:** SnapshotStore combines SnapshotDb metadata with BlobStore content-addressed blobs
- **Search Overlay:** SearchOverlay state in Session, results queried from HistoryDb on every keystroke
- **Session Focus:** SessionMux routes keyboard/mouse to focused session

## Key Abstractions

**Block:**
- Purpose: Represents one prompt-command-output cycle
- Examples: `crates/glass_terminal/src/block_manager.rs` lines 24-61
- Pattern: Tracks state (PromptActive/InputActive/Executing/Complete), line ranges, exit code, timing, pipeline stages, SOI summary

**Session:**
- Purpose: All state for one terminal session (tab or split pane)
- Examples: `crates/glass_mux/src/session.rs` lines 25-67
- Pattern: Owns PTY sender, Term<EventProxy>, BlockManager, StatusState, HistoryDb, SnapshotStore, SearchOverlay

**AppEvent:**
- Purpose: Cross-thread event routing from PTY reader, SOI worker, config watcher, agent subprocess
- Examples: `crates/glass_core/src/event.rs` lines 67-168
- Pattern: Enum variants for TerminalDirty, Shell, GitInfo, CommandOutput, ConfigReloaded, SoiReady, AgentProposal, OrchestratorResponse, VerifyComplete, Usage events

**FrameRenderer:**
- Purpose: Orchestrates GPU rendering pipeline
- Examples: `crates/glass_renderer/src/frame.rs` lines 37-55
- Pattern: Owns GlyphCache, GridRenderer, RectRenderer, BlockRenderer, SearchOverlayRenderer, StatusBar/TabBar renderers

**SnapshotStore:**
- Purpose: Wraps SnapshotDb metadata + BlobStore blobs for atomic file snapshots
- Examples: `crates/glass_snapshot/src/lib.rs` lines 28-80
- Pattern: create_snapshot() → store_file() → update_command_id() handles pre-exec file capture

**CoordinationDb:**
- Purpose: Global agent registry and file locking
- Examples: `crates/glass_coordination/src/lib.rs`
- Pattern: All agents register in ~/.glass/agents.db, acquire/release file locks atomically, check for conflicts

## Entry Points

**GUI Application:**
- Location: `src/main.rs` line 7167
- Triggers: Executable invocation with no arguments
- Responsibilities: Create event loop, spawn first session, enter Processor event handler loop, dispatch winit/AppEvent variants

**History CLI:**
- Location: `src/main.rs` lines 50-123 (CLI structs), `src/history.rs` (handlers)
- Triggers: `glass history search/list` subcommands
- Responsibilities: Query HistoryDb with filters, format results, output to stdout

**MCP Server:**
- Location: `src/main.rs` lines 112-114, `crates/glass_mcp/src/lib.rs`
- Triggers: `glass mcp serve` subcommand
- Responsibilities: Start rmcp server over stdio, expose history/context/undo/diff tools

**Undo CLI:**
- Location: `src/main.rs` lines 69-73
- Triggers: `glass undo <command_id>` subcommand
- Responsibilities: Load UndoEngine from snapshot store, restore files from snapshot

## Error Handling

**Strategy:** Result-based error propagation with contextual logging.

**Patterns:**
- PTY spawn failures → log error, exit process
- History DB schema mismatch → auto-migrate or recreate
- Snapshot restore failures → log warning, return partial results
- Config parse error → use GlassConfig::default(), emit ConfigReloaded event with error flag
- SOI parse timeout → emit SoiReady with generic "parse timeout" message
- Orchestrator stuck detection → checkpoint (kill/respawn agent)
- File lock conflict → return LockConflict with lock holder info

## Cross-Cutting Concerns

**Logging:** tracing crate with stderr output, debug builds enable detailed terminal emulation logging

**Validation:** CommandRecord schema validation in HistoryDb, shell integration OSC scanner validates sequence format

**Authentication:** Multi-agent coordination uses advisory file locks (sqlite WAL mode with PID checking), not cryptographic auth

**Concurrency:**
- PTY reader thread: polling-based event loop, sends via mpsc channel to main thread
- SOI worker thread: async Tokio task, sends via EventLoopProxy
- Config watcher thread: notify-based filesystem events, sends AppEvent::ConfigReloaded
- Coordination poller thread: background SQLite polling, sends AppEvent::CoordinationUpdate
- Agent subprocess: managed via child process handle, communicates via stderr/stdout markers
- Main thread: Tokio runtime with winit event loop, Mutex<Term> for grid mutations during rendering

**Platform Abstraction:**
- PTY layer: `crates/glass_terminal/src/pty.rs` abstracts ConPTY (Windows) vs forkpty (Unix)
- Shell integration scripts: `shell-integration/` contains glass.bash/zsh/fish/ps1, auto-injected by PTY spawner
- File watching: notify crate (cross-platform) for snapshot filesystem watcher
- Config paths: glass_mux::platform::{config_dir, data_dir, default_shell} resolve platform-specific paths


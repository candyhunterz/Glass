# Architecture

**Analysis Date:** 2026-03-08

## Pattern Overview

**Overall:** Event-driven GUI application with a Rust workspace of 8 crates, using winit for windowing and wgpu for GPU-accelerated rendering. The architecture follows a layered crate separation where `src/main.rs` acts as the orchestrator, wiring together terminal emulation, rendering, session management, history, and snapshot subsystems.

**Key Characteristics:**
- Monolithic `main.rs` (2537 lines) implements `winit::ApplicationHandler` and all event routing
- Crate boundaries enforce dependency direction: `glass_core` at the bottom, `glass_mux` and `glass_renderer` in the middle, binary at the top
- PTY I/O runs on dedicated threads; events flow back to the main thread via `EventLoopProxy<AppEvent>`
- Terminal state is shared via `Arc<FairMutex<Term>>` with a snapshot-then-render pattern to minimize lock hold time
- Shell integration via OSC escape sequences drives the command lifecycle (blocks, history, snapshots)

## Layers

**Core (`glass_core`):**
- Purpose: Shared types, configuration, and cross-cutting concerns
- Location: `crates/glass_core/src/`
- Contains: `GlassConfig` (TOML config), `AppEvent` enum (all inter-thread events), `SessionId`, `ShellEvent`, `ConfigError`, config file watcher, update checker
- Depends on: `winit` (for `WindowId` in `AppEvent`), `serde`/`toml`, `notify` (file watcher), `dirs`
- Used by: Every other crate

**Terminal (`glass_terminal`):**
- Purpose: PTY management, terminal grid operations, shell integration parsing
- Location: `crates/glass_terminal/src/`
- Contains: `spawn_pty()` (PTY creation + reader thread), `EventProxy` (bridges PTY events to winit), `OscScanner` (parses OSC 133/7/9 sequences), `BlockManager` (command lifecycle tracking), `GridSnapshot` (lock-free rendering data), `OutputBuffer` (captures command output), `encode_key()` (keyboard input encoding)
- Depends on: `glass_core`, `glass_pipes`, `alacritty_terminal`, `polling`, `vte`
- Used by: `glass_renderer`, `glass_mux`, binary

**Renderer (`glass_renderer`):**
- Purpose: GPU rendering pipeline via wgpu + glyphon
- Location: `crates/glass_renderer/src/`
- Contains: `GlassRenderer` (wgpu surface/device/queue), `FrameRenderer` (orchestrates full render pipeline), `GlyphCache` (glyphon text rendering), `GridRenderer` (terminal cell metrics), `RectRenderer` (instanced quad pipeline for backgrounds), `BlockRenderer` (command block labels), `StatusBarRenderer`, `TabBarRenderer`, `SearchOverlayRenderer`, `ConfigErrorOverlay`
- Depends on: `glass_core`, `glass_terminal`, `glass_pipes`, `wgpu`, `glyphon`, `bytemuck`
- Used by: Binary

**Multiplexer (`glass_mux`):**
- Purpose: Session management for tabs and split panes
- Location: `crates/glass_mux/src/`
- Contains: `Session` (per-terminal state: PTY sender, term, block manager, history DB, snapshot store), `SessionMux` (manages sessions organized into tabs), `Tab` (holds `SplitNode` tree), `SplitNode` (binary tree layout engine), `ViewportLayout` (pixel rect computation), `SearchOverlay` (history search UI state), platform helpers (`default_shell`, `config_dir`, shortcut detection)
- Depends on: `glass_core`, `glass_terminal`, `glass_history`, `glass_snapshot`, `alacritty_terminal`
- Used by: Binary

**History (`glass_history`):**
- Purpose: SQLite-backed command history with FTS5 full-text search
- Location: `crates/glass_history/src/`
- Contains: `HistoryDb` (SQLite operations), `CommandRecord`, `QueryFilter`, `SearchResult`, `RetentionPolicy`, output processing (ANSI stripping, binary detection), `resolve_db_path()` (project-local `.glass/history.db` with global fallback)
- Depends on: `rusqlite`, `dirs`, `chrono`
- Used by: `glass_mux`, `glass_mcp`, binary

**Snapshot (`glass_snapshot`):**
- Purpose: Content-addressed file snapshots for command undo
- Location: `crates/glass_snapshot/src/`
- Contains: `SnapshotStore` (high-level API), `BlobStore` (content-addressed blob storage using blake3 hashes), `SnapshotDb` (SQLite metadata), `CommandParser` (predicts which files a command will modify), `IgnoreRules`, `Pruner` (retention enforcement), `UndoEngine` (restores pre-command state), `FsWatcher` (filesystem change tracking during command execution)
- Depends on: `rusqlite`, `blake3`, `dirs`, `shlex`
- Used by: `glass_mux`, `glass_mcp`, binary

**Pipes (`glass_pipes`):**
- Purpose: Pipeline-aware command parsing
- Location: `crates/glass_pipes/src/`
- Contains: `parse_pipeline()`, `split_pipes()`, `CapturedStage` and related types
- Depends on: `shlex`
- Used by: `glass_terminal`, `glass_renderer`

**MCP (`glass_mcp`):**
- Purpose: Model Context Protocol server for AI assistant integration
- Location: `crates/glass_mcp/src/`
- Contains: `GlassServer` (MCP tool provider), `run_mcp_server()` (stdio JSON-RPC transport), tools: `GlassHistory`, `GlassContext`, `GlassUndo`, `GlassFileDiff`
- Depends on: `glass_history`, `glass_snapshot`, `rmcp`, `tokio`
- Used by: Binary (via `glass mcp serve` subcommand)

## Crate Dependency Graph

```
binary (src/main.rs)
├── glass_core
├── glass_terminal ──> glass_core, glass_pipes
├── glass_renderer ──> glass_core, glass_terminal, glass_pipes
├── glass_mux ──────> glass_core, glass_terminal, glass_history, glass_snapshot
├── glass_history
├── glass_snapshot
├── glass_pipes
└── glass_mcp ──────> glass_history, glass_snapshot

Leaf crates (no glass_* deps): glass_core, glass_history, glass_snapshot, glass_pipes
```

## Data Flow

**PTY Read -> Render Flow:**

1. `spawn_pty()` in `crates/glass_terminal/src/pty.rs` creates a PTY and spawns a dedicated reader thread
2. Reader thread polls PTY fd using `polling::Poller`, reads bytes into buffer
3. `OscScanner` scans raw bytes for OSC sequences, emitting `ShellEvent` via `EventProxy`
4. `OutputBuffer` accumulates command output bytes between `CommandExecuted` and `CommandFinished`
5. Raw bytes are fed to `alacritty_terminal::Term` via `term.lock()` + `ansi::Processor`
6. `EventProxy` sends `AppEvent::TerminalDirty` / `AppEvent::Shell` / `AppEvent::CommandOutput` to winit event loop
7. `Processor::user_event()` in `src/main.rs` handles the `AppEvent`, routes to correct `Session` via `SessionMux`
8. On `RedrawRequested`: `snapshot_term()` copies renderable data under brief lock, then `FrameRenderer::draw_frame()` renders without holding the lock

**Command Lifecycle (Shell Integration):**

1. Shell emits OSC 133;A (PromptStart) -> `BlockManager` creates new `Block` in `PromptActive` state
2. OSC 133;B (CommandStart) -> Block transitions to `InputActive`
3. OSC 133;C (CommandExecuted) -> Block transitions to `Executing`, timer starts, `FsWatcher` starts, pre-exec snapshot taken
4. OSC 133;D;{exit_code} (CommandFinished) -> Block transitions to `Complete`, duration recorded, output captured, history record inserted, post-exec snapshot comparison, `FsWatcher` stopped

**Keyboard Input Flow:**

1. `WindowEvent::KeyboardInput` in `Processor::window_event()`
2. Glass shortcuts intercepted (Ctrl+Shift+T/W/Tab for tabs, Ctrl+Shift+D/arrows for splits, Ctrl+Shift+F for search, etc.)
3. Non-shortcut keys: `encode_key()` converts to VT escape sequences
4. Encoded bytes sent via `session.pty_sender.send(PtyMsg::Input(bytes))`
5. PTY reader thread writes bytes to PTY fd

**State Management:**
- Per-window: `WindowContext` holds `GlassRenderer`, `FrameRenderer`, `SessionMux`
- Per-session: `Session` holds `PtySender`, `Arc<FairMutex<Term>>`, `BlockManager`, `StatusState`, `HistoryDb`, `SnapshotStore`
- Application-wide: `Processor` holds `HashMap<WindowId, WindowContext>`, `GlassConfig`, `EventLoopProxy`
- Terminal grid state: `Arc<FairMutex<Term>>` shared between PTY reader thread and main thread

## Key Abstractions

**AppEvent (event bus):**
- Purpose: All inter-thread communication funnels through `winit::EventLoop<AppEvent>`
- Definition: `crates/glass_core/src/event.rs`
- Variants: `TerminalDirty`, `SetTitle`, `TerminalExit`, `Shell`, `GitInfo`, `CommandOutput`, `ConfigReloaded`, `UpdateAvailable`
- Pattern: PTY threads, git query threads, config watcher thread, and update checker thread all send `AppEvent` via `EventLoopProxy`; main thread handles in `Processor::user_event()`

**Session:**
- Purpose: Encapsulates all per-terminal state (PTY, grid, blocks, history, snapshots)
- Definition: `crates/glass_mux/src/session.rs`
- Pattern: Created by `create_session()` in `src/main.rs`, owned by `SessionMux`, identified by `SessionId`

**Block:**
- Purpose: Represents one prompt-command-output cycle with lifecycle state machine
- Definition: `crates/glass_terminal/src/block_manager.rs`
- States: `PromptActive` -> `InputActive` -> `Executing` -> `Complete`
- Pattern: `BlockManager` tracks a `Vec<Block>`, updated by OSC events

**GridSnapshot:**
- Purpose: Lock-free rendering data extracted from terminal grid
- Definition: `crates/glass_terminal/src/grid_snapshot.rs`
- Pattern: `snapshot_term()` copies all renderable cells under a brief `FairMutex` lock, then rendering proceeds without the lock

**SplitNode:**
- Purpose: Binary tree representing pane layout within a tab
- Definition: `crates/glass_mux/src/split_tree.rs`
- Pattern: `Leaf(SessionId)` or `Split { direction, left, right, ratio }`. `compute_layout()` recursively computes pixel rects.

## Entry Points

**GUI Terminal (default):**
- Location: `src/main.rs` -> `fn main()` (line 2384)
- Triggers: Running `glass` with no subcommand
- Responsibilities: Parse config, create winit event loop, create `Processor`, run event loop. `Processor::resumed()` creates window, GPU surface, PTY, initial session.

**CLI History:**
- Location: `src/main.rs` -> `Commands::History` branch, dispatches to `src/history.rs` -> `run_history()`
- Triggers: `glass history [list|search]`
- Responsibilities: Open history DB, query with filters, format output to stdout

**CLI Undo:**
- Location: `src/main.rs` -> `Commands::Undo` branch (line 2470)
- Triggers: `glass undo <command_id>`
- Responsibilities: Open snapshot store, execute undo via `UndoEngine`, print results

**MCP Server:**
- Location: `src/main.rs` -> `Commands::Mcp` branch -> `glass_mcp::run_mcp_server()`
- Triggers: `glass mcp serve`
- Responsibilities: Start tokio runtime, launch MCP server over stdio (JSON-RPC 2.0)

## Error Handling

**Strategy:** Graceful degradation. Optional subsystems (history, snapshots, git status) fail with warnings and disable themselves rather than crashing.

**Patterns:**
- PTY spawn failures: `expect()` (fatal, cannot run without a terminal)
- GPU init failures: `expect()` (fatal, cannot render without GPU)
- History DB open: `match` with `Ok/Err`, `None` disables history for that session
- Snapshot store open: `match` with `Ok/Err`, `None` disables snapshots for that session
- Config parse errors: Stored as `Option<ConfigError>`, displayed as red overlay banner, falls back to defaults
- Surface errors (Lost/Outdated): Silently skip frame, request new surface next frame
- Background thread panics: Caught at thread join boundaries

## Cross-Cutting Concerns

**Logging:** `tracing` crate with `tracing-subscriber` EnvFilter. `RUST_LOG` env var controls verbosity. Optional `perf` feature enables Chrome trace output (`glass-trace.json`). Performance metrics logged at startup (`PERF cold_start`, `PERF memory_physical`).

**Configuration:** TOML file at `~/.glass/config.toml`, loaded via `GlassConfig::load()` in `crates/glass_core/src/config.rs`. Hot-reloaded via filesystem watcher (`crates/glass_core/src/config_watcher.rs`). Validation errors shown as overlay, defaults used on failure.

**Shell Integration:** OSC escape sequences emitted by shell scripts in `shell-integration/` (bash, zsh, fish, PowerShell). Auto-injected at session startup by `find_shell_integration()` in `src/main.rs` (line 2324). Parsed by `OscScanner` in PTY reader thread.

**Data Storage:** Project-local `.glass/` directory (walked up from CWD) with global `~/.glass/` fallback. Contains `history.db` (SQLite), `snapshots.db` (SQLite), `blobs/` (content-addressed files).

---

*Architecture analysis: 2026-03-08*

# Codebase Structure

**Analysis Date:** 2026-03-15

## Directory Layout

```
Glass/
├── src/                                    # Main binary crate
│   ├── main.rs                            # GUI entry point, event loop, UI state (7655 lines)
│   ├── orchestrator.rs                    # Orchestrator state machine, verify baseline tracking (1127 lines)
│   ├── usage_tracker.rs                   # OAuth usage polling, pause/hard-stop thresholds
│   ├── history.rs                         # CLI handlers for history/undo/mcp subcommands
│   └── tests.rs                           # Integration tests
│
├── crates/                                 # Modular workspace crates
│   ├── glass_terminal/                    # PTY & shell integration (11 modules)
│   │   ├── src/
│   │   │   ├── lib.rs                     # Public exports
│   │   │   ├── pty.rs                     # PTY spawning, reader thread loop (586 lines)
│   │   │   ├── block_manager.rs           # Block lifecycle tracking (920 lines)
│   │   │   ├── osc_scanner.rs             # OSC 133 parsing (shell events + pipeline)
│   │   │   ├── event_proxy.rs             # Bridges PTY thread to winit event loop
│   │   │   ├── grid_snapshot.rs           # Terminal grid capture for rendering
│   │   │   ├── output_capture.rs          # CommandExecuted→CommandFinished output buffer
│   │   │   ├── input.rs                   # Keyboard input encoding for PTY
│   │   │   ├── silence.rs                 # SilenceTracker for orchestrator polling
│   │   │   ├── status.rs                  # Status bar state (CWD, git branch)
│   │   │   └── tests.rs                   # PTY and block manager tests
│   │
│   ├── glass_mux/                         # Session multiplexer (tabs, splits, search)
│   │   ├── src/
│   │   │   ├── lib.rs                     # Public exports
│   │   │   ├── session.rs                 # Single session struct
│   │   │   ├── session_mux.rs             # Multiple sessions, tab/pane routing
│   │   │   ├── split_tree.rs              # Binary tree pane layout
│   │   │   ├── tab.rs                     # Tab metadata
│   │   │   ├── search_overlay.rs          # Search history UI state
│   │   │   ├── layout.rs                  # Viewport layout calculations
│   │   │   ├── types.rs                   # SessionId, TabId, focus/split types
│   │   │   └── platform.rs                # Cross-platform shell/config detection
│   │
│   ├── glass_renderer/                    # wgpu GPU rendering (17 modules)
│   │   ├── src/
│   │   │   ├── lib.rs                     # Public exports
│   │   │   ├── frame.rs                   # FrameRenderer orchestration (2615 lines)
│   │   │   ├── surface.rs                 # wgpu surface lifecycle
│   │   │   ├── grid_renderer.rs           # Render terminal grid cells
│   │   │   ├── glyph_cache.rs             # glyphon font system + caching
│   │   │   ├── block_renderer.rs          # Draw block labels/timing
│   │   │   ├── rect_renderer.rs           # Draw cell background rects
│   │   │   ├── search_overlay_renderer.rs # Draw search results UI
│   │   │   ├── scrollbar.rs               # Scrollbar rendering + hit detection
│   │   │   ├── status_bar.rs              # Status bar (CWD, git, command timing)
│   │   │   ├── tab_bar.rs                 # Tab labels + close buttons
│   │   │   ├── proposal_overlay_renderer.rs  # Agent proposal UI
│   │   │   ├── proposal_toast_renderer.rs    # Toast notifications
│   │   │   ├── activity_overlay.rs           # Activity/coordination overlay
│   │   │   ├── config_error_overlay.rs       # Config parse error display
│   │   │   └── conflict_overlay.rs           # File lock conflict display
│   │
│   ├── glass_history/                     # SQLite command history
│   │   ├── src/
│   │   │   ├── lib.rs                     # Public exports
│   │   │   ├── db.rs                      # CommandRecord, HistoryDb CRUD
│   │   │   ├── query.rs                   # QueryFilter (exit code, time, cwd, limit)
│   │   │   ├── search.rs                  # FTS5 full-text search
│   │   │   ├── compress.rs                # Diff compression, token budget
│   │   │   ├── output.rs                  # Output processing (ANSI strip, binary detect)
│   │   │   ├── retention.rs               # Pruning old records
│   │   │   ├── config.rs                  # HistoryConfig from TOML
│   │   │   ├── soi.rs                     # SOI record storage + retrieval
│   │   │   └── tests.rs
│   │
│   ├── glass_snapshot/                    # File snapshot & undo system
│   │   ├── src/
│   │   │   ├── lib.rs                     # SnapshotStore public API
│   │   │   ├── db.rs                      # SnapshotDb schema + CRUD
│   │   │   ├── blob_store.rs              # blake3 content-addressed blobs
│   │   │   ├── command_parser.rs          # Destructive command detection (rm, sed -i, etc.)
│   │   │   ├── undo.rs                    # UndoEngine file restoration
│   │   │   ├── watcher.rs                 # FsWatcher (notify-based) for live tracking
│   │   │   ├── ignore_rules.rs            # .glassignore pattern matching
│   │   │   ├── types.rs                   # Confidence, FileOutcome enums
│   │   │   ├── pruner.rs                  # Snapshot retention cleanup
│   │   │   └── tests.rs
│   │
│   ├── glass_soi/                         # Structured Output Intelligence
│   │   ├── src/
│   │   │   ├── lib.rs                     # classify(), parse() entry points
│   │   │   ├── classifier.rs              # Output type detection
│   │   │   ├── types.rs                   # OutputType, ParsedOutput
│   │   │   ├── ansi.rs                    # ANSI sequence stripping
│   │   │   ├── cargo_*.rs                 # Cargo build/test/misc parsers
│   │   │   ├── cpp_compiler.rs            # C++ compiler output parser
│   │   │   ├── docker.rs                  # Docker output parser
│   │   │   ├── generic_compiler.rs        # Generic compiler pattern matching
│   │   │   ├── git.rs                     # Git output parser
│   │   │   ├── csv_parser.rs              # CSV/structured data detection
│   │   │   └── tests.rs
│   │
│   ├── glass_core/                        # Config, events, background tasks
│   │   ├── src/
│   │   │   ├── lib.rs                     # Public exports
│   │   │   ├── event.rs                   # AppEvent enum (168 lines)
│   │   │   ├── config.rs                  # GlassConfig TOML schema
│   │   │   ├── config_watcher.rs          # Notify-based hot-reload
│   │   │   ├── agent_runtime.rs           # AgentProposalData, AgentHandoffData
│   │   │   ├── activity_stream.rs         # Activity log for coordination
│   │   │   ├── updater.rs                 # Version check polling
│   │   │   ├── coordination_poller.rs     # CoordinationUpdate events
│   │   │   ├── ipc.rs                     # IPC channel setup
│   │   │   ├── error.rs                   # ConfigError type
│   │   │   └── tests.rs
│   │
│   ├── glass_coordination/                # Multi-agent coordination
│   │   ├── src/
│   │   │   ├── lib.rs                     # CoordinationDb public API
│   │   │   ├── db.rs                      # SQLite global agents.db
│   │   │   ├── types.rs                   # AgentInfo, FileLock, LockConflict
│   │   │   ├── event_log.rs               # CoordinationEvent logging
│   │   │   ├── pid.rs                     # PID alive check
│   │   │   └── tests.rs
│   │
│   ├── glass_agent/                       # Worktree isolation & session persistence
│   │   ├── src/
│   │   │   ├── lib.rs                     # WorktreeManager, AgentSessionDb
│   │   │   ├── worktree_manager.rs        # Git worktree/copy creation
│   │   │   ├── worktree_db.rs             # Worktree metadata
│   │   │   ├── session_db.rs              # Session handoff persistence
│   │   │   ├── types.rs                   # PendingWorktree, WorktreeHandle
│   │   │   └── tests.rs
│   │
│   ├── glass_pipes/                       # Pipeline parsing & capture
│   │   ├── src/
│   │   │   ├── lib.rs                     # Public exports
│   │   │   ├── parser.rs                  # parse_pipeline(), split_pipes()
│   │   │   ├── types.rs                   # CapturedStage, PipelineInfo
│   │   │   └── tests.rs
│   │
│   ├── glass_mcp/                         # MCP server for AI assistants
│   │   ├── src/
│   │   │   ├── lib.rs                     # run_mcp_server() entry point
│   │   │   ├── tools.rs                   # Tool implementations
│   │   │   ├── context.rs                 # GlassContext builder
│   │   │   ├── ipc_client.rs              # IPC client for queries
│   │   │   └── tests.rs
│   │
│   └── glass_errors/                      # Structured error extraction
│       ├── src/
│       │   ├── lib.rs                     # extract_errors() entry point
│       │   ├── detect.rs                  # Auto-detect compiler type
│       │   ├── rust_human.rs              # Rust human-readable parser
│       │   ├── rust_json.rs               # Rust JSON parser
│       │   ├── generic.rs                 # Fallback pattern matcher
│       │   └── tests.rs
│
├── shell-integration/                     # Shell integration scripts
│   ├── glass.bash                        # Bash OSC 133 injection
│   ├── glass.zsh                         # Zsh OSC 133 injection
│   ├── glass.fish                        # Fish OSC 133 injection
│   └── glass.ps1                         # PowerShell OSC 133 injection
│
├── assets/                                # Static resources
│   └── icon.ico                           # Windows icon
│
├── benches/                               # Criterion benchmarks
│
├── tests/                                 # Integration tests
│   └── tests/
│       └── *.rs                           # E2E test suites
│
├── .planning/                             # GSD planning documents
│   ├── codebase/                          # Codebase analysis (this directory)
│   │   ├── ARCHITECTURE.md
│   │   ├── STRUCTURE.md
│   │   ├── CONVENTIONS.md
│   │   ├── TESTING.md
│   │   ├── STACK.md
│   │   ├── INTEGRATIONS.md
│   │   └── CONCERNS.md
│   ├── phases/                            # Granular phase planning
│   ├── milestones/                        # Milestone summaries
│   └── PROJECT.md
│
├── .glass/                                # Local Glass state
│   ├── history.db                        # Project command history
│   ├── snapshots.db                      # File snapshot metadata
│   ├── agents.db                         # Global agent coordination DB
│   └── blob/                              # Content-addressed file blobs
│
├── Cargo.toml                             # Workspace root manifest
├── Cargo.lock                             # Dependency lock file
├── build.rs                               # Build script (icon embedding on Windows)
│
├── CLAUDE.md                              # Project context for Claude
├── README.md                              # Project documentation
├── PRD.md                                 # Product requirements
├── SOI_AND_AGENT_MODE.md                 # Agent mode design
├── AGENT_COORDINATION_DESIGN.md          # Multi-agent coordination design
├── AGENT_MCP_FEATURES.md                 # MCP tools specification
│
└── .github/workflows/                     # CI configuration
    └── ci.yml                             # Format, clippy, build+test matrix
```

## Directory Purposes

**src/:**
- Purpose: Main executable crate, wires all subsystems together
- Contains: GUI event loop, CLI subcommands, orchestrator state machine
- Key files: `main.rs` (7655 lines), `orchestrator.rs` (1127 lines)

**crates/glass_terminal/:**
- Purpose: PTY abstraction and shell integration event detection
- Contains: ConPTY/forkpty wrappers, alacritty_terminal embedding, OSC 133 parsing
- Key files: `pty.rs`, `block_manager.rs`, `osc_scanner.rs`

**crates/glass_mux/:**
- Purpose: Session multiplexing for tabs and split panes
- Contains: Session struct (owns PTY, grid, history), SessionMux router, search overlay
- Key files: `session.rs`, `session_mux.rs`, `split_tree.rs`

**crates/glass_renderer/:**
- Purpose: GPU rendering pipeline with wgpu and glyphon
- Contains: Frame composition, grid rendering, overlay renderers, scrollbar/tab bar UI
- Key files: `frame.rs` (2615 lines), `surface.rs`, `grid_renderer.rs`

**crates/glass_history/:**
- Purpose: Command history persistence and search
- Contains: SQLite schema, FTS5 full-text search, output compression, SOI records
- Key files: `db.rs`, `query.rs`, `search.rs`, `soi.rs`

**crates/glass_snapshot/:**
- Purpose: File change tracking and undo system
- Contains: blake3 blob store, snapshot metadata DB, destructive command detection, undo engine
- Key files: `blob_store.rs`, `db.rs`, `command_parser.rs`, `undo.rs`

**crates/glass_soi/:**
- Purpose: Output classification and parsing
- Contains: Language-specific parsers (Cargo, Git, Docker, C++, etc.), ANSI handling
- Key files: `classifier.rs`, `cargo_test.rs`, `cargo_build.rs`

**crates/glass_core/:**
- Purpose: Configuration, event types, and background task management
- Contains: AppEvent enum, GlassConfig TOML schema, config watcher, updater, coordination poller
- Key files: `event.rs` (168 lines), `config.rs`, `config_watcher.rs`

**crates/glass_coordination/:**
- Purpose: Multi-agent coordination via shared SQLite
- Contains: Agent registration, file locking, inter-agent messaging
- Key files: `db.rs`, `types.rs`

**crates/glass_agent/:**
- Purpose: Isolated worktree management and session handoff
- Contains: Git worktree creation, agent session DB for continuity
- Key files: `worktree_manager.rs`, `session_db.rs`

**crates/glass_pipes/:**
- Purpose: Pipeline parsing and stage capture
- Contains: Pipe splitting, CapturedStage structs
- Key files: `parser.rs`, `types.rs`

**crates/glass_mcp/:**
- Purpose: MCP server for AI assistant integration
- Contains: History query tool, context summary tool, undo tool, file diff tool
- Key files: `lib.rs`, `tools.rs`

**crates/glass_errors/:**
- Purpose: Compiler error extraction
- Contains: Rust/C++/generic parser, structured error structs
- Key files: `lib.rs`, `rust_json.rs`, `rust_human.rs`

**shell-integration/:**
- Purpose: Shell integration scripts
- Contains: Bash, Zsh, Fish, PowerShell OSC 133 emission scripts
- Usage: Auto-injected into PTY by spawner

**assets/:**
- Purpose: Static resources
- Contains: Windows icon for executable
- Generated: No
- Committed: Yes

**benches/:**
- Purpose: Performance benchmarks
- Contains: Criterion benchmark suites
- Generated: Yes (binaries)
- Committed: No

**tests/:**
- Purpose: Integration tests
- Contains: E2E test harnesses
- Generated: Yes (binaries)
- Committed: No

**.planning/:**
- Purpose: GSD phase planning and codebase documentation
- Contains: Phase plans, milestone summaries, codebase analysis docs
- Generated: Yes (by GSD tools)
- Committed: Yes

**.glass/:**
- Purpose: Local Glass state (per-project or global)
- Contains: history.db (project), snapshots.db (project), agents.db (global), blob/ (global)
- Generated: Yes (auto-created by Glass)
- Committed: No

## Key File Locations

**Entry Points:**
- `src/main.rs:7167` - fn main() - GUI entry point
- `src/main.rs:50-123` - Cli/Commands/HistoryAction enums - CLI subcommands
- `crates/glass_mcp/src/lib.rs` - run_mcp_server() - MCP server entry point
- `src/history.rs` - History subcommand handlers

**Configuration:**
- `src/main.rs:163-186` - WindowContext struct definition
- `crates/glass_core/src/config.rs` - GlassConfig TOML schema
- `crates/glass_core/src/config_watcher.rs` - Hot-reload via notify

**Core Logic:**
- `crates/glass_terminal/src/pty.rs` - PTY spawning and reader loop
- `crates/glass_terminal/src/block_manager.rs` - Block lifecycle (PromptActive → Executing → Complete)
- `crates/glass_history/src/db.rs` - CommandRecord schema and queries
- `crates/glass_snapshot/src/blob_store.rs` - blake3 content-addressed storage
- `src/orchestrator.rs` - Orchestrator state machine (VerifyCommand, MetricBaseline, AgentResponse)

**Testing:**
- `src/tests.rs` - Main integration tests
- `crates/*/src/tests.rs` - Per-crate unit tests
- `tests/` directory - E2E test suites

**Rendering:**
- `crates/glass_renderer/src/frame.rs` - FrameRenderer orchestration
- `crates/glass_renderer/src/surface.rs` - wgpu surface binding
- `crates/glass_renderer/src/grid_renderer.rs` - Grid cell rendering

## Naming Conventions

**Files:**
- Modules use snake_case: `block_manager.rs`, `grid_snapshot.rs`, `config_watcher.rs`
- Tests in same file as code: `#[cfg(test)] mod tests`
- Main entry point: `main.rs` in binary crate, `lib.rs` in library crates

**Directories:**
- Workspace crates prefix with `glass_`: `glass_terminal`, `glass_renderer`, `glass_history`
- Nested module directories match module names: `src/block_manager/` for large modules (not used; kept flat)
- Hidden directories prefix with `.`: `.glass/`, `.planning/`, `.github/`

**Types:**
- Structs: PascalCase (`BlockManager`, `FrameRenderer`, `Session`)
- Enums: PascalCase (`AppEvent`, `BlockState`, `ShellEvent`)
- Constants: SCREAMING_SNAKE_CASE (`READ_BUFFER_SIZE`, `PTY_READ_WRITE_TOKEN`)
- Traits: PascalCase (`EventListener`)

**Functions:**
- Module-level: snake_case (`spawn_pty`, `encode_key`, `snapshot_term`)
- Methods: snake_case (`new()`, `create_snapshot()`, `resolve_db_path()`)
- Test functions: `#[test] fn test_*` (e.g., `test_resolve_db_path_project`)

**Variables:**
- Local: snake_case (`window_id`, `session_id`, `exit_code`)
- Mutable: same convention (`mut block_manager`)
- Lifetime parameters: 'a, 'b (lowercase single quote)

## Where to Add New Code

**New Feature (e.g., New Terminal Command):**
- Primary code: `crates/glass_terminal/src/` (PTY-related) or appropriate domain crate
- Tests: Same file as implementation, `#[cfg(test)] mod tests`
- CLI handler: `src/history.rs` (if CLI-facing) or `src/main.rs` event handler

**New Component/Module:**
- Implementation: Create `crates/glass_*/src/new_module.rs`, export from `lib.rs`
- Tests: Add `#[cfg(test)] mod tests` in same file
- Integration: Wire into appropriate handler (e.g., AppEvent handler in main.rs)

**New Event Type:**
- Definition: Add variant to `AppEvent` enum in `crates/glass_core/src/event.rs`
- Handler: Add arm to `Processor::handle_event()` in `src/main.rs:1537`
- Sender: Use `event_loop_proxy.send_event()` from background thread

**New Rendering Overlay:**
- Definition: Create `crates/glass_renderer/src/foo_renderer.rs`
- Struct: Implement `FooRenderer` with `new()` and `render()` methods
- Integration: Add to `FrameRenderer` struct, call from `draw_frame()`
- State: Add to `Session` or `WindowContext` as needed

**Utilities & Helpers:**
- Shared across crates: `crates/glass_core/src/` (if config/event-related) or new utility crate
- Single-crate: Within that crate's `src/` directory
- Formatting: `cargo fmt --all`, linting: `cargo clippy --workspace -- -D warnings`

**Database Schema Changes:**
- Location: `crates/glass_history/src/db.rs` (history) or `crates/glass_snapshot/src/db.rs` (snapshots)
- Pattern: Add column in schema, increment version, auto-migrate in `open()` or `create()`
- Testing: Add to crate's `#[cfg(test)]` module with temp database

## Special Directories

**src/:**
- Purpose: Main executable and orchestrator
- Generated: No (source)
- Committed: Yes

**crates/:*/src/**/**tests.rs:**
- Purpose: Unit tests co-located with code
- Generated: No (source, compiled as part of crate)
- Committed: Yes

**.planning/codebase/:**
- Purpose: Analysis documents (ARCHITECTURE.md, STRUCTURE.md, etc.)
- Generated: Yes (by GSD mapper tool)
- Committed: Yes

**.glass/:**
- Purpose: Local state (history DB, snapshots, agents registry)
- Generated: Yes (auto-created on first run)
- Committed: No (in .gitignore)

**target/:**
- Purpose: Build artifacts
- Generated: Yes (cargo build)
- Committed: No (in .gitignore)

**shell-integration/:**
- Purpose: OSC 133 shell scripts
- Generated: No (source)
- Committed: Yes
- Injection: Auto-injected into PTY by `crates/glass_terminal/src/pty.rs` at spawn time


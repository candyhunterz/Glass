# Codebase Structure

**Analysis Date:** 2026-03-08

## Directory Layout

```
Glass/
├── src/
│   ├── main.rs             # Binary entry point, winit event loop, all event handling (2537 lines)
│   ├── history.rs           # CLI history subcommand dispatch and formatting
│   └── tests.rs             # Integration tests for main binary
├── crates/
│   ├── glass_core/src/      # Shared types, config, events
│   │   ├── lib.rs
│   │   ├── config.rs        # GlassConfig (TOML deserialization)
│   │   ├── config_watcher.rs# Filesystem watcher for config hot-reload
│   │   ├── error.rs         # Error types
│   │   ├── event.rs         # AppEvent enum, SessionId, ShellEvent, GitStatus
│   │   └── updater.rs       # Background update checker
│   ├── glass_terminal/src/  # PTY management, terminal grid, shell integration
│   │   ├── lib.rs
│   │   ├── pty.rs           # spawn_pty(), PtyMsg, PtySender, reader thread
│   │   ├── event_proxy.rs   # EventProxy bridges PTY -> winit
│   │   ├── osc_scanner.rs   # OscScanner parses OSC 133/7/9 sequences
│   │   ├── block_manager.rs # Block lifecycle (PromptActive->Complete)
│   │   ├── grid_snapshot.rs # GridSnapshot for lock-free rendering
│   │   ├── input.rs         # encode_key() keyboard -> VT escape sequences
│   │   ├── output_capture.rs# OutputBuffer for capturing command output
│   │   ├── status.rs        # StatusState, query_git_status()
│   │   └── tests.rs         # Unit tests
│   ├── glass_renderer/src/  # GPU rendering pipeline
│   │   ├── lib.rs
│   │   ├── surface.rs       # GlassRenderer (wgpu surface/device/queue)
│   │   ├── frame.rs         # FrameRenderer (orchestrates full render pipeline)
│   │   ├── glyph_cache.rs   # GlyphCache (glyphon text rendering state)
│   │   ├── grid_renderer.rs # GridRenderer (cell metrics, text layout)
│   │   ├── rect_renderer.rs # RectRenderer (instanced quad pipeline)
│   │   ├── block_renderer.rs# BlockRenderer (command block labels/decorations)
│   │   ├── status_bar.rs    # StatusBarRenderer (bottom bar: CWD, git, memory)
│   │   ├── tab_bar.rs       # TabBarRenderer (top bar: tab titles)
│   │   ├── search_overlay_renderer.rs # Search overlay UI
│   │   └── config_error_overlay.rs    # Red banner for config parse errors
│   ├── glass_mux/src/       # Session multiplexer (tabs + split panes)
│   │   ├── lib.rs
│   │   ├── session.rs       # Session struct (per-terminal state)
│   │   ├── session_mux.rs   # SessionMux (tab management, focus routing)
│   │   ├── tab.rs           # Tab (holds SplitNode tree)
│   │   ├── split_tree.rs    # SplitNode (binary tree layout engine)
│   │   ├── layout.rs        # ViewportLayout (pixel rect computation)
│   │   ├── search_overlay.rs# SearchOverlay state (history search UI)
│   │   ├── platform.rs      # Cross-platform helpers (shell, dirs, shortcuts)
│   │   └── types.rs         # SessionId, TabId, SplitDirection, FocusDirection
│   ├── glass_history/src/   # SQLite command history with FTS5
│   │   ├── lib.rs           # resolve_db_path() (project-local + global fallback)
│   │   ├── db.rs            # HistoryDb (SQLite CRUD, schema migration)
│   │   ├── config.rs        # HistoryConfig
│   │   ├── query.rs         # QueryFilter, filtered_query(), parse_time()
│   │   ├── search.rs        # SearchResult type
│   │   ├── output.rs        # Output processing (ANSI stripping, binary detection)
│   │   └── retention.rs     # Retention policy enforcement
│   ├── glass_snapshot/src/  # Content-addressed file snapshots
│   │   ├── lib.rs           # SnapshotStore (high-level API), resolve_glass_dir()
│   │   ├── blob_store.rs    # BlobStore (blake3-hashed file storage)
│   │   ├── db.rs            # SnapshotDb (SQLite metadata)
│   │   ├── command_parser.rs# Predicts files a command will modify
│   │   ├── ignore_rules.rs  # .gitignore-style exclusion rules
│   │   ├── pruner.rs        # Pruner (retention: age, count, size limits)
│   │   ├── types.rs         # SnapshotRecord, Confidence, FileOutcome, etc.
│   │   ├── undo.rs          # UndoEngine (restore pre-command file state)
│   │   └── watcher.rs       # FsWatcher (filesystem change tracking)
│   ├── glass_pipes/src/     # Pipeline-aware command parsing
│   │   ├── lib.rs
│   │   ├── parser.rs        # parse_pipeline(), split_pipes()
│   │   └── types.rs         # CapturedStage and related types
│   └── glass_mcp/src/       # MCP server for AI assistants
│       ├── lib.rs           # run_mcp_server() (stdio JSON-RPC)
│       ├── tools.rs         # GlassServer, MCP tool implementations
│       └── context.rs       # Context/summary generation
├── shell-integration/       # Shell integration scripts (auto-injected)
│   ├── glass.bash           # Bash OSC 133 hooks
│   ├── glass.zsh            # Zsh OSC 133 hooks
│   ├── glass.fish           # Fish OSC 133 hooks
│   └── glass.ps1            # PowerShell OSC 133 hooks
├── tests/
│   └── mcp_integration.rs   # MCP server integration tests
├── benches/
│   └── perf_benchmarks.rs   # Criterion benchmarks
├── packaging/               # Platform-specific packaging
│   ├── homebrew/            # Homebrew formula
│   ├── linux/               # .desktop file, deb packaging
│   ├── macos/               # macOS bundle
│   └── winget/              # Windows Package Manager manifest
├── wix/                     # WiX MSI installer
├── docs/                    # Documentation
│   └── src/
│       ├── features/
│       └── installation/
├── Cargo.toml               # Workspace root + binary package manifest
└── .github/
    └── workflows/           # CI/CD workflows
```

## Directory Purposes

**`src/`:**
- Purpose: Binary crate (the `glass` executable)
- Contains: `main.rs` (event loop, window management, input handling, rendering orchestration, CLI dispatch), `history.rs` (history subcommand), `tests.rs`
- Key files: `src/main.rs` is the application orchestrator

**`crates/glass_core/src/`:**
- Purpose: Foundation crate with zero glass_* dependencies
- Contains: Configuration loading/validation, event types shared across all crates, config file watcher, update checker
- Key files: `event.rs` (defines `AppEvent` used everywhere), `config.rs` (defines `GlassConfig`)

**`crates/glass_terminal/src/`:**
- Purpose: All terminal emulation concerns
- Contains: PTY spawning, I/O polling, OSC parsing, command block tracking, grid snapshot extraction, keyboard encoding
- Key files: `pty.rs` (PTY lifecycle), `block_manager.rs` (command lifecycle), `osc_scanner.rs` (shell integration parser)

**`crates/glass_renderer/src/`:**
- Purpose: All GPU rendering concerns
- Contains: wgpu surface management, text rendering via glyphon, background rect rendering via instanced quads, UI chrome (tab bar, status bar, overlays)
- Key files: `frame.rs` (render orchestrator), `surface.rs` (GPU init), `grid_renderer.rs` (cell layout)

**`crates/glass_mux/src/`:**
- Purpose: Multi-session management
- Contains: Session state, tab management, split pane layout, platform detection
- Key files: `session.rs` (per-terminal state), `session_mux.rs` (tab/focus management), `split_tree.rs` (binary tree layout)

**`crates/glass_history/src/`:**
- Purpose: Persistent command history
- Contains: SQLite database operations, full-text search, query filtering, output processing
- Key files: `db.rs` (database operations), `query.rs` (filtering)

**`crates/glass_snapshot/src/`:**
- Purpose: File state capture for command undo
- Contains: Content-addressed blob storage, snapshot metadata, command parsing for file prediction, undo engine, filesystem watcher
- Key files: `lib.rs` (SnapshotStore API), `undo.rs` (UndoEngine), `command_parser.rs` (file prediction)

**`crates/glass_pipes/src/`:**
- Purpose: Pipeline command parsing
- Contains: Pipe splitting, pipeline stage types
- Key files: `parser.rs`, `types.rs`

**`crates/glass_mcp/src/`:**
- Purpose: AI assistant integration via MCP protocol
- Contains: MCP server, tool definitions (history, context, undo, file diff)
- Key files: `tools.rs` (tool implementations), `lib.rs` (server startup)

**`shell-integration/`:**
- Purpose: Shell scripts that emit OSC sequences for Glass to parse
- Contains: One script per supported shell (bash, zsh, fish, PowerShell)
- Auto-injected by `find_shell_integration()` in `src/main.rs` at session startup

## Key File Locations

**Entry Points:**
- `src/main.rs`: Binary entry, CLI parsing, event loop, all event handling
- `src/history.rs`: History CLI subcommand handler
- `crates/glass_mcp/src/lib.rs`: MCP server entry (`run_mcp_server()`)

**Configuration:**
- `crates/glass_core/src/config.rs`: `GlassConfig` struct definition and loading
- `crates/glass_core/src/config_watcher.rs`: Hot-reload watcher
- Runtime config: `~/.glass/config.toml`

**Core Logic:**
- `crates/glass_terminal/src/pty.rs`: PTY creation and reader thread
- `crates/glass_terminal/src/block_manager.rs`: Command lifecycle state machine
- `crates/glass_terminal/src/osc_scanner.rs`: Shell integration sequence parser
- `crates/glass_renderer/src/frame.rs`: Render pipeline orchestrator
- `crates/glass_mux/src/session_mux.rs`: Tab/session management
- `crates/glass_mux/src/split_tree.rs`: Split pane binary tree

**Data Storage:**
- `crates/glass_history/src/db.rs`: History database operations
- `crates/glass_snapshot/src/db.rs`: Snapshot metadata database
- `crates/glass_snapshot/src/blob_store.rs`: Content-addressed blob storage
- Runtime data: `.glass/history.db`, `.glass/snapshots.db`, `.glass/blobs/`

**Testing:**
- `crates/glass_terminal/src/tests.rs`: Terminal unit tests
- `tests/mcp_integration.rs`: MCP integration tests
- `benches/perf_benchmarks.rs`: Performance benchmarks
- Inline `#[cfg(test)] mod tests` blocks in most crate files

## Naming Conventions

**Files:**
- `snake_case.rs`: All Rust source files use snake_case
- `glass.{shell}`: Shell integration scripts named by shell type
- Crate names: `glass_{domain}` pattern (e.g., `glass_terminal`, `glass_renderer`)

**Directories:**
- `crates/glass_{name}/`: Each workspace crate follows `glass_` prefix convention
- `src/` inside each crate: Standard Rust layout

**Modules:**
- One concept per file: `session.rs`, `session_mux.rs`, `split_tree.rs`
- `lib.rs` re-exports public API via `pub use`
- Test modules: either `tests.rs` sibling or inline `#[cfg(test)] mod tests`

## Where to Add New Code

**New Feature (e.g., new terminal capability):**
- Primary code: Add module in relevant crate under `crates/glass_{domain}/src/`
- Register in `crates/glass_{domain}/src/lib.rs` with `pub mod` and `pub use`
- Wire into `src/main.rs` event handling if it produces/consumes `AppEvent`
- Tests: Add `#[cfg(test)] mod tests` block in the new module

**New Renderer Component (e.g., new overlay):**
- Implementation: `crates/glass_renderer/src/{name}_renderer.rs`
- Register: Add `pub mod` in `crates/glass_renderer/src/lib.rs`
- Integrate: Call from `FrameRenderer::draw_frame()` in `crates/glass_renderer/src/frame.rs`

**New CLI Subcommand:**
- Add variant to `Commands` enum in `src/main.rs` (line 47)
- Add match arm in `fn main()` (line 2400)
- For complex subcommands, create handler in `src/{name}.rs` and add `mod {name}` at top of `src/main.rs`

**New Crate:**
- Create `crates/glass_{name}/` with `Cargo.toml` and `src/lib.rs`
- Add to `[workspace]` members in root `Cargo.toml` (auto-included via `"crates/*"`)
- Add dependency in consuming crates' `Cargo.toml`

**New Shell Integration:**
- Add `shell-integration/glass.{shell}` script
- Add detection logic in `find_shell_integration()` in `src/main.rs` (line 2324)

**New MCP Tool:**
- Add tool method in `crates/glass_mcp/src/tools.rs`
- Register in `GlassServer` tool list

**Utilities / Shared Helpers:**
- Cross-crate types: `crates/glass_core/src/`
- Terminal-specific: `crates/glass_terminal/src/`
- Platform helpers: `crates/glass_mux/src/platform.rs`

## Special Directories

**`shell-integration/`:**
- Purpose: Shell scripts auto-injected into terminal sessions for OSC sequence emission
- Generated: No (hand-written)
- Committed: Yes

**`packaging/`:**
- Purpose: Platform-specific installer/package configurations
- Generated: No
- Committed: Yes

**`wix/`:**
- Purpose: WiX MSI installer definition for Windows
- Generated: No
- Committed: Yes

**`target/`:**
- Purpose: Cargo build output
- Generated: Yes
- Committed: No

**`.planning/`:**
- Purpose: Project planning and analysis documents
- Generated: Yes (by tooling)
- Committed: Yes

**`.glass/` (runtime, not in repo):**
- Purpose: Per-project data directory (history.db, snapshots.db, blobs/)
- Generated: Yes (at runtime)
- Committed: No (should be in .gitignore)

---

*Structure analysis: 2026-03-08*

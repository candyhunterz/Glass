# Codebase Structure

**Analysis Date:** 2026-03-18

## Directory Layout

```
Glass/
├── src/                          # Main binary crate (glass)
│   ├── main.rs                   # Event loop, window management, session creation, keyboard/mouse handling
│   ├── orchestrator.rs           # Orchestrator state machine, silence detection, proposal parsing, metric guard
│   ├── orchestrator_events.rs    # Event buffer for orchestrator transcript logging
│   ├── checkpoint_synth.rs       # Ephemeral agent for checkpoint synthesis
│   ├── ephemeral_agent.rs        # Spawn claude subprocess, timeout, JSON response parsing
│   ├── script_bridge.rs          # Integration point between Processor and ScriptSystem
│   ├── usage_tracker.rs          # OAuth usage polling, budget gates
│   └── history.rs                # CLI history subcommand handler
│
├── crates/
│   ├── glass_core/               # Configuration, events, core types
│   │   ├── src/config.rs         # GlassConfig (TOML), hot reload watcher
│   │   ├── src/event.rs          # AppEvent, SessionId, ShellEvent, EphemeralPurpose
│   │   ├── src/config_watcher.rs # File watcher for ~/.glass/config.toml
│   │   ├── src/agent_runtime.rs  # AgentRuntimeConfig, CooldownTracker, BudgetTracker
│   │   ├── src/coordination_poller.rs # Background thread polling agents.db
│   │   ├── src/updater.rs        # Check for newer Glass releases
│   │   ├── src/ipc.rs            # McpEventRequest type for IPC
│   │   └── src/activity_stream.rs # Activity event filtering and deduplication
│   │
│   ├── glass_terminal/           # PTY management and shell integration
│   │   ├── src/lib.rs            # Module exports
│   │   ├── src/pty.rs            # spawn_pty, ConPTY/forkpty, shell script injection
│   │   ├── src/block_manager.rs  # Block state machine (Prompt→Input→Exec→Complete)
│   │   ├── src/osc_scanner.rs    # Parse OSC 133 sequences, pipeline stage detection
│   │   ├── src/grid_snapshot.rs  # Snapshot alacritty_terminal grid with colors
│   │   ├── src/input.rs          # encode_key for keyboard input
│   │   ├── src/status.rs         # query_git_status, git branch/dirty detection
│   │   ├── src/event_proxy.rs    # EventProxy: PTY thread → AppEvent dispatcher
│   │   ├── src/silence.rs        # SilenceTracker: periodic silence detection for orchestrator
│   │   ├── src/output_capture.rs # Accumulate CommandOutput bytes between exec/finish
│   │   └── shell-integration/    # Shell integration scripts (auto-injected)
│   │       ├── glass.bash
│   │       ├── glass.zsh
│   │       ├── glass.fish
│   │       └── glass.ps1
│   │
│   ├── glass_renderer/           # GPU rendering via wgpu + glyphon
│   │   ├── src/lib.rs            # Module exports
│   │   ├── src/surface.rs        # GlassRenderer: wgpu surface management
│   │   ├── src/frame.rs          # FrameRenderer: frame composition, pane viewport layout
│   │   ├── src/grid_renderer.rs  # GridRenderer: render terminal grid to GPU
│   │   ├── src/block_renderer.rs # BlockRenderer: render block decorators (badges, labels)
│   │   ├── src/tab_bar.rs        # TabBarRenderer, tab drag/close hit testing
│   │   ├── src/status_bar.rs     # StatusBarRenderer: git branch, session info
│   │   ├── src/scrollbar.rs      # ScrollbarRenderer, hit detection for mouse drag
│   │   ├── src/glyph_cache.rs    # GlyphCache: glyphon integration
│   │   ├── src/rect_renderer.rs  # RectRenderer: solid color rectangles
│   │   ├── src/search_overlay_renderer.rs # Search UI overlay
│   │   ├── src/proposal_overlay_renderer.rs # Agent proposal review overlay
│   │   ├── src/proposal_toast_renderer.rs  # Toast notification for new proposals
│   │   ├── src/activity_overlay.rs # Activity stream dashboard
│   │   ├── src/conflict_overlay.rs # Agent file lock conflict display
│   │   ├── src/config_error_overlay.rs # Config parse error banner
│   │   └── src/settings_overlay.rs # Settings UI (font, shell, history limits)
│   │
│   ├── glass_mux/                # Session multiplexer, tabs, split panes
│   │   ├── src/lib.rs            # Module exports
│   │   ├── src/session_mux.rs    # SessionMux: tab + session management
│   │   ├── src/session.rs        # Session: PTY, grid, block manager, history DB
│   │   ├── src/tab.rs            # Tab: split tree root
│   │   ├── src/split_tree.rs     # SplitNode: binary tree pane layout
│   │   ├── src/layout.rs         # ViewportLayout: pane geometry (x, y, width, height)
│   │   ├── src/search_overlay.rs # SearchOverlay: search UI state
│   │   ├── src/types.rs          # SessionId, TabId, SplitDirection, FocusDirection
│   │   └── src/platform.rs       # Platform abstraction (shell detection, config dirs)
│   │
│   ├── glass_history/            # Command history with FTS5 search
│   │   ├── src/lib.rs            # Module exports, resolve_db_path
│   │   ├── src/db.rs             # HistoryDb: SQLite with FTS5, CommandRecord
│   │   ├── src/query.rs          # QueryFilter: time/exit code/cwd filters
│   │   ├── src/search.rs         # FTS5 full-text search
│   │   ├── src/output.rs         # Output record types for parsed command output
│   │   ├── src/compress.rs       # Output compression: diff vs. full, TokenBudget
│   │   ├── src/soi.rs            # SOI summary storage (one-line, token estimate, severity)
│   │   ├── src/config.rs         # HistoryConfig (retention, max output capture)
│   │   └── src/retention.rs      # Auto-pruning old records
│   │
│   ├── glass_snapshot/           # File snapshots and undo engine
│   │   ├── src/lib.rs            # Module exports, SnapshotStore, resolve paths
│   │   ├── src/db.rs             # SnapshotDb: SQLite metadata (command_id, file paths, blob hashes)
│   │   ├── src/blob_store.rs     # BlobStore: content-addressed files (blake3 hash)
│   │   ├── src/undo.rs           # UndoEngine: restore pre-command file state
│   │   ├── src/command_parser.rs # Parse command text, detect destructive commands (rm, mv, sed -i, etc.)
│   │   ├── src/watcher.rs        # FsWatcher: monitor filesystem for command execution
│   │   ├── src/types.rs          # SnapshotRecord, FileOutcome, ParseResult
│   │   ├── src/ignore_rules.rs   # .glassignore parsing
│   │   └── src/pruner.rs         # Retention policy (age, space)
│   │
│   ├── glass_pipes/              # Pipeline parsing and stage capture
│   │   ├── src/lib.rs            # Module exports
│   │   ├── src/parser.rs         # parse_pipeline, split_pipes (parse pipe stages)
│   │   └── src/types.rs          # CapturedStage (name, stdout/stderr bytes)
│   │
│   ├── glass_soi/                # Structured Output Intelligence
│   │   ├── src/lib.rs            # classify(), parse() dispatch
│   │   ├── src/classifier.rs     # OutputType detection (cargo, npm, pytest, git, docker, etc.)
│   │   ├── src/types.rs          # OutputType, OutputSummary, ParsedOutput, Severity
│   │   ├── src/ansi.rs           # Strip ANSI codes
│   │   ├── src/cargo_test.rs     # Rust test output parser
│   │   ├── src/cargo_build.rs    # Rust compiler output parser
│   │   ├── src/cargo_misc.rs     # Other cargo commands
│   │   ├── src/npm.rs            # Node package manager parser
│   │   ├── src/pytest.rs         # Python test parser
│   │   ├── src/jest.rs           # Jest test parser
│   │   ├── src/git.rs            # Git command output parser
│   │   ├── src/docker.rs         # Docker command output parser
│   │   ├── src/kubectl.rs        # Kubernetes output parser
│   │   ├── src/tsc.rs            # TypeScript compiler parser
│   │   ├── src/go_build.rs       # Go build output parser
│   │   ├── src/go_test.rs        # Go test output parser
│   │   ├── src/json_lines.rs     # JSONL parser
│   │   ├── src/json_object.rs    # JSON object parser
│   │   ├── src/csv_parser.rs     # CSV parser
│   │   ├── src/terraform.rs      # Terraform output parser
│   │   ├── src/tap.rs            # TAP (Test Anything Protocol) parser
│   │   ├── src/cpp_compiler.rs   # C++ compiler output parser
│   │   └── src/generic_compiler.rs # Generic compiler output parser
│   │
│   ├── glass_mcp/                # MCP server for AI integration
│   │   ├── src/lib.rs            # run_mcp_server()
│   │   ├── src/tools.rs          # GlassServer with four tools
│   │   ├── src/context.rs        # Activity summary for AI
│   │   └── src/ipc_client.rs     # IPC client to communicate with Glass GUI
│   │
│   ├── glass_feedback/           # Self-improving orchestrator feedback
│   │   ├── src/lib.rs            # on_run_start(), on_run_end(), FeedbackState/Result
│   │   ├── src/analyzer.rs       # Analyze runs, produce findings
│   │   ├── src/rules.rs          # RuleEngine, Rule types (Tier 1 heuristics)
│   │   ├── src/lifecycle.rs      # Apply findings, config changes, rule promotion/rejection
│   │   ├── src/regression.rs     # Check for metric regressions, auto-rollback
│   │   ├── src/quality.rs        # Quality metrics (waste, stuck rate)
│   │   ├── src/llm.rs            # LLM prompt generation (Tier 3 findings)
│   │   ├── src/coverage.rs       # Script coverage tracking
│   │   ├── src/types.rs          # Finding, RuleStatus, ConfigSnapshot, RunMetrics
│   │   ├── src/defaults.rs       # Default feedback config
│   │   ├── src/io.rs             # Load/save rules, metrics, history TOML files
│   │   └── src/schema.rs         # JSON schema for config validation
│   │
│   ├── glass_scripting/          # Rhai-based event-driven automation
│   │   ├── src/lib.rs            # ScriptSystem: top-level orchestrator
│   │   ├── src/engine.rs         # Rhai ScriptEngine (compile, execute, sandbox)
│   │   ├── src/hooks.rs          # HookRegistry: register scripts per hook point
│   │   ├── src/lifecycle.rs      # Script promotion (Provisional → Confirmed)
│   │   ├── src/loader.rs         # Load scripts from disk
│   │   ├── src/context.rs        # HookContext, HookEventData
│   │   ├── src/actions.rs        # Action types (ConfigValue, Log, MCP call)
│   │   ├── src/profile.rs        # Export/import script bundles
│   │   ├── src/sandbox.rs        # SandboxConfig (CPU time, memory, array size limits)
│   │   ├── src/types.rs          # HookPoint, LoadedScript, ScriptManifest, ScriptStatus
│   │   └── src/mcp.rs            # ScriptToolRegistry for MCP integration
│   │
│   ├── glass_coordination/       # Multi-agent coordination
│   │   ├── src/lib.rs            # Module exports, resolve_db_path()
│   │   ├── src/db.rs             # CoordinationDb: agent registry, file locking, messaging
│   │   ├── src/types.rs          # AgentInfo, FileLock, LockConflict, Message
│   │   ├── src/event_log.rs      # CoordinationEvent for audit trail
│   │   └── src/pid.rs            # is_pid_alive: check if agent process is running
│   │
│   ├── glass_errors/             # Error types
│   │   └── src/lib.rs            # Custom error types, From implementations
│   │
│   └── glass_agent/              # Agent worktree isolation
│       ├── src/lib.rs            # WorktreeManager: create/apply/dismiss/prune worktrees
│       └── src/worktree.rs       # Worktree lifecycle (git clone, checkout branch, apply diff)
│
├── benches/
│   └── perf_benchmarks.rs        # Criterion benchmarks for hot paths
│
├── docs/
│   ├── superpowers/              # AI agent internal documentation
│   │   ├── plans/                # Phase plans generated by GSD orchestrator
│   │   └── specs/                # Design specifications
│   └── src/                      # Public documentation
│
├── packaging/                    # Distribution packages
│   ├── homebrew/                 # Homebrew formula
│   └── linux/                    # Linux package metadata
│
├── shell-integration/            # Shell integration scripts (symlinked to crates/glass_terminal/shell-integration/)
│   ├── glass.bash
│   ├── glass.zsh
│   ├── glass.fish
│   └── glass.ps1
│
├── .planning/
│   ├── codebase/                 # Deep analysis documents (ARCHITECTURE, CONVENTIONS, etc.)
│   ├── milestones/               # Milestone plans and specs
│   ├── phases/                   # Phase-by-phase execution plans
│   └── research/                 # Architecture research and spikes
│
├── .glass/                       # Local Glass data (generated at runtime)
│   ├── blobs/                    # Content-addressed blob store (blake3 hashes)
│   ├── history.db                # Project-local command history (SQLite)
│   ├── snapshots.db              # File snapshot metadata (SQLite)
│   ├── agents.db                 # Multi-agent coordination (SQLite, WAL)
│   ├── config.toml               # User configuration
│   ├── rules.toml                # Orchestrator feedback rules (Tier 1-2)
│   ├── run-metrics.toml          # Metrics from last run
│   ├── tuning-history.toml       # History of config changes
│   ├── archived-rules.toml       # Archived rules (promoted/rejected)
│   ├── scripts/                  # Rhai scripts (Tier 4 automation)
│   │   ├── script_name.rhai
│   │   └── manifest.toml
│   ├── profiles/                 # Exported script profile bundles
│   └── glass_pty_loop.json       # PTY event log (for agent consumption)
│
├── Cargo.toml                    # Workspace manifest, dependency definitions
├── Cargo.lock                    # Dependency lock file
├── CLAUDE.md                     # Architecture reference (auto-loads in Claude conversations)
├── ORCHESTRATOR.md               # Complete orchestrator documentation
├── PRD.md                        # Product requirements document
└── README.md                     # Public README
```

## Directory Purposes

**`src/`:**
- Main binary crate housing the event loop, orchestrator, and bootstrap logic.
- Coordinates all subsystems and manages the winit window/event loop.
- ~3000 lines across main.rs (2200), orchestrator.rs (1800), script_bridge.rs (500), and supporting modules.

**`crates/glass_core/`:**
- Stable core types and system integration (config, events, coordination).
- No GUI or terminal dependencies — cross-crate safe imports.
- Used by all other crates for AppEvent, SessionId, config structures.

**`crates/glass_terminal/`:**
- PTY management and shell integration parsing.
- Platform-specific (ConPTY on Windows, forkpty on Unix).
- OscScanner parses OSC 133 sequences into structured shell events.
- BlockManager tracks command lifecycle (state machine).

**`crates/glass_renderer/`:**
- GPU rendering via wgpu + glyphon.
- Frame composition, pane layout geometry, hit detection for interactive elements.
- Separated from core terminal logic to enable headless testing of grid/block logic.

**`crates/glass_mux/`:**
- Session and tab management.
- Split pane layout via binary tree (SplitNode).
- Platform abstraction (shell detection, home dir, config dir).

**`crates/glass_history/`:**
- SQLite database for queryable command history with FTS5.
- Output compression (diff-based vs. full), retention pruning.
- Used by MCP server for query tools and by main loop for recording.

**`crates/glass_snapshot/`:**
- Content-addressed blob store (blake3) + metadata DB.
- Command parsing and destructive command detection.
- Undo engine for file restoration.

**`crates/glass_soi/`:**
- Parser dispatch and per-type parsers (cargo, npm, pytest, docker, git, etc.).
- Token-efficient structured summaries for AI consumption.
- No dependencies outside std + ansi stripping.

**`crates/glass_feedback/`:**
- Feedback loop analysis and config/rule/script generation.
- Tier 1-3 analysis (heuristics, LLM prompts, script generation).
- Regression detection and auto-rollback.

**`crates/glass_scripting/`:**
- Rhai-based event-driven automation.
- Script loader, compiler, sandboxing, hook registry.
- Promotion/rejection lifecycle for scripts.

**`crates/glass_mcp/`:**
- MCP server exposing Glass tools (history, context, undo, file diff).
- Runs as subprocess invoked by Claude Code with stdio transport.
- Handles agent coordination (register, lock, unlock, message).

**`crates/glass_coordination/`:**
- Global SQLite DB (`~/.glass/agents.db`) for multi-agent file locking and messaging.
- Agent registration, advisory locks, inter-agent communication.

**`crates/glass_pipes/`:**
- Pipeline parsing (identify pipe stages) and captured stage metadata.
- Minimal: just types and parser.

**`crates/glass_errors/`:**
- Custom error types with From implementations.
- Centralized error definitions.

**`crates/glass_agent/`:**
- Agent worktree management (git clone, checkout, apply diff, prune).
- Isolates agent changes to per-agent worktrees for safe rollback.

**`.planning/codebase/`:**
- Deep-dive analysis documents (ARCHITECTURE, CONVENTIONS, TESTING, etc.).
- Consumed by `/gsd:plan-phase` and `/gsd:execute-phase` orchestrators.

**`.glass/` (runtime-generated):**
- Project-local database files (history.db, snapshots.db, agents.db).
- Configuration (config.toml, rules.toml, run-metrics.toml).
- Scripts and profiles (scripts/, profiles/).
- Content-addressed blob store (blobs/).

## Key File Locations

**Entry Points:**
- `src/main.rs` — GUI window, event loop, CLI subcommand dispatcher
- `src/main.rs` → CLI match `Commands::Mcp { .. }` → `crates/glass_mcp/lib.rs::run_mcp_server()` — MCP server over stdio
- `src/main.rs` → CLI match `Commands::History { .. }` → `src/history.rs` — History query subcommand
- `src/main.rs` → CLI match `Commands::Undo { .. }` → `crates/glass_snapshot/` — Undo restore
- `src/main.rs` → CLI match `Commands::Profile { .. }` → `crates/glass_scripting/profile.rs` — Script export/import

**Configuration:**
- `crates/glass_core/src/config.rs` — GlassConfig struct with TOML serialization
- `~/.glass/config.toml` — User configuration file (hot-reloaded)

**Core Logic:**
- `src/orchestrator.rs` — Orchestrator state machine (silence → proposal → feedback)
- `crates/glass_terminal/src/block_manager.rs` — Block state machine (Prompt → Input → Exec → Complete)
- `crates/glass_terminal/src/osc_scanner.rs` — OSC 133 sequence parser
- `crates/glass_terminal/src/pty.rs` — PTY spawning and shell integration
- `crates/glass_mux/src/session.rs` — Session struct (PTY, grid, block manager, history)
- `crates/glass_mux/src/session_mux.rs` — SessionMux (tabs, split panes)
- `crates/glass_mux/src/split_tree.rs` — SplitNode binary pane tree
- `crates/glass_history/src/db.rs` — HistoryDb (command records, FTS5 search)
- `crates/glass_snapshot/src/undo.rs` — UndoEngine (file restoration)
- `crates/glass_soi/src/classifier.rs` — OutputType detection
- `crates/glass_feedback/src/lifecycle.rs` — Apply feedback findings, config changes, rule promotion
- `crates/glass_scripting/src/engine.rs` — Rhai execution, sandbox, error handling

**Testing:**
- Tests co-located in source files via `#[cfg(test)] mod tests`
- Key test modules: `crates/glass_snapshot/src/lib.rs::tests`, `crates/glass_history/src/lib.rs::tests`, `src/tests.rs`

## Naming Conventions

**Files:**
- Rust modules: `snake_case.rs` (e.g., `block_manager.rs`, `session_mux.rs`)
- Binaries: `snake_case` (e.g., `glass`)
- Scripts: `snake_case.bash`, `snake_case.zsh`, `snake_case.ps1`
- Config: `snake_case.toml` (e.g., `config.toml`, `rules.toml`)

**Directories:**
- Crates: `snake_case` (e.g., `glass_terminal`, `glass_renderer`)
- Project structure: `snake_case` or `.snake_case` (e.g., `.glass`, `.planning`)

**Types:**
- Structs: `PascalCase` (e.g., `Block`, `Session`, `SessionMux`)
- Enums: `PascalCase` (e.g., `BlockState`, `AppEvent`, `OutputType`)
- Trait names: `PascalCase` (e.g., `Dimensions` from alacritty_terminal)
- Functions: `snake_case` (e.g., `spawn_pty`, `parse_pipeline`)

**Variables:**
- Local vars: `snake_case` (e.g., `window_id`, `session_id`)
- Constants: `SCREAMING_SNAKE_CASE` (e.g., `MAX_BLOCKS`, `SCROLLBAR_WIDTH`)

## Where to Add New Code

**New Feature (e.g., new command, new rendering element):**
- Primary code: `src/main.rs` (event loop integration) + feature crate (e.g., `crates/glass_terminal/` for PTY features, `crates/glass_renderer/` for UI)
- Tests: Co-located in source file
- Example: Adding a new overlay → new module in `crates/glass_renderer/src/new_overlay.rs`, integrate in `src/main.rs::render_frame()`

**New Module/Component:**
- Create new `.rs` file in the most logical crate
- Add `pub mod new_module;` to `src/lib.rs` (or crate's `lib.rs`)
- Export public types via `pub use`
- Example: New OSC sequence type → `crates/glass_terminal/src/new_sequence.rs`, export in `lib.rs`

**Utilities / Helpers:**
- Shared helpers across crates: `crates/glass_core/src/` (safe imports everywhere)
- Crate-specific helpers: New module in that crate
- Example: New config option → `crates/glass_core/src/config.rs`, add field to `GlassConfig` struct

**Scripts / Automation:**
- Shell integration scripts: `crates/glass_terminal/shell-integration/`
- Rhai scripts (user-facing): `~/.glass/scripts/` (loaded at startup)
- Example: New bash function → `glass.bash`, will be auto-injected into shell

**Tests:**
- Unit tests: `#[cfg(test)] mod tests { ... }` at bottom of source file
- Integration tests: Separate module or use `cargo test --test integration_test`
- Example: Test new Block state transition → in `crates/glass_terminal/src/block_manager.rs::tests`

## Special Directories

**`.glass/` (User Data):**
- Purpose: Project and global Glass configuration, databases, and generated files
- Generated: Yes (created on first run)
- Committed: No (ignored by git via `.gitignore`)
- Contents: `config.toml`, `history.db`, `snapshots.db`, `agents.db`, `scripts/`, `blobs/`, etc.
- Project-local or global: Walks up from cwd to find `.glass/`; falls back to `~/.glass/` for global

**`.planning/` (GSD Automation):**
- Purpose: Phase plans, codebase analysis docs, research, todos for orchestrator
- Generated: Yes (populated by GSD orchestrator during planning/execution)
- Committed: Yes (committed to track orchestration history)
- Contents: `codebase/` (ARCHITECTURE.md, STRUCTURE.md, etc.), `phases/` (plan.md files), `milestones/`, `research/`, `todos/`

**`shell-integration/` or `crates/glass_terminal/shell-integration/`:**
- Purpose: Shell integration scripts (bash, zsh, fish, PowerShell)
- Generated: No (hand-written)
- Committed: Yes
- Contents: `glass.bash`, `glass.zsh`, `glass.fish`, `glass.ps1` — auto-injected by PTY spawner

**`docs/superpowers/`:**
- Purpose: Internal AI agent documentation (plans, specs, self-improvement)
- Generated: Yes (by GSD orchestrator)
- Committed: Yes (commit history tracks orchestration iterations)

**`packaging/`:**
- Purpose: Distribution package metadata (Homebrew, Linux, Windows installers)
- Generated: No (hand-maintained)
- Committed: Yes

---

*Structure analysis: 2026-03-18*

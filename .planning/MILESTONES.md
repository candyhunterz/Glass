# Milestones

## v2.3 Agent MCP Features (Shipped: 2026-03-10)

**Phases completed:** 5 phases, 9 plans
**Lines of code:** ~30,000 Rust (up from ~24,047)
**Timeline:** 2026-03-09 to 2026-03-10 (1 day)
**Git range:** feat(35-01) to feat(39-01)
**Files changed:** 34 files, +5,811 / -25 lines

**Delivered:** Token-efficient AI agent terminal integration with IPC command channel, multi-tab orchestration, structured error extraction, filtered/cached/compressed output tools, and live command awareness through 12 new MCP tools.

**Key accomplishments:**
- Platform-abstracted IPC channel (named pipe/Unix socket) between MCP server and GUI event loop with JSON-line protocol and oneshot reply channels
- 5 tab orchestration MCP tools (create, list, send, output, close) with dual identifier support (tab_index + session_id)
- Token-saving tools: head/tail filtered output, cache staleness detection via file mtime, unified diffs via similar crate, budget-aware compressed context with focus modes
- glass_errors crate with auto-detecting parsers for Rust JSON, Rust human-readable, and generic file:line:col formats
- glass_extract_errors MCP tool exposing structured error extraction (file, line, column, message, severity) to AI agents
- Live command awareness: has_running_command with elapsed time reporting, cancel_command sending ETX byte to PTY

---

## v2.2 Multi-Agent Coordination (Shipped: 2026-03-10)

**Phases completed:** 4 phases, 8 plans, 16 tasks
**Lines of code:** 24,047 Rust (up from ~19,455)
**Timeline:** 2026-03-09 to 2026-03-10 (2 days)
**Git range:** feat(31-01) to feat(34-02)
**Files changed:** 32 files, +4,592 / -29 lines

**Delivered:** Multi-agent coordination layer enabling AI agents in separate tabs to register, claim advisory file locks, exchange messages, and avoid conflicts through a shared SQLite database, with full MCP tool exposure and GUI integration.

**Key accomplishments:**
- glass_coordination crate with agent registry, heartbeat liveness, PID detection, and SQLite WAL mode with IMMEDIATE transactions
- Atomic all-or-nothing file locking with conflict detection, path canonicalization via dunce, and project-scoped isolation
- Inter-agent messaging (broadcast + directed) with per-recipient fan-out rows and mark-as-read semantics
- 11 MCP tools exposing all coordination capabilities with conflict-as-success pattern and implicit heartbeat refresh
- CLAUDE.md coordination protocol for AI agents plus cross-connection integration tests proving concurrent WAL access
- GUI integration: status bar agent/lock counts with 5s polling, tab lock indicators, and conflict warning overlay

---

## v2.1 Packaging & Polish (Shipped: 2026-03-07)

**Phases completed:** 5 phases, 11 plans
**Lines of code:** 36,692 Rust (up from 17,868)
**Timeline:** 2026-03-07 (1 day)
**Git range:** feat(26-01) to feat(30-03)
**Files changed:** 86 files, +8,793 / -77 lines

**Delivered:** Production-ready distribution with performance baselines, config hot-reload, platform-native installers, auto-update, public documentation, and package manager listings across Windows, macOS, and Linux.

**Key accomplishments:**
- Criterion benchmark infrastructure with feature-gated tracing instrumentation and PERFORMANCE.md baselines (522ms cold start, 3-7us input latency, 86MB idle memory)
- Config validation with structured errors (line/column/snippet) and live hot-reload via notify file watcher with font rebuild and error overlay
- Platform-native installers: Windows MSI (cargo-wix), macOS DMG (hdiutil), Linux .deb (cargo-deb) with stable UpgradeCode
- GitHub Actions release workflow triggered on v* tags with parallel cross-platform builds and automated GitHub Releases upload
- Background auto-update checker against GitHub Releases with semver comparison, status bar notification, and Ctrl+Shift+U one-click update
- mdBook documentation site (16 pages) with GitHub Pages deployment, project README with badges, winget manifest, and Homebrew cask

**Tech debt (from audit):**
- Cold start 522ms (4.4% over 500ms target, within measurement variance)
- README screenshot placeholder (`<!-- TODO: Add screenshot -->`)
- Installation docs hardcode `anthropics/glass` instead of actual repo owner
- Homebrew DMG filename pattern needs verification against build script
- `<GITHUB_USER>` and `<SHA256>` placeholders need replacement at publish time
- macOS code signing and Windows code signing deferred to future milestone

---

## v2.0 Cross-Platform & Tabs (Shipped: 2026-03-07)

**Phases completed:** 5 phases, 12 plans
**Lines of code:** 17,868 Rust (up from 14,822)
**Timeline:** 2026-03-06 to 2026-03-07 (2 days)
**Git range:** feat(21-01) to feat(shell)
**Files changed:** 29 files, +4,265 / -1,091 lines

**Delivered:** Multi-session terminal architecture with tab bar, split panes, cross-platform compilation, and shell integration for all major shells (bash, zsh, fish, PowerShell).

**Key accomplishments:**
- glass_mux crate with SessionMux multiplexer, Session struct, and platform cfg-gated helpers (default_shell, is_glass_shortcut)
- SessionId newtype routing through AppEvent/EventProxy for multi-session event dispatch
- Cross-platform compilation (Windows/macOS/Linux) with CI matrix, platform-aware font defaults, and shell detection
- Shell integration for bash, zsh, fish, and PowerShell with auto-injection via find_shell_integration()
- Tab system with TabBarRenderer (GPU rect/text), keyboard shortcuts (Ctrl+Shift+T/W), mouse click, CWD inheritance
- Binary tree split pane layout engine (SplitTree) with compute_layout, remove_leaf, find_neighbor, resize_ratio (26 TDD tests)
- Per-pane scissor-clipped rendering with viewport offsets, focus accent borders, and divider drawing
- Pane-aware TerminalExit handler routing PTY exit to close_pane or close_tab based on pane count

**Tech debt (from audit):**
- default_shell_program() duplicated in pty.rs and platform.rs
- config_dir() and data_dir() exported but never consumed (orphaned API)
- ScaleFactorChanged is log-only -- no dynamic font metric recalculation
- Human visual verification pending for split pane rendering, focus borders, resize, mouse click

---

## v1.3 Pipe Visualization (Shipped: 2026-03-06)

**Phases completed:** 6 phases, 11 plans
**Lines of code:** 28,885 Rust (up from 12,214)
**Timeline:** 2026-03-04 to 2026-03-06 (2 days)
**Git range:** feat(15-01) to feat(20-01)

**Delivered:** Pipe visualization system with transparent intermediate stage capture, multi-row pipeline UI blocks, per-stage storage, MCP inspection tools, and full config gating across shell/terminal/DB layers.

**Key accomplishments:**
- Byte-level pipe parser (glass_pipes crate) with shell quoting awareness, TTY detection, and --no-glass opt-out
- Shell capture via tee rewriting (bash/zsh) and Tee-Object (PowerShell) with OSC 133;S/P protocol transport
- Multi-row pipeline UI with auto-expand on failure, click/keyboard stage expansion, and sampled output rendering
- pipe_stages DB table with schema v2 migration, FK cascade, and retention policy integration
- GlassPipeInspect MCP tool + GlassContext pipeline stats for AI integration
- Three-layer pipes.enabled config gate (PTY env var, shell script checks, main.rs event processing)

**Tech debt (from audit):**
- PipeStage.is_tty populated by parse_pipeline but never consumed at runtime (vestigial after classify.rs removal)
- SCHEMA_VERSION const produces dead_code warning (used only in tests, migration uses hardcoded values)

---

## v1.2 Command-Level Undo (Shipped: 2026-03-06)

**Phases completed:** 5 phases, 13 plans
**Lines of code:** 12,214 Rust (up from 8,473)
**Timeline:** 2026-03-05 to 2026-03-06 (2 days)
**Git range:** feat(10-01) to docs(phase-14)

**Delivered:** Command-level undo system with automatic filesystem snapshots, pre-exec command parsing, FS watcher engine, one-keystroke revert (Ctrl+Shift+Z), CLI undo, MCP tools for AI integration, and storage lifecycle management.

**Key accomplishments:**
- Content-addressed blob store (glass_snapshot crate) with BLAKE3 hashing, deduplication, and SQLite snapshot metadata
- POSIX + PowerShell command parser identifying file targets for pre-exec snapshot (rm, mv, sed -i, cp, chmod, git checkout, Remove-Item, etc.)
- Filesystem watcher engine with .glassignore pattern matching and notify-based event monitoring during command execution
- UndoEngine with conflict detection (BLAKE3 hash comparison), file restoration, and confidence tracking
- [undo] label on command blocks, Ctrl+Shift+Z keybinding, `glass undo <id>` CLI, GlassUndo + GlassFileDiff MCP tools
- Storage pruning with configurable age/count limits and orphan blob cleanup on startup

**Tech debt (from audit):**
- pruner.rs: max_size_mb accepted but not enforced (count and age pruning work; size is secondary)
- Nyquist validation partial across all 5 phases (VALIDATION.md exists but draft/non-compliant)

**v1.1 tech debt resolved by v1.2:**
- Command text extraction fixed (now extracted at CommandExecuted time, no longer empty string)
- prune() now auto-triggered on startup via background thread

---

## v1.1 Structured Scrollback + MCP Server (Shipped: 2026-03-05)

**Phases completed:** 5 phases, 12 plans
**Lines of code:** 8,473 Rust (up from 4,343)
**Timeline:** 2026-03-05 (1 day)
**Git range:** feat(05-01) to docs(phase-09)

**Delivered:** Structured scrollback database with FTS5 search, PTY output capture, CLI query interface, in-terminal search overlay, and MCP server exposing terminal history to AI assistants.

**Key accomplishments:**
- SQLite history database (glass_history crate) with FTS5 full-text search, per-project storage, and retention policies
- PTY output capture pipeline with alt-screen detection, binary filtering, ANSI stripping, and schema migration
- CLI query interface (`glass history search/list`) with combined filters (exit code, time range, cwd, text) and formatted output
- Search overlay (Ctrl+Shift+F) with live incremental search, 150ms debounce, and epoch-based scroll-to-block navigation
- MCP server (glass_mcp crate) exposing GlassHistory and GlassContext tools over stdio JSON-RPC via rmcp SDK
- Clap subcommand routing with Option<Commands> pattern preserving default terminal launch

**Tech debt (from audit):**
- Command text stored as empty string (metadata + output captured; grid extraction deferred)
- PTY throughput not benchmarked (architecture is non-blocking but no quantitative baseline)
- prune() has no runtime caller (retention policies exist but never auto-triggered)
- test_resolve_db_path_global_fallback fails on machines with ~/.glass/ (test isolation)

---

## v1.0 MVP (Shipped: 2026-03-05)

**Phases completed:** 4 phases, 12 plans
**Lines of code:** 4,343 Rust
**Timeline:** 2026-03-04 (1 day)
**Git range:** feat(01-01) to perf(04-02)

**Delivered:** A GPU-accelerated terminal emulator with shell integration, block-based command output, and daily-drivable performance on Windows.

**Key accomplishments:**
- 7-crate Rust workspace with wgpu DX12 GPU surface and ConPTY PTY spawn
- Full terminal rendering pipeline — instanced GPU rects, glyphon text, 24-bit color, cursor, font-metrics resize
- Complete keyboard encoding with Ctrl/Alt/arrow/function keys, clipboard, bracketed paste, scrollback
- Shell integration data layer — OscScanner, BlockManager, StatusState with 27 TDD tests
- Block UI rendering — separator lines, exit code badges, duration labels, status bar with CWD and git branch
- TOML configuration, 360ms cold start, 3-7us key latency, 86MB idle memory

**Tech debt (from audit):**
- display_offset hardcoded to 0 in frame.rs — block decorations render at wrong positions during scrollback
- ConPTY test execution not formally logged
- Nyquist validation partial for phases 2, 3, 4

---


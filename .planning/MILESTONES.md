# Milestones

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


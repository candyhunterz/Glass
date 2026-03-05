# Project Research Summary

**Project:** Glass v1.1 -- Structured Scrollback + MCP Server
**Domain:** SQLite FTS5 history database, search overlay UI, MCP server, and CLI query interface for a GPU-accelerated terminal emulator
**Researched:** 2026-03-05
**Confidence:** HIGH

## Executive Summary

Glass v1.1 adds a structured command history database to the existing GPU-accelerated terminal emulator, enabling full-text search across commands and their output, a search overlay UI, a CLI query interface, and an MCP server that exposes terminal history to AI assistants. This is a well-understood domain: Atuin proved the SQLite-backed history model in Rust, FTS5 is mature and heavily documented, and the MCP specification has an official Rust SDK (rmcp 1.1). The key differentiator is that Glass captures command *output* -- not just the command text -- which no existing tool indexes for search.

The recommended approach leverages the existing v1.0 architecture patterns: a dedicated writer thread with channel (matching the PTY reader pattern), SQLite WAL mode for concurrent read/write access, and the MCP server as a separate process invocation (`glass mcp serve`) sharing the database file. The stack additions are minimal -- rusqlite is already in the workspace with FTS5 enabled via the `bundled` feature, and the new dependencies (rmcp, clap, schemars, chrono) add only ~2.2MB to the binary and ~5-10MB runtime memory, well within constraints.

The primary risks are: (1) output capture degrading PTY throughput -- mitigated by in-memory buffering with flush-on-command-complete, never writing to SQLite from the PTY hot path; (2) SQLite connection sharing causing deadlocks -- mitigated by one-connection-per-thread with WAL mode; (3) MCP stdout corruption from logging -- mitigated by stderr-only logging with `#![deny(clippy::print_stdout)]`; and (4) search overlay blocking the render loop -- mitigated by async queries on a background thread. All four risks have clear, proven prevention strategies.

## Key Findings

### Recommended Stack

The v1.1 stack builds on the existing v1.0 foundation with five new workspace dependencies. No existing dependencies change version. The total binary size impact is ~2.2MB (negligible against the existing ~80MB GPU binary), and runtime memory increases by ~5-10MB.

**Core technologies:**
- **rusqlite 0.38** (existing): SQLite with FTS5 full-text search -- the `bundled` feature compiles SQLite 3.51.1 with FTS5 enabled unconditionally; direct SQL is the right abstraction since ORMs fight FTS5 MATCH/rank syntax
- **rmcp 1.1** (new): Official Rust MCP SDK -- provides `#[tool]` macros, JSON Schema generation, and stdio transport; tracks the canonical MCP spec (2025-11-25)
- **clap 4.5** (new): CLI argument parsing -- routes `glass` (terminal), `glass history` (queries), and `glass mcp serve` (MCP server) from a single binary
- **schemars 1.0** (new): JSON Schema generation -- required by rmcp for MCP tool parameter schemas; must be ^1.0 (incompatible with 0.8)
- **chrono 0.4** (new): Timestamp handling -- already a transitive dep of rmcp; used directly for history record timestamps and retention calculations

**Disagreement resolved:** The architecture research suggests hand-rolling JSON-RPC instead of using rmcp to minimize dependencies. The stack research recommends rmcp. **Use rmcp** -- it is the official SDK, released March 2026, with excellent macro ergonomics. Hand-rolling JSON-RPC for spec compliance is a maintenance burden that outweighs the ~500KB dependency cost.

### Expected Features

**Must have (table stakes):**
- Command text, CWD, exit code, duration, timestamp storage in SQLite
- Output capture (truncated, primary screen only, skip alternate screen)
- FTS5 full-text indexing on command text
- Session and hostname tracking
- Search overlay (Ctrl+Shift+F) with live results, arrow key navigation, jump-to-block
- CLI query interface (`glass history search`) with exit code, CWD, time range, and limit filters
- MCP server with stdio transport, `initialize` handshake, `tools/list`, `tools/call`
- GlassHistory and GlassContext MCP tools
- Retention policies (max age, max DB size) with TOML configuration

**Should have (differentiators):**
- FTS5 on command output (unique -- no terminal emulator does this)
- MCP GlassContext tool exposing live terminal context to AI assistants
- Structured JSON output from MCP tools via `outputSchema`
- CLI JSON output format (`--format json`) for scriptability
- Per-command output size tracking

**Defer to v1.2+:**
- FTS5 on output text (validate storage impact first)
- Output preview in search overlay (high rendering complexity)
- Directory/workspace filtering (requires git root detection)
- MCP resources capability (tools cover the core use case)
- Cloud sync, shell history import, regex search, AI suggestions (explicitly anti-features)

### Architecture Approach

The system introduces two new crates (glass_history, glass_mcp) and modifies three existing ones (glass_terminal, glass_renderer, glass_core). The database is the central integration point: a dedicated writer thread owns the write connection and receives records via mpsc channel, while the search overlay, CLI, and MCP server each open independent read-only connections. SQLite WAL mode enables this concurrent access pattern. The MCP server runs as a separate process (`glass mcp serve`), sharing the database file -- it cannot run inside the terminal process because both need stdin/stdout for different purposes.

**Major components:**
1. **glass_history** -- SQLite database with FTS5 schema, CRUD operations, search queries, retention enforcement
2. **glass_mcp** -- MCP JSON-RPC stdio server with GlassHistory and GlassContext tools, reads from glass_history
3. **Output capture** (in glass_terminal) -- buffers command output in memory during execution, captures text from terminal grid on command completion, sends to history writer
4. **Search overlay** (in glass_renderer) -- modal overlay rendered on top of terminal content, queries history via read-only connection, supports live incremental search
5. **CLI interface** (in glass binary) -- subcommand routing via clap, direct database queries, stdout formatting

### Critical Pitfalls

1. **SQLite connection sharing across threads** -- use one connection per thread, WAL mode, `busy_timeout(5000ms)`, and `BEGIN IMMEDIATE` for writes. Never share a `rusqlite::Connection` via `Arc<Mutex>`.
2. **Output capture killing PTY throughput** -- never write to SQLite from the PTY reader thread. Buffer output in memory, flush to DB only on command complete (OSC 133;D). Benchmark against v1.0 baseline.
3. **MCP stdout corruption from logging** -- route ALL logging to stderr, add `#![deny(clippy::print_stdout)]`, set custom panic hook to stderr. The single most common MCP server bug.
4. **FTS5 external content table sync corruption** -- prefer content tables (not external content) for v1.1. If using external content, test trigger syntax exhaustively. Run FTS5 integrity check on startup.
5. **Search overlay blocking the render loop** -- debounce queries (150ms), run on background thread, send results via EventLoopProxy. FTS5 on large corpora can take 50-200ms.

## Implications for Roadmap

Based on research, suggested phase structure:

### Phase 1: History Database Foundation

**Rationale:** Everything depends on the SQLite schema and write path. glass_history has zero dependencies on the terminal or renderer, making it fully testable in isolation. Getting the threading model and WAL configuration right here prevents cascading bugs in all later phases.
**Delivers:** glass_history crate with schema creation, CRUD, FTS5 search, retention enforcement, and comprehensive unit tests
**Addresses:** Command storage, FTS5 indexing, session/hostname tracking, retention policies
**Avoids:** Pitfall 1 (connection sharing), Pitfall 3 (FTS5 sync corruption)
**Stack:** rusqlite 0.38 (existing), chrono 0.4 (new)

### Phase 2: Output Capture + History Writer Integration

**Rationale:** This is the hardest integration -- it modifies the PTY read loop, the most sensitive code path. Tackling it early with a small scope reduces risk. It also provides the data pipeline that feeds everything else.
**Delivers:** PtyBlockTracker in PTY thread, output_capture module, history writer thread with channel, BlockManager enhancements (unix timestamps, CWD tracking), config extensions
**Addresses:** Output capture (truncated), command text capture from terminal grid, alternate screen detection
**Avoids:** Pitfall 2 (PTY throughput regression), Pitfall 14 (OSC 133 gaps)
**Prerequisite tech debt:** Re-baseline PTY throughput before starting; fix display_offset if needed for later phases

### Phase 3: CLI Query Interface

**Rationale:** Provides immediate validation that the database is being populated correctly, without requiring any renderer changes. Quick to build, high diagnostic value.
**Delivers:** `glass history search`, `glass history list` subcommands with filters (--exit, --cwd, --after, --before, --limit, --format)
**Addresses:** CLI query interface, JSON output format
**Avoids:** Pitfall 8 (DB locking conflicts)
**Stack:** clap 4.5 (new), serde_json 1.0 (new)

### Phase 4: Search Overlay UI

**Rationale:** Requires renderer modifications and input handling changes. By this point, the database is proven working and the query layer is validated via CLI. The overlay is the primary user-facing feature of v1.1.
**Delivers:** SearchOverlay component in glass_renderer, Ctrl+Shift+F activation, live incremental search, arrow key navigation, jump-to-block on Enter
**Addresses:** Search overlay with live results, result highlighting, dismiss with Escape
**Avoids:** Pitfall 5 (blocking render loop), Pitfall 7 (display_offset), Pitfall 10 (z-order conflicts)
**Prerequisite:** display_offset must be fixed before or at the start of this phase

### Phase 5: MCP Server

**Rationale:** Most self-contained new crate. Only reads from the database, no interaction with the terminal event loop. Could theoretically be built in parallel with phases 3-4, but sequencing it last reduces cognitive load.
**Delivers:** glass_mcp crate with JSON-RPC stdio server, GlassHistory tool, GlassContext tool, `glass mcp serve` subcommand
**Addresses:** MCP server with stdio transport, tool discovery, tool invocation, structured output
**Avoids:** Pitfall 4 (stdout corruption), Pitfall 6 (embedded vs. separate process), Pitfall 12 (schema drift)
**Stack:** rmcp 1.1 (new), schemars 1.0 (new)

### Phase 6: Polish + Retention + Performance

**Rationale:** Correctness and polish concern, not functionality. The system works without it but grows unbounded. Also the right time for end-to-end performance validation.
**Delivers:** Periodic retention enforcement, TOML config for history settings, performance benchmarks, FTS5 integrity checks on startup
**Addresses:** Retention policies, config integration, performance validation
**Avoids:** Pitfall 9 (FTS5 index size explosion), Pitfall 13 (retention/FTS5 orphans)

### Phase Ordering Rationale

- **Bottom-up dependency chain:** glass_history -> output capture -> CLI/overlay/MCP. Each phase is testable before the next begins.
- **Risk-first:** Output capture (Phase 2) is the riskiest integration, so it comes early when scope is small and rollback is cheap.
- **Validation ladder:** CLI (Phase 3) validates the database before the overlay (Phase 4) builds UI on top of it.
- **Isolation:** MCP server (Phase 5) is architecturally independent -- it runs as a separate process. Deferring it until late reduces work-in-progress.
- **Polish last:** Retention and performance (Phase 6) are tuning concerns that benefit from real usage data from earlier phases.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 2 (Output Capture):** Complex integration with PTY read loop and alacritty_terminal internals. Needs careful analysis of the lock acquisition sequence and grid text extraction API. The PtyBlockTracker design requires understanding of line numbering across scrollback.
- **Phase 4 (Search Overlay):** GPU rendering integration, glyphon TextArea management for overlay text, and the async query pattern via EventLoopProxy need implementation-level research. The display_offset tech debt may require significant investigation.

Phases with standard patterns (skip research-phase):
- **Phase 1 (History Database):** Well-documented SQLite + FTS5 patterns, rusqlite API is straightforward, plenty of prior art (Atuin).
- **Phase 3 (CLI):** Standard clap subcommand pattern, query logic already exists in glass_history.
- **Phase 5 (MCP Server):** rmcp SDK provides macros and examples, MCP spec is well-documented, stdio transport is simple.
- **Phase 6 (Polish):** Standard SQLite maintenance patterns, config is an extension of existing TOML system.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | All dependencies verified against official docs and crates.io. Version compatibility confirmed. rusqlite FTS5 verified via build.rs source. rmcp 1.1 released 2026-03-04. |
| Features | HIGH | Feature landscape validated against Atuin (closest prior art), MCP spec (official docs), and established terminal emulator patterns (WezTerm, fzf). Anti-features are well-reasoned. |
| Architecture | HIGH | Based on direct analysis of Glass v1.0 source code. Threading model follows existing PTY reader pattern. WAL concurrency is a proven SQLite pattern. |
| Pitfalls | HIGH | Pitfalls sourced from SQLite official docs, rusqlite GitHub issues, MCP spec requirements, and direct codebase analysis. All have concrete prevention strategies. |

**Overall confidence:** HIGH

### Gaps to Address

- **rmcp vs. hand-rolled JSON-RPC:** Stack research recommends rmcp; architecture research suggests hand-rolling. Recommendation: use rmcp. Resolve definitively during Phase 5 planning.
- **display_offset tech debt:** Noted as hardcoded to 0 in v1.0. Must be investigated and fixed before Phase 4 (search overlay). Actual severity is unknown until inspected.
- **Output capture from terminal grid:** The exact API for extracting plain text from `alacritty_terminal::Term` grid cells between line ranges needs implementation-level research. The architecture describes the approach but the specific alacritty_terminal types and methods need validation.
- **FTS5 on output text vs. command text only:** Features research recommends deferring FTS5 on output to v1.2. Architecture research includes it in the schema. Decision: index command text in FTS5 for v1.1; add output indexing as a fast-follow once storage patterns are understood.
- **Content table vs. external content table:** Pitfalls research recommends content tables (simpler, self-managing); architecture research uses external content tables (less storage). Start with content tables for safety, optimize later if needed.

## Sources

### Primary (HIGH confidence)
- [rusqlite GitHub + build.rs](https://github.com/rusqlite/rusqlite) -- FTS5 unconditionally enabled in bundled builds
- [SQLite FTS5 documentation](https://sqlite.org/fts5.html) -- query syntax, external content tables, integrity checks
- [SQLite WAL documentation](https://sqlite.org/wal.html) -- concurrent reader/writer guarantees
- [MCP Specification (2025-11-25)](https://modelcontextprotocol.io/specification/2025-11-25) -- protocol, tools capability, stdio transport
- [rmcp official Rust SDK v1.1](https://github.com/modelcontextprotocol/rust-sdk) -- tool macros, schemars integration
- [Atuin](https://github.com/atuinsh/atuin) -- closest prior art for SQLite-backed structured shell history in Rust
- Glass v1.0 source code -- direct codebase analysis for all integration points

### Secondary (MEDIUM confidence)
- [Shuttle MCP server guide](https://www.shuttle.dev/blog/2025/07/18/how-to-build-a-stdio-mcp-server-in-rust) -- stderr logging pattern, stdout purity requirement
- [SQLite connection pool write performance](https://emschwartz.me/psa-your-sqlite-connection-pool-might-be-ruining-your-write-performance/) -- single writer best practices
- [SQLITE_BUSY despite timeout](https://berthub.eu/articles/posts/a-brief-post-on-sqlite3-database-locked-despite-timeout/) -- BEGIN IMMEDIATE requirement
- [WezTerm Search](https://wezterm.org/config/lua/keyassignment/Search.html) -- terminal emulator search overlay reference

### Tertiary (LOW confidence)
- [rmcp tool macros guide (HackMD)](https://hackmd.io/@Hamze/S1tlKZP0kx) -- community guide, may drift from latest API

---
*Research completed: 2026-03-05*
*Ready for roadmap: yes*

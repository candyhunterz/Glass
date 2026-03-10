# Project Research Summary

**Project:** Glass v2.3 -- Agent MCP Features
**Domain:** AI agent tooling for GPU-accelerated terminal emulator
**Researched:** 2026-03-09
**Confidence:** MEDIUM-HIGH

## Executive Summary

Glass v2.3 adds 12 new MCP tools that let AI agents orchestrate multiple terminal tabs, extract structured errors, save tokens through filtered/cached output, and monitor live command state. The core technical challenge is bridging the process boundary between the MCP server (currently a separate process) and the GUI's main event loop (which owns all live session state). Two new runtime dependencies are needed (`similar` for diffs, `regex` for error parsing) plus one new crate (`glass_errors`). Everything else reuses the existing validated stack.

The recommended architecture is a hybrid approach: keep the MCP server as a separate process (`glass mcp serve`) but add a lightweight IPC listener (localhost TCP) inside the GUI process for the 7 tools that need live session data. DB-only tools (5 of 12) continue working without the GUI running. This avoids the `#![windows_subsystem = "windows"]` stdin conflict that would block an embedded MCP approach, while keeping the IPC surface minimal (7 JSON-line methods over localhost TCP). The alternative of fully embedding the MCP server is simpler in theory but creates a transport problem on Windows that adds its own complexity.

The top risks are: (1) the process boundary itself -- the entire communication design must be settled before writing code, as the wrong choice forces a complete rewrite; (2) FairMutex contention on the terminal grid when MCP reads compete with the PTY reader and renderer; and (3) tab ID instability if tab indices (not stable SessionIds) are used as identifiers. All three are preventable with upfront design decisions documented in the research.

## Key Findings

### Recommended Stack

The existing workspace stack (tokio 1.50 full, rmcp 1.1.0, rusqlite 0.38, winit 0.30) is unchanged. Only 2 new runtime dependencies are added. See [STACK.md](./STACK.md) for full details.

**Core technologies:**
- `similar` 2.7.0: unified diff generation for `glass_changed_files` -- de facto Rust diffing library, zero transitive deps, pure Rust
- `regex` 1.12.3: error pattern matching in `glass_errors`, output filtering in `glass_output` -- already a transitive dependency, immune to ReDoS by design
- `tokio::sync::mpsc` + `oneshot` (existing): request/response channel pattern for MCP-to-event-loop communication -- zero new deps, documented Tokio pattern
- `std::sync::LazyLock` (stable since Rust 1.80): lazy regex compilation -- replaces need for `lazy_static` or `once_cell`

**What NOT to add:** `crossbeam-channel` (not async-aware), `nom`/`winnow` (overkill for line-oriented error parsing), `lazy_static` (superseded by std), `signal-hook` (PTY cancel is just writing `\x03`), `tokio-rusqlite` (project uses synchronous `spawn_blocking` pattern successfully).

### Expected Features

See [FEATURES.md](./FEATURES.md) for full feature landscape and competitor analysis.

**Must have (table stakes):**
- MCP Command Channel -- async bridge between MCP server and main event loop (infrastructure, not user-facing, but blocks 7 of 12 tools)
- Multi-tab lifecycle (create/list/close) -- iTerm2 and kitty both expose this; agents expect it
- Run command in tab + read output -- core agent workflow, every terminal MCP server provides this
- Filtered/truncated output retrieval -- Claude Code already truncates at 30K chars; pattern filtering is the next step
- Live command status (running/complete) -- agents need to know when output is final before reading it
- Basic structured error extraction -- agents waste tokens parsing raw error text; at minimum a generic `file:line:col: message` parser

**Should have (differentiators):**
- `glass_cached_result` with staleness detection -- unique to Glass (cross-references history timestamps with file snapshot timestamps)
- `glass_changed_files` with unified diffs -- unique to Glass (content-addressed blob store enables pre/post command file diffs)
- Budget-aware `glass_context` -- token-budget-aware context compression after context resets
- Command cancel via MCP -- enables autonomous "run, check, cancel if stuck" workflows

**Defer:**
- Python/Node/Go/GCC dedicated error parsers -- generic fallback handles `file:line:col: message` pattern; add based on user demand
- Streaming output via MCP -- MCP stdio transport does not support server-initiated streaming; polling with `has_running_command` flag is sufficient
- Persistent named sessions across restarts -- adds state management complexity for ephemeral agent workflows
- Tab output diffing (delta between polls) -- requires per-caller state management; agents can diff locally

### Architecture Approach

Hybrid architecture: MCP server remains a separate process for backward compatibility and to avoid the Windows `#![windows_subsystem = "windows"]` stdin conflict. The GUI process adds a lightweight IPC listener (tokio TcpListener on localhost) that handles only the 7 live-data requests. The MCP server discovers the GUI via `~/.glass/gui.port`. DB-only tools continue working without the GUI. See [ARCHITECTURE.md](./ARCHITECTURE.md) for component diagrams and data flows.

**Major components:**
1. `glass_core/mcp_channel.rs` (NEW) -- McpRequest/McpResponse enums, `AppEvent::Mcp` variant, IPC protocol types (~80 lines)
2. `glass_errors/` (NEW CRATE) -- Pure error parsing library: `parse(&str, Option<&str>) -> Vec<ParsedError>`, regex-based, no async deps (~600 lines)
3. IPC listener in `main.rs` -- Tokio TCP listener on localhost, receives JSON-line requests, routes through EventLoopProxy to winit event loop, replies via oneshot channel
4. 12 new MCP tool handlers in `glass_mcp/tools.rs` -- 7 live-data tools (route through IPC to GUI), 5 DB-only tools (existing `spawn_blocking` pattern)

**Components NOT modified:** glass_terminal, glass_renderer, glass_mux, glass_history, glass_snapshot, glass_coordination, glass_pipes.

### Critical Pitfalls

See [PITFALLS.md](./PITFALLS.md) for full pitfall analysis with prevention strategies.

1. **Process boundary misconception** -- MCP server and GUI are separate processes; mpsc channels cannot cross process boundaries. Must resolve embed-vs-IPC architecture before writing any code. Recommendation: hybrid IPC approach. *Phase 1 blocking decision.*
2. **FairMutex grid contention** -- Adding MCP grid reads creates a three-way lock fight (PTY reader, renderer, MCP). Write a minimal text-only grid reader, lock-copy-release pattern, never process strings under lock. Target <5ms lock hold. *Phase 2.*
3. **Tab ID instability** -- Tab indices shift when tabs close. Use stable SessionId (monotonic u64), not vector index, as the MCP identifier from day one. *Phase 2.*
4. **Oneshot reply channel drops** -- Every error path in MCP request handlers must send a reply. A dropped oneshot sender hangs the MCP tool indefinitely. Structure handlers as functions that always return a response; add 5-second timeout on recv. *Phase 1.*
5. **Windows subsystem stdio conflict** -- `#![windows_subsystem = "windows"]` suppresses stdin/stdout. Embedded MCP must use socket transport, not stdio. The hybrid IPC approach sidesteps this entirely. *Phase 1.*

## Implications for Roadmap

Based on research, suggested 5-phase structure:

### Phase 1: MCP Command Channel + IPC Foundation
**Rationale:** Every live-data feature (7 of 12 tools) depends on this. The architecture decision (hybrid IPC) must be implemented and validated before any live-data tools can be built. This phase concentrates the highest-risk technical decisions.
**Delivers:** McpRequest/McpResponse types in glass_core, AppEvent::Mcp variant, IPC TCP listener in main.rs, IPC client helper in glass_mcp, port file discovery via `~/.glass/gui.port`.
**Addresses:** MCP Command Channel infrastructure (FEATURES table stakes).
**Avoids:** Process boundary pitfall (1), Windows subsystem stdio conflict (5), event loop starvation (3), oneshot reply drops (4).
**Estimated scope:** ~400 lines across glass_core, glass_mcp, main.rs.

### Phase 2: Multi-Tab Orchestration
**Rationale:** Core agent workflow -- create tabs, run commands, read output. Depends on Phase 1 IPC. Build in order: list -> output -> create -> run -> close (each validates a deeper integration point).
**Delivers:** 5 MCP tools (glass_tab_create, glass_tab_list, glass_tab_run, glass_tab_output, glass_tab_close).
**Addresses:** Tab lifecycle and command execution (FEATURES table stakes).
**Avoids:** Tab ID instability (use SessionId), FairMutex contention (text-only grid reader), unbounded output (100KB cap, default 50 lines), concurrent PTY writes (serialize per session).
**Estimated scope:** ~350 lines in glass_mcp/tools.rs + main.rs handlers.

### Phase 3: Token-Saving Tools
**Rationale:** High value, low risk, mostly DB-only. Can partially overlap with Phase 1/2 since DB-only tools need no IPC. Building after Phase 2 allows adding live-grid mode (via tab_id parameter) in addition to history-DB mode (via command_id).
**Delivers:** 4 MCP tools (glass_output, glass_cached_result, glass_changed_files, glass_context budget/focus).
**Uses:** `similar` crate for diff generation, existing HistoryDb and SnapshotStore APIs.
**Avoids:** Stale cache (check filesystem mtimes, not just snapshot DB; default max_age 120s), large file diffs (50KB cap, skip binary files), history truncation (indicate when output was capped at 50KB).
**Estimated scope:** ~400 lines in glass_mcp/tools.rs.

### Phase 4: Structured Error Extraction
**Rationale:** Requires building the new glass_errors crate. Can be developed in parallel with Phases 1-3 since the crate has zero dependency on other glass_* crates. Ship with Rust + Generic fallback parsers only -- add others post-launch based on demand.
**Delivers:** glass_errors crate + glass_errors MCP tool.
**Uses:** `regex` crate, `std::sync::LazyLock` for compiled patterns.
**Avoids:** Parser scope creep (start with 2 parsers, not 6), ANSI contamination (strip escapes before parsing, reuse existing stripper).
**Estimated scope:** ~600 lines in glass_errors crate + ~50 lines MCP wiring.

### Phase 5: Live Command Awareness
**Rationale:** Smallest scope, simplest implementation. Depends on Phase 1 IPC channel. CommandStatus reads block_manager state; CommandCancel writes one byte to PTY sender.
**Delivers:** 2 MCP tools (glass_command_status, glass_command_cancel).
**Avoids:** Cancel race condition (re-check block state before sending Ctrl+C; only send 0x03 byte; return `already_complete` if command finished).
**Estimated scope:** ~100 lines.

### Phase Ordering Rationale

- **Phase 1 must come first** because 7 of 12 tools depend on the IPC channel. The architecture decision (hybrid vs embedded) is a blocking prerequisite.
- **Phase 2 before Phase 3** because tab orchestration is table stakes that agents expect, and Phase 3 tools benefit from having live-grid access (via tab_id) in addition to history-DB access.
- **Phase 4 is parallelizable** with Phases 1-3 because glass_errors is a pure library crate with no cross-crate dependencies. A developer could work on error parsers while the IPC channel is being built.
- **Phase 5 is last** because it is the smallest scope and least critical. Command status/cancel are useful but agents can work without them by polling output.
- **Total estimated scope:** ~1,900 lines of new code across all phases.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 1:** IPC implementation details -- verify rmcp supports custom AsyncRead+AsyncWrite transports for the hybrid approach. Prototype the localhost TCP listener and port file discovery before committing. Cross-platform socket behavior (Windows named pipes vs TCP) needs validation. The `create_session()` function takes 10 parameters; extracting window state into a helper struct needs design.
- **Phase 2:** Grid content extraction -- the FairMutex lock pattern for text-only reads needs prototyping to measure actual lock hold times under heavy PTY output.

Phases with standard patterns (skip research-phase):
- **Phase 3:** DB-only tools follow the established `spawn_blocking` + SQLite pattern used by all existing MCP tools. `similar` crate usage is well-documented.
- **Phase 4:** Regex-based text parsing is straightforward. Test fixtures from real compiler output are the main effort, not architectural decisions.
- **Phase 5:** Writing a byte to PTY sender and reading block_manager state are trivial operations following existing patterns.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | Only 2 new deps, both well-established. All existing deps unchanged. Verified via cargo search. |
| Features | MEDIUM-HIGH | Strong prior art from iTerm2, kitty, Claude Code patterns. AI-agent-specific terminal tooling is still emerging -- some feature priorities may shift based on real agent usage. |
| Architecture | MEDIUM | Hybrid IPC approach is sound but unvalidated in this codebase. The IPC listener is new infrastructure. rmcp custom transport support needs verification. Embedded alternative is viable fallback. |
| Pitfalls | HIGH | Derived from direct codebase analysis. Process boundary, FairMutex contention, and tab ID instability are concrete, verifiable risks with clear mitigations. |

**Overall confidence:** MEDIUM-HIGH

### Gaps to Address

- **rmcp custom transport support:** Verify that rmcp 1.1.0 accepts custom `AsyncRead + AsyncWrite` implementations for socket-based transport. If not, the hybrid IPC approach needs a protocol adapter layer. Check rmcp source or docs during Phase 1 planning.
- **IPC discovery mechanism:** The `~/.glass/gui.port` file approach is simple but has edge cases (stale file from crashed process, multiple Glass instances). May need a PID check or heartbeat mechanism.
- **create_session() parameter extraction:** The function takes 10 parameters from window state. Need to design a helper struct or closure that captures the needed state for MCP-initiated tab creation without exposing renderer internals.
- **Output capture limit for errors:** History DB truncates at 50KB. For commands with many errors, this may cut off relevant diagnostics. Consider increasing the limit for failed commands or documenting the limitation clearly.
- **Concurrent MCP connections:** Two agents connecting to the same terminal could interleave PTY writes. Need to decide whether to serialize writes per session or integrate with the existing coordination lock system.

## Sources

### Primary (HIGH confidence)
- Glass codebase: main.rs, glass_mcp, glass_core, glass_mux, glass_terminal (direct analysis of event loop, MCP server, session management, PTY, block manager)
- tokio::sync documentation (mpsc, oneshot channel patterns)
- similar crate (crates.io, v2.7.0, unified diff generation)
- regex crate (crates.io, v1.12.3, ReDoS-immune by design)
- rustc JSON diagnostics format (official docs)
- alacritty_terminal FairMutex semantics (core design property)

### Secondary (MEDIUM confidence)
- iTerm2 Python API, kitty remote control protocol (competitive landscape, feature expectations)
- Anthropic engineering blog posts on tool design and context engineering (agent pain points, token savings patterns)
- Claude Code Bash tool behavior and output overflow issues (real agent constraints)
- rmcp SDK transport flexibility (inferred from Cargo.toml features, needs verification)

### Tertiary (LOW confidence)
- Terminal MCP server ecosystem (emerging, rapidly changing landscape)
- Token budget approximation ratios (heuristic, varies by model and content type)

---
*Research completed: 2026-03-09*
*Ready for roadmap: yes*

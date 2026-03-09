# Project Research Summary

**Project:** Glass v2.2 -- Multi-Agent Coordination
**Domain:** Agent orchestration layer for GPU-accelerated terminal emulator
**Researched:** 2026-03-09
**Confidence:** HIGH

## Executive Summary

Glass v2.2 adds a multi-agent coordination layer that enables multiple AI coding agents (Claude Code, Cursor, Copilot) to register, claim files via advisory locks, and communicate through a shared SQLite database. This is a well-scoped infrastructure milestone that leverages Glass's existing architectural patterns -- WAL-mode SQLite, synchronous crate libraries wrapped in `spawn_blocking` at the MCP layer, and background polling threads for GUI state. The domain is young but converging: Claude Code Agent Teams, Warp 2.0, Overstory, and mcp_agent_mail all implement variations of the same core primitives (agent registry, file locking, heartbeat liveness, messaging). Glass's unique advantage is being the only tool that pairs a GPU-rendered terminal GUI with an MCP-exposed coordination layer.

The recommended approach is to build a new `glass_coordination` crate as a pure synchronous library with zero dependency on any other glass_* crate, then wire it into `glass_mcp` via 11 new MCP tool handlers, followed by integration testing with CLAUDE.md instructions, and finally GUI integration in the status bar. The entire milestone requires only 2 new runtime crates (`uuid` for agent IDs, `dunce` for Windows path canonicalization) and reuses existing workspace dependencies (rusqlite, anyhow, tracing, dirs, windows-sys). This is the lightest-weight milestone in terms of new dependencies.

The key risks are concentrated in Phase 1: Windows UNC path canonicalization silently breaking lock matching across agents, SQLite `SQLITE_BUSY` errors from deferred transaction upgrades under concurrent writes, and WAL checkpoint starvation from persistent reader connections. All three have well-documented prevention strategies. The behavioral risk -- whether AI agents will reliably follow CLAUDE.md coordination instructions -- is harder to mitigate technically and must be validated through real multi-agent testing in Phase 3.

## Key Findings

### Recommended Stack

The milestone adds minimal new dependencies to the workspace. The core coordination infrastructure reuses rusqlite 0.38 (bundled) with the same WAL+PRAGMA pattern proven in glass_history and glass_snapshot. No new async runtime, IPC framework, or message queue is needed -- SQLite WAL replaces all of them. See [STACK.md](STACK.md) for full details.

**Core technologies:**
- **rusqlite 0.38 (existing):** Coordination DB (agents.db) -- same WAL pattern as HistoryDb and SnapshotDb, validated across hundreds of tests
- **uuid 1.22 (new):** Agent ID generation (v4 random) -- de facto Rust UUID crate, adds only getrandom as transitive dep (~5KB binary impact)
- **dunce 1.0.5 (new):** Windows-safe path canonicalization -- strips `\\?\` UNC prefix, zero dependencies, 150 lines
- **Raw libc/windows-sys (existing):** PID liveness checking -- ~30 lines of platform-gated code, avoids pulling process_alive (windows-sys version conflict) or sysinfo (3MB overkill)

**Explicitly rejected:** process_alive (windows-sys 0.61 conflict), sysinfo (massive), nix (overkill), async-sqlite (breaks established sync pattern), any IPC/message queue framework (SQLite IS the coordination mechanism).

### Expected Features

See [FEATURES.md](FEATURES.md) for full feature landscape and competitor analysis.

**Must have (table stakes):**
- Agent registration and discovery with UUID identity and project scoping
- Advisory file locking with atomic all-or-nothing acquisition (eliminates TOCTOU)
- Heartbeat-based liveness detection with PID fallback for immediate crash detection
- Inter-agent messaging (broadcast + directed) with structured message types
- 11 MCP tools exposing all coordination primitives
- CLAUDE.md integration instructions (the "glue" that makes agents self-coordinate)
- Lock conflict reporting with holder identity and reason

**Should have (differentiators):**
- Status bar agent/lock count display -- low complexity, high visibility, no other terminal does this
- Agent status task descriptions -- trivially extends registry, enables activity awareness
- Tab-level agent indicators -- moderate complexity, maps agents to tabs visually

**Defer (future milestone):**
- Conflict warning overlay -- HIGH complexity, requires new overlay event types and real-time DB monitoring
- Enforced file locking -- breaks terminal behavior, advisory locks work because agents cooperate
- Full A2A protocol -- enterprise-scale, overkill for local coordination
- Network agent discovery -- out of scope, SQLite requires same-host

### Architecture Approach

The coordination feature integrates as a new crate (`glass_coordination`) that is a pure synchronous library owning its own database (`~/.glass/agents.db`), with modifications to `glass_mcp` (11 new tool handlers), `glass_renderer` (status bar extension), and `src/main.rs` (background polling thread). Every pattern follows established precedent in the codebase. See [ARCHITECTURE.md](ARCHITECTURE.md) for component diagrams and data flows.

**Major components:**
1. **glass_coordination (new crate)** -- Pure synchronous library: CoordinationDb struct, agent registry, file locks, messaging, stale pruning. Zero dependency on any glass_* crate. Owns `~/.glass/agents.db`.
2. **glass_mcp (modified)** -- 11 new MCP tool handlers wrapping coordination operations in spawn_blocking. Adds agents_db_path field to GlassServer. Follows existing rmcp tool_router pattern exactly.
3. **glass_renderer (modified)** -- Status bar extended with optional CoordinationDisplay (agent count, lock count). Single new parameter threaded through build_status_text/draw_frame pipeline.
4. **src/main.rs (modified)** -- Background std::thread polling agents.db every 5 seconds, storing results in Arc<AtomicUsize> pairs. Render loop reads atomics (zero-cost) and passes to renderer.

**Key architectural decisions:**
- agents.db is ALWAYS global (`~/.glass/agents.db`), never per-project -- prevents two agents in different subdirectories from seeing different DBs
- CoordinationDb holds a Connection, not Arc<Mutex<Connection>> -- thread safety handled by open-per-call pattern
- GUI uses atomic polling, not AppEvent variants -- keeps glass_core unchanged
- Path canonicalization happens inside lock_files/unlock_file, not at caller -- ensures consistency

### Critical Pitfalls

See [PITFALLS.md](PITFALLS.md) for full pitfall analysis with recovery strategies.

1. **UNC path canonicalization on Windows** -- `std::fs::canonicalize()` produces `\\?\C:\...` paths that don't match normal paths. Two agents could lock the same file without detecting conflict. Use `dunce::canonicalize()`, normalize separators, and lowercase on Windows. Must be correct before any lock logic is built on top. *Phase 1.*

2. **SQLITE_BUSY from deferred transaction upgrades** -- Default `BEGIN` starts deferred transactions; upgrading to write fails without honoring busy_timeout. Use `TransactionBehavior::Immediate` for ALL write transactions from day one. *Phase 1.*

3. **WAL checkpoint starvation** -- Multiple persistent reader connections prevent WAL checkpointing, causing unbounded WAL growth. Set aggressive autocheckpoint (100 pages), attempt TRUNCATE checkpoint on open, never cache DB connections. *Phase 1.*

4. **Heartbeat timer drift** -- AI agents cannot reliably call heartbeat on a schedule. Piggyback heartbeat refresh on ALL MCP tool calls, increase stale timeout to 10 minutes. *Phase 1 + Phase 2.*

5. **Atomic lock acquisition livelock** -- Two agents requesting overlapping file sets in different order repeatedly conflict. Sort paths lexicographically before acquiring locks, add retry_after_ms hint to conflict responses. *Phase 1 + Phase 2.*

## Implications for Roadmap

Based on research, the design document's 4-phase structure is well-justified by dependency ordering and risk concentration. The phases map cleanly to architecture boundaries.

### Phase 1: Coordination Crate (Foundation)
**Rationale:** Everything depends on this. It has zero dependencies on other glass_* crates and must be built and tested before MCP tools can wrap it. This phase concentrates the highest-risk pitfalls (path canonicalization, transaction behavior, WAL management).
**Delivers:** `glass_coordination` crate with full agent registry, atomic file locking, messaging, and stale pruning. All public APIs unit-tested.
**Addresses:** Agent registration, file locking, liveness detection, messaging, lock conflict reporting (all table stakes).
**Avoids:** UNC path mismatch (Pitfall 3), SQLITE_BUSY (Pitfall 2), WAL growth (Pitfall 1), prune race conditions (Pitfall 6), project scope mismatch (Pitfall 11).
**Stack:** rusqlite (existing), uuid (new), dunce (new), libc/windows-sys (existing).

### Phase 2: MCP Tools (Interface Layer)
**Rationale:** Agents cannot use coordination without MCP exposure. Depends on Phase 1 being complete. Follows established rmcp patterns closely, making it predictable.
**Delivers:** 11 new MCP tool handlers in glass_mcp, CoordinationDb wiring into GlassServer, updated server info instructions.
**Addresses:** MCP tool exposure (table stakes), implicit heartbeat on all tools (Pitfall 7 mitigation), retry_after_ms in conflict responses (Pitfall 5 mitigation).
**Avoids:** Connection sharing anti-pattern (Pitfall 10), message ordering assumptions (Pitfall 8).

### Phase 3: Integration Testing and CLAUDE.md
**Rationale:** The system only works if AI agents actually follow coordination instructions. This phase validates behavioral correctness, not just technical correctness. Can overlap with Phase 4 since it modifies different files.
**Delivers:** CLAUDE.md coordination protocol, multi-server integration tests, manual multi-agent validation.
**Addresses:** CLAUDE.md integration instructions (table stakes), structured message types (differentiator).
**Risk:** MEDIUM -- whether real Claude Code sessions reliably follow instructions is a behavioral question, not a technical one.

### Phase 4: GUI Integration (Visual Layer)
**Rationale:** Can start in parallel with Phase 3 once the DB schema is stable (Phase 1 complete). Modifies the hot rendering path, so changes must be minimal. Status bar is low-hanging fruit; tab indicators and conflict overlay are progressively harder.
**Delivers:** Status bar agent/lock count display, background polling thread, CoordinationDisplay struct.
**Addresses:** Status bar agent indicator (differentiator), tab-level indicators (differentiator, partial -- defer per-tab agent mapping).
**Avoids:** GUI polling overhead (Pitfall 9) -- 5-second background thread with atomics, not render-loop polling.
**Defers:** Conflict warning overlay to a future milestone (HIGH complexity, new overlay event types).

### Phase Ordering Rationale

- **Phase 1 before Phase 2:** glass_mcp depends on glass_coordination as a crate dependency. Cannot build tool handlers without the library.
- **Phase 2 before Phase 3:** CLAUDE.md instructions reference MCP tool names. Integration tests require working tools.
- **Phase 4 parallel with Phase 3:** GUI reads agents.db directly. Only needs the schema to be stable, not the MCP layer.
- **Risk-front-loaded:** Phase 1 contains all 6 critical pitfalls. Getting path canonicalization, transaction behavior, and WAL management right early prevents cascading problems.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 1 (locks.rs):** Path canonicalization edge cases on Windows need exhaustive testing -- junctions, symlinks, case sensitivity, non-existent files. Research the exact behavior of `dunce::canonicalize()` with NTFS junction points.
- **Phase 3 (CLAUDE.md):** Whether Claude Code reliably follows multi-step coordination protocols is an open behavioral question. May need iteration on instruction phrasing. Test with real 2-agent sessions.
- **Phase 4 (tab indicators):** Mapping MCP agent PID to Glass tab SessionId requires process tree walking. Research platform-specific APIs for parent PID resolution. May be infeasible -- defer if so.

Phases with standard patterns (skip research-phase):
- **Phase 1 (schema, agents, messages, prune):** Directly replicates glass_history/glass_snapshot patterns. WAL mode, PRAGMA config, user_version migrations are battle-tested in the codebase.
- **Phase 2 (MCP tools):** Exact replication of existing 5-tool pattern in glass_mcp/tools.rs. spawn_blocking, parameter structs with schemars, CallToolResult responses.
- **Phase 4 (status bar):** Extending build_status_text with one more optional parameter. Same pattern as when update_text was added.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | Only 2 new crates, both trivial. All other deps reused from workspace. Version compatibility verified via docs.rs. No novel technology. |
| Features | MEDIUM-HIGH | Core patterns converging across Claude Code Agent Teams, Warp 2.0, Overstory, mcp_agent_mail. Domain is young but the primitives are well-established. |
| Architecture | HIGH | Every pattern directly replicates existing codebase patterns. No architectural novelty. |
| Pitfalls | HIGH | SQLite concurrency pitfalls are thoroughly documented. Windows UNC path issue has a known solution. Sources include official SQLite docs and established Rust ecosystem documentation. |

**Overall confidence:** HIGH

### Gaps to Address

- **AI agent behavioral compliance:** Whether Claude Code reliably follows CLAUDE.md coordination instructions (heartbeat calls, lock-before-edit, check messages) is untestable until Phase 3 manual validation. If agents are unreliable, the system degrades to heartbeat-timeout-based cleanup only, which still works but is slower.
- **PID start-time verification complexity:** The research recommends verifying process start time alongside PID for robust stale detection, but the implementation is platform-specific. Decide in Phase 1 planning whether this is worth the complexity or whether heartbeat timeout alone is sufficient.
- **Tab-to-agent mapping:** Correlating MCP agent PIDs with Glass tab SessionIds requires process tree walking. No cross-platform Rust crate handles this cleanly. May need to accept aggregate state rather than per-tab agent identity.
- **Case-insensitive path matching on Windows:** NTFS has per-directory case sensitivity edge cases (Windows 10 1803+). Decide whether to always lowercase on Windows or respect per-directory settings.
- **Message retention policy:** No max age or pruning strategy for messages is specified in the design. Without it, the messages table grows unbounded. Add a `max_message_age` (24h) pruning step to `prune_stale()` before Phase 3.

## Sources

### Primary (HIGH confidence)
- [SQLite WAL mode documentation](https://sqlite.org/wal.html) -- concurrent access, checkpoint behavior, same-host requirement
- [rusqlite TransactionBehavior docs](https://docs.rs/rusqlite/latest/rusqlite/enum.TransactionBehavior.html) -- BEGIN IMMEDIATE support
- [uuid crate v1.22.0](https://docs.rs/uuid/latest/uuid/) -- v4 features, dependency footprint
- [dunce crate](https://docs.rs/dunce/latest/dunce/) -- UNC prefix stripping behavior
- [Rust std::fs::canonicalize UNC issue #42869](https://github.com/rust-lang/rust/issues/42869) -- Windows path problem
- [windows-sys Threading module](https://docs.rs/windows-sys/latest/windows_sys/Win32/System/Threading/index.html) -- OpenProcess, GetExitCodeProcess
- [Claude Code Agent Teams documentation](https://code.claude.com/docs/en/agent-teams) -- multi-agent coordination patterns
- Glass codebase: glass_history/db.rs, glass_mcp/tools.rs, glass_snapshot/ignore_rules.rs -- validated existing patterns

### Secondary (MEDIUM confidence)
- [Warp 2.0 Agentic Development Environment](https://www.warp.dev/blog/reimagining-coding-agentic-development-environment) -- agent status indicators, management panel
- [Overstory multi-agent orchestration](https://github.com/jayminwest/overstory) -- git worktree isolation, SQLite mail system
- [mcp_agent_mail](https://github.com/Dicklesworthstone/mcp_agent_mail) -- MCP-exposed agent mail with file reservations
- [SQLite busy_timeout pitfalls (Bert Hubert)](https://berthub.eu/articles/posts/a-brief-post-on-sqlite3-database-locked-despite-timeout/) -- transaction upgrade behavior
- [SQLite concurrent writes analysis](https://tenthousandmeters.com/blog/sqlite-concurrent-writes-and-database-is-locked-errors/) -- BEGIN IMMEDIATE vs DEFERRED
- [SkyPilot: Abusing SQLite for concurrency](https://blog.skypilot.co/abusing-sqlite-to-handle-concurrency/) -- multi-process SQLite coordination patterns

### Tertiary (LOW confidence)
- [The Heartbeat Pattern for AI Agents](https://dev.to/askpatrick/the-heartbeat-pattern-how-to-keep-ai-agents-alive-between-tasks-2b0p) -- heartbeat intervals and stale detection
- [PID Reuse Race Conditions (LWN.net)](https://lwn.net/Articles/773459/) -- Linux PID reuse timing
- [NTFS Case Sensitivity Internals](https://www.tiraniddo.dev/2019/02/ntfs-case-sensitivity-on-windows.html) -- per-directory case sensitivity edge cases

---
*Research completed: 2026-03-09*
*Ready for roadmap: yes*

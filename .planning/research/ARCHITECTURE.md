# Architecture Research

**Domain:** SOI & Agent Mode integration into Glass GPU terminal emulator (v3.0)
**Researched:** 2026-03-12
**Confidence:** HIGH — derived from direct code inspection of existing crates, not documentation alone

---

## System Overview

### Existing Pipeline (v2.5, unchanged for v3.0)

```
PTY reader thread (std::thread, blocking I/O)
    |
    ├── OscScanner (glass_terminal/osc_scanner.rs)
    |       Detects OSC 133 shell lifecycle sequences
    |       Emits AppEvent::Shell { ShellEvent::CommandFinished, ... }
    |
    ├── OutputBuffer (glass_terminal/output_capture.rs)
    |       Accumulates raw bytes between CommandExecuted and CommandFinished
    |       Emits AppEvent::CommandOutput { raw_output: Vec<u8> }
    |
    └── EventProxy (glass_terminal)
            Sends AppEvent → winit EventLoopProxy<AppEvent>

winit main event loop (src/main.rs, ~2200 lines)
    |
    ├── AppEvent::Shell { CommandFinished } → insert CommandRecord into HistoryDb
    |       location: main.rs:2664
    |       stores: command text, cwd, exit_code, started_at, finished_at, duration_ms
    |
    ├── AppEvent::CommandOutput { raw_output } → update_output() in HistoryDb
    |       location: main.rs:2882
    |       processes: ANSI strip, binary filter, truncation (glass_history::output::process_output)
    |       stores: processed text into commands.output column
    |
    └── AppEvent renders to wgpu via FrameRenderer (glass_renderer)

MCP server (glass_mcp, separate process `glass mcp serve`)
    IPC channel: named pipe (Windows) / Unix socket
    Reads HistoryDb directly via glass_history
    Sends MCP requests back to GUI via AppEvent::McpRequest
```

### New Pipeline (v3.0, SOI + Agent Mode)

```
PTY reader thread
    |
    ├── (unchanged) OscScanner → AppEvent::Shell
    └── (unchanged) OutputBuffer → AppEvent::CommandOutput

winit main event loop
    |
    ├── AppEvent::Shell { CommandFinished }
    |       (existing) → insert CommandRecord → HistoryDb → session.last_command_id = Some(id)
    |       (NEW) → signal SOI pipeline: (command_id, raw_output_ref, command_text, exit_code)
    |
    ├── AppEvent::CommandOutput
    |       (existing) → update_output() in HistoryDb
    |       (NEW) → store raw_output in session.pending_soi_output: Vec<u8>
    |
    ├── (NEW) AppEvent::SoiReady { command_id, summary }
    |       → store summary on Block for rendering (shell injection)
    |       → write summary line to PTY stdin (shell summary injection)
    |       → forward ActivityEvent to glass_agent activity stream
    |
    └── (NEW) AppEvent::AgentProposal { proposal }
            → add to Processor.pending_proposals queue
            → show toast notification via FrameRenderer
            → update status bar agent indicator

glass_soi crate (new, Tokio task, NOT a separate process)
    Spawned per CommandFinished via tokio::spawn in main event loop
    Receives: (command_id, command_text, raw_bytes, exit_code, db_path)
    Pipeline: classify → parse → compress → store → emit SoiReady

glass_agent crate (new, background OS process managed by glass_agent::Runtime)
    AgentRuntime struct in main process (not a separate process itself)
    Spawns: `claude` CLI as Child process
    Feeds: ActivityEvent stream via claude stdin
    Reads: AgentProposal from claude stdout
    Persists: agent_sessions table in HistoryDb

MCP server (glass_mcp, unchanged process boundary)
    (NEW) glass_query, glass_query_trend, glass_query_drill tools
    Read from new command_output_records + output_records tables in HistoryDb
```

---

## Component Responsibilities

### New: glass_soi

| Sub-component | Responsibility | Location in crate |
|---------------|----------------|-------------------|
| OutputClassifier | Detect output type from command text + output content | src/classifier.rs |
| Parser registry | Route to correct format parser | src/parsers/mod.rs |
| Rust/cargo parser | Parse cargo test, cargo build, cargo clippy | src/parsers/rust.rs |
| Jest/pytest/gotest parsers | Test runner extraction | src/parsers/test_runners.rs |
| npm/pip/cargo-add parsers | Package event extraction | src/parsers/pkg_mgr.rs |
| Git/docker/kubectl parsers | DevOps tool extraction | src/parsers/devops.rs |
| JSON/CSV parsers | Structured data extraction | src/parsers/structured.rs |
| CompressionEngine | Token-budgeted summaries at 4 levels | src/compression.rs |
| SoiDb | Write command_output_records + output_records tables | src/db.rs |
| SoiStore | Public API wrapping SoiDb + CompressionEngine | src/store.rs |

glass_soi depends on: glass_core (config), glass_errors (reuse existing parsers), serde_json, regex

### New: glass_agent

| Sub-component | Responsibility | Location in crate |
|---------------|----------------|-------------------|
| AgentRuntime | Spawn + manage `claude` CLI child process | src/runtime.rs |
| ActivityStream | Bounded mpsc channel, rolling budget window | src/activity.rs |
| WorktreeManager | git worktree create/diff/apply/cleanup | src/worktree.rs |
| ProposalQueue | Ordered list of pending AgentProposals | src/proposals.rs |
| SessionStore | Write/read agent_sessions table in HistoryDb | src/session_store.rs |

glass_agent depends on: glass_core (config, events), glass_soi (ActivityEvent), glass_history (agent_sessions table), tokio, uuid, serde_json

### Modified: glass_core (glass_core/src/event.rs)

Add new AppEvent variants:

```rust
// Add to AppEvent enum:
SoiReady {
    window_id: winit::window::WindowId,
    session_id: SessionId,
    command_id: i64,
    summary: String,           // one-line SOI summary for shell injection
    severity: glass_soi::Severity,
},
AgentProposal {
    proposal: Box<glass_agent::AgentProposal>,
},
AgentStatusChanged {
    mode: Option<glass_agent::AgentMode>,  // None = agent off
    proposal_count: usize,
},
```

### Modified: glass_history (glass_history/src/db.rs)

Add new tables to existing HistoryDb (schema v2 → v3):

```sql
-- Linked to commands.id (existing table)
CREATE TABLE command_output_records ( ... );  -- per-command SOI summary
CREATE TABLE output_records ( ... );          -- individual structured records
CREATE TABLE agent_sessions ( ... );          -- agent session lifecycle + handoff
```

Schema migration handled by existing migrate() pattern (PRAGMA user_version bump).

No new crate — SoiDb and SessionStore write into the same HistoryDb file via the same Connection pattern. This maintains the existing "open per-request in spawn_blocking" thread safety property.

### Modified: glass_mcp (crates/glass_mcp/src/tools.rs)

Add 3 new tools to GlassServer:
- `glass_query` — query structured output by command_id/scope/file/budget
- `glass_query_trend` — compare recent runs of same command pattern
- `glass_query_drill` — expand specific record_id for full detail

GlassServer already receives db_path; it will instantiate SoiStore from the same path.

### Modified: glass_renderer (crates/glass_renderer/src/)

Add new rendering components:

| Component | What it renders | Parallel to |
|-----------|-----------------|-------------|
| ToastRenderer | Agent proposal toast (auto-dismiss) | config_error_overlay.rs |
| AgentOverlayRenderer | Review overlay with diff preview | search_overlay_renderer.rs |
| Updated StatusBarRenderer | Agent mode indicator + proposal count | status_bar.rs (extend) |
| Updated BlockRenderer | SOI summary label on complete blocks | block_renderer.rs (extend) |

### Modified: src/main.rs

Two integration points require changes:

**Point 1 — CommandFinished handler (line 2664):**
After `db.insert_command(&record)` succeeds and `session.last_command_id = Some(id)`:
```rust
// Spawn SOI parsing as non-blocking Tokio task
if let Some(raw) = session.pending_soi_output.take() {
    let db_path = session.history_db_path.clone();
    let proxy = self.proxy.clone();
    let window_id = window_id;
    let session_id = session_id;
    tokio::task::spawn_blocking(move || {
        // glass_soi::pipeline::run(id, command_text, raw, exit_code, &db_path)
        // emits AppEvent::SoiReady on completion
    });
}
```

**Point 2 — CommandOutput handler (line 2882):**
Before or alongside update_output(), stash raw bytes for SOI:
```rust
session.pending_soi_output = Some(raw_output.clone());
```

**Point 3 — New AppEvent handlers:**
```
SoiReady → inject summary into PTY, forward to agent activity stream
AgentProposal → add to Processor.pending_proposals, trigger toast
AgentStatusChanged → update status bar render state
```

**Point 4 — Keyboard shortcuts:**
```
Ctrl+Shift+A → open agent review overlay
```

**Point 5 — Processor struct additions:**
```rust
agent_runtime: Option<glass_agent::AgentRuntime>,
pending_proposals: Vec<glass_agent::AgentProposal>,
agent_toast: Option<(AgentProposal, std::time::Instant)>,  // auto-dismiss
```

---

## Data Flow

### SOI Pipeline Data Flow

```
CommandExecuted OSC 133;C
    ↓
OutputBuffer starts accumulating raw PTY bytes
    ↓
CommandFinished OSC 133;D
    ↓
AppEvent::Shell { CommandFinished } → main loop
    ↓
db.insert_command() → command_id: i64          ← EXISTING
    ↓
AppEvent::CommandOutput → main loop
    ↓
process_output() → ANSI strip/truncate
    ↓
db.update_output(command_id, text)              ← EXISTING
session.pending_soi_output = Some(raw_bytes)    ← NEW (stash before/alongside)
    ↓
tokio::task::spawn_blocking (non-blocking)      ← NEW
    ↓
glass_soi::pipeline::run(command_id, command_text, raw_bytes, exit_code)
    |
    ├── OutputClassifier.classify(command_text, &raw_bytes) → OutputType
    ├── parser_registry.parse(output_type, &raw_bytes) → ParsedOutput
    ├── SoiDb.insert(command_id, &parsed_output)    → stored in HistoryDb
    └── CompressionEngine.compress(&parsed, budget=50) → one-line summary
    ↓
EventLoopProxy.send_event(AppEvent::SoiReady { command_id, summary, severity })
    ↓
main loop: SoiReady handler
    ├── write summary to PTY stdin (shell summary injection)
    └── send ActivityEvent to AgentRuntime.activity_tx
```

### Agent Mode Data Flow

```
AppEvent::SoiReady → ActivityEvent into AgentRuntime.activity_tx (mpsc)
    ↓
AgentRuntime background task (tokio::spawn)
    |
    ├── Collects ActivityEvents into rolling budget window
    ├── Every N seconds OR on high-severity event:
    |       Formats context JSON → writes to claude stdin
    ↓
claude CLI process (Child)
    |
    ├── Reads context from stdin
    ├── Calls glass_query MCP tool for drill-down
    └── Outputs AgentProposal JSON to stdout
    ↓
AgentRuntime reads from claude stdout
    ↓
EventLoopProxy.send_event(AppEvent::AgentProposal { proposal })
    ↓
main loop: AgentProposal handler
    ├── pending_proposals.push(proposal)
    ├── agent_toast = Some((proposal.clone(), Instant::now()))
    └── window.request_redraw()
    ↓
FrameRenderer.draw_frame()
    ├── ToastRenderer draws notification if agent_toast is Some and < 10s old
    └── StatusBarRenderer shows "Agent: N proposals" indicator
    ↓
User presses Ctrl+Shift+A
    ↓
AgentOverlayRenderer.draw_overlay(pending_proposals)
    |
    ├── Shows proposal list with diff preview
    └── [Apply] / [Dismiss] keyboard handling
        ↓
    Apply → WorktreeManager.apply(proposal.worktree_path)
    Dismiss → pending_proposals.remove(idx)
```

### Shell Summary Injection Data Flow

```
AppEvent::SoiReady arrives with summary="3 errors in src/auth.rs"
    ↓
main loop:
    let summary_line = format!("\r\n\x1b[2m⎡ Glass: {} │ glass_query(\"last\") for details ⎤\x1b[0m\r\n", summary);
    session.pty_sender.send(PtyMsg::Write(summary_line.into_bytes()))
    ↓
PTY receives bytes → terminal renders muted summary line
↓
Claude Code Bash tool sees the summary line in its output capture
↓
Claude Code calls glass_query("last") MCP tool for structured drill-down
```

---

## New Crate Structure

### glass_soi

```
crates/glass_soi/
├── Cargo.toml
└── src/
    ├── lib.rs              # Public API: OutputType, ParsedOutput, OutputRecord, Severity
    ├── classifier.rs       # OutputClassifier: command hint + regex pattern matching
    ├── compression.rs      # CompressionEngine: token-budgeted summaries
    ├── db.rs               # SoiDb: write/read command_output_records + output_records
    ├── store.rs            # SoiStore: high-level API combining db + compression
    ├── pipeline.rs         # run(): orchestrates classify→parse→store→return summary
    └── parsers/
        ├── mod.rs          # Parser registry + trait
        ├── rust.rs         # cargo build, cargo test, cargo clippy, rustc
        ├── test_runners.rs # jest, pytest, go test, generic TAP
        ├── pkg_mgr.rs      # npm, pip, cargo add/update
        ├── devops.rs       # git, docker, kubectl, terraform
        └── structured.rs   # JSON lines, JSON object, CSV
```

Key design: parsers implement a common `Parser` trait returning `ParsedOutput`. The registry matches `OutputType` to the correct parser. Adding new parsers requires no changes to pipeline.rs.

### glass_agent

```
crates/glass_agent/
├── Cargo.toml
└── src/
    ├── lib.rs              # Public types: AgentProposal, AgentMode, ActivityEvent
    ├── runtime.rs          # AgentRuntime: spawn/manage claude CLI child process
    ├── activity.rs         # ActivityStream: bounded channel + rolling budget window
    ├── worktree.rs         # WorktreeManager: git worktree create/diff/apply/cleanup
    ├── proposals.rs        # ProposalQueue: ordered pending proposals with max limit
    └── session_store.rs    # SessionStore: agent_sessions table reads/writes
```

Key design: AgentRuntime is NOT a separate process — it is a struct held in Processor in main.rs. It manages the `claude` CLI child process internally. This matches the existing pattern of background pollers (coordination_poller, update_checker) being spawned from main.rs.

---

## Architectural Patterns

### Pattern 1: Async Off Main Thread via spawn_blocking

**What:** SOI parsing is CPU-bound work. It must not block winit's event loop (which handles rendering). Use `tokio::task::spawn_blocking` with a channel-back to main loop via EventLoopProxy.

**When to use:** Any work >1ms that can be deferred after CommandFinished.

**Trade-offs:** Slight delay (1-100ms) between command finishing and summary appearing. Acceptable — the next prompt renders immediately regardless.

**Example:**
```rust
// In AppEvent::CommandOutput handler, after update_output():
let proxy = self.proxy.clone();
let window_id = *window_id;
let session_id = *session_id;
let db_path = session.history_db_path.clone();
tokio::task::spawn_blocking(move || {
    match glass_soi::pipeline::run(cmd_id, &command_text, &raw_bytes, exit_code, &db_path) {
        Ok((summary, severity)) => {
            let _ = proxy.send_event(AppEvent::SoiReady {
                window_id, session_id, command_id: cmd_id, summary, severity,
            });
        }
        Err(e) => tracing::warn!("SOI pipeline failed: {}", e),
    }
});
```

Note: Glass already runs a Tokio runtime (for the MCP IPC listener). `tokio::task::spawn_blocking` is available.

### Pattern 2: Open-Per-Request SQLite for New Tables

**What:** SoiDb and SessionStore open the HistoryDb connection fresh per operation (not holding a persistent Connection). This matches the existing pattern in glass_mcp (GlassServer opens HistoryDb in spawn_blocking per request).

**When to use:** Any crate that needs DB access but isn't the primary owner of the connection.

**Trade-offs:** Slight overhead per operation. WAL mode + busy_timeout=5000ms prevents conflicts. Acceptable at command-level granularity (not per-keystroke).

**Why not a shared Arc<Mutex<HistoryDb>>:** Connection is !Send in rusqlite by default. The open-per-request pattern avoids this entirely, consistent with all existing DB users in the codebase.

### Pattern 3: Overlay Pattern for New UI (Toast + Agent Review)

**What:** New UI elements follow the existing overlay pattern: stateless renderers passed data from main.rs, drawn on top of the terminal frame via additional draw calls after the main frame.

**When to use:** Any UI that doesn't affect the terminal grid itself.

**Existing overlays to model after:**
- `config_error_overlay.rs` — single-line banner, drawn after main frame
- `conflict_overlay.rs` — amber warning with text, drawn after main frame
- `search_overlay_renderer.rs` — complex scrollable list

**Trade-offs:** Each overlay is an additional GPU draw call. Acceptable — overlays are rare (not every frame).

### Pattern 4: AppEvent for Cross-Thread Communication

**What:** All cross-thread communication flows through `AppEvent` via `EventLoopProxy<AppEvent>`. New components (SOI pipeline, AgentRuntime) use the same proxy pattern.

**When to use:** Any background thread/task that needs to update GUI state.

**Trade-offs:** All GUI state changes are serialized through the winit event loop. Cannot update renderer state directly from background threads. This is a feature — prevents data races.

---

## Integration Points: New vs Modified

### Unmodified (no changes needed)

| Component | Why unchanged |
|-----------|---------------|
| glass_terminal (PTY, VT, block_manager) | SOI taps captured bytes in main.rs, not in glass_terminal |
| glass_pipes | No interaction with SOI — pipeline stages are separate |
| glass_snapshot | No interaction with SOI or agent mode |
| glass_coordination | Agent mode uses it for lock management via MCP (already exists) |
| glass_mux (SessionMux, SplitTree) | No changes |
| alacritty_terminal | No changes |
| Shell integration scripts | No changes — summary injection happens via PTY write, not shell scripts |

### Modified (extend existing)

| Component | What changes | Where |
|-----------|-------------|-------|
| glass_core/event.rs | +3 AppEvent variants (SoiReady, AgentProposal, AgentStatusChanged) | event.rs |
| glass_history/db.rs | +3 new tables, schema v2→v3 migration | db.rs, migrate() |
| glass_history/lib.rs | Export SoiDb and SessionStore types | lib.rs |
| glass_mcp/tools.rs | +3 new MCP tools (glass_query, glass_query_trend, glass_query_drill) | tools.rs |
| glass_renderer/frame.rs | +ToastRenderer, +AgentOverlayRenderer draw calls | frame.rs |
| glass_renderer/status_bar.rs | Agent mode indicator + proposal count text | status_bar.rs |
| glass_renderer/block_renderer.rs | SOI summary label on complete blocks (optional) | block_renderer.rs |
| src/main.rs | SOI spawn in CommandFinished/CommandOutput handlers; new AppEvent arms; agent keyboard shortcuts; Processor state fields | main.rs |

### New (create from scratch)

| Component | Location | Dependencies |
|-----------|----------|-------------|
| glass_soi crate | crates/glass_soi/ | glass_core, glass_errors, serde_json, regex |
| glass_agent crate | crates/glass_agent/ | glass_core, glass_soi, glass_history, tokio, uuid, serde_json |

---

## Build Order

The dependency graph dictates this order. Each phase must compile before the next.

```
Phase 1: glass_soi crate core (classifier + parsers + types)
    No new crate dependencies. Can be built and tested in isolation.
    Produces: OutputType, ParsedOutput, OutputRecord, OutputSummary

Phase 2: glass_history schema extension (new tables)
    Depends on: Phase 1 types (ParsedOutput for DB shape)
    Produces: SoiDb, SessionStore, schema v3 migration

Phase 3: glass_soi pipeline integration (SOI fires on CommandFinished)
    Depends on: Phase 1 + Phase 2
    Modifies: glass_core/event.rs (+SoiReady), src/main.rs (spawn SOI task)
    Produces: End-to-end SOI parsing on every command

Phase 4: glass_soi compression engine
    Depends on: Phase 1 (ParsedOutput types)
    No new crate changes — internal to glass_soi
    Produces: CompressionEngine with OneLine/Summary/Detailed/Full levels

Phase 5: SOI shell summary injection
    Depends on: Phase 3 (SoiReady AppEvent exists), Phase 4 (summaries exist)
    Modifies: src/main.rs (SoiReady handler writes to PTY)
    Produces: Summary lines visible in terminal after commands

Phase 6: SOI MCP tools
    Depends on: Phase 2 (SoiDb queryable), Phase 4 (CompressedOutput type)
    Modifies: glass_mcp/tools.rs (+3 tools)
    Produces: glass_query, glass_query_trend, glass_query_drill

Phase 7: SOI additional parsers
    Depends on: Phase 1 (parser trait)
    Internal to glass_soi/parsers/
    Produces: Coverage for 10+ dev tools

Phase 8: glass_agent activity stream
    Depends on: Phase 3 (SoiReady emits ActivityEvent)
    Creates: glass_agent crate with ActivityStream
    Produces: Bounded activity channel with rolling budget

Phase 9: glass_agent runtime
    Depends on: Phase 8 (ActivityStream), Phase 2 (agent_sessions table)
    Creates: AgentRuntime, spawns claude CLI
    Modifies: src/main.rs (agent_runtime field in Processor, AppEvent::AgentProposal handler)
    Produces: Background agent session, structured proposals

Phase 10: WorktreeManager
    Depends on: Phase 9 (AgentProposal::CodeFix type defined)
    Internal to glass_agent/worktree.rs
    Produces: Isolated git worktree for agent code changes

Phase 11: Approval UI
    Depends on: Phase 9 (proposals exist), Phase 10 (worktree diffs)
    Modifies: glass_renderer (+ToastRenderer, +AgentOverlayRenderer, status_bar update)
    Modifies: src/main.rs (keyboard shortcuts, toast state in Processor)
    Produces: Status bar indicator, toast notifications, Ctrl+Shift+A review overlay

Phase 12: Session continuity
    Depends on: Phase 9 (agent_sessions table), Phase 2 (SessionStore)
    Internal to glass_agent/session_store.rs + runtime.rs
    Produces: Handoff JSON on session end, restored context on new session start

Phase 13: Configuration and polish
    Depends on: All above
    Modifies: glass_core/config.rs (+[soi] and [agent] sections), src/main.rs
    Produces: Full config.toml support, permission system, graceful degradation
```

**Critical dependency:** Phases 1-3 must complete before any agent work (Phases 8+) because the agent runtime feeds on SOI events. Phases 1-3 are also independently shippable — SOI without agent mode is immediately useful.

---

## Scaling Considerations

Glass is a local desktop application. "Scaling" means handling edge cases under heavy usage, not distributed load.

| Concern | Risk | Mitigation |
|---------|------|-----------|
| SOI parsing large output (10MB+ logs) | Parser hangs, high memory | Truncate at 1MB before classifying; FreeformChunk fallback for unrecognized |
| output_records table growth | DB size explodes over weeks | Aligned with existing retention/pruning via FK cascade from commands table |
| Agent activity stream flooding | 100 commands/minute overwhelms context | Rate limiting + deduplication in ActivityStream; rolling window evicts old events |
| claude CLI process crash | AgentRuntime in broken state | Restart with exponential backoff; proposals queue preserved across restarts |
| claude CLI API rate limits | Agent blocked, proposal latency | Cooldown timer (30s default) prevents bursts; graceful degradation (silent, no crash) |
| shell summary injection timing | Summary appears before next prompt | SoiReady arrives async; injection timing relies on PTY buffering being fast enough. Risk: summary interleaved with prompt. Mitigation: prepend \r\n, append \r\n |
| HistoryDb lock contention | SOI + MCP + main loop all writing | WAL mode + PRAGMA busy_timeout=5000 handles this; open-per-request prevents long locks |

---

## Anti-Patterns to Avoid

### Anti-Pattern 1: Blocking the Winit Event Loop with SOI Parsing

**What people do:** Call `glass_soi::pipeline::run(...)` directly in the CommandFinished handler in main.rs.

**Why it's wrong:** SOI parsing involves regex matching, JSON serialization, and SQLite writes. Even at 10ms, this blocks the event loop and makes the terminal feel sluggish. On large outputs (cargo test with 500 tests), it could take 100ms+.

**Do this instead:** `tokio::task::spawn_blocking(|| ...)` and emit `AppEvent::SoiReady` when done. The Tokio runtime for the IPC listener is already initialized in main.rs — reuse it.

### Anti-Pattern 2: glass_soi Holding a Persistent SQLite Connection

**What people do:** Store `conn: Connection` in `SoiStore` and reuse it across calls.

**Why it's wrong:** rusqlite Connection is !Send. Storing it in a struct that crosses thread boundaries (spawn_blocking) requires unsafe Send impl or Arc<Mutex<>> gymnastics. All existing DB users in Glass use open-per-request.

**Do this instead:** Open the Connection at the start of `pipeline::run()`, do all operations, close it. This is the established pattern in glass_mcp/tools.rs and glass_snapshot.

### Anti-Pattern 3: Making glass_agent a Separate Process

**What people do:** Implement `glass agent serve` as a second long-running process, with IPC to the GUI.

**Why it's wrong:** Doubles the IPC complexity already present (MCP server is already a separate process). Agent runtime needs tight coupling with the GUI (real-time proposal delivery, toast auto-dismiss timers). Separate process adds latency and failure modes.

**Do this instead:** AgentRuntime as a struct in Processor (main.rs), managing the claude CLI child process internally. This is the same pattern as glass_core::coordination_poller::spawn_coordination_poller — a background Tokio task that sends AppEvents.

### Anti-Pattern 4: Injecting SOI Summary via Shell Integration Scripts

**What people do:** Modify glass.bash/glass.zsh to emit the SOI summary after each command.

**Why it's wrong:** Shell integration scripts can't call into Rust to get the SOI result. The SOI pipeline runs asynchronously after the shell emits CommandFinished. The timing gap makes this unreliable.

**Do this instead:** Write the summary directly to the PTY's input side (via `session.pty_sender.send(PtyMsg::Write(...))`) in the SoiReady handler. The PTY delivers it to the terminal emulator as if it were output, appearing after the command output but before the next prompt is drawn.

### Anti-Pattern 5: glass_soi Depending on glass_history

**What people do:** Import glass_history::HistoryDb directly in glass_soi and write records there.

**Why it's wrong:** Creates a direct dependency between glass_soi and glass_history. The SOI DB operations should be self-contained within glass_soi (using raw rusqlite, same as glass_history does internally).

**Do this instead:** glass_soi defines its own SoiDb struct that opens the same HistoryDb *file path* but manages its own Connection. The tables are co-located in the same SQLite file but owned by different crates. This matches how glass_pipes data is stored via glass_history::HistoryDb::insert_pipe_stages() — the caller (main.rs) bridges between the two crates. For SOI, the bridge is `pipeline::run(command_id, ..., &db_path)`.

---

## Integration Boundaries Summary

```
glass_core ←── glass_soi ───→ (rusqlite directly)
     ↑               ↓
  AppEvent      ActivityEvent
     |               |
src/main.rs ←── glass_agent ──→ glass_history (db_path only)
     |
     ↓
glass_renderer (ToastRenderer, AgentOverlayRenderer)
     +
glass_mcp (glass_query tools) ←── glass_soi::SoiStore
```

**Key boundary rule:** glass_soi does NOT import glass_history. It receives a `db_path: &Path` and opens its own connection. Main.rs bridges the two by passing `command_id` (from history DB insert) into the SOI pipeline.

---

## Sources

- Direct code inspection: `src/main.rs` (lines 2664-2752, 2882-2917) — CommandFinished and CommandOutput handlers
- Direct code inspection: `crates/glass_core/src/event.rs` — AppEvent variants and ShellEvent
- Direct code inspection: `crates/glass_history/src/db.rs` — HistoryDb open-per-request pattern
- Direct code inspection: `crates/glass_mcp/src/lib.rs` — MCP server spawn_blocking pattern
- Direct code inspection: `crates/glass_renderer/src/frame.rs` — overlay draw call pattern
- Direct code inspection: `crates/glass_terminal/src/block_manager.rs` — Block lifecycle
- `.planning/PROJECT.md` — SOI_AND_AGENT_MODE.md feature spec with full type definitions
- `SOI_AND_AGENT_MODE.md` — Architecture diagram, phase breakdown, risk table

---

*Architecture research for: Glass v3.0 SOI & Agent Mode integration*
*Researched: 2026-03-12*

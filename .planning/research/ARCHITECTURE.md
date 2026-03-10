# Architecture Patterns

**Domain:** Agent MCP Features (v2.3) for GPU-accelerated terminal emulator
**Researched:** 2026-03-09

## Current Architecture (Relevant Subset)

```
┌─────────────────────────────────────────────────────────────────┐
│  glass mcp serve (SEPARATE PROCESS)                             │
│  ┌───────────────┐                                              │
│  │  GlassServer   │──→ SQLite only (history.db, snapshots.db,   │
│  │  (rmcp stdio)  │       agents.db)                            │
│  │  16 tools      │  spawn_blocking per request                 │
│  └───────────────┘                                              │
│  NO access to: SessionMux, PTY, terminal grids, BlockManager   │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│  glass (MAIN GUI PROCESS)                                       │
│  ┌────────────┐    ┌──────────────┐    ┌─────────────────────┐  │
│  │ winit       │    │ SessionMux   │    │ PTY reader threads  │  │
│  │ event loop  │◄──→│ tabs[]       │    │ (std::thread)       │  │
│  │ user_event()│    │ sessions{}   │    │ → EventProxy →      │  │
│  │             │    │              │    │   AppEvent to loop   │  │
│  └────────────┘    └──────────────┘    └─────────────────────┘  │
│        ▲                  │                                      │
│        │            ┌─────┴─────┐                                │
│        │            │ Session   │                                 │
│        │            │ .term     │ Arc<FairMutex<Term>>            │
│        │            │ .pty_sender│ PtyMsg mpsc channel            │
│        │            │ .block_mgr │ command lifecycle tracking     │
│        │            │ .history_db│ per-session SQLite             │
│        │            │ .snapshot  │ per-session blob store         │
│        │            └───────────┘                                │
└─────────────────────────────────────────────────────────────────┘
```

**The fundamental gap:** MCP server (separate process) cannot access live session state. All features requiring live data (tab orchestration, live output, command status/cancel) need a communication bridge between the MCP server and the main event loop.

## Recommended Architecture: Embedded MCP with Async Channel Bridge

### The Core Decision: Embed MCP in the GUI Process

Move the MCP server from a separate process into the main GUI process as a tokio task. The MCP server communicates with the winit event loop via a bounded `tokio::sync::mpsc` channel, with per-request `tokio::sync::oneshot` channels for responses.

**Why in-process, not IPC:**
- Avoids named pipes (Windows) or Unix sockets (macOS/Linux) for cross-process communication
- Zero serialization overhead for grid snapshots and terminal state
- Still isolated: MCP runs on its own tokio task, not on the winit event loop thread
- rmcp supports any `AsyncRead + AsyncWrite` transport; embedding changes nothing about the protocol
- The `glass mcp serve` CLI subcommand remains for backward compatibility and DB-only tools

**Why not keep out-of-process and add IPC:**
- Would require a listener socket in the GUI process + client in MCP process
- Platform-divergent socket implementations (named pipes vs Unix sockets)
- Serialization/deserialization overhead for terminal grid content
- Two codepaths: DB-only external + live-data internal
- Process lifecycle coordination (what if GUI crashes? what if MCP outlives GUI?)

### Target Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│  glass (MAIN GUI PROCESS)                                           │
│                                                                     │
│  ┌──────────────────┐                                               │
│  │ Tokio runtime     │                                               │
│  │  ┌──────────────┐ │   tokio::sync::mpsc        ┌───────────────┐ │
│  │  │ MCP Server   │─│───McpRequest────────────────│ Bridge task   │ │
│  │  │ (rmcp stdio) │ │   + oneshot::Sender         │               │ │
│  │  │ GlassServer  │ │                             │ recv request  │ │
│  │  │ 28 tools     │ │                             │ → proxy.send  │ │
│  │  └──────────────┘ │                             │   (AppEvent:: │ │
│  └──────────────────┘                              │    Mcp)       │ │
│                                                     └───────┬───────┘ │
│                                                             │         │
│  ┌────────────┐    ┌──────────────┐         AppEvent::Mcp   │         │
│  │ winit       │◄───────────────────────────────────────────┘         │
│  │ event loop  │    │ SessionMux   │                                   │
│  │ user_event()│───→│ tabs/panes   │──→ Process request               │
│  │  match Mcp  │    │ sessions     │     Reply via oneshot            │
│  └────────────┘    └──────────────┘                                   │
│        ▲                                                              │
│  ┌─────┴──────────────────────────────────────────┐                   │
│  │ PTY reader threads (std::thread, unchanged)     │                   │
│  └─────────────────────────────────────────────────┘                   │
└─────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────┐
│  glass mcp serve             │  (STILL WORKS — backward compat)
│  DB-only tools (16 existing) │  No channel, no live data
└──────────────────────────────┘
```

### Component Boundaries

| Component | Responsibility | Status | Modification |
|-----------|---------------|--------|-------------|
| `glass_core/mcp_channel.rs` | McpRequest/McpResponse enums, channel type aliases | **NEW** | ~80 lines |
| `glass_core/event.rs` | New `AppEvent::Mcp(McpRequest)` variant | **MODIFY** | +2 lines |
| `glass_errors/` | Pure error parsing library (`&str` -> `Vec<ParsedError>`) | **NEW CRATE** | ~600 lines |
| `glass_mcp/tools.rs` | 12 new MCP tool handlers, `McpSender` field on GlassServer | **MODIFY** | +500 lines |
| `glass_mcp/lib.rs` | New `run_mcp_server_embedded()` accepting sender | **MODIFY** | +25 lines |
| `glass_mcp/Cargo.toml` | Add deps: glass_core, glass_errors, similar, regex | **MODIFY** | +4 lines |
| `src/main.rs` | Spawn embedded MCP, bridge task, handle AppEvent::Mcp | **MODIFY** | +300 lines |
| `Cargo.toml` (workspace) | Add glass_errors to workspace members, add similar + regex | **MODIFY** | +5 lines |

### Components NOT Modified

| Component | Why Unchanged |
|-----------|---------------|
| `glass_terminal` | No terminal emulation changes; grid reads use existing FairMutex API |
| `glass_renderer` | No rendering changes for v2.3; output goes through MCP, not GUI |
| `glass_mux` | SessionMux API is sufficient; new methods not needed |
| `glass_history` | Existing query API covers glass_output, glass_cached_result needs |
| `glass_snapshot` | Existing API covers glass_changed_files needs |
| `glass_coordination` | Already shipped in v2.2, no changes needed |
| `glass_pipes` | Pipe capture is orthogonal to MCP features |

## New Component: MCP Channel Types

Lives in `glass_core` because `event.rs` (which defines `AppEvent`) already lives there. McpRequest must be in the same crate as AppEvent to be a variant payload.

```rust
// glass_core/src/mcp_channel.rs

use tokio::sync::oneshot;
use serde_json::Value;

/// Request from an MCP tool handler to the main event loop.
/// Each variant carries a oneshot::Sender for the response.
pub enum McpRequest {
    // --- Tab Orchestration ---
    TabCreate {
        name: Option<String>,
        shell: Option<String>,
        cwd: Option<String>,
        reply: oneshot::Sender<McpResponse>,
    },
    TabList {
        reply: oneshot::Sender<McpResponse>,
    },
    TabRun {
        tab_index: usize,
        command: String,
        reply: oneshot::Sender<McpResponse>,
    },
    TabOutput {
        tab_index: usize,
        lines: Option<usize>,
        pattern: Option<String>,
        reply: oneshot::Sender<McpResponse>,
    },
    TabClose {
        tab_index: usize,
        reply: oneshot::Sender<McpResponse>,
    },

    // --- Live Command Awareness ---
    CommandStatus {
        tab_index: Option<usize>,
        reply: oneshot::Sender<McpResponse>,
    },
    CommandCancel {
        tab_index: Option<usize>,
        reply: oneshot::Sender<McpResponse>,
    },
}

pub enum McpResponse {
    Ok(Value),
    Error(String),
}

pub type McpSender = tokio::sync::mpsc::Sender<McpRequest>;
pub type McpReceiver = tokio::sync::mpsc::Receiver<McpRequest>;
```

**Why McpRequest lives in glass_core, not glass_mcp:**
- `AppEvent::Mcp(McpRequest)` needs McpRequest in the same crate as AppEvent
- glass_core is the natural hub crate (event.rs, config.rs, coordination_poller.rs)
- Avoids circular dependency: main.rs depends on glass_mcp AND glass_core; if McpRequest were in glass_mcp, glass_core couldn't reference it in AppEvent

**Tokio dependency in glass_core:** glass_core currently does not depend on tokio. Adding `tokio = { workspace = true, features = ["sync"] }` is necessary for `oneshot::Sender` and `mpsc::Sender/Receiver`. This is a lightweight addition (sync feature only, no runtime). Alternative: define channel types without tokio using std channels, but tokio channels are needed because MCP tools are async and need `await`-able oneshot receivers.

## Data Flow Diagrams

### Tab Orchestration: glass_tab_run

```
AI Agent (Claude Code, via stdio JSON-RPC)
    │
    ▼
rmcp transport layer (tokio task in GUI process)
    │
    ▼
GlassServer.glass_tab_run(tab_index=2, command="cargo test")
    │
    ├── let (tx, rx) = oneshot::channel()
    ├── mcp_sender.send(McpRequest::TabRun { tab_index: 2,
    │                     command: "cargo test\n", reply: tx })
    │
    ▼ (awaits rx)                        Bridge task
                                          │
                        mcp_rx.recv() ────┘
                                          │
                        proxy.send_event(AppEvent::Mcp(request))
                                          │
                                          ▼
main.rs user_event(AppEvent::Mcp(McpRequest::TabRun { .. }))
    │
    ├── let tab = session_mux.tabs()[2]
    ├── let session = session_mux.session(tab.focused_pane)
    ├── session.pty_sender.send(PtyMsg::Input("cargo test\n".bytes()))
    ├── reply.send(McpResponse::Ok(json!({"ok": true})))
    │
    ▼ (oneshot fires)

rx.await → McpResponse::Ok → CallToolResult::success
    │
    ▼
AI Agent receives { "ok": true }
```

### Tab Output: glass_tab_output

```
McpRequest::TabOutput { tab_index: 2, lines: 20, pattern: None, reply }
    │
    ▼ (in main.rs user_event)

1. Resolve tab → session
2. let term = session.term.lock()        // FairMutex, <1ms hold
3. Read last 20 lines from grid content   // scrollback + visible
4. drop(term)                             // release lock immediately
5. Strip ANSI escape sequences
6. Check session.block_manager for Executing state
7. reply.send(McpResponse::Ok(json!({
       "output": stripped_text,
       "total_lines": total,
       "has_running_command": is_executing
   })))
```

### Error Extraction: glass_errors (No Channel — DB Path)

```
AI Agent calls glass_errors(command_id=42)
    │
    ▼
GlassServer.glass_errors():
    │
    ├── spawn_blocking:
    │   ├── HistoryDb::open(db_path)
    │   ├── db.get_command(42) → CommandRecord { output, command_text, .. }
    │   └── glass_errors::parse(output, Some(command_text))
    │       └── Returns Vec<ParsedError>
    │
    ├── Serialize to JSON
    └── CallToolResult::success
```

### Token-Saving: glass_changed_files (No Channel — Snapshot DB)

```
AI Agent calls glass_changed_files(command_id=42)
    │
    ▼
GlassServer.glass_changed_files():
    │
    ├── spawn_blocking:
    │   ├── SnapshotStore::open(glass_dir)
    │   ├── store.get_snapshot_files(42)
    │   ├── For each file:
    │   │   ├── Read blob content from blob store
    │   │   ├── Read current file from disk
    │   │   └── similar::TextDiff::from_lines(old, new).unified_diff()
    │   └── Return file list with diffs
    │
    ├── Serialize to JSON
    └── CallToolResult::success
```

### Embedded MCP Server Startup

```rust
// In main.rs, during App initialization:

// 1. Create MCP command channel (bounded at 32 — more than enough
//    for sequential MCP tool calls from an AI agent)
let (mcp_tx, mcp_rx) = tokio::sync::mpsc::channel::<McpRequest>(32);

// 2. Spawn embedded MCP server on tokio runtime
//    Uses stdin/stdout for rmcp transport (same as glass mcp serve)
let tokio_rt = tokio::runtime::Runtime::new().unwrap();
tokio_rt.spawn(async move {
    if let Err(e) = glass_mcp::run_mcp_server_embedded(mcp_tx).await {
        tracing::error!("Embedded MCP server error: {}", e);
    }
});

// 3. Bridge task: forward McpRequests as AppEvents to winit event loop
let proxy_for_mcp = event_loop_proxy.clone();
tokio_rt.spawn(async move {
    while let Some(request) = mcp_rx.recv().await {
        let _ = proxy_for_mcp.send_event(AppEvent::Mcp(request));
    }
});
```

**Why a bridge task instead of direct try_recv in the event loop:**
- `mpsc::Receiver` is not `Sync`, so it cannot be polled from the winit event loop
- The bridge task converts channel messages to AppEvents, which is the established pattern for all async event sources (PTY reader threads use EventProxy, config watcher uses EventLoopProxy, coordination poller uses EventLoopProxy)
- Consistent: user_event() handles ALL events uniformly

**Startup sequencing concern:** The embedded MCP server reads stdin. But `glass` is a GUI app (`#![windows_subsystem = "windows"]`). stdin is not connected when launched from a desktop shortcut. The MCP server must only be spawned when stdin is available (i.e., when invoked from an existing terminal or by an AI agent). Detection: check if stdin is a pipe or connected.

**Resolution:** The embedded MCP server should be opt-in. Either:
- A `--mcp` flag on the glass binary: `glass --mcp` spawns the embedded server
- Or: always spawn, but use a named pipe / Unix socket instead of stdio

**Recommended:** Use `--mcp` flag. When present, spawn the embedded server with stdio transport. When absent (normal GUI launch), do not spawn MCP. This matches the existing `glass mcp serve` pattern but keeps the server in-process.

Alternative for agents: The agent's MCP config points to `glass mcp serve` (separate process). For live-data tools, the separate process communicates with the GUI process via a lightweight IPC mechanism (e.g., a Unix domain socket or Windows named pipe that the GUI always listens on).

**Simplest viable approach for v2.3:** Keep `glass mcp serve` as separate process. Add a small TCP/Unix socket server in the GUI process that handles only the 7 live-data requests (TabCreate, TabList, TabRun, TabOutput, TabClose, CommandStatus, CommandCancel). The MCP server connects to this socket when it needs live data. This avoids the stdin/GUI conflict entirely.

### Revised Architecture: Hybrid Approach

```
┌──────────────────────────────────────────────────────────────┐
│  glass mcp serve (SEPARATE PROCESS, as today)                │
│  ┌───────────────┐                                           │
│  │  GlassServer   │──→ SQLite (DB-only tools: 16 existing    │
│  │  (rmcp stdio)  │       + glass_output, glass_cached_result│
│  │  28 tools      │       + glass_changed_files, glass_errors│
│  │                │       + glass_context budget)             │
│  │                │                                           │
│  │  Live tools:   │──→ Connect to GUI's IPC socket            │
│  │  tab_*, cmd_*  │    for live session data                  │
│  └───────────────┘                                           │
└──────────────────────────────────────────────────────────────┘
         │ IPC (localhost TCP or named pipe)
         ▼
┌──────────────────────────────────────────────────────────────┐
│  glass (MAIN GUI PROCESS)                                    │
│  ┌────────────────┐                                          │
│  │ IPC Listener    │  Lightweight: only McpRequest/McpResponse│
│  │ (tokio task)    │  JSON over TCP/pipe                      │
│  │                 │──→ EventLoopProxy::send(AppEvent::Mcp)   │
│  └────────────────┘                                          │
│         ▲                                                    │
│  ┌──────┴─────┐    ┌──────────────┐                          │
│  │ winit loop  │    │ SessionMux   │                          │
│  │ user_event  │───→│ process req  │──→ reply via oneshot     │
│  └────────────┘    └──────────────┘                          │
└──────────────────────────────────────────────────────────────┘
```

**Trade-off analysis:**

| Approach | Pros | Cons |
|----------|------|------|
| Embedded MCP (in-process) | No IPC, no serialization, simplest data flow | stdin conflict with GUI, only works when launched by agent |
| Hybrid (separate MCP + GUI IPC) | MCP stays external (proven), GUI always works standalone | Requires IPC server in GUI, serialization for live data |
| Fully embedded with --mcp flag | Clean when it works | Two launch modes to maintain, agent must know to use --mcp |

**Recommendation: Hybrid approach.** The GUI process starts a lightweight IPC listener (tokio TcpListener on localhost or named pipe). The MCP server process (launched by agents via `glass mcp serve`) connects to it when live-data tools are called. DB-only tools continue working without the GUI running.

**IPC discovery:** The GUI writes its listener address to `~/.glass/gui.sock` (or `~/.glass/gui.port`). The MCP server reads this file to discover the GUI. If the file doesn't exist or connection fails, live-data tools return a clear error: "Glass GUI is not running. Live-data tools require the Glass terminal to be open."

### IPC Protocol (Minimal)

```rust
// Sent over TCP/named pipe as JSON lines
#[derive(Serialize, Deserialize)]
pub struct IpcRequest {
    pub id: u64,
    pub method: String,          // "tab_create", "tab_list", etc.
    pub params: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
pub struct IpcResponse {
    pub id: u64,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}
```

JSON lines over TCP on localhost. Simple, debuggable, cross-platform. No need for a full RPC framework — there are only 7 methods.

## New Crate: `glass_errors`

Pure library crate for structured error extraction from compiler/test output.

```
crates/glass_errors/
    Cargo.toml          - deps: regex (lazy_static or once_cell for compiled regexes)
    src/
        lib.rs          - ParsedError, Severity, parse() dispatcher
        detect.rs       - Format auto-detection from command hint + content patterns
        rust.rs         - Rust/cargo: error[E0308] + --> file:line:col, cargo test failures
        python.rs       - Python: Traceback + File "x", line N
        node.rs         - Node/TS: SyntaxError/TypeError with .js/.ts paths
        go.rs           - Go: file.go:line:col: message
        gcc.rs          - GCC/Clang: file:line:col: error: message
        generic.rs      - Fallback: file:line: message, file(line,col): message (MSVC)
```

### Key Types

```rust
pub struct ParsedError {
    pub file: String,
    pub line: u32,
    pub column: Option<u32>,
    pub message: String,
    pub severity: Severity,
    pub source_line: Option<String>,
}

pub enum Severity { Error, Warning, Note }

/// Parse command output into structured errors.
/// `hint` is the command text (e.g., "cargo build") to select parser.
pub fn parse(output: &str, hint: Option<&str>) -> Vec<ParsedError>
```

### Parser Selection Strategy

1. If `hint` provided, match command name to parser (cargo/rustc -> rust, python/pytest -> python, etc.)
2. If no hint or hint doesn't match, scan first 50 lines of output for format signatures
3. Apply matched parser(s) -- can try multiple and merge results
4. Deduplicate by (file, line, message) tuple

### Why a Separate Crate

- **Testability:** Unit tests with real compiler output fixtures, no DB or MCP setup needed
- **Reusability:** Could be used from main.rs (live grid output) or glass_mcp (history DB output)
- **Independence:** Zero dependency on any glass_* crate -- pure `&str -> Vec<ParsedError>`
- **Follows precedent:** Same pattern as glass_pipes (parsing logic) and glass_snapshot/command_parser.rs

## Patterns to Follow

### Pattern 1: Request/Reply via Oneshot Channel

**What:** Each McpRequest carries a `oneshot::Sender<McpResponse>`. The MCP tool handler awaits the oneshot receiver while the event loop processes the request synchronously.

**When:** All 7 live-data MCP tools (tab_create, tab_list, tab_run, tab_output, tab_close, command_status, command_cancel).

**Why:** Clean async boundary. No thread blocked while waiting. Oneshot auto-errors if event loop disconnects.

```rust
// In MCP tool handler (glass_mcp/tools.rs):
async fn glass_tab_list(&self, ...) -> Result<CallToolResult, McpError> {
    let (tx, rx) = oneshot::channel();
    self.mcp_sender.as_ref()
        .ok_or_else(|| internal_error("GUI not connected"))?
        .send(McpRequest::TabList { reply: tx })
        .await
        .map_err(|_| internal_error("Event loop disconnected"))?;

    match rx.await {
        Ok(McpResponse::Ok(value)) => Ok(CallToolResult::success(
            vec![Content::text(value.to_string())]
        )),
        Ok(McpResponse::Error(msg)) => Ok(CallToolResult::error(
            vec![Content::text(msg)]
        )),
        Err(_) => Err(internal_error("Event loop dropped request")),
    }
}
```

### Pattern 2: DB-Only Tools Stay DB-Only

**What:** Tools that only need SQLite data continue using the existing `spawn_blocking` pattern with no channel involvement.

**Which tools:** glass_output (with command_id), glass_cached_result, glass_changed_files, glass_context budget, glass_errors (with command_id)

**Why:** Simpler, faster, no IPC round-trip. These tools work even when the GUI process is not running.

### Pattern 3: Tab Index as User-Facing Identifier

**What:** Use 0-based tab index (position in tab bar) in all MCP tool parameters.

**Why:** Intuitive for agents ("run in tab 0"). SessionId is an internal u64 counter with no external meaning. Include session_id in responses for agents that need stable references across tab close/reorder operations.

**Caveat:** Tab indices shift when tabs are closed or reordered. MCP tools should validate the index and return clear errors ("tab index 5 out of range, 3 tabs open").

### Pattern 4: Grid Content Extraction

**What:** Reading terminal grid content for glass_tab_output.

**How:** The terminal grid is behind `Arc<FairMutex<Term<EventProxy>>>`. Lock it, read content, release immediately.

```rust
// In main.rs AppEvent::Mcp handler:
fn read_grid_lines(session: &Session, max_lines: usize) -> String {
    let term = session.term.lock();
    let grid = term.grid();
    // Read from (total_lines - max_lines) to total_lines
    // grid.display_iter() gives Cell references with content
    // Concatenate into string, strip ANSI
    drop(term); // explicit drop for clarity
    result
}
```

The FairMutex lock is held for microseconds (same as renderer). No risk of blocking PTY reader threads.

## Anti-Patterns to Avoid

### Anti-Pattern 1: Holding FairMutex Across Await Points

**What:** Locking `session.term` in an async context and holding across `.await`.

**Why bad:** FairMutex is not async-aware. PTY reader thread (std::thread) also locks this mutex to write terminal state. Holding across await could starve the PTY reader, causing visible terminal lag.

**Instead:** All grid reads happen synchronously in `user_event()`. Lock, copy data to owned String, drop lock, then send reply. This is the existing pattern used by the renderer.

### Anti-Pattern 2: Arc<Mutex<SessionMux>> for Direct MCP Access

**What:** Wrapping SessionMux in Arc<Mutex> so MCP tools can access sessions directly.

**Why bad:** SessionMux is owned by the App struct on the main thread. Adding shared ownership means EVERY session access (including the hot rendering path at 60fps) goes through Mutex contention. Frame rate would degrade.

**Instead:** Message-passing via channel. The event loop thread remains the sole owner of SessionMux.

### Anti-Pattern 3: Unbounded Output in MCP Responses

**What:** Returning entire terminal scrollback (10K+ lines) through MCP.

**Why bad:** Memory spike, serialization cost, MCP message size explosion. AI agents cannot usefully consume 10K lines anyway (context window limits).

**Instead:** Default to 50 lines. Cap at 100KB. Require explicit `lines` parameter. Support `pattern` filtering to return only relevant lines.

### Anti-Pattern 4: Blocking IPC in the Event Loop

**What:** Making synchronous IPC calls from user_event() to query external services.

**Why bad:** user_event() runs on the winit main thread. Any blocking call freezes the GUI.

**Instead:** All IPC is async (tokio tasks). Results arrive as AppEvents via the EventLoopProxy. The event loop only does synchronous, fast operations (read atomics, lock FairMutex briefly, send PtyMsg).

## Suggested Build Order

Build order follows dependency chains. Each phase produces testable, shippable value.

### Phase 1: MCP Command Channel + IPC Foundation

**What builds:**
- `glass_core/mcp_channel.rs` — McpRequest, McpResponse, type aliases
- `glass_core/event.rs` — Add `AppEvent::Mcp(McpRequest)` variant
- `glass_core/Cargo.toml` — Add `tokio = { features = ["sync"] }` and `serde_json`
- IPC listener in main.rs (localhost TCP, tokio task)
- IPC client helper in glass_mcp
- GUI writes port to `~/.glass/gui.port` on startup, removes on shutdown

**Test:** Send a dummy IPC request from a test client, receive response. Verify round-trip through event loop.

**Why first:** Every live-data feature depends on this. If the channel/IPC doesn't work, tab orchestration and live awareness are blocked.

**Risk:** MEDIUM. New tokio dependency in glass_core. IPC listener is new infrastructure. Mitigation: the listener is simple (JSON lines over TCP, 7 methods).

### Phase 2: Multi-Tab Orchestration (5 tools)

**What builds:**
- `glass_tab_list` — Read-only, validates IPC end-to-end
- `glass_tab_output` — Read grid content, validates FairMutex pattern
- `glass_tab_create` — Reuses existing `create_session()` flow from main.rs
- `glass_tab_run` — Write to PTY sender
- `glass_tab_close` — Session teardown

**Dependencies:** Phase 1 (channel/IPC must work)

**Build order within phase matters:** list -> output -> create -> run -> close. Each validates a deeper integration point.

**Key integration point for glass_tab_create:** The `create_session()` function in main.rs takes 10 parameters (proxy, window_id, session_id, config, working_directory, cell_w, cell_h, window_width, window_height, tab_bar_lines). The MCP handler needs access to all of these. Extract window state (cell dims, window size, config ref) into a struct that the AppEvent::Mcp handler can reference.

### Phase 3: Token-Saving Tools (4 tools, DB-only)

**What builds:**
- `glass_output` — Filtered read from commands table output column
- `glass_cached_result` — LIKE/FTS query + staleness check via snapshot timestamps
- `glass_changed_files` — Snapshot query + `similar` crate for unified diff generation
- `glass_context` budget/focus — Enhance existing tool with budget and focus parameters

**Dependencies:** None (DB-only, can be built in parallel with Phase 1 or 2)

**New dependency:** `similar` crate for diff generation in glass_changed_files

**Why Phase 3 despite no dependencies:** Token-saving tools are high value and low risk. Building them after the channel is validated means they can use `tab_id` parameter for live output (via the channel) in addition to `command_id` parameter (via DB). But they work without the channel using command_id only.

### Phase 4: Structured Error Extraction (1 crate + 1 tool)

**What builds:**
- `glass_errors` crate with ParsedError types
- Rust parser (most relevant — this is a Rust project)
- Generic fallback parser (file:line:col: message)
- Python, Node, Go, GCC parsers
- `glass_errors` MCP tool wiring

**Dependencies:** None (pure library crate + DB-only MCP tool)

**Can be built in parallel with Phases 1-3.** The crate has zero dependency on any glass_* crate.

### Phase 5: Live Command Awareness (2 tools)

**What builds:**
- `glass_command_status` — Read BlockManager state via IPC channel
- `glass_command_cancel` — Send `\x03` (Ctrl+C) to PTY via channel

**Dependencies:** Phase 1 (needs IPC channel)

**Why last:** Smallest scope (2 tools), simplest implementation. CommandStatus reads `block_manager.current_block().state == BlockState::Executing`. CommandCancel writes one byte to pty_sender.

## Integration Risk Assessment

| Integration Point | Risk | Mitigation |
|-------------------|------|------------|
| tokio dep in glass_core | LOW | Only `sync` feature, no runtime. glass_core already uses std::sync |
| AppEvent::Mcp variant | LOW | Follows exact pattern of 8 existing variants |
| IPC listener in GUI | MEDIUM | New infrastructure; use localhost TCP for simplicity |
| create_session() from MCP | MEDIUM | Needs window state (cell dims, config); extract into helper struct |
| FairMutex grid reads | LOW | Already done by renderer; identical lock pattern |
| PTY input from MCP | LOW | Existing `pty_sender.send(PtyMsg::Input(...))` API |
| `similar` crate for diffs | LOW | Well-maintained, 0 transitive deps, widely used |
| glass_errors regex parsers | LOW | Pure library, no integration risk, only testing effort |
| Ctrl+C via PTY | LOW | Write `\x03` byte, same as keyboard Ctrl+C handler |

## Scalability Considerations

| Concern | At 5 tabs | At 20 tabs | At 50 tabs |
|---------|-----------|------------|------------|
| IPC throughput | Trivial | Trivial | Trivial (sequential MCP calls) |
| Grid read latency | <1ms | <1ms | <1ms (per-tab, not aggregate) |
| MCP response size | <10KB typical | Same | Same |
| Memory per tab | ~15MB (PTY + grid) | ~300MB | ~750MB (PTY/grid limit, not MCP) |
| IPC connections | 1 (one MCP server) | 1 | 1 |

The channel/IPC is not the bottleneck. MCP tool calls are sequential and infrequent. The real constraint is terminal memory per session, which is orthogonal to this architecture.

## Sources

- Glass codebase: `crates/glass_mcp/src/tools.rs` — GlassServer structure, spawn_blocking pattern, tool handler pattern (HIGH confidence)
- Glass codebase: `crates/glass_core/src/event.rs` — AppEvent variants, EventLoopProxy pattern (HIGH confidence)
- Glass codebase: `crates/glass_mux/src/session_mux.rs` — SessionMux API, Tab/Session ownership (HIGH confidence)
- Glass codebase: `crates/glass_mux/src/session.rs` — Session struct fields, FairMutex<Term> (HIGH confidence)
- Glass codebase: `src/main.rs` — create_session() parameters, user_event() dispatch, PTY spawn flow (HIGH confidence)
- Glass codebase: `crates/glass_terminal/src/block_manager.rs` — BlockState::Executing, command lifecycle (HIGH confidence)
- Glass codebase: `AGENT_MCP_FEATURES.md` — feature design document (HIGH confidence)
- rmcp SDK: transport-io feature supports any AsyncRead+AsyncWrite (HIGH confidence, from Cargo.toml)
- tokio::sync documentation: mpsc bounded channel, oneshot channel (HIGH confidence)
- similar crate: unified diff generation, widely used Rust diffing library (HIGH confidence)

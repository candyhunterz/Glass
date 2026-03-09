# Architecture Patterns: Multi-Agent Coordination Integration

**Domain:** Multi-agent coordination layer for GPU-accelerated terminal emulator
**Researched:** 2026-03-09
**Overall confidence:** HIGH

## Recommended Architecture

The coordination feature integrates as a new crate (`glass_coordination`) with modifications to two existing crates (`glass_mcp`, `glass_renderer`) and the root binary (`src/main.rs`). The design follows established patterns already proven in the codebase.

```
                   src/main.rs (MODIFIED)
                   - Reads agents.db periodically for GUI indicators
                   - Passes coordination state to renderer
                        |
         +--------------+--------------+
         |              |              |
   glass_mux       glass_renderer    glass_mcp (MODIFIED)
   (unchanged)     (MODIFIED)        - Adds glass_coordination dep
                   - Status bar:     - 11 new MCP tool handlers
                     agent count     - Opens CoordinationDb on startup
                   - Tab bar:        - Wraps sync calls in spawn_blocking
                     lock indicators
                        |              |
                        +------+-------+
                               |
                    glass_coordination (NEW)
                    - CoordinationDb struct
                    - SQLite schema (agents.db)
                    - Agent registry, file locks, messaging
                    - Path canonicalization
                    - Stale agent pruning (heartbeat + PID)
                               |
                         rusqlite (existing workspace dep)
                         uuid (NEW workspace dep)
```

### Component Classification

| Component | Status | Role |
|-----------|--------|------|
| `crates/glass_coordination/` | **NEW** | Pure library: SQLite coordination DB, all agent/lock/message operations |
| `crates/glass_mcp/src/tools.rs` | **MODIFIED** | Add 11 tool handlers, wire CoordinationDb into GlassServer |
| `crates/glass_mcp/src/lib.rs` | **MODIFIED** | Open CoordinationDb path during server startup |
| `crates/glass_mcp/Cargo.toml` | **MODIFIED** | Add `glass_coordination` dependency |
| `crates/glass_renderer/src/status_bar.rs` | **MODIFIED** | Add agent count display to status bar |
| `crates/glass_renderer/src/tab_bar.rs` | **MODIFIED** | Add lock indicator (colored dot) per tab |
| `crates/glass_renderer/src/frame.rs` | **MODIFIED** | Pass coordination display state through to status bar |
| `src/main.rs` | **MODIFIED** | Periodic coordination state polling, pass to renderer |
| `Cargo.toml` (workspace root) | **MODIFIED** | Add `uuid` to workspace deps, add `glass_coordination` to root deps |
| `CLAUDE.md` | **MODIFIED** | Add coordination instructions for AI agents |

### Components NOT Modified

| Component | Why Unchanged |
|-----------|---------------|
| `glass_terminal` | No terminal emulation changes needed; coordination is above PTY layer |
| `glass_core` | No new AppEvent variants needed (coordination state is polled, not event-driven) |
| `glass_history` | History DB is separate from agents.db; no schema changes |
| `glass_snapshot` | Snapshot operations are independent of coordination |
| `glass_pipes` | Pipe capture is orthogonal to agent coordination |
| `glass_mux` | Session multiplexer does not need coordination awareness; tab metadata flows through main.rs |

## New Crate: `glass_coordination`

### Design Principles

Follow the exact patterns established by `glass_history` and `glass_snapshot`:

1. **Pure synchronous library** -- no async runtime, no Tokio dependency. All SQLite operations are blocking. The MCP layer wraps them in `tokio::task::spawn_blocking`.
2. **Own its database** -- `agents.db` lives in `~/.glass/` alongside `history.db` and `snapshots.db`. Separate DB for independent lifecycle and no migration risk to existing data.
3. **WAL mode with busy_timeout** -- identical PRAGMA configuration to `HistoryDb` and `SnapshotDb`: `journal_mode = WAL`, `synchronous = NORMAL`, `busy_timeout = 5000`, `foreign_keys = ON`.
4. **PRAGMA user_version for migrations** -- same migration pattern used by glass_history (v0 -> v1 -> v2) and glass_snapshot (v0).

### Internal Structure

```
crates/glass_coordination/
  Cargo.toml
  src/
    lib.rs          -- CoordinationDb struct, open(), resolve_agents_db_path()
    schema.rs       -- CREATE TABLE statements, migrations
    agents.rs       -- register, deregister, heartbeat, set_status, list_agents, get_agent
    locks.rs        -- lock_files (atomic), unlock_file, unlock_all, list_locks
    messages.rs     -- broadcast, send_message, read_messages
    types.rs        -- AgentInfo, FileLock, Message, LockResult, LockConflict structs
    prune.rs        -- prune_stale(), PID liveness check (cross-platform)
```

### Cargo.toml

```toml
[package]
name = "glass_coordination"
version = "0.1.0"
edition = "2021"

[dependencies]
rusqlite = { workspace = true }
uuid = { version = "1", features = ["v4"] }
anyhow = { workspace = true }
tracing = { workspace = true }
chrono = { workspace = true }

[dev-dependencies]
tempfile = "3"
```

### Key API Design Decisions

**CoordinationDb holds a `Connection`, not `Arc<Mutex<Connection>>`**. This matches `HistoryDb` and `SnapshotDb`. Thread safety is handled by the caller (MCP layer uses `spawn_blocking` with the DB path cloned into the closure, opening per-call).

**`lock_files` is atomic (all-or-nothing)**. Uses a single SQLite transaction with `BEGIN IMMEDIATE`. If any path is already locked by another agent, the entire request fails with conflict details, and no locks are acquired. This eliminates TOCTOU races and prevents partial-lock deadlocks.

**Path canonicalization happens inside `lock_files`/`unlock_file`**, not at the caller. This ensures consistency regardless of how the path arrives. Uses `std::fs::canonicalize()` which resolves symlinks and produces absolute paths. On Windows, this produces UNC paths (`\\?\C:\...`) -- stored as-is since all paths go through the same canonicalization.

**DB location: `~/.glass/agents.db`** (always global, never per-project). Unlike history and snapshots which can be project-local (`.glass/` in project root), coordination must be global so agents in different CWDs within the same project can discover each other. The `project` field in the agents table handles scoping by project root path.

### Database Schema: `~/.glass/agents.db`

```sql
-- Schema version 0

CREATE TABLE agents (
    id              TEXT PRIMARY KEY,        -- UUID v4, assigned on register
    name            TEXT NOT NULL,            -- Human-readable label ("Claude A")
    agent_type      TEXT NOT NULL,            -- "claude-code", "cursor", "copilot", "human"
    project         TEXT NOT NULL,            -- Canonical project root (scopes visibility)
    cwd             TEXT NOT NULL,            -- Working directory
    pid             INTEGER,                 -- OS process ID (liveness fallback)
    status          TEXT NOT NULL DEFAULT 'active',  -- active | busy | idle
    task            TEXT,                     -- Current task description
    registered_at   INTEGER NOT NULL,
    last_heartbeat  INTEGER NOT NULL
);

CREATE TABLE file_locks (
    path       TEXT PRIMARY KEY,             -- Canonical absolute path
    agent_id   TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    reason     TEXT,
    locked_at  INTEGER NOT NULL
);
CREATE INDEX idx_locks_agent ON file_locks(agent_id);

CREATE TABLE messages (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    from_agent  TEXT REFERENCES agents(id) ON DELETE SET NULL,
    to_agent    TEXT,                         -- NULL = broadcast
    msg_type    TEXT NOT NULL DEFAULT 'info', -- info | conflict_warning | task_complete | request_unlock
    content     TEXT NOT NULL,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    read        INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_messages_unread ON messages(to_agent, read);
```

### Concurrency Model: Why SQLite WAL Works Here

SQLite WAL mode supports concurrent readers with a single writer. Multiple `glass mcp serve` processes safely share `agents.db`:

- **Reads** (list_agents, list_locks, read_messages): Fully concurrent, never blocked by writers.
- **Writes** (register, lock_files, heartbeat, send_message): Serialized by SQLite's write lock. With `busy_timeout = 5000ms`, a writer waits up to 5 seconds. Coordination writes are small and fast (single-row INSERTs/UPDATEs), so contention is negligible even with 10+ agents.
- **Atomic transactions** (`lock_files`): Uses `BEGIN IMMEDIATE` to acquire the write lock at transaction start, preventing the upgrade deadlock where two readers try to simultaneously upgrade to writers.

This is the identical pattern used by `HistoryDb` and `SnapshotDb`. WAL mode, synchronous=NORMAL, busy_timeout=5000 are already proven in the codebase across hundreds of tests.

### PID Liveness Check (Cross-Platform)

For stale agent detection, `prune_stale()` needs to check if a PID is still running. Two approaches, in order of preference:

**Approach A (Recommended): Manual platform-specific checks.** Zero new dependencies. Small amount of platform-specific code behind `#[cfg]`:

```rust
fn is_pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // kill(pid, 0) checks process existence without sending a signal
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        use windows_sys::Win32::Foundation::CloseHandle;
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle == 0 { return false; }
        unsafe { CloseHandle(handle); }
        true
    }
}
```

This avoids adding libc/windows-sys as direct dependencies to glass_coordination. Instead, use `std::process::Command` to call platform tools:
- Unix: `kill -0 <pid>` (returns success if process exists)
- Windows: `tasklist /FI "PID eq <pid>"` or use `windows-sys` (already a workspace dependency)

**Approach B: Use `process_alive` crate.** Adds one tiny dependency. Handles all platform nuances. If the manual approach proves fragile, fall back to this.

## Modified Component: `glass_mcp`

### GlassServer Changes

The `GlassServer` struct gains a new field for the coordination DB path:

```rust
#[derive(Clone)]
pub struct GlassServer {
    tool_router: ToolRouter<Self>,
    db_path: PathBuf,        // existing: history DB path
    glass_dir: PathBuf,      // existing: snapshot glass_dir
    agents_db_path: PathBuf, // NEW: coordination DB path (~/.glass/agents.db)
}
```

**Why store the path, not an open `CoordinationDb`?** Same pattern as `glass_dir` for snapshots. `CoordinationDb` wraps `rusqlite::Connection` which is `!Send`. `GlassServer` must be `Clone + Send` for rmcp's `ServerHandler` trait. Each tool handler opens the DB inside `spawn_blocking` on a dedicated thread. This is the exact pattern used by the existing `glass_undo` and `glass_file_diff` handlers which open `SnapshotStore` per-request.

### 11 New Tool Handlers

Each follows the identical pattern to existing handlers. Example:

```rust
#[tool(description = "Register this agent for coordination. Returns agent_id and count of active agents.")]
async fn glass_agent_register(
    &self,
    Parameters(params): Parameters<RegisterParams>,
) -> Result<CallToolResult, McpError> {
    let agents_db_path = self.agents_db_path.clone();
    let result = tokio::task::spawn_blocking(move || {
        let db = CoordinationDb::open(&agents_db_path).map_err(internal_err)?;
        db.register(&params.name, &params.agent_type, &params.project, &params.cwd, None)
            .map_err(internal_err)
    })
    .await
    .map_err(internal_err)??;

    let content = Content::json(&serde_json::json!({
        "agent_id": result.agent_id,
        "agents_active": result.active_count,
    }))?;
    Ok(CallToolResult::success(vec![content]))
}
```

### MCP Server Startup Changes

In `lib.rs`, `run_mcp_server()` adds agents DB path resolution:

```rust
pub async fn run_mcp_server() -> anyhow::Result<()> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let db_path = glass_history::resolve_db_path(&cwd);
    let glass_dir = glass_snapshot::resolve_glass_dir(&cwd);
    let agents_db_path = glass_coordination::resolve_agents_db_path(); // NEW

    let server = tools::GlassServer::new(db_path, glass_dir, agents_db_path);
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
```

`resolve_agents_db_path()` always returns `~/.glass/agents.db` (global, no per-project walk).

### Heartbeat Strategy

The MCP server process is long-lived (runs for the entire Claude Code session). Heartbeats must be sent every 60 seconds to prevent stale pruning (5-minute timeout).

**Use explicit heartbeat tool calls.** The agent (Claude Code) is instructed via CLAUDE.md to call `glass_agent_heartbeat` periodically. This keeps the MCP server stateless -- it does not self-register or maintain its own agent identity. The MCP server is a tool provider; the agent using the tools is the registered entity.

This is the approach specified in the design document. Each tool call is independent and stateless from the MCP server's perspective.

## Modified Component: `glass_renderer`

### Status Bar: Agent Count

Extend `StatusBarRenderer::build_status_text()` to accept optional coordination state:

```rust
pub struct CoordinationDisplay {
    pub agent_count: usize,
    pub lock_count: usize,
}
```

The `build_status_text` method signature adds one parameter:

```rust
pub fn build_status_text(
    &self,
    cwd: &str,
    git_info: Option<&GitInfo>,
    update_text: Option<&str>,
    coordination: Option<&CoordinationDisplay>, // NEW
    viewport_height: f32,
) -> StatusLabel { ... }
```

When `coordination` is `Some` and `agent_count > 0`, append to `right_text` after git info:

```
main +3 | 2 agents, 5 locks
```

If no git info, show just coordination:

```
2 agents, 5 locks
```

**Why extend right_text rather than using center_text?** Center text is reserved for the update notification. Right text already shows git info and naturally extends with coordination info using a `|` separator.

### Tab Bar: Lock Indicators (Phase 4, Deferred)

Extend `TabDisplayInfo` with optional lock state:

```rust
pub struct TabDisplayInfo {
    pub title: String,
    pub is_active: bool,
    pub has_agent_locks: bool, // NEW: true if an agent in this tab holds file locks
}
```

**Challenge: Mapping MCP agents to Glass tabs.** MCP servers are separate processes spawned by Claude Code, which runs inside a shell that runs inside a Glass tab's PTY. The Glass GUI knows each tab's shell PID (from PTY spawn), but the MCP agent knows its own PID. Correlating requires process tree walking (MCP PID -> parent shell PID -> Glass PTY PID).

**Recommendation:** Defer per-tab lock indicators to Phase 4. For Phase 3, show aggregate agent/lock counts in the status bar only. This avoids process tree walking complexity while still providing coordination visibility.

### Frame Renderer Changes

Both `draw_frame()` and `draw_multi_pane_frame()` need the coordination display data passed through. The signature change cascades from `main.rs` through to `build_status_text()`. This is the same pattern used when `update_text` was added -- a new optional parameter threaded through the rendering pipeline.

## Modified Component: `src/main.rs`

### Coordination State Polling

The main event loop needs periodic access to coordination state for GUI display.

**Use a background polling thread with `Arc<AtomicUsize>` pairs.** This avoids adding new `AppEvent` variants (keeping `glass_core` unchanged) and matches the lightweight pattern used for `update_info`.

```rust
struct GlassApp {
    // ... existing fields ...
    coordination_agent_count: Arc<AtomicUsize>, // NEW
    coordination_lock_count: Arc<AtomicUsize>,  // NEW
}
```

On startup, spawn a background `std::thread` (not Tokio -- same as git status queries):

```rust
let agent_count = Arc::clone(&self.coordination_agent_count);
let lock_count = Arc::clone(&self.coordination_lock_count);
let agents_db_path = glass_coordination::resolve_agents_db_path();

std::thread::spawn(move || {
    loop {
        if let Ok(db) = CoordinationDb::open(&agents_db_path) {
            if let Ok(agents) = db.list_agents() {
                agent_count.store(agents.len(), Ordering::Relaxed);
            }
            if let Ok(locks) = db.list_locks(None) {
                lock_count.store(locks.len(), Ordering::Relaxed);
            }
        }
        std::thread::sleep(Duration::from_secs(5));
    }
});
```

**Why not `AppEvent`-driven?** Adding a `CoordinationUpdate` variant to `AppEvent` in `glass_core` would require modifying `glass_core`, which is depended on by 5 other crates. The atomic approach is simpler: the render loop reads two atomics (essentially free, no event handling code) and passes the values to the renderer. This is appropriate because coordination state is non-urgent display-only data.

**Why not `notify` file watcher on agents.db?** SQLite WAL creates `-wal` and `-shm` companion files. Every write to any table triggers filesystem events on multiple files, causing excessive redundant GUI updates. Simple 5-second polling is sufficient for a status bar display.

### Render Loop Integration

In the `RedrawRequested` handler, read atomics and construct `CoordinationDisplay`:

```rust
let coordination = {
    let ac = self.coordination_agent_count.load(Ordering::Relaxed);
    let lc = self.coordination_lock_count.load(Ordering::Relaxed);
    if ac > 0 {
        Some(CoordinationDisplay { agent_count: ac, lock_count: lc })
    } else {
        None
    }
};
```

Pass this to `build_status_text()` alongside existing `update_text`.

## Data Flow Diagrams

### Agent Registration Flow (MCP Process)

```
Claude Code                glass mcp serve              agents.db
    |                           |                           |
    |-- glass_agent_register -->|                           |
    |                           |-- spawn_blocking -------->|
    |                           |   CoordinationDb::open()  |
    |                           |   db.register(...)        |
    |                           |   INSERT INTO agents      |
    |                           |<-- agent_id (UUID) -------|
    |<-- { agent_id, count } ---|                           |
```

### File Lock Flow (Atomic All-or-Nothing)

```
Claude Code                glass mcp serve              agents.db
    |                           |                           |
    |-- glass_agent_lock ------>|                           |
    |   paths: [a.rs, b.rs]    |-- spawn_blocking -------->|
    |                           |   BEGIN IMMEDIATE         |
    |                           |   canonicalize(a.rs)      |
    |                           |   canonicalize(b.rs)      |
    |                           |   SELECT from file_locks  |
    |                           |   -- no conflicts? -->    |
    |                           |   INSERT INTO file_locks  |
    |                           |   COMMIT                  |
    |<-- { locked: [...] } -----|                           |
    |                           |                           |
    |   -- OR if conflict: --   |                           |
    |                           |   ROLLBACK                |
    |<-- { conflicts: [...] } --|                           |
```

### GUI Coordination Display Flow (Glass Terminal Process)

```
Glass main.rs          Background Thread         agents.db
    |                       |                       |
    | (startup)             |                       |
    |-- spawn thread ------>|                       |
    |                       |-- poll every 5s ----->|
    |                       |   SELECT COUNT(*)     |
    |                       |<-- agent_count -------|
    |                       |-- store AtomicUsize   |
    |                       |                       |
    | (render frame)        |                       |
    |-- read atomics        |                       |
    |-- pass to renderer    |                       |
    |-- status bar shows    |                       |
    |   "2 agents, 5 locks" |                       |
```

### Cross-Process Coordination (Two Agents)

```
Glass Tab 1              agents.db              Glass Tab 2
(Claude A)                                      (Claude B)
    |                       |                       |
    |-- register ---------->|                       |
    |<-- agent_id: AAA -----|                       |
    |                       |<-- register ----------|
    |                       |--- agent_id: BBB ---->|
    |                       |                       |
    |-- lock src/main.rs -->|                       |
    |<-- locked ------------|                       |
    |                       |                       |
    |                       |<-- lock src/main.rs --|
    |                       |--- conflict: AAA ---->|
    |                       |                       |
    |                       |<-- send_message ------|
    |                       |   "need main.rs"      |
    |                       |                       |
    |-- read_messages ----->|                       |
    |<-- "need main.rs" ----|                       |
    |                       |                       |
    |-- unlock src/main.rs->|                       |
    |                       |<-- lock src/main.rs --|
    |                       |--- locked ----------->|
```

## Path Canonicalization Strategy

### The Problem

File paths arrive at the coordination layer from different agents running in different working directories. The same file could be referenced as:
- `src/main.rs` (relative)
- `./src/main.rs` (relative with dot)
- `C:\Users\nkngu\apps\Glass\src\main.rs` (Windows absolute)
- `/c/Users/nkngu/apps/Glass/src/main.rs` (Git Bash style)
- `\\?\C:\Users\nkngu\apps\Glass\src\main.rs` (Windows UNC)

### The Solution

`std::fs::canonicalize()` inside `lock_files()` and `unlock_file()` before any DB operation. This produces a canonical absolute path that resolves symlinks. All agents go through the same canonicalization, so identical files produce identical DB keys.

### Interaction with Existing Canonicalization

The existing `glass_snapshot` crate already uses `canonicalize()` in two places:
- `IgnoreRules::new()` canonicalizes CWD for gitignore matching
- `FsWatcher::new()` canonicalizes the watch directory

The coordination crate's canonicalization is **completely independent** -- it operates on a different database (`agents.db` vs `snapshots.db`) and different path sets (advisory locks vs watched files). No interaction or conflicts.

### Windows UNC Path Consistency

On Windows, `canonicalize()` returns extended-length paths like `\\?\C:\Users\...`. Since ALL paths go through the same `canonicalize()` call, they consistently use this format. Two agents locking the same file produce identical UNC paths. No special handling needed.

### Non-Existent File Paths

`std::fs::canonicalize()` requires the file to exist on disk. For locking files that do not exist yet (agent is about to create them), use a fallback strategy:

1. Canonicalize the parent directory (which must exist)
2. Append the filename

This matches the pattern in `glass_snapshot::IgnoreRules::canonicalize_path()`. Duplicate the logic in glass_coordination rather than creating a shared utility, to avoid coupling between crates that have no other dependency relationship.

## Patterns to Follow

### Pattern 1: Per-Request DB Opening (from glass_mcp/tools.rs)

**What:** Open SQLite DB inside `spawn_blocking` for each MCP tool call, not at server startup.
**When:** All coordination tool handlers.
**Why:** `rusqlite::Connection` is `!Send`. Storing it in `GlassServer` (which must be `Clone + Send` for rmcp) would require `Arc<Mutex<>>`. Per-request opening is cheap (SQLite open is <1ms) and matches the existing pattern used by all 5 existing tools.
**Example:** See existing `glass_undo` handler in `tools.rs` lines 251-257.

### Pattern 2: PRAGMA Configuration (from glass_history/db.rs)

**What:** Set WAL mode, synchronous=NORMAL, busy_timeout=5000, foreign_keys=ON on every connection open.
**When:** `CoordinationDb::open()`.
**Why:** WAL mode persists in the database file but PRAGMAs like `busy_timeout` and `foreign_keys` are per-connection settings. Must be set every time a connection is opened. Exact same 4-line PRAGMA block as `HistoryDb::open()` and `SnapshotDb::open()`.

### Pattern 3: Schema Migration via user_version (from glass_history/db.rs, glass_snapshot/db.rs)

**What:** Use `PRAGMA user_version` to track schema version. Apply migrations in a match statement.
**When:** `CoordinationDb::open()` after initial schema creation.
**Why:** Simple, built-in, proven in two existing crates. History DB has migrated from v0 -> v1 -> v2 successfully. No migration framework dependency needed.

### Pattern 4: BEGIN IMMEDIATE for Write Transactions

**What:** Use `BEGIN IMMEDIATE` instead of plain `BEGIN` for transactions that will write.
**When:** `lock_files()` -- the only multi-statement write operation.
**Why:** Plain `BEGIN` starts a deferred transaction (read-only initially). If two connections both start deferred transactions and then try to upgrade to write, one gets `SQLITE_BUSY`. `BEGIN IMMEDIATE` acquires the write lock upfront, so the second connection waits (up to busy_timeout) rather than failing.

### Pattern 5: Atomic State via Arc + AtomicUsize (from main.rs update_info pattern)

**What:** Background thread stores results in `Arc<AtomicUsize>`. Render loop reads with `Ordering::Relaxed`.
**When:** GUI coordination state display.
**Why:** Avoids adding `AppEvent` variants (keeping glass_core unchanged), avoids event loop overhead for non-urgent display data, and is essentially zero-cost in the render path.

## Anti-Patterns to Avoid

### Anti-Pattern 1: Sharing Connection Across Threads

**What:** Wrapping `rusqlite::Connection` in `Arc<Mutex<>>` and sharing between the Tokio runtime and tool handlers.
**Why bad:** Creates unnecessary contention. The Mutex must be held for the entire DB operation duration. Under load, tool handlers queue up waiting for the lock.
**Instead:** Open a fresh connection per `spawn_blocking` call. SQLite connections are cheap to create (<1ms). WAL mode handles the real concurrency at the DB level.

### Anti-Pattern 2: Polling agents.db from the Render Loop

**What:** Opening and querying `agents.db` synchronously during frame rendering in the `RedrawRequested` handler.
**Why bad:** SQLite I/O in the render loop would block frame submission. Even a 1ms query at 60fps (16.6ms budget) causes visible jank.
**Instead:** Background thread polls every 5 seconds, stores results in atomics. Render loop reads atomics (effectively zero-cost).

### Anti-Pattern 3: Event-Driven Coordination State via notify

**What:** Using `notify` file watcher on `agents.db` to trigger GUI updates when coordination state changes.
**Why bad:** SQLite WAL creates `-wal` and `-shm` companion files. Every write to any table triggers filesystem events on multiple files, causing excessive redundant redraws. Also, `notify` events don't tell you WHAT changed, so you'd still have to query the DB.
**Instead:** Simple periodic polling (5-second interval) is appropriate for non-urgent status bar display.

### Anti-Pattern 4: Making glass_coordination Async

**What:** Adding Tokio as a dependency to glass_coordination for async DB operations.
**Why bad:** Rusqlite is synchronous. Wrapping sync calls in async adds complexity without benefit. The MCP layer already handles the sync-to-async bridge via `spawn_blocking`. Adding Tokio would also break the pattern established by glass_history and glass_snapshot (both pure sync).
**Instead:** Keep glass_coordination purely synchronous. Let the consumer (glass_mcp) handle threading.

### Anti-Pattern 5: Coupling Agent ID to SessionId

**What:** Using Glass's internal `SessionId` (u64 counter) as the agent identifier.
**Why bad:** MCP servers are separate processes spawned by the AI tool (e.g., Claude Code). They have no access to Glass's internal session IDs. There is no IPC channel between the Glass GUI process and MCP server processes to communicate SessionIds.
**Instead:** Use UUID v4 for agent IDs. Self-generated, globally unique, no coordination with Glass GUI needed.

### Anti-Pattern 6: Per-Project agents.db

**What:** Using the same `resolve_glass_dir()` walk-up-and-find pattern used by history.db and snapshots.db.
**Why bad:** Two agents working on the same project but from different subdirectories might resolve to different `.glass/` directories. One agent in `/project/` finds `/project/.glass/agents.db`; another in `/project/subdir/` might find a different `.glass/` or fall back to `~/.glass/`. They would not see each other.
**Instead:** Always use `~/.glass/agents.db` (global). The `project` column in the agents table handles scoping.

## Build Order (Dependency-Respecting)

The components have a strict dependency chain. Build in this order:

### Phase 1: `glass_coordination` crate (foundation, no dependents yet)

**What to build:**
- `Cargo.toml` with rusqlite, uuid, anyhow, chrono, tracing
- `src/lib.rs` -- CoordinationDb struct, open(), resolve_agents_db_path()
- `src/schema.rs` -- CREATE TABLE statements, user_version = 0
- `src/types.rs` -- AgentInfo, FileLock, Message, LockResult, LockConflict
- `src/agents.rs` -- register, deregister, heartbeat, set_status, list_agents, get_agent
- `src/locks.rs` -- lock_files (atomic), unlock_file, unlock_all, list_locks, path canonicalization
- `src/messages.rs` -- broadcast, send_message, read_messages
- `src/prune.rs` -- prune_stale with PID liveness check

**Tests:** Unit tests for every public method. Use `tempfile` for DB paths. Test concurrent access by opening two connections to the same WAL-mode DB. Test atomic lock_files failure (partial conflicts return no locks). Test stale pruning with expired timestamps. Test path canonicalization (relative paths resolve correctly).

**Dependencies:** Only workspace deps (rusqlite) + new uuid. Zero dependency on any glass_* crate.

**Must complete before:** Phase 2 (glass_mcp depends on it).

### Phase 2: `glass_mcp` integration (depends on Phase 1)

**What to build:**
- Add `glass_coordination` to glass_mcp/Cargo.toml
- Add `agents_db_path: PathBuf` field to GlassServer
- Update `GlassServer::new()` signature (3 args -> 3 args, backward compatible if we add to end)
- Add 11 new parameter structs (RegisterParams, LockParams, StatusParams, etc.) with schemars derives
- Add 11 new `#[tool]` handlers following existing spawn_blocking pattern
- Update `run_mcp_server()` to resolve and pass agents_db_path
- Update `ServerHandler::get_info()` instructions text to mention coordination tools

**Tests:** Param deserialization tests (existing pattern). Integration test: open real tempdir DB, register agent, lock files, send message, verify round-trip.

**Must complete before:** Phase 3.

### Phase 3: Integration testing + CLAUDE.md (depends on Phase 2)

**What to build:**
- CLAUDE.md coordination instructions section (7-bullet protocol)
- Integration test: two GlassServer instances sharing same agents.db via tempdir
- Test: register from server A, see agent from server B's list
- Test: lock from A, conflict from B, unlock from A, lock from B succeeds

**Must complete before:** Phase 4 (GUI needs real data to display).

### Phase 4: GUI integration (depends on Phase 1; can start in parallel with Phase 3)

**What to build:**
- Add `glass_coordination` to root Cargo.toml dependencies
- Add `Arc<AtomicUsize>` fields to `GlassApp` struct (or equivalent Processor struct in main.rs)
- Background polling thread (5-second interval, reads agents.db)
- Add `CoordinationDisplay` struct to glass_renderer
- Extend `StatusBarRenderer::build_status_text()` with `coordination` parameter
- Update `draw_frame()` and `draw_multi_pane_frame()` signatures in frame.rs
- Update call sites in main.rs `RedrawRequested` handler
- Future: TabDisplayInfo `has_agent_locks` field (defer per-tab mapping)

**Note:** Phase 4 modifies `src/main.rs` and `glass_renderer` -- both in the hot rendering path. Changes must be minimal (reading two atomics, passing one extra Option parameter) to avoid performance regression.

## Scalability Considerations

| Concern | 2 agents | 10 agents | 50+ agents |
|---------|----------|-----------|------------|
| DB write contention | Negligible | Minimal (~5ms worst case) | busy_timeout may trigger; consider connection pooling |
| Lock table size | <20 entries | <100 entries | May need index on project column for filtered queries |
| Message broadcast fan-out | Trivial | Moderate (N-1 message rows per broadcast) | Need message TTL/auto-prune to prevent table growth |
| GUI poll cost | <1ms | <1ms | <5ms (still fine at 5s poll interval) |
| Stale pruning | Instant | Instant | Consider batch DELETE with LIMIT for large agent tables |

The design is comfortable for the expected use case of 2-5 concurrent agents. This is the realistic scenario for a developer using Glass with Claude Code instances.

## Sources

- Glass codebase: `crates/glass_history/src/db.rs` lines 48-61 (WAL + PRAGMA pattern, HIGH confidence)
- Glass codebase: `crates/glass_mcp/src/tools.rs` lines 145-410 (spawn_blocking tool handler pattern, HIGH confidence)
- Glass codebase: `crates/glass_snapshot/src/ignore_rules.rs` lines 23-82 (canonicalize pattern, HIGH confidence)
- Glass codebase: `crates/glass_renderer/src/status_bar.rs` (status bar rendering pattern, HIGH confidence)
- Glass codebase: `crates/glass_renderer/src/tab_bar.rs` (tab bar rendering pattern, HIGH confidence)
- Glass codebase: `src/main.rs` lines 661-840 (render loop, draw_frame call sites, HIGH confidence)
- AGENT_COORDINATION_DESIGN.md (design document for this milestone, HIGH confidence)
- [SQLite WAL mode](https://www.sqlite.org/wal.html) -- concurrent reader/single writer semantics (HIGH confidence)
- [uuid crate v1.22.0](https://crates.io/crates/uuid) -- features=["v4"] for random UUIDs (HIGH confidence)
- [process_alive crate](https://lib.rs/crates/process_alive) -- cross-platform PID liveness checking (MEDIUM confidence, alternative approach)

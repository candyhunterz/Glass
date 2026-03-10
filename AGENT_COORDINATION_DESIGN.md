# Glass Multi-Agent Coordination — Design Document

## Problem

Multiple AI agents (Claude Code instances, Cursor, etc.) running in separate Glass tabs are blind to each other. They can edit the same files, run conflicting commands, and waste work — with no way to detect or prevent collisions.

## Solution

Glass becomes an **agent orchestration layer**. It provides a shared coordination database that all agents access through new MCP tools. Agents register themselves, claim files, exchange messages, and query each other's activity — all through Glass.

## Architecture

```
┌─────────────────────────────────────────────────┐
│                   Glass GUI                      │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐      │
│  │  Tab 1   │  │  Tab 2   │  │  Tab 3   │      │
│  │ Claude A │  │ Claude B │  │  Human   │      │
│  └────┬─────┘  └────┬─────┘  └──────────┘      │
│       │              │                           │
│  ┌────┴─────┐  ┌────┴─────┐                     │
│  │ MCP srv  │  │ MCP srv  │  (each Claude Code  │
│  │ (stdio)  │  │ (stdio)  │   spawns its own)   │
│  └────┬─────┘  └────┴─────┘                     │
│       │              │                           │
│       └──────┬───────┘                           │
│              │                                   │
│     ┌────────▼────────┐                          │
│     │  ~/.glass/      │                          │
│     │  agents.db      │  (shared SQLite, WAL)    │
│     └─────────────────┘                          │
└─────────────────────────────────────────────────┘
```

Each `glass mcp serve` instance connects to the same `~/.glass/agents.db`. SQLite WAL mode handles concurrent readers/writers safely.

## New Crate: `glass_coordination`

Handles all shared agent state. Pure library — no async runtime, just synchronous SQLite operations.

### Database Schema (`~/.glass/agents.db`)

```sql
-- Active agent registry
CREATE TABLE agents (
    id          TEXT PRIMARY KEY,        -- UUID assigned on register
    name        TEXT NOT NULL,           -- Human-readable label
    agent_type  TEXT NOT NULL,           -- "claude-code", "cursor", "copilot", "human"
    project     TEXT NOT NULL,           -- Canonical project root (scopes locks/visibility)
    cwd         TEXT NOT NULL,           -- Working directory
    pid         INTEGER,                 -- OS process ID (for liveness fallback)
    status      TEXT NOT NULL DEFAULT 'active',  -- active | busy | idle
    task        TEXT,                    -- What the agent is currently doing
    registered_at  INTEGER NOT NULL,
    last_heartbeat INTEGER NOT NULL
);

-- Advisory file locks (exclusive per path)
CREATE TABLE file_locks (
    path       TEXT PRIMARY KEY,             -- Canonical absolute path (one lock per file)
    agent_id   TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    reason     TEXT,
    locked_at  INTEGER NOT NULL
);
CREATE INDEX idx_locks_agent ON file_locks(agent_id);

-- Inter-agent messages
CREATE TABLE messages (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    from_agent  TEXT NOT NULL REFERENCES agents(id) ON DELETE SET NULL,  -- Preserve messages after sender deregisters
    to_agent    TEXT,                    -- NULL = broadcast to all
    msg_type    TEXT NOT NULL DEFAULT 'info',  -- info | conflict_warning | task_complete | request_unlock
    content     TEXT NOT NULL,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
    read        INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_messages_unread ON messages(to_agent, read);  -- Composite index for read_messages query
```

### Public API

```rust
pub struct CoordinationDb { conn: Connection }

impl CoordinationDb {
    // Lifecycle
    pub fn open() -> Result<Self>;                          // Opens ~/.glass/agents.db

    // Registration
    pub fn register(&self, name, agent_type, project, cwd, pid) -> Result<String>;  // Returns agent_id (UUID)
    pub fn deregister(&self, agent_id) -> Result<()>;      // Remove + cascade locks (messages preserved)
    pub fn heartbeat(&self, agent_id) -> Result<()>;       // Update last_heartbeat
    pub fn set_status(&self, agent_id, status, task) -> Result<()>;

    // Discovery
    pub fn list_agents(&self) -> Result<Vec<AgentInfo>>;   // Active agents (prunes stale)
    pub fn get_agent(&self, agent_id) -> Result<Option<AgentInfo>>;

    // File locks (advisory, atomic — returns Ok or Conflict, no separate check needed)
    pub fn lock_files(&self, agent_id, paths, reason) -> Result<LockResult>;  // Atomic: locks all or returns conflicts
    pub fn unlock_file(&self, agent_id, path) -> Result<()>;
    pub fn unlock_all(&self, agent_id) -> Result<()>;
    pub fn list_locks(&self, project: Option<&str>) -> Result<Vec<FileLock>>;  // Filter by project

    // Messaging
    pub fn broadcast(&self, from_agent, msg_type, content) -> Result<()>;
    pub fn send_message(&self, from_agent, to_agent, msg_type, content) -> Result<()>;
    pub fn read_messages(&self, agent_id) -> Result<Vec<Message>>;  // Unread, marks as read

    // Cleanup
    pub fn prune_stale(&self, timeout_secs: i64) -> Result<Vec<String>>;  // Returns pruned IDs
}
```

### Path Canonicalization

All file paths are canonicalized to absolute paths before storing in the database. This prevents the same file being locked under different representations (`src/main.rs` vs `./src/main.rs` vs `C:\Users\...\src\main.rs`). The `lock_files` method resolves paths using `std::fs::canonicalize()` before insertion.

### Stale Agent Detection

Agents are considered stale if `last_heartbeat` is older than 5 minutes **or** if the stored `pid` is no longer running (checked via OS process liveness). `prune_stale()` removes stale agents and cascades to their locks. Sent messages from pruned agents are preserved (from_agent set to NULL) so recipients can still read them. Called automatically on `list_agents()` and `list_locks()`.

Agents should send heartbeats every **60 seconds**. The 5-minute timeout provides tolerance for ~4 missed heartbeats before pruning.

## New MCP Tools

Added to `glass_mcp/src/tools.rs`. Each tool calls into `glass_coordination`.

### 1. `glass_agent_register`
```
Input:  { name: "Claude A", agent_type: "claude-code", project: "/home/user/myapp", cwd: "/home/user/myapp" }
Output: { agent_id: "a1b2c3", agents_active: 2 }
```
Register this agent. `project` is the repo/project root used to scope lock visibility. Returns assigned ID and count of other active agents in the same project (immediate awareness).

### 2. `glass_agent_deregister`
```
Input:  { agent_id: "a1b2c3" }
Output: { ok: true }
```
Unregister. Releases all locks, cleans up messages.

### 3. `glass_agent_list`
```
Input:  {}
Output: { agents: [{ id, name, agent_type, cwd, status, task, last_heartbeat }] }
```
List all active agents. Auto-prunes stale entries.

### 4. `glass_agent_status`
```
Input:  { agent_id: "a1b2c3", status: "busy", task: "refactoring config module" }
Output: { ok: true }
```
Update own status and current task description.

### 5. `glass_agent_lock`
```
Input:  { agent_id: "a1b2c3", paths: ["src/main.rs", "Cargo.toml"], reason: "adding feature X" }
Output: { locked: ["src/main.rs", "Cargo.toml"], conflicts: [] }
   — or —
Output: { locked: [], conflicts: [{ path: "src/main.rs", held_by: "Claude B", reason: "fixing bug" }] }
```
Atomically claim advisory locks on files. Paths are canonicalized before storage. If any path conflicts, **none are locked** — the agent must resolve conflicts first and retry. This prevents partial-lock states and eliminates the TOCTOU gap of check-then-lock.

### 6. `glass_agent_unlock`
```
Input:  { agent_id: "a1b2c3", paths: ["src/main.rs"] }
Output: { ok: true }
```
Release file locks. Omit `paths` to release all.

### 7. `glass_agent_locks`
```
Input:  {}
Output: { locks: [{ path, agent_id, agent_name, reason, locked_at }] }
```
List all active file locks across all agents.

### 8. `glass_agent_broadcast`
```
Input:  { agent_id: "a1b2c3", msg_type: "info", content: "Starting refactor of glass_renderer, avoid editing" }
Output: { delivered_to: 2 }
```
Send message to all other agents in the same project.

### 9. `glass_agent_send`
```
Input:  { agent_id: "a1b2c3", to_agent: "d4e5f6", msg_type: "request_unlock", content: "Need src/main.rs for bug fix" }
Output: { ok: true }
```
Send a directed message to a specific agent.

### 10. `glass_agent_messages`
```
Input:  { agent_id: "a1b2c3" }
Output: { messages: [{ from: "Claude B", msg_type: "info", content: "Done with renderer changes", at: "..." }] }
```
Read unread messages. Marks them as read. `from` is null if the sender has since deregistered.

### 11. `glass_agent_heartbeat`
```
Input:  { agent_id: "a1b2c3" }
Output: { ok: true }
```
Keep-alive ping. Should be called every 60 seconds.

## CLAUDE.md Integration

Add these instructions so Claude Code auto-coordinates:

```markdown
## Multi-Agent Coordination
Glass provides agent coordination via MCP. Follow these rules:
- On session start: call glass_agent_register to announce yourself
- Before modifying files: call glass_agent_lock to claim them (atomic — returns conflicts if held)
- When done with files: call glass_agent_unlock to release them
- Update your status with glass_agent_status when starting/finishing tasks
- Check glass_agent_messages periodically for messages from other agents
- If a lock conflicts: use glass_agent_send with msg_type "request_unlock" to ask the holder
- On session end: call glass_agent_deregister to clean up
```

## Implementation Plan

### Phase 1: Coordination Crate
- Create `crates/glass_coordination/`
- SQLite schema, `CoordinationDb` struct, all public methods
- Unit tests for register/deregister, locks, messages, stale pruning
- **Files:** `Cargo.toml`, `src/lib.rs`

### Phase 2: MCP Tools
- Add `glass_coordination` dependency to `glass_mcp`
- Implement 11 new MCP tool handlers in `tools.rs`
- Wire `CoordinationDb::open()` into MCP server startup
- **Files:** `crates/glass_mcp/Cargo.toml`, `crates/glass_mcp/src/tools.rs`

### Phase 3: Integration & Testing
- Add coordination instructions to CLAUDE.md
- Integration test: two MCP instances coordinating via shared DB
- Manual testing with two Claude Code sessions in Glass tabs

### Phase 4: GUI Integration (future)
- Show active agents in Glass status bar
- Visual indicator on tab when agent holds file locks
- Conflict warning overlay when two agents touch same file

## Key Design Decisions

1. **SQLite over IPC** — MCP servers are separate processes. SQLite WAL gives concurrent access without building an IPC protocol. Simple, reliable, zero new dependencies (rusqlite already in workspace).

2. **Advisory locks, not enforced** — Agents can ignore locks. Enforcement would require intercepting file writes at the PTY level, which is fragile. Advisory locks work because AI agents follow instructions.

3. **Heartbeat-based liveness** — No persistent connections between MCP instances. Stale detection via heartbeat timeout (5 min, 60s recommended interval) with auto-pruning. PID liveness checked as immediate fallback — if the process is gone, the agent is pruned without waiting for timeout.

4. **UUID agent IDs** — Not SessionId. MCP servers don't know their Glass SessionId (they're separate processes). UUIDs are self-assigned and globally unique.

5. **Messages are semi-ephemeral** — Read-once semantics. Messages survive sender deregistration (from_agent set to NULL) so recipients can still read them. Pruned when the recipient is pruned. Not a durable message queue — for coordination signals, not audit logs.

6. **Atomic lock acquisition** — `lock_files` is all-or-nothing within a transaction. If any requested path is already held by another agent, the entire request fails with conflict details. This eliminates the TOCTOU race of check-then-lock and prevents partial-lock deadlocks.

7. **Path canonicalization** — All file paths are resolved to canonical absolute paths before storage, preventing the same file from being locked under different representations across agents.

8. **Project scoping** — Agents register with a `project` root. Lock visibility and agent listing are scoped by project, so agents on unrelated repos don't interfere with each other.

9. **Structured message types** — Messages carry a `msg_type` field (`info`, `conflict_warning`, `task_complete`, `request_unlock`) so agents can programmatically triage and respond to coordination signals without parsing free text.

## Dependencies

- `rusqlite` — already in workspace
- `uuid` — new dependency (small, no-std compatible), for agent ID generation
- No new async dependencies — all DB ops are synchronous, wrapped in `spawn_blocking` at MCP layer

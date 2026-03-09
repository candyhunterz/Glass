# Pitfalls Research: Multi-Agent Coordination

**Domain:** Adding multi-agent coordination (SQLite WAL shared DB, advisory locks, heartbeat liveness, inter-agent messaging) to an existing GPU-accelerated terminal emulator
**Researched:** 2026-03-09
**Confidence:** HIGH (pitfalls are well-documented across SQLite docs, Rust ecosystem, and distributed systems literature)

---

## Critical Pitfalls

### Pitfall 1: SQLite WAL Checkpoint Starvation and Unbounded WAL Growth

**What goes wrong:**
The `agents.db` WAL file grows without bound. Each `glass mcp serve` process opens a connection and performs periodic reads (heartbeats, message checks, lock queries). If there is always at least one active reader holding a snapshot, SQLite cannot checkpoint the WAL file because it cannot overwrite pages that any reader might still need. With multiple MCP servers each polling on their own schedules, there is rarely a "reader gap" where all connections are idle simultaneously. The WAL file grows from kilobytes to hundreds of megabytes over days of continuous use.

**Why it happens:**
SQLite WAL checkpointing requires that no reader holds a snapshot of pages being checkpointed. The design calls for multiple MCP processes each with their own connection, plus the GUI polling coordination state. If any of these connections has an open read transaction (even briefly), it prevents checkpoint progress. The default auto-checkpoint (1000 pages) triggers a PASSIVE checkpoint that does its best but silently stops at the first page held by a reader. Developers see "checkpoint succeeded" and assume the WAL was fully recycled, but it was only partially checkpointed.

**How to avoid:**
1. Set `PRAGMA wal_autocheckpoint = 100;` on `agents.db` connections to checkpoint more aggressively (the coordination DB is tiny -- 3 small tables).
2. Never hold read transactions open longer than necessary. Each `spawn_blocking` call should open a connection, execute, and close. Do NOT cache a `CoordinationDb` connection across multiple MCP tool calls.
3. On `CoordinationDb::open()`, attempt a `PRAGMA wal_checkpoint(TRUNCATE)` which truncates the WAL to zero bytes if it can get exclusive access. This opportunistically cleans up on startup.
4. Consider periodic `PRAGMA wal_checkpoint(RESTART)` calls during `prune_stale()` since pruning already implies a quiet moment.

**Warning signs:**
- `agents.db-wal` file grows beyond 1MB (the actual data should be under 100KB)
- `PRAGMA wal_checkpoint` returns `(0, N, 0)` where N is large (meaning N pages checkpointed but 0 moved back -- all blocked by readers)
- Disk usage in `~/.glass/` grows steadily over multi-day sessions

**Phase to address:**
Phase 1 (Coordination Crate) -- bake checkpoint strategy into `CoordinationDb::open()` and expose a `maintenance()` method.

---

### Pitfall 2: SQLITE_BUSY Despite busy_timeout with Transaction Upgrades

**What goes wrong:**
MCP tool handlers get intermittent `SQLITE_BUSY` / "database is locked" errors that surface as MCP error responses to AI agents. The agents retry, get the same error, and eventually give up or hallucinate that coordination is broken. This happens even though `busy_timeout = 5000` is set.

**Why it happens:**
The existing codebase uses `PRAGMA busy_timeout = 5000` (seen in both `glass_history/db.rs` and `glass_snapshot/db.rs`). This works for simple read/write patterns. But the coordination crate introduces read-then-write transactions: `lock_files` must read existing locks, check for conflicts, then insert new locks -- all atomically in one transaction. If the transaction starts as a deferred read (`BEGIN` default) and later tries to upgrade to a write lock, SQLite may return `SQLITE_BUSY` immediately without honoring the busy timeout, because another connection already holds a read lock that would conflict with the upgrade.

The design document correctly specifies atomic lock acquisition, but the implementation must use `BEGIN IMMEDIATE` (or `BEGIN EXCLUSIVE`) for any transaction that will write. Rusqlite's `conn.transaction()` defaults to `DEFERRED`.

**How to avoid:**
1. Use `conn.transaction_with_behavior(TransactionBehavior::Immediate)` for ALL write transactions in `CoordinationDb`. This is critical for `lock_files`, `register`, `deregister`, `heartbeat`, `broadcast`, `send_message`, and `prune_stale`.
2. Keep the 5000ms busy_timeout but understand it only works when the lock contention happens at `BEGIN IMMEDIATE` time (which is the correct place).
3. For read-only operations (`list_agents`, `list_locks`, `read_messages`), deferred transactions are fine.
4. Handle `SQLITE_BUSY` at the MCP layer with a single retry + exponential backoff before returning an error to the agent.

**Warning signs:**
- Sporadic "database is locked" errors in MCP tool responses
- Errors correlate with multiple agents performing actions simultaneously
- Errors disappear when only one agent is active

**Phase to address:**
Phase 1 (Coordination Crate) -- use `Immediate` for all write transactions from day one. This is not something to "fix later."

---

### Pitfall 3: Path Canonicalization Produces UNC Paths on Windows

**What goes wrong:**
Agent A locks `src/main.rs` which gets canonicalized to `\\?\C:\Users\nkngu\apps\Glass\src\main.rs`. Agent B tries to lock the same file, but its CWD resolves differently (or it passes an already-absolute path), producing `C:\Users\nkngu\apps\Glass\src\main.rs` (without UNC prefix). The two strings don't match, so both agents believe they have exclusive locks on the same file. The entire coordination system silently fails.

**Why it happens:**
Rust's `std::fs::canonicalize()` on Windows calls `GetFinalPathNameByHandleW` which returns extended-length path syntax (`\\?\`). This is a well-documented Rust issue (rust-lang/rust#42869, open since 2017). The design document specifies `std::fs::canonicalize()` for path normalization, which will produce inconsistent results depending on:
- Whether the caller passes a relative or absolute path
- Whether the path goes through junctions or symlinks
- Whether the file exists at canonicalization time

The existing codebase already uses `canonicalize()` in `glass_snapshot/watcher.rs` and `ignore_rules.rs`, but those are single-process -- the same connection always sees the same UNC-prefixed paths. With multi-process coordination, different processes may produce different path forms for the same file.

**How to avoid:**
1. Add the `dunce` crate (small, well-maintained, already used by Deno, Cargo, and many Rust projects). Use `dunce::canonicalize()` instead of `std::fs::canonicalize()` everywhere in `glass_coordination`. This strips the `\\?\` prefix when it's safe to do so, producing consistent paths across all callers.
2. Additionally normalize path separators to forward slashes before storing in the DB. This handles edge cases where paths come from different shells (PowerShell uses `\`, bash uses `/`).
3. Store paths as lowercase on Windows (NTFS is case-insensitive by default). Use `path.to_string_lossy().to_lowercase()` on `cfg(target_os = "windows")`.
4. If `dunce::canonicalize()` fails (file doesn't exist yet), fall back to `std::path::absolute()` + normalization rather than returning an error.

**Warning signs:**
- Two agents hold locks on what looks like the same file but with different path prefixes
- Lock conflicts never trigger even when agents are clearly editing the same files
- Works on macOS/Linux but fails silently on Windows

**Phase to address:**
Phase 1 (Coordination Crate) -- the path normalization function must be correct before any lock logic is built on top of it. Write a `normalize_path()` helper with explicit tests for UNC, relative, forward-slash, and case-insensitive scenarios.

---

### Pitfall 4: PID Reuse Causing Incorrect Stale Agent Pruning or Ghost Agents

**What goes wrong:**
Two failure modes: (1) A stale agent's PID gets reused by an unrelated process. The liveness check sees the PID is alive, so the stale agent is never pruned. Its file locks persist forever, blocking other agents. (2) A new agent process happens to get the same PID as a recently-crashed agent. The system incorrectly associates the new process with the old agent's registration, causing identity confusion.

**Why it happens:**
The design uses PID as a "fast liveness fallback" -- if the PID is dead, the agent is immediately prunable without waiting for the 5-minute heartbeat timeout. On long-running systems, PID reuse is guaranteed (Linux recycles PIDs from a pool of 32768 by default, Windows has no guaranteed minimum cycle time). The race window is: Agent A crashes at time T, its PID is recycled at T+1s, prune_stale runs at T+2s, sees PID alive, keeps the stale agent registered.

**How to avoid:**
1. Use PID-based pruning only as an acceleration, never as the sole liveness signal. The heartbeat timeout (5 minutes) is the authoritative signal.
2. When checking PID liveness, also verify the process start time if the OS provides it. On Windows, use `GetProcessTimes()` via `windows-sys`. On Unix, read `/proc/<pid>/stat` field 22 (start time). If the process start time is newer than the agent's `registered_at`, the PID was reused.
3. Store the process start time in the `agents` table alongside `pid`. This makes the PID+start_time pair globally unique.
4. If start-time verification is too complex for Phase 1, accept the limitation: PID liveness is advisory and the 5-minute heartbeat timeout is the real cleanup mechanism. Document this explicitly.

**Warning signs:**
- Stale agents persist in `glass_agent_list` output after their terminal tab is closed
- Lock conflicts reference agents that no longer exist
- `prune_stale()` runs but doesn't remove agents it should

**Phase to address:**
Phase 1 (Coordination Crate) -- implement PID check with start-time verification, or explicitly document the limitation and rely on heartbeat timeout as primary. Phase 3 (Integration Testing) should include a test that simulates PID reuse.

---

### Pitfall 5: Atomic Lock Acquisition Deadlock Under Contention

**What goes wrong:**
Agent A requests locks on `[main.rs, config.rs]`. Agent B simultaneously requests locks on `[config.rs, main.rs]`. Both agents' `lock_files` transactions start concurrently. Under the all-or-nothing design, both fail (each sees the other holding one file). They retry immediately, and the same thing happens. This produces a livelock where neither agent can make progress.

**Why it happens:**
The design correctly avoids partial-lock states with all-or-nothing semantics. But it doesn't specify retry behavior or lock ordering. Without consistent ordering, concurrent lock requests on overlapping file sets will repeatedly conflict. AI agents following the CLAUDE.md instructions will retry lock acquisition, potentially in a tight loop.

**How to avoid:**
1. Sort requested paths lexicographically before acquiring locks inside the `lock_files` transaction. This establishes a consistent lock ordering that prevents the A-B / B-A deadlock pattern.
2. In the MCP tool response for conflicts, include a `retry_after_ms` hint (e.g., 1000-3000ms with jitter) so agents don't retry immediately.
3. In the CLAUDE.md coordination instructions, explicitly tell agents: "If lock acquisition fails, wait at least 2 seconds before retrying. After 3 failures, use glass_agent_send to negotiate with the lock holder."
4. Inside the SQLite transaction, insert locks in sorted order to minimize the write-lock hold time.

**Warning signs:**
- Agents report repeated lock conflicts on the same files
- MCP tool call volume spikes with lock/unlock cycles
- Agents get stuck in retry loops visible in their conversation history

**Phase to address:**
Phase 1 (Coordination Crate) -- sort paths in `lock_files`. Phase 2 (MCP Tools) -- add `retry_after_ms` to conflict responses. Phase 3 (Integration) -- CLAUDE.md retry guidance.

---

### Pitfall 6: Stale Agent Cleanup Race During Concurrent prune_stale Calls

**What goes wrong:**
Two MCP servers call `list_agents()` simultaneously. Both trigger `prune_stale()`. Both identify Agent X as stale. Agent A deletes Agent X. Agent B tries to delete Agent X, fails silently (or causes a cascading FK delete on already-deleted locks). Messages from Agent X are orphaned or double-processed. In the worst case, a new agent that registered between the two prune calls gets its data corrupted.

**Why it happens:**
The design says `prune_stale()` is called automatically on `list_agents()` and `list_locks()`. Multiple MCP servers calling these concurrently will trigger concurrent prune operations on the same shared DB. Without idempotent delete logic, this creates races.

**How to avoid:**
1. Make `prune_stale()` idempotent: use `DELETE FROM agents WHERE id IN (SELECT id FROM agents WHERE ...)` in a single statement rather than selecting stale IDs then deleting them in separate steps.
2. Use `BEGIN IMMEDIATE` for the prune transaction (it writes).
3. Accept that multiple processes may prune concurrently -- the operation should be harmless when repeated.
4. Consider rate-limiting prune calls: only prune if the last prune was more than 60 seconds ago (store a module-level timestamp or a DB metadata row).

**Warning signs:**
- "no such row" or "foreign key constraint failed" errors during prune operations
- `glass_agent_list` returns inconsistent results between rapid calls
- Messages disappear unexpectedly

**Phase to address:**
Phase 1 (Coordination Crate) -- implement prune as a single idempotent SQL statement.

---

## Moderate Pitfalls

### Pitfall 7: Heartbeat Timer Drift in MCP Processes

**What goes wrong:**
AI agents are instructed to call `glass_agent_heartbeat` every 60 seconds. In practice, MCP tool calls happen as part of agent reasoning, which is inherently irregular. An agent deep in a complex code change may not call any MCP tools for 6+ minutes. The agent gets pruned mid-task, its file locks are released, and another agent swoops in and edits the same files.

**Why it happens:**
AI agents don't have reliable timer mechanisms. They call MCP tools when their reasoning loop dictates, not on a schedule. The CLAUDE.md instruction "call heartbeat every 60 seconds" is aspirational, not enforceable. Claude Code's internal MCP call patterns are determined by the model's reasoning, not by wall-clock timers.

**How to avoid:**
1. Piggyback heartbeats on ALL MCP tool calls. Every `glass_agent_*` tool should silently refresh `last_heartbeat` for the calling agent. If Agent A calls `glass_agent_lock`, `glass_agent_status`, or `glass_agent_messages`, each call also updates the heartbeat. This way, any agent activity automatically extends liveness.
2. Increase the stale timeout from 5 minutes to 10 minutes. A coding agent may easily go 5 minutes between tool calls during complex refactoring.
3. In CLAUDE.md instructions, add: "If you will be performing a long operation (>3 minutes) without MCP calls, call glass_agent_heartbeat before starting."
4. Consider making the stale timeout configurable in `config.toml` under a `[coordination]` section.

**Warning signs:**
- Active agents get pruned during long coding sessions
- Locks disappear while an agent is still working
- Agents re-register frequently (sign they were pruned and noticed)

**Phase to address:**
Phase 1 (implicit heartbeat on all tool calls), Phase 2 (MCP tool implementations), Phase 3 (CLAUDE.md instructions and timeout tuning).

---

### Pitfall 8: Message Ordering Not Guaranteed Across Concurrent Writers

**What goes wrong:**
Agent A sends a broadcast "I'm starting work on renderer" at T=0. Agent B sends "I need renderer locks" at T=0.001. Due to SQLite write serialization and the timing of `BEGIN IMMEDIATE` lock acquisition, Agent B's message may get `AUTOINCREMENT` ID 5 while Agent A's gets ID 6. When Agent C reads messages, it sees B's request before A's announcement, leading to incorrect coordination decisions.

**Why it happens:**
SQLite `AUTOINCREMENT` guarantees monotonically increasing IDs within a single connection, but not across concurrent connections. With WAL mode, the write lock is held briefly for each INSERT, and the ordering depends on which connection acquires the write lock first. This is a wall-clock race, not a logical ordering issue.

**How to avoid:**
1. Accept that message ordering is approximate, not strict. The `created_at` column already uses `unixepoch()` which has 1-second granularity -- messages within the same second are inherently unordered.
2. Do NOT rely on message ID ordering for correctness. Use `created_at` for display ordering and treat messages as unordered within the same second.
3. For coordination correctness, rely on the lock mechanism (which IS atomic) rather than message ordering. Messages are informational; locks are authoritative.
4. If sub-second ordering ever matters, add a `created_at_ms` column using a custom function or application-supplied timestamp.

**Warning signs:**
- Agents make decisions based on message order that don't match chronological reality
- "I announced first but the other agent didn't see it" type coordination failures

**Phase to address:**
Phase 2 (MCP Tools) -- document in tool descriptions that messages are informational and ordering is approximate.

---

### Pitfall 9: GUI Polling Overhead for Coordination State

**What goes wrong:**
The Glass GUI adds a timer to poll `agents.db` every 500ms to update status bar indicators and tab decorations. Each poll opens a SQLite connection (or reuses a cached one), runs queries, and triggers a redraw. With 60fps vsync rendering, this adds measurable latency to the event loop. The terminal feels sluggish, especially when multiple panes are open and each triggers coordination state queries.

**Why it happens:**
The GUI event loop in Glass is driven by winit's `EventLoop<AppEvent>`. The existing pattern uses `EventLoopProxy::send_event()` from background threads (PTY reader, config watcher, update checker). Adding another polling thread for coordination state seems natural, but the frequency must be tuned. Opening a SQLite connection per poll is expensive (~0.5ms per open on Windows). Even reusing a connection, the poll query + event dispatch + redraw cycle adds CPU overhead that compounds with pane count.

**How to avoid:**
1. Poll coordination state infrequently: every 5 seconds is sufficient for status bar updates. Agent activity doesn't change rapidly.
2. Use a dedicated background thread (not tokio) that sends `AppEvent::CoordinationUpdate(CoordinationState)` through the existing `EventLoopProxy`. This keeps SQLite I/O off the render thread entirely.
3. Cache the last coordination state in memory. Only send an event (and trigger redraw) when state actually changes (agent count, lock count, or unread messages differ from cached values).
4. Open a single `CoordinationDb` connection in the background thread and reuse it. Set `PRAGMA busy_timeout = 1000` (shorter than MCP connections) so the GUI thread doesn't block long on contention.
5. Do NOT poll during frames -- the background thread should be completely independent of the render loop.

**Warning signs:**
- Cold start time increases by >50ms after adding coordination GUI
- Input latency increases measurably (>1ms increase in bench)
- Status bar flickering or unnecessary redraws
- CPU usage increases at idle (polling with no agents active)

**Phase to address:**
Phase 4 (GUI Integration) -- design the polling architecture before adding visual elements.

---

### Pitfall 10: CoordinationDb Connection Shared Across spawn_blocking Calls

**What goes wrong:**
The developer caches a `CoordinationDb` instance in `GlassServer` (alongside `db_path` and `glass_dir`) and reuses it across all MCP tool calls. Since `Connection` is `!Send + !Sync`, this either fails to compile or requires `Arc<Mutex<CoordinationDb>>`. With the mutex, all 11 coordination tools serialize through a single lock, creating contention. Worse, a single long-running transaction (like `lock_files` with many paths) blocks all other tool calls including heartbeats.

**Why it happens:**
The existing `GlassServer` pattern clones `db_path` and opens a fresh `HistoryDb` connection inside each `spawn_blocking` call. This works because each tool call is independent. The temptation with `CoordinationDb` is to optimize by caching the connection, since "it's the same DB and opening connections is expensive." But rusqlite `Connection` is deliberately `!Send` -- you cannot share it across threads without unsafe code.

**How to avoid:**
1. Follow the existing pattern exactly: clone `agents_db_path` into `GlassServer`, open a fresh `CoordinationDb` inside each `spawn_blocking` call. SQLite connection opens are ~0.5ms, which is negligible for MCP tool call latency.
2. If connection open overhead proves measurable, use `thread_local!` storage inside the `spawn_blocking` closure to reuse connections within the same OS thread (tokio may schedule multiple spawn_blocking calls on the same thread pool thread).
3. Do NOT use `Arc<Mutex<Connection>>` -- it serializes all tool calls and defeats SQLite WAL's concurrent read capability.

**Warning signs:**
- Compile errors about `Send`/`Sync` bounds on `Connection`
- `Arc<Mutex<>>` wrapper around the DB connection
- All MCP tool calls serialized (visible in tracing output as sequential, never overlapping)

**Phase to address:**
Phase 2 (MCP Tools) -- use the same open-per-call pattern as existing tools from the start.

---

### Pitfall 11: File Lock Scope Mismatch With Project Scoping

**What goes wrong:**
Agent A registers with project `/home/user/myapp`. Agent B registers with project `/home/user/myapp/` (trailing slash). They are treated as different projects, so their locks don't conflict. Alternatively, Agent A uses the project root while Agent B uses a subdirectory as its project root. Lock scoping fails silently because paths are canonicalized but project strings may not be.

**Why it happens:**
The `project` field in the `agents` table is a freeform `TEXT` field set by the agent during registration. The design says "scopes locks/visibility" but doesn't specify normalization. Agents pass whatever path their `cwd` resolves to, which may differ in trailing slash, symlink resolution, or case.

**How to avoid:**
1. Canonicalize the `project` path using the same `normalize_path()` helper used for file locks. Strip trailing separators.
2. The `lock_files` and `list_locks` queries should match on canonical project path, not on string equality of user-provided paths.
3. Consider scoping locks by the canonical path prefix: if Agent A's project is `/home/user/myapp` and locks `/home/user/myapp/src/main.rs`, Agent B with project `/home/user/myapp/frontend` should still see the conflict because the locked file is under B's project too.
4. Store the canonical project path at registration time and use it consistently.

**Warning signs:**
- Agents in the same repo don't see each other's locks
- `glass_agent_list` shows agents with slightly different project paths for the same repo
- Locks exist but `list_locks` returns empty (project filter mismatch)

**Phase to address:**
Phase 1 (Coordination Crate) -- normalize project paths at registration time.

---

### Pitfall 12: ON DELETE CASCADE Surprises with Foreign Keys

**What goes wrong:**
Deleting a stale agent via `prune_stale()` cascades to delete all its file locks (correct) but also sets `from_agent = NULL` on all its messages via `ON DELETE SET NULL` (correct by design but potentially surprising). If a message references a pruned agent and the recipient reads it, the `from` field is null. The agent tool response says "message from: null" which is confusing for AI agents trying to coordinate.

**Why it happens:**
The schema correctly uses `ON DELETE CASCADE` for `file_locks` and `ON DELETE SET NULL` for `messages.from_agent`. But the MCP tool `glass_agent_messages` response format shows `from: "Claude B"` by joining against the agents table. After pruning, this join returns NULL, and the response shows `from: null`.

**How to avoid:**
1. Store the sender's name in the messages table itself (`from_name TEXT`) in addition to the `from_agent` foreign key. This denormalizes slightly but ensures messages remain readable after the sender is pruned.
2. Alternatively, in the `read_messages` query, use `COALESCE(agents.name, '(departed agent)')` to provide a human-readable fallback.
3. The MCP tool response should handle null sender gracefully: `"from": "(departed agent)"` rather than `"from": null`.

**Warning signs:**
- Messages with `from: null` appearing in agent message feeds
- AI agents confused by messages from "null" and unable to respond
- Messages becoming useless after sender departure

**Phase to address:**
Phase 1 (schema design) -- add `from_name` column. Phase 2 (MCP tools) -- handle null sender in response formatting.

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Skip PID start-time verification | Simpler implementation, no platform-specific code | Rare ghost agents on long-running systems | Phase 1 MVP only; add start-time check before v2.2 release |
| Open new DB connection per MCP call | Simple, follows existing pattern, no Send/Sync issues | ~0.5ms overhead per tool call | Always acceptable for coordination DB (tiny and infrequent) |
| String equality for project scoping | No path normalization complexity | Mismatched projects when paths differ in trailing slash/case | Never -- normalize from day one |
| 5-minute stale timeout without tuning | Works for fast agent sessions | Active agents pruned during long coding tasks | Phase 1 only; increase to 10 minutes and make configurable |
| No message retention policy | Messages accumulate indefinitely | DB bloat over weeks of use | Phase 1 only; add `max_message_age` pruning in Phase 3 |
| Case-sensitive path comparison on Windows | Works if all callers use same case | Silent lock bypass when cases differ | Never on Windows -- lowercase normalize from day one |

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| glass_mcp tool_router | Adding 11 tools to existing `#[tool_router]` makes the impl block massive and hard to navigate | Create a separate `CoordinationTools` struct or split into `coordination_tools.rs` module; delegate to `GlassServer` via composition |
| rusqlite Connection in async context | Holding a `Connection` reference across an `.await` point causes `!Send` errors | Always clone paths/data INTO the `spawn_blocking` closure; open `CoordinationDb` inside the closure |
| PRAGMA settings not inherited | Opening a new connection doesn't inherit PRAGMAs from other connections to the same DB | Every `CoordinationDb::open()` must set WAL mode, busy_timeout, and foreign_keys -- these are per-connection settings (except WAL which is persistent but should still be set defensively) |
| uuid crate feature flags | `uuid = "1"` alone doesn't include generation functions | Use `uuid = { version = "1", features = ["v4"] }` for `Uuid::new_v4()` |
| Existing `Cargo.toml` workspace | Adding `glass_coordination` crate without updating workspace members | Add to `[workspace] members` in root `Cargo.toml` and verify `cargo build --workspace` succeeds |
| ON DELETE CASCADE requires PRAGMA | Foreign key constraints are OFF by default in SQLite | Every connection must set `PRAGMA foreign_keys = ON;` BEFORE any operations -- the existing codebase already does this, but the new crate must too |

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| GUI polling coordination DB every frame | Dropped frames, increased idle CPU, sluggish input | Poll every 5 seconds from a background thread; only trigger redraw on state change | Immediately visible with 2+ panes open |
| Selecting all messages without pagination | `read_messages` returns thousands of old messages | Add `LIMIT 100` to message queries; prune messages older than 24 hours | After days of active multi-agent use |
| `canonicalize()` on every lock operation | Filesystem syscall per path per lock call | Cache canonical paths within a single `lock_files` transaction; batch canonicalization | With 20+ files locked per request |
| WAL file not truncated | Disk usage grows, reads slow as WAL is scanned | Periodic `PRAGMA wal_checkpoint(TRUNCATE)` during maintenance | After hours of continuous multi-agent use |
| Heartbeat UPDATE without index | Full table scan on every heartbeat | The `agents` table uses `id TEXT PRIMARY KEY` which is already indexed; no action needed | N/A (already handled) |

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| Agent IDs are guessable UUIDs | Any process with filesystem access to `agents.db` can impersonate another agent (deregister it, steal locks, read messages) | Accept this risk -- the system is advisory and local-only. Document that coordination is trust-based, not authenticated. No MCP over network transport (already out of scope). |
| Path traversal in lock paths | Agent locks `../../../etc/passwd` to troll other agents | Validate that locked paths are under the registered project root. Reject paths outside project scope. |
| Unbounded message content | Agent sends a 100MB message body, bloating the DB | Add a `MAX_MESSAGE_SIZE` constant (e.g., 10KB) and reject messages exceeding it in `send_message` / `broadcast`. |
| SQLite injection via path strings | Malicious path like `'; DROP TABLE agents; --` | rusqlite uses parameterized queries (`params![]`), which prevent SQL injection. No action needed as long as all queries use parameters (the existing codebase does this correctly). |

## UX Pitfalls

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| Lock conflicts show raw canonical paths | User sees `\\?\C:\Users\nkngu\apps\Glass\src\main.rs` in error messages | Strip UNC prefix and show paths relative to project root in MCP responses: `src/main.rs` |
| No indication which tab has which agent | Human can't tell which Glass tab corresponds to "Claude A" vs "Claude B" | Show agent name in tab title or a small badge on the tab bar |
| Silent heartbeat-based pruning | Agent gets pruned and its locks vanish without notification | Send a `conflict_warning` message to remaining agents when a stale agent is pruned: "Agent X was pruned due to inactivity; its locks on [files] have been released" |
| Message flood from chatty agents | Human agent gets spammed with coordination noise | Add a `glass_agent_messages` filter for `msg_type` so agents can read only `conflict_warning` messages and ignore `info` chatter |
| No way to see coordination state without MCP | Human user in a regular terminal tab can't see what agents are doing | The GUI status bar integration (Phase 4) is essential for human oversight. Before Phase 4, provide a `glass coordination list` CLI subcommand. |

## "Looks Done But Isn't" Checklist

- [ ] **Path canonicalization:** Often missing Windows case-insensitivity handling -- verify with test: `lock("src/Main.rs")` and `lock("src/main.rs")` conflict on Windows
- [ ] **Heartbeat implicit refresh:** Often missing on non-heartbeat tools -- verify that `glass_agent_lock` also updates `last_heartbeat`
- [ ] **Transaction behavior:** Often using default `DEFERRED` for writes -- verify ALL write methods use `TransactionBehavior::Immediate`
- [ ] **WAL checkpoint on open:** Often missing cleanup -- verify `CoordinationDb::open()` attempts a truncate checkpoint
- [ ] **Foreign key enforcement:** Often forgotten per-connection -- verify `PRAGMA foreign_keys = ON` in `CoordinationDb::open()`
- [ ] **Prune idempotency:** Often uses SELECT-then-DELETE pattern -- verify prune is a single atomic DELETE statement
- [ ] **Message sender name:** Often joins only on `from_agent` FK -- verify response handles NULL sender gracefully after pruning
- [ ] **UUID generation:** Often missing feature flag -- verify `uuid` dependency includes `v4` feature
- [ ] **Cross-platform PID check:** Often uses Unix-only APIs -- verify PID liveness check compiles on Windows, macOS, and Linux
- [ ] **Project path normalization:** Often stores raw user input -- verify trailing slash stripped and path canonicalized at registration

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| WAL file grows unbounded | LOW | Run `PRAGMA wal_checkpoint(TRUNCATE)` manually via SQLite CLI on `~/.glass/agents.db`. Add checkpoint to `CoordinationDb::open()` to self-heal. |
| Ghost agents with stale locks | LOW | `DELETE FROM agents WHERE last_heartbeat < unixepoch() - 600;` manually. Or restart Glass (re-open triggers prune). |
| UNC path mismatch causing duplicate locks | MEDIUM | Identify affected rows, normalize paths, deduplicate. Requires a schema migration adding a `canonical_path` index. |
| SQLITE_BUSY errors blocking agents | LOW | Agents retry naturally. Fix by switching to `BEGIN IMMEDIATE`. No data loss. |
| PID reuse causing ghost agents | LOW | The 5-10 minute heartbeat timeout eventually cleans up. Manual `glass coordination prune` CLI command as escape hatch. |
| Message ordering confusion | LOW | Not a data integrity issue. Adjust agent instructions to not rely on message order. |
| GUI polling causing frame drops | MEDIUM | Requires refactoring poll timer from render-coupled to background thread. May need to rearchitect the coordination state cache. |

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| WAL checkpoint starvation | Phase 1: Coordination Crate | `agents.db-wal` stays under 1MB after 1 hour of multi-agent use |
| SQLITE_BUSY with deferred transactions | Phase 1: Coordination Crate | `lock_files` under concurrent load never returns "database is locked" |
| UNC path canonicalization | Phase 1: Coordination Crate | Unit test: same file locked via relative, absolute, and UNC paths all conflict |
| PID reuse ghost agents | Phase 1: Coordination Crate | Integration test: simulate PID reuse, verify heartbeat timeout still prunes |
| Lock acquisition deadlock/livelock | Phase 1: Coordination Crate + Phase 2: MCP Tools | Two agents locking overlapping file sets both succeed within 10 seconds |
| Concurrent prune_stale race | Phase 1: Coordination Crate | Stress test: 5 concurrent prune calls complete without errors |
| Heartbeat timer drift | Phase 2: MCP Tools | Agent active for 8 minutes with no explicit heartbeat calls is not pruned |
| Message ordering assumptions | Phase 2: MCP Tools | Tool documentation states ordering is approximate |
| GUI polling overhead | Phase 4: GUI Integration | Input latency benchmark shows <0.5ms increase after adding coordination GUI |
| Connection sharing anti-pattern | Phase 2: MCP Tools | No `Arc<Mutex<Connection>>` in codebase; each spawn_blocking opens fresh |
| Project scope mismatch | Phase 1: Coordination Crate | Agents with `/path/to/project` and `/path/to/project/` see same locks |
| FK cascade message sender | Phase 1: Schema + Phase 2: MCP Tools | Messages readable after sender pruned; `from` field shows name not null |

## Sources

- [SQLite WAL Mode Documentation](https://sqlite.org/wal.html) -- authoritative source on checkpoint behavior, reader blocking, and WAL growth
- [SQLite Busy Timeout Pitfalls (Bert Hubert)](https://berthub.eu/articles/posts/a-brief-post-on-sqlite3-database-locked-despite-timeout/) -- explains why busy_timeout doesn't help with transaction upgrades
- [SQLite Concurrent Writes Analysis](https://tenthousandmeters.com/blog/sqlite-concurrent-writes-and-database-is-locked-errors/) -- detailed analysis of BEGIN IMMEDIATE vs DEFERRED
- [Rust std::fs::canonicalize UNC Issue #42869](https://github.com/rust-lang/rust/issues/42869) -- open since 2017, documents Windows UNC path problem
- [dunce crate](https://docs.rs/dunce) -- drop-in replacement for canonicalize that strips UNC prefix safely
- [Heartbeat Patterns (Martin Fowler)](https://martinfowler.com/articles/patterns-of-distributed-systems/heartbeat.html) -- authoritative pattern reference for distributed liveness
- [PID Reuse Race Conditions (LWN.net)](https://lwn.net/Articles/773459/) -- Linux kernel discussion of PID reuse timing
- [macOS PID Reuse Attacks (HackTricks)](https://book.hacktricks.wiki/en/macos-hardening/macos-security-and-privilege-escalation/macos-proces-abuse/macos-ipc-inter-process-communication/macos-xpc/macos-xpc-connecting-process-check/macos-pid-reuse.html) -- demonstrates real PID reuse exploitation
- [SQLite WAL Checkpoint Starvation (sqlite-users)](https://sqlite-users.sqlite.narkive.com/muT0rMYt/sqlite-wal-checkpoint-starved) -- community report of unbounded WAL growth
- [SkyPilot: Abusing SQLite for Concurrency](https://blog.skypilot.co/abusing-sqlite-to-handle-concurrency/) -- patterns for multi-process SQLite coordination
- [Fixing Claude Code Concurrent Sessions with SQLite WAL (DEV Community)](https://dev.to/daichikudo/fixing-claude-codes-concurrent-session-problem-implementing-memory-mcp-with-sqlite-wal-mode-o7k) -- directly relevant: using SQLite WAL for MCP coordination
- [NTFS Case Sensitivity Internals (Tyranid's Lair)](https://www.tiraniddo.dev/2019/02/ntfs-case-sensitivity-on-windows.html) -- explains NTFS case-sensitivity edge cases
- [SQLite WAL File Growth Guide (CopyProgramming)](https://copyprogramming.com/howto/sqlite-wal-file-size-keeps-growing) -- practical guide to WAL size management

---
*Pitfalls research for: Multi-Agent Coordination (Glass v2.2)*
*Researched: 2026-03-09*

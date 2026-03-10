# Phase 32: MCP Tools - Research

**Researched:** 2026-03-09
**Domain:** MCP server tool registration, rmcp framework, async-to-sync bridging, coordination DB integration
**Confidence:** HIGH

## Summary

Phase 32 exposes the `glass_coordination` crate's full API (built in Phase 31) as 11 MCP tool handlers within the existing `glass_mcp` crate. The existing MCP crate already implements 5 tools (history, context, undo, file_diff, pipe_inspect) using rmcp 1.1.0 with the `#[tool]`, `#[tool_router]`, and `#[tool_handler]` macro pattern. Adding 11 coordination tools follows the exact same pattern -- define parameter structs with `schemars::JsonSchema`, implement async handler methods that delegate to `spawn_blocking`, and return `CallToolResult`.

The primary technical challenge is the `&mut self` API of `CoordinationDb`. All coordination DB methods take `&mut self` because they use `transaction_with_behavior(Immediate)`. Since `GlassServer` implements `Clone` and tool handlers receive `&self`, the solution is the same open-per-call pattern already used by the existing tools: open a fresh `CoordinationDb` inside each `spawn_blocking` closure. This is both the established project pattern and the recommended approach per STATE.md decisions ("CoordinationDb is synchronous library, thread safety via open-per-call").

The second design consideration is MCP-12: all tool calls must implicitly refresh the calling agent's heartbeat. Most coordination operations already do this internally (lock_files, broadcast, send_message, read_messages all refresh heartbeat in their transactions). For tools that don't (list_agents, list_locks), an explicit heartbeat call should be added in the MCP handler when an `agent_id` parameter is present. For tools without an `agent_id` (like list_agents which takes `project`), implicit heartbeat is not applicable.

**Primary recommendation:** Add `glass_coordination` as a dependency to `glass_mcp`, store the coordination DB path in `GlassServer`, implement 11 new `#[tool]` handler methods following the existing spawn_blocking + open-per-call pattern, and update `ServerInfo` instructions. All work is in 2 files: `crates/glass_mcp/Cargo.toml` and `crates/glass_mcp/src/tools.rs`.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| MCP-01 | `glass_agent_register` tool registers agent and returns ID + active agent count | `CoordinationDb::register()` returns UUID; follow up with `list_agents()` to get count; return both in JSON response |
| MCP-02 | `glass_agent_deregister` tool unregisters agent and cascades cleanup | `CoordinationDb::deregister()` handles CASCADE on locks, SET NULL on messages |
| MCP-03 | `glass_agent_list` tool lists active agents with auto-pruning | `CoordinationDb::prune_stale()` then `list_agents()` in sequence within spawn_blocking |
| MCP-04 | `glass_agent_status` tool updates agent status and task description | `CoordinationDb::update_status()` already implicitly refreshes heartbeat |
| MCP-05 | `glass_agent_lock` tool atomically claims advisory file locks | `CoordinationDb::lock_files()` returns `LockResult::Acquired` or `LockResult::Conflict` |
| MCP-06 | `glass_agent_unlock` tool releases file locks | `CoordinationDb::unlock_file()` for specific paths, `unlock_all()` for all |
| MCP-07 | `glass_agent_locks` tool lists all active locks across agents | `CoordinationDb::list_locks(Some(project))` returns `Vec<FileLock>` |
| MCP-08 | `glass_agent_broadcast` tool sends typed message to all project agents | `CoordinationDb::broadcast()` fans out to per-recipient rows, returns count |
| MCP-09 | `glass_agent_send` tool sends directed message to specific agent | `CoordinationDb::send_message()` returns message ID |
| MCP-10 | `glass_agent_messages` tool reads unread messages | `CoordinationDb::read_messages()` marks as read, returns chronological order |
| MCP-11 | `glass_agent_heartbeat` tool refreshes liveness timestamp | `CoordinationDb::heartbeat()` updates last_heartbeat |
| MCP-12 | All MCP tool calls implicitly refresh the calling agent's heartbeat | Coordination DB methods already refresh heartbeat in most write operations; MCP handlers add explicit heartbeat for read-only tools that receive agent_id |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| rmcp | 1.1.0 | MCP protocol framework (server, tool macros) | Already in use by glass_mcp, provides #[tool], #[tool_router], #[tool_handler] |
| glass_coordination | 0.1.0 (path dep) | Coordination database operations | Phase 31 output, provides all DB logic |
| schemars | 1.0 | JSON Schema generation for MCP tool parameters | Already in glass_mcp, required by rmcp for auto-schema |
| tokio | 1.50.0 (workspace) | Async runtime, spawn_blocking for sync DB ops | Already in glass_mcp |
| serde | 1.0.228 (workspace) | Parameter deserialization | Already in glass_mcp |
| serde_json | 1.0 | JSON response construction | Already in glass_mcp |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| anyhow | 1.0.102 (workspace) | Error handling inside spawn_blocking | Already in glass_mcp |
| tracing | 0.1.44 (workspace) | Debug logging for tool calls | Already in glass_mcp |

### No New Dependencies
All libraries are already in the glass_mcp crate or workspace. The only change is adding `glass_coordination = { path = "../glass_coordination" }` to `crates/glass_mcp/Cargo.toml`.

## Architecture Patterns

### File Changes
```
crates/glass_mcp/
  Cargo.toml           # Add glass_coordination dependency
  src/
    tools.rs           # Add 11 new #[tool] handlers + parameter structs
    lib.rs             # Add coord_db_path to GlassServer constructor, update run_mcp_server
```

### Pattern 1: Open-Per-Call DB Access
**What:** Each MCP tool handler opens a fresh `CoordinationDb` inside `spawn_blocking`, does synchronous work, closes on drop.
**When to use:** Every coordination tool handler.
**Why:** `CoordinationDb` methods take `&mut self` (they use exclusive transactions). `GlassServer` is `Clone` and handlers get `&self`. Opening per-call avoids Mutex contention and matches the existing pattern for `HistoryDb`.

```rust
// Existing pattern from glass_history tools:
#[tool(description = "...")]
async fn glass_agent_register(
    &self,
    Parameters(params): Parameters<RegisterParams>,
) -> Result<CallToolResult, McpError> {
    let coord_db_path = self.coord_db_path.clone();

    let result = tokio::task::spawn_blocking(move || {
        let mut db = glass_coordination::CoordinationDb::open(&coord_db_path)
            .map_err(internal_err)?;
        let agent_id = db.register(
            &params.name,
            &params.agent_type,
            &params.project,
            &params.cwd,
            params.pid,
        ).map_err(internal_err)?;
        let agents = db.list_agents(&params.project).map_err(internal_err)?;
        Ok::<_, McpError>(serde_json::json!({
            "agent_id": agent_id,
            "agents_active": agents.len(),
        }))
    })
    .await
    .map_err(internal_err)??;

    let content = Content::json(&result)?;
    Ok(CallToolResult::success(vec![content]))
}
```

### Pattern 2: Implicit Heartbeat on Agent-ID Tools
**What:** For tools that receive an `agent_id` parameter but whose underlying DB method does NOT already refresh heartbeat, add an explicit `db.heartbeat(agent_id)` call.
**When to use:** `glass_agent_list` (if called with project, no agent_id), `glass_agent_locks` (lists locks, no heartbeat refresh). Actually, these tools don't take agent_id as a parameter, so implicit heartbeat doesn't apply to them. Most tools that take agent_id already refresh heartbeat via their DB operations.

**Analysis of which operations already refresh heartbeat:**
| Tool | DB Method | Already Refreshes? |
|------|-----------|--------------------|
| register | `register()` | YES (sets initial heartbeat) |
| deregister | `deregister()` | N/A (agent removed) |
| list | `prune_stale()` + `list_agents()` | NO -- but takes project, not agent_id |
| status | `update_status()` | YES |
| lock | `lock_files()` | YES |
| unlock | `unlock_file()` / `unlock_all()` | NO -- add explicit heartbeat |
| locks | `list_locks()` | NO -- but takes project, not agent_id |
| broadcast | `broadcast()` | YES |
| send | `send_message()` | YES |
| messages | `read_messages()` | YES |
| heartbeat | `heartbeat()` | YES (that's its purpose) |

For MCP-12 compliance, the `unlock` tool should add an explicit `db.heartbeat(agent_id)` call since it takes agent_id but `unlock_file`/`unlock_all` don't refresh heartbeat internally.

### Pattern 3: GlassServer State Extension
**What:** Add `coord_db_path: PathBuf` field to `GlassServer` struct.
**How:** The coordination DB path is resolved via `glass_coordination::resolve_db_path()` which returns `~/.glass/agents.db`.

```rust
#[derive(Clone)]
pub struct GlassServer {
    tool_router: ToolRouter<Self>,
    db_path: PathBuf,        // history DB (existing)
    glass_dir: PathBuf,      // snapshot dir (existing)
    coord_db_path: PathBuf,  // coordination DB (new)
}
```

### Pattern 4: Tool Parameter Structs
**What:** Each tool gets a dedicated parameter struct with `#[derive(Debug, Deserialize, schemars::JsonSchema)]` and `#[schemars(description = "...")]` on each field.
**When to use:** All 11 new tools.

### Anti-Patterns to Avoid
- **Shared Mutex<CoordinationDb>:** Don't wrap a single DB in Arc<Mutex<>>. The open-per-call pattern is established and avoids lock contention across tools.
- **Async DB operations:** Don't make CoordinationDb async. It's synchronous by design; wrap in spawn_blocking.
- **Tool-level heartbeat middleware:** Don't try to create wrapper/middleware for implicit heartbeat. Just add `db.heartbeat()` where needed within each handler's spawn_blocking closure.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| MCP tool registration | Manual JSON-RPC dispatch | rmcp `#[tool]` + `#[tool_router]` macros | Handles schema generation, routing, error formatting |
| Parameter validation | Manual JSON parsing | schemars + rmcp `Parameters<T>` wrapper | Auto-generates JSON Schema, validates types |
| Coordination DB operations | New SQL in MCP layer | `glass_coordination::CoordinationDb` methods | All SQL is in Phase 31 crate, MCP layer is pure delegation |
| Path canonicalization | Manual dunce calls in MCP tools | Let `CoordinationDb` methods handle it | lock_files, unlock_file already canonicalize internally |
| Heartbeat management | Custom keepalive system | Built into CoordinationDb transactions | Most write operations already refresh heartbeat |

**Key insight:** The MCP tools layer should be a thin async wrapper around synchronous `CoordinationDb` calls. Zero business logic in the MCP layer -- just parameter translation, spawn_blocking, and JSON response formatting.

## Common Pitfalls

### Pitfall 1: Forgetting to Handle LockResult Variants
**What goes wrong:** Returning LockResult::Conflict as a server error instead of a success response with conflict details.
**Why it happens:** Conflicts are not errors -- they're expected coordination responses.
**How to avoid:** Map `LockResult::Acquired(paths)` to success JSON with locked paths; map `LockResult::Conflict(conflicts)` to success JSON with conflict details (holder identity, reason, retry hint).
**Warning signs:** Using `?` to propagate LockResult or converting Conflict to McpError.

### Pitfall 2: Missing Heartbeat in Unlock
**What goes wrong:** Agent that only does unlock operations (never lock, send, etc.) appears stale and gets pruned.
**Why it happens:** `unlock_file()` and `unlock_all()` are simple DELETE statements that don't update heartbeat.
**How to avoid:** Add explicit `db.heartbeat(agent_id)` call in the MCP unlock handler.
**Warning signs:** Active agent that only unlocks files gets pruned unexpectedly.

### Pitfall 3: Wrong Error Type in spawn_blocking
**What goes wrong:** Compile error when trying to use `?` with anyhow::Error inside spawn_blocking that expects McpError.
**Why it happens:** `spawn_blocking` closure returns `Result<T, McpError>` but coordination DB methods return `Result<T, anyhow::Error>`.
**How to avoid:** Use `.map_err(internal_err)?` on every DB call, matching the existing pattern.
**Warning signs:** Type mismatch errors between anyhow::Error and McpError.

### Pitfall 4: Path Parameter Types
**What goes wrong:** MCP tool receives paths as `Vec<String>` but `lock_files` expects `&[PathBuf]`.
**Why it happens:** JSON params are strings; Rust API expects PathBuf.
**How to avoid:** Convert `params.paths.iter().map(PathBuf::from).collect::<Vec<_>>()` inside spawn_blocking.
**Warning signs:** Type mismatch on path parameters.

### Pitfall 5: Forgetting to Update ServerInfo Instructions
**What goes wrong:** AI agents don't know about new coordination tools because ServerInfo description only mentions history/undo tools.
**Why it happens:** ServerInfo `with_instructions()` text is hardcoded and not updated.
**How to avoid:** Update the instructions string to mention all coordination tools.
**Warning signs:** AI agents only discover tools via schema listing, not instructions.

### Pitfall 6: Project Parameter Inconsistency
**What goes wrong:** Agent registers with project "C:\Users\foo\project" but list_agents is called with "./project".
**Why it happens:** Different tools receive project path in different formats.
**How to avoid:** CoordinationDb methods already canonicalize project paths internally via `canonicalize_path()`. The MCP layer should pass through whatever the caller provides.
**Warning signs:** list_agents returns empty when agents are registered.

## Code Examples

### Tool Parameter Struct Pattern
```rust
// Source: existing tools.rs pattern
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RegisterParams {
    /// Human-readable agent name (e.g. "claude-code-1").
    #[schemars(description = "Human-readable agent name")]
    pub name: String,
    /// Agent type (e.g. "claude-code", "cursor", "human").
    #[schemars(description = "Agent type (e.g. 'claude-code', 'cursor', 'human')")]
    pub agent_type: String,
    /// Project root path (used to scope agent visibility and locks).
    #[schemars(description = "Project root path for scoping")]
    pub project: String,
    /// Current working directory.
    #[schemars(description = "Current working directory")]
    pub cwd: String,
    /// OS process ID (optional, for liveness fallback).
    #[schemars(description = "OS process ID for liveness detection")]
    pub pid: Option<u32>,
}
```

### Lock Tool with Conflict Handling
```rust
// Source: design doc + existing pattern
#[tool(description = "Atomically claim advisory file locks. Returns conflicts if any file is held by another agent.")]
async fn glass_agent_lock(
    &self,
    Parameters(params): Parameters<LockParams>,
) -> Result<CallToolResult, McpError> {
    let coord_db_path = self.coord_db_path.clone();

    let result = tokio::task::spawn_blocking(move || {
        let mut db = glass_coordination::CoordinationDb::open(&coord_db_path)
            .map_err(internal_err)?;
        let paths: Vec<std::path::PathBuf> = params.paths.iter().map(PathBuf::from).collect();
        db.lock_files(&params.agent_id, &paths, params.reason.as_deref())
            .map_err(internal_err)
    })
    .await
    .map_err(internal_err)??;

    let response = match result {
        glass_coordination::LockResult::Acquired(paths) => {
            serde_json::json!({
                "locked": paths,
                "conflicts": [],
            })
        }
        glass_coordination::LockResult::Conflict(conflicts) => {
            let conflict_details: Vec<serde_json::Value> = conflicts.iter().map(|c| {
                serde_json::json!({
                    "path": c.path,
                    "held_by": c.held_by_agent_name,
                    "held_by_id": c.held_by_agent_id,
                    "reason": c.reason,
                    "retry_hint": "Wait and retry, or send a 'request_unlock' message to the holder",
                })
            }).collect();
            serde_json::json!({
                "locked": [],
                "conflicts": conflict_details,
            })
        }
    };

    let content = Content::json(&response)?;
    Ok(CallToolResult::success(vec![content]))
}
```

### Unlock Tool with Implicit Heartbeat (MCP-12)
```rust
#[tool(description = "Release file locks. Omit paths to release all locks.")]
async fn glass_agent_unlock(
    &self,
    Parameters(params): Parameters<UnlockParams>,
) -> Result<CallToolResult, McpError> {
    let coord_db_path = self.coord_db_path.clone();

    let result = tokio::task::spawn_blocking(move || {
        let mut db = glass_coordination::CoordinationDb::open(&coord_db_path)
            .map_err(internal_err)?;

        let released = if let Some(paths) = &params.paths {
            let mut count = 0u64;
            for p in paths {
                if db.unlock_file(&params.agent_id, &PathBuf::from(p))
                    .map_err(internal_err)? {
                    count += 1;
                }
            }
            count
        } else {
            db.unlock_all(&params.agent_id).map_err(internal_err)?
        };

        // MCP-12: Implicit heartbeat (unlock_file/unlock_all don't refresh internally)
        db.heartbeat(&params.agent_id).map_err(internal_err)?;

        Ok::<_, McpError>(serde_json::json!({
            "released": released,
        }))
    })
    .await
    .map_err(internal_err)??;

    let content = Content::json(&result)?;
    Ok(CallToolResult::success(vec![content]))
}
```

### Updated GlassServer Constructor
```rust
pub fn new(db_path: PathBuf, glass_dir: PathBuf, coord_db_path: PathBuf) -> Self {
    Self {
        tool_router: Self::tool_router(),
        db_path,
        glass_dir,
        coord_db_path,
    }
}
```

### Updated run_mcp_server in lib.rs
```rust
pub async fn run_mcp_server() -> anyhow::Result<()> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let db_path = glass_history::resolve_db_path(&cwd);
    let glass_dir = glass_snapshot::resolve_glass_dir(&cwd);
    let coord_db_path = glass_coordination::resolve_db_path();

    tracing::info!(
        "MCP server starting, db_path={}, glass_dir={}, coord_db_path={}",
        db_path.display(),
        glass_dir.display(),
        coord_db_path.display(),
    );

    let server = tools::GlassServer::new(db_path, glass_dir, coord_db_path);
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
```

## Complete Tool Inventory

All 11 tools with their parameter structs and DB method mappings:

| # | Tool Name | Params Struct | Key Fields | DB Method(s) | Returns |
|---|-----------|---------------|------------|--------------|---------|
| 1 | `glass_agent_register` | `RegisterParams` | name, agent_type, project, cwd, pid? | `register()` + `list_agents()` | agent_id, agents_active |
| 2 | `glass_agent_deregister` | `DeregisterParams` | agent_id | `deregister()` | ok: bool |
| 3 | `glass_agent_list` | `ListAgentsParams` | project | `prune_stale()` + `list_agents()` | agents: Vec |
| 4 | `glass_agent_status` | `StatusParams` | agent_id, status, task? | `update_status()` | ok: bool |
| 5 | `glass_agent_lock` | `LockParams` | agent_id, paths, reason? | `lock_files()` | locked/conflicts |
| 6 | `glass_agent_unlock` | `UnlockParams` | agent_id, paths? | `unlock_file()`/`unlock_all()` + `heartbeat()` | released: count |
| 7 | `glass_agent_locks` | `ListLocksParams` | project? | `list_locks()` | locks: Vec |
| 8 | `glass_agent_broadcast` | `BroadcastParams` | agent_id, project, msg_type, content | `broadcast()` | delivered_to: count |
| 9 | `glass_agent_send` | `SendParams` | agent_id, to_agent, msg_type, content | `send_message()` | message_id |
| 10 | `glass_agent_messages` | `MessagesParams` | agent_id | `read_messages()` | messages: Vec |
| 11 | `glass_agent_heartbeat` | `HeartbeatParams` | agent_id | `heartbeat()` | ok: bool |

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| rmcp 0.x manual tool dispatch | rmcp 1.x macro-based #[tool] routing | rmcp 1.0 release | Zero-boilerplate tool registration |
| Shared CoordinationDb with Mutex | Open-per-call pattern | Phase 31 design decision | Eliminates lock contention, matches SQLite WAL concurrent access model |

**Deprecated/outdated:**
- None -- all project dependencies are current

## Open Questions

1. **Stale timeout value for prune_stale in list_agents**
   - What we know: Design doc says 5 minutes (300s), Phase 31 code accepts `timeout_secs` parameter
   - What's unclear: What value should the MCP list_agents handler pass?
   - Recommendation: Use 600 seconds (10 minutes) matching the success criteria "10min timeout" from Phase 31

2. **list_agents auto-pruning scope**
   - What we know: Design doc says "Auto-prunes stale entries" but current `prune_stale` is global (prunes all stale agents, not just in the project)
   - What's unclear: Is global pruning on list_agents acceptable?
   - Recommendation: Yes -- stale agents should be pruned regardless of project. This is the conservative/safe approach.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in Rust test framework) |
| Config file | None needed (standard Cargo workspace) |
| Quick run command | `cargo test --package glass_mcp` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements --> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| MCP-01 | register returns agent_id + count | unit | `cargo test --package glass_mcp -- glass_agent_register` | Wave 0 |
| MCP-02 | deregister cleans up | unit | `cargo test --package glass_mcp -- deregister` | Wave 0 |
| MCP-03 | list_agents with pruning | unit | `cargo test --package glass_mcp -- agent_list` | Wave 0 |
| MCP-04 | status update | unit | `cargo test --package glass_mcp -- agent_status` | Wave 0 |
| MCP-05 | lock acquisition + conflict | unit | `cargo test --package glass_mcp -- agent_lock` | Wave 0 |
| MCP-06 | unlock specific + all | unit | `cargo test --package glass_mcp -- agent_unlock` | Wave 0 |
| MCP-07 | list locks | unit | `cargo test --package glass_mcp -- agent_locks` | Wave 0 |
| MCP-08 | broadcast message | unit | `cargo test --package glass_mcp -- agent_broadcast` | Wave 0 |
| MCP-09 | send directed message | unit | `cargo test --package glass_mcp -- agent_send` | Wave 0 |
| MCP-10 | read messages | unit | `cargo test --package glass_mcp -- agent_messages` | Wave 0 |
| MCP-11 | heartbeat refresh | unit | `cargo test --package glass_mcp -- agent_heartbeat` | Wave 0 |
| MCP-12 | implicit heartbeat on all calls | unit | `cargo test --package glass_mcp -- implicit_heartbeat` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test --package glass_mcp`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before verification

### Wave 0 Gaps
Note: Tests for the MCP tool handlers should be synchronous unit tests validating parameter deserialization and response JSON structure, matching the existing test pattern in tools.rs. Full end-to-end MCP protocol tests are Phase 33 (integration testing). The MCP handler tests should focus on:
- Parameter struct deserialization from JSON
- Response JSON structure validation
- Direct DB operation verification (open tempfile DB, call handler logic, check DB state)

Since existing tests are purely synchronous (no tokio runtime), the new tests should follow the same pattern -- test parameter deserialization and DB operations directly rather than going through the async MCP protocol layer.

- [ ] `RegisterParams` deserialization test
- [ ] `DeregisterParams` deserialization test
- [ ] `LockParams` deserialization with paths array
- [ ] `UnlockParams` deserialization with optional paths
- [ ] `BroadcastParams` deserialization
- [ ] `SendParams` deserialization
- [ ] `MessagesParams` deserialization
- [ ] `HeartbeatParams` deserialization
- [ ] `StatusParams` deserialization
- [ ] `ListAgentsParams` deserialization
- [ ] `ListLocksParams` deserialization with optional project

## Sources

### Primary (HIGH confidence)
- Existing `crates/glass_mcp/src/tools.rs` -- 5 working MCP tool handlers with established pattern
- Existing `crates/glass_coordination/src/db.rs` -- complete DB API (35 passing tests)
- `AGENT_COORDINATION_DESIGN.md` -- MCP tool specifications with input/output schemas
- `.planning/STATE.md` -- project decisions (open-per-call, BEGIN IMMEDIATE, etc.)
- `crates/glass_mcp/Cargo.toml` -- rmcp 1.1.0 with server + transport-io features

### Secondary (MEDIUM confidence)
- rmcp crate documentation (docs.rs/rmcp) -- macro patterns verified against existing code

### Tertiary (LOW confidence)
- None -- all findings verified against existing codebase

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all libraries already in use, no new dependencies except path dep
- Architecture: HIGH -- follows established pattern exactly (5 existing tools as template)
- Pitfalls: HIGH -- identified from code review of existing tools and coordination DB API

**Research date:** 2026-03-09
**Valid until:** 2026-04-09 (stable -- no moving parts, all deps pinned)

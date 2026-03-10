# Phase 33: Integration and Testing - Research

**Researched:** 2026-03-09
**Domain:** Rust integration testing, CLAUDE.md documentation, SQLite cross-process coordination
**Confidence:** HIGH

## Summary

Phase 33 has three deliverables: (1) add a coordination protocol section to CLAUDE.md so AI agents know how to use the MCP tools, (2) write an integration test that spawns two CoordinationDb instances against the same SQLite file to validate cross-process registration, lock conflicts, and messaging, and (3) write a focused lock conflict detection test. All three build on the completed Phase 31 (glass_coordination crate) and Phase 32 (MCP tools).

The coordination infrastructure is fully built. glass_coordination has 35 unit tests covering all APIs. glass_mcp has 33 tests covering parameter deserialization. What is missing is: (a) documentation telling AI agents how to use the tools, (b) integration tests proving two independent database connections can coordinate correctly through the shared SQLite WAL-mode database.

**Primary recommendation:** Write integration tests at the glass_coordination crate level using two separate `CoordinationDb::open()` calls to the same tempfile DB path. This tests the real cross-process scenario (separate connections, WAL mode concurrency) without needing to spawn actual MCP server processes, which would require stdio transport setup that adds complexity without testing the coordination logic.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| INTG-01 | CLAUDE.md includes coordination protocol instructions for AI agents | Design doc has exact text template in "CLAUDE.md Integration" section; append to existing CLAUDE.md |
| INTG-02 | Multi-server integration test validates two MCP instances coordinating via shared DB | Two CoordinationDb connections to same tempfile DB validates agent registration, lock conflict detection, and message exchange across "processes" |
| INTG-03 | Integration test validates lock conflict detection across agents | Subset of INTG-02; agent A locks file X, agent B's lock request returns Conflict identifying agent A |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| glass_coordination | 0.1.0 | CoordinationDb with register/lock/message APIs | Already built in Phase 31 |
| rusqlite | workspace (0.38) | SQLite with WAL mode | Already in workspace |
| tempfile | 3 | Temp directories for test databases | Already a dev-dependency |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| N/A | - | No new dependencies needed | - |

## Architecture Patterns

### CLAUDE.md Coordination Section

The design doc (AGENT_COORDINATION_DESIGN.md) already specifies the exact text for CLAUDE.md. The section should be appended after the existing "Conventions" section. Key instructions to include:

1. Register on session start with `glass_agent_register`
2. Lock files before editing with `glass_agent_lock`
3. Unlock files when done with `glass_agent_unlock`
4. Update status with `glass_agent_status`
5. Check messages periodically with `glass_agent_messages`
6. Handle lock conflicts with `glass_agent_send` (msg_type: request_unlock)
7. Deregister on session end with `glass_agent_deregister`

The CLAUDE.md section must also mention that the architecture section should list `glass_coordination` as a crate.

### Integration Test Pattern: Two Connections, One DB

```rust
// Source: glass_coordination/src/db.rs test_db() pattern
fn shared_test_db() -> (CoordinationDb, CoordinationDb, TempDir) {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test-agents.db");
    let db1 = CoordinationDb::open(&db_path).unwrap();
    let db2 = CoordinationDb::open(&db_path).unwrap();
    (db1, db2, dir)
}
```

This pattern opens two independent SQLite connections to the same file, simulating what happens when two MCP server processes open `~/.glass/agents.db`. SQLite WAL mode (already configured in `CoordinationDb::open`) supports concurrent readers and serialized writers via `PRAGMA busy_timeout = 5000`.

### Test Location

Tests should live in `crates/glass_coordination/src/db.rs` within the existing `#[cfg(test)] mod tests` block, following project convention of co-located tests. The integration tests are functionally "two connection" tests rather than multi-binary tests, so they belong alongside the existing unit tests.

### Anti-Patterns to Avoid
- **Spawning actual MCP server processes:** The MCP server uses stdio transport. Setting up two child processes with JSON-RPC over stdin/stdout for an integration test is fragile and tests transport, not coordination. The real coordination happens at the CoordinationDb level.
- **Using a single CoordinationDb instance:** The existing unit tests all use a single connection. The integration value is proving that two separate connections see each other's changes through SQLite WAL.
- **Testing with in-memory SQLite:** `:memory:` databases cannot be shared between connections. Must use a real file in a tempdir.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Test DB setup | Custom DB initialization | Existing `test_db()` pattern, extended for two connections | Already proven in 35 existing tests |
| Temp file paths | Manual temp path management | `tempfile::TempDir` (RAII cleanup) | Already a dev-dependency |
| Path canonicalization in tests | Manual path strings | Create real files in TempDir + let lock_files canonicalize | Matches how production code works |

## Common Pitfalls

### Pitfall 1: SQLite Locking Between Connections
**What goes wrong:** Two connections writing simultaneously can get SQLITE_BUSY
**Why it happens:** WAL mode allows concurrent reads but serializes writes
**How to avoid:** Already handled -- `PRAGMA busy_timeout = 5000` in CoordinationDb::open gives 5 second retry window. `BEGIN IMMEDIATE` prevents write starvation.
**Warning signs:** Intermittent test failures on CI

### Pitfall 2: File Canonicalization in Tests
**What goes wrong:** Lock tests fail because paths don't match
**Why it happens:** `lock_files` canonicalizes paths via `dunce::canonicalize`, which requires the file to exist on disk
**How to avoid:** Always create real files with `std::fs::write()` in the TempDir before locking. The existing unit tests already do this correctly.
**Warning signs:** "No such file or directory" errors from canonicalize

### Pitfall 3: TempDir Drop Order
**What goes wrong:** TempDir gets dropped before DB connections are closed, causing errors on Windows
**Why it happens:** Rust drops struct fields in declaration order; if TempDir is dropped first, the DB file is deleted while connections are open
**How to avoid:** Return TempDir last in the tuple (it's the `_dir` convention used in existing tests). The caller holds it in scope.
**Warning signs:** "The process cannot access the file" errors on Windows CI

### Pitfall 4: CLAUDE.md Formatting
**What goes wrong:** The new section doesn't follow existing CLAUDE.md conventions
**Why it happens:** Copy-pasting from design doc without adapting to existing file style
**How to avoid:** Read the existing CLAUDE.md structure and match its heading level, bullet style, and section naming.

## Code Examples

### Integration Test: Two Agents Coordinating via Shared DB
```rust
// Two separate connections simulating two MCP server processes
#[test]
fn test_cross_connection_registration_and_locks() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("shared-agents.db");

    // "MCP Server 1" opens its connection
    let mut db1 = CoordinationDb::open(&db_path).unwrap();
    // "MCP Server 2" opens its own connection
    let mut db2 = CoordinationDb::open(&db_path).unwrap();

    // Agent A registers via server 1
    let id_a = db1.register("Agent-A", "claude-code", "project", "/tmp", None).unwrap();

    // Agent B registers via server 2
    let id_b = db2.register("Agent-B", "claude-code", "project", "/tmp", None).unwrap();

    // Both agents visible from either connection
    let agents_from_1 = db1.list_agents("project").unwrap();
    let agents_from_2 = db2.list_agents("project").unwrap();
    assert_eq!(agents_from_1.len(), 2);
    assert_eq!(agents_from_2.len(), 2);

    // Create a real file for locking
    let file = dir.path().join("contested.rs");
    std::fs::write(&file, "").unwrap();

    // Agent A locks the file via server 1
    let result = db1.lock_files(&id_a, &[file.clone()], Some("editing")).unwrap();
    assert!(matches!(result, LockResult::Acquired(_)));

    // Agent B tries to lock same file via server 2 -- CONFLICT
    let result = db2.lock_files(&id_b, &[file], Some("also want it")).unwrap();
    match result {
        LockResult::Conflict(conflicts) => {
            assert_eq!(conflicts.len(), 1);
            assert_eq!(conflicts[0].held_by_agent_id, id_a);
            assert_eq!(conflicts[0].held_by_agent_name, "Agent-A");
        }
        _ => panic!("Expected conflict"),
    }
}
```

### Integration Test: Cross-Connection Message Exchange
```rust
#[test]
fn test_cross_connection_messaging() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("shared-agents.db");

    let mut db1 = CoordinationDb::open(&db_path).unwrap();
    let mut db2 = CoordinationDb::open(&db_path).unwrap();

    let id_a = db1.register("Agent-A", "claude-code", "project", "/tmp", None).unwrap();
    let id_b = db2.register("Agent-B", "claude-code", "project", "/tmp", None).unwrap();

    // Agent A sends message via connection 1
    db1.send_message(&id_a, &id_b, "info", "starting refactor").unwrap();

    // Agent B reads message via connection 2
    let msgs = db2.read_messages(&id_b).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].content, "starting refactor");
    assert_eq!(msgs[0].from_name.as_deref(), Some("Agent-A"));
}
```

### CLAUDE.md Section Template
```markdown
## Multi-Agent Coordination

Glass provides agent coordination through MCP tools when multiple AI agents work on the same project. Follow this protocol:

- **On session start:** Call `glass_agent_register` with your name, type, and project root
- **Before editing files:** Call `glass_agent_lock` to claim advisory locks (atomic -- returns conflicts if held by another agent)
- **After editing files:** Call `glass_agent_unlock` to release locks
- **Periodically:** Call `glass_agent_messages` to check for messages from other agents
- **On lock conflict:** Use `glass_agent_send` with msg_type `request_unlock` to ask the holder to release
- **When changing tasks:** Call `glass_agent_status` to update your status and task description
- **On session end:** Call `glass_agent_deregister` to clean up

All coordination data lives in `~/.glass/agents.db` (shared SQLite). Agents are scoped by project root -- agents on different projects don't see each other.
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Single-connection unit tests | Multi-connection integration tests | Phase 33 | Proves WAL-mode concurrent access works |
| No AI agent instructions | CLAUDE.md coordination protocol | Phase 33 | Agents auto-coordinate when using Glass MCP |

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in Rust test harness) |
| Config file | Cargo.toml (workspace) |
| Quick run command | `cargo test -p glass_coordination` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| INTG-01 | CLAUDE.md contains coordination protocol section | manual | Visual inspection of CLAUDE.md | N/A (doc change) |
| INTG-02 | Two DB connections: registration + lock conflict + messages | integration | `cargo test -p glass_coordination test_cross_connection` | Wave 0 |
| INTG-03 | Lock conflict detection across connections | integration | `cargo test -p glass_coordination test_cross_connection_lock_conflict` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_coordination`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before verify

### Wave 0 Gaps
- [ ] Cross-connection integration tests in `crates/glass_coordination/src/db.rs` -- covers INTG-02, INTG-03

## Open Questions

None. This phase is straightforward -- all infrastructure exists, we just need documentation and cross-connection tests.

## Sources

### Primary (HIGH confidence)
- `crates/glass_coordination/src/db.rs` - Full implementation + 35 existing unit tests reviewed
- `crates/glass_coordination/src/lib.rs` - Public API and canonicalization
- `crates/glass_mcp/src/tools.rs` - MCP tool implementations (spawn_blocking + open-per-call pattern)
- `AGENT_COORDINATION_DESIGN.md` - Design document with CLAUDE.md template
- `CLAUDE.md` - Current state, needs coordination section appended

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - no new dependencies, all code already exists
- Architecture: HIGH - extending existing test patterns with two-connection variant
- Pitfalls: HIGH - identified from reading existing test code and SQLite WAL behavior

**Research date:** 2026-03-09
**Valid until:** 2026-04-09 (stable -- no external dependencies changing)

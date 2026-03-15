# Testing Patterns

**Analysis Date:** 2026-03-15

## Test Framework

**Runner:**
- Built-in Rust test harness (no external test runner)
- Compile: `cargo test --workspace --no-run`
- Execute: `cargo test --workspace --no-fail-fast` or `cargo test --lib` (unit tests only)

**Assertion Library:**
- Standard Rust `assert!`, `assert_eq!`, `assert_ne!` macros
- No external assertion crate dependency

**Run Commands:**
```bash
cargo test --workspace              # Run all tests (unit + integration + doc tests)
cargo test --workspace --lib        # Unit tests only
cargo test --workspace --test       # Integration tests only
cargo test --workspace --doc        # Doc tests only
cargo test --workspace --no-fail-fast  # Continue on first failure
cargo test -- --nocapture          # Show println! output during tests
cargo test -- --ignored             # Run ignored tests only
```

**Test Count:**
- Total: 118 tests across all crates (as of 2026-03-15)
- Main binary tests: 57 tests
- Remaining: distributed across 11 crates
- Doc tests: 1 (in `glass_soi`)
- Status: all passing (0 failures)

## Test File Organization

**Location:**
- Inline with source code, NOT in separate `tests/` directory
- Pattern: `#[cfg(test)] mod tests { ... }` at bottom of each `.rs` file
- Exception: `tests/mcp_integration.rs` for MCP integration testing (at workspace root)

**Naming:**
- Test modules named `tests` (convention): `#[cfg(test)] mod tests`
- Test functions use test name as description: `test_register()`, `test_deregister()`
- Descriptive names follow pattern: `test_{subject}_{condition}_{expected}`
  - Example: `test_resolve_db_path_ancestor()` — tests path resolution when .glass/ is in ancestor
  - Example: `test_deregister_cascades_locks()` — tests cascading delete behavior

**Structure:**
```
crates/
  glass_coordination/src/
    db.rs                         # Public API
    └── #[cfg(test)] mod tests    # Tests inline
         └── fn test_register()
         └── fn test_deregister()
  glass_history/src/
    lib.rs                        # Barrel file + basic tests
    db.rs                         # Database implementation + tests
```

## Test Structure

**Suite Organization** (from `crates/glass_coordination/src/db.rs`):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_db() -> (CoordinationDb, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test-agents.db");
        let db = CoordinationDb::open(&db_path).unwrap();
        (db, dir)
    }

    #[test]
    fn test_register() {
        let (mut db, _dir) = test_db();
        // Test body
    }
}
```

**Patterns:**

1. **Setup Helper** (test_db pattern):
   - Private function returning `(Instance, TempDir)` tuple
   - TempDir held in tuple so cleanup happens on test end
   - Used by `#[test]` functions: `let (mut db, _dir) = test_db()`

2. **Assertion Structure**:
   ```rust
   // Arrange
   let (mut db, _dir) = test_db();

   // Act
   let id = db.register("agent-1", "claude-code", ".", "/tmp", None).unwrap();

   // Assert
   assert_eq!(id.len(), 36);
   let agents = db.list_agents(".").unwrap();
   assert_eq!(agents.len(), 1);
   ```

3. **Comments in Tests**:
   - Inline comments explain non-obvious test logic
   - Example (from `crates/glass_agent/src/session_db.rs` line 283):
   ```rust
   assert_eq!(
       loaded.id, "id-new",
       "should return the record with highest created_at"
   );
   ```

## Mocking

**Framework:** `tempfile` crate for temporary filesystem/database mocking

**Patterns:**
- Database tests use `TempDir::new().unwrap()` to create isolated test databases
- No explicit mocking library (mockall, etc.) — manual setup/assertion approach
- Database state verified via direct query after operations

**Example Mock Database** (from `crates/glass_coordination/src/db.rs` lines 835-860):
```rust
fn test_db() -> (CoordinationDb, TempDir) {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test-agents.db");
    let db = CoordinationDb::open(&db_path).unwrap();
    (db, dir)
}

#[test]
fn test_register() {
    let (mut db, _dir) = test_db();
    let id = db.register("agent-1", "claude-code", ".", "/tmp", Some(1234)).unwrap();

    // Verify via query
    let agents = db.list_agents(".").unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].id, id);
}
```

**What to Mock:**
- Temporary filesystems: `tempfile::TempDir`
- Temporary databases: in-memory or temp file SQLite via `TempDir`
- SQL state: verified via direct query assertions

**What NOT to Mock:**
- Actual database open/migration logic — use real temp database
- SQL operations — execute real queries to verify behavior
- Platform-specific code — use conditional compilation, don't mock

## Fixtures and Factories

**Test Data** (from `crates/glass_agent/src/session_db.rs`):
```rust
fn make_record(
    id: &str,
    root: &str,
    session_id: &str,
    prev: Option<&str>,
    created_at: i64,
) -> AgentSessionRecord {
    AgentSessionRecord {
        id: id.to_string(),
        project_root: root.to_string(),
        session_id: session_id.to_string(),
        handoff: HandoffData {
            work_completed: "Implemented feature X".to_string(),
            work_remaining: "Write tests for Y".to_string(),
            key_decisions: "Used approach Z".to_string(),
            previous_session_id: prev.map(|s| s.to_string()),
        },
        raw_handoff: r#"{"work_completed":"Implemented feature X"..."#.to_string(),
        created_at,
    }
}
```

**Location:**
- Factory functions defined in same test module: `fn make_record(...)`
- Helper functions defined at module level: `fn test_db() -> (Instance, TempDir)`
- Inline data construction acceptable for simple cases: `TempDir::new().unwrap()`

## Coverage

**Requirements:**
- No enforced coverage target in CI
- All public APIs expected to have unit tests
- Core logic (database, state machines, parsers) have high coverage (>90%)
- Rendering and GUI code coverage lower (integration tested via orchestrator)

**View Coverage:**
```bash
# Install tarpaulin
cargo install cargo-tarpaulin

# Generate coverage report
cargo tarpaulin --workspace --out Html --output-dir coverage/

# View report
open coverage/index.html
```

**Coverage Gaps:**
- Orchestrator integration with Glass agent (tested via e2e)
- Windows-specific PTY code (gated with `#[cfg(target_os = "windows")]`)
- Rendering framebuffer logic (visual regression tested manually)

## Test Types

**Unit Tests:**
- Scope: single function or small group of functions
- Approach: pure function testing (no side effects)
- Location: inline `#[cfg(test)]` module in source file
- Assertions: exact behavior validation
- Examples:
  - `crates/glass_history/src/lib.rs` — path resolution unit tests
  - `crates/glass_terminal/src/osc_scanner.rs` — OSC sequence parsing tests

**Integration Tests:**
- Scope: multi-component interaction (database + parser, pty + terminal)
- Approach: real resources (temp databases, actual file I/O)
- Location: `tests/mcp_integration.rs` (at workspace root)
- Assertions: end-to-end behavior
- Examples:
  - Session handoff roundtrip: insert → load → verify state persists
  - Worktree lifecycle: create → diff → apply → delete

**E2E Tests:**
- NOT automated in CI (too complex for headless)
- Manual: orchestrator spawns Glass subprocess, verifies agent loop
- Scope: full system from Glass UI through agent coordination layer
- Testing approach: crash log inspection, manual verification

## Common Patterns

**Async Testing:**
- No async test attribute (`#[tokio::test]`) used in current codebase
- Async code tested via `block_on()` in regular synchronous tests
- Pattern: call `.unwrap()` on blocking operations, no spawning within tests

**Error Testing:**
```rust
// From crates/glass_agent/src/worktree_manager.rs
#[test]
fn test_dismiss_removes_pending() {
    let (mut manager, _dir) = test_manager();
    let pending = manager.create_pending(...).unwrap();
    let handle = pending.handle();

    // Act: dismiss should succeed
    manager.dismiss(&handle).unwrap();

    // Assert: handle no longer valid
    let err = manager.apply(&handle, root_changes).unwrap_err();
    assert!(err.to_string().contains("not found"));
}
```

**Platform-Specific Testing:**
- Conditional compilation: `#[cfg(target_os = "windows")]` on test functions
- Example: ConPTY-specific escape sequence tests gated for Windows only
- Pattern: run same test logic on platform where feature is available

**Resource Cleanup:**
- Automatic via `TempDir` drop when test function returns
- No explicit cleanup needed for database — temp file deleted on drop
- Pattern: `let (db, _dir) = test_db()` — underscore signals "not used in test body"

## Test Conventions

**Naming Style:**
- Snake case function names: `test_register()`, `test_deregister_cascades_locks()`
- Descriptive predicate in name: `test_{action}_{condition}_{result}`
  - `test_load_prior_handoff_returns_most_recent_by_created_at()`
  - `test_session_record_survives_connection_close_and_reopen()`

**Assertion Messages:**
- Use assert_eq! third parameter for context: `assert_eq!(loaded.id, "id-new", "should return the record with highest created_at")`
- Keep messages short and specific to failure context

**Test Isolation:**
- Each test creates fresh temp database via `test_db()` helper
- No shared state between tests
- Database files auto-deleted when `TempDir` drops at function exit

**Edge Cases:**
- Boundary conditions tested: empty collections, None values, default states
- Example (from `crates/glass_history/src/lib.rs` line 52):
```rust
#[test]
fn test_resolve_db_path_project() {
    // Tests when .glass/ dir exists (success path)
}

#[test]
fn test_resolve_db_path_ancestor() {
    // Tests when .glass/ is in ancestor directory
}

#[test]
fn test_resolve_db_path_global_fallback() {
    // Tests when .glass/ not found (fallback path)
}
```

---

*Testing analysis: 2026-03-15*

# Testing Patterns

**Analysis Date:** 2026-03-18

## Test Framework

**Runner:**
- `cargo test --workspace` (runs all tests across all crates)
- Rust built-in test harness (no external test runner)
- Unit tests: compile into main binary with `#[cfg(test)]`
- No separate test files (tests live inline with source)

**Run Commands:**
```bash
cargo test --workspace              # Run all tests
cargo test --workspace -- --nocapture   # Show println! output
cargo test crate_name               # Run tests in specific crate
cargo test function_name            # Run test by name
```

**CI:**
- GitHub Actions in `.github/workflows/ci.yml`
- Matrix: Linux (ubuntu-latest), macOS (aarch64), Windows (x86_64)
- Runs `cargo test --workspace` on all platforms
- Format check: `cargo fmt --all -- --check` (ubuntu)
- Lint check: `cargo clippy --workspace -- -D warnings` (windows)

## Test File Organization

**Location:**
- Co-located tests: Tests live in same file as implementation (pattern: `#[cfg(test)] mod tests { ... }`)
- Separate test files for integration tests: `crates/glass_terminal/src/tests.rs`, `src/tests.rs` (main binary)
- Example: `crates/glass_core/src/config.rs` line 548 has `#[cfg(test)] mod tests { ... }` with 100+ test cases

**Naming:**
- Test modules: `mod tests { ... }`
- Test functions: `#[test] fn test_*` prefix (e.g., `test_no_subcommand_is_none`, `test_register`)
- Test groups: Nested modules for related tests (e.g., within outer `mod tests`)

**Structure:**
```
crates/glass_core/src/
  config.rs           # Line 548: #[cfg(test)] mod tests
  ipc.rs             # Tests for IPC functionality
  updater.rs         # Tests for update checking
  error.rs           # (Error type only, minimal tests)

crates/glass_coordination/src/
  db.rs              # Coordination database tests
  event_log.rs       # Event logging tests
  lib.rs             # Registry tests
  pid.rs             # PID validation tests
```

## Test Structure

**Suite Organization:**

Tests are grouped by functionality within `mod tests`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // Helper function
    fn test_db() -> (CoordinationDb, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test-agents.db");
        let db = CoordinationDb::open(&db_path).unwrap();
        (db, dir)
    }

    // Test case
    #[test]
    fn test_register() {
        let (mut db, _dir) = test_db();
        let id = db.register("agent-1", "claude-code", ".", "/tmp", Some(1234)).unwrap();
        assert_eq!(id.len(), 36); // UUID v4 format
    }
}
```

**Patterns:**
- **Setup**: Helper functions (e.g., `test_db()`) create fixtures and return reusable state
- **Teardown**: Automatic (fixtures dropped at end of test scope)
- **Assertion**: Standard Rust assertions: `assert!()`, `assert_eq!()`, `assert_ne!()`

## Mocking

**Framework:** Not used (tests prefer real implementations or in-memory doubles)

**Approach:**
- **Fixtures**: Use `tempfile::TempDir` for ephemeral filesystem operations
- **In-memory storage**: Tests create real SQLite databases in temp directories
- **Test helpers**: Custom setup functions like `test_db()` that yield usable fixtures

**Patterns:**

Test helper pattern from `glass_coordination/src/db.rs`:
```rust
fn test_db() -> (CoordinationDb, TempDir) {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test-agents.db");
    let db = CoordinationDb::open(&db_path).unwrap();
    (db, dir)
}

#[test]
fn test_register() {
    let (mut db, _dir) = test_db();  // _dir keeps temp directory alive
    let id = db.register("agent-1", "claude-code", ".", "/tmp", Some(1234)).unwrap();
    assert_eq!(id.len(), 36);
}
```

**What to Mock:**
- Nothing: Tests use real implementations
- Network calls are avoided by testing at API level (not HTTP client)
- Example: IPC tests use localhost TCP connections, not actual file descriptors

**What NOT to Mock:**
- Database operations: Create real SQLite DBs
- Filesystem operations: Use `tempfile` crate for real directories
- State machines: Test state transitions with real structs

## Fixtures and Factories

**Test Data:**

Configuration fixture from `glass_core/src/config.rs`:
```rust
#[test]
fn load_validated_valid_toml_returns_ok() {
    let result = GlassConfig::load_validated("font_family = \"Cascadia\"\nfont_size = 16.0");
    assert!(result.is_ok());
    let config = result.unwrap();
    assert_eq!(config.font_family, "Cascadia");
    assert_eq!(config.font_size, 16.0);
}
```

CLI argument fixture from `src/tests.rs`:
```rust
#[test]
fn test_history_list_with_all_filters() {
    let cli = Cli::try_parse_from([
        "glass", "history", "list", "--exit", "1", "--after", "1h", "--cwd", "/project", "-n", "10",
    ])
    .unwrap();
    assert_eq!(cli.command, Some(Commands::History { action: Some(...) }));
}
```

Pipeline parsing fixture from `glass_pipes/src/parser.rs`:
```rust
#[test]
fn split_pipes_basic_multi_stage() {
    let result = split_pipes("cat file | grep foo | wc -l");
    assert_eq!(result, vec!["cat file", "grep foo", "wc -l"]);
}
```

**Location:**
- In-test data: Inline in test function
- Helper functions: Same `mod tests` block, before test functions
- Tempfile fixtures: Created in helper (e.g., `test_db()`)
- No external fixture files (no factory crates or data directories)

## Coverage

**Requirements:** No coverage enforcement in CI (not configured)

**Tools:** No coverage metrics configured in `Cargo.toml` or CI

**Approach:**
- Tests are written for critical paths and edge cases
- Database operations: 100+ tests across coordination, history, snapshot crates
- Error handling: Extensive tests for malformed input, validation failures
- State machines: Comprehensive block lifecycle tests

## Test Types

**Unit Tests:**
- Scope: Single function or struct method
- Approach: Direct function calls with simple inputs
- Example: `test_split_pipes_basic_multi_stage()` tests `split_pipes()` function
- Location: Inline in source file with `#[cfg(test)] mod tests`

**Integration Tests:**
- Scope: Multi-component workflows (e.g., database + schema + migrations)
- Approach: Create full database in temp directory, exercise multiple operations
- Example: `test_register()` calls `CoordinationDb::open()`, schema creation, insert, and verification
- Location: Separate files like `src/tests.rs`, `crates/glass_terminal/src/tests.rs`

**E2E Tests:**
- Not found: No end-to-end tests (no external service interaction tests)
- Architecture: Tests focus on library correctness, not full app workflows

## Common Patterns

**Async Testing:**

From `glass_core/src/ipc.rs`:
```rust
#[tokio::test]
async fn ipc_round_trip_over_tcp() {
    // Test body can use .await
}
```

Async pattern:
- Decorator: `#[tokio::test]` instead of `#[test]`
- Runtime: tokio runtime spawned automatically
- Async/await: Full async syntax available in test

**Error Testing:**

From `glass_core/src/config.rs`:
```rust
#[test]
fn load_validated_malformed_toml_returns_error() {
    let result = GlassConfig::load_validated("invalid {{{{");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(!err.message.is_empty());
}
```

Pattern:
- Check `is_err()` or `is_ok()`
- Unwrap and inspect error details
- Verify error fields (message, line, column, etc.)

**Option Testing:**

From `glass_snapshot/src/undo.rs` (implied by undo logic):
```rust
pub fn undo_latest(&self) -> Result<Option<UndoResult>> {
    let snapshot = match self.store.db().get_latest_parser_snapshot()? {
        Some(s) => s,
        None => return Ok(None),  // No snapshot — not an error
    };
    // ... restore ...
}
```

Test pattern (not explicitly shown but implied):
```rust
#[test]
fn undo_latest_returns_none_when_no_snapshot() {
    let engine = UndoEngine::new(&store);
    let result = engine.undo_latest().unwrap();
    assert!(result.is_none());
}
```

**Boolean Assertions:**

From `glass_core/src/config.rs`:
```rust
#[test]
fn font_changed_different_font_size() {
    let a = GlassConfig::default();
    let b = GlassConfig {
        font_size: 18.0,
        ..GlassConfig::default()
    };
    assert!(a.font_changed(&b));  // Positive assertion
}

#[test]
fn font_changed_same_font_different_shell() {
    let a = GlassConfig {
        shell: Some("bash".to_string()),
        ..GlassConfig::default()
    };
    let b = GlassConfig {
        shell: Some("zsh".to_string()),
        ..GlassConfig::default()
    };
    assert!(!a.font_changed(&b));  // Negative assertion
}
```

**Enum Matching:**

From `glass_errors/src/lib.rs`:
```rust
#[test]
fn extract_errors_windows_path() {
    let output = r"C:\Users\foo\main.rs:10:5: warning: unused";
    let errors = extract_errors(output, None);
    assert_eq!(errors[0].severity, Severity::Warning);
}
```

Pattern:
- Extract enum variant
- Compare with `assert_eq!()`
- Verify variant values

**String Comparisons:**

From `glass_pipes/src/parser.rs`:
```rust
#[test]
fn split_pipes_basic_multi_stage() {
    let result = split_pipes("cat file | grep foo | wc -l");
    assert_eq!(result, vec!["cat file", "grep foo", "wc -l"]);
}
```

## Command Parsing Tests

Tests for CLI argument parsing use `clap` derive:

From `src/tests.rs`:
```rust
#[test]
fn test_history_search_with_limit() {
    let cli = Cli::try_parse_from([
        "glass", "history", "search", "deploy", "--limit", "5"
    ]).unwrap();
    assert_eq!(cli.command, Some(Commands::History { action: Some(...) }));
}
```

Pattern:
- Use `Cli::try_parse_from()` to simulate command-line args as array
- Verify parsed struct matches expected `Commands` enum variant
- Check nested action and filter fields

## Platform-Specific Tests

**Windows-Only Tests:**

Marked with `#[cfg(target_os = "windows")]`:
```rust
#[cfg(target_os = "windows")]
#[test]
fn conpty_specific_test() {
    // Test ConPTY behavior
}
```

Usage:
- ConPTY tests gated to Windows
- `escape_args` field in `PTY options` is Windows-only
- Tests skip on Unix platforms automatically in CI

## Test Statistics

**Scale:**
- ~114 files with `#[cfg(test)]` blocks across crates
- ~420 total tests (from MEMORY.md)
- Heavy coverage in: coordination, history, config, errors, pipes
- Light/no coverage in: rendering (GPU-specific), main event loop

---

*Testing analysis: 2026-03-18*

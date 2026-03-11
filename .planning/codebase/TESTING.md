# Testing Patterns

**Analysis Date:** 2026-03-08

## Test Framework

**Runner:**
- Rust built-in test framework (`#[test]`, `#[cfg(test)]`)
- No external test runner (no nextest)
- Criterion 0.5 for benchmarks

**Assertion Library:**
- Standard `assert!`, `assert_eq!`, `assert_ne!` macros
- Verbose assertion messages with format strings:
```rust
assert!(
    result.is_err(),
    "Unknown subcommand should produce a clap error"
);
assert_eq!(
    result.confidence,
    Confidence::ReadOnly,
    "Expected ReadOnly for '{cmd}'"
);
```

**Run Commands:**
```bash
cargo test --workspace         # Run all tests (~460 tests)
cargo test -p glass_history    # Run tests for a specific crate
cargo test test_name           # Run a specific test by name
cargo bench                    # Run Criterion benchmarks
```

## Test File Organization

**Location:**
- **Co-located** in the same file as production code using `#[cfg(test)] mod tests`
- This is the dominant pattern across all crates

**Exceptions:**
- `crates/glass_terminal/src/tests.rs` -- separate file for ConPTY integration tests (Windows-only)
- `src/tests.rs` -- separate file for binary CLI tests
- `tests/mcp_integration.rs` -- top-level integration test (spawns child process)

**Naming:**
- Test functions use `test_` prefix: `test_insert_and_retrieve`, `test_rm_single_file`
- Test modules use descriptive names: `subcommand_tests`, `codepage_tests`, `escape_seq_tests`

**Structure:**
```
crates/glass_core/src/config.rs        # 24 tests inline
crates/glass_core/src/event.rs         # 4 tests inline
crates/glass_history/src/db.rs         # 17 tests inline
crates/glass_history/src/lib.rs        # 3 tests inline
crates/glass_snapshot/src/command_parser.rs  # 20+ tests inline
crates/glass_pipes/src/parser.rs       # 16 tests inline
crates/glass_mux/src/types.rs          # 8 tests inline
crates/glass_mux/src/split_tree.rs     # tests inline
tests/mcp_integration.rs              # 3 integration tests
```

## Test Structure

**Suite Organization:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Optional: helper functions at the top of the test module
    fn test_db() -> (HistoryDb, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let db = HistoryDb::open(&db_path).unwrap();
        (db, dir)
    }

    fn sample_record(command: &str) -> CommandRecord {
        CommandRecord {
            id: None,
            command: command.to_string(),
            cwd: "/home/user".to_string(),
            // ...
        }
    }

    #[test]
    fn test_insert_and_retrieve() {
        let (db, _dir) = test_db();
        // arrange, act, assert
    }
}
```

**Patterns:**
- **Arrange-Act-Assert** structure (not explicitly labeled, but consistently followed)
- **Helper functions** defined at the top of test modules for setup: `test_db()`, `sample_record()`, `cwd()`
- **No setup/teardown hooks** -- each test creates its own state via helpers
- **TempDir for filesystem tests** -- `tempfile::TempDir` used for databases and file operations; automatically cleaned up on drop
- **Return the TempDir** from helpers to keep it alive for the test duration:
```rust
fn test_db() -> (HistoryDb, TempDir) {
    let dir = TempDir::new().unwrap();
    let db = HistoryDb::open(&dir.path().join("test.db")).unwrap();
    (db, dir)  // TempDir must outlive the db
}
```

## Mocking

**Framework:** No mocking framework. Tests use real implementations or hand-rolled test doubles.

**Patterns:**

1. **Inline test doubles** -- defined inside test functions:
```rust
#[test]
fn test_conpty_spawns_and_wakeup_fires() {
    #[derive(Clone)]
    struct TestListener {
        wakeup_received: Arc<AtomicBool>,
    }

    impl EventListener for TestListener {
        fn send_event(&self, event: alacritty_terminal::event::Event) {
            if matches!(event, alacritty_terminal::event::Event::Wakeup) {
                self.wakeup_received.store(true, Ordering::SeqCst);
            }
        }
    }
    // ...
}
```

2. **Real databases** -- tests create actual SQLite databases in temp directories:
```rust
let dir = TempDir::new().unwrap();
let db = HistoryDb::open(&dir.path().join("test.db")).unwrap();
```

3. **Process-based integration tests** -- spawn the actual binary and communicate via stdio:
```rust
struct McpTestClient {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    rx: mpsc::Receiver<String>,
}

impl McpTestClient {
    fn spawn(tmp_dir: &std::path::Path) -> Self {
        let glass_bin = env!("CARGO_BIN_EXE_glass");
        let mut child = Command::new(glass_bin)
            .args(["mcp", "serve"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("Failed to spawn glass mcp serve");
        // ...
    }
}
```

**What to Mock:**
- Event listeners (trait implementations for `alacritty_terminal::event::EventListener`)
- Nothing else -- the codebase prefers real implementations over mocks

**What NOT to Mock:**
- Databases -- always use real SQLite via `tempfile::TempDir`
- File operations -- use real filesystem with temp directories
- The binary itself -- integration tests spawn the actual `glass` process

## Fixtures and Factories

**Test Data:**
```rust
// Factory function for command records
fn sample_record(command: &str) -> CommandRecord {
    CommandRecord {
        id: None,
        command: command.to_string(),
        cwd: "/home/user".to_string(),
        exit_code: Some(0),
        started_at: 1700000000,
        finished_at: 1700000005,
        duration_ms: 5000,
        output: None,
    }
}

// Fixed path helper for snapshot/parser tests
fn cwd() -> PathBuf {
    PathBuf::from("/home/user/project")
}

fn resolved(relative: &str) -> PathBuf {
    cwd().join(relative)
}
```

**Location:**
- Fixtures are defined as functions inside `#[cfg(test)] mod tests` blocks
- No shared fixture files or directories
- Each test module is self-contained with its own helpers

## Coverage

**Requirements:** None enforced. No coverage threshold in CI.

**View Coverage:**
```bash
# Not configured -- use cargo-llvm-cov or similar if needed
cargo install cargo-llvm-cov
cargo llvm-cov --workspace
```

## Test Types

**Unit Tests (~450 tests):**
- Inline in source files via `#[cfg(test)] mod tests`
- Test individual functions with known inputs/outputs
- Cover edge cases: empty input, malformed data, boundary conditions
- Examples: config parsing, command parsing, pipe splitting, ID types, query filters

**Integration Tests:**
- `tests/mcp_integration.rs`: Spawns `glass mcp serve` as a child process, communicates via JSON-RPC over stdin/stdout
- `crates/glass_terminal/src/tests.rs`: ConPTY round-trip tests (Windows-only) -- spawns real PTY with PowerShell
- `crates/glass_history/src/db.rs`: Database migration tests that manually create v0/v1 schemas and verify migration

**E2E Tests:**
- Not used. No browser/GUI testing framework.
- Human verification checkpoints documented in PRD for visual/interactive features.

**Benchmarks:**
- `benches/perf_benchmarks.rs` using Criterion 0.5
- Benchmarks: `resolve_color` (truecolor/named/indexed), `osc_scan`, `cold_start` (process startup)
- Run: `cargo bench`

## Platform-Specific Tests

**Windows-only tests** gated with `#[cfg(target_os = "windows")]`:
```rust
#[cfg(test)]
#[cfg(target_os = "windows")]
mod escape_seq_tests {
    // ConPTY tests that need a real Windows PTY
}

#[test]
#[cfg(target_os = "windows")]
fn test_utf8_codepage_65001_active() {
    // Windows console codepage test
}
```

**Cross-platform test considerations:**
- Path-based tests use `PathBuf` resolution (handles `/` vs `\` automatically)
- Helper functions like `resolved()` abstract platform path differences

## Common Patterns

**Async Testing:**
- No async test framework used
- Async code (MCP server) tested via process spawning, not async test runtime
- PTY tests use `std::thread::sleep` for timing (not ideal but functional)

**Error Testing:**
```rust
#[test]
fn load_validated_malformed_toml_returns_error() {
    let result = GlassConfig::load_validated("invalid {{{{");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(!err.message.is_empty());
}

#[test]
fn test_unknown_subcommand_errors() {
    let result = Cli::try_parse_from(["glass", "bogus"]);
    assert!(result.is_err(), "Unknown subcommand should produce a clap error");
}
```

**Data-driven testing:**
```rust
#[test]
fn test_readonly_commands() {
    for cmd in &["ls foo", "cat file.txt", "grep pattern file", "echo hello", "pwd"] {
        let result = parse_command(cmd, &cwd());
        assert_eq!(
            result.confidence,
            Confidence::ReadOnly,
            "Expected ReadOnly for '{cmd}'"
        );
    }
}
```

**Database lifecycle testing:**
```rust
#[test]
fn test_full_lifecycle_integration() {
    let (db, _dir) = test_db();
    // 1. Insert records
    // 2. Verify count
    // 3. Search and verify
    // 4. Insert old records
    // 5. Prune by age
    // 6. Verify old records gone
    // 7. Verify recent records survive
}
```

**Migration testing:**
```rust
#[test]
fn test_migration_v0_to_v1() {
    // 1. Create v0 schema manually
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch("CREATE TABLE commands (... without output column ...)");
    conn.execute_batch("PRAGMA user_version = 0;");

    // 2. Open via HistoryDb::open -- triggers migration
    let db = HistoryDb::open(&db_path).unwrap();

    // 3. Verify new column works
    let mut record = sample_record("migrated cmd");
    record.output = Some("output after migration".to_string());
    let id = db.insert_command(&record).unwrap();
    // ...
}
```

**Process integration testing (MCP):**
```rust
#[test]
fn test_mcp_initialize_handshake() {
    let tmp = tempfile::tempdir().expect("Failed to create temp dir");
    let mut client = McpTestClient::spawn(tmp.path());

    let resp = client.initialize();

    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    // ...
}
```

## CI Testing

**GitHub Actions workflow** runs tests on three platforms:
- Linux (x86_64)
- macOS (aarch64)
- Windows (x86_64)

**CI commands:**
```bash
cargo fmt --all -- --check     # Format check (ubuntu only)
cargo clippy --workspace -- -D warnings  # Lint (windows)
cargo test --workspace         # Tests (all platforms)
```

## Dev Dependencies

**Workspace root:**
- `serde_json = "1.0"` -- JSON parsing in tests
- `tempfile = "3"` -- Temporary directories/files
- `criterion = "0.5"` -- Benchmarks

**Per-crate (in `[dev-dependencies]`):**
- `glass_history`: `tempfile = "3"`, `toml` (workspace)
- `glass_snapshot`: `tempfile = "3"`
- `glass_mcp`: `tempfile = "3"`

---

*Testing analysis: 2026-03-08*

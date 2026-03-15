# Coding Conventions

**Analysis Date:** 2026-03-15

## Naming Patterns

**Files:**
- Snake case for source files: `block_manager.rs`, `pty.rs`, `osc_scanner.rs`
- Test files in same crate (no separate `tests/` directory): inline `#[cfg(test)] mod tests`
- Public modules exposed via `pub mod module_name` in `lib.rs` or `main.rs`

**Functions:**
- Snake case: `parse_command()`, `encode_key()`, `query_git_status()`
- Private helpers prefixed with intent: `tokenize_powershell()`, `strip_redirects()`, `contains_unparseable_syntax()`
- Constructor functions named `new()` or specific to type: `open()`, `open_default()`, `resolve_db_path()`
- Methods on impl blocks follow snake case convention

**Variables:**
- Snake case for local bindings: `let prompt_line = line`, `let redirect_targets = vec![]`
- Single-letter for loop counters and iteration: `for (id, name, pid, last_heartbeat) in &agents`
- Temporary mutable variables: `let mut result = ParseResult { ... }`
- Underscore prefix for intentionally unused: `let (mut db, _dir) = test_db()`

**Types:**
- PascalCase for structs, enums, traits: `Block`, `BlockState`, `BlockManager`, `CoordinationDb`
- SCREAMING_SNAKE_CASE for constants: `READ_BUFFER_SIZE`, `MAX_LOCKED_READ`, `PTY_READ_WRITE_TOKEN`
- Short uppercase for enum variants: `PromptActive`, `InputActive`, `Complete`, `Header`, `StageRow`

## Code Style

**Formatting:**
- Enforced by `cargo fmt` in CI
- 4-space indentation
- Max line length enforced via clippy rules
- Brace style: opening brace on same line (Rust convention)

**Linting:**
- Clippy with `-D warnings` — all warnings are errors in CI
- Configuration enforced in CI at `.github/workflows/ci.yml` (remote only)
- Warnings must be fixed, not suppressed, except:
  - `#[allow(dead_code)]` on orchestrator module (`src/orchestrator.rs` line 7)
  - Platform-specific dead code gated with `#[cfg(target_os = "...")]`

**Module Organization:**
- Crate root (`lib.rs` or `main.rs`) declares public modules
- Submodules in separate files: `crate::module_name` resolves to `crate/module_name.rs`
- Barrel file pattern: `lib.rs` re-exports public types: `pub use compress::...`, `pub use db::CommandRecord`
- Example: `crates/glass_history/src/lib.rs` exports types from submodules

## Import Organization

**Order:**
1. Standard library imports: `use std::...`
2. External crate imports: `use anyhow::Result`, `use rusqlite::...`
3. Workspace crate imports: `use glass_core::...`, `use crate::...`
4. Module-level (optional): items from same crate submodules

**Pattern Example** (from `src/main.rs`):
```rust
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Write as _;
use std::sync::Arc;

use alacritty_terminal::event::WindowSize;
use clap::{Parser, Subcommand};
use glass_core::config::GlassConfig;
use glass_history::{
    db::{CommandRecord, HistoryDb},
    resolve_db_path,
};
```

**Path Aliases:**
- No explicit path aliases in `Cargo.toml`
- Imports use full qualified paths: `use glass_terminal::...`
- Workspace members referenced by full path

## Error Handling

**Patterns:**
- Primary error type: `anyhow::Result<T>` (wraps any error via `?` operator)
- File: `crates/glass_core/src/error.rs` provides error definitions
- Error propagation via `?` operator preferred over `match` on `Err`
- SQLite errors wrapped automatically by `anyhow::Result`

**Example Pattern** (from `crates/glass_history/src/db.rs`):
```rust
pub fn open(path: &Path) -> Result<Self> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;  // Propagate via ?
    }
    let conn = Connection::open(path)?;
    // ... setup ...
    Ok(Self { conn, path: path.to_path_buf() })
}
```

**Panic Usage (Limited):**
- Panic acceptable in:
  - Main thread initialization (e.g., font system startup in `src/main.rs` line 1569)
  - Test assertion failures
  - Orchestrator state machine invariant violations (`src/orchestrator.rs` lines 723, 735, 744, 808, 1060)
- Orchestrator panics intentional for impossible state detection during agent control loop

## Logging

**Framework:** `tracing` crate (workspace dependency)

**Patterns:**
- Structured logging with field syntax: `tracing::info!(agent_id = %id, reason = %reason, "Pruning stale agent")`
- Levels: `trace!`, `debug!`, `info!`, `warn!`, `error!`
- Usage in critical paths: agent registration, database operations, coordinate multiplexing
- Example (from `crates/glass_coordination/src/db.rs` line 813):
```rust
tracing::info!(
    agent_id = %id,
    agent_name = %name,
    reason = %reason,
    "Pruning stale agent"
);
```

**Environment Control:**
- Tracing subscriber initialized in main binary
- Filter configuration via environment variables (tracing-subscriber feature `env-filter`)
- Feature flag `perf` enables `tracing-chrome` instrumentation: `cargo build --features perf`

## Comments

**When to Comment:**
- Module-level documentation: `//!` doc comments at top of file
- Function documentation: `///` doc comments above public functions
- Non-obvious logic: inline comments `// Explanation` before complex code blocks
- Invariant assertions: comments explaining why a specific approach was needed

**JSDoc/TSDoc Style:**
- Rust uses markdown in doc comments: `/// Text describing the function`
- Parameter descriptions in doc comment text
- Example: `crates/glass_terminal/src/block_manager.rs` line 86:
```rust
/// Calculate the duration of command execution.
pub fn duration(&self) -> Option<Duration> {
```

**Module Documentation Example** (from `crates/glass_history/src/lib.rs`):
```rust
//! glass_history -- SQLite-backed command history with FTS5 search.
//!
//! Provides a database for storing, searching, and managing command execution
//! history. Supports project-local databases (`.glass/history.db`) with
//! global fallback (`~/.glass/global-history.db`).
```

## Function Design

**Size:**
- Median function length: 20-50 lines
- Longer functions (100+ lines) typically database operations with transaction management
- Helper functions extracted for readability: `tokenize()`, `strip_redirects()`, `dispatch_command()`

**Parameters:**
- Pass by reference for borrowed data: `fn parse_command(command_text: &str, cwd: &Path)`
- Move semantics for owned data: `fn new(prompt_line: usize) -> Self`
- Return types explicit: `Result<T>` for fallible operations, `Option<T>` for nullable

**Return Values:**
- Success paths return `Ok(value)` via early returns
- Fallible operations return `Result<T>` with `anyhow::Result` wrapper
- Null-like values use `Option<T>`: `Option<i32>` for exit codes, `Option<String>` for output
- None-type for pure state changes: no return value needed

## Module Design

**Exports:**
- Public types re-exported in crate root via barrel file pattern
- Example (`crates/glass_history/src/lib.rs`):
```rust
pub use compress::{diff_compress, CompressedOutput, DiffSummary, RecordFingerprint};
pub use config::HistoryConfig;
pub use db::{CommandRecord, HistoryDb, PipeStageRow};
```

**Barrel Files:**
- Crate root (`lib.rs`) imports and re-exports submodule public types
- Consumers use: `use glass_history::{CommandRecord, HistoryDb}`
- No deep paths required: modules are implementation detail

**Visibility:**
- Default private (`fn`, `struct`, `impl` without `pub`)
- Explicit `pub` for public API surface
- Private helper functions with leading underscore optional (not convention)
- Test modules always gated with `#[cfg(test)]`

## Type Definitions

**Struct Design:**
- Pub fields acceptable for simple data holders (e.g., `Block` struct in `crates/glass_terminal/src/block_manager.rs`)
- Private fields with getter methods for encapsulation when invariants must be maintained
- Derived traits: `#[derive(Debug, Clone)]` for most public structs
- Constructor patterns: `new()` for zero-state, `open()` for resource initialization

**Enum Design:**
- Variants without associated data for state machines: `BlockState::PromptActive`
- Variants with data for heterogeneous results: `PipelineHit::StageRow(usize)`
- Example (`crates/glass_terminal/src/block_manager.rs`):
```rust
pub enum BlockState {
    PromptActive,
    InputActive,
    Executing,
    Complete,
}

pub enum PipelineHit {
    Header,
    StageRow(usize),
}
```

---

*Convention analysis: 2026-03-15*

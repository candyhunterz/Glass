# Coding Conventions

**Analysis Date:** 2026-03-18

## Naming Patterns

**Files:**
- Lowercase with underscores: `block_manager.rs`, `session_db.rs`
- Module file names match public struct/module names: `session_mux.rs` exports `SessionMux`
- Test modules within source: `tests.rs`, `*_test.rs` for integration tests (e.g., `cargo_test.rs`, `go_test.rs`)

**Functions:**
- snake_case for all functions: `handle_event()`, `focused_session()`, `load_validated()`
- Getter methods use `fn name()` not `get_name()`: `focused_session()`, `current()`
- Mutable getters append `_mut`: `focused_session_mut()`, `session_mut()`
- Builder methods use `with_*` or `set_*`: `set_expanded_stage()`, `toggle_pipeline_expanded()`
- Test functions prefixed with `test_`: `test_no_subcommand_is_none()`, `test_register()`

**Variables:**
- Local variables: snake_case: `prompt_start_line`, `exit_code`, `block_manager`
- Constants: SCREAMING_SNAKE_CASE: `MAX_BLOCKS`, `SCROLLBAR_WIDTH`
- Private fields in structs: snake_case: `blocks`, `current`, `sessions`

**Types:**
- Struct names: PascalCase: `Block`, `BlockState`, `SessionMux`, `Pipeline`
- Enum names: PascalCase: `BlockState`, `Commands`, `OscEvent`
- Enum variants: PascalCase: `PromptActive`, `InputActive`, `Executing`, `Complete`
- Type aliases: PascalCase if new concept, lowercase if wrapper: `type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>`

## Code Style

**Formatting:**
- Tool: `rustfmt` (enforced in CI)
- Check: `cargo fmt --all -- --check` (must pass in CI)
- All code must be formatted before commit
- Line length: default rustfmt settings (100 char soft limit)

**Linting:**
- Tool: `clippy` with `-D warnings` (all warnings are errors)
- Command: `cargo clippy --workspace -- -D warnings` (must pass in CI)
- Suppression: Use `#[allow(dead_code)]` or `#[allow(...)]` for false positives only
- Example from `src/main.rs`: `#[allow(dead_code)]` decorates orchestrator-related modules when not in use

**Edition:**
- Rust 2021 edition (workspace-wide in `Cargo.toml`)

## Import Organization

**Order:**
1. Standard library imports: `use std::...`
2. External crates: `use tokio::`, `use serde::`, `use anyhow::`
3. Workspace crates: `use glass_core::`, `use glass_terminal::`
4. Local module imports: `use crate::...`
5. Re-exports in pub files: Listed after `pub mod` declarations

**Path Aliases:**
- No path aliases configured (no `#[path = "..."]`)
- Direct relative imports: `use crate::types::`, `use super::*`

**Module Exports:**
- Explicit re-exports in lib.rs: `pub use config::GlassConfig;`, `pub use db::HistoryDb;`
- Glob exports for convenience: `pub use types::*;` (in `glass_pipes/src/lib.rs`)
- Barrel files: Each crate has a `lib.rs` or `main.rs` that re-exports public interfaces

**Example from `glass_core/src/lib.rs`:**
```rust
pub mod activity_stream;
pub mod agent_runtime;
pub mod config;
pub mod config_watcher;
pub mod coordination_poller;
pub mod error;
pub mod event;
pub mod ipc;
pub mod updater;
```

## Error Handling

**Patterns:**
- **Custom Result type**: `pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>` in `glass_core/src/error.rs`
- **anyhow::Result**: Used in functions that need flexible error handling: `anyhow::Result<()>` for IPC, updater
- **Custom error structs**: `ConfigError` with context (file, line, column, snippet) in `glass_core/src/config.rs`
- **Option vs Result**: Use `Option<T>` when value may not exist (return `None`), use `Result<T>` when operation can fail with error context

**Error Construction:**
- Use `?` operator liberally: errors propagate through layers
- Wrap errors with context: `self.store.db().get_latest_parser_snapshot()?` in `glass_snapshot/src/undo.rs`
- Custom error creation: `ConfigError { message: String, line: Option<usize>, column: Option<usize>, snippet: Option<String> }`

**Example from `glass_snapshot/src/undo.rs`:**
```rust
pub fn undo_latest(&self) -> Result<Option<UndoResult>> {
    let snapshot = match self.store.db().get_latest_parser_snapshot()? {
        Some(s) => s,
        None => return Ok(None),  // No snapshot exists — not an error
    };
    let result = self.restore_snapshot(&snapshot)?;
    Ok(Some(result))
}
```

## Logging

**Framework:** `tracing` crate (tokio-owned structured logging)

**Usage:**
- `tracing::info!()` for important events: `tracing::info!("Auto-injecting shell integration: {}", path.display())`
- `tracing::warn!()` for recoverable issues: `tracing::warn!("Unexpected CommandExecuted in {:?} state", block.state)`
- `tracing::debug!()` for diagnostic info (not yet observed in codebase)
- `eprintln!()` for CLI errors (history subcommand): `eprintln!("Error: {}", e)`
- `println!()` for CLI output (history results): `println!("No matching commands found.")`

**Levels:**
- info: Startup events, integration points
- warn: State violations, fallbacks, non-critical failures
- debug: (implied pattern, not heavily used)
- error: Via `eprintln!` in CLI

**Example from `src/orchestrator.rs`:**
```rust
tracing::warn!(
    "Orchestrator: exceeded max retries ({}), triggering stuck detection",
    self.max_retries_before_stuck
);
```

## Comments

**When to Comment:**
- Document public APIs with `///` doc comments (observed in all public modules)
- Explain non-obvious logic (state machine transitions, buffer overflow handling)
- Warn about invariants: "Each tab holds a `SplitNode` tree of panes"
- Mark limitations: "Shell integration only re-emits OSC 133 for the current prompt"

**Doc Comments (Triple Slash):**
- Used extensively on public structs: `/// Multiplexer that manages terminal sessions organized into tabs.`
- Used on public functions: `/// Create a new `SessionMux` with a single session.`
- Describe parameters with inline comments or expanded descriptions
- Example from `glass_mux/src/session_mux.rs`:
```rust
/// Add a new tab with the given session.
///
/// When `background` is false, the tab is inserted after the active tab
/// and becomes active. When `background` is true, the tab is appended
/// to the end without changing focus (used for MCP-created tabs during
/// orchestration).
pub fn add_tab(&mut self, session: Session, background: bool) -> TabId
```

**Module-Level Comments:**
- Markdown-style module docs with `//!` at file top:
```rust
//! Block manager for shell integration command lifecycle tracking.
//!
//! Tracks commands through PromptActive -> InputActive -> Executing -> Complete
//! states, recording line ranges, exit codes, and timing for duration display.
```

**Inline Comments:**
- Rare; code is self-documenting where possible
- Used to explain state machine edge cases and buffer policies

## Function Design

**Size:**
- Most public functions are 20-50 lines (getters, simple mutations)
- Complex functions (e.g., `handle_event`) are 100+ lines but remain readable via clear branching
- No formal line limit enforced, but readability is prioritized

**Parameters:**
- Use `&self` for read operations, `&mut self` for mutations
- Owned values for types that need to be consumed: `session: Session`
- References for large types: `&OscEvent`, `&Path`, `&str`
- Option parameters for optional values: `Option<usize>`, `Option<i64>`

**Return Values:**
- Unit `()` for operations that mutate state or have side effects
- `Option<T>` when result may not exist (no error context needed)
- `Result<T>` when operation can fail with error context
- Multiple return values via tuple: Not observed; use structs instead (e.g., `UndoResult`)

**Example from `glass_mux/src/session_mux.rs`:**
```rust
/// Look up a session by its ID.
pub fn session(&self, id: SessionId) -> Option<&Session> {
    self.sessions.get(&id)
}

/// Get a mutable reference to the focused session.
pub fn focused_session_mut(&mut self) -> Option<&mut Session> {
    let focused_pane = self.tabs.get(self.active_tab)?.focused_pane;
    self.sessions.get_mut(&focused_pane)
}
```

## Module Design

**Exports:**
- Public types explicitly re-exported in `lib.rs`: `pub use db::HistoryDb;`, `pub use config::HistoryConfig;`
- Glob exports for type packages: `pub use types::*;` in `glass_pipes/src/lib.rs`
- All public APIs available at crate root

**Organization:**
- One main concept per file: `session_mux.rs` contains `SessionMux`, `block_manager.rs` contains `BlockManager`
- Internal types in separate `types.rs` module: `glass_pipes/src/types.rs` defines `Pipeline`, `PipeStage`, `BufferPolicy`
- Separate modules for variants of a concern: `glass_errors/src/` has `rust_json.rs`, `rust_human.rs`, `generic.rs` parsers

**Visibility:**
- Private by default: `struct Block` without `pub` is private to module
- Explicitly public: `pub struct BlockManager`, `pub fn handle_event(&mut self, ...)`
- Module re-exports create public API surface: `pub use types::*;`

**Barrel Files:**
Example from `crates/glass_coordination/src/lib.rs`:
```rust
pub mod db;
pub mod event_log;
pub mod types;

pub use db::CoordinationDb;
pub use types::{AgentInfo, FileLock, LockConflict, Message};
```

---

*Convention analysis: 2026-03-18*

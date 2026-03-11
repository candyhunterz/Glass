# Coding Conventions

**Analysis Date:** 2026-03-08

## Naming Patterns

**Files:**
- Use `snake_case.rs` for all source files: `block_manager.rs`, `osc_scanner.rs`, `grid_snapshot.rs`
- Crate names use `snake_case` with `glass_` prefix: `glass_core`, `glass_terminal`, `glass_renderer`
- Test files are either `tests.rs` (co-located module) or same file with `#[cfg(test)] mod tests`

**Functions:**
- Use `snake_case` for all functions: `spawn_pty`, `resolve_db_path`, `parse_command`
- Constructor pattern: `new()` for simple constructors, `open()` for I/O-backed types (databases, stores)
- Predicate functions use `is_` prefix: `is_binary()`, `is_powershell_cmdlet()`, `blob_exists()`
- Getter functions omit `get_` prefix in simple cases: `val()`, `conn()`
- Database accessors use `get_` prefix: `get_command()`, `get_snapshot()`, `get_pipe_stages()`

**Variables:**
- Use `snake_case` for all variables and fields
- Abbreviated names acceptable for loop counters and short-lived values: `i`, `j`, `b`, `tx`, `rx`
- Descriptive names for struct fields: `wakeup_received`, `prompt_start_line`, `pipeline_stage_count`

**Types:**
- Use `PascalCase` for structs, enums, and traits
- Enum variants use `PascalCase`: `BlockState::PromptActive`, `Confidence::High`
- ID wrapper types follow `{Thing}Id` pattern: `SessionId`, `TabId`
- Config structs follow `{Thing}Config` or `{Thing}Section` pattern: `GlassConfig`, `HistorySection`

**Constants:**
- Use `SCREAMING_SNAKE_CASE`: `SCHEMA_VERSION`, `PS_ALIASES`

## Code Style

**Formatting:**
- `cargo fmt` (rustfmt) with default settings
- Enforced in CI via `cargo fmt --all -- --check`
- No custom `rustfmt.toml` or `.rustfmt.toml` -- uses Rust defaults

**Linting:**
- `cargo clippy --workspace -- -D warnings` (all warnings are errors)
- No custom `clippy.toml` -- uses Clippy defaults
- Enforced in CI (runs on Windows to match primary dev platform)

## Import Organization

**Order:**
1. Standard library (`std::*`)
2. External crate imports (`alacritty_terminal::*`, `winit::*`, `rusqlite::*`)
3. Workspace crate imports (`glass_core::*`, `glass_terminal::*`)
4. Local module imports (`crate::*`, `super::*`)

**Example from `src/main.rs`:**
```rust
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::grid::{Dimensions, Scroll};
use clap::{Parser, Subcommand};
use glass_core::config::GlassConfig;
use glass_core::event::{AppEvent, GitStatus, SessionId, ShellEvent};
use glass_terminal::{
    encode_key, query_git_status, snapshot_term, Block, BlockManager, ...
};
use winit::application::ApplicationHandler;
```

**Path Aliases:**
- No path aliases. Use full crate paths via workspace dependencies.
- Re-exports in `lib.rs` provide shorthand: `use glass_terminal::BlockManager` instead of `use glass_terminal::block_manager::BlockManager`

**Re-export Pattern:**
- Each crate's `lib.rs` declares `pub mod` for all modules and `pub use` for key public types
- Example from `crates/glass_terminal/src/lib.rs`:
```rust
pub mod block_manager;
pub mod event_proxy;
// ...
pub use block_manager::{format_duration, Block, BlockManager, BlockState, PipelineHit};
pub use event_proxy::EventProxy;
```

## Error Handling

**Patterns:**

1. **`anyhow::Result<T>` for fallible operations** -- used throughout `glass_history`, `glass_snapshot`, `glass_mcp`:
```rust
pub fn open(path: &Path) -> Result<Self> { ... }
pub fn store_file(&self, source_path: &Path) -> Result<(String, u64)> { ... }
```

2. **Custom `Result<T>` type alias** in `glass_core`:
```rust
// crates/glass_core/src/error.rs
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
```

3. **Graceful fallback for config loading** -- never crash on bad config:
```rust
pub fn load() -> Self {
    match std::fs::read_to_string(&config_path) {
        Ok(contents) => Self::load_from_str(&contents),
        Err(_) => Self::default(),  // Fall back to defaults
    }
}
```

4. **Structured errors for user-facing messages** -- `ConfigError` with line/column info:
```rust
pub struct ConfigError {
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub snippet: Option<String>,
}
```

5. **`expect()` for programmer errors** (things that should never fail):
```rust
self.session_mux.focused_session().expect("no focused session")
```

6. **`tracing` for recoverable errors** -- log warnings, continue with defaults:
```rust
tracing::warn!("Failed to parse config TOML: {err}; using defaults");
```

## Logging

**Framework:** `tracing` with `tracing-subscriber` (env-filter enabled)

**Patterns:**
- Use `tracing::debug!` for routine operations (config loading, file paths)
- Use `tracing::info!` for significant events (config loaded, server started)
- Use `tracing::warn!` for recoverable errors (bad config, missing files)
- Use `tracing::error!` sparingly -- only for truly unexpected failures
- Performance-critical functions use conditional instrumentation:
```rust
#[cfg_attr(feature = "perf", tracing::instrument(skip_all))]
pub fn scan(&mut self, data: &[u8]) -> Vec<OscEvent> { ... }
```

**Performance tracing:**
- Feature flag `perf` enables `tracing::instrument` on hot-path functions
- Used in renderer (`frame.rs`, `grid_renderer.rs`) and terminal (`osc_scanner.rs`, `pty.rs`, `grid_snapshot.rs`)
- Build with `cargo build --features perf` to enable
- `tracing-chrome` crate (optional dep) for Chrome trace format output

## Comments

**When to Comment:**
- Module-level `//!` doc comments on every module explaining purpose
- `///` doc comments on all public structs, enums, functions, and fields
- Inline comments for non-obvious logic (e.g., "belt and suspenders with CASCADE")
- Section separators using `// ---------------------------------------------------------------------------`

**Doc comment style:**
```rust
/// Parse a shell command and extract file modification targets.
///
/// This is a heuristic parser -- it handles common destructive commands
/// (rm, mv, cp, sed -i, chmod, git checkout, truncate) and returns
/// `Confidence::Low` for anything it cannot parse.
pub fn parse_command(command_text: &str, cwd: &Path) -> ParseResult { ... }
```

**Section separators in large files:**
```rust
// ---------------------------------------------------------------------------
// CLI definition (clap derive)
// ---------------------------------------------------------------------------
```

## Function Design

**Size:** No strict limit, but most functions are under 50 lines. `src/main.rs` contains the monolithic event loop (~2200 lines total file).

**Parameters:**
- Use `&Path` for filesystem path parameters (not `&str` or `String`)
- Use `&str` for string references, `String` for owned strings
- Use `&[T]` for slice parameters
- Prefer borrowing over cloning

**Return Values:**
- Use `Result<T>` (anyhow) for fallible I/O operations
- Use `Option<T>` for lookups that may not find a result
- Return owned types from constructors, borrowed from accessors
- Builder-like patterns return `Self` for chaining (e.g., `QueryFilter`)

## Module Design

**Exports:**
- Every crate has a `lib.rs` that declares all modules and re-exports key types
- Use `pub use` to flatten the public API surface
- Internal-only functions stay private (no `pub`)

**Barrel Files:**
- Each crate's `lib.rs` acts as a barrel file with `pub use` re-exports
- Example: `pub use block_manager::{Block, BlockManager, BlockState}` in `glass_terminal/src/lib.rs`

**Wildcard re-exports:**
- Used sparingly: `pub use types::*` in `glass_pipes/src/lib.rs`
- Prefer named re-exports for clarity

## Struct Design

**Derive macros -- standard set per type:**
- Data types: `#[derive(Debug, Clone)]`
- Enum variants: `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`
- ID wrappers: `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]`
- Config types: `#[derive(Debug, Clone, PartialEq, Deserialize)]` with `#[serde(default)]`

**Newtype ID pattern:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(u64);

impl SessionId {
    pub fn new(id: u64) -> Self { Self(id) }
    pub fn val(self) -> u64 { self.0 }
}
```

## Platform-Specific Code

**Pattern:** Use `#[cfg(target_os = "...")]` for platform-specific code:
```rust
#[cfg(target_os = "windows")]
fn default_font_family() -> &'static str { "Consolas" }

#[cfg(target_os = "macos")]
fn default_font_family() -> &'static str { "Menlo" }

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn default_font_family() -> &'static str { "Monospace" }
```

- Windows-only dependencies use `[target.'cfg(windows)'.dependencies]` in `Cargo.toml`
- Platform-specific tests use `#[cfg(target_os = "windows")]` on the test module

## Serde/Config Patterns

**Default values pattern:**
```rust
#[derive(Deserialize)]
#[serde(default)]
pub struct GlassConfig {
    pub font_size: f32,
    // ...
}

impl Default for GlassConfig {
    fn default() -> Self { ... }
}
```

**Per-field defaults with helper functions:**
```rust
#[serde(default = "default_max_output_capture_kb")]
pub max_output_capture_kb: u32,

fn default_max_output_capture_kb() -> u32 { 50 }
```

---

*Convention analysis: 2026-03-08*

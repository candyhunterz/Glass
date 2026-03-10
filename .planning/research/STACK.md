# Technology Stack: v2.3 Agent MCP Features

**Project:** Glass v2.3 -- MCP command channel, multi-tab orchestration, structured errors, token-saving tools, live command awareness
**Researched:** 2026-03-09
**Overall Confidence:** HIGH

## Scope

This document covers ONLY new stack additions for v2.3 features. The existing validated workspace (tokio 1.50 with "full", rmcp 1.1.0, rusqlite 0.38 bundled, serde/serde_json, etc.) is unchanged and not re-researched.

New capabilities needed:
1. Async channel between MCP server and main event loop (request/response pattern)
2. Multi-tab orchestration via MCP tools (create, list, run, output, close)
3. Structured error extraction with language-specific regex parsers
4. Token-saving tools: filtered output, file change diffs, cached results, compressed context
5. Live command status checking and cancellation via PTY signals

---

## Recommended Stack

### Core Framework (No Changes)

The existing tokio 1.50.0 with `features = ["full"]` already provides everything needed for the MCP command channel (`tokio::sync::mpsc`, `tokio::sync::oneshot`). No version bump required.

### New Runtime Dependencies

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `similar` | 2.7.0 | Unified diff generation for `glass_changed_files` tool | De facto Rust diffing library. 32M downloads, actively maintained. Provides `TextDiff` with unified diff output format. Pure Rust, no system dependencies. Only dependency is `borrow` (zero-cost). The `unified_diff` feature module produces standard unified diff strings directly. |
| `regex` | 1.12.3 | Error pattern matching in `glass_errors` parsers, output filtering in `glass_output` | The standard Rust regex engine. Already a transitive dependency of several workspace crates but not directly depended upon. Needed for structured parsing of compiler output formats (Rust error codes, Python tracebacks, GCC-style file:line:col patterns). |

**Total new runtime crates: 2** (`similar`, `regex`). Both are well-established, widely-used crates.

### Why `similar` Over Alternatives

| Crate | Why Not |
|-------|---------|
| `diff` | Abandoned (last release 2020). `similar` is its maintained successor by the same author (Armin Ronacher). |
| `unified-diff` 0.2.1 | Wrapper around `similar`. Use `similar` directly for fewer dependencies and more control over diff formatting. |
| `diffy` | Less maintained, smaller user base. `similar` is the community standard. |
| Manual line-by-line comparison | Misses moved lines, insertions, and context windows. Diff algorithms (Myers, patience) are non-trivial. |

### Why `regex` Over `regex-lite`

| Option | Verdict |
|--------|---------|
| `regex` 1.12.3 | Use this. Already a transitive dependency (zero added weight). Full Unicode support, compiled DFA for performance. Error parsers will run many patterns per invocation. |
| `regex-lite` 0.1.9 | Optimized for binary size and compile time. Glass is a desktop app where binary size is not a constraint. `regex-lite` lacks features like `(?x)` verbose mode which improves readability of complex error patterns. |

### Existing Dependencies Reused (No Version Changes)

| Technology | Current Version | Reuse For | Notes |
|------------|----------------|-----------|-------|
| `tokio` | 1.50.0 (full) | `mpsc::channel` for MCP requests, `oneshot::channel` for responses | Already has `sync` feature via `full`. The mpsc+oneshot request/response pattern is the documented Tokio approach for this exact use case. |
| `serde` | 1.0.228 | MCP tool parameter structs for new tools | Already in glass_mcp. |
| `serde_json` | 1.0 | `McpResponse::Ok(serde_json::Value)` and JSON formatting | Already in glass_mcp and glass_core. |
| `schemars` | 1.0 | JSON Schema for new MCP tool parameter types | Already in glass_mcp. |
| `chrono` | 0.4 | Timestamps in `glass_cached_result` age calculations | Already in workspace. |
| `anyhow` | 1.0.102 | Error handling in glass_errors crate | Standard workspace error type. |
| `tracing` | 0.1.44 | Logging in MCP request handling and error parsing | Already in workspace. |
| `rusqlite` | 0.38.0 | History/snapshot queries for token-saving tools | Already used by glass_history, glass_snapshot. |
| `winit` | 0.30.13 | `EventLoopProxy` for sending `AppEvent::McpRequest` from MCP server to main loop | Already the event loop driver. `EventLoopProxy::send_event()` is the thread-safe mechanism. |

---

## MCP Command Channel Architecture

### Why tokio::sync::mpsc + oneshot (Not Alternatives)

The MCP server needs to send requests to the main event loop (which owns `SessionMux`) and receive responses. This is a classic request/response-over-channel pattern.

**Chosen: `tokio::sync::mpsc` (requests) + `tokio::sync::oneshot` (per-request response)**

This is already in `tokio 1.50.0` with `features = ["full"]`. Zero new dependencies.

```
MCP tool handler (async, in tokio runtime)
  -> creates oneshot::channel()
  -> sends McpRequest { ..., reply: oneshot::Sender } via mpsc::Sender
  -> awaits oneshot::Receiver for response

main.rs event loop (winit, not async)
  -> receives McpRequest from mpsc::Receiver (polled via try_recv or AppEvent)
  -> processes with full SessionMux access
  -> sends McpResponse via oneshot::Sender
```

| Alternative | Why Not |
|-------------|---------|
| `crossbeam-channel` | Not async-aware. MCP tool handlers are async (rmcp uses tokio). Would need `spawn_blocking` wrappers to bridge, adding complexity. tokio channels integrate naturally. |
| `flume` | Good crate but adds a dependency for something tokio already provides identically. |
| Direct `Arc<Mutex<SessionMux>>` sharing | SessionMux interacts with winit event loop, PTY handles, and GPU renderer state. Sharing across threads would require extensive refactoring and introduce lock contention. The channel pattern keeps ownership clear. |
| `EventLoopProxy` only (no mpsc) | `EventLoopProxy::send_event()` can push events into the winit loop, but there's no built-in response mechanism. Still need oneshot for the response half. Could use `EventLoopProxy` to deliver requests instead of mpsc, routing through `AppEvent`. This is a valid alternative and may be cleaner since main.rs already processes `AppEvent` variants. |

### Integration Decision: EventLoopProxy vs mpsc

Two viable approaches for delivering MCP requests to main.rs:

**Option A: Pure mpsc** -- MCP server holds `mpsc::Sender<McpRequest>`, main.rs polls `mpsc::Receiver` in the event loop (e.g., via a timer or a dedicated `AppEvent::PollMcp` trigger).

**Option B: EventLoopProxy bridge** -- MCP server holds `EventLoopProxy<AppEvent>` and sends `AppEvent::McpRequest(request)` directly into the winit event loop. Response still via oneshot.

**Recommendation: Option B (EventLoopProxy).** Because:
1. main.rs already dispatches all work via `AppEvent` variants -- this is the established pattern
2. No polling/timer needed -- winit wakes automatically on `send_event()`
3. The `EventLoopProxy` is already `Clone + Send` and used by the coordination poller, config watcher, and updater
4. The MCP server already runs in a separate process (`glass mcp serve`), so it needs the proxy passed at construction time -- same as the coordination poller pattern

The oneshot response channel is embedded in the `McpRequest` enum variant, so the main loop can respond directly after processing.

---

## Structured Error Parsing (glass_errors crate)

### Why `regex` is Sufficient (No Parser Combinator Needed)

| Approach | Verdict |
|----------|---------|
| `regex` patterns | Use this. Compiler error formats are line-oriented with well-defined patterns. Regex handles `error[E0308]: ...`, `File "app.py", line 15`, and `file:line:col: message` efficiently. |
| `nom` / `winnow` (parser combinators) | Overkill. Error output is not a formal grammar -- it's line-by-line pattern matching. Parser combinators add complexity and a learning curve for what amounts to `Regex::captures()` calls. |
| `pest` / `tree-sitter` | Grammar-based parsers designed for programming languages, not compiler diagnostic output. Wrong tool. |
| `aho-corasick` | Multi-pattern string matching. Useful if scanning for many literal strings, but we need capture groups (file, line, column, message). regex handles both matching and extraction. |

### Parser Crate Design

`glass_errors` should be a pure library crate with zero async dependencies:

```toml
[package]
name = "glass_errors"
version = "0.1.0"
edition = "2021"

[dependencies]
regex = "1"
serde = { workspace = true }

[dev-dependencies]
# No special dev deps -- tests use inline string fixtures
```

Dependencies: `regex` for pattern matching, `serde` for `Serialize` on `ParsedError` (needed for MCP JSON response). That's it. No `tokio`, no `anyhow`, no `tracing` -- pure input/output transformation.

### Regex Compilation Strategy

Use `std::sync::LazyLock` (stable since Rust 1.80) to compile regex patterns once:

```rust
use std::sync::LazyLock;
use regex::Regex;

static RUST_ERROR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(error|warning)\[E\d+\]: (.+)$").unwrap()
});

static RUST_LOCATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*--> (.+):(\d+):(\d+)$").unwrap()
});
```

This avoids recompiling regexes on every `parse()` call. `LazyLock` is in `std` -- no `lazy_static` or `once_cell` dependency needed.

---

## Token-Saving Tools: Diff Generation

### `similar` Usage for `glass_changed_files`

The `glass_changed_files` tool needs to produce unified diffs between snapshot blobs and current file contents. `similar` 2.7.0 provides this directly:

```rust
use similar::{TextDiff, ChangeTag};

let diff = TextDiff::from_lines(old_content, new_content);
let unified = diff.unified_diff()
    .context_radius(3)
    .header("a/path", "b/path")
    .to_string();
```

This produces standard unified diff format that agents can parse. The `context_radius` controls how many unchanged lines surround each change hunk.

### `similar` Integration Point

`similar` is used in glass_mcp (for the `glass_changed_files` tool) where it reads blob content from `SnapshotStore` and current file content, then diffs them. The existing `glass_file_diff` MCP tool already does a conceptually similar operation -- `similar` replaces manual comparison with proper diff output.

```
glass_mcp/src/tools.rs:
  glass_changed_files()
    -> spawn_blocking {
         SnapshotStore::open()
         For each snapshot_file:
           old = blob_store.read(blob_hash)
           new = std::fs::read_to_string(path)
           diff = TextDiff::from_lines(&old, &new).unified_diff()
       }
```

---

## Live Command Cancel: PTY Signal Sending

### No New Dependencies Needed

Sending Ctrl+C (SIGINT) to a PTY process requires writing the interrupt byte (`\x03`) to the PTY master. This is already how keyboard input works in Glass:

```rust
// Existing pattern in main.rs for keyboard input:
session.pty_sender.send(PtyMsg::Input(bytes))
```

For `glass_command_cancel`, the implementation sends `\x03` (ETX, the Ctrl+C byte) through the same channel:

```rust
session.pty_sender.send(PtyMsg::Input(vec![0x03]))
```

On Windows (ConPTY), writing `\x03` to the PTY input triggers the same behavior as the user pressing Ctrl+C. On Unix, it's equivalent to `SIGINT`. No platform-specific signal APIs needed.

---

## Workspace Configuration Changes

### Root Cargo.toml Additions

```toml
[workspace.dependencies]
# Diff generation for file change tracking
similar = "2"

# Error pattern matching
regex = "1"
```

### New Crate: `crates/glass_errors/Cargo.toml`

```toml
[package]
name = "glass_errors"
version = "0.1.0"
edition = "2021"

[dependencies]
regex = { workspace = true }
serde = { workspace = true }
```

Minimal dependency footprint. No async runtime, no IO, no framework dependencies.

### Modified Crate: `crates/glass_mcp/Cargo.toml`

Add dependencies for new tools:

```toml
[dependencies]
# ... existing deps ...
glass_errors = { path = "../glass_errors" }
similar = { workspace = true }
regex = { workspace = true }
```

### Modified Crate: `crates/glass_core/Cargo.toml`

Add tokio for channel types in McpRequest/McpResponse (if types live in glass_core):

```toml
[dependencies]
# ... existing deps ...
tokio = { workspace = true }  # For oneshot::Sender in McpRequest
```

Note: glass_core currently does NOT depend on tokio. Adding it is necessary because `McpRequest` contains `tokio::sync::oneshot::Sender<McpResponse>` for the reply channel. Since `AppEvent` and related types live in glass_core, the channel types must be available there.

**Alternative:** Define `McpRequest`/`McpResponse` in glass_mcp instead of glass_core, and use a simpler bridge type in `AppEvent` (e.g., `AppEvent::McpRequest(Box<dyn FnOnce(&mut SessionMux) + Send>)`). This avoids adding tokio to glass_core but reduces type safety. The tokio dependency is lightweight when only using `sync` types, so the direct approach is cleaner.

---

## What NOT to Add

| Temptation | Why Not |
|------------|---------|
| **`tokio-rusqlite` or `async-sqlite`** | The project uses synchronous rusqlite in `spawn_blocking`. This pattern is proven across glass_history, glass_snapshot, and glass_coordination. Async SQLite wrappers add complexity for zero benefit in this architecture. |
| **`crossbeam-channel`** | Not async-aware. tokio channels work naturally in the async MCP handler context. Adding crossbeam would require bridge code. |
| **`nom`, `winnow`, `pest`** | Parser combinators/generators for compiler error output is overkill. Error formats are line-oriented patterns, not formal grammars. `regex` handles all target formats cleanly. |
| **`tree-sitter`** | Designed for parsing source code, not diagnostic output. Requires grammar files and C compilation. |
| **`lazy_static` or `once_cell`** | `std::sync::LazyLock` is stable since Rust 1.80 and covers all lazy initialization needs. No external crate needed. |
| **`strip-ansi-escapes` crate** | Glass already has ANSI stripping in glass_terminal (output capture path). Reuse existing code or extract to a shared utility. |
| **`lru` or caching crate** | `glass_cached_result` does not need an in-process cache. It queries SQLite history directly. The "cache" is the history database itself. |
| **`signal-hook` or `nix`** | PTY signal sending is just writing `\x03` to the PTY master fd. No signal crate needed. |
| **`flume`** | Async channel alternative. tokio::sync::mpsc is already available and sufficient. Adding flume duplicates functionality. |
| **`async-trait`** | Not needed. rmcp's `#[tool]` macro handles async tool methods. No custom async traits required for v2.3 features. |
| **`regex-lite`** | Smaller but lacks verbose mode (`(?x)`) and has slower matching. Glass is a desktop app where binary size is not a constraint, and regex is already a transitive dependency. |

---

## Compile & Dependency Impact

| Addition | New Transitive Deps | Compile Impact | Binary Size | Notes |
|----------|---------------------|----------------|-------------|-------|
| `similar` 2.7.0 | 0 (only `borrow` trait, zero-cost) | MINIMAL (~2s) | ~15 KB | Pure Rust diff algorithms (Myers, patience). |
| `regex` 1.12.3 | `regex-automata`, `regex-syntax`, `aho-corasick`, `memchr` | LOW (~5s, likely already compiled as transitive dep) | ~200 KB | Likely already in the dependency tree via transitive deps. Check with `cargo tree -d`. |
| `glass_errors` crate | 0 (only workspace deps) | MINIMAL (~1s) | ~10 KB | Pure parsing logic, no framework code. |
| **Total** | ~0-4 new transitive crates | ~3-8s | ~225 KB | `regex` is the largest addition but is likely already transitively compiled. |

---

## Integration Points with Existing Architecture

### MCP Command Channel (glass_core + glass_mcp + main.rs)

```
glass_core/src/event.rs:
  + McpRequest enum (TabCreate, TabList, TabRun, TabOutput, TabClose, CommandStatus, CommandCancel)
  + McpResponse enum (Ok(Value), Error(String))
  + AppEvent::McpRequest(McpRequest) variant

glass_mcp/src/tools.rs (GlassServer):
  + mcp_sender: Option<EventLoopProxy<AppEvent>>  (set when running embedded, None for standalone)
  + 12 new #[tool] methods following existing pattern
  + Channel-dependent tools check mcp_sender.is_some() and return helpful error if None

main.rs:
  + Handle AppEvent::McpRequest in user_event()
  + Route requests to SessionMux methods
  + Send responses via embedded oneshot::Sender
```

### Error Extraction (glass_errors + glass_mcp)

```
glass_errors/src/lib.rs:
  + parse(output: &str, hint: Option<&str>) -> Vec<ParsedError>
  + No IO, no async, no framework deps

glass_mcp/src/tools.rs:
  + glass_errors tool: get output from history or live grid, call glass_errors::parse()
```

### Token-Saving Tools (glass_mcp + glass_history + glass_snapshot)

```
glass_mcp/src/tools.rs:
  + glass_output: query HistoryDb, apply regex/line filters
  + glass_changed_files: query SnapshotStore, generate diffs with similar
  + glass_cached_result: query HistoryDb with age filter, check snapshot timestamps
  + glass_context enhancement: add budget/focus params to existing tool
```

---

## Version Compatibility Matrix

| Package | Version | Compatible With | Confidence |
|---------|---------|-----------------|------------|
| `similar` | 2.7.0 | Rust 2021, no async needed, no platform deps | HIGH (cargo search confirmed) |
| `regex` | 1.12.3 | Rust 2021, no async needed, stable API since 1.0 | HIGH (cargo search confirmed) |
| `tokio::sync::mpsc` | 1.50.0 (existing) | Already in workspace with "full" features | HIGH (existing validated dep) |
| `tokio::sync::oneshot` | 1.50.0 (existing) | Already in workspace with "full" features | HIGH (existing validated dep) |
| `std::sync::LazyLock` | Rust 1.80+ | Glass uses Rust 2021 edition, any recent stable toolchain | HIGH (std library, stable since 1.80) |

---

## Summary of Changes

| Category | What Changes | What Stays |
|----------|-------------|------------|
| **New workspace deps** | `similar = "2"`, `regex = "1"` | All existing workspace deps unchanged |
| **New crate** | `glass_errors` (pure parsing library) | All existing 13 crates unchanged |
| **Modified crates** | `glass_mcp` (+glass_errors, +similar, +regex), `glass_core` (+tokio for channel types) | glass_terminal, glass_renderer, glass_mux, glass_history, glass_snapshot, glass_coordination, glass_pipes unchanged |
| **Channel mechanism** | tokio mpsc+oneshot via EventLoopProxy bridge | Existing AppEvent dispatch pattern |
| **Signal sending** | Write `\x03` to existing PTY sender | PTY architecture unchanged |

**Net new runtime dependencies: 2** (similar, regex). Everything else reuses the existing stack.

---

## Sources

- [tokio::sync::mpsc documentation](https://docs.rs/tokio/latest/tokio/sync/mpsc/index.html) -- Channel API reference (HIGH confidence)
- [tokio::sync::oneshot documentation](https://docs.rs/tokio/latest/tokio/sync/oneshot/index.html) -- Oneshot channel for request/response (HIGH confidence)
- [Tokio channels tutorial](https://tokio.rs/tokio/tutorial/channels) -- mpsc+oneshot request/response pattern (HIGH confidence)
- [similar crate (crates.io)](https://crates.io/crates/similar) -- v2.7.0, unified diff generation (HIGH confidence, cargo search confirmed)
- [regex crate (crates.io)](https://crates.io/crates/regex) -- v1.12.3, pattern matching (HIGH confidence, cargo search confirmed)
- [regex-lite crate (crates.io)](https://crates.io/crates/regex-lite) -- v0.1.9, considered and rejected (HIGH confidence)
- [std::sync::LazyLock](https://doc.rust-lang.org/std/sync/struct.LazyLock.html) -- Stable since Rust 1.80 (HIGH confidence)
- [Existing glass_mcp/Cargo.toml](../../../crates/glass_mcp/Cargo.toml) -- Current MCP dependencies (HIGH confidence)
- [Existing glass_core/src/event.rs](../../../crates/glass_core/src/event.rs) -- AppEvent pattern for EventLoopProxy integration (HIGH confidence)

---
*Stack research for: Glass v2.3 Agent MCP Features*
*Researched: 2026-03-09*

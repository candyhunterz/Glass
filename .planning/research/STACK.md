# Stack Research: v3.0 SOI & Agent Mode

**Domain:** Structured output parsing, background AI process management, git worktree isolation
**Researched:** 2026-03-12
**Confidence:** HIGH

## Context: What Already Exists

Glass ships with the following stack. Do NOT re-add any of these:

| Already Have | Version | Do Not Re-Add Because |
|-------------|---------|----------------------|
| `regex` | 1 (workspace dep) | Already in Cargo.toml root and glass_errors |
| `serde` / `serde_json` | 1.0 | Used throughout all crates |
| `rusqlite` | 0.38.0 (bundled) | Used by glass_history, glass_coordination, glass_snapshot |
| `tokio` | 1.50.0 (full features) | Async runtime, includes `tokio::process`, `tokio::sync::mpsc` |
| `strip-ansi-escapes` | 0.2 | Used in glass_history for ANSI stripping before DB store |
| `chrono` | 0.4 | Timestamps throughout |
| `anyhow` | 1.0 | Error handling throughout |
| `tracing` | 0.1 | Logging throughout |
| `similar` | 2 | Unified diffs in glass_mcp |
| `dirs` | 6 | Path resolution |
| `blake3` | 1.8.3 | Content hashing |

---

## New Dependencies Needed

### For glass_soi (new crate)

| Library | Version | Purpose | Why This One |
|---------|---------|---------|--------------|
| `regex` (workspace) | 1.12.3 | Pattern matching for output classification and format-specific parsers | Already a workspace dep — no new dependency. Use `std::sync::LazyLock<Regex>` for zero-overhead compilation (official regex docs recommendation). OnceLock pattern already established in glass_errors |
| `serde` (workspace) | 1.0 | Serialize `ParsedOutput`, `OutputRecord` to JSON for SQLite storage | Already workspace dep |
| `serde_json` | 1.0 | JSON serialization of `OutputRecord` into `detail_json` column; JSON lines parser for NDJSON output type | Already in several crates — add as direct dep in glass_soi |
| `rusqlite` (workspace) | 0.38.0 | New tables (`command_output_records`, `output_records`) in glass_history DB via schema migration | Already workspace dep |

**Assessment: glass_soi requires zero new crates.** All parsing, storage, and serialization is covered by existing workspace dependencies. The `regex` crate at 1.12.3 handles all pattern matching; the `LazyLock<Regex>` pattern (already used in glass_errors via `OnceLock`) provides zero-overhead compilation.

**Do NOT add:**
- `nom` — parser combinator is overkill for line-oriented output parsing. Regex + line-by-line iteration is faster to write, easier to test, and sufficient for compiler/test output
- `pest` — same rationale as nom; structural parsers for context-free grammars are not needed for terminal output
- `tap_parser` crate — TAP output can be parsed with 5 regexes; pulling in a dependency for this is not justified
- `junit-parser` crate — JUnit XML is not in scope for v3.0 (SOI_AND_AGENT_MODE.md scope is `cargo test`, `jest`, `pytest`, `go test`, `npm`, `git`, `docker`, `kubectl`, `tsc`)
- `ndjson-stream` — JSON lines parsing with serde_json is 3 lines: split on `\n`, filter blank, `serde_json::from_str` per line

### For glass_agent (new crate)

| Library | Version | Purpose | Why This One |
|---------|---------|---------|--------------|
| `tokio::process` (tokio workspace dep) | 1.50.0 | Spawn background Claude CLI process with piped stdin/stdout; async read agent proposals, async write activity stream | Already in workspace as `tokio = { version = "1.50.0", features = ["full"] }`. `tokio::process::Command` provides `Stdio::piped()`, `AsyncWriteExt`, `BufReader` for line-by-line stdout reading |
| `tokio::sync::mpsc` (tokio workspace dep) | 1.50.0 | Bounded channel for activity stream (SOI → agent runtime); bounded channel for proposals (agent runtime → approval UI) | Already in workspace; bounded mpsc is the correct choice — back-pressure prevents flooding the agent on rapid command execution |
| `uuid` | 1.22.0 | `AgentProposal.id`, `agent_sessions.id` — unique identifiers for proposals and sessions | NEW. Lightweight (pure Rust, no system deps). Only need `v4` feature for random UUIDs. The `AgentProposal` struct (defined in SOI_AND_AGENT_MODE.md) explicitly uses `Uuid` |
| `git2` | 0.20.4 | Worktree creation, diff generation, worktree cleanup for `WorktreeManager` | NEW. See rationale below |
| `serde` (workspace) | 1.0 | Serialize `ActivityEvent`, `AgentProposal`, `SessionHandoff` for JSON wire protocol and DB storage | Already workspace dep |
| `serde_json` | 1.0 | JSON protocol between Glass and Claude CLI process (Glass writes JSON events to stdin, reads JSON proposals from stdout) | Already in several crates |
| `rusqlite` (workspace) | 0.38.0 | `agent_sessions` table for session continuity | Already workspace dep |

---

## git2 vs Subprocess for Worktree Management

**Recommendation: git2 0.20.4 for worktrees, subprocess (`std::process::Command`) for fallback diff.**

**Rationale:**

git2 0.20.4 exposes `WorktreeAddOptions`, `Repository::worktree_add()`, and `Worktree::prune()` — exactly what `WorktreeManager` needs. The libgit2 source is bundled in `libgit2-sys` so there is no system dependency requirement. The `Worktree` struct provides: `path()`, `validate()`, `lock()`, `unlock()`, `prune()`, `is_prunable()`.

The alternative — shelling out to `git worktree add/remove` — would work but introduces process spawning overhead for operations that happen during the approval flow, and requires parsing `git` command output (fragile across git versions). git2 is already the de facto Rust binding for libgit2 at 0.20.4.

**Limitation noted:** `Worktree` is not `Send` or `Sync` in git2. This means `WorktreeManager` operations must run on a single thread or use `spawn_blocking`. The `spawn_blocking` pattern is already established in glass_mcp (used for `SnapshotStore` which is similarly `!Send`), so this is a known-good approach.

**Non-git projects:** Fall back to `std::fs::copy` + temp directories. No library needed for this fallback path.

**For diff generation:** Use the existing `similar` crate (already in glass_mcp) to produce unified diffs between worktree files and working tree files. No new dependency needed for diffing.

---

## Recommended Stack Summary

### New Crates to Add

| Library | Version | Add To | Cargo.toml Entry |
|---------|---------|--------|------------------|
| `uuid` | 1.22.0 | workspace deps + glass_agent | `uuid = { version = "1", features = ["v4"] }` |
| `git2` | 0.20.4 | workspace deps + glass_agent | `git2 = "0.20"` |

That is the complete list of new dependencies. Two crates.

### Workspace vs Crate-Level

Add `uuid` and `git2` to `[workspace.dependencies]` in root `Cargo.toml` so other crates (e.g., glass_mcp for proposal queries) can reference them without version divergence. glass_agent declares both as `workspace = true`.

---

## New Crate Dependency Graphs

### glass_soi

```
glass_soi
  ├── regex (workspace)       — output classification patterns
  ├── serde (workspace)       — serialize ParsedOutput, OutputRecord
  ├── serde_json              — JSON storage in detail_json column
  ├── rusqlite (workspace)    — command_output_records / output_records tables
  ├── strip-ansi-escapes      — clean ANSI before parsing (reuse glass_history version)
  ├── tracing (workspace)     — parser telemetry
  ├── anyhow (workspace)      — error propagation
  └── glass_history (path)    — access history DB, link records to command_id
```

Note: `strip-ansi-escapes 0.2` is already a dep of `glass_history`. glass_soi must add it as a direct dep too since it strips ANSI before parsing — same version, no conflict.

### glass_agent

```
glass_agent
  ├── tokio (workspace)       — process::Command, sync::mpsc, spawn_blocking
  ├── uuid (workspace)        — AgentProposal.id, agent_sessions.id
  ├── git2 (workspace)        — WorktreeManager: worktree_add, prune, path
  ├── serde (workspace)       — ActivityEvent, AgentProposal serialization
  ├── serde_json              — JSON wire protocol for Claude CLI stdin/stdout
  ├── rusqlite (workspace)    — agent_sessions table
  ├── tracing (workspace)     — agent lifecycle logging
  ├── anyhow (workspace)      — error propagation
  ├── chrono (workspace)      — session timestamps
  ├── dirs (workspace)        — ~/.glass/ path resolution
  ├── glass_soi (path)        — ActivityEvent feeds from SOI completion events
  ├── glass_history (path)    — session storage, command_id for drill-down
  └── glass_coordination (path) — advisory lock integration for agent file ops
```

---

## Integration Points with Existing Crates

| New Code | Hooks Into | How |
|----------|-----------|-----|
| `glass_soi::OutputClassifier` | `glass_terminal::block_manager.rs` | Called after `CommandFinished` with captured output bytes |
| `glass_soi::CompressionEngine` | `glass_mcp` tools | `glass_compressed_context` delegates to SOI for command summaries |
| `glass_soi` parsers | `glass_errors` | `glass_errors::extract_errors` becomes one parser dispatch path within SOI; existing `StructuredError` maps to `OutputRecord::CompilerError` |
| `glass_agent::ActivityStream` | `src/main.rs` event loop | Subscribes to `AppEvent::SoiReady { command_id }` events via `tokio::sync::mpsc` sender cloned at startup |
| `glass_agent::AgentRuntime` | `glass_mcp` | Agent receives glass_query/glass_query_drill tools via the existing MCP IPC channel (same named pipe / Unix socket infrastructure from v2.3) |
| `glass_agent::WorktreeManager` | `similar` (already in glass_mcp) | Diffs generated between worktree file and working tree file using `similar::TextDiff` |
| `glass_agent` approval UI | `glass_renderer` | New `AgentOverlay` following `SearchOverlay` / `ErrorOverlay` architectural pattern from v2.1 |

---

## Regex Strategy for SOI Parsers

The glass_errors crate already establishes the correct pattern: `OnceLock<Regex>` compiled once, dispatched via enum. SOI extends this with more parser kinds.

**Pattern to follow (already in glass_errors/detect.rs):**

```rust
use std::sync::OnceLock;
use regex::Regex;

static CARGO_TEST_RE: OnceLock<Regex> = OnceLock::new();

fn cargo_test_regex() -> &'static Regex {
    CARGO_TEST_RE.get_or_init(|| {
        Regex::new(r"test (.+) \.\.\. (ok|FAILED|ignored)").unwrap()
    })
}
```

The official regex 1.12.3 docs recommend `std::sync::LazyLock` as the preferred modern idiom (stable since Rust 1.80). Either `OnceLock` or `LazyLock` works. Use whichever matches the pattern in existing glass_errors code for consistency.

**No `lazy-regex` crate needed.** The macro convenience is not worth the extra dependency given the existing OnceLock pattern is already established in the codebase.

---

## Agent Runtime: Process Communication Pattern

The Claude CLI communicates via stdin/stdout. glass_agent uses `tokio::process::Command` with `Stdio::piped()`:

```rust
// Spawn Claude CLI with piped I/O
let mut child = tokio::process::Command::new("claude")
    .args(&["--model", &config.model, "--no-markdown"])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?;

// Write activity updates to stdin (JSON lines protocol)
let mut stdin = child.stdin.take().unwrap();
// async write via AsyncWriteExt::write_all

// Read proposals from stdout (line-by-line via BufReader)
let stdout = child.stdout.take().unwrap();
let reader = tokio::io::BufReader::new(stdout);
// async read via AsyncBufReadExt::lines()
```

This pattern is entirely within existing `tokio 1.50.0` — no new crate needed.

**Session continuity:** The `--resume <session-token>` flag on Claude CLI provides session continuation. glass_agent stores the session token in `agent_sessions.session_token` (VARCHAR column) and passes it on restart. This is a Claude CLI protocol detail, not a library dependency.

---

## What NOT to Add

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| `nom` / `pest` | Parser combinators are overkill for line-oriented terminal output. Adds compile time, learning curve, and complexity for no benefit over regex | `regex` crate with `LazyLock<Regex>` |
| `tap_parser` crate | 5 regexes cover TAP 12/13 (ok/not ok, plan, SKIP, TODO). External crate for this is unjustified | regex-based parser in glass_soi |
| `junit-parser` | JUnit XML is not in v3.0 scope per SOI_AND_AGENT_MODE.md | Defer to future milestone |
| `ndjson-stream` | JSON lines = split on `\n` + `serde_json::from_str` per line. 3 lines of code | `serde_json` (already present) |
| `lazy_static` | Superseded by `std::sync::OnceLock` (stable since 1.70) and `LazyLock` (stable since 1.80). Both are in stdlib | `std::sync::OnceLock` or `LazyLock` |
| `once_cell` | Same as lazy_static — superceded by stdlib equivalents in modern Rust | `std::sync::OnceLock` |
| `gitoxide` / `gix` | Promising pure-Rust git but still maturing for worktree operations; not battle-tested for the create/diff/apply/cleanup flow. git2 (libgit2 bindings) is stable and complete | `git2` |
| `subprocess` crate | Thin wrapper over std::process with no async support. tokio::process handles async process management better | `tokio::process` (already present) |
| `crossbeam-channel` | tokio's bounded mpsc channels cover the activity stream and proposal queue needs. No need for crossbeam in an async-first codebase | `tokio::sync::mpsc` (already present) |
| `reqwest` / `ureq` (new) | ureq 3 is already in the workspace for update checker. No additional HTTP client needed | existing `ureq` |

---

## Cargo.toml Changes Required

### Root `[workspace.dependencies]` additions:

```toml
# Agent Mode
uuid    = { version = "1", features = ["v4"] }
git2    = "0.20"
```

### New `crates/glass_soi/Cargo.toml`:

```toml
[package]
name = "glass_soi"
version = "0.1.0"
edition = "2021"
description = "Structured Output Intelligence: output classification, parsing, compression"

[dependencies]
regex          = { workspace = true }
serde          = { workspace = true }
serde_json     = "1.0"
rusqlite       = { workspace = true }
strip-ansi-escapes = "0.2"
tracing        = { workspace = true }
anyhow         = { workspace = true }
glass_history  = { path = "../glass_history" }

[dev-dependencies]
tempfile = "3"
```

### New `crates/glass_agent/Cargo.toml`:

```toml
[package]
name = "glass_agent"
version = "0.1.0"
edition = "2021"
description = "Agent Mode: background Claude CLI runtime, activity stream, worktree isolation, approval system"

[dependencies]
tokio          = { workspace = true }
uuid           = { workspace = true }
git2           = { workspace = true }
serde          = { workspace = true }
serde_json     = "1.0"
rusqlite       = { workspace = true }
tracing        = { workspace = true }
anyhow         = { workspace = true }
chrono         = { workspace = true }
dirs           = { workspace = true }
glass_soi      = { path = "../glass_soi" }
glass_history  = { path = "../glass_history" }
glass_coordination = { path = "../glass_coordination" }

[dev-dependencies]
tempfile = "3"
```

---

## Version Compatibility

| Package | Compatible With | Notes |
|---------|-----------------|-------|
| `git2 0.20` | Rust 2021 edition | Bundles libgit2 via libgit2-sys; no system libgit2 install needed. `Worktree` is `!Send` — must use `spawn_blocking` for async contexts (same pattern as `SnapshotStore` in glass_mcp) |
| `uuid 1.22` | `serde 1.0` | Add `features = ["v4", "serde"]` if UUID serialization to JSON is needed directly; otherwise `v4` alone suffices for generation |
| `regex 1.12.3` | Already in workspace | Root Cargo.toml has `regex = "1"` which resolves to 1.12.3. glass_soi uses workspace dep, no version conflict |
| `strip-ansi-escapes 0.2` | Already in glass_history | Same version — Cargo deduplicates, no conflict |

---

## Sources

- `C:/Users/nkngu/apps/Glass/Cargo.toml` — verified existing workspace deps (regex, serde_json, tokio full, rusqlite, similar, strip-ansi-escapes, chrono, dirs, anyhow)
- `C:/Users/nkngu/apps/Glass/SOI_AND_AGENT_MODE.md` — feature spec defining OutputRecord types, AgentProposal struct, WorktreeManager API, SessionHandoff schema
- [docs.rs/regex/latest](https://docs.rs/regex/latest/regex/) — version 1.12.3 confirmed; `LazyLock<Regex>` recommended pattern for single compilation (HIGH confidence)
- [docs.rs/uuid/latest](https://docs.rs/uuid/latest/uuid/) — version 1.22.0 confirmed; `features = ["v4"]` for `Uuid::new_v4()` (HIGH confidence)
- [docs.rs/git2/latest — Worktree struct](https://docs.rs/git2/latest/git2/struct.Worktree.html) — version 0.20.4, `WorktreeAddOptions`, `path()`, `validate()`, `prune()`, `is_prunable()` methods confirmed (HIGH confidence)
- [tokio::process docs](https://docs.rs/tokio/latest/tokio/process/index.html) — `Command`, `Stdio::piped()`, `AsyncWriteExt`, `BufReader` for background process management; all in tokio 1.50.0 `full` features (HIGH confidence)
- [tokio::sync::mpsc docs](https://docs.rs/tokio/latest/tokio/sync/mpsc/index.html) — bounded channel for activity stream back-pressure (HIGH confidence)
- Codebase review: `glass_errors/src/lib.rs`, `glass_errors/src/detect.rs` — OnceLock<Regex> pattern already established, confirms zero new infra needed for regex parsing

---
*Stack research for: Glass v3.0 SOI & Agent Mode*
*Researched: 2026-03-12*

# Technology Stack: v2.2 Multi-Agent Coordination

**Project:** Glass v2.2 -- Shared coordination DB, agent registry, file locks, messaging
**Researched:** 2026-03-09
**Overall Confidence:** HIGH

## Scope

This document covers ONLY new stack additions for multi-agent coordination. The existing validated workspace (rusqlite 0.38 bundled, rmcp MCP server, tokio async, blake3, etc.) is unchanged and not re-researched.

New capabilities needed:
1. Shared SQLite DB with WAL mode for multi-process concurrent access
2. UUID generation for agent IDs
3. Cross-platform PID liveness checking (Windows/macOS/Linux)
4. Path canonicalization that avoids Windows UNC prefix issues
5. No new async runtime, no IPC framework

---

## Recommended Stack

### Core Framework (No Changes)

The existing rusqlite 0.38 (bundled) handles everything needed for the coordination database. No version bump or feature additions required.

### New Runtime Dependencies

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| `uuid` | 1.22 | Agent ID generation (v4 random) | De facto Rust UUID crate. 1.22.0 is latest stable. With features `["v4"]` it generates random UUIDs using `getrandom`. Minimal footprint: the base crate has zero dependencies; `v4` adds only `getrandom`. No-std compatible. |
| `dunce` | 1.0.5 | Windows-safe path canonicalization | Wraps `std::fs::canonicalize()` but strips the `\\?\` UNC prefix on Windows when safe. Zero dependencies, 150 lines. The project already uses `std::fs::canonicalize()` in glass_snapshot (ignore_rules.rs, watcher.rs) which produces UNC paths on Windows -- dunce fixes this for path comparison in the lock table. |

**Total new runtime crates: 2** (uuid + getrandom, dunce). Minimal dependency footprint.

### Existing Dependencies Reused (No Version Changes)

| Technology | Current Version | Reuse For | Notes |
|------------|----------------|-----------|-------|
| `rusqlite` | 0.38.0 (bundled) | Coordination DB (agents.db) | Already used by glass_history and glass_snapshot with identical WAL+PRAGMA pattern. The new glass_coordination crate reuses the workspace dependency unchanged. |
| `anyhow` | 1.0.102 | Error handling | Standard workspace error type. |
| `tracing` | 0.1.44 | Logging | Structured logging for agent registration, lock conflicts, stale pruning. |
| `dirs` | 6 | `~/.glass/` path resolution | Already used by glass_history for locating the glass data directory. |
| `serde` | 1.0.228 | Serialization for MCP tool params/responses | Already in glass_mcp. |
| `schemars` | 1.0 | JSON Schema generation for MCP tools | Already in glass_mcp for tool parameter schemas. |
| `serde_json` | 1.0 | JSON serialization for MCP responses | Already in glass_mcp. |
| `chrono` | 0.4 | Timestamp formatting in messages | Already in workspace. |
| `windows-sys` | 0.59 | PID liveness checking on Windows (extended features) | Already in workspace for Console APIs. Needs additional feature `Win32_System_Threading` for `OpenProcess` / `GetExitCodeProcess`. |

### Platform-Specific PID Checking (No New Crates)

PID liveness checking is implemented using platform APIs already available or transitively present:

| Platform | API | Source | Implementation |
|----------|-----|--------|----------------|
| **Windows** | `OpenProcess` + `GetExitCodeProcess` | `windows-sys` 0.59 (already in workspace) | Open process handle with `PROCESS_QUERY_LIMITED_INFORMATION`, check if exit code equals `STILL_ACTIVE` (259). Close handle after. |
| **Unix (macOS/Linux)** | `kill(pid, 0)` | `libc` (already a transitive dependency) | Signal 0 tests process existence without sending a signal. Returns 0 if alive, ESRCH if not. |

**Why NOT add `process_alive` or `sysinfo` crate:**

| Rejected Crate | Why Not |
|----------------|---------|
| `process_alive` 0.2.0 | Adds `windows-sys` 0.61 as a dependency -- version conflict with workspace's 0.59. Its entire implementation is ~30 lines of `kill(0)` / `OpenProcess`. Not worth a dependency for trivial platform code. |
| `sysinfo` | Massive crate (~3MB compiled) that enumerates ALL system processes. We need a single `is_pid_alive(u32) -> bool` check. Overkill by orders of magnitude. |

The PID checking logic is approximately 30 lines of platform-gated code:

```rust
/// Check if a process with the given PID is still running.
pub fn is_pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // kill(pid, 0) checks existence without sending a signal
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{
            OpenProcess, GetExitCodeProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows_sys::Win32::Foundation::CloseHandle;
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle == 0 {
                return false;
            }
            let mut exit_code: u32 = 0;
            let ok = GetExitCodeProcess(handle, &mut exit_code);
            CloseHandle(handle);
            ok != 0 && exit_code == 259 // STILL_ACTIVE
        }
    }
}
```

---

## SQLite WAL Mode Configuration for Multi-Process Access

### Existing Pattern (Validated)

Glass already uses WAL mode in two databases with identical PRAGMA blocks:

```sql
-- glass_history/src/db.rs:57-60 and glass_snapshot/src/db.rs:25-28
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
PRAGMA foreign_keys = ON;
```

### Coordination DB Pattern (Same + One Addition)

The new `glass_coordination` crate uses the exact same PRAGMA block, with one critical addition for write-heavy coordination:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
PRAGMA foreign_keys = ON;
```

**Key multi-process behaviors with WAL mode:**

| Behavior | Detail |
|----------|--------|
| **Concurrent reads** | Multiple MCP server processes can read agents/locks/messages simultaneously without blocking. |
| **Write serialization** | Only one writer at a time. SQLite queues writers automatically; `busy_timeout = 5000` means a writer waits up to 5 seconds for the lock before failing. |
| **No IPC needed** | WAL uses shared memory (the `-shm` file) for coordination between processes on the same host. This replaces any need for pipes, sockets, or message queues. |
| **Crash safety** | If an MCP process crashes, its connection is released. Other processes are not affected. The WAL file self-recovers on next open. |

**Transaction pattern for atomic lock acquisition:**

Use `BEGIN IMMEDIATE` (via `conn.transaction_with_behavior(TransactionBehavior::Immediate)`) for all write operations. This acquires the write lock at transaction start rather than on first write statement, which:
1. Prevents upgrade-from-read deadlocks
2. Makes `busy_timeout` apply from the start
3. Is the recommended pattern for SQLite multi-process writes

This is supported directly by rusqlite 0.38:

```rust
use rusqlite::TransactionBehavior;

let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
// All writes within this transaction are atomic
tx.commit()?;
```

---

## Path Canonicalization Strategy

### The Problem

`std::fs::canonicalize()` on Windows produces UNC paths like `\\?\C:\Users\nkngu\apps\Glass\src\main.rs`. These paths:
- Are not comparable with normal paths (`C:\Users\nkngu\apps\Glass\src\main.rs`)
- Break many Windows tools and libraries
- Would cause file lock mismatches if one agent uses `canonicalize()` and another constructs paths normally

### The Solution: `dunce::canonicalize()`

`dunce` 1.0.5 wraps `std::fs::canonicalize()` and strips the `\\?\` prefix when the path can be expressed as a standard Windows path (drive-letter paths). It passes through unchanged on Unix. Zero dependencies, battle-tested (10M+ downloads).

### Integration

The new `glass_coordination` crate uses `dunce::canonicalize()` in `lock_files()` before storing paths. This also benefits the existing `glass_snapshot` crate -- the `ignore_rules.rs` and `watcher.rs` files currently use raw `std::fs::canonicalize()` which produces UNC paths on Windows. Consider migrating those to `dunce` as a followup.

### Existing Canonicalization Code

`glass_snapshot/src/ignore_rules.rs` already has a `canonicalize_path()` method that handles non-existent files by canonicalizing the deepest existing ancestor. The coordination crate should reuse this pattern for lock paths that reference files not yet created.

---

## Workspace Configuration Changes

### Root Cargo.toml Additions

```toml
[workspace.dependencies]
# Agent ID generation
uuid = { version = "1", features = ["v4"] }

# Windows-safe path canonicalization
dunce = "1"
```

### New Crate: `crates/glass_coordination/Cargo.toml`

```toml
[package]
name = "glass_coordination"
version = "0.1.0"
edition = "2021"

[dependencies]
rusqlite = { workspace = true }
uuid = { workspace = true }
dunce = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
dirs = { workspace = true }

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.59", features = [
    "Win32_System_Threading",
    "Win32_Foundation",
] }

[dev-dependencies]
tempfile = "3"
```

### Modified Crate: `crates/glass_mcp/Cargo.toml`

Add one dependency:

```toml
[dependencies]
# ... existing deps ...
glass_coordination = { path = "../glass_coordination" }
```

---

## What NOT to Add

| Temptation | Why Not |
|------------|---------|
| **`process_alive` crate** | Pulls `windows-sys` 0.61 (version conflict with workspace 0.59). The implementation is ~30 lines of trivial platform code. Write it inline. |
| **`sysinfo` crate** | 3MB compiled weight to check if one PID is alive. Enumerates all system processes. Absurd overkill. |
| **`nix` crate** | Large Unix abstraction layer. We need one function: `kill(pid, 0)`. Use raw `libc` call. |
| **`tokio::sync::watch` or channels for IPC** | The design explicitly chose SQLite WAL over IPC. No inter-process channels needed. Each MCP process polls the shared DB. |
| **`crossbeam-channel` or message queue** | Same as above. SQLite `messages` table IS the message queue. No in-memory IPC. |
| **`notify` for DB change watching** | Watching SQLite files for changes is unreliable (WAL writes to `-wal` and `-shm` files, not the main DB). Agents poll via `read_messages()`. |
| **`async-sqlite` or `tokio-rusqlite`** | The design specifies synchronous SQLite wrapped in `spawn_blocking` at the MCP layer. This is the same pattern used by glass_mcp for glass_history and glass_snapshot. Adding async SQLite would break the established pattern for no benefit. |
| **`parking_lot` or custom locks** | SQLite handles all locking via its internal lock manager. No Rust-level synchronization needed between processes. |
| **`uuid` v7 (time-ordered)** | v4 random UUIDs are sufficient. Agent IDs don't need time-ordering. v4 avoids any clock dependency. |

---

## Alternatives Considered

| Category | Recommended | Alternative | Why Not Alternative |
|----------|-------------|-------------|---------------------|
| UUID generation | `uuid` 1.22 with v4 | `nanoid` | UUID v4 is the standard for distributed IDs. nanoid produces shorter strings but loses the universal tooling and format recognition. |
| UUID generation | `uuid` 1.22 with v4 | `ulid` | ULIDs are time-ordered. Agent IDs don't need ordering. uuid is more widely used and understood. |
| Path canonicalization | `dunce` 1.0.5 | Manual `\\?\` prefix stripping | Stripping `\\?\` is correct for drive-letter paths but incorrect for device paths (`\\?\GLOBALROOT\...`). dunce handles the edge cases correctly. 150 lines, zero deps -- no reason not to use it. |
| Path canonicalization | `dunce` 1.0.5 | `normpath` | normpath does lexical normalization (no filesystem I/O). We need actual canonicalization (symlink resolution, existence check). |
| PID checking | Raw `libc` / `windows-sys` | `process_alive` 0.2.0 | Version conflict with workspace `windows-sys` 0.59 vs 0.61. Implementation is trivial. |
| PID checking | Raw `libc` / `windows-sys` | `sysinfo` | Massive dependency for a single boolean check. |
| Coordination mechanism | SQLite WAL | Redis / ZeroMQ | External service dependency. Glass is a local desktop app. SQLite is embedded and already in the stack. |
| Coordination mechanism | SQLite WAL | Named pipes / Unix domain sockets | Requires designing a wire protocol, connection management, reconnection logic. SQLite gives ACID transactions for free. |
| Coordination mechanism | SQLite WAL | Shared memory (mmap) | Requires manual synchronization, no schema, crash recovery is manual. SQLite provides all of this. |

---

## Compile & Dependency Impact

| Addition | New Transitive Deps | Compile Impact | Binary Size | Notes |
|----------|---------------------|----------------|-------------|-------|
| `uuid` 1.22 (v4 feature) | `getrandom` (likely already present via other deps) | MINIMAL (~1s) | ~5 KB | Tiny crate. getrandom is a common transitive dep. |
| `dunce` 1.0.5 | None | MINIMAL (<1s) | ~2 KB | 150 lines, zero dependencies. |
| `libc` 0.2 (unix only) | None | NONE (already transitive) | 0 KB | Already pulled in by alacritty_terminal, notify, and others. |
| `windows-sys` feature additions | None (features on existing dep) | MINIMAL | ~1 KB | Just adds Threading/Foundation bindings to already-compiled crate. |
| **Total** | ~1 new transitive crate (getrandom, if not already present) | ~2s | ~8 KB | Negligible impact. |

This is the lightest-weight milestone in terms of new dependencies. The design intentionally reuses SQLite infrastructure that's already proven in the workspace.

---

## Integration Points with Existing Architecture

### glass_coordination (New Crate)

```
CoordinationDb::open()
  -> Opens ~/.glass/agents.db
  -> Same WAL + PRAGMA pattern as HistoryDb and SnapshotDb
  -> Schema: agents, file_locks, messages tables
  -> Pure synchronous API (no async, no tokio)
```

### glass_mcp Integration

```
GlassServer (existing)
  -> Currently holds: glass_dir (PathBuf) for snapshot operations
  -> Add: CoordinationDb instance (opened once at server startup)
  -> 11 new #[tool_handler] methods following existing pattern
  -> Write operations use spawn_blocking (same as existing undo/snapshot tools)
```

**Pattern precedent:** The existing `glass_undo` tool in glass_mcp opens `SnapshotStore` in `spawn_blocking`:

```rust
#[tool_handler]
async fn glass_undo(&self, params: UndoParams) -> Result<CallToolResult, McpError> {
    let glass_dir = self.glass_dir.clone();
    let result = tokio::task::spawn_blocking(move || {
        // Synchronous SQLite operations here
    }).await.map_err(/* ... */)?;
    // ...
}
```

The new coordination tools follow this exact pattern.

### windows-sys Feature Extension

Current workspace definition:
```toml
windows-sys = { version = "0.59", features = ["Win32_System_Console"] }
```

The glass_coordination crate adds its own platform-specific dependency with additional features. This does NOT modify the workspace-level definition -- the crate declares its own `windows-sys` dependency with the features it needs. Cargo merges features when resolving.

---

## Version Compatibility Matrix

| Package | Compatible With | Verified |
|---------|-----------------|----------|
| `uuid` 1.22.0 | Rust 2021 edition, no async runtime needed | Version confirmed via docs.rs (HIGH confidence) |
| `dunce` 1.0.5 | Any Rust edition, zero deps | Version confirmed via docs.rs (HIGH confidence) |
| `libc` 0.2.x | Already resolved in Cargo.lock as transitive dep | Confirmed in Cargo.lock (HIGH confidence) |
| `windows-sys` 0.59 | Threading + Foundation features available | Features confirmed via docs.rs (HIGH confidence) |
| `rusqlite` 0.38.0 | WAL mode, `TransactionBehavior::Immediate`, `busy_timeout` pragma | Existing usage in glass_history and glass_snapshot validates all needed APIs (HIGH confidence) |

---

## Sources

- [uuid crate docs (docs.rs)](https://docs.rs/uuid/latest/uuid/) -- v1.22.0, features confirmed (HIGH confidence)
- [dunce crate docs (docs.rs)](https://docs.rs/dunce/latest/dunce/) -- v1.0.5, UNC stripping behavior confirmed (HIGH confidence)
- [SQLite WAL mode documentation (sqlite.org)](https://sqlite.org/wal.html) -- Multi-process concurrency guarantees (HIGH confidence)
- [SQLite recommended PRAGMAs](https://highperformancesqlite.com/articles/sqlite-recommended-pragmas) -- busy_timeout and WAL configuration (MEDIUM confidence)
- [rusqlite TransactionBehavior (docs.rs)](https://docs.rs/rusqlite/latest/rusqlite/enum.TransactionBehavior.html) -- BEGIN IMMEDIATE support confirmed (HIGH confidence)
- [std::fs::canonicalize UNC issue (rust-lang/rust#42869)](https://github.com/rust-lang/rust/issues/42869) -- Windows UNC path problem documented (HIGH confidence)
- [windows-sys Threading module (docs.rs)](https://docs.rs/windows-sys/latest/windows_sys/Win32/System/Threading/index.html) -- OpenProcess, GetExitCodeProcess available (HIGH confidence)
- [process_alive crate (lib.rs)](https://lib.rs/crates/process_alive) -- v0.2.0, uses windows-sys 0.61 (version conflict confirmed) (HIGH confidence)
- [SQLite busy_timeout behavior (berthub.eu)](https://berthub.eu/articles/posts/a-brief-post-on-sqlite3-database-locked-despite-timeout/) -- BEGIN IMMEDIATE recommended for writes (MEDIUM confidence)
- [Existing glass_history/src/db.rs WAL pattern](../../../crates/glass_history/src/db.rs) -- Validated working WAL configuration (HIGH confidence)
- [Existing glass_snapshot/src/ignore_rules.rs canonicalization](../../../crates/glass_snapshot/src/ignore_rules.rs) -- Existing canonicalize_path pattern for non-existent files (HIGH confidence)

---
*Stack research for: Glass v2.2 Multi-Agent Coordination*
*Researched: 2026-03-09*

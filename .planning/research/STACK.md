# Stack Research

**Domain:** SQLite FTS5 history database + MCP server for GPU-accelerated terminal emulator
**Researched:** 2026-03-05
**Confidence:** HIGH

## Scope

This document covers ONLY the new dependencies needed for v1.1 (Structured Scrollback + MCP Server). The existing v1.0 stack (wgpu 28.0, winit 0.30.13, alacritty_terminal 0.25.1, glyphon 0.10.0, tokio 1.50.0, serde 1.0.228, etc.) is validated and unchanged.

---

## New Dependencies for v1.1

### Core Technologies

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| rusqlite | 0.38.0 | SQLite database with FTS5 full-text search | Already in workspace with `bundled` feature. The bundled build unconditionally compiles SQLite 3.51.1 with `-DSQLITE_ENABLE_FTS5` (verified in libsqlite3-sys build.rs) -- FTS5 works out of the box with zero additional feature flags. Direct SQL is the right abstraction for FTS5 virtual tables; ORMs fight FTS5's MATCH/rank syntax. |
| rmcp | 1.1.0 | Official Rust MCP SDK for JSON-RPC server over stdio | The official Model Context Protocol SDK at github.com/modelcontextprotocol/rust-sdk. Released 2026-03-04. Provides `#[tool]` and `#[tool_box]` procedural macros for declaring MCP tools, automatic JSON Schema generation via schemars, and built-in stdio transport via `transport-io` feature. Tracks the canonical MCP spec. |
| clap | 4.5 | CLI argument parsing for `glass history` and `glass mcp serve` | Industry standard for Rust CLIs. Derive API makes subcommand definitions declarative and type-safe. Needed to route `glass` (terminal mode), `glass history search` (query mode), and `glass mcp serve` (MCP server mode) from a single binary. |
| schemars | 1.0 | JSON Schema generation for MCP tool parameters | Required by rmcp 1.1's `schemars` feature. The `#[tool(param)]` and `#[tool(aggr)]` macros use schemars to auto-generate parameter schemas that MCP clients (Claude, etc.) consume for tool discovery. Must be ^1.0 -- rmcp 1.1 is incompatible with schemars 0.8. |
| chrono | 0.4 | Timestamp handling for command history records | Storing/querying command execution timestamps, duration formatting in search results, retention policy expiry calculations. rmcp brings chrono transitively, but glass_history uses it directly -- declare explicitly. Use `serde` feature for database serialization. |
| serde_json | 1.0 | JSON serialization for MCP messages and CLI output | glass_history and glass_mcp use it directly for structured output and MCP content serialization. Already a transitive dep of rmcp and serde, but declare explicitly for direct usage. |

### Supporting Libraries

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| uuid | 1.0 | Unique IDs for command history entries | Generate unique entry IDs for the history database. Use `v4` feature for random UUIDs. Lightweight, no external deps with `v4`. Alternative: use SQLite INTEGER PRIMARY KEY AUTOINCREMENT (simpler, avoids the dep). Prefer uuid only if entries need to be referenced across processes (e.g., MCP tool responses). |

### Existing Dependencies Now Active (Already in Workspace)

These were already declared in the workspace Cargo.toml but were unused stubs. They now become active:

| Library | Version | Crate Using It | Notes |
|---------|---------|----------------|-------|
| rusqlite | 0.38.0 | glass_history | Was listed as stub dep. Now primary database layer. No version change needed. |
| tokio | 1.50.0 | glass_history, glass_mcp | Already active in v1.0. glass_mcp uses it for async MCP transport. glass_history uses `spawn_blocking` for DB access. |
| serde | 1.0.228 | glass_history, glass_mcp | Already active in v1.0. Now also used for history record serialization. |
| tracing | 0.1.44 | glass_history, glass_mcp | Already active. Critical: MCP stdio servers must NEVER use println! (corrupts JSON-RPC). All output goes through tracing to stderr. |
| anyhow | 1.0.102 | glass_history, glass_mcp | Already active. Error propagation in new crates. |

---

## Installation

### Add to `[workspace.dependencies]` in root Cargo.toml

```toml
# v1.1: History + MCP (NEW)
rmcp          = { version = "1.1", features = ["server", "transport-io", "macros", "schemars"] }
clap          = { version = "4.5", features = ["derive"] }
schemars      = "1.0"
chrono        = { version = "0.4", features = ["serde"] }
serde_json    = "1.0"
```

### No Change Needed for Existing Entries

```toml
# Already present -- no modifications required
rusqlite      = { version = "0.38.0", features = ["bundled"] }
tokio         = { version = "1.50.0", features = ["full"] }
serde         = { version = "1.0.228", features = ["derive"] }
```

The `bundled` feature already enables FTS5 at the SQLite compile level. Do NOT add `bundled-full` unless you need extra SQLite features like `serialize` or `column_decltype` -- `bundled` alone provides FTS5, WAL mode, and everything needed for a history database.

### Per-Crate Dependencies

**glass_history/Cargo.toml:**
```toml
[dependencies]
rusqlite.workspace = true
chrono.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
tracing.workspace = true
anyhow.workspace = true
```

**glass_mcp/Cargo.toml:**
```toml
[dependencies]
glass_history = { path = "../glass_history" }
rmcp.workspace = true
schemars.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
tracing.workspace = true
anyhow.workspace = true
```

**Root binary (glass/Cargo.toml) additions:**
```toml
[dependencies]
# Add to existing dependencies
clap.workspace = true
glass_history = { path = "crates/glass_history" }
glass_mcp     = { path = "crates/glass_mcp" }
```

---

## Architecture Integration Notes

### SQLite Threading Model

rusqlite::Connection is `Send` but not `Sync`. Two viable patterns for tokio integration:

**Pattern A: Dedicated thread + channel (RECOMMENDED)**
```rust
// Matches existing PTY reader thread architecture
let (tx, rx) = tokio::sync::mpsc::channel(64);
std::thread::spawn(move || {
    let conn = Connection::open("~/.glass/history.db").unwrap();
    // Own the Connection, recv query/insert commands via channel
    while let Some(cmd) = rx.blocking_recv() {
        match cmd { /* execute SQL, send results back via oneshot */ }
    }
});
```

This is preferred because it mirrors the existing dedicated PTY reader thread pattern and avoids Mutex contention between the renderer loop and history writes.

**Pattern B: Mutex + spawn_blocking**
```rust
let db = Arc::new(tokio::sync::Mutex::new(conn));
tokio::task::spawn_blocking(move || {
    let conn = db.blocking_lock();
    conn.execute(...);
});
```

Simpler but risks blocking the tokio runtime if many queries queue up. Use only for the CLI query interface where there is no render loop contention.

### MCP Server: Separate Entry Point, Same Binary

rmcp's stdio transport reads from stdin and writes to stdout. The MCP server CANNOT run in the same process instance as the terminal emulator (which owns stdin/stdout for the PTY). The solution:

```
glass                      # Launch terminal emulator (default, no clap)
glass history search "X"   # CLI query, reads DB directly, exits
glass history list         # List recent commands, exits
glass mcp serve            # Start MCP server on stdio, blocks until client disconnects
```

The MCP client (Claude Desktop, etc.) spawns `glass mcp serve` as a child process. The MCP server opens the same SQLite database file as the terminal emulator. SQLite's WAL mode allows concurrent readers, so the MCP server can query while the terminal writes.

**rmcp server setup:**
```rust
use rmcp::{ServiceExt, transport::io::stdio};

let service = GlassMcpServer::new(db_path);
let transport = stdio();
let server = service.serve(transport).await?;
server.waiting().await?;
```

**Critical: no stdout in MCP mode.** The `tracing-subscriber` must be configured to write to stderr (or a file) when running as MCP server. Any stdout output corrupts the JSON-RPC stream.

### FTS5 Schema Design

FTS5 is a SQLite virtual table extension. No Rust-side feature flags needed beyond `bundled`. Usage is pure SQL:

```sql
-- Main history table
CREATE TABLE commands (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    command TEXT NOT NULL,
    output TEXT,
    cwd TEXT,
    exit_code INTEGER,
    duration_ms INTEGER,
    started_at TEXT NOT NULL,  -- ISO 8601 via chrono
    shell TEXT
);

-- FTS5 virtual table for full-text search
CREATE VIRTUAL TABLE commands_fts USING fts5(
    command,
    output,
    cwd,
    content='commands',
    content_rowid='id'
);

-- Search query
SELECT c.* FROM commands c
JOIN commands_fts f ON c.id = f.rowid
WHERE commands_fts MATCH ?
ORDER BY rank
LIMIT 50;
```

The `content=` and `content_rowid=` options create an "external content" FTS5 table that shares storage with the main table, avoiding data duplication.

### SQLite WAL Mode for Concurrency

WAL (Write-Ahead Logging) mode is essential because the terminal emulator writes history while the MCP server may be reading concurrently:

```sql
PRAGMA journal_mode=WAL;
```

WAL mode allows one writer + multiple concurrent readers. This is supported by SQLite 3.51.1 (bundled version). Set this once at database creation.

---

## Alternatives Considered

| Recommended | Alternative | When to Use Alternative |
|-------------|-------------|-------------------------|
| rusqlite (direct SQL) | sqlx | Never for this project. sqlx's compile-time query checking does not support FTS5 virtual table syntax. Its async model adds unnecessary complexity for single-file embedded SQLite. |
| rusqlite (direct SQL) | diesel | Never. Diesel's ORM layer fights FTS5 MATCH queries and rank functions. Massive compile-time cost for zero benefit on an embedded database. |
| rusqlite (direct SQL) | sea-orm | Never. Same ORM-vs-FTS5 impedance mismatch as diesel. |
| rmcp (official SDK) | rust-mcp-sdk | Only if you need backward compat with older MCP spec versions (2024-11-05). rmcp tracks the latest spec (2025-11-25) and has the best macro ergonomics. |
| rmcp (official SDK) | Hand-rolled JSON-RPC | Only if you need absolute minimal dependencies. Not worth the maintenance burden of tracking MCP spec changes manually. |
| clap (derive) | argh | Only if binary size matters. Glass is already 80MB+ with GPU drivers, so clap's ~200KB is irrelevant. clap has vastly better documentation and ecosystem. |
| chrono | time crate | Only if you want fewer transitive dependencies. chrono is already pulled by rmcp, so adding it costs nothing. Its formatting API (`%Y-%m-%d %H:%M:%S`) is more ergonomic. |
| Dedicated DB thread | tokio-rusqlite crate | tokio-rusqlite wraps rusqlite in spawn_blocking internally. Using it hides the threading model. The dedicated thread + channel pattern is explicit and matches the existing PTY reader architecture. |
| SQLite INTEGER PK | uuid crate | Prefer SQLite AUTOINCREMENT for simplicity. Only add uuid if cross-process entry references become necessary (unlikely for v1.1). |

---

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| sqlx | Async SQL with compile-time checking. FTS5 virtual tables not supported by sqlx macros. Overkill for single-connection embedded SQLite. | rusqlite with direct SQL |
| diesel | Heavy ORM. FTS5 queries require raw SQL escapes. Massive compile cost. | rusqlite with direct SQL |
| jsonrpc crate | Low-level JSON-RPC without MCP protocol awareness. Would require reimplementing tool registration, schema generation, and protocol negotiation from scratch. | rmcp with `transport-io` |
| r2d2 (connection pool) | SQLite is single-writer. Connection pooling adds complexity without benefit for an embedded database in one process. | Single Connection behind a channel or Mutex |
| tokio-rusqlite | Abstracts away the threading model. Hides whether spawn_blocking or a dedicated thread is used. Prefer explicit control. | `std::thread::spawn` + channel (matches PTY reader pattern) |
| schemars 0.8 | rmcp 1.1.0 requires schemars ^1.0. Using 0.8 causes version conflicts. | schemars 1.0 |
| println! (in MCP server) | Writes to stdout, corrupting the JSON-RPC message stream. The single most common MCP server bug. | tracing macros with stderr subscriber |

---

## Version Compatibility

| Package | Compatible With | Notes |
|---------|-----------------|-------|
| rmcp 1.1 | tokio 1.x | Uses tokio for async transport. Compatible with workspace tokio 1.50.0. |
| rmcp 1.1 | serde 1.x | Uses serde for JSON-RPC serialization. Compatible with workspace serde 1.0.228. |
| rmcp 1.1 | schemars ^1.0 | Must use schemars 1.0+, not 0.8. This is a hard requirement. |
| rmcp 1.1 | chrono 0.4.x | Transitive dependency. Compatible with explicit chrono 0.4.43. |
| rusqlite 0.38 | SQLite 3.51.1 | Bundled version. FTS5 + WAL mode enabled at compile time. |
| clap 4.5 | serde 1.x | Optional serde integration via `serde` feature if needed. |
| chrono 0.4 | serde 1.x | With `serde` feature for serializable timestamps. |

---

## Dependency Impact Assessment

### Compile Time Impact

| Dependency | Estimated Impact | Mitigation |
|------------|-----------------|------------|
| rmcp 1.1 | MODERATE -- pulls in futures, tokio-util, schemars, chrono | Already shares tokio and serde with workspace. Incremental builds unaffected after first compile. |
| clap 4.5 (derive) | LOW-MODERATE -- proc macro compilation | One-time cost. Derive macro runs at compile time only. |
| schemars 1.0 | LOW | Small crate, derive macro. |
| rusqlite 0.38 (bundled) | MODERATE -- compiles SQLite C source from scratch on first build | Already in workspace. C compilation cached by cargo. ~30s first build, then cached. |

### Binary Size Impact

| Dependency | Estimated Size | Notes |
|------------|---------------|-------|
| rusqlite + bundled SQLite | ~1.5 MB | SQLite C library statically linked. Acceptable for a desktop app. |
| rmcp | ~500 KB | JSON-RPC protocol, tool system. |
| clap | ~200 KB | CLI parsing. Minimal for a desktop app. |
| Total v1.1 addition | ~2.2 MB | Negligible compared to existing ~80MB GPU driver overhead. |

### Runtime Memory Impact

| Component | Estimated Memory | Notes |
|-----------|-----------------|-------|
| SQLite connection | ~2-5 MB | Depends on page cache size. Default 2MB cache. |
| FTS5 index | Proportional to data | ~30% of indexed text size for the FTS index. |
| MCP server | ~1-2 MB | JSON-RPC buffers, tool registry. Only when `glass mcp serve` is running. |
| Search overlay UI | ~1 MB | Search results buffer, rendered text. |

Total estimated memory increase: ~5-10 MB on top of current ~86 MB idle. Well within the <120 MB constraint.

---

## Sources

- [rusqlite GitHub](https://github.com/rusqlite/rusqlite) -- verified FTS5 enabled unconditionally in bundled build via `build.rs` (`-DSQLITE_ENABLE_FTS5`)
- [rusqlite libsqlite3-sys build.rs](https://github.com/rusqlite/rusqlite/blob/master/libsqlite3-sys/build.rs) -- confirmed FTS5 flag is unconditional in bundled builds
- [rusqlite feature flags](https://docs.rs/crate/rusqlite/latest/features) -- 45 feature flags, `bundled` includes `modern_sqlite`, no separate FTS5 flag exists
- [rmcp docs.rs v1.1.0](https://docs.rs/crate/rmcp/latest) -- released 2026-03-04, features: server (default), transport-io, macros (default), schemars
- [MCP Rust SDK GitHub](https://github.com/modelcontextprotocol/rust-sdk) -- official SDK, server + transport-io + macros pattern, schemars ^1.0 dependency
- [rmcp tool macros guide](https://hackmd.io/@Hamze/S1tlKZP0kx) -- #[tool], #[tool_box], schemars integration, parameter schema generation
- [MCP stdio server in Rust](https://www.shuttle.dev/blog/2025/07/18/how-to-build-a-stdio-mcp-server-in-rust) -- critical warning: never use println! in stdio MCP servers
- [clap docs.rs v4.5.60](https://docs.rs/crate/clap/latest) -- latest stable, derive API for subcommands
- [serde_json docs.rs v1.0.149](https://docs.rs/crate/serde_json/latest) -- latest stable
- [chrono docs.rs v0.4.43](https://docs.rs/crate/chrono/latest) -- latest stable, released 2026-01-14
- [SQLite FTS5 documentation](https://sqlite.org/fts5.html) -- external content tables, MATCH syntax, rank function

---
*Stack research for: Glass v1.1 Structured Scrollback + MCP Server*
*Researched: 2026-03-05*

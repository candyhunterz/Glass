# Phase 5: History Database Foundation - Research

**Researched:** 2026-03-05
**Domain:** SQLite database with FTS5 full-text search, subcommand routing, Rust
**Confidence:** HIGH

## Summary

Phase 5 builds the `glass_history` crate as a standalone SQLite-backed command history database with FTS5 full-text search, project-aware storage paths, automatic retention policies, and adds clap-based subcommand routing to the `glass` binary. The crate currently exists as a stub with only a comment in `lib.rs`.

The workspace already pins `rusqlite = { version = "0.38.0", features = ["bundled"] }` in `Cargo.toml`. The `bundled` feature compiles SQLite from source with `-DSQLITE_ENABLE_FTS5` enabled by default, so FTS5 is available without additional feature flags. For subcommand routing, clap 4.x with the `derive` feature provides the cleanest ergonomics -- the binary currently has no CLI argument parsing at all (it goes straight to the winit event loop).

**Primary recommendation:** Build `glass_history` as a pure library crate with zero GUI dependencies. Use rusqlite directly (no ORM). Store timestamps as INTEGER (Unix epoch seconds) for simplicity and query performance. Use WAL journal mode. Add clap to the root `glass` binary with `Option<Subcommand>` so no subcommand means "launch terminal" (preserving current behavior).

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| HIST-01 | Every command execution logged to SQLite with metadata (command text, cwd, exit code, start/end timestamps, duration) | Schema design in Architecture Patterns; rusqlite INSERT patterns in Code Examples |
| HIST-03 | FTS5 full-text search index on command text | FTS5 virtual table design, MATCH syntax, BM25 ranking in Architecture Patterns and Code Examples |
| HIST-04 | Per-project database (`.glass/history.db`) with global fallback (`~/.glass/global-history.db`) | Database path resolution pattern in Architecture Patterns |
| HIST-05 | Retention policies: configurable max age (default 30 days) and max size (default 1 GB), automatic pruning | Pruning strategy in Architecture Patterns; SQLite page_count/page_size for size check |
| INFR-01 | Subcommand routing via clap (default = terminal, `history` = CLI, `mcp serve` = MCP server) | Clap derive pattern with Option<Subcommand> in Architecture Patterns and Code Examples |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| rusqlite | 0.38.0 | SQLite database access | Already in workspace Cargo.toml; bundled feature compiles SQLite with FTS5 enabled |
| clap | 4.5.x | CLI argument/subcommand parsing | De facto standard for Rust CLIs; derive macro provides clean subcommand routing |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| dirs | 6.x | Home directory resolution | Already in workspace; needed for `~/.glass/global-history.db` path |
| chrono | 0.4.x | Timestamp formatting for display | Only if human-readable timestamps needed in output; otherwise use i64 Unix epoch |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| rusqlite | sqlx | sqlx is async and heavier; rusqlite is synchronous which is simpler for a writer thread pattern |
| i64 Unix timestamps | chrono DateTime | chrono adds a dependency; i64 is simpler for storage, chrono only needed for display formatting |
| clap derive | clap builder | Builder API is more verbose; derive is cleaner for the simple routing Glass needs |

**Installation (add to workspace Cargo.toml):**
```toml
[workspace.dependencies]
clap = { version = "4.5", features = ["derive"] }

# glass_history/Cargo.toml
[dependencies]
rusqlite = { workspace = true }
dirs = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
serde = { workspace = true }

# Root glass/Cargo.toml additions
clap = { workspace = true }
glass_history = { path = "crates/glass_history" }
```

## Architecture Patterns

### Recommended Project Structure
```
crates/glass_history/src/
  lib.rs          # Public API: HistoryDb, CommandRecord, SearchResult
  db.rs           # Connection management, schema migration, WAL setup
  schema.rs       # CREATE TABLE statements, migration logic
  search.rs       # FTS5 query building and search
  retention.rs    # Pruning by age and size
  config.rs       # HistoryConfig (max_age, max_size, db path overrides)
  error.rs        # Error types
```

### Pattern 1: Database Schema
**What:** Two tables -- a main `commands` table and an FTS5 virtual table indexing command text.
**When to use:** Always -- this is the core data model.

```sql
-- Main storage table
CREATE TABLE IF NOT EXISTS commands (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    command     TEXT NOT NULL,
    cwd         TEXT NOT NULL,
    exit_code   INTEGER,
    started_at  INTEGER NOT NULL,  -- Unix epoch seconds
    finished_at INTEGER NOT NULL,  -- Unix epoch seconds
    duration_ms INTEGER NOT NULL,  -- Precomputed for fast queries
    created_at  INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Indexes for common query patterns
CREATE INDEX IF NOT EXISTS idx_commands_started_at ON commands(started_at);
CREATE INDEX IF NOT EXISTS idx_commands_cwd ON commands(cwd);
CREATE INDEX IF NOT EXISTS idx_commands_exit_code ON commands(exit_code);

-- FTS5 content table (stores its own copy of command text)
-- Decision: content FTS5 tables (not external content) per STATE.md
CREATE VIRTUAL TABLE IF NOT EXISTS commands_fts USING fts5(
    command,
    content='commands',
    content_rowid='id',
    tokenize='unicode61'
);

-- Triggers to keep FTS in sync with main table
CREATE TRIGGER IF NOT EXISTS commands_ai AFTER INSERT ON commands BEGIN
    INSERT INTO commands_fts(rowid, command) VALUES (new.id, new.command);
END;

CREATE TRIGGER IF NOT EXISTS commands_ad AFTER DELETE ON commands BEGIN
    INSERT INTO commands_fts(commands_fts, rowid, command) VALUES('delete', old.id, old.command);
END;

CREATE TRIGGER IF NOT EXISTS commands_au AFTER UPDATE ON commands BEGIN
    INSERT INTO commands_fts(commands_fts, rowid, command) VALUES('delete', old.id, old.command);
    INSERT INTO commands_fts(rowid, command) VALUES (new.id, new.command);
END;
```

**IMPORTANT CORRECTION on content tables:** The STATE.md says "Use content FTS5 tables (not external content) for safety." However, the SQL above uses `content='commands'` which IS an external content table. A true "content" (standard) FTS5 table would omit the `content=` option entirely, storing its own copy. The decision in STATE.md should be interpreted as: use a standard FTS5 table that maintains its own copy of data (no `content=` or `content=''` options). This is safer because it avoids sync issues between the FTS index and the source table.

**Recommended approach (standard FTS5 table, no external content):**
```sql
-- Standard FTS5 table (stores its own copy -- safe, no sync triggers needed)
CREATE VIRTUAL TABLE IF NOT EXISTS commands_fts USING fts5(
    command,
    tokenize='unicode61'
);
```
With this approach, you INSERT into both `commands` and `commands_fts` in the same transaction. No triggers needed. Deletion requires deleting from both tables. This is simpler and matches the STATE.md decision.

### Pattern 2: Database Path Resolution (HIST-04)
**What:** Project-local database with global fallback.
**When to use:** Every time a `HistoryDb` is opened.

```rust
use std::path::{Path, PathBuf};

/// Resolve the database path:
/// 1. If cwd (or any ancestor) contains `.glass/`, use `.glass/history.db`
/// 2. Otherwise, use `~/.glass/global-history.db`
pub fn resolve_db_path(cwd: &Path) -> PathBuf {
    // Walk up from cwd looking for .glass/ directory
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        let glass_dir = d.join(".glass");
        if glass_dir.is_dir() {
            return glass_dir.join("history.db");
        }
        dir = d.parent();
    }
    // Fallback to global
    let home = dirs::home_dir().expect("Could not determine home directory");
    let global_dir = home.join(".glass");
    std::fs::create_dir_all(&global_dir).ok();
    global_dir.join("global-history.db")
}
```

### Pattern 3: Connection Setup with WAL
**What:** Open SQLite with WAL mode and performance pragmas.
**When to use:** Every connection open.

```rust
use rusqlite::Connection;

pub fn open_db(path: &Path) -> anyhow::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        PRAGMA foreign_keys = ON;
        PRAGMA busy_timeout = 5000;
    ")?;
    // Run migrations
    create_schema(&conn)?;
    Ok(conn)
}
```

### Pattern 4: Retention / Pruning (HIST-05)
**What:** Delete old records and keep database within size limits.
**When to use:** On database open and periodically (e.g., every 100 inserts).

```rust
pub fn prune(conn: &Connection, max_age_days: u32, max_size_bytes: u64) -> anyhow::Result<u64> {
    let mut total_deleted = 0u64;

    // 1. Age-based pruning
    let cutoff = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64 - (max_age_days as i64 * 86400);

    let deleted = conn.execute(
        "DELETE FROM commands WHERE started_at < ?1",
        [cutoff],
    )?;
    total_deleted += deleted as u64;

    // 2. Size-based pruning (delete oldest until under limit)
    let db_size: u64 = conn.query_row(
        "SELECT page_count * page_size FROM pragma_page_count, pragma_page_size",
        [],
        |row| row.get(0),
    )?;

    if db_size > max_size_bytes {
        // Delete oldest 10% of records
        let count: u64 = conn.query_row(
            "SELECT COUNT(*) FROM commands", [], |row| row.get(0)
        )?;
        let to_delete = (count / 10).max(1);
        let deleted = conn.execute(
            "DELETE FROM commands WHERE id IN (SELECT id FROM commands ORDER BY started_at ASC LIMIT ?1)",
            [to_delete],
        )?;
        total_deleted += deleted as u64;
    }

    // Also delete from FTS if using standard (non-content) FTS table
    // ... (mirror deletes)

    Ok(total_deleted)
}
```

### Pattern 5: Subcommand Routing (INFR-01)
**What:** clap derive with optional subcommand, default = launch terminal.
**When to use:** In `src/main.rs`.

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "glass", about = "GPU-accelerated terminal emulator")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Query command history
    History {
        #[command(subcommand)]
        action: Option<HistoryAction>,
    },
    /// Run the MCP server
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
}

#[derive(Subcommand)]
enum HistoryAction {
    /// Search command history
    Search { query: String },
    // More in Phase 7
}

#[derive(Subcommand)]
enum McpAction {
    /// Start the MCP server
    Serve,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        None => run_terminal(),       // Default: launch GUI terminal
        Some(Commands::History { .. }) => todo!("Phase 7"),
        Some(Commands::Mcp { action }) => match action {
            McpAction::Serve => todo!("Phase 9"),
        },
    }
}
```

### Anti-Patterns to Avoid
- **Using an ORM (diesel, sea-orm):** Overkill for a single-table schema. Raw rusqlite is simpler and has no migration framework dependency.
- **Async SQLite:** rusqlite is synchronous. Wrapping in spawn_blocking adds complexity. Use a dedicated writer thread (std::thread) instead -- same pattern as the PTY reader thread.
- **External content FTS5 tables with manual sync:** Per STATE.md decision, use standard FTS5 tables that store their own copy. External content tables require triggers and are fragile if the app crashes mid-write.
- **Storing timestamps as TEXT:** Integer Unix epoch is faster to compare, sort, and index than ISO 8601 strings.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| SQLite compilation | Custom build scripts | rusqlite `bundled` feature | Cross-platform, includes FTS5, no system SQLite dependency |
| Full-text search | Custom text indexing | SQLite FTS5 | Battle-tested, handles tokenization, ranking (BM25), prefix queries |
| CLI argument parsing | Manual argv parsing | clap derive | Handles help text, error messages, subcommand routing, type validation |
| Home directory | Environment variable parsing | `dirs` crate | Cross-platform (Windows, macOS, Linux) home directory resolution |
| Database migrations | Manual version tracking | Schema version pragma + idempotent CREATE IF NOT EXISTS | Simple enough for 2-3 tables; no migration framework needed |

**Key insight:** The entire database layer is standard SQLite patterns. The only Glass-specific logic is the path resolution (project vs global) and the integration point where BlockManager events feed into database writes.

## Common Pitfalls

### Pitfall 1: FTS5 Table and Main Table Out of Sync
**What goes wrong:** If using external content FTS5 (`content='commands'`), crashes between writing to `commands` and updating the FTS index leave them inconsistent.
**Why it happens:** SQLite triggers fire within the same transaction, but app-level two-step writes might not.
**How to avoid:** Use standard (non-external-content) FTS5 tables per STATE.md decision. INSERT into both `commands` and `commands_fts` in the same transaction.
**Warning signs:** Search returns results that don't exist in the main table, or vice versa.

### Pitfall 2: SQLite Busy Errors
**What goes wrong:** Multiple threads or processes trying to write simultaneously get SQLITE_BUSY errors.
**Why it happens:** SQLite allows only one writer at a time. Without WAL mode, readers also block writers.
**How to avoid:** Enable WAL mode (`PRAGMA journal_mode = WAL`), set `PRAGMA busy_timeout = 5000`, and funnel all writes through a single writer thread/connection.
**Warning signs:** Intermittent "database is locked" errors.

### Pitfall 3: Forgetting to Create `.glass/` Directory
**What goes wrong:** Opening the database fails because the parent directory doesn't exist.
**Why it happens:** `Connection::open()` creates the .db file but not parent directories.
**How to avoid:** Always `std::fs::create_dir_all()` the parent directory before opening.
**Warning signs:** "unable to open database file" errors on first run.

### Pitfall 4: Blocking the Render Thread with DB Writes
**What goes wrong:** Terminal becomes laggy because database writes happen on the event loop thread.
**Why it happens:** SQLite writes can take 1-50ms depending on WAL checkpoint timing.
**How to avoid:** Use a dedicated writer thread with an mpsc channel, same pattern as the existing PTY reader thread. The BlockManager sends completed command records through a channel; the writer thread handles insertion.
**Warning signs:** Visible frame drops after commands complete.

### Pitfall 5: clap Conflicts with winit Event Loop
**What goes wrong:** clap's `parse()` consumes process arguments before winit can see them.
**Why it happens:** Both clap and winit want to inspect argv.
**How to avoid:** Parse clap first, then only start the winit event loop if the terminal mode is selected. winit doesn't use argv on Windows (ConPTY), so this is safe.
**Warning signs:** Unexpected argument errors from either library.

### Pitfall 6: Database Size Check with VACUUM
**What goes wrong:** Running VACUUM to reclaim space locks the database for the entire duration and can take seconds on large databases.
**Why it happens:** VACUUM rebuilds the entire database file.
**How to avoid:** Use `PRAGMA page_count * PRAGMA page_size` to check size without VACUUM. Only VACUUM occasionally (e.g., after large prune operations) or use `PRAGMA auto_vacuum = INCREMENTAL`.
**Warning signs:** Multi-second hangs during pruning.

## Code Examples

### Opening Database and Creating Schema
```rust
// Source: rusqlite docs + SQLite FTS5 docs
use rusqlite::{Connection, params};
use std::path::Path;

pub struct HistoryDb {
    conn: Connection,
}

impl HistoryDb {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA busy_timeout = 5000;
        ")?;
        Self::create_schema(&conn)?;
        Ok(Self { conn })
    }

    fn create_schema(conn: &Connection) -> anyhow::Result<()> {
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS commands (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                command     TEXT NOT NULL,
                cwd         TEXT NOT NULL,
                exit_code   INTEGER,
                started_at  INTEGER NOT NULL,
                finished_at INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                created_at  INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE INDEX IF NOT EXISTS idx_commands_started_at ON commands(started_at);
            CREATE INDEX IF NOT EXISTS idx_commands_cwd ON commands(cwd);

            CREATE VIRTUAL TABLE IF NOT EXISTS commands_fts USING fts5(
                command,
                tokenize='unicode61'
            );
        ")?;
        Ok(())
    }
}
```

### Inserting a Command Record
```rust
// Source: rusqlite docs
pub fn insert_command(
    &self,
    command: &str,
    cwd: &str,
    exit_code: Option<i32>,
    started_at: i64,    // Unix epoch seconds
    finished_at: i64,   // Unix epoch seconds
    duration_ms: i64,
) -> anyhow::Result<i64> {
    let tx = self.conn.unchecked_transaction()?;

    tx.execute(
        "INSERT INTO commands (command, cwd, exit_code, started_at, finished_at, duration_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![command, cwd, exit_code, started_at, finished_at, duration_ms],
    )?;
    let rowid = self.conn.last_insert_rowid();

    // Also insert into FTS table
    tx.execute(
        "INSERT INTO commands_fts (rowid, command) VALUES (?1, ?2)",
        params![rowid, command],
    )?;

    tx.commit()?;
    Ok(rowid)
}
```

### FTS5 Search with BM25 Ranking
```rust
// Source: SQLite FTS5 docs (https://sqlite.org/fts5.html)
use rusqlite::params;

pub struct SearchResult {
    pub id: i64,
    pub command: String,
    pub cwd: String,
    pub exit_code: Option<i32>,
    pub started_at: i64,
    pub finished_at: i64,
    pub duration_ms: i64,
    pub rank: f64,
}

pub fn search(
    &self,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let mut stmt = self.conn.prepare(
        "SELECT c.id, c.command, c.cwd, c.exit_code, c.started_at,
                c.finished_at, c.duration_ms, f.rank
         FROM commands_fts f
         JOIN commands c ON c.id = f.rowid
         WHERE commands_fts MATCH ?1
         ORDER BY f.rank
         LIMIT ?2"
    )?;

    let results = stmt.query_map(params![query, limit as i64], |row| {
        Ok(SearchResult {
            id: row.get(0)?,
            command: row.get(1)?,
            cwd: row.get(2)?,
            exit_code: row.get(3)?,
            started_at: row.get(4)?,
            finished_at: row.get(5)?,
            duration_ms: row.get(6)?,
            rank: row.get(7)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;

    Ok(results)
}
```

### Subcommand Routing in main.rs
```rust
// Source: clap derive tutorial (https://docs.rs/clap/latest/clap/_derive/_tutorial/)
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "glass", version, about = "GPU-accelerated terminal emulator")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Query command history
    History,
    /// MCP server commands
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
}

#[derive(Subcommand)]
enum McpAction {
    /// Start the MCP server over stdio
    Serve,
}

// In main():
fn main() {
    let cli = Cli::parse();
    match cli.command {
        None => {
            // No subcommand = launch terminal (current behavior)
            // ... existing winit event loop code ...
        }
        Some(Commands::History) => {
            eprintln!("glass history: not yet implemented");
            std::process::exit(1);
        }
        Some(Commands::Mcp { action: McpAction::Serve }) => {
            eprintln!("glass mcp serve: not yet implemented");
            std::process::exit(1);
        }
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `content=''` contentless FTS5 | Standard or external content FTS5 | Always (SQLite design) | Contentless can't retrieve values; standard is safer |
| DELETE journal mode | WAL journal mode | SQLite 3.7.0 (2010) | Concurrent reads+writes, better performance |
| Manual SQL string building | Parameterized queries (rusqlite params!) | Always | Prevents SQL injection, correct type handling |
| System SQLite linkage | rusqlite `bundled` feature | rusqlite convention | Consistent behavior across platforms, guaranteed FTS5 |

**Deprecated/outdated:**
- FTS3/FTS4: Superseded by FTS5. FTS5 has better ranking (BM25), better tokenizer support, and is actively maintained.
- rusqlite `functions` feature: Not needed for FTS5; FTS5 ranking is built into SQLite.

## Open Questions

1. **Command text extraction from BlockManager**
   - What we know: BlockManager tracks lifecycle via OSC events and knows prompt_start_line, command_start_line. The actual command TEXT is in the terminal grid, not extracted by BlockManager.
   - What's unclear: How to extract the actual command text string from the terminal grid between command_start_line and output_start_line.
   - Recommendation: When CommandExecuted fires, read the terminal grid from command_start_line to output_start_line to extract command text. This integration logic belongs in the `glass` binary (or `glass_terminal`), not in `glass_history`.

2. **Writer thread ownership**
   - What we know: Database writes must not block the render thread. The project uses dedicated threads (e.g., PTY reader, git query).
   - What's unclear: Whether the writer thread belongs in `glass_history` (as part of the library) or in the `glass` binary.
   - Recommendation: `glass_history` should be a synchronous library. The `glass` binary spawns a writer thread that owns a `HistoryDb` and receives `CommandRecord` structs via mpsc channel. This keeps `glass_history` testable without threading concerns.

3. **Pruning trigger timing**
   - What we know: Pruning must happen automatically per HIST-05.
   - What's unclear: Whether to prune on every insert, on a timer, or on startup.
   - Recommendation: Prune on database open (startup) and every N inserts (e.g., every 100). This avoids timer complexity while keeping the database bounded.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test framework (cargo test) |
| Config file | None needed -- Rust's test framework works out of the box |
| Quick run command | `cargo test -p glass_history` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| HIST-01 | Insert command record with all metadata fields | unit | `cargo test -p glass_history -- insert` | No -- Wave 0 |
| HIST-03 | FTS5 MATCH search returns ranked results | unit | `cargo test -p glass_history -- search` | No -- Wave 0 |
| HIST-04 | Project db path resolved when .glass/ exists; global fallback otherwise | unit | `cargo test -p glass_history -- resolve_db_path` | No -- Wave 0 |
| HIST-05 | Age pruning deletes old records; size pruning keeps db under limit | unit | `cargo test -p glass_history -- prune` | No -- Wave 0 |
| INFR-01 | `glass history` and `glass mcp serve` route correctly; no args launches terminal | integration | `cargo test -p glass -- subcommand` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_history`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_history/src/lib.rs` -- needs actual module structure (currently just a stub comment)
- [ ] `crates/glass_history/Cargo.toml` -- needs rusqlite, dirs, anyhow, tracing dependencies
- [ ] Tests for HIST-01: insert and retrieve a command record
- [ ] Tests for HIST-03: FTS5 search with MATCH syntax
- [ ] Tests for HIST-04: path resolution logic (can test with tempdir)
- [ ] Tests for HIST-05: pruning by age and size
- [ ] Tests for INFR-01: clap subcommand routing in root crate

## Sources

### Primary (HIGH confidence)
- [SQLite FTS5 Extension docs](https://sqlite.org/fts5.html) -- MATCH syntax, content tables, BM25 ranking, tokenizer options
- [rusqlite GitHub build.rs](https://github.com/rusqlite/rusqlite/blob/master/libsqlite3-sys/build.rs) -- confirms `-DSQLITE_ENABLE_FTS5` enabled in bundled builds
- [clap derive tutorial](https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html) -- subcommand derive patterns
- [SQLite WAL docs](https://sqlite.org/wal.html) -- WAL mode benefits and configuration

### Secondary (MEDIUM confidence)
- [SQLite pragma cheatsheet](https://cj.rs/blog/sqlite-pragma-cheatsheet-for-performance-and-consistency/) -- WAL + synchronous NORMAL recommendation
- [clap cargo example](https://docs.rs/clap/latest/clap/_cookbook/cargo_example_derive/) -- real-world subcommand pattern

### Tertiary (LOW confidence)
- None -- all findings verified with primary sources

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- rusqlite already in workspace, FTS5 confirmed in bundled builds, clap is industry standard
- Architecture: HIGH -- standard SQLite patterns, FTS5 well-documented, project already uses dedicated thread pattern
- Pitfalls: HIGH -- SQLite concurrency and FTS sync issues are well-known and documented

**Research date:** 2026-03-05
**Valid until:** 2026-04-05 (stable domain -- SQLite and rusqlite change slowly)

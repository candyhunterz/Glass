# Phase 10: Content Store + DB Schema - Research

**Researched:** 2026-03-05
**Domain:** Content-addressed blob storage, SQLite schema design, terminal grid text extraction
**Confidence:** HIGH

## Summary

Phase 10 builds the data foundation for command-level undo: a content-addressed blob store using BLAKE3 hashing with filesystem-based deduplication, a separate `snapshots.db` SQLite database for snapshot metadata, and command text extraction from the terminal grid at command start time (fixing the empty-string tech debt from v1.1).

The glass_snapshot crate is currently a stub (`//! glass_snapshot -- stub crate, filled in future phases`). This phase fills it with `SnapshotStore` (blob storage + SQLite metadata) and `BlobStore` (content-addressed file storage). The crate must NOT depend on glass_history -- both share the `.glass/` directory but maintain independent databases, following the project decision documented in STATE.md.

SNAP-05 (command text extraction) requires moving the grid text extraction logic from `CommandFinished` to `CommandExecuted` time in `src/main.rs`. The extraction code already exists (lines 630-661 of main.rs) and works correctly -- it just runs at the wrong lifecycle point. Moving it earlier ensures command text is available before execution for the command parser in Phase 11.

**Primary recommendation:** Build `BlobStore` (hash + store + dedup) and `SnapshotDb` (SQLite metadata) as separate modules within glass_snapshot, then move command text extraction to `CommandExecuted` time. Keep the crate isolated -- `command_id` is just an `i64`, no glass_history imports.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SNAP-02 | File contents are stored in a content-addressed blob store using BLAKE3 hashing with deduplication | BlobStore module using blake3 1.8.3, 2-char directory sharding under `.glass/blobs/`, dedup by checking blob existence before write |
| SNAP-05 | Command text is extracted from the terminal grid at command start (fixes empty-string tech debt) | Move existing grid extraction code from CommandFinished handler to CommandExecuted handler in main.rs; store on WindowContext for later use |
| SNAP-06 | Snapshot metadata is stored in a separate snapshots.db with command_id linking to history.db | Separate SQLite file at `.glass/snapshots.db` with own PRAGMA user_version, WAL mode, snapshots + snapshot_files tables |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| blake3 | 1.8.3 | Content-addressed file hashing | 5-14x faster than SHA-256 via SIMD, 256-bit output eliminates collision risk, built-in `.to_hex()`, pure Rust. Standard choice for CAS in Rust ecosystem. |
| rusqlite | 0.38.0 (bundled) | Snapshot metadata database | Already in workspace, bundled SQLite 3.51.1, PRAGMA user_version migrations already established in glass_history |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| anyhow | 1.0.102 (workspace) | Error handling | All fallible operations in glass_snapshot |
| tracing | 0.1.44 (workspace) | Logging | Blob store operations, DB operations |
| dirs | 6 (workspace) | Home directory resolution | Global fallback for `.glass/` directory |
| tempfile | 3 (dev-dependency) | Test infrastructure | All unit tests needing temp directories |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| blake3 | sha2 (SHA-256) | 2-14x slower, no benefit for local non-cryptographic CAS |
| blake3 | xxhash (xxh3) | Only 64/128-bit output -- unacceptable collision risk across thousands of snapshots |
| Separate snapshots.db | Extend history.db | Project decision (STATE.md) is separate DB -- avoids migration risk, enables independent pruning |
| PRAGMA user_version | refinery migration framework | Already using user_version in glass_history, consistency > novelty |

**Installation:**
```toml
# Add to workspace Cargo.toml [workspace.dependencies]
blake3 = "1.8.3"

# glass_snapshot/Cargo.toml [dependencies]
blake3 = { workspace = true }
rusqlite = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
dirs = { workspace = true }

# glass_snapshot/Cargo.toml [dev-dependencies]
tempfile = "3"
```

## Architecture Patterns

### Recommended Project Structure
```
crates/glass_snapshot/
  src/
    lib.rs          # Public API, re-exports
    blob_store.rs   # Content-addressed file storage (BLAKE3 hash, dedup, read/write)
    db.rs           # SQLite snapshots.db schema, migrations, CRUD operations
    types.rs        # Shared types: SnapshotRecord, SnapshotFileRecord
```

### Pattern 1: Content-Addressed Blob Storage
**What:** Hash file contents with BLAKE3, store blob at `{glass_dir}/blobs/{hash[0:2]}/{hash}.blob`, skip write if blob already exists (deduplication).
**When to use:** Every file stored by the snapshot engine.
**Example:**
```rust
// Source: BLAKE3 docs (https://docs.rs/blake3/latest/blake3/)
use blake3::Hasher;
use std::path::{Path, PathBuf};

pub struct BlobStore {
    blob_dir: PathBuf,
}

impl BlobStore {
    pub fn new(glass_dir: &Path) -> Self {
        let blob_dir = glass_dir.join("blobs");
        Self { blob_dir }
    }

    /// Store file contents, returning the BLAKE3 hex hash.
    /// Deduplicates: if blob already exists, skips the write.
    pub fn store_file(&self, source_path: &Path) -> anyhow::Result<(String, u64)> {
        let content = std::fs::read(source_path)?;
        let file_size = content.len() as u64;
        let hash = blake3::hash(&content);
        let hex = hash.to_hex().to_string();

        let shard_dir = self.blob_dir.join(&hex[..2]);
        let blob_path = shard_dir.join(format!("{}.blob", &hex));

        if !blob_path.exists() {
            std::fs::create_dir_all(&shard_dir)?;
            std::fs::write(&blob_path, &content)?;
        }

        Ok((hex, file_size))
    }

    /// Read blob contents by hash.
    pub fn read_blob(&self, hash: &str) -> anyhow::Result<Vec<u8>> {
        let blob_path = self.blob_dir.join(&hash[..2]).join(format!("{}.blob", hash));
        Ok(std::fs::read(&blob_path)?)
    }

    /// Check if a blob exists.
    pub fn blob_exists(&self, hash: &str) -> bool {
        let blob_path = self.blob_dir.join(&hash[..2]).join(format!("{}.blob", hash));
        blob_path.exists()
    }

    /// Delete a blob by hash. Returns true if it existed.
    pub fn delete_blob(&self, hash: &str) -> anyhow::Result<bool> {
        let blob_path = self.blob_dir.join(&hash[..2]).join(format!("{}.blob", hash));
        if blob_path.exists() {
            std::fs::remove_file(&blob_path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
```

### Pattern 2: Separate Database with Consistent Resolution
**What:** snapshots.db lives alongside history.db in the `.glass/` directory. Uses the same ancestor-walk resolution logic. Independent PRAGMA user_version.
**When to use:** Opening the snapshot database at terminal startup or from CLI/MCP.
**Example:**
```rust
// Same pattern as glass_history::resolve_db_path, but for snapshots.db
pub fn resolve_snapshot_db_path(cwd: &Path) -> PathBuf {
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        let glass_dir = d.join(".glass");
        if glass_dir.is_dir() {
            return glass_dir.join("snapshots.db");
        }
        dir = d.parent();
    }
    let home = dirs::home_dir().expect("Could not determine home directory");
    let global_dir = home.join(".glass");
    std::fs::create_dir_all(&global_dir).ok();
    global_dir.join("snapshots.db")
}

// Also need to resolve the glass_dir for blob storage
pub fn resolve_glass_dir(cwd: &Path) -> PathBuf {
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        let glass_dir = d.join(".glass");
        if glass_dir.is_dir() {
            return glass_dir;
        }
        dir = d.parent();
    }
    let home = dirs::home_dir().expect("Could not determine home directory");
    let global_dir = home.join(".glass");
    std::fs::create_dir_all(&global_dir).ok();
    global_dir
}
```

### Pattern 3: Command Text Extraction at CommandExecuted Time
**What:** Move the existing grid text extraction from `CommandFinished` to `CommandExecuted` handler, storing the extracted text on `WindowContext` for use by both history DB insert and future snapshot/parser operations.
**When to use:** On every `ShellEvent::CommandExecuted` event.
**Key insight:** The command text is still visible in the terminal grid at CommandExecuted time because the prompt line has not scrolled away yet. The same `block.command_start_line` to `block.output_start_line` range works.
**Example:**
```rust
// In Processor::user_event, ShellEvent::CommandExecuted handler:
if matches!(shell_event, ShellEvent::CommandExecuted) {
    ctx.command_started_wall = Some(std::time::SystemTime::now());

    // NEW: Extract command text NOW (at command start)
    let command_text = {
        let blocks = ctx.block_manager.blocks();
        if let Some(block) = blocks.last() {
            let start = block.command_start_line;
            let end = block.output_start_line
                .map(|o| o.max(start + 1))
                .unwrap_or(start + 1);
            let term_guard = ctx.term.lock();
            // ... same grid extraction logic as current CommandFinished ...
            text.trim().to_string()
        } else {
            String::new()
        }
    };
    ctx.pending_command_text = Some(command_text); // NEW field on WindowContext
}

// In CommandFinished handler, use stored text instead of re-extracting:
let command_text = ctx.pending_command_text.take().unwrap_or_default();
```

### Anti-Patterns to Avoid
- **glass_snapshot depending on glass_history:** The command_id is just an `i64`. Do not import `HistoryDb` or `CommandRecord`. The root binary coordinates both crates.
- **Storing blob contents in SQLite:** SQLite performance degrades with large BLOBs (>100KB per SQLite's own guidance). Store blobs on the filesystem, hashes in SQLite.
- **Forgetting PRAGMA foreign_keys = ON:** SQLite has foreign keys OFF by default. The snapshots.db schema uses `REFERENCES` and `ON DELETE CASCADE` which silently do nothing without this PRAGMA.
- **Hex string hashes in indexes:** Store BLAKE3 hashes as TEXT (hex) in SQLite. The 64-char hex string is fine for this use case -- it allows easy debugging via `sqlite3` CLI. BLOB storage of raw 32-byte hash saves 50% space per hash but makes debugging harder. At our scale (thousands of snapshots, not millions), TEXT is the right tradeoff.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Content hashing | Custom hash function | blake3 crate | SIMD-accelerated, collision-resistant, well-tested |
| DB path resolution | New resolution logic | Clone glass_history's 15-line `resolve_db_path` pattern | Consistency with existing crate, same `.glass/` directory |
| Schema migrations | Migration framework | PRAGMA user_version (existing pattern) | Already proven in glass_history v0->v1 migration |
| Atomic file writes | Manual temp-file-then-rename | std::fs::write (sufficient for blobs) | Blobs are write-once, content-addressed. A partial write produces a different hash and is never referenced. |

**Key insight:** The CAS design makes many atomicity concerns disappear. A partially-written blob has the wrong hash, so it will never be referenced by any snapshot_files row. Orphaned blobs are cleaned up during pruning.

## Common Pitfalls

### Pitfall 1: Foreign Keys Silently Disabled
**What goes wrong:** `ON DELETE CASCADE` on snapshot_files does nothing because SQLite foreign keys are off by default.
**Why it happens:** Historical SQLite backwards compatibility. `PRAGMA foreign_keys` defaults to OFF.
**How to avoid:** Add `PRAGMA foreign_keys = ON;` to the connection setup, alongside WAL mode and busy_timeout.
**Warning signs:** Deleting a snapshot row leaves orphaned snapshot_files rows. Tests pass because they never test cascading deletes.

### Pitfall 2: TOCTOU in Blob Dedup Check
**What goes wrong:** Checking `blob_path.exists()` then calling `std::fs::write()` has a race window. Two concurrent snapshot operations for the same file could both check, both find no blob, and both write.
**Why it happens:** File existence check and write are not atomic.
**How to avoid:** This is actually harmless for CAS. Both writes produce identical content (same hash = same content). The second write overwrites with the same bytes. No corruption possible. Do not over-engineer this with file locking.
**Warning signs:** None -- this is a non-issue for CAS by design.

### Pitfall 3: Command Text Empty at CommandExecuted
**What goes wrong:** At `CommandExecuted` time, `block.output_start_line` might not be set yet (it gets set by the `CommandExecuted` event handler itself).
**Why it happens:** The block state transitions in `block_manager.handle_event()` -- need to verify the output_start_line is set before or after the handler runs.
**How to avoid:** Extract command text AFTER `block_manager.handle_event()` has processed `CommandExecuted`. The current code does this correctly for `CommandFinished`. Verify in block_manager.rs that `output_start_line` is set in the `CommandExecuted` handler (line 100-110 of block_manager.rs confirms: `block.output_start_line = Some(line)` is set in CommandExecuted).
**Warning signs:** Empty command text in snapshot records despite visible command in terminal.

### Pitfall 4: Glass Directory Does Not Exist Yet
**What goes wrong:** First-time user has no `.glass/` directory. The blob store and snapshot DB both need the directory to exist.
**Why it happens:** `.glass/` is created on demand by glass_history's `resolve_db_path` which calls `create_dir_all`. But glass_snapshot resolves independently.
**How to avoid:** Both `resolve_snapshot_db_path` and `BlobStore::new` should `create_dir_all` as needed, matching the glass_history pattern. The `SnapshotDb::open` method should create parent directories, just like `HistoryDb::open` does.
**Warning signs:** "No such file or directory" errors on first terminal launch in a new project.

## Code Examples

### SnapshotDb Schema and Setup
```rust
// Source: glass_history db.rs pattern + project ARCHITECTURE.md schema
const SCHEMA_VERSION: i64 = 1;

pub struct SnapshotDb {
    conn: rusqlite::Connection,
}

impl SnapshotDb {
    pub fn open(path: &std::path::Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = rusqlite::Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA foreign_keys = ON;"
        )?;
        Self::create_schema(&conn)?;
        Self::migrate(&conn)?;
        Ok(Self { conn })
    }

    fn create_schema(conn: &rusqlite::Connection) -> anyhow::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS snapshots (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                command_id  INTEGER NOT NULL,
                cwd         TEXT NOT NULL,
                created_at  INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE INDEX IF NOT EXISTS idx_snapshots_command ON snapshots(command_id);

            CREATE TABLE IF NOT EXISTS snapshot_files (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
                file_path   TEXT NOT NULL,
                blob_hash   TEXT,
                file_size   INTEGER,
                source      TEXT NOT NULL DEFAULT 'parser'
            );
            CREATE INDEX IF NOT EXISTS idx_sf_snapshot ON snapshot_files(snapshot_id);
            CREATE INDEX IF NOT EXISTS idx_sf_hash ON snapshot_files(blob_hash);"
        )?;
        Ok(())
    }

    fn migrate(conn: &rusqlite::Connection) -> anyhow::Result<()> {
        let version: i64 =
            conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version < 1 {
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(())
    }
}
```

### SnapshotStore (Coordinating BlobStore + SnapshotDb)
```rust
/// High-level API combining blob storage and metadata.
pub struct SnapshotStore {
    db: SnapshotDb,
    blobs: BlobStore,
}

impl SnapshotStore {
    pub fn open(glass_dir: &std::path::Path) -> anyhow::Result<Self> {
        let db = SnapshotDb::open(&glass_dir.join("snapshots.db"))?;
        let blobs = BlobStore::new(glass_dir);
        Ok(Self { db, blobs })
    }

    pub fn create_snapshot(&self, command_id: i64, cwd: &str) -> anyhow::Result<i64> {
        self.db.create_snapshot(command_id, cwd)
    }

    pub fn store_file(&self, snapshot_id: i64, path: &std::path::Path, source: &str) -> anyhow::Result<()> {
        if !path.exists() {
            // File does not exist -- record NULL hash (file was absent before command)
            self.db.insert_snapshot_file(snapshot_id, path, None, None, source)?;
            return Ok(());
        }
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.is_symlink() {
            tracing::debug!("Skipping symlink: {}", path.display());
            return Ok(());
        }
        let (hash, size) = self.blobs.store_file(path)?;
        self.db.insert_snapshot_file(snapshot_id, path, Some(&hash), Some(size), source)?;
        Ok(())
    }

    pub fn update_command_id(&self, snapshot_id: i64, command_id: i64) -> anyhow::Result<()> {
        self.db.update_command_id(snapshot_id, command_id)
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| SHA-256 for CAS | BLAKE3 for CAS | blake3 1.0 (2020) | 5-14x faster hashing, same security margin for local dedup |
| SQLite BLOBs for file content | Filesystem blobs with SQLite hash references | SQLite guidance (2017+) | Better performance for files >100KB, independent storage management |
| Command text at CommandFinished | Command text at CommandExecuted | This phase | Enables pre-exec command parsing for snapshot targeting |

**Deprecated/outdated:**
- None relevant to this phase's scope.

## Open Questions

1. **Hash storage format: TEXT vs BLOB in SQLite**
   - What we know: BLAKE3 produces 32 bytes. Hex encoding = 64 chars TEXT. Raw = 32 bytes BLOB.
   - What's unclear: Performance difference at our scale (thousands, not millions of rows).
   - Recommendation: Use TEXT (hex). Easier debugging with `sqlite3` CLI, negligible performance difference at our scale. Consistent with git's hex hash display convention.

2. **Should blob_hash be nullable for "file did not exist" vs separate sentinel?**
   - What we know: Some snapshot_files entries represent "this file did not exist before the command" (e.g., file was created by the command). On undo, these files should be deleted.
   - What's unclear: NULL hash vs empty string vs sentinel value.
   - Recommendation: Use NULL. It is semantically correct (no content = no hash) and SQLite handles NULL indexing efficiently. The `source` column still indicates how the entry was recorded.

3. **fs_changes table: include now or defer to Phase 12?**
   - What we know: The ARCHITECTURE.md schema includes an `fs_changes` table for FS watcher events. Phase 10 scope is content store + DB schema. Phase 12 is FS watcher.
   - What's unclear: Whether to create the table now (schema completeness) or in Phase 12 (when it's actually used).
   - Recommendation: Defer to Phase 12. Create only the tables needed now (`snapshots`, `snapshot_files`). The migration system supports adding tables later. Avoids creating unused schema.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[cfg(test)]` + tempfile 3 |
| Config file | None -- Rust's built-in test harness |
| Quick run command | `cargo test -p glass_snapshot` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SNAP-02 (dedup) | Store same file twice, only one blob on disk | unit | `cargo test -p glass_snapshot -- blob_store::tests::test_dedup -x` | Wave 0 |
| SNAP-02 (hash) | BLAKE3 hash matches expected value for known content | unit | `cargo test -p glass_snapshot -- blob_store::tests::test_hash_correctness -x` | Wave 0 |
| SNAP-02 (read) | Stored blob can be read back with identical content | unit | `cargo test -p glass_snapshot -- blob_store::tests::test_store_and_read -x` | Wave 0 |
| SNAP-05 (extract) | Command text extracted at CommandExecuted time is non-empty | integration | Manual -- requires terminal + shell integration | Manual |
| SNAP-06 (persist) | Snapshot metadata survives DB close and reopen | unit | `cargo test -p glass_snapshot -- db::tests::test_persistence -x` | Wave 0 |
| SNAP-06 (schema) | Schema creates snapshots + snapshot_files tables with correct columns | unit | `cargo test -p glass_snapshot -- db::tests::test_schema_creation -x` | Wave 0 |
| SNAP-06 (link) | Snapshot records link to command_id correctly | unit | `cargo test -p glass_snapshot -- db::tests::test_command_id_link -x` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_snapshot`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_snapshot/src/blob_store.rs` -- BlobStore implementation with tests
- [ ] `crates/glass_snapshot/src/db.rs` -- SnapshotDb implementation with tests
- [ ] `crates/glass_snapshot/src/types.rs` -- Shared types
- [ ] `crates/glass_snapshot/src/lib.rs` -- Module declarations and re-exports
- [ ] `crates/glass_snapshot/Cargo.toml` -- Add blake3, rusqlite, anyhow, tracing, dirs dependencies
- [ ] Root `Cargo.toml` -- Add `blake3 = "1.8.3"` to workspace dependencies
- [ ] tempfile dev-dependency already available in workspace

## Sources

### Primary (HIGH confidence)
- Glass v1.1 source code -- Direct analysis of glass_history/src/db.rs (schema, migrations, PRAGMA patterns), glass_snapshot/src/lib.rs (stub), src/main.rs (command text extraction at lines 630-661)
- [blake3 1.8.3 docs.rs](https://docs.rs/blake3/latest/blake3/) -- Hasher API, `hash()` convenience function, `Hash::to_hex()`
- [blake3 crates.io](https://crates.io/crates/blake3) -- Version 1.8.3, MSRV 1.85
- [rusqlite 0.38.0 docs.rs](https://docs.rs/crate/rusqlite/latest) -- Bundled SQLite 3.51.1
- [SQLite foreign keys](https://sqlite.org/foreignkeys.html) -- PRAGMA foreign_keys must be enabled per-connection
- Glass .planning/research/ARCHITECTURE.md -- Schema design for snapshots, snapshot_files, fs_changes tables
- Glass .planning/research/STACK.md -- BLAKE3 integration pattern, workspace dependency plan
- Glass .planning/research/PITFALLS.md -- TOCTOU, storage growth, schema migration risks

### Secondary (MEDIUM confidence)
- Glass .planning/STATE.md -- Decision: "Separate snapshots.db from history.db -- avoids migration risk, independent pruning"
- [SQLite Internal vs External BLOBs](https://sqlite.org/intern-v-extern-blob.html) -- >100KB threshold for filesystem storage

### Tertiary (LOW confidence)
- None.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- blake3 and rusqlite are established, versions verified, already used/planned in project
- Architecture: HIGH -- follows existing patterns from glass_history exactly, decisions locked in STATE.md
- Pitfalls: HIGH -- directly analyzed from existing codebase and SQLite documentation

**Research date:** 2026-03-05
**Valid until:** 2026-04-05 (stable domain, no fast-moving dependencies)

# Phase 18: Storage + Retention - Research

**Researched:** 2026-03-06
**Domain:** SQLite schema migration, data persistence for pipeline stages, retention cascading
**Confidence:** HIGH

## Summary

Phase 18 adds persistent storage for pipeline stage data in the existing `history.db` SQLite database. The core task is creating a `pipe_stages` child table linked to `commands.id`, migrating the schema from version 1 to version 2 without data loss, wiring the insert logic into the existing `CommandFinished` event flow, and extending the retention/pruning code to cascade deletes to the new table.

The project already has well-established patterns for all of these concerns: the `glass_history` crate uses `PRAGMA user_version` for migrations (v0->v1 added the `output` column), `retention.rs` performs age-based and size-based pruning with per-id FTS cleanup, and the `glass_snapshot` crate demonstrates parent-child table design with `ON DELETE CASCADE`. The FinalizedBuffer enum in `glass_pipes::types` has three variants (Complete, Sampled, Binary) that need distinct serialization strategies for storage.

**Primary recommendation:** Add a `pipe_stages` table with a foreign key to `commands(id)` using `ON DELETE CASCADE`, bump `SCHEMA_VERSION` to 2, store stage data as TEXT (UTF-8 output) or BLOB (binary placeholder), and add a single `DELETE FROM pipe_stages WHERE command_id IN (...)` call in the pruning loop before parent command deletion.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| STOR-01 | Pipe stage data stored in `pipe_stages` table linked to command_id in history.db | Schema design (pipe_stages table), migration v1->v2, insert_pipe_stages() method, wiring in CommandFinished event flow |
| STOR-02 | Stage data included in retention/pruning policies | ON DELETE CASCADE on FK, explicit DELETE in pruning loop for safety, cascade verification tests |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| rusqlite | 0.38.0 (bundled) | SQLite database operations | Already used throughout glass_history; bundled feature avoids system SQLite dependency |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| anyhow | workspace | Error handling | All glass_history functions return `anyhow::Result` |
| tempfile | 3 | Test database creation | Already used in glass_history dev-dependencies |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| ON DELETE CASCADE | Manual deletion in pruning code only | CASCADE is safer (no orphans from direct SQL), but explicit delete in pruner is also needed since history.db does NOT currently enable foreign_keys pragma |

**Installation:**
No new dependencies needed. All required crates are already in the workspace.

## Architecture Patterns

### Recommended Project Structure
Changes within existing files:
```
crates/glass_history/
  src/
    db.rs            # Schema v2 migration, insert_pipe_stages(), get_pipe_stages()
    retention.rs     # Add pipe_stages deletion before commands deletion
    lib.rs           # Re-export new types (PipeStageRecord)
src/
  main.rs            # Wire insert_pipe_stages() call after insert_command() on CommandFinished
```

### Pattern 1: Schema Migration via user_version Pragma
**What:** The project uses `PRAGMA user_version` to track schema versions and apply migrations sequentially.
**When to use:** Every time a new table or column is added to the database.
**Example (existing v0->v1 pattern):**
```rust
// Source: crates/glass_history/src/db.rs lines 78-98
const SCHEMA_VERSION: i64 = 1; // WILL BECOME 2

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 =
        conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if version < 1 {
        // v0->v1: add output column
        let has_output: bool = conn
            .prepare("SELECT output FROM commands LIMIT 0")
            .is_ok();
        if !has_output {
            conn.execute_batch("ALTER TABLE commands ADD COLUMN output TEXT;")?;
        }
        conn.pragma_update(None, "user_version", 1)?;
    }

    // NEW: if version < 2 { ... create pipe_stages table ... }

    Ok(())
}
```

### Pattern 2: Child Table with Foreign Key (from glass_snapshot)
**What:** The `snapshot_files` table uses `REFERENCES snapshots(id) ON DELETE CASCADE`.
**When to use:** When child rows should be automatically cleaned up when parent is deleted.
**Example (from snapshots.db):**
```rust
// Source: crates/glass_snapshot/src/db.rs lines 36-54
conn.execute_batch(
    "CREATE TABLE IF NOT EXISTS snapshot_files (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
        file_path   TEXT NOT NULL,
        blob_hash   TEXT,
        ...
    );"
);
```

### Pattern 3: Insert in Transaction (from insert_command)
**What:** Related inserts are wrapped in a transaction to ensure atomicity.
**When to use:** When inserting into multiple tables that should succeed or fail together.
**Example:**
```rust
// Source: crates/glass_history/src/db.rs lines 101-123
pub fn insert_command(&self, record: &CommandRecord) -> Result<i64> {
    let tx = self.conn.unchecked_transaction()?;
    tx.execute("INSERT INTO commands ...", params![...])?;
    let rowid = tx.last_insert_rowid();
    tx.execute("INSERT INTO commands_fts ...", params![rowid, ...])?;
    tx.commit()?;
    Ok(rowid)
}
```

### Pattern 4: Pruning with FTS Cleanup (from retention.rs)
**What:** When deleting command records, both the main table and the FTS table must be cleaned.
**When to use:** The same pattern extends to pipe_stages -- delete child rows before parent rows.
**Example (current pruning):**
```rust
// Source: crates/glass_history/src/retention.rs lines 30-43
let tx = conn.unchecked_transaction()?;
for &id in &ids_to_delete {
    tx.execute("DELETE FROM commands_fts WHERE rowid = ?1", params![id])?;
}
for &id in &ids_to_delete {
    tx.execute("DELETE FROM commands WHERE id = ?1", params![id])?;
}
tx.commit()?;
```

### Anti-Patterns to Avoid
- **Relying solely on CASCADE for pruning:** The current history.db does NOT set `PRAGMA foreign_keys = ON` (unlike snapshots.db). Must either (a) enable the pragma, or (b) explicitly delete pipe_stages rows in pruning code. Safest: do both.
- **Storing raw bytes as BLOB for text output:** Pipeline output is text (ANSI-stripped). Store as TEXT for queryability. Only use a size placeholder for Binary variants.
- **Storing FinalizedBuffer's Vec<u8> head/tail separately in Sampled mode:** For Sampled output, concatenate head + "[... N bytes omitted ...]" + tail into a single TEXT column at insert time. The database is for historical lookup, not in-memory rendering reconstruction.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Schema versioning | Custom version tracking | `PRAGMA user_version` | Already used, battle-tested, atomic |
| Transaction management | Manual BEGIN/COMMIT | `conn.unchecked_transaction()` | Existing pattern, auto-rollback on drop |
| Child row cleanup | Background sweeper | ON DELETE CASCADE + explicit delete in pruner | Both mechanisms ensure no orphans |
| Binary detection | New detection logic | `glass_pipes::FinalizedBuffer::Binary` variant | Already classified at capture time |

**Key insight:** All infrastructure for this phase already exists in the codebase. This is a composition task, not a novel engineering task.

## Common Pitfalls

### Pitfall 1: Foreign Keys Disabled in history.db
**What goes wrong:** `ON DELETE CASCADE` silently does nothing because `PRAGMA foreign_keys` is not enabled.
**Why it happens:** The `HistoryDb::open()` method sets WAL mode and busy_timeout, but NOT `foreign_keys = ON`. The `SnapshotDb::open()` does enable it.
**How to avoid:** Add `PRAGMA foreign_keys = ON;` to the `HistoryDb::open()` pragma batch. This is safe for existing databases since there are no existing foreign key constraints.
**Warning signs:** Orphaned pipe_stages rows remaining after parent command deletion.

### Pitfall 2: Migration Must Be Idempotent
**What goes wrong:** Running migration twice (e.g., on reopened database) causes errors if table already exists.
**Why it happens:** `CREATE TABLE` without `IF NOT EXISTS` fails on second run.
**How to avoid:** Always use `CREATE TABLE IF NOT EXISTS` in migration code (matching existing pattern in `create_schema`).
**Warning signs:** "table already exists" errors on database reopen.

### Pitfall 3: FinalizedBuffer Serialization
**What goes wrong:** Trying to store `Vec<u8>` directly as TEXT causes encoding errors for non-UTF-8 data.
**Why it happens:** FinalizedBuffer::Complete contains raw bytes that may not be valid UTF-8.
**How to avoid:** Use `String::from_utf8_lossy()` to convert bytes to text at storage time. For Binary variants, store just a size placeholder string like `[binary: N bytes]`.
**Warning signs:** rusqlite UTF-8 errors, garbled data in queries.

### Pitfall 4: Pruning Must Delete pipe_stages BEFORE commands
**What goes wrong:** If foreign_keys is enabled but pruning deletes commands first without CASCADE, the FK constraint blocks deletion.
**Why it happens:** Order of operations matters with FK constraints when CASCADE is not set.
**How to avoid:** With CASCADE enabled, the order doesn't matter (cascade handles it). Without CASCADE, delete children first. Best practice: enable CASCADE AND explicitly delete children first in pruner (belt and suspenders).
**Warning signs:** "FOREIGN KEY constraint failed" errors during pruning.

### Pitfall 5: Not All Commands Have Pipeline Stages
**What goes wrong:** Trying to insert empty stage vectors or wasting queries on non-pipeline commands.
**Why it happens:** Most commands are single-stage (no pipes). Only pipeline commands have stages.
**How to avoid:** Check `block.pipeline_stages.is_empty()` before attempting pipe_stages inserts.
**Warning signs:** Millions of empty-query roundtrips, unnecessary database load.

## Code Examples

### Pipe Stages Table Schema
```sql
-- Source: Designed for this phase following existing patterns
CREATE TABLE IF NOT EXISTS pipe_stages (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    command_id  INTEGER NOT NULL REFERENCES commands(id) ON DELETE CASCADE,
    stage_index INTEGER NOT NULL,
    command     TEXT NOT NULL,
    output      TEXT,
    total_bytes INTEGER NOT NULL,
    is_binary   INTEGER NOT NULL DEFAULT 0,
    is_sampled  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_pipe_stages_command ON pipe_stages(command_id);
```

### insert_pipe_stages Method
```rust
// Follows existing insert_command pattern
pub fn insert_pipe_stages(
    &self,
    command_id: i64,
    stages: &[CapturedStage],
    stage_commands: &[String],
) -> Result<()> {
    if stages.is_empty() {
        return Ok(());
    }
    let tx = self.conn.unchecked_transaction()?;
    for stage in stages {
        let cmd_text = stage_commands
            .get(stage.index)
            .map(|s| s.as_str())
            .unwrap_or("");
        let (output, total_bytes, is_binary, is_sampled) = match &stage.data {
            FinalizedBuffer::Complete(data) => {
                let text = String::from_utf8_lossy(data).into_owned();
                (Some(text), data.len() as i64, false, false)
            }
            FinalizedBuffer::Sampled { head, tail, total_bytes } => {
                let head_text = String::from_utf8_lossy(head);
                let tail_text = String::from_utf8_lossy(tail);
                let combined = format!(
                    "{}\n[...{} bytes omitted...]\n{}",
                    head_text,
                    total_bytes - head.len() - tail.len(),
                    tail_text
                );
                (Some(combined), *total_bytes as i64, false, true)
            }
            FinalizedBuffer::Binary { size } => {
                (None, *size as i64, true, false)
            }
        };
        tx.execute(
            "INSERT INTO pipe_stages (command_id, stage_index, command, output, total_bytes, is_binary, is_sampled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![command_id, stage.index as i64, cmd_text, output, total_bytes, is_binary, is_sampled],
        )?;
    }
    tx.commit()?;
    Ok(())
}
```

### Migration v1 to v2
```rust
// Source: extends existing migrate() in db.rs
if version < 2 {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS pipe_stages (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            command_id  INTEGER NOT NULL REFERENCES commands(id) ON DELETE CASCADE,
            stage_index INTEGER NOT NULL,
            command     TEXT NOT NULL,
            output      TEXT,
            total_bytes INTEGER NOT NULL,
            is_binary   INTEGER NOT NULL DEFAULT 0,
            is_sampled  INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_pipe_stages_command ON pipe_stages(command_id);"
    )?;
    conn.pragma_update(None, "user_version", 2)?;
}
```

### Pruning Extension
```rust
// Add before the existing command deletion loops in retention.rs
// Delete pipe_stages for commands about to be pruned
for &id in &ids_to_delete {
    tx.execute(
        "DELETE FROM pipe_stages WHERE command_id = ?1",
        params![id],
    )?;
}
```

### Wiring in main.rs (CommandFinished handler)
```rust
// After successful insert_command() call, persist pipeline stages
match db.insert_command(&record) {
    Ok(id) => {
        ctx.last_command_id = Some(id);
        tracing::debug!("Inserted command record id={}", id);

        // Persist pipeline stage data if present
        if let Some(block) = ctx.block_manager.blocks().last() {
            if !block.pipeline_stages.is_empty() {
                if let Err(e) = db.insert_pipe_stages(
                    id,
                    &block.pipeline_stages,
                    &block.pipeline_stage_commands,
                ) {
                    tracing::warn!("Failed to insert pipe stages: {}", e);
                }
            }
        }
    }
    // ...
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| No pipe_stages table | pipe_stages linked to commands | This phase | Pipeline data survives terminal restart |
| SCHEMA_VERSION = 1 | SCHEMA_VERSION = 2 | This phase | Migration path for existing databases |
| Pruning only cleans commands + FTS | Pruning also cleans pipe_stages | This phase | No orphaned stage data accumulates |

**Current state of the codebase:**
- `HistoryDb` at SCHEMA_VERSION 1 (output column exists)
- `retention.rs` prunes commands + commands_fts by age and size
- Snapshot pruning runs at startup (background thread), but history pruning is NOT currently called at startup (only `db.prune()` exists as a method, never invoked in main.rs). This is existing tech debt noted in STATE.md as "pruner.rs max_size_mb not enforced"
- `PRAGMA foreign_keys` is NOT enabled in history.db (it IS in snapshots.db)

## Open Questions

1. **Should history pruning be wired at startup?**
   - What we know: `HistoryDb::prune()` exists but is never called in main.rs. Snapshot pruning runs at startup.
   - What's unclear: Whether this was intentional or an oversight from earlier phases.
   - Recommendation: Out of scope for this phase (STOR-02 says "cascade to pipe_stages when parent commands are pruned" -- the mechanism must work, but whether pruning runs is a separate concern). Flag for Phase 19 polish.

2. **get_pipe_stages() for Phase 19**
   - What we know: Phase 19 needs `GlassPipeInspect(command_id, stage)` which requires reading pipe_stages back.
   - What's unclear: Whether to add the read method now or in Phase 19.
   - Recommendation: Add `get_pipe_stages(command_id)` in this phase alongside the insert method. It's trivial and lets Phase 19 focus on MCP wiring.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + tempfile for database fixtures |
| Config file | None (Cargo.toml `[dev-dependencies]` only) |
| Quick run command | `cargo test -p glass_history` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| STOR-01 | pipe_stages table created by migration | unit | `cargo test -p glass_history -- test_migration_v1_to_v2 -x` | Wave 0 |
| STOR-01 | Existing data survives v1->v2 migration | unit | `cargo test -p glass_history -- test_existing_records_survive_v2_migration -x` | Wave 0 |
| STOR-01 | insert_pipe_stages stores and retrieves stages | unit | `cargo test -p glass_history -- test_insert_and_get_pipe_stages -x` | Wave 0 |
| STOR-01 | Empty pipeline produces no pipe_stages rows | unit | `cargo test -p glass_history -- test_no_pipe_stages_for_simple_command -x` | Wave 0 |
| STOR-01 | FinalizedBuffer variants stored correctly | unit | `cargo test -p glass_history -- test_pipe_stage_buffer_variants -x` | Wave 0 |
| STOR-02 | Age-based pruning cascades to pipe_stages | unit | `cargo test -p glass_history -- test_prune_cascades_to_pipe_stages -x` | Wave 0 |
| STOR-02 | Size-based pruning cascades to pipe_stages | unit | `cargo test -p glass_history -- test_size_prune_cascades_to_pipe_stages -x` | Wave 0 |
| STOR-02 | delete_command cascades to pipe_stages | unit | `cargo test -p glass_history -- test_delete_command_cascades_pipe_stages -x` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_history`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- None of the pipe_stages tests exist yet -- they are all new test functions
- No framework gaps -- existing test infrastructure (tempfile, in-memory SQLite) covers all needs
- No new dev-dependencies needed

## Sources

### Primary (HIGH confidence)
- `crates/glass_history/src/db.rs` -- current schema, migration pattern, SCHEMA_VERSION, insert/delete/update methods
- `crates/glass_history/src/retention.rs` -- pruning logic (age-based + size-based), FTS cleanup
- `crates/glass_history/src/lib.rs` -- module structure, public API surface
- `crates/glass_pipes/src/types.rs` -- CapturedStage, FinalizedBuffer enum variants
- `crates/glass_terminal/src/block_manager.rs` -- Block struct with pipeline_stages field
- `crates/glass_snapshot/src/db.rs` -- ON DELETE CASCADE pattern for child tables
- `src/main.rs` -- CommandFinished handler (line ~950), pipeline stage temp file reading (line ~810)

### Secondary (MEDIUM confidence)
- `crates/glass_history/src/config.rs` -- HistoryConfig with max_age_days, max_size_bytes
- `crates/glass_core/src/config.rs` -- GlassConfig, HistorySection (no retention config fields currently)
- `.planning/STATE.md` -- Tech debt note about "pruner.rs max_size_mb not enforced"

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - uses only existing crates, no new dependencies
- Architecture: HIGH - all patterns already established in codebase (migrations, FK cascade, pruning)
- Pitfalls: HIGH - identified from direct code inspection (foreign_keys pragma gap, migration idempotency, FinalizedBuffer serialization)

**Research date:** 2026-03-06
**Valid until:** 2026-04-06 (stable -- internal codebase patterns, no external API dependencies)

# Phase 49: SOI Storage Schema - Research

**Researched:** 2026-03-12
**Domain:** SQLite schema migration, rusqlite, glass_history crate extension
**Confidence:** HIGH

## Summary

Phase 49 adds two new SQLite tables — `command_output_records` and `output_records` — to the existing history database, running automatically on startup via the established `PRAGMA user_version` migration pattern. The `OutputRecord` enum from `glass_soi` must be serialized to JSON for storage, linked to the `commands` table via `command_id`, and pruned in lockstep with the existing retention logic.

The work lives entirely in `glass_history`. The `glass_soi` crate already provides all the types needed (`ParsedOutput`, `OutputRecord`, `Severity`, `OutputType`) — no changes to `glass_soi` are required in this phase. The schema design follows the exact pattern established by `pipe_stages` in migration v2: a child table with `ON DELETE CASCADE`, an index on `command_id`, explicit deletion before pruning the parent, and a `PRAGMA user_version` bump to 3.

**Primary recommendation:** Add two tables in migration v3 — one per-command summary row (`command_output_records`) and one per-record detail row (`output_records`) — with the `OutputRecord` variant serialized as JSON. Use `ON DELETE CASCADE` plus explicit pre-deletion in `retention.rs` to keep pruning consistent with the existing pattern.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SOIS-01 | Parsed output records persist in SQLite tables (`command_output_records`, `output_records`) linked to existing `commands` table | Schema design section covers both tables, FK to `commands(id)` |
| SOIS-02 | Schema migration from v2 to v3 runs automatically on startup using existing `PRAGMA user_version` pattern | Migration pattern section covers exact `if version < 3` guard |
| SOIS-03 | Individual records queryable by `command_id`, severity, file path, and record type | Query design section covers indexes and query helper design |
| SOIS-04 | Retention/pruning of SOI records cascades with existing history retention policies | Retention section covers explicit deletion + CASCADE |
</phase_requirements>

---

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| rusqlite | 0.38.0 (workspace) | SQLite access, schema migration, transactions | Already the project's DB layer; bundled SQLite with FTS5 |
| serde_json | 1.0 (already in glass_soi) | Serialize `OutputRecord` enum variants to TEXT column | `OutputRecord` already derives `Serialize`; JSON is portable |
| anyhow | workspace | Error propagation in DB operations | Project-wide convention |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tempfile | 3 (dev-dep) | In-process temporary DB for tests | All `glass_history` tests use this pattern |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| JSON TEXT column for `OutputRecord` | Separate column per variant | JSON is simpler, schema stays fixed as new record variants are added; column-per-variant would require schema changes for every new `OutputRecord` arm |
| `ON DELETE CASCADE` alone | Explicit pre-deletion loop in retention | Project already uses both (see `pipe_stages` — explicit loop in `retention.rs` PLUS cascade as belt-and-suspenders); follow the same pattern for consistency |
| Storing all data in one flat table | Two-table design (summary + records) | Two tables separate the per-command summary (one row, always fast) from the per-command record list (N rows, indexed); aligns with SOIS-01 table names from requirements |

**No new dependencies needed.** `glass_history` already has `rusqlite` and `serde`. Add `glass_soi` as a dependency (for `ParsedOutput`/`OutputRecord` types) and `serde_json` for serialization.

---

## Architecture Patterns

### Recommended Module Structure

The implementation extends `glass_history` only. No new crate is needed.

```
crates/glass_history/src/
├── db.rs            # Add: v3 migration, insert_parsed_output, get_output_records
├── retention.rs     # Add: explicit SOI deletion in both prune paths
├── soi.rs           # New: SoiRecord row type, insert/query helpers
└── lib.rs           # Add: pub use soi::SoiRecord, re-export insert/query
```

### Pattern 1: Migration v3 — Two New Tables

**What:** `create_schema` is idempotent (uses `CREATE TABLE IF NOT EXISTS`). The v3 migration block only runs when `user_version < 3`.

**When to use:** Startup — `HistoryDb::open` calls `migrate` after `create_schema`.

```rust
// In migrate(), after the existing `if version < 2` block:
if version < 3 {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS command_output_records (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            command_id      INTEGER NOT NULL REFERENCES commands(id) ON DELETE CASCADE,
            output_type     TEXT NOT NULL,
            severity        TEXT NOT NULL,
            one_line        TEXT NOT NULL,
            token_estimate  INTEGER NOT NULL,
            raw_line_count  INTEGER NOT NULL,
            raw_byte_count  INTEGER NOT NULL,
            created_at      INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE INDEX IF NOT EXISTS idx_cor_command ON command_output_records(command_id);

        CREATE TABLE IF NOT EXISTS output_records (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            command_id      INTEGER NOT NULL REFERENCES commands(id) ON DELETE CASCADE,
            record_type     TEXT NOT NULL,
            severity        TEXT,
            file_path       TEXT,
            data            TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_or_command   ON output_records(command_id);
        CREATE INDEX IF NOT EXISTS idx_or_severity  ON output_records(severity);
        CREATE INDEX IF NOT EXISTS idx_or_file      ON output_records(file_path);
        CREATE INDEX IF NOT EXISTS idx_or_type      ON output_records(record_type);",
    )?;
    conn.pragma_update(None, "user_version", 3)?;
}
```

Key decisions in this schema:
- `command_output_records.severity` stores the highest severity from `OutputSummary` (a TEXT enum value like `"Error"`).
- `output_records.data` stores the full `OutputRecord` enum variant as JSON via `serde_json::to_string`.
- `output_records.severity` and `file_path` are extracted from the JSON at insert time and stored as indexed TEXT columns to enable filtered queries without JSON parsing.
- `record_type` is the enum variant name (`"CompilerError"`, `"TestResult"`, etc.) extracted at insert time.

### Pattern 2: Insert `ParsedOutput`

```rust
// In db.rs or soi.rs, taking ParsedOutput from glass_soi:
pub fn insert_parsed_output(&self, command_id: i64, parsed: &ParsedOutput) -> Result<()> {
    let tx = self.conn.unchecked_transaction()?;

    // 1. Insert summary row
    tx.execute(
        "INSERT INTO command_output_records
         (command_id, output_type, severity, one_line, token_estimate, raw_line_count, raw_byte_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            command_id,
            format!("{:?}", parsed.output_type),   // Debug repr of OutputType
            format!("{:?}", parsed.summary.severity),
            parsed.summary.one_line,
            parsed.summary.token_estimate as i64,
            parsed.raw_line_count as i64,
            parsed.raw_byte_count as i64,
        ],
    )?;

    // 2. Insert individual records
    for record in &parsed.records {
        let (record_type, severity, file_path) = extract_record_meta(record);
        let data = serde_json::to_string(record)?;
        tx.execute(
            "INSERT INTO output_records (command_id, record_type, severity, file_path, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![command_id, record_type, severity, file_path, data],
        )?;
    }

    tx.commit()?;
    Ok(())
}

/// Extract indexed scalar fields from an OutputRecord without serializing.
fn extract_record_meta(record: &OutputRecord) -> (&'static str, Option<String>, Option<String>) {
    match record {
        OutputRecord::CompilerError { file, severity, .. } =>
            ("CompilerError", Some(format!("{:?}", severity)), Some(file.clone())),
        OutputRecord::TestResult { status, .. } =>
            ("TestResult", Some(format!("{:?}", status_to_severity(status))), None),
        OutputRecord::TestSummary { failed, .. } =>
            ("TestSummary", Some(if *failed > 0 { "Error" } else { "Success" }.to_string()), None),
        OutputRecord::PackageEvent { .. } => ("PackageEvent", None, None),
        OutputRecord::GitEvent { .. }     => ("GitEvent", None, None),
        OutputRecord::DockerEvent { .. }  => ("DockerEvent", None, None),
        OutputRecord::GenericDiagnostic { severity, file, .. } =>
            ("GenericDiagnostic", Some(format!("{:?}", severity)), file.clone()),
        OutputRecord::FreeformChunk { .. } => ("FreeformChunk", None, None),
    }
}
```

### Pattern 3: Query by command_id, severity, file, type

```rust
pub fn get_output_records(
    &self,
    command_id: i64,
    severity: Option<&str>,
    file_path: Option<&str>,
    record_type: Option<&str>,
    limit: usize,
) -> Result<Vec<OutputRecordRow>> {
    let mut sql = String::from(
        "SELECT id, command_id, record_type, severity, file_path, data
         FROM output_records WHERE command_id = ?1"
    );
    let mut params: Vec<rusqlite::types::Value> = vec![
        rusqlite::types::Value::Integer(command_id),
    ];
    if let Some(sev) = severity {
        sql.push_str(" AND severity = ?");
        params.push(rusqlite::types::Value::Text(sev.to_owned()));
    }
    if let Some(fp) = file_path {
        sql.push_str(" AND file_path = ?");
        params.push(rusqlite::types::Value::Text(fp.to_owned()));
    }
    if let Some(rt) = record_type {
        sql.push_str(" AND record_type = ?");
        params.push(rusqlite::types::Value::Text(rt.to_owned()));
    }
    sql.push_str(" LIMIT ?");
    params.push(rusqlite::types::Value::Integer(limit as i64));
    // ... query_map
}
```

### Pattern 4: Retention cascade

Extend the two deletion loops in `retention.rs` to explicitly delete SOI rows before deleting commands. `ON DELETE CASCADE` is the safety net; explicit deletion follows the established `pipe_stages` pattern:

```rust
// In both the age-prune and size-prune loops:
for &id in &ids_to_delete {
    tx.execute("DELETE FROM pipe_stages WHERE command_id = ?1", params![id])?;
    tx.execute("DELETE FROM output_records WHERE command_id = ?1", params![id])?;
    tx.execute("DELETE FROM command_output_records WHERE command_id = ?1", params![id])?;
}
```

### Anti-Patterns to Avoid

- **Bumping SCHEMA_VERSION in the test-only const before adding the migration block**: The `#[cfg(test)]` `SCHEMA_VERSION = 2` constant is used in the existing migration test assertion `assert_eq!(version, SCHEMA_VERSION)`. It must be updated to `3` in the same commit as the v3 migration.
- **Storing OutputRecord variants as separate columns**: Schema would need alteration every time a new record type is added. JSON TEXT column is the correct tradeoff here.
- **Forgetting to add glass_soi as a dependency in glass_history/Cargo.toml**: `glass_history` currently has no dependency on `glass_soi`. This must be added before any code using `ParsedOutput` compiles.
- **Using `Debug` format for severity in the summary row but not matching it in queries**: Ensure consistency — the stored string ("Error", "Warning", "Info", "Success") must match what query callers pass in.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Serializing `OutputRecord` enum variants | Custom serializer, string matching | `serde_json::to_string` | `OutputRecord` already derives `Serialize`; any custom approach will diverge from the type definition |
| Schema version management | Custom version table | `PRAGMA user_version` | Already the project standard (v0→v1, v1→v2); all tests validate this pattern |
| Cascade deletion | Manual child-table tracking | `ON DELETE CASCADE` + explicit loop | Exact pattern of `pipe_stages` — belt and suspenders is the project convention |
| Transaction wrapping | Per-insert autocommit | `unchecked_transaction()` | All multi-insert operations in `db.rs` use this pattern; required for atomicity |

---

## Common Pitfalls

### Pitfall 1: `glass_soi` not in `glass_history` dependencies
**What goes wrong:** Compile error — `use glass_soi::ParsedOutput` not resolved.
**Why it happens:** `glass_history/Cargo.toml` currently has no dep on `glass_soi`.
**How to avoid:** Add `glass_soi = { path = "../glass_soi" }` to `glass_history/Cargo.toml` dependencies. Also add `serde_json = "1.0"` if not already present.
**Warning signs:** `cargo check` fails immediately on the new soi.rs module.

### Pitfall 2: Migration test constant not updated
**What goes wrong:** The test `test_fresh_db_has_output_column_and_version` fails because `SCHEMA_VERSION` is still `2`.
**Why it happens:** `SCHEMA_VERSION` is `#[cfg(test)]` only and used in multiple test assertions. Must be bumped to `3`.
**How to avoid:** Update the constant in the same diff as the migration block.

### Pitfall 3: `OutputType` and `Severity` Debug output includes module path
**What goes wrong:** `format!("{:?}", Severity::Error)` produces `"Error"` — correct. But if a future Rust version changes Debug repr, stored strings diverge from query expectations.
**Why it happens:** Rust's `Debug` derive does not guarantee stability across versions.
**How to avoid:** Implement explicit `as_str()` methods on `Severity` and `OutputType`, or match arms to convert to known string literals. This makes the stored value explicit and immune to derive changes.

### Pitfall 4: Retention loops delete in wrong order
**What goes wrong:** Deleting from `commands` before deleting from `output_records` and `command_output_records` when `PRAGMA foreign_keys = ON`.
**Why it happens:** CASCADE fires on parent delete, but the explicit loop pattern in `retention.rs` iterates over child tables first, then commands. If the order is reversed, FK violation or unexpected behavior.
**How to avoid:** Follow the exact order from `pipe_stages` — child tables first, then `commands_fts`, then `commands`.

### Pitfall 5: Forgetting `PRAGMA foreign_keys = ON` is set
**What goes wrong:** Assuming CASCADE works without it — but `HistoryDb::open` sets `PRAGMA foreign_keys = ON` explicitly. CASCADE does work here, but explicitly deleting children first remains the pattern.
**Why it happens:** SQLite disables FK enforcement by default; this project enables it.
**How to avoid:** Rely on explicit deletion in retention loops (pattern already exists) rather than assuming CASCADE.

### Pitfall 6: Migration idempotency on fresh databases
**What goes wrong:** A fresh DB created with the new `create_schema` (which includes the v3 tables) will have `user_version = 0`. The `if version < 3` migration block will run and try to `CREATE TABLE IF NOT EXISTS` — which succeeds because the guard is `IF NOT EXISTS`. Then it bumps user_version to 3 correctly.
**Why it happens:** `create_schema` uses `IF NOT EXISTS` on all tables, and the migration fills in all schema levels atomically.
**How to avoid:** This is already handled by the existing `IF NOT EXISTS` guards. Verify fresh-DB test passes with `SCHEMA_VERSION = 3`.

---

## Code Examples

### Existing migration v2 block (reference pattern — source: db.rs)
```rust
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
        CREATE INDEX IF NOT EXISTS idx_pipe_stages_command ON pipe_stages(command_id);",
    )?;
    conn.pragma_update(None, "user_version", 2)?;
}
```

### Existing retention explicit-delete pattern (source: retention.rs)
```rust
for &id in &ids_to_delete {
    tx.execute("DELETE FROM pipe_stages WHERE command_id = ?1", params![id])?;
}
for &id in &ids_to_delete {
    tx.execute("DELETE FROM commands_fts WHERE rowid = ?1", params![id])?;
}
for &id in &ids_to_delete {
    tx.execute("DELETE FROM commands WHERE id = ?1", params![id])?;
}
```
The v3 pattern inserts the SOI deletions after `pipe_stages` but before `commands_fts`.

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| No structured output storage | Two new tables for SOI records | Phase 49 | Enables MCP queries by severity/file/type (Phase 53) |
| Schema v2 (pipe_stages only) | Schema v3 (+ output_records + command_output_records) | Phase 49 | Automatic migration on startup, no data loss |

---

## Open Questions

1. **Severity string encoding**
   - What we know: `Severity` enum has 4 variants (Error, Warning, Info, Success). The derive `Debug` produces `"Error"`, etc.
   - What's unclear: Whether to use `Debug` format, an explicit `as_str()`, or the `Serialize` derive's JSON string output (which is also `"Error"` for a unit variant with `serde`'s default behavior).
   - Recommendation: Add an `as_str() -> &'static str` to `Severity` in `glass_soi/types.rs` returning the canonical lowercase or title-case string. Alternatively, call `serde_json::to_string(&severity)` and strip the outer quotes. The planner should pick one approach and document it as a decision.

2. **Where does `insert_parsed_output` live?**
   - What we know: Options are `db.rs` (inline method on `HistoryDb`) or a new `soi.rs` module with its own row type.
   - What's unclear: Whether the new `SoiRecord` row type (returned from query) should be a first-class public type in `lib.rs`.
   - Recommendation: New `soi.rs` module with `pub struct OutputRecordRow` and `pub struct CommandOutputSummaryRow`, re-exported from `lib.rs`. Keeps `db.rs` focused on core record CRUD. `HistoryDb` gains `insert_parsed_output` and `get_output_records` methods that delegate to `soi.rs` functions, mirroring the `search.rs`/`retention.rs` module pattern.

---

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + cargo test |
| Config file | none — inline `#[cfg(test)] mod tests` |
| Quick run command | `cargo test -p glass_history` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SOIS-01 | Insert `ParsedOutput` → rows appear in `command_output_records` and `output_records` | unit | `cargo test -p glass_history -- soi` | ❌ Wave 0 |
| SOIS-01 | Query by `command_id` returns correct rows | unit | `cargo test -p glass_history -- soi` | ❌ Wave 0 |
| SOIS-02 | Fresh DB auto-migrates to v3 (user_version = 3) | unit | `cargo test -p glass_history -- test_fresh_db_has_output_column_and_version` | ✅ exists (update) |
| SOIS-02 | v2 DB migrates to v3 without data loss | unit | `cargo test -p glass_history -- test_migration_v2_to_v3` | ❌ Wave 0 |
| SOIS-03 | Filter by severity returns correct subset | unit | `cargo test -p glass_history -- soi` | ❌ Wave 0 |
| SOIS-03 | Filter by file_path returns correct subset | unit | `cargo test -p glass_history -- soi` | ❌ Wave 0 |
| SOIS-03 | Filter by record_type returns correct subset | unit | `cargo test -p glass_history -- soi` | ❌ Wave 0 |
| SOIS-04 | Age prune removes associated SOI rows | unit | `cargo test -p glass_history -- prune_cascades_to_soi` | ❌ Wave 0 |
| SOIS-04 | Size prune removes associated SOI rows | unit | `cargo test -p glass_history -- size_prune_cascades_to_soi` | ❌ Wave 0 |
| SOIS-04 | No orphaned rows after delete_command | unit | `cargo test -p glass_history -- delete_command_cascades_soi` | ❌ Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_history`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_history/src/soi.rs` — new module with `OutputRecordRow`, `CommandOutputSummaryRow`, insert/query functions
- [ ] Tests for SOIS-01 in `soi.rs` (inline `#[cfg(test)] mod tests`)
- [ ] `test_migration_v2_to_v3` in `db.rs` tests
- [ ] `test_prune_cascades_to_soi` in `retention.rs` tests
- [ ] `test_size_prune_cascades_to_soi` in `retention.rs` tests
- [ ] `test_delete_command_cascades_soi` in `db.rs` tests
- [ ] Update `SCHEMA_VERSION` constant from `2` to `3` in `db.rs`

---

## Sources

### Primary (HIGH confidence)
- `crates/glass_history/src/db.rs` — migration pattern, transaction pattern, SCHEMA_VERSION constant, pipe_stages schema
- `crates/glass_history/src/retention.rs` — explicit child-table deletion pattern
- `crates/glass_soi/src/types.rs` — `OutputRecord`, `Severity`, `OutputType`, `ParsedOutput` structures
- `crates/glass_history/Cargo.toml` — current dependencies (no glass_soi, no serde_json)
- `crates/glass_soi/Cargo.toml` — serde_json 1.0 already present here
- `.planning/REQUIREMENTS.md` — SOIS-01 through SOIS-04 definitions
- `Cargo.toml` workspace — rusqlite 0.38.0 bundled

### Secondary (MEDIUM confidence)
- SQLite `ON DELETE CASCADE` behavior with `PRAGMA foreign_keys = ON` — verified by existing pipe_stages cascade tests in db.rs
- `serde_json::to_string` on `OutputRecord` (Serialize derive already present) — standard serde usage

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all libraries already in workspace; no new deps beyond adding `glass_soi` to `glass_history`
- Architecture: HIGH — migration pattern is exactly replicated from v1/v2; retention pattern is exactly replicated from pipe_stages
- Pitfalls: HIGH — identified from direct code reading of existing migration tests and retention logic

**Research date:** 2026-03-12
**Valid until:** 2026-06-12 (stable Rust crate ecosystem; rusqlite API is stable)

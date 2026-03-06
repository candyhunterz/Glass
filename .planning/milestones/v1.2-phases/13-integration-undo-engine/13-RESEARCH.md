# Phase 13: Integration + Undo Engine - Research

**Researched:** 2026-03-05
**Domain:** Snapshot lifecycle orchestration, file restoration, conflict detection, config extension
**Confidence:** HIGH

## Summary

Phase 13 is the integration phase that ties together the three infrastructure pieces built in Phases 10-12 (content store, command parser, FS watcher) into a working undo system. The phase has two major axes: (1) **pre-exec snapshot lifecycle** -- when a command begins executing (OSC 133;C), parse the command text, identify file targets, and snapshot them *before* the command modifies them; and (2) **undo engine** -- when the user presses Ctrl+Shift+Z, find the most recent file-modifying command's snapshot and restore files to their pre-command state, with conflict detection and confidence reporting.

All the building blocks already exist in the codebase. `SnapshotStore` (Phase 10) provides `create_snapshot()`, `store_file()`, `blobs().read_blob()`. `command_parser::parse_command()` (Phase 11) returns `ParseResult` with `targets` and `confidence`. `FsWatcher` (Phase 12) captures post-exec file changes. The current `main.rs` already starts the watcher on `CommandExecuted` and drains it on `CommandFinished` -- but it does NOT do pre-exec snapshots of parser-identified targets yet (that is SNAP-01, this phase's work). The watcher currently stores files *after* the command runs (their post-command state), which is useful for knowing what changed but not for undo. Pre-exec snapshots store the *before* state.

**Primary recommendation:** Add pre-exec snapshot at `CommandExecuted` time (right after command text extraction, before watcher start), implement an `UndoEngine` struct in `glass_snapshot` that handles file restoration with conflict detection, wire Ctrl+Shift+Z to the undo engine in `main.rs`, and extend `GlassConfig` with a `[snapshot]` TOML section.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SNAP-01 | Glass automatically snapshots target files before a command executes, triggered by OSC 133;C | Command text is already extracted at CommandExecuted time in main.rs; `parse_command()` identifies targets; `SnapshotStore::store_file()` stores them. Integration point is clear -- insert snapshot logic between text extraction and watcher start in the CommandExecuted handler. |
| UNDO-01 | User can undo the most recent file-modifying command via Ctrl+Shift+Z | Keybinding infrastructure exists (Ctrl+Shift+C/V/F pattern in main.rs); need to add Ctrl+Shift+Z handler that calls into UndoEngine. Must query snapshots DB for most recent snapshot with files. |
| UNDO-02 | Undo restores snapshotted file contents to their pre-command state | `BlobStore::read_blob(hash)` retrieves file contents; `std::fs::write()` restores them. For files that did not exist pre-command (NULL blob_hash), restoration means deleting the file. |
| UNDO-03 | Conflict detection warns if a file has been modified since the tracked command ran | Compare current file's BLAKE3 hash against the post-command state (watcher snapshot). If current hash differs from watcher-recorded hash, the file was modified after the command -- warn before overwriting. |
| UNDO-04 | Each command displays its undo confidence level (pre-exec snapshot vs watcher-only) | `ParseResult::confidence` already has High/Low/ReadOnly. Map to undo confidence: High = "full pre-exec snapshot", Low = "watcher-only recording". Store confidence in snapshot metadata. |
| STOR-03 | Snapshot configuration section in config.toml (enabled, max_count, max_size_mb, retention_days) | Extend `GlassConfig` with `SnapshotSection`. Use `serde(default)` for backward compatibility. Config checked at snapshot creation time. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| glass_snapshot (internal) | 0.1.0 | SnapshotStore, BlobStore, SnapshotDb, command_parser, FsWatcher | Already built in Phases 10-12; all infrastructure exists |
| blake3 | 1.8.3 | Content hashing for conflict detection | Already in workspace; used for blob addressing |
| rusqlite | 0.38.0 | Snapshot metadata queries | Already in workspace; powers SnapshotDb |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| toml/serde | workspace | Config deserialization | Extending GlassConfig for [snapshot] section |
| tracing | workspace | Structured logging for undo operations | All undo actions should be logged |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| In-process UndoEngine | Background thread/channel | Unnecessary complexity -- undo is triggered by user action, runs infrequently, file I/O is fast for small file sets |
| BLAKE3 hash comparison for conflict detection | File mtime comparison | Mtime is unreliable (filesystem resolution, clock skew); BLAKE3 is authoritative |
| Storing undo confidence in snapshot_files.source | New DB column | source="parser" already implies High confidence, source="watcher" implies Low -- no schema change needed |

## Architecture Patterns

### Recommended Module Structure
```
crates/glass_snapshot/src/
  lib.rs            # existing -- add UndoEngine, extend SnapshotStore
  undo.rs           # NEW -- UndoEngine struct with restore/conflict logic
  blob_store.rs     # existing -- unchanged
  command_parser.rs # existing -- unchanged
  db.rs             # existing -- add query methods (latest snapshot, snapshots with files)
  ignore_rules.rs   # existing -- unchanged
  types.rs          # existing -- add UndoResult, UndoFileOutcome types
  watcher.rs        # existing -- unchanged

crates/glass_core/src/
  config.rs         # existing -- add SnapshotSection

src/
  main.rs           # existing -- add pre-exec snapshot + Ctrl+Shift+Z handler
```

### Pattern 1: Pre-Exec Snapshot at CommandExecuted
**What:** When OSC 133;C fires, parse command text, snapshot identified file targets, then start watcher.
**When to use:** Every command execution.
**Example:**
```rust
// In the CommandExecuted handler in main.rs, AFTER extracting command_text,
// BEFORE starting the watcher:

if let Some(ref store) = ctx.snapshot_store {
    let cwd_path = std::path::Path::new(ctx.status.cwd());
    let parse_result = glass_snapshot::command_parser::parse_command(
        &command_text, cwd_path,
    );

    if parse_result.confidence != Confidence::ReadOnly {
        // Create snapshot with command_id=0 (will be updated on CommandFinished)
        match store.create_snapshot(0, ctx.status.cwd()) {
            Ok(snapshot_id) => {
                for target in &parse_result.targets {
                    if let Err(e) = store.store_file(snapshot_id, target, "parser") {
                        tracing::warn!("Pre-exec snapshot failed for {}: {}", target.display(), e);
                    }
                }
                ctx.pending_snapshot_id = Some(snapshot_id);
                ctx.pending_parse_confidence = Some(parse_result.confidence);
            }
            Err(e) => tracing::warn!("Failed to create pre-exec snapshot: {}", e),
        }
    }
}
```

### Pattern 2: Undo Engine with Conflict Detection
**What:** UndoEngine takes a snapshot, checks each file for conflicts, restores non-conflicted files.
**When to use:** When user triggers undo via Ctrl+Shift+Z.
**Example:**
```rust
pub struct UndoEngine<'a> {
    store: &'a SnapshotStore,
}

pub enum FileOutcome {
    Restored,           // File restored to pre-command state
    Deleted,            // File did not exist pre-command, deleted
    Skipped,            // File was not in snapshot
    Conflict {          // File modified since command ran
        current_hash: String,
        snapshot_hash: Option<String>,
    },
    Error(String),      // I/O error during restore
}

pub struct UndoResult {
    pub snapshot_id: i64,
    pub command_id: i64,
    pub files: Vec<(PathBuf, FileOutcome)>,
}

impl<'a> UndoEngine<'a> {
    pub fn undo_latest(&self) -> Result<Option<UndoResult>> {
        // 1. Query DB for most recent snapshot that has files
        // 2. For each file in snapshot:
        //    a. Hash current file on disk
        //    b. Compare against expected post-command hash (if watcher recorded it)
        //    c. If conflict, record Conflict outcome
        //    d. If no conflict, restore from blob or delete
        // 3. Return UndoResult with per-file outcomes
    }
}
```

### Pattern 3: Config-Gated Snapshot Creation
**What:** Check config before creating snapshots; respect enabled/max_count/max_size_mb.
**When to use:** At snapshot creation time.
**Example:**
```rust
#[derive(Debug, Clone, Deserialize)]
pub struct SnapshotSection {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_max_count")]
    pub max_count: u32,
    #[serde(default = "default_max_size_mb")]
    pub max_size_mb: u32,
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
}

fn default_enabled() -> bool { true }
fn default_max_count() -> u32 { 1000 }
fn default_max_size_mb() -> u32 { 500 }
fn default_retention_days() -> u32 { 30 }
```

### Anti-Patterns to Avoid
- **Storing post-command state as "undo data":** The watcher currently stores files AFTER the command runs. This records what changed but not the pre-command state. Pre-exec snapshots (SNAP-01) store the BEFORE state, which is what undo restores TO.
- **Blocking the event loop during undo:** File restoration could be slow for many files. However, given typical command file counts (1-5 files), synchronous restoration on the main thread is acceptable. Do NOT over-engineer with async/channels for V1.
- **Merging pre-exec and watcher snapshots into one record:** Keep them as separate snapshot records with different source values. The pre-exec snapshot has the "before" state; the watcher snapshot has the "after" state. Both reference the same command_id.
- **Deleting snapshot records after undo:** Keep them for audit trail. Mark as "undone" if needed but do not delete.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Content hashing for conflict check | Custom hash function | `blake3::hash()` | Already used throughout; consistent with blob store |
| Config parsing | Manual TOML parsing | `serde + toml` derive | Already used for GlassConfig; `#[serde(default)]` handles missing fields |
| File content restoration | Custom file writer with atomic rename | `std::fs::write()` | Atomic rename adds complexity; std::fs::write is sufficient for V1 |

## Common Pitfalls

### Pitfall 1: Pre-exec snapshot timing relative to watcher start
**What goes wrong:** If the watcher starts BEFORE the pre-exec snapshot, the snapshot file reads trigger watcher events (reading file = access event). This is harmless since the watcher filters access events, but the ORDER matters for correctness.
**Why it happens:** The current code starts the watcher immediately on CommandExecuted. Pre-exec snapshot must happen first.
**How to avoid:** In the CommandExecuted handler, execute in this order: (1) extract command text, (2) parse command, (3) create pre-exec snapshot, (4) start watcher.
**Warning signs:** Watcher events containing the snapshot target files.

### Pitfall 2: command_id not yet available at CommandExecuted time
**What goes wrong:** The command history record is inserted at CommandFinished time, so command_id is not known at CommandExecuted when the pre-exec snapshot is created.
**Why it happens:** CommandRecord needs exit_code and duration, which are only available at CommandFinished.
**How to avoid:** Create the pre-exec snapshot with `command_id=0`, store the `snapshot_id` in `pending_snapshot_id`, then update the snapshot's command_id on CommandFinished using `store.update_command_id(snapshot_id, command_id)`. This method already exists on SnapshotStore.
**Warning signs:** Snapshots with command_id=0 that never get updated (if CommandFinished never fires).

### Pitfall 3: Conflict detection requires knowing post-command file hash
**What goes wrong:** To detect if a file was modified AFTER the undone command ran, you need to know what the file looked like immediately after the command. But the pre-exec snapshot stores the BEFORE state.
**Why it happens:** The watcher snapshot stores files after the command, but it stores their current content at drain time (which IS the post-command state). However, if we want conflict detection, we need to compare current disk state against the post-command state.
**How to avoid:** Use the watcher snapshot's blob_hash as the "expected current state." If the file's current BLAKE3 hash matches the watcher hash, no conflict. If it differs, the file was modified after the command ran. If there is no watcher record for a file, conflict detection is not possible for that file (no ground truth).
**Warning signs:** False conflict warnings on files that haven't actually been modified since the command.

### Pitfall 4: Files that did not exist before the command
**What goes wrong:** Pre-exec snapshot records NULL blob_hash for files that don't exist yet. Undo should delete these files (restoring to "non-existent" state). But the file might have been further modified by subsequent commands.
**Why it happens:** NULL hash means "file was absent."
**How to avoid:** On undo, if blob_hash is NULL: check conflict first (is current file what the command created?), then delete the file. If conflict detected, warn before deleting.

### Pitfall 5: Undo of the watcher-only (Low confidence) commands
**What goes wrong:** For Low confidence commands, there is no pre-exec snapshot (parser couldn't identify targets). The watcher captured what changed, but we don't have the "before" state.
**Why it happens:** Parser returns `Confidence::Low` for unknown commands.
**How to avoid:** For watcher-only snapshots, undo is NOT possible in V1 (no pre-command state stored). Display this as confidence level to the user. The watcher snapshot is useful for showing WHAT changed, not for restoring.
**Warning signs:** Attempting to undo a watcher-only snapshot and having no blob to restore from.

### Pitfall 6: Config checked at wrong time
**What goes wrong:** Snapshot config (enabled, max_count, max_size_mb) must be checked at snapshot creation time, not at startup. Config could be reloaded.
**Why it happens:** Loading config once at startup and never re-reading.
**How to avoid:** For V1, config is loaded once at startup and passed into the snapshot creation path. The `enabled` flag gates the entire pre-exec snapshot flow. Max count/size are informational for Phase 14's pruning -- do not enforce them here (STOR-01 is Phase 14).
**Warning signs:** Config changes not taking effect until restart.

## Code Examples

### Pre-exec Snapshot Integration Point (main.rs CommandExecuted handler)
```rust
// After extracting command_text, before starting watcher:
let mut pending_snapshot_id: Option<i64> = None;

if let Some(ref store) = ctx.snapshot_store {
    let cwd_path = std::path::Path::new(ctx.status.cwd());
    let parse_result = glass_snapshot::command_parser::parse_command(
        &command_text, cwd_path,
    );

    // Only snapshot for non-read-only commands
    if parse_result.confidence != glass_snapshot::Confidence::ReadOnly {
        if !parse_result.targets.is_empty() {
            match store.create_snapshot(0, ctx.status.cwd()) {
                Ok(sid) => {
                    for target in &parse_result.targets {
                        let _ = store.store_file(sid, target, "parser");
                    }
                    pending_snapshot_id = Some(sid);
                    tracing::debug!(
                        "Pre-exec snapshot {} with {} targets (confidence={:?})",
                        sid, parse_result.targets.len(), parse_result.confidence,
                    );
                }
                Err(e) => tracing::warn!("Pre-exec snapshot creation failed: {}", e),
            }
        }
    }
}
ctx.pending_snapshot_id = pending_snapshot_id;
```

### Undo Engine Core Logic
```rust
pub fn undo_snapshot(&self, snapshot_id: i64) -> Result<UndoResult> {
    let snapshot = self.store.db().get_snapshot(snapshot_id)?
        .ok_or_else(|| anyhow::anyhow!("Snapshot {} not found", snapshot_id))?;
    let files = self.store.db().get_snapshot_files(snapshot_id)?;

    let mut outcomes = Vec::new();

    for file_rec in &files {
        // Only restore files from "parser" source (pre-exec snapshots)
        if file_rec.source != "parser" {
            continue;
        }

        let path = std::path::Path::new(&file_rec.file_path);

        match &file_rec.blob_hash {
            Some(hash) => {
                // File existed before command -- restore its content
                match self.store.blobs().read_blob(hash) {
                    Ok(content) => {
                        if let Err(e) = std::fs::write(path, &content) {
                            outcomes.push((path.to_path_buf(), FileOutcome::Error(e.to_string())));
                        } else {
                            outcomes.push((path.to_path_buf(), FileOutcome::Restored));
                        }
                    }
                    Err(e) => {
                        outcomes.push((path.to_path_buf(), FileOutcome::Error(e.to_string())));
                    }
                }
            }
            None => {
                // File did not exist before command -- delete it
                if path.exists() {
                    if let Err(e) = std::fs::remove_file(path) {
                        outcomes.push((path.to_path_buf(), FileOutcome::Error(e.to_string())));
                    } else {
                        outcomes.push((path.to_path_buf(), FileOutcome::Deleted));
                    }
                } else {
                    outcomes.push((path.to_path_buf(), FileOutcome::Skipped));
                }
            }
        }
    }

    Ok(UndoResult {
        snapshot_id,
        command_id: snapshot.command_id,
        files: outcomes,
    })
}
```

### Conflict Detection
```rust
fn check_conflict(&self, file_path: &Path, snapshot_id: i64) -> Result<bool> {
    // Get current file hash
    if !file_path.exists() {
        return Ok(false); // File was deleted -- no conflict check needed
    }
    let current_content = std::fs::read(file_path)?;
    let current_hash = blake3::hash(&current_content).to_hex().to_string();

    // Find the watcher snapshot for the same command
    let snapshot = self.store.db().get_snapshot(snapshot_id)?
        .ok_or_else(|| anyhow::anyhow!("Snapshot not found"))?;

    // Look for watcher snapshot files for this command
    let watcher_snapshots = self.store.db().get_snapshots_by_command(snapshot.command_id)?;
    for ws in &watcher_snapshots {
        let ws_files = self.store.db().get_snapshot_files(ws.id)?;
        for wf in &ws_files {
            if wf.source == "watcher" && wf.file_path == file_path.to_string_lossy() {
                if let Some(ref watcher_hash) = wf.blob_hash {
                    // File was recorded by watcher after command
                    // If current hash differs from watcher hash, file was modified since
                    return Ok(&current_hash != watcher_hash);
                }
            }
        }
    }

    // No watcher data for this file -- cannot determine conflict
    // Default to no conflict (optimistic)
    Ok(false)
}
```

### Ctrl+Shift+Z Keybinding
```rust
// In the Ctrl+Shift key handler block in main.rs:
Key::Character(c) if c.as_str().eq_ignore_ascii_case("z") => {
    if let Some(ref store) = ctx.snapshot_store {
        let engine = glass_snapshot::UndoEngine::new(store);
        match engine.undo_latest() {
            Ok(Some(result)) => {
                tracing::info!(
                    "Undo complete: {} files processed for command {}",
                    result.files.len(), result.command_id,
                );
                for (path, outcome) in &result.files {
                    match outcome {
                        FileOutcome::Restored => tracing::info!("  restored: {}", path.display()),
                        FileOutcome::Deleted => tracing::info!("  deleted: {}", path.display()),
                        FileOutcome::Conflict { .. } => tracing::warn!("  CONFLICT: {}", path.display()),
                        FileOutcome::Error(e) => tracing::error!("  error: {}: {}", path.display(), e),
                        FileOutcome::Skipped => tracing::debug!("  skipped: {}", path.display()),
                    }
                }
            }
            Ok(None) => {
                tracing::info!("Nothing to undo");
            }
            Err(e) => {
                tracing::error!("Undo failed: {}", e);
            }
        }
    }
    return;
}
```

### Config Extension
```rust
// In glass_core/src/config.rs:
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GlassConfig {
    pub font_family: String,
    pub font_size: f32,
    pub shell: Option<String>,
    pub history: Option<HistorySection>,
    pub snapshot: Option<SnapshotSection>,  // NEW
}

#[derive(Debug, Clone, Deserialize)]
pub struct SnapshotSection {
    #[serde(default = "default_snapshot_enabled")]
    pub enabled: bool,
    #[serde(default = "default_max_count")]
    pub max_count: u32,
    #[serde(default = "default_max_size_mb")]
    pub max_size_mb: u32,
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
}

fn default_snapshot_enabled() -> bool { true }
fn default_max_count() -> u32 { 1000 }
fn default_max_size_mb() -> u32 { 500 }
fn default_retention_days() -> u32 { 30 }
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Watcher-only snapshots (Phase 12) | Pre-exec + watcher dual mechanism | Phase 13 | Pre-exec captures BEFORE state; watcher captures AFTER state |
| No undo capability | Ctrl+Shift+Z undo with conflict detection | Phase 13 | Users can revert file-modifying commands |
| Flat config (font, shell, history) | Config with [snapshot] section | Phase 13 | Users can control snapshot behavior |

## Open Questions

1. **Should undo be limited to "most recent command only" or allow undo of any past command?**
   - What we know: Requirements say "most recent file-modifying command" (UNDO-01). Phase 14 adds `glass undo <command-id>` for specific commands.
   - What's unclear: Whether Ctrl+Shift+Z should skip read-only commands automatically to find the most recent file-modifying one.
   - Recommendation: Yes, skip ReadOnly commands. Query for most recent snapshot that has parser-sourced files.

2. **How to handle undo when the pre-exec snapshot has files but the watcher also recorded different files?**
   - What we know: Parser identifies known targets; watcher catches everything. They may overlap or differ.
   - What's unclear: Should undo restore ALL files (parser + watcher) or only parser files?
   - Recommendation: Only restore parser-sourced files (pre-exec snapshots). Watcher files don't have pre-command state. Display watcher-only files as "not restorable" in the undo result.

3. **Should files larger than max_size_mb be skipped during pre-exec snapshot?**
   - What we know: max_size_mb is a total storage limit for STOR-03, not a per-file limit.
   - What's unclear: Whether individual large files should be skipped.
   - Recommendation: No per-file limit for V1. max_size_mb is enforced by Phase 14's pruning (STOR-01). At snapshot time, just store everything.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test framework (cargo test) |
| Config file | Cargo.toml (workspace, already configured) |
| Quick run command | `cargo test -p glass_snapshot --lib` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SNAP-01 | Pre-exec snapshot stores file contents before command modifies them | unit | `cargo test -p glass_snapshot --lib -- undo::tests::test_pre_exec_snapshot -x` | Wave 0 |
| UNDO-01 | undo_latest returns most recent file-modifying snapshot | unit | `cargo test -p glass_snapshot --lib -- undo::tests::test_undo_latest -x` | Wave 0 |
| UNDO-02 | Undo restores file contents from blob store | unit | `cargo test -p glass_snapshot --lib -- undo::tests::test_restore_file -x` | Wave 0 |
| UNDO-03 | Conflict detection compares current hash with watcher hash | unit | `cargo test -p glass_snapshot --lib -- undo::tests::test_conflict_detection -x` | Wave 0 |
| UNDO-04 | Undo result includes confidence level from parser | unit | `cargo test -p glass_snapshot --lib -- undo::tests::test_confidence_level -x` | Wave 0 |
| STOR-03 | SnapshotSection deserializes from TOML with defaults | unit | `cargo test -p glass_core --lib -- config::tests::test_snapshot_config -x` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_snapshot --lib && cargo test -p glass_core --lib`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_snapshot/src/undo.rs` -- new module with UndoEngine and tests
- [ ] `crates/glass_core/src/config.rs` -- add SnapshotSection tests
- [ ] `crates/glass_snapshot/src/db.rs` -- add query methods for latest snapshot with parser files

## Sources

### Primary (HIGH confidence)
- Project source code: `crates/glass_snapshot/src/` -- all Phase 10-12 implementations reviewed
- Project source code: `src/main.rs` -- CommandExecuted/CommandFinished lifecycle reviewed
- Project source code: `crates/glass_core/src/config.rs` -- config pattern reviewed
- Project source code: `crates/glass_snapshot/src/types.rs` -- existing types reviewed

### Secondary (MEDIUM confidence)
- Rust std::fs documentation -- file read/write/remove operations
- blake3 crate -- hash comparison for conflict detection

### Tertiary (LOW confidence)
- None -- this phase is entirely internal integration, no new external dependencies

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all libraries already in workspace, no new deps needed
- Architecture: HIGH -- all integration points identified in existing code, patterns clear
- Pitfalls: HIGH -- identified from direct code review (command_id timing, snapshot ordering, conflict detection logic)

**Research date:** 2026-03-05
**Valid until:** 2026-04-05 (stable -- internal integration, no external API changes)

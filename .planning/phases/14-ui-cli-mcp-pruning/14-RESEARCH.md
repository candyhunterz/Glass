# Phase 14: UI + CLI + MCP + Pruning - Research

**Researched:** 2026-03-05
**Domain:** Terminal UI rendering, CLI subcommands, MCP tool extension, storage lifecycle
**Confidence:** HIGH

## Summary

Phase 14 completes the v1.2 Command-Level Undo milestone by making undo discoverable, accessible, and sustainable. It covers four distinct areas: (1) UI labels on command blocks showing `[undo]` and post-undo feedback, (2) a `glass undo <command-id>` CLI subcommand, (3) two new MCP tools (GlassUndo, GlassFileDiff) for AI assistant integration, and (4) automatic storage pruning based on configured age/size limits.

The existing codebase provides strong foundations for all four areas. The block renderer already produces `BlockLabel` text and `RectInstance` decorations for exit codes and durations -- the `[undo]` label follows the identical pattern. The MCP server uses rmcp v1 with `#[tool_router]` / `#[tool_handler]` macros, making new tool addition mechanical. The CLI uses clap derive with subcommands. The snapshot DB already has `delete_snapshot`, `get_snapshots_by_command`, and `created_at` columns needed for pruning. The UndoEngine has `undo_latest()` but needs a new `undo_command(command_id)` method.

**Primary recommendation:** Implement in four independent workstreams that share a small common layer (undo-by-command-id in UndoEngine), then wire each into its integration point.

<phase_requirements>

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| UI-01 | File-modifying command blocks display an [undo] label | BlockRenderer.build_block_text already produces labels; add [undo] label for blocks with parser snapshots |
| UI-02 | After undo, visual confirmation shows which files were restored, skipped, or errored | UndoResult already contains per-file FileOutcome; render as overlay or inline text near the command block |
| UI-03 | User can undo a specific command via `glass undo <command-id>` CLI | Add Undo subcommand to clap Commands enum; add undo_command(id) to UndoEngine |
| STOR-01 | Storage pruning enforces configurable max age and max size limits with automatic cleanup | Config already has max_count, max_size_mb, retention_days; add pruner module to glass_snapshot |
| MCP-01 | GlassUndo MCP tool allows AI assistants to trigger undo programmatically | Add glass_undo tool handler to GlassServer using rmcp #[tool] macro |
| MCP-02 | GlassFileDiff MCP tool allows AI assistants to inspect file diffs from commands | Add glass_file_diff tool handler querying snapshot_files and blob content |

</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| rmcp | 1.x | MCP server framework | Already used for GlassHistory/GlassContext tools |
| clap | 4.5 (derive) | CLI argument parsing | Already used for glass subcommands |
| glyphon | 0.10.0 | GPU text rendering | Already used for block labels and status bar |
| rusqlite | 0.38.0 | SQLite snapshot/history DB | Already used for all DB operations |
| blake3 | 1.8.3 | Content-addressed hashing | Already used for blob deduplication |
| schemars | 1.0 | JSON Schema for MCP params | Already used in glass_mcp for auto-schema |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| chrono | 0.4 | Time calculations for pruning | Already in workspace deps |
| serde | 1.0.228 | Serialization for MCP responses | Already in workspace deps |
| tempfile | 3 | Test fixtures | Already in dev-dependencies |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Custom diff | similar crate | Not needed -- MCP-02 returns pre/post content, not line diffs |

**Installation:**
```bash
# No new dependencies needed -- everything is already in workspace
```

## Architecture Patterns

### Recommended Project Structure
```
crates/glass_snapshot/src/
  undo.rs          # Add undo_command(command_id) method
  pruner.rs        # NEW: storage pruning logic (STOR-01)
  db.rs            # Add pruning queries (delete old snapshots, get total size)
  lib.rs           # Re-export Pruner

crates/glass_mcp/src/
  tools.rs         # Add glass_undo + glass_file_diff tool handlers
  lib.rs           # Add glass_snapshot dependency, resolve snapshot DB path

crates/glass_renderer/src/
  block_renderer.rs  # Add [undo] label generation (UI-01)
  frame.rs           # Pass snapshot availability info to block renderer

src/main.rs
  Commands enum    # Add Undo { command_id } subcommand (UI-03)
  Ctrl+Shift+Z     # Add visual feedback after undo (UI-02)
  window creation  # Trigger pruning on startup (STOR-01)
```

### Pattern 1: Block Label Extension (UI-01)
**What:** Add `[undo]` label to `build_block_text` for blocks that have associated parser snapshots.
**When to use:** For any visible, completed block with file-modifying activity.
**Implementation approach:**

The `build_block_text` method in `block_renderer.rs` currently creates labels for exit code badges and duration text. The `[undo]` label follows the same pattern -- create a `BlockLabel` positioned to the left of the duration text. The block renderer needs to know which blocks have snapshots.

Two options for passing snapshot availability to the renderer:
1. **Add a `has_snapshot: bool` field to Block** -- simplest, set it during CommandFinished handling
2. **Pass a HashSet of command_ids with snapshots** -- more flexible, but requires DB query per frame

Recommendation: Option 1. Add `has_snapshot: bool` to `Block` struct, set it to `true` when `pending_snapshot_id` is resolved. The renderer checks this field.

```rust
// In block_renderer.rs build_block_text:
if block.has_snapshot {
    let undo_text = "[undo]";
    let undo_width = undo_text.len() as f32 * self.cell_width;
    // Position left of duration text
    let undo_x = viewport_width - badge_width - duration_width - undo_width - self.cell_width * 2.0;
    labels.push(BlockLabel {
        x: undo_x,
        y,
        text: undo_text.to_string(),
        color: Rgb { r: 100, g: 160, b: 220 }, // Subtle blue
    });
}
```

### Pattern 2: Undo Visual Feedback (UI-02)
**What:** After Ctrl+Shift+Z undo, show per-file outcomes in the terminal.
**When to use:** Immediately after undo completes.
**Implementation approach:**

Currently, undo results are only logged via `tracing::info!`. For visual feedback, write the results directly to the PTY so they appear in the terminal output. This is the simplest approach that requires no new rendering infrastructure.

```rust
// After undo_latest() returns Ok(Some(result)):
let mut feedback = String::new();
feedback.push_str("\r\n\x1b[1;36m--- Undo Complete ---\x1b[0m\r\n");
for (path, outcome) in &result.files {
    let line = match outcome {
        FileOutcome::Restored => format!("  \x1b[32mrestored\x1b[0m {}\r\n", path.display()),
        FileOutcome::Deleted => format!("  \x1b[33mdeleted\x1b[0m  {}\r\n", path.display()),
        FileOutcome::Skipped => format!("  \x1b[90mskipped\x1b[0m  {}\r\n", path.display()),
        FileOutcome::Conflict { .. } => format!("  \x1b[31mconflict\x1b[0m {}\r\n", path.display()),
        FileOutcome::Error(e) => format!("  \x1b[31merror\x1b[0m    {}: {}\r\n", path.display(), e),
    };
    feedback.push_str(&line);
}
// Write to PTY so it appears in terminal
let _ = ctx.pty_sender.send(PtyMsg::Input(Cow::Owned(feedback.into_bytes())));
```

Alternative: Write feedback directly to terminal grid. But PTY injection is simpler and consistent with how terminal emulators surface information.

**Important caveat:** Writing to the PTY sends the text to the shell's stdin, which may echo it oddly. A better approach is to use the terminal's direct write mechanism or a notification overlay. The cleanest approach for V1 is to write ANSI-colored text to the PTY output (not input) if such a mechanism exists. If not, log to a temporary overlay similar to the search overlay.

**Revised recommendation:** Create a simple timed notification that displays for 3-5 seconds at the top or bottom of the viewport, using the existing overlay buffer pattern from search_overlay_renderer. This avoids polluting shell state.

### Pattern 3: CLI Undo Subcommand (UI-03)
**What:** Add `glass undo <command-id>` as a clap subcommand.
**When to use:** When user wants to undo a specific command from outside the terminal GUI.
**Implementation approach:**

Follow the exact pattern of `Commands::History` and `Commands::Mcp`:

```rust
// In Commands enum:
/// Undo a file-modifying command
Undo {
    /// Command ID to undo (from history)
    command_id: i64,
},

// In main():
Some(Commands::Undo { command_id }) => {
    // Init tracing
    let cwd = std::env::current_dir().unwrap_or_default();
    let glass_dir = glass_snapshot::resolve_glass_dir(&cwd);
    let store = glass_snapshot::SnapshotStore::open(&glass_dir).unwrap();
    let engine = glass_snapshot::UndoEngine::new(&store);
    match engine.undo_command(command_id) {
        Ok(Some(result)) => { /* print outcomes */ }
        Ok(None) => { eprintln!("No snapshot found for command {}", command_id); }
        Err(e) => { eprintln!("Undo failed: {}", e); std::process::exit(1); }
    }
}
```

This requires adding `undo_command(command_id: i64)` to `UndoEngine`.

### Pattern 4: MCP Tool Addition (MCP-01, MCP-02)
**What:** Add two new tools to GlassServer following existing rmcp patterns.
**When to use:** AI assistants querying Glass via MCP protocol.
**Implementation approach:**

The existing `glass_history` and `glass_context` tools demonstrate the full pattern: define a params struct with `#[derive(Deserialize, schemars::JsonSchema)]`, implement an `async fn` with `#[tool]` attribute, use `tokio::task::spawn_blocking` for DB access.

For GlassUndo, the MCP server needs access to the snapshot store in addition to the history DB. The `GlassServer` struct needs a `snapshot_db_path` or `glass_dir` field.

```rust
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UndoParams {
    /// Command ID to undo
    #[schemars(description = "The command ID to undo (from glass_history results)")]
    pub command_id: i64,
}

#[tool(description = "Undo a file-modifying command by restoring files to their pre-command state.")]
async fn glass_undo(&self, Parameters(params): Parameters<UndoParams>) -> Result<CallToolResult, McpError> {
    // spawn_blocking with SnapshotStore + UndoEngine
}
```

For GlassFileDiff, return the pre-command and post-command file contents:

```rust
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FileDiffParams {
    /// Command ID to inspect
    #[schemars(description = "The command ID to get file diffs for")]
    pub command_id: i64,
}
```

### Pattern 5: Storage Pruning (STOR-01)
**What:** Automatic cleanup of old/oversized snapshot storage on startup.
**When to use:** Every time the Glass terminal window opens.
**Implementation approach:**

Create a `pruner.rs` module in `glass_snapshot` that:
1. Deletes snapshots older than `retention_days`
2. If total snapshot count exceeds `max_count`, deletes oldest until under limit
3. If total blob size exceeds `max_size_mb`, deletes oldest snapshots until under limit

The pruner needs new DB queries:
- `delete_snapshots_before(epoch)` -- delete by created_at
- `count_snapshots()` -- for max_count enforcement
- `get_oldest_snapshots(limit)` -- for iterative deletion
- `get_all_blob_hashes()` -- for orphan blob cleanup
- `get_referenced_hashes()` -- to identify which blobs are still needed

After deleting snapshot records (which cascade to snapshot_files), scan the blob directory and delete any blobs not referenced by remaining snapshot_files rows.

Run pruning on a background thread at window creation to avoid blocking startup:

```rust
// In window creation, after snapshot_store is opened:
if let Some(ref store) = ctx.snapshot_store {
    let config = self.config.snapshot.clone();
    // spawn background pruning
    std::thread::spawn(move || {
        let pruner = glass_snapshot::Pruner::new(&store, &config);
        if let Err(e) = pruner.prune() {
            tracing::warn!("Storage pruning failed: {}", e);
        }
    });
}
```

**Key design consideration:** Pruning must handle the content-addressed blob store carefully. Multiple snapshots may reference the same blob hash. Only delete a blob file when NO snapshot_files rows reference it anymore. Use a SQL query: `SELECT DISTINCT blob_hash FROM snapshot_files WHERE blob_hash IS NOT NULL` to get the set of referenced hashes, then walk the blob directory and delete unreferenced files.

### Anti-Patterns to Avoid
- **Querying DB per frame for snapshot availability:** Cache `has_snapshot` on the Block struct instead
- **Deleting blobs without checking references:** Content-addressed store means multiple snapshots share blobs
- **Blocking startup with pruning:** Always run on background thread
- **Writing undo feedback to shell stdin:** Will be echoed/interpreted by the shell; use overlay or direct terminal output instead

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| MCP tool schema | Manual JSON schema | schemars derive + rmcp #[tool] | Auto-generates correct JSON Schema from Rust types |
| CLI argument parsing | Manual arg parsing | clap derive | Already used, handles help text, validation, subcommands |
| Text rendering | Custom font rasterization | glyphon via existing BlockLabel pipeline | Already working, GPU-accelerated |
| SQLite query building | String concatenation | rusqlite params! macro | SQL injection prevention, type safety |

## Common Pitfalls

### Pitfall 1: Blob Reference Counting During Pruning
**What goes wrong:** Deleting snapshot records cascades to snapshot_files, but blob files on disk are not automatically cleaned up. Conversely, deleting a blob that is still referenced by another snapshot corrupts the store.
**Why it happens:** Content-addressed deduplication means the relationship between blobs and snapshots is many-to-many.
**How to avoid:** After deleting snapshot DB records, query remaining referenced hashes, then walk the blob directory and delete only unreferenced blobs.
**Warning signs:** Blob directory grows without bound, or undo fails with "blob not found".

### Pitfall 2: MCP Server Needs Both DB Paths
**What goes wrong:** The MCP server currently only resolves `history.db` path. For GlassUndo and GlassFileDiff, it also needs access to `snapshots.db` and the blob store.
**Why it happens:** The MCP server was built before the snapshot system existed.
**How to avoid:** Resolve the `.glass/` directory (not just db path) in `run_mcp_server()`, pass it to GlassServer, and open SnapshotStore alongside HistoryDb.

### Pitfall 3: Block has_snapshot Not Updated After Undo
**What goes wrong:** After undoing a command, the `[undo]` label still shows because `has_snapshot` is still true.
**Why it happens:** Block state is not updated after undo operation.
**How to avoid:** After successful undo, either remove the snapshot from DB (preventing re-undo) or set `has_snapshot = false` on the block. Consider whether undo should be a one-shot operation or allow re-undo.

### Pitfall 4: Pruning Deletes Currently-In-Use Snapshot
**What goes wrong:** Background pruning thread deletes a snapshot that the user is about to undo.
**Why it happens:** No locking between pruning and undo operations.
**How to avoid:** SQLite WAL mode provides read isolation. Pruning should skip the most recent N snapshots (e.g., last 10) regardless of age. Additionally, the pruner should be a brief operation that completes quickly.

### Pitfall 5: UndoEngine undo_command vs undo_latest Divergence
**What goes wrong:** `undo_command(id)` and `undo_latest()` have different logic paths that drift over time.
**Why it happens:** Copy-paste with modifications.
**How to avoid:** Extract the core restore logic into a private method that both call. `undo_latest` finds the snapshot, `undo_command` finds the snapshot by command_id -- both delegate to the same `restore_snapshot(snapshot)` method.

### Pitfall 6: Undo Feedback Text Encoding
**What goes wrong:** File paths with non-ASCII characters corrupt the feedback display.
**Why it happens:** Mixing string representations of paths.
**How to avoid:** Use `path.display()` consistently and ensure all feedback text is valid UTF-8.

## Code Examples

### Adding a New MCP Tool (verified pattern from existing glass_mcp/tools.rs)
```rust
// Source: crates/glass_mcp/src/tools.rs (existing pattern)
#[tool(description = "Tool description here.")]
async fn glass_tool_name(
    &self,
    Parameters(params): Parameters<ToolParams>,
) -> Result<CallToolResult, McpError> {
    let db_path = self.db_path.clone();
    let result = tokio::task::spawn_blocking(move || {
        // DB operations here
    })
    .await
    .map_err(internal_err)??;
    let content = Content::json(&result)?;
    Ok(CallToolResult::success(vec![content]))
}
```

### Adding a CLI Subcommand (verified pattern from existing Commands enum)
```rust
// Source: src/main.rs (existing pattern)
#[derive(Subcommand, Debug, PartialEq)]
enum Commands {
    // ... existing variants ...
    /// Undo a file-modifying command
    Undo {
        /// Command ID to undo
        command_id: i64,
    },
}
```

### Adding a Block Label (verified pattern from block_renderer.rs)
```rust
// Source: crates/glass_renderer/src/block_renderer.rs (existing pattern)
labels.push(BlockLabel {
    x: calculated_x,
    y: calculated_y,
    text: "[undo]".to_string(),
    color: Rgb { r: 100, g: 160, b: 220 },
});
```

### Pruning Query Pattern
```rust
// Delete snapshots older than retention period
pub fn delete_snapshots_before(&self, epoch: i64) -> Result<Vec<i64>> {
    let mut stmt = self.conn.prepare(
        "SELECT id FROM snapshots WHERE created_at < ?1"
    )?;
    let ids: Vec<i64> = stmt.query_map(params![epoch], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    self.conn.execute("DELETE FROM snapshots WHERE created_at < ?1", params![epoch])?;
    Ok(ids) // Return deleted IDs for blob cleanup
}

// Get all referenced blob hashes (for orphan detection)
pub fn get_referenced_hashes(&self) -> Result<std::collections::HashSet<String>> {
    let mut stmt = self.conn.prepare(
        "SELECT DISTINCT blob_hash FROM snapshot_files WHERE blob_hash IS NOT NULL"
    )?;
    let hashes = stmt.query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<std::collections::HashSet<_>, _>>()?;
    Ok(hashes)
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Undo only via Ctrl+Shift+Z | Also via CLI + MCP | Phase 14 | Broader accessibility |
| No storage limits | Config-driven pruning | Phase 14 | Prevents unbounded disk growth |
| Undo feedback via tracing only | Visual feedback in terminal | Phase 14 | User can see what happened |

## Open Questions

1. **Undo visual feedback mechanism**
   - What we know: PTY input injection would pollute shell state. Search overlay pattern exists for temporary displays.
   - What's unclear: Best UX for showing undo results -- overlay vs inline vs PTY output write.
   - Recommendation: Use a timed notification overlay (similar to search overlay but simpler). Display for 3-5 seconds, auto-dismiss. If too complex, writing ANSI-colored text directly through the event proxy as terminal output (not shell stdin) is acceptable.

2. **Should undo be one-shot?**
   - What we know: Currently `undo_latest()` always returns the most recent parser snapshot. After undo, the snapshot still exists.
   - What's unclear: Should undoing delete or mark the snapshot as "undone"? Can the user undo the same command twice?
   - Recommendation: Mark snapshot as undone (add `undone_at` column or delete it). Prevents confusion. For V1, deleting the snapshot after successful undo is simplest.

3. **Pruning trigger frequency**
   - What we know: Requirement says "on startup". Config has max_age, max_size, max_count.
   - What's unclear: Should pruning also run periodically during long sessions?
   - Recommendation: Startup-only for V1. Long sessions are rare for terminal emulators (users close/reopen). Periodic pruning adds complexity for minimal benefit.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test + cargo test |
| Config file | Cargo.toml (workspace) |
| Quick run command | `cargo test -p glass_snapshot --lib` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| UI-01 | [undo] label generated for blocks with snapshots | unit | `cargo test -p glass_renderer block_renderer` | Needs new tests |
| UI-02 | Undo feedback shows per-file outcomes | unit | `cargo test -p glass_snapshot undo` | Needs new tests |
| UI-03 | CLI undo subcommand parses and executes | unit + integration | `cargo test -p glass undo` | Needs new tests |
| STOR-01 | Pruning deletes old snapshots and orphan blobs | unit | `cargo test -p glass_snapshot pruner` | Wave 0 (new file) |
| MCP-01 | GlassUndo tool triggers undo via MCP | unit | `cargo test -p glass_mcp glass_undo` | Needs new tests |
| MCP-02 | GlassFileDiff tool returns file diffs | unit | `cargo test -p glass_mcp glass_file_diff` | Needs new tests |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_snapshot --lib && cargo test -p glass_mcp --lib`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before verification

### Wave 0 Gaps
- [ ] `crates/glass_snapshot/src/pruner.rs` -- new module, needs test fixtures
- [ ] Tests for `undo_command(command_id)` in `undo.rs`
- [ ] Tests for pruning DB queries in `db.rs`
- [ ] Tests for MCP undo/diff tools in `glass_mcp/src/tools.rs`
- [ ] Tests for `[undo]` label positioning in `block_renderer.rs`
- [ ] Tests for `has_snapshot` field on Block struct

## Sources

### Primary (HIGH confidence)
- Codebase analysis: `crates/glass_mcp/src/tools.rs` -- existing MCP tool pattern with rmcp v1
- Codebase analysis: `crates/glass_renderer/src/block_renderer.rs` -- BlockLabel rendering pattern
- Codebase analysis: `crates/glass_snapshot/src/undo.rs` -- UndoEngine with undo_latest()
- Codebase analysis: `crates/glass_snapshot/src/db.rs` -- SnapshotDb with delete, query methods
- Codebase analysis: `crates/glass_snapshot/src/blob_store.rs` -- BlobStore with delete_blob()
- Codebase analysis: `crates/glass_core/src/config.rs` -- SnapshotSection with max_count, max_size_mb, retention_days
- Codebase analysis: `src/main.rs` -- CLI Commands enum, Ctrl+Shift+Z handler, window creation

### Secondary (MEDIUM confidence)
- rmcp crate API (v1) -- tool_router, tool_handler, tool macros (verified via existing usage)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - all libraries already in workspace, no new dependencies needed
- Architecture: HIGH - all patterns directly observed in existing codebase
- Pitfalls: HIGH - derived from concrete code analysis (blob reference counting, DB path resolution, block state)
- UI feedback: MEDIUM - overlay approach is sound but implementation details depend on FrameRenderer capabilities

**Research date:** 2026-03-05
**Valid until:** 2026-04-05 (stable codebase, no external dependency changes expected)

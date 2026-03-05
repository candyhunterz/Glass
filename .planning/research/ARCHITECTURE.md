# Architecture Patterns: v1.2 Command-Level Undo

**Domain:** Command-level undo with filesystem snapshots for terminal emulator
**Researched:** 2026-03-05
**Confidence:** HIGH (based on direct source code analysis of existing Glass v1.1 codebase, notify crate docs, BLAKE3 docs)

---

## System Overview: v1.2 Additions

```
+-----------------------------------------------------------------------+
|                        Application Layer                               |
|  glass binary (main.rs)                                                |
|  +----------------+  +----------------+  +--------------------------+  |
|  |  Processor     |  |  WindowCtx     |  |  CLI Subcommands         |  |
|  |  (winit loop)  |  |  (per-window)  |  |  glass undo <id>   NEW  |  |
|  +-------+--------+  +-------+--------+  +--------------------------+  |
|          |                    |                                         |
+----------+--------------------+----------------------------------------+
|          |    Service Layer   |                                         |
|  +-------v--------------+    |  +--------------+  +----------------+   |
|  |  glass_terminal      |    |  |glass_renderer|  |  glass_core    |   |
|  |  PTY + OscScanner    |    |  | wgpu + GPU   |  |  events,config |   |
|  |  BlockManager        |    |  |              |  |                |   |
|  |  OutputCapture       |    |  | +[undo] label|  | +SnapshotCfg  |   |
|  +-----------------------+   |  |  NEW         |  |  NEW           |   |
|                              |  +--------------+  +----------------+   |
|  +-----------------------+   |                                         |
|  |  glass_snapshot  NEW  |   |  +----------------------------------+   |
|  |  SnapshotStore       <----+  |  glass_mcp                      |   |
|  |  CommandParser        |   |  |  +GlassUndo tool          NEW   |   |
|  |  FsWatcher (notify)   |   |  |  +GlassFileDiff tool      NEW  |   |
|  |  UndoEngine           |   |  +----------------------------------+   |
|  +-----------------------+   |                                         |
|                              |  +----------------------------------+   |
|  +-----------------------+   |  |  glass_history (unchanged)       |   |
|  |  glass_config         |   +-->  SQLite + FTS5                   |   |
|  |  (unchanged)          |      |  CommandRecord, HistoryDb        |   |
|  +-----------------------+      +----------------------------------+   |
+------------------------------------------------------------------------+
```

---

## Existing Architecture (What We Build On)

Understanding the existing data flow is critical. Here is how a command lifecycle currently works:

```
PTY Reader Thread (std::thread)         Main Thread (winit event loop)
================================         ==============================

OscScanner detects OSC 133;C
  -> AppEvent::Shell{Executed}  -------> Processor::user_event:
                                           block_manager.handle_event()
                                           command_started_wall = now()

... PTY output flows ...                ... output_buffer accumulates ...

OscScanner detects OSC 133;D
  -> AppEvent::Shell{Finished}  -------> Processor::user_event:
  -> AppEvent::CommandOutput    -------> 1. Extract command text from grid
                                         2. HistoryDb::insert_command()
                                         3. process_output() + update_output()
```

**Key architectural facts from code analysis:**

1. **PTY reader thread** runs on `std::thread` (not tokio). It holds the `Term` lock briefly, scans OSC sequences, and sends `AppEvent` variants through `EventLoopProxy<AppEvent>`.

2. **Command text extraction** currently happens at `CommandFinished` time in `Processor::user_event`. It reads from the terminal grid between `block.command_start_line` and `block.output_start_line`.

3. **History DB** is owned by `WindowContext` on the main thread (`history_db: Option<HistoryDb>`). Inserts happen synchronously in `user_event` -- this is fine because SQLite inserts are <1ms.

4. **Block lifecycle** is tracked by `BlockManager` with states: PromptActive -> InputActive -> Executing -> Complete. Each block records line numbers and timing.

5. **Keybindings** are handled in `window_event` for `KeyboardInput`. Ctrl+Shift+{C,V,F} are already intercepted. Adding Ctrl+Shift+Z follows the same pattern.

6. **glass_snapshot** is currently an empty stub crate with only `//! glass_snapshot -- stub crate, filled in future phases`.

---

## New Components

### 1. glass_snapshot::SnapshotStore

Content-addressed file storage using BLAKE3 hashing with SQLite metadata.

**Storage layout:**
```
.glass/
  history.db          (existing -- glass_history)
  snapshots.db        (NEW -- snapshot metadata)
  blobs/
    ab/
      ab3f...7e.blob  (content-addressed files, 2-char directory sharding)
```

**Schema (snapshots.db, separate from history.db):**
```sql
CREATE TABLE snapshots (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    command_id  INTEGER NOT NULL,     -- matches commands.id in history.db
    cwd         TEXT NOT NULL,
    created_at  INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX idx_snapshots_command ON snapshots(command_id);

CREATE TABLE snapshot_files (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    file_path   TEXT NOT NULL,         -- absolute path
    blob_hash   TEXT,                  -- BLAKE3 hash, NULL = file did not exist
    file_size   INTEGER,              -- original file size in bytes
    source      TEXT NOT NULL DEFAULT 'parser'  -- 'parser' or 'watcher'
);
CREATE INDEX idx_sf_snapshot ON snapshot_files(snapshot_id);
CREATE INDEX idx_sf_hash ON snapshot_files(blob_hash);

CREATE TABLE fs_changes (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    file_path   TEXT NOT NULL,
    change_type TEXT NOT NULL,          -- 'create', 'modify', 'delete', 'rename'
    detected_at INTEGER NOT NULL
);
CREATE INDEX idx_fc_snapshot ON fs_changes(snapshot_id);
```

**Why separate DB from history.db:**
- glass_snapshot must NOT depend on glass_history (different crate, different concern)
- Blob storage can grow large; separate DB allows independent vacuum/pruning
- Schema versioning is independent (PRAGMA user_version tracks separately)
- Follows existing pattern: both DBs resolve to the same `.glass/` directory via the same ancestor-walk logic

**Content deduplication:** Files with identical BLAKE3 hashes share a single blob on disk. Before writing, check if blob exists. During pruning, delete blobs with zero remaining references.

**Why BLAKE3 over SHA-256:** BLAKE3 is 5-10x faster on modern hardware, has a clean single-crate Rust implementation (`blake3` 1.6+), and provides more than sufficient collision resistance for local file deduplication (this is not cryptographic authentication).

**Key struct:**
```rust
pub struct SnapshotStore {
    conn: rusqlite::Connection,
    blob_dir: PathBuf,
}

impl SnapshotStore {
    pub fn open(glass_dir: &Path) -> Result<Self>;
    pub fn create_snapshot(&self, command_id: i64, cwd: &str) -> Result<i64>;
    pub fn store_file(&self, snapshot_id: i64, path: &Path, source: &str) -> Result<()>;
    pub fn record_change(&self, snapshot_id: i64, path: &Path, change_type: &str) -> Result<()>;
    pub fn get_snapshot_files(&self, snapshot_id: i64) -> Result<Vec<SnapshotFile>>;
    pub fn get_changes(&self, snapshot_id: i64) -> Result<Vec<FsChange>>;
    pub fn find_by_command(&self, command_id: i64) -> Result<Option<i64>>;
    pub fn restore_snapshot(&self, snapshot_id: i64) -> Result<RestoreReport>;
    pub fn prune(&self, max_age_days: u32, max_size_bytes: u64) -> Result<u64>;
    pub fn total_blob_size(&self) -> Result<u64>;
}
```

### 2. glass_snapshot::CommandParser

Heuristic parser that extracts file/directory targets from command text. NOT a full shell parser -- handles common cases and is honest about limitations.

```rust
pub struct ParseResult {
    pub targets: Vec<PathBuf>,
    pub confidence: Confidence,
    pub watch_cwd: bool,
}

pub enum Confidence {
    High,      // Known destructive command with clear targets (rm, mv, sed -i)
    Low,       // Unknown command or ambiguous targets
    ReadOnly,  // Command is read-only (ls, cat, grep) -- skip snapshot
}

pub fn parse_command(command_text: &str, cwd: &Path) -> ParseResult;
```

**Known command patterns:**

| Pattern | Confidence | Targets | watch_cwd |
|---------|-----------|---------|-----------|
| `rm file1 file2` | High | file1, file2 | false |
| `mv src dst` | High | src | false |
| `cp src dst` | High | dst (overwrite) | false |
| `sed -i 's/a/b/' file` | High | file | false |
| `chmod 755 file` | High | file | false |
| `echo text > file` | High | file (redirect target) | false |
| `cargo build` | Low | none | true |
| `npm install` | Low | none | true |
| `make` | Low | none | true |
| `./script.sh` | Low | none | true |
| Unknown command | Low | none | true |
| `ls`, `cat`, `grep`, `git log` | ReadOnly | none | false |

**Path resolution:** All relative paths are resolved against `cwd`. Glob patterns (e.g., `rm *.txt`) are expanded via `std::fs::read_dir` + pattern matching. Quoted arguments are unquoted. Flag arguments (starting with `-`) are skipped except for recognized flag-value pairs (e.g., `sed -i`).

**Why heuristic, not full parser:** Shell syntax is impossible to parse fully without executing it (aliases, variable expansion, subshells, command substitution). A heuristic parser covers ~80% of real-world destructive commands. The FS watcher catches the remaining ~20%. The UI honestly communicates limitations.

### 3. glass_snapshot::FsWatcher

Wraps the `notify` crate (v8.2) for command-scoped filesystem monitoring.

**Platform backends (via notify::RecommendedWatcher):**
- Windows: ReadDirectoryChangesW
- Linux: inotify
- macOS: FSEvents

```rust
pub struct CommandWatcher {
    watcher: Option<notify::RecommendedWatcher>,
    changes: Arc<Mutex<Vec<FsChangeEvent>>>,
    cwd: PathBuf,
}

impl CommandWatcher {
    pub fn start(cwd: &Path, ignore_patterns: &[String]) -> Result<Self>;
    pub fn stop(&mut self) -> Vec<FsChangeEvent>;
}

pub struct FsChangeEvent {
    pub path: PathBuf,
    pub kind: ChangeKind,
    pub timestamp: i64,
}

pub enum ChangeKind {
    Create,
    Modify,
    Delete,
    Rename { from: Option<PathBuf> },
}
```

**Lifecycle:**
1. `CommandExecuted` -> `CommandWatcher::start(cwd)` with recursive watching
2. notify's internal thread collects events into `Arc<Mutex<Vec<FsChangeEvent>>>`
3. `CommandFinished` -> `CommandWatcher::stop()` drains and returns all events
4. Events are filtered, deduplicated, and recorded in SnapshotStore

**Noise filtering (default ignore list):**
- `.git/` internals (index, objects, refs -- but NOT `.gitignore`)
- `target/` (Rust build directory)
- `node_modules/`
- `*.tmp`, `*.swp`, `*.lock` (editor swap/lock files)
- `.glass/` directory itself
- Configurable via `[snapshot] ignore_dirs` in config.toml

**Threading model:** The `notify` crate spawns its own internal OS thread for the platform backend. Events are pushed into an `Arc<Mutex<Vec>>`. The main thread drains this on `CommandFinished`. This is simple and avoids async coordination. The Mutex contention is negligible because the main thread only accesses it once (at stop time), while notify's thread appends events.

### 4. glass_snapshot::UndoEngine

Orchestrates file restoration with conflict detection.

```rust
pub struct RestoreReport {
    pub restored: Vec<PathBuf>,
    pub created_removed: Vec<PathBuf>,  // files created by command, now deleted
    pub skipped: Vec<(PathBuf, SkipReason)>,
    pub errors: Vec<(PathBuf, String)>,
}

pub enum SkipReason {
    ModifiedAfterCommand,  // file changed by a later command
    NoSnapshotData,        // watcher detected change but no pre-state
    PermissionDenied,
}

impl UndoEngine {
    /// Undo the most recent undoable command for the current CWD.
    pub fn undo_latest(store: &SnapshotStore, cwd: &str) -> Result<Option<RestoreReport>>;

    /// Undo a specific command by its command_id.
    pub fn undo_command(store: &SnapshotStore, command_id: i64) -> Result<RestoreReport>;
}
```

**Restore strategy:**
1. Look up snapshot by command_id
2. For each `snapshot_file` with `blob_hash IS NOT NULL`: copy blob -> original path
3. For each `snapshot_file` with `blob_hash IS NULL`: the file did not exist before, so delete the current file if it exists
4. For each `fs_change` of type 'create' NOT covered by snapshot_files: delete (file was created by command)
5. Report results

**Conflict detection:** Before restoring, compare the current file state against what we expect. If a file was modified by a SUBSEQUENT command (checked via `SELECT ... FROM snapshots WHERE command_id > ? AND snapshot_id IN (SELECT snapshot_id FROM snapshot_files WHERE file_path = ?)`), skip it and report `ModifiedAfterCommand`.

---

## Modifications to Existing Components

### glass_core::event -- New AppEvent Variant

```rust
pub enum AppEvent {
    // ... existing variants unchanged ...

    /// Result of an undo operation, for UI feedback.
    UndoComplete {
        window_id: WindowId,
        command_text: String,
        restored_count: usize,
        error_count: usize,
    },
}
```

**Why only UndoComplete, not more:** The snapshot lifecycle (create, store files, record changes) runs synchronously on the main thread during `user_event` handling. No new AppEvent is needed for those -- they piggyback on existing `Shell { CommandExecuted }` and `Shell { CommandFinished }` handling. Only the undo result needs a separate event because it triggers UI feedback (status bar message or toast).

**Alternative considered:** Adding a `CommandText` event to carry command text from `CommandExecuted`. Rejected because command text extraction already works in the existing `CommandFinished` handler by reading the terminal grid. The same grid-reading technique works at `CommandExecuted` time -- the command text is still visible in the grid because the prompt line has not scrolled away yet.

### glass_core::config -- SnapshotSection

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct GlassConfig {
    // ... existing fields ...
    pub snapshot: Option<SnapshotSection>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SnapshotSection {
    #[serde(default = "default_true")]
    pub enabled: bool,                    // default: true
    #[serde(default = "default_500")]
    pub max_storage_mb: u32,              // default: 500
    #[serde(default = "default_7")]
    pub max_age_days: u32,                // default: 7
    #[serde(default = "default_ignore")]
    pub ignore_dirs: Vec<String>,         // default: [".git", "target", "node_modules"]
}
```

### main.rs -- WindowContext Extensions

```rust
struct WindowContext {
    // ... existing fields ...

    /// Snapshot store for this window.
    snapshot_store: Option<SnapshotStore>,
    /// Active filesystem watcher during command execution. None when no command running.
    active_watcher: Option<CommandWatcher>,
    /// Snapshot ID being populated during current command execution.
    active_snapshot_id: Option<i64>,
}
```

### main.rs -- Event Flow Changes

**On `Shell { CommandExecuted }`** (additions to existing handler):

```rust
ShellEvent::CommandExecuted => {
    // EXISTING: block_manager.handle_event(), command_started_wall = now()

    // NEW: Extract command text NOW (before command runs)
    let command_text = extract_command_text_from_grid(&ctx.term, &ctx.block_manager);

    // NEW: Parse command for file targets
    let cwd = ctx.status.cwd().to_string();
    let parse_result = glass_snapshot::CommandParser::parse_command(&command_text, &cwd);

    if parse_result.confidence != Confidence::ReadOnly {
        if let Some(ref store) = ctx.snapshot_store {
            // Create snapshot record
            let snap_id = store.create_snapshot(0, &cwd); // command_id=0, updated later
            ctx.active_snapshot_id = snap_id.ok();

            // Pre-exec snapshot of parser-identified targets
            for target in &parse_result.targets {
                let _ = store.store_file(snap_id, target, "parser");
            }

            // Start FS watcher if needed
            if parse_result.watch_cwd {
                let ignore = config.snapshot.ignore_dirs.clone();
                ctx.active_watcher = CommandWatcher::start(&cwd, &ignore).ok();
            }
        }
    }
}
```

**On `Shell { CommandFinished }`** (additions to existing handler):

```rust
ShellEvent::CommandFinished { exit_code } => {
    // EXISTING: extract command text, insert HistoryDb record

    // NEW: Stop watcher and record changes
    if let Some(mut watcher) = ctx.active_watcher.take() {
        let changes = watcher.stop();
        if let (Some(ref store), Some(snap_id)) = (&ctx.snapshot_store, ctx.active_snapshot_id) {
            for change in &changes {
                let _ = store.record_change(snap_id, &change.path, &change.kind.as_str());
            }
            // Update snapshot's command_id now that we have it
            if let Some(cmd_id) = ctx.last_command_id {
                let _ = store.update_command_id(snap_id, cmd_id);
            }
        }
    }
    ctx.active_snapshot_id = None;
}
```

**Ctrl+Shift+Z keybinding** (addition to existing keyboard handler):

```rust
// In the Ctrl+Shift section of KeyboardInput handler:
Key::Character(c) if c.as_str().eq_ignore_ascii_case("z") => {
    if let Some(ref store) = ctx.snapshot_store {
        let cwd = ctx.status.cwd().to_string();
        match glass_snapshot::UndoEngine::undo_latest(store, &cwd) {
            Ok(Some(report)) => {
                // Show result in status bar or toast
                tracing::info!("Undo: restored {} files, {} errors",
                    report.restored.len(), report.errors.len());
            }
            Ok(None) => {
                tracing::info!("Nothing to undo");
            }
            Err(e) => {
                tracing::warn!("Undo failed: {}", e);
            }
        }
        ctx.window.request_redraw();
    }
    return;
}
```

### glass_renderer -- [undo] Label on Blocks

Extend `BlockRenderer::build_block_text` to render an "[undo]" label on blocks that have associated snapshots.

```rust
pub fn build_block_text(
    &self,
    blocks: &[&Block],
    display_offset: usize,
    screen_lines: usize,
    viewport_width: f32,
    undoable_epochs: &HashSet<i64>,  // NEW: started_epoch values with snapshots
) -> Vec<BlockLabel>
```

The `undoable_epochs` set is populated by querying the snapshot store for all command_ids that have snapshots, then mapping those to block `started_epoch` values. This query runs once per frame only when blocks are visible (cheap: `SELECT DISTINCT command_id FROM snapshots`).

### glass_mcp -- New Tools

```rust
#[tool(description = "Undo filesystem changes made by a specific command.")]
async fn glass_undo(&self, params: UndoParams) -> Result<CallToolResult, McpError>;

#[tool(description = "Show what files were changed by a specific command.")]
async fn glass_file_diff(&self, params: FileDiffParams) -> Result<CallToolResult, McpError>;
```

The MCP server opens a read-only connection to snapshots.db alongside the existing read-only connection to history.db. GlassUndo calls UndoEngine directly. GlassFileDiff queries fs_changes + snapshot_files.

---

## Complete Data Flow: Snapshot Lifecycle

```
User types "rm important.txt" + Enter
    |
    +-- Shell emits OSC 133;C (command about to execute)
    |
    v
PTY Reader Thread:
    OscScanner detects CommandExecuted
    -> AppEvent::Shell { event: CommandExecuted, line: N }
    |
    v
Main Thread (Processor::user_event):
    1. block_manager.handle_event()              [EXISTING]
    2. command_started_wall = now()               [EXISTING]
    3. Extract "rm important.txt" from grid       [NEW - moved earlier]
    4. CommandParser::parse_command("rm important.txt", cwd)
       -> ParseResult { targets: [cwd/important.txt], confidence: High, watch_cwd: false }
    5. SnapshotStore::create_snapshot(command_id=0, cwd)
       -> snapshot_id = 42
    6. SnapshotStore::store_file(42, "important.txt", "parser")
       -> reads file content, BLAKE3 hash, writes to blobs/ab/ab3f...blob
       -> inserts snapshot_files row
    7. active_snapshot_id = Some(42)
    |
    ... rm executes, file is deleted ...
    |
    +-- Shell emits OSC 133;D;0 (command finished, exit 0)
    |
    v
PTY Reader Thread:
    OscScanner detects CommandFinished
    -> AppEvent::Shell { event: CommandFinished{exit_code: Some(0)}, line: M }
    |
    v
Main Thread (Processor::user_event):
    8. block_manager.handle_event()               [EXISTING]
    9. HistoryDb::insert_command() -> cmd_id=99   [EXISTING]
    10. store.update_command_id(42, 99)            [NEW]
    11. active_snapshot_id = None                  [NEW]
    |
    ... user realizes mistake, presses Ctrl+Shift+Z ...
    |
    v
Main Thread (Processor::window_event):
    12. UndoEngine::undo_latest(store, cwd)
    13. find_by_command(99) -> snapshot_id=42
    14. get_snapshot_files(42) -> [{path: "important.txt", hash: "ab3f...", source: "parser"}]
    15. Read blob from blobs/ab/ab3f...blob
    16. Write content back to cwd/important.txt
    17. RestoreReport { restored: ["important.txt"], skipped: [], errors: [] }
    18. Display: "Restored 1 file"
```

---

## Crate Dependency Graph (After v1.2)

```
glass_core (events, config, error)
    ^           ^           ^
    |           |           |
glass_terminal  |     glass_snapshot [NEW]
    ^           |       ^       ^
    |           |       |       |
glass_renderer  |       |   glass_mcp
    ^           |       |       ^
    |           |       |       |
    +-----+-----+------+-------+
          |
       root binary (Processor coordinates everything)
          |
     glass_history
```

**Critical dependency rule:** glass_snapshot does NOT depend on glass_history or glass_terminal. The command_id is just an `i64`. The root binary is the sole coordinator, following the exact same pattern as v1.1 where the root binary coordinates glass_history and glass_terminal without either depending on the other.

**New dependency for glass_snapshot:**
```toml
[dependencies]
rusqlite = { workspace = true }
blake3 = "1.6"
notify = "8.2"
anyhow = { workspace = true }
tracing = { workspace = true }
dirs = { workspace = true }
```

---

## Patterns to Follow

### Pattern 1: Event-Driven Coordination (Matches Existing Architecture)

All inter-component communication goes through the `AppEvent` channel via `EventLoopProxy`, exactly like existing Shell, GitInfo, and CommandOutput events. The snapshot lifecycle piggybacks on existing `Shell { CommandExecuted }` and `Shell { CommandFinished }` events -- no new channel infrastructure needed.

### Pattern 2: Non-Fatal Degradation

Snapshot failures (disk full, permission denied, watcher failure) log warnings and continue. The terminal must remain usable when snapshotting fails. This matches the existing pattern where `history_db` failure logs a warning and sets the field to `None`.

```rust
// Same pattern as existing history_db initialization:
let snapshot_store = match SnapshotStore::open(&glass_dir) {
    Ok(store) => {
        tracing::info!("Snapshot store opened");
        Some(store)
    }
    Err(e) => {
        tracing::warn!("Failed to open snapshot store: {} -- undo disabled", e);
        None
    }
};
```

### Pattern 3: Blocking I/O on Main Thread (For Small Operations)

Pre-exec snapshots (typically 1-10 files, <1MB each) run synchronously on the main thread during `CommandExecuted` handling. The time between OSC 133;C and actual command execution is essentially zero -- the shell has already started running the command by the time the event reaches the main thread. Snapshotting a few files (read + hash + write blob) takes <10ms. This matches the existing pattern where `HistoryDb::insert_command()` runs synchronously on the main thread.

**Exception rule:** If the parser identifies >20 targets or any file >10MB, log a warning and skip those files. The FS watcher still records what changed.

### Pattern 4: Separate DB, Same Directory

snapshots.db lives alongside history.db in `.glass/`, with independent schemas and PRAGMA user_version. Both resolve to the same directory via the existing `resolve_db_path` ancestor-walk logic (reuse or clone the function in glass_snapshot).

### Pattern 5: Resolve DB Path Consistently

glass_snapshot needs the same `.glass/` directory resolution as glass_history. Rather than creating a dependency, duplicate the 15-line `resolve_db_path` function (or extract it to glass_core if cleaner). The function walks up from CWD looking for `.glass/`, falling back to `~/.glass/`.

---

## Anti-Patterns to Avoid

### Anti-Pattern 1: Snapshotting Inside the PTY Reader Thread

**What:** Running file I/O (reads, hashes, blob writes) on the PTY reader thread.
**Why bad:** The PTY reader thread is the most latency-critical code path. Any blocking I/O adds input lag. The existing architecture keeps this thread minimal: read bytes, scan OSC, feed VTE parser.
**Instead:** Send events to the main thread; do snapshot I/O there.

### Anti-Pattern 2: glass_snapshot Depending on glass_history

**What:** Importing `HistoryDb` or `CommandRecord` from glass_history into glass_snapshot.
**Why bad:** Creates coupling between unrelated concerns. The command_id is just an i64 -- no need to import the entire history module.
**Instead:** glass_snapshot takes `command_id: i64` as a parameter. The root binary coordinates both crates.

### Anti-Pattern 3: Full Shell Parsing

**What:** Building or importing a complete POSIX/PowerShell parser.
**Why bad:** Shell syntax is context-dependent (aliases, functions, variable expansion, command substitution). No parser handles `eval $(generate_commands)`. Enormous complexity for diminishing returns.
**Instead:** Heuristic parser for known commands + FS watcher as safety net. Honest UI about limitations.

### Anti-Pattern 4: Watching Entire Home Directory

**What:** Recursive FS watching on `~` or `/`.
**Why bad:** Thousands of irrelevant events, exhausts inotify watches (Linux default 8192), wastes CPU.
**Instead:** Watch only the command's CWD recursively. Most commands operate within CWD.

### Anti-Pattern 5: Storing File Contents as SQLite BLOBs

**What:** Putting file binary data directly in the SQLite database.
**Why bad:** SQLite performance degrades with large BLOBs. Vacuum becomes expensive. DB file grows unbounded.
**Instead:** Content-addressed blob files on the filesystem, hash stored in SQLite. Enables filesystem-level dedup and efficient pruning.

### Anti-Pattern 6: Starting Watcher Before Snapshot

**What:** Starting the FS watcher before taking the pre-exec snapshot.
**Why bad:** Race condition -- if the command starts modifying files before the snapshot completes, you capture a partial/corrupted pre-state.
**Instead:** Take pre-exec snapshot FIRST (synchronous, blocking), THEN start the watcher. The ordering matters.

---

## Suggested Build Order

### Phase 1: SnapshotStore Core (Foundation)

- glass_snapshot: `SnapshotStore::open`, `create_snapshot`, `store_file`, `get_snapshot_files`, `restore_snapshot`
- BLAKE3 hashing with 2-char directory sharding for blobs
- Schema creation with PRAGMA user_version migration support
- `resolve_snapshot_db_path` (same logic as glass_history's resolve_db_path)
- Unit tests with tempdir

**Why first:** Everything else depends on having a place to store and retrieve snapshots. Pure library code, zero integration dependencies.

### Phase 2: CommandParser

- `parse_command(text, cwd) -> ParseResult`
- Pattern matching for destructive commands (rm, mv, sed -i, cp, chmod, redirect)
- ReadOnly detection (ls, cat, grep, git status, pwd)
- Path resolution (relative -> absolute using cwd)
- Glob expansion for wildcard arguments
- Comprehensive unit tests per command pattern

**Why second:** No dependencies on other new code. Pure functions, fully testable in isolation.

### Phase 3: FsWatcher

- `CommandWatcher::start(cwd, ignore)` / `stop() -> Vec<FsChangeEvent>`
- notify crate integration with RecommendedWatcher
- Noise filtering (ignore patterns for .git, target, node_modules, etc.)
- Deduplication of rapid modify events on the same path
- Integration tests with actual filesystem modifications

**Why third:** Depends on SnapshotStore for recording changes. Introduces the `notify` dependency.

### Phase 4: Main Thread Integration + Undo Engine

- Extend `Processor::user_event` for CommandExecuted (pre-exec snapshot + watcher start)
- Extend `Processor::user_event` for CommandFinished (stop watcher + record changes)
- Move command text extraction to CommandExecuted time
- UndoEngine with conflict detection
- Ctrl+Shift+Z keybinding in keyboard handler
- Config extensions (SnapshotSection)
- Add snapshot_store and active_watcher to WindowContext

**Why fourth:** Requires Phases 1-3 to be complete. This is the integration phase where existing crate boundaries get extended.

### Phase 5: UI + CLI + MCP + Pruning

- [undo] label on command blocks (glass_renderer extension)
- `glass undo <command-id>` CLI subcommand
- GlassUndo and GlassFileDiff MCP tools in glass_mcp
- Auto-pruning on startup (max_age_days, max_storage_mb)
- Undo result feedback (status bar message)
- `undoable_epochs` set for renderer

**Why last:** All infrastructure must exist first. These are presentation and lifecycle management features built on top of the core engine.

---

## Scalability Considerations

| Concern | At 100 cmds/day | At 1K cmds/day | Mitigation |
|---------|-----------------|----------------|------------|
| Blob storage | ~50MB (small files) | ~500MB | Auto-prune by age (7d), size cap (500MB), content dedup |
| SQLite metadata | Negligible | ~1MB | Prune alongside blobs |
| Inotify watches (Linux) | ~100 | ~100 (1 watcher at a time) | Stop watcher on CommandFinished |
| ReadDirectoryChangesW | Negligible | Negligible | Single recursive watch per command |
| Pre-exec snapshot latency | <5ms (1-3 files) | <5ms | Only snapshot parser-identified targets |
| Undo restore time | <50ms | <50ms | Direct blob-to-file copy |
| Watcher event volume | ~10/cmd | ~10/cmd | Noise filtering, dedup |

---

## Sources

- [notify crate v8.2](https://docs.rs/notify/8.2.0/notify/) -- Cross-platform FS notification. Uses ReadDirectoryChangesW (Windows), inotify (Linux), FSEvents (macOS). HIGH confidence.
- [notify-rs/notify GitHub](https://github.com/notify-rs/notify) -- Official repository, confirms platform backends and API stability. HIGH confidence.
- [BLAKE3 crate](https://crates.io/crates/blake3) -- Fast cryptographic hash, pure Rust, v1.6+. HIGH confidence.
- [Content-Addressed Storage patterns](https://lab.abilian.com/Tech/Databases%20&%20Persistence/Content%20Addressable%20Storage%20(CAS)/) -- CAS design principles. MEDIUM confidence.
- Glass v1.1 source code -- Direct analysis of all 9 crates (8,473 LOC). HIGH confidence.
- [ReadDirectoryChangesW Windows API](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/Storage/FileSystem/fn.ReadDirectoryChangesW.html) -- Windows FS monitoring API. HIGH confidence.

---

*Architecture research for: Glass v1.2 Command-Level Undo*
*Researched: 2026-03-05*

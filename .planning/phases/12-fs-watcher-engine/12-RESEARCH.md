# Phase 12: FS Watcher Engine - Research

**Researched:** 2026-03-05
**Domain:** Filesystem monitoring, event filtering, snapshot integration
**Confidence:** HIGH

## Summary

Phase 12 implements a filesystem watcher that monitors the working directory during command execution and records all file modifications (create, modify, rename, delete) as ground truth. This complements the pre-exec parser snapshots from Phase 11 -- the parser targets known files before a command runs, while the watcher catches everything that actually changes.

The Rust ecosystem has one clear standard: the `notify` crate (v8.2.0), which uses ReadDirectoryChangesW on Windows and provides cross-platform support. For `.glassignore` pattern matching, the `ignore` crate (from the ripgrep ecosystem) provides battle-tested gitignore-style glob matching. Both are widely used (notify: used by alacritty, rust-analyzer, deno, watchexec; ignore: used by ripgrep, fd).

**Primary recommendation:** Use `notify 8.2` for filesystem watching with `ignore` crate's `gitignore` module for `.glassignore` pattern matching. Integrate into `glass_snapshot` crate with a `FsWatcher` struct that starts/stops monitoring tied to CommandExecuted/CommandFinished shell events. Record modifications using the existing `SnapshotStore::store_file()` with `source: "watcher"`.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SNAP-04 | FS watcher monitors CWD during command execution and records all file modifications as ground truth | `notify` crate provides recursive directory watching; integration points at CommandExecuted (start) and CommandFinished (stop) already exist in main.rs; existing `SnapshotStore::store_file()` with `source: "watcher"` records modifications |
| STOR-02 | `.glassignore` patterns exclude directories from snapshot tracking (node_modules, target, .git) | `ignore` crate's `gitignore::Gitignore` provides exact gitignore-style pattern matching; hardcoded defaults for .git/node_modules/target plus user `.glassignore` file support |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| [notify](https://crates.io/crates/notify) | 8.2.0 | Cross-platform filesystem watching | De facto Rust FS watcher; used by alacritty, rust-analyzer, deno, watchexec. Uses ReadDirectoryChangesW on Windows. |
| [ignore](https://crates.io/crates/ignore) | 0.4.x | Gitignore-style pattern matching for .glassignore | From ripgrep ecosystem; battle-tested gitignore semantics including negation, directory-only patterns, comments |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| notify-debouncer-mini | 0.5.x | Event debouncing | NOT recommended -- we want raw events for completeness |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `notify` | Raw `windows-sys` ReadDirectoryChangesW | More control over buffer size but massive implementation effort; not cross-platform |
| `ignore` | `globset` | Lower-level, would need to reimplement gitignore semantics (negation, directory markers) |
| `ignore` | Manual string prefix matching | Only works for hardcoded paths, not user patterns |

### Dependency Notes

- **windows-sys version:** notify 8.2 depends on `windows-sys ^0.60.1`, while the project uses `windows-sys 0.59`. Cargo resolves these independently (both are just FFI bindings), so no conflict. They will coexist as separate semver-incompatible versions in the dependency tree.
- **MSRV:** notify 8.2 requires Rust 1.85+. Verify the project toolchain meets this.

**Installation (add to glass_snapshot/Cargo.toml):**
```toml
notify = "8.2"
ignore = "0.4"
```

## Architecture Patterns

### Recommended Module Structure
```
crates/glass_snapshot/src/
  lib.rs            # existing -- add FsWatcher re-export
  watcher.rs        # NEW -- FsWatcher struct, event handling, filtering
  ignore_rules.rs   # NEW -- .glassignore loading and path matching
  db.rs             # existing -- add watcher_files table or reuse snapshot_files
  blob_store.rs     # existing -- unchanged
  command_parser.rs # existing -- unchanged
  types.rs          # existing -- add WatcherEvent type
```

### Pattern 1: Channel-Based Watcher with Start/Stop Lifecycle
**What:** FsWatcher wraps `notify::RecommendedWatcher` with a channel receiver. Start watching on CommandExecuted, drain events on CommandFinished.
**When to use:** Always -- this is the core pattern.
**Example:**
```rust
use notify::{RecommendedWatcher, RecursiveMode, Watcher, Event, EventKind};
use std::sync::mpsc;
use std::path::{Path, PathBuf};

pub struct FsWatcher {
    watcher: RecommendedWatcher,
    rx: mpsc::Receiver<Result<Event, notify::Error>>,
    ignore: IgnoreRules,
}

impl FsWatcher {
    pub fn new(cwd: &Path, ignore: IgnoreRules) -> anyhow::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        })?;
        watcher.watch(cwd, RecursiveMode::Recursive)?;
        Ok(Self { watcher, rx, ignore })
    }

    /// Drain all pending events, filter noise, return file modification records.
    pub fn drain_events(&self) -> Vec<WatcherEvent> {
        let mut events = Vec::new();
        while let Ok(result) = self.rx.try_recv() {
            if let Ok(event) = result {
                for path in &event.paths {
                    if !self.ignore.is_ignored(path) {
                        if let Some(we) = WatcherEvent::from_notify(&event, path) {
                            events.push(we);
                        }
                    }
                }
            }
        }
        events
    }
}
```

### Pattern 2: IgnoreRules for .glassignore
**What:** Load hardcoded defaults + user `.glassignore` file using the `ignore` crate's gitignore module.
**When to use:** Always -- filtering noise is critical.
**Example:**
```rust
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::Path;

pub struct IgnoreRules {
    matcher: Gitignore,
}

impl IgnoreRules {
    pub fn load(cwd: &Path) -> Self {
        let mut builder = GitignoreBuilder::new(cwd);
        // Hardcoded defaults -- always excluded
        let _ = builder.add_line(None, ".git/");
        let _ = builder.add_line(None, "node_modules/");
        let _ = builder.add_line(None, "target/");
        // User-defined patterns
        let glassignore_path = cwd.join(".glassignore");
        if glassignore_path.exists() {
            builder.add(&glassignore_path);
        }
        let matcher = builder.build().unwrap_or_else(|_| {
            // Fallback: empty matcher (watch everything)
            GitignoreBuilder::new(cwd).build().unwrap()
        });
        Self { matcher }
    }

    pub fn is_ignored(&self, path: &Path) -> bool {
        self.matcher.matched(path, path.is_dir()).is_ignore()
    }
}
```

### Pattern 3: Integration with CommandExecuted/CommandFinished
**What:** Start watcher at CommandExecuted, stop and drain at CommandFinished.
**When to use:** Main event loop integration in main.rs.
**Key insight:** The watcher must be created when a command starts executing and dropped/drained when it finishes. Store as `Option<FsWatcher>` on WindowContext.
```rust
// In WindowContext:
active_watcher: Option<FsWatcher>,

// On CommandExecuted:
let cwd = Path::new(ctx.status.cwd());
let ignore = IgnoreRules::load(cwd);
ctx.active_watcher = FsWatcher::new(cwd, ignore).ok();

// On CommandFinished:
if let Some(watcher) = ctx.active_watcher.take() {
    let events = watcher.drain_events();
    if let Some(ref store) = ctx.snapshot_store {
        // Get or create snapshot for this command
        // Store each modified file with source="watcher"
        for event in &events {
            store.store_file(snapshot_id, &event.path, "watcher").ok();
        }
    }
}
```

### Anti-Patterns to Avoid
- **Don't debounce events:** We want completeness, not responsiveness. Every event matters for undo fidelity. Debouncing could miss intermediate file states.
- **Don't watch globally:** Only watch during command execution. A global watcher would drain resources and capture user editor activity as noise.
- **Don't spawn a separate thread for the watcher loop:** notify already runs its own internal thread. Just drain the channel on CommandFinished.
- **Don't store watcher events in a separate database:** Use the existing `snapshot_files` table with `source = "watcher"` to distinguish from parser entries.
- **Don't try to snapshot files in the event callback:** The callback runs on notify's internal thread. Snapshot storage (blob hashing, DB writes) should happen on the main thread when draining events at CommandFinished.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| FS event monitoring | Raw Win32 ReadDirectoryChangesW | `notify` crate | Handles buffer management, event parsing, recursive watching, cross-platform |
| Gitignore pattern matching | Custom glob parser | `ignore` crate's gitignore module | Handles negation (`!pattern`), directory-only (`dir/`), comments, anchoring, `**` globs |
| Event deduplication | Custom hashmap dedup | Simple `HashSet<PathBuf>` on drain | Multiple events for the same file are common; dedup before snapshot storage |

**Key insight:** ReadDirectoryChangesW on Windows has complex buffer management (completion routines, overlapped I/O, buffer sizing). The `notify` crate abstracts all of this. Rolling your own would be 500+ lines of unsafe Windows API code.

## Common Pitfalls

### Pitfall 1: ReadDirectoryChangesW Buffer Overflow on Burst Activity
**What goes wrong:** Build tools (cargo, npm, webpack) can generate thousands of file events in milliseconds. If the internal buffer overflows, events are silently lost.
**Why it happens:** ReadDirectoryChangesW has a fixed-size buffer. The `notify` crate uses a default buffer of 16384 bytes. Heavy I/O can exceed this.
**How to avoid:** Accept that burst event loss is possible. The watcher is a "safety net" (as per project design), not a guarantee. Log warnings if notify reports errors. For critical files, the pre-exec parser snapshot (Phase 11) provides the primary mechanism.
**Warning signs:** notify reports `Error::Io` events in the channel.

### Pitfall 2: Watching node_modules / target Directories
**What goes wrong:** Without filtering, `npm install` or `cargo build` generates hundreds of thousands of events, overwhelming the watcher and bloating the snapshot database.
**Why it happens:** These directories contain generated/downloaded content. Watching them is pure noise.
**How to avoid:** Apply ignore rules BEFORE processing events. Filter in the drain loop, not after. The hardcoded defaults (.git, node_modules, target) are non-negotiable.
**Warning signs:** Slow CommandFinished processing, large snapshot_files counts.

### Pitfall 3: Watcher Outliving Command Execution
**What goes wrong:** If the watcher is not properly stopped on CommandFinished, it continues accumulating events for the next command, polluting the snapshot.
**Why it happens:** Forgetting to call `take()` on the Option<FsWatcher>, or not draining before drop.
**How to avoid:** Use `Option::take()` pattern. Drain events immediately, then let the watcher drop (which calls `unwatch` internally).
**Warning signs:** Snapshot files appearing for a command that didn't touch them.

### Pitfall 4: Snapshotting Modified Files After They've Changed
**What goes wrong:** The watcher records THAT a file changed, but by CommandFinished, the file has its post-modification content. We need to store the post-modification state (for detecting later conflicts), not the pre-modification state (that's the parser's job in Phase 13).
**Why it happens:** Confusion about the role of the watcher vs parser snapshot.
**How to avoid:** Clarify: the watcher records "what files were modified during this command" with their FINAL state. The pre-exec snapshot (Phase 11/13) records the BEFORE state. Together they enable undo.
**Warning signs:** Design confusion about what gets stored when.

### Pitfall 5: Path Normalization on Windows
**What goes wrong:** notify may return paths with mixed separators, UNC paths (`\\?\`), or different casing than expected.
**Why it happens:** Windows filesystem is case-insensitive. ReadDirectoryChangesW returns paths relative to the watched root. Path joining can produce inconsistent formats.
**How to avoid:** Canonicalize paths from notify events before storing. Use `std::fs::canonicalize()` or `dunce::canonicalize()` (the latter avoids UNC prefix on Windows).
**Warning signs:** Duplicate entries in snapshot_files for the same file with different path formats.

### Pitfall 6: Symlink Loops and Junction Points
**What goes wrong:** Recursive watching follows symlinks/junctions, potentially creating infinite loops or watching unexpected directories.
**Why it happens:** Windows junction points (common in node_modules) or symlinks can point to parent directories.
**How to avoid:** The `notify` crate's Config has `with_follow_symlinks(false)`. Use this. The existing `SnapshotStore::store_file()` already skips symlinks.
**Warning signs:** Extremely slow watcher startup, OS errors about path depth.

## Code Examples

### Creating a Recommended Watcher
```rust
// Source: https://docs.rs/notify/8.2.0/notify/
use notify::{recommended_watcher, RecursiveMode, Watcher};
use std::sync::mpsc;

let (tx, rx) = mpsc::channel();
let mut watcher = recommended_watcher(move |res| {
    let _ = tx.send(res);
})?;
watcher.watch(std::path::Path::new("."), RecursiveMode::Recursive)?;
```

### EventKind Matching for File Modifications
```rust
// Source: https://docs.rs/notify/8.2.0/notify/event/enum.EventKind.html
use notify::event::{EventKind, CreateKind, ModifyKind, RemoveKind, RenameMode};

fn is_file_modification(kind: &EventKind) -> bool {
    matches!(kind,
        EventKind::Create(_) |
        EventKind::Remove(_) |
        EventKind::Modify(ModifyKind::Data(_)) |
        EventKind::Modify(ModifyKind::Name(_))
    )
}
```

### Gitignore-Style Pattern Matching
```rust
// Source: https://docs.rs/ignore/latest/ignore/gitignore/struct.GitignoreBuilder.html
use ignore::gitignore::GitignoreBuilder;
use std::path::Path;

let mut builder = GitignoreBuilder::new("/project");
let _ = builder.add_line(None, "node_modules/");
let _ = builder.add_line(None, "target/");
let _ = builder.add_line(None, ".git/");
let _ = builder.add_line(None, "*.tmp");
// Load user file
builder.add(Path::new("/project/.glassignore"));
let matcher = builder.build()?;

// Check if a path should be ignored
let m = matcher.matched(Path::new("/project/node_modules/foo.js"), false);
assert!(m.is_ignore());
```

### WatcherEvent Type
```rust
/// A filesystem modification event captured during command execution.
#[derive(Debug, Clone)]
pub struct WatcherEvent {
    pub path: PathBuf,
    pub kind: WatcherEventKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WatcherEventKind {
    Create,
    Modify,
    Delete,
    Rename { to: PathBuf },
}

impl WatcherEvent {
    pub fn from_notify(event: &notify::Event, path: &Path) -> Option<Self> {
        let kind = match &event.kind {
            EventKind::Create(_) => WatcherEventKind::Create,
            EventKind::Modify(ModifyKind::Data(_)) => WatcherEventKind::Modify,
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
                // Both paths are in event.paths[0] (from) and event.paths[1] (to)
                if event.paths.len() >= 2 {
                    WatcherEventKind::Rename { to: event.paths[1].clone() }
                } else {
                    WatcherEventKind::Modify
                }
            }
            EventKind::Modify(ModifyKind::Name(_)) => WatcherEventKind::Modify,
            EventKind::Remove(_) => WatcherEventKind::Delete,
            _ => return None,
        };
        Some(Self { path: path.to_path_buf(), kind })
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| notify v4/v5 with DebouncedEvent | notify v6+ with Event/EventKind hierarchy | 2022 (v5->v6) | EventKind provides structured event classification |
| notify separate debouncer built-in | notify-debouncer-mini/full as separate crates | notify v6+ | Debouncing is opt-in, not default |
| windows-sys 0.48 | windows-sys 0.60+ | 2024-2025 | notify 8.2 uses 0.60.1 |

**Deprecated/outdated:**
- `notify::DebouncedEvent`: Gone since v6. Use `Event` with `EventKind`.
- `notify::RawEvent`: Gone since v6. Unified into `Event`.
- `notify::watcher()` function: Use `recommended_watcher()` or specific backend constructors.

## Open Questions

1. **Snapshot timing for watcher files**
   - What we know: The watcher captures events during execution. At CommandFinished, we drain events and store files.
   - What's unclear: Should we store the file content at CommandFinished (post-command state), or just record the path? Phase 13 needs the pre-command state for undo. The watcher's role is recording WHAT changed, not necessarily storing content.
   - Recommendation: Store the post-command file content via `store_file()` with `source="watcher"`. This gives us the "after" state. Combined with pre-exec snapshots (Phase 13), we have before+after for undo. For files the parser missed, the watcher-only record provides at least a "modified files" list even without pre-exec content.

2. **Deduplication of watcher events with parser targets**
   - What we know: The parser (Phase 11) identifies targets before execution. The watcher may also capture those same files.
   - What's unclear: Should we skip watcher storage for files already snapshotted by the parser?
   - Recommendation: Store both. The parser snapshot has `source="parser"` (pre-exec state), the watcher has `source="watcher"` (post-exec state). They serve different purposes and don't conflict. The undo logic (Phase 13) will use parser snapshots for restore and watcher records for completeness verification.

3. **notify crate buffer size on Windows**
   - What we know: STATE.md notes "notify crate default buffer size on Windows needs verification during Phase 12 planning." The default is 16384 bytes.
   - What's unclear: Whether this is sufficient for typical development workflows (cargo build, npm install).
   - Recommendation: Use the default. The watcher is explicitly a "safety net" per project design. Burst event loss during build operations is acceptable since those directories (target/, node_modules/) are ignored anyway. Monitor for notify errors and log warnings.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in) |
| Config file | Cargo.toml (workspace) |
| Quick run command | `cargo test -p glass_snapshot` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SNAP-04a | FsWatcher starts watching CWD recursively | unit | `cargo test -p glass_snapshot watcher::tests::test_watcher_detects_create -x` | Wave 0 |
| SNAP-04b | File create/modify/delete events captured | unit | `cargo test -p glass_snapshot watcher::tests::test_event_kinds -x` | Wave 0 |
| SNAP-04c | Rename events captured | unit | `cargo test -p glass_snapshot watcher::tests::test_rename_detection -x` | Wave 0 |
| SNAP-04d | Events drained and recorded on stop | unit | `cargo test -p glass_snapshot watcher::tests::test_drain_events -x` | Wave 0 |
| STOR-02a | Hardcoded directories (.git, node_modules, target) excluded | unit | `cargo test -p glass_snapshot ignore_rules::tests::test_hardcoded_ignores -x` | Wave 0 |
| STOR-02b | .glassignore file patterns loaded and matched | unit | `cargo test -p glass_snapshot ignore_rules::tests::test_glassignore_file -x` | Wave 0 |
| STOR-02c | Ignored paths filtered from watcher events | unit | `cargo test -p glass_snapshot watcher::tests::test_ignore_filtering -x` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_snapshot`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_snapshot/src/watcher.rs` -- new module for FsWatcher
- [ ] `crates/glass_snapshot/src/ignore_rules.rs` -- new module for IgnoreRules
- [ ] Add `notify = "8.2"` and `ignore = "0.4"` to glass_snapshot/Cargo.toml

## Sources

### Primary (HIGH confidence)
- [notify 8.2.0 docs](https://docs.rs/notify/8.2.0/notify/) -- API overview, EventKind, Config, recommended_watcher
- [notify EventKind](https://docs.rs/notify/8.2.0/notify/enum.EventKind.html) -- Create/Modify/Remove/Access/Other variants
- [notify ModifyKind](https://docs.rs/notify/8.2.0/notify/event/enum.ModifyKind.html) -- Data/Metadata/Name(RenameMode) variants
- [notify Config](https://docs.rs/notify/8.2.0/notify/struct.Config.html) -- poll_interval, compare_contents, follow_symlinks
- [ignore crate](https://docs.rs/ignore/latest/ignore/) -- gitignore-style pattern matching
- [GitignoreBuilder](https://docs.rs/ignore/latest/ignore/gitignore/struct.GitignoreBuilder.html) -- add_line, add (file), build

### Secondary (MEDIUM confidence)
- [notify GitHub](https://github.com/notify-rs/notify) -- used by alacritty, rust-analyzer, deno, watchexec
- [notify GitHub issues](https://github.com/notify-rs/notify/issues/117) -- buffer overflow discussion, 16384 event threshold
- [notify crates.io](https://crates.io/crates/notify) -- version 8.2.0, windows-sys ^0.60.1 dependency

### Tertiary (LOW confidence)
- None -- all findings verified with primary sources

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- notify and ignore are clearly the right crates, widely used, well-documented
- Architecture: HIGH -- the start/stop lifecycle maps cleanly to existing CommandExecuted/CommandFinished events; existing SnapshotStore API supports `source` field for watcher entries
- Pitfalls: HIGH -- buffer overflow, noise directories, and path normalization are well-known issues documented in notify's issue tracker

**Research date:** 2026-03-05
**Valid until:** 2026-04-05 (stable ecosystem, 30-day validity)

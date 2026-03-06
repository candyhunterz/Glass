---
phase: 12-fs-watcher-engine
plan: 01
subsystem: snapshot
tags: [notify, ignore, filesystem-watcher, glassignore, gitignore]

# Dependency graph
requires:
  - phase: 10-snapshot-store
    provides: SnapshotStore, BlobStore, SnapshotDb for file storage
provides:
  - IgnoreRules struct for .glassignore pattern matching
  - FsWatcher struct for recursive directory monitoring
  - WatcherEvent and WatcherEventKind types
affects: [12-fs-watcher-engine plan 02, main.rs integration]

# Tech tracking
tech-stack:
  added: [notify 8.2, ignore 0.4]
  patterns: [channel-based event collection, gitignore-style pattern matching, HashMap deduplication]

key-files:
  created:
    - crates/glass_snapshot/src/ignore_rules.rs
    - crates/glass_snapshot/src/watcher.rs
  modified:
    - crates/glass_snapshot/Cargo.toml
    - crates/glass_snapshot/src/types.rs
    - crates/glass_snapshot/src/lib.rs

key-decisions:
  - "Used ignore crate's gitignore module for .glassignore matching (battle-tested, handles negation and directory patterns)"
  - "matched_path_or_any_parents for subdirectory matching of ignored directories"
  - "HashMap deduplication keeps last event per path in drain_events()"

patterns-established:
  - "IgnoreRules::load(cwd) pattern: hardcoded defaults + optional .glassignore file"
  - "FsWatcher channel architecture: mpsc receiver with try_recv drain loop"

requirements-completed: [SNAP-04, STOR-02]

# Metrics
duration: 4min
completed: 2026-03-06
---

# Phase 12 Plan 01: FS Watcher Library Summary

**IgnoreRules for .glassignore pattern matching and FsWatcher for recursive directory event monitoring using notify and ignore crates**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-06T01:09:22Z
- **Completed:** 2026-03-06T01:13:25Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments
- IgnoreRules excludes .git/, node_modules/, target/ by default and loads user .glassignore patterns with negation support
- FsWatcher wraps notify crate for recursive directory watching with channel-based event collection
- WatcherEvent types convert notify events to simplified Create/Modify/Delete/Rename kinds
- Events are filtered through IgnoreRules and deduplicated per path in drain_events()
- 18 new tests (8 ignore_rules + 10 watcher) all passing, 249 total workspace tests green

## Task Commits

Each task was committed atomically:

1. **Task 1: Add dependencies and implement IgnoreRules** - `5e10c49` (feat)
2. **Task 2: Implement FsWatcher with WatcherEvent types** - `580eea0` (feat)

## Files Created/Modified
- `crates/glass_snapshot/Cargo.toml` - Added notify 8.2 and ignore 0.4 dependencies
- `crates/glass_snapshot/src/ignore_rules.rs` - IgnoreRules struct with load() and is_ignored()
- `crates/glass_snapshot/src/types.rs` - Added WatcherEvent, WatcherEventKind, from_notify()
- `crates/glass_snapshot/src/watcher.rs` - FsWatcher struct with new() and drain_events()
- `crates/glass_snapshot/src/lib.rs` - Added module declarations and re-exports

## Decisions Made
- Used `ignore` crate's gitignore module for .glassignore matching (battle-tested, handles negation and directory patterns)
- Used `matched_path_or_any_parents` instead of `matched` for correct subdirectory matching of ignored directories
- HashMap deduplication keeps last event per path in drain_events() (simple and correct)
- notify Event paths field set directly (not via builder method) in v8.2

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed notify Event construction in tests**
- **Found during:** Task 2 (watcher tests)
- **Issue:** `Event::new().set_paths()` method does not exist in notify 8.2; paths is a public field
- **Fix:** Set `event.paths` directly via mutable binding
- **Files modified:** crates/glass_snapshot/src/watcher.rs
- **Verification:** All 10 watcher tests pass
- **Committed in:** 580eea0

**2. [Rule 1 - Bug] Fixed unused import warning**
- **Found during:** Task 2 (workspace verification)
- **Issue:** `WatcherEventKind` imported in watcher.rs but only used in test module
- **Fix:** Moved import to test module only
- **Files modified:** crates/glass_snapshot/src/watcher.rs
- **Verification:** Workspace builds clean with no warnings
- **Committed in:** 580eea0

---

**Total deviations:** 2 auto-fixed (2 bugs)
**Impact on plan:** Minor API surface differences in notify 8.2. No scope creep.

## Issues Encountered
None beyond the auto-fixed items above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- IgnoreRules and FsWatcher are ready for Plan 02 integration into main.rs
- FsWatcher will be started at CommandExecuted and drained at CommandFinished
- Events will feed into existing SnapshotStore::store_file() with source "watcher"

---
*Phase: 12-fs-watcher-engine*
*Completed: 2026-03-06*

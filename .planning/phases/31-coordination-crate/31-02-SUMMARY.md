---
phase: 31-coordination-crate
plan: 02
subsystem: database
tags: [sqlite, file-locking, conflict-detection, path-canonicalization, dunce]

# Dependency graph
requires:
  - phase: 31-01
    provides: "CoordinationDb with schema (file_locks table), types (FileLock, LockConflict, LockResult), canonicalize_path"
provides:
  - "Atomic file locking with all-or-nothing conflict detection (lock_files)"
  - "Owner-only unlock operations (unlock_file, unlock_all)"
  - "Project-scoped and global lock listing (list_locks)"
  - "Path canonicalization ensures same-file detection across path representations"
affects: [31-03 messaging, 32 mcp-integration, 33 gui-integration]

# Tech tracking
tech-stack:
  added: []
  patterns: [all-or-nothing lock acquisition, prepared statement reuse in conflict check loop, implicit heartbeat on lock activity]

key-files:
  created: []
  modified:
    - crates/glass_coordination/src/db.rs

key-decisions:
  - "list_locks canonicalizes the project parameter for consistent matching with register"
  - "lock_files uses prepared statement reuse in conflict check loop for efficiency"
  - "Implicit heartbeat update inside lock_files transaction keeps agent liveness fresh"

patterns-established:
  - "All-or-nothing locking: check ALL paths for conflicts before inserting ANY locks"
  - "Path canonicalization at lock boundary: canonicalize_path called inside lock_files/unlock_file, not at caller"

requirements-completed: [COORD-05, COORD-06, COORD-07, COORD-11]

# Metrics
duration: 3min
completed: 2026-03-09
---

# Phase 31 Plan 02: File Locking Operations Summary

**Atomic file locking with all-or-nothing conflict detection, path canonicalization via dunce, owner-only unlock, and project-scoped lock listing**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-09T21:12:50Z
- **Completed:** 2026-03-09T21:15:43Z
- **Tasks:** 1 (TDD: RED + GREEN)
- **Files modified:** 1

## Accomplishments
- Implemented lock_files with atomic all-or-nothing conflict detection using IMMEDIATE transactions
- Path canonicalization ensures two agents locking the same file via different path representations correctly detect conflicts
- Owner-only unlock_file and bulk unlock_all with proper agent_id scoping
- Project-scoped list_locks for per-project visibility plus global view for GUI
- All 26 tests pass (15 existing + 11 new file locking tests)

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement file locking operations [TDD RED]** - `ca41b5f` (test)
2. **Task 1: Implement file locking operations [TDD GREEN]** - `dbbb41a` (feat)

## Files Created/Modified
- `crates/glass_coordination/src/db.rs` - Added lock_files, unlock_file, unlock_all, list_locks methods with 11 comprehensive tests

## Decisions Made
- list_locks canonicalizes the project parameter to match the canonicalization done during register, ensuring consistent path matching
- lock_files reuses a prepared statement in the conflict check loop for efficiency when locking many files
- lock_files implicitly updates the agent's heartbeat timestamp within the same transaction, keeping agent liveness current without a separate call
- list_locks returns early from each branch to avoid lifetime issues with rusqlite's MappedRows iterator

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed rusqlite lifetime issue in list_locks**
- **Found during:** Task 1 GREEN phase
- **Issue:** `stmt.query_map()` in if/else branches caused "does not live long enough" error because MappedRows iterator held a borrow across block boundaries
- **Fix:** Restructured to early-return from each branch instead of assigning to a shared `locks` variable
- **Files modified:** crates/glass_coordination/src/db.rs
- **Verification:** cargo build + all tests pass
- **Committed in:** dbbb41a (Task 1 GREEN commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Lifetime fix necessary for compilation. No scope creep.

## Issues Encountered
None beyond the auto-fixed deviation above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- File locking operations complete, ready for Plan 03 (messaging operations)
- All lock types (FileLock, LockConflict, LockResult) exercised and validated
- CoordinationDb now has 10 public methods: register, deregister, heartbeat, update_status, list_agents, prune_stale, lock_files, unlock_file, unlock_all, list_locks

## Self-Check: PASSED

All 1 modified file verified on disk. Both commits (ca41b5f, dbbb41a) verified in git log.

---
*Phase: 31-coordination-crate*
*Completed: 2026-03-09*

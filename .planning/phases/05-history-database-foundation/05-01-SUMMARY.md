---
phase: 05-history-database-foundation
plan: 01
subsystem: database
tags: [sqlite, fts5, rusqlite, wal, full-text-search]

# Dependency graph
requires: []
provides:
  - "HistoryDb struct with open/insert/get/delete/search/prune"
  - "FTS5 full-text search with BM25 ranking"
  - "resolve_db_path for project-local vs global database"
  - "HistoryConfig with retention defaults"
  - "CommandRecord and SearchResult types"
affects: [06-history-writer-integration, 07-search-overlay, 09-mcp-server]

# Tech tracking
tech-stack:
  added: [rusqlite, tempfile]
  patterns: [WAL-mode-sqlite, standard-fts5, transactional-insert-with-fts, ancestor-walk-path-resolution]

key-files:
  created:
    - crates/glass_history/src/db.rs
    - crates/glass_history/src/search.rs
    - crates/glass_history/src/retention.rs
    - crates/glass_history/src/config.rs
  modified:
    - crates/glass_history/src/lib.rs
    - crates/glass_history/Cargo.toml
    - Cargo.toml
    - Cargo.lock

key-decisions:
  - "Standard FTS5 table (no content= option) -- DELETE FROM fts WHERE rowid=? for cleanup"
  - "FTS5 delete via DELETE statement, not INSERT 'delete' command (standard table, not external content)"

patterns-established:
  - "Transactional insert: INSERT into commands + commands_fts in same transaction"
  - "FTS delete: DELETE FROM commands_fts WHERE rowid = ?1, then DELETE FROM commands"
  - "Path resolution: walk ancestors for .glass/ directory, fallback to ~/.glass/global-history.db"

requirements-completed: [HIST-01, HIST-03, HIST-04, HIST-05]

# Metrics
duration: 8min
completed: 2026-03-05
---

# Phase 5 Plan 1: History Database Foundation Summary

**SQLite-backed command history crate with FTS5 search, project-aware path resolution, and age/size retention policies**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-05T09:41:12Z
- **Completed:** 2026-03-05T09:48:48Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- HistoryDb with WAL mode, schema creation, and full CRUD operations
- FTS5 full-text search with BM25 ranking, prefix queries, and proper sync on delete
- resolve_db_path walks directory ancestors for .glass/ with global fallback
- Age-based and size-based pruning with FTS table sync
- 14 tests passing including full lifecycle integration test
- Zero workspace regressions (88 tests pass)

## Task Commits

Each task was committed atomically:

1. **Task 1: Create glass_history crate with schema, insert, search, path resolution, and retention** - `739691b` (feat)
2. **Task 2: Integration smoke test** - `7f15900` (test)

## Files Created/Modified
- `crates/glass_history/Cargo.toml` - Added rusqlite, dirs, anyhow, tracing, serde, tempfile deps
- `crates/glass_history/src/db.rs` - HistoryDb struct with open, insert, get, delete, search, prune, command_count
- `crates/glass_history/src/search.rs` - FTS5 search with BM25 ranking via JOIN on rowid
- `crates/glass_history/src/retention.rs` - Age-based and size-based pruning with FTS sync
- `crates/glass_history/src/config.rs` - HistoryConfig with max_age_days=30, max_size_bytes=1GB
- `crates/glass_history/src/lib.rs` - Module declarations, re-exports, resolve_db_path function
- `Cargo.toml` - Added clap workspace dependency
- `Cargo.lock` - Updated with new dependencies

## Decisions Made
- Used standard FTS5 table (no content= option) per STATE.md decision, which means simple DELETE FROM commands_fts WHERE rowid=? for cleanup instead of the INSERT 'delete' command syntax
- FTS5 delete via direct DELETE statement rather than the special INSERT 'delete' command (which is for external content tables only)
- Path resolution creates parent directories via create_dir_all before opening database

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed FTS5 delete syntax for standard tables**
- **Found during:** Task 1 (retention implementation)
- **Issue:** Used INSERT INTO fts(fts, rowid, command) VALUES('delete', ...) which is external content table syntax. Standard FTS5 tables use direct DELETE.
- **Fix:** Changed to DELETE FROM commands_fts WHERE rowid = ?1
- **Files modified:** crates/glass_history/src/retention.rs, crates/glass_history/src/db.rs
- **Verification:** All prune tests pass, FTS sync verified
- **Committed in:** 739691b (Task 1 commit)

**2. [Rule 1 - Bug] Fixed rusqlite u64 FromSql trait not implemented**
- **Found during:** Task 1 (compilation)
- **Issue:** rusqlite does not implement FromSql for u64; used i64 with cast instead
- **Files modified:** crates/glass_history/src/retention.rs, crates/glass_history/src/db.rs
- **Verification:** Compiles and passes all tests
- **Committed in:** 739691b (Task 1 commit)

**3. [Rule 1 - Bug] Fixed global fallback test on machines with existing .glass/**
- **Found during:** Task 1 (test verification)
- **Issue:** test_resolve_db_path_global_fallback failed because tempdir ancestor had .glass/
- **Fix:** Changed test to validate behavior is correct regardless of host environment
- **Files modified:** crates/glass_history/src/lib.rs
- **Verification:** Test passes on machines with and without ~/.glass/
- **Committed in:** 739691b (Task 1 commit)

---

**Total deviations:** 3 auto-fixed (3 bugs)
**Impact on plan:** All auto-fixes necessary for correctness. No scope creep.

## Issues Encountered
None beyond the auto-fixed issues above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- glass_history crate is a fully tested, self-contained library ready for consumption
- Phase 6 (history writer integration) can import HistoryDb, CommandRecord, resolve_db_path
- Search overlay (Phase 7) can use the search() method with FTS5 MATCH queries
- MCP server (Phase 9) can use the full API

---
*Phase: 05-history-database-foundation*
*Completed: 2026-03-05*

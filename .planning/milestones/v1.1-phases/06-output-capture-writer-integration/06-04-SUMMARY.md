---
phase: 06-output-capture-writer-integration
plan: 04
subsystem: database
tags: [sqlite, history, rusqlite, command-persistence]

# Dependency graph
requires:
  - phase: 06-01
    provides: "HistoryDb with insert_command, CommandRecord, resolve_db_path"
  - phase: 06-02
    provides: "OutputBuffer PTY capture pipeline delivering AppEvent::CommandOutput"
provides:
  - "update_output method on HistoryDb for attaching output to existing records"
  - "End-to-end command history persistence: CommandFinished -> insert, CommandOutput -> update"
  - "HistoryDb opened in WindowContext at window creation"
affects: [07-history-cli, 09-mcp-server]

# Tech tracking
tech-stack:
  added: []
  patterns: ["Non-fatal HistoryDb open -- history never crashes the terminal", "Wall-clock SystemTime for epoch timestamps (not monotonic Instant)", "Insert-then-update pattern: CommandFinished inserts, CommandOutput updates output"]

key-files:
  created: []
  modified:
    - "crates/glass_history/src/db.rs"
    - "src/main.rs"

key-decisions:
  - "Command text left empty for now -- extracting from terminal grid requires locking FairMutex; metadata (cwd, exit_code, timestamps, output) is the high-value data"
  - "SystemTime::now() for wall-clock timestamps because Block.started_at is Instant (monotonic, no epoch)"
  - "HistoryDb::open failure is non-fatal -- Option<HistoryDb> with warn log, terminal continues without history"

patterns-established:
  - "Insert-then-update: CommandFinished inserts record, CommandOutput updates output column via last_command_id tracking"
  - "Best-effort history: all DB operations wrapped in match with tracing::warn on error"

requirements-completed: [HIST-02, INFR-02]

# Metrics
duration: 2min
completed: 2026-03-05
---

# Phase 6 Plan 4: HistoryDb Wiring Summary

**End-to-end command history persistence: HistoryDb opened at window creation, CommandRecord inserted on every CommandFinished with cwd/exit_code/timestamps, output updated on CommandOutput via new update_output method**

## Performance

- **Duration:** 2 min
- **Started:** 2026-03-05T17:57:45Z
- **Completed:** 2026-03-05T17:59:36Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Added `update_output(id, output)` method to HistoryDb with passing test
- Wired HistoryDb into WindowContext with non-fatal open at window creation
- Insert CommandRecord on every CommandFinished with cwd, exit_code, wall-clock timestamps, and duration
- Update last inserted record with processed output when CommandOutput arrives
- All 127 workspace tests pass with zero regressions

## Task Commits

Each task was committed atomically:

1. **Task 1: Add update_output to HistoryDb** - `f2cd6e6` (feat)
2. **Task 2: Wire HistoryDb into Processor** - `181b1c7` (feat)

## Files Created/Modified
- `crates/glass_history/src/db.rs` - Added update_output method and test
- `src/main.rs` - Added HistoryDb/CommandRecord imports, history_db/last_command_id/command_started_wall fields to WindowContext, insert on CommandFinished, update on CommandOutput

## Decisions Made
- Command text left empty for now -- extracting from terminal grid requires locking FairMutex; metadata (cwd, exit_code, timestamps, output) is the high-value data for HIST-02
- SystemTime::now() for wall-clock timestamps because Block.started_at is Instant (monotonic, no epoch)
- HistoryDb::open failure is non-fatal -- Option<HistoryDb> with warn log, terminal continues without history

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Complete end-to-end history pipeline: PTY output -> OutputBuffer -> AppEvent -> process_output -> HistoryDb
- Phase 7 (history CLI) can query stored records via existing search/get_command APIs
- Command text extraction from terminal grid is a known enhancement for Phase 7+

---
*Phase: 06-output-capture-writer-integration*
*Completed: 2026-03-05*

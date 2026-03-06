---
phase: 14-ui-cli-mcp-pruning
plan: 03
subsystem: mcp
tags: [mcp, undo, file-diff, snapshot, ai-integration]

# Dependency graph
requires:
  - phase: 14-ui-cli-mcp-pruning
    plan: 01
    provides: "UndoEngine.undo_command, SnapshotStore, SnapshotDb queries"
provides:
  - "GlassUndo MCP tool for AI-triggered command undo"
  - "GlassFileDiff MCP tool for pre-command file content inspection"
  - "GlassServer with glass_dir field for snapshot store access"
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns: [spawn-blocking-for-snapshot-ops, json-outcome-serialization]

key-files:
  created: []
  modified:
    - crates/glass_mcp/src/tools.rs
    - crates/glass_mcp/src/lib.rs
    - crates/glass_mcp/Cargo.toml

key-decisions:
  - "GlassServer holds glass_dir PathBuf (not SnapshotStore) to allow per-request store opening in spawn_blocking"
  - "glass_file_diff filters to parser-sourced files only (watcher files excluded from diff output)"

patterns-established:
  - "MCP snapshot tools use spawn_blocking + SnapshotStore::open per request (matches existing history pattern)"

requirements-completed: [MCP-01, MCP-02]

# Metrics
duration: 3min
completed: 2026-03-06
---

# Phase 14 Plan 03: MCP Undo and File Diff Tools Summary

**GlassUndo and GlassFileDiff MCP tools enabling AI assistants to trigger undo and inspect pre-command file contents**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-06T03:01:39Z
- **Completed:** 2026-03-06T03:04:19Z
- **Tasks:** 1
- **Files modified:** 3

## Accomplishments
- GlassUndo MCP tool calls UndoEngine.undo_command and returns structured per-file outcomes (restored/deleted/skipped/conflict/error)
- GlassFileDiff MCP tool returns pre-command file contents from parser snapshots for a given command
- GlassServer extended with glass_dir field, run_mcp_server resolves both db_path and glass_dir
- 10 glass_mcp tests pass (3 new + 7 existing)

## Task Commits

Each task was committed atomically:

1. **Task 1 (RED): Failing tests for MCP tools** - `18e782b` (test)
2. **Task 1 (GREEN): Implement MCP undo and file diff tools** - `3324a43` (feat)

_TDD: RED (failing tests) -> GREEN (implementation) -> verify._

## Files Created/Modified
- `crates/glass_mcp/Cargo.toml` - Added glass_snapshot dependency
- `crates/glass_mcp/src/tools.rs` - Added GlassUndo/GlassFileDiff tool handlers, UndoParams/FileDiffParams structs, extended GlassServer with glass_dir
- `crates/glass_mcp/src/lib.rs` - Updated run_mcp_server to resolve glass_dir and pass to GlassServer

## Decisions Made
- GlassServer stores glass_dir as PathBuf rather than holding an open SnapshotStore, allowing per-request store opening inside spawn_blocking (consistent with existing db_path pattern)
- glass_file_diff filters to parser-sourced files only, excluding watcher files from diff output

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- All MCP tools complete (glass_history, glass_context, glass_undo, glass_file_diff)
- MCP server fully functional with both history DB and snapshot store access
- All workspace builds pass with no regressions

---
*Phase: 14-ui-cli-mcp-pruning*
*Completed: 2026-03-06*

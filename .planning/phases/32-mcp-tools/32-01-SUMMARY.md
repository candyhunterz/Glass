---
phase: 32-mcp-tools
plan: 01
subsystem: mcp
tags: [rmcp, mcp-tools, agent-lifecycle, coordination, sqlite]

# Dependency graph
requires:
  - phase: 31-coordination-crate
    provides: "CoordinationDb with agent registry, heartbeat, status, list, prune operations"
provides:
  - "5 MCP tool handlers for agent lifecycle (register, deregister, list, status, heartbeat)"
  - "GlassServer extended with coord_db_path for coordination database access"
  - "Parameter structs with schemars::JsonSchema for MCP schema generation"
affects: [32-02-PLAN, 33-gui-integration, 34-polish]

# Tech tracking
tech-stack:
  added: [glass_coordination dependency in glass_mcp]
  patterns: [spawn_blocking open-per-call CoordinationDb, same as HistoryDb pattern]

key-files:
  created: []
  modified:
    - crates/glass_mcp/Cargo.toml
    - crates/glass_mcp/src/lib.rs
    - crates/glass_mcp/src/tools.rs

key-decisions:
  - "Open-per-call CoordinationDb in spawn_blocking matches existing HistoryDb tool pattern"
  - "prune_stale(600) in list_agents matches Phase 31 10-minute timeout convention"
  - "AgentInfo serialized directly via serde Serialize derive from types.rs"

patterns-established:
  - "Agent coordination tool pattern: clone coord_db_path, spawn_blocking, open CoordinationDb, call method, map_err(internal_err), Content::json"

requirements-completed: [MCP-01, MCP-02, MCP-03, MCP-04, MCP-11, MCP-12]

# Metrics
duration: 3min
completed: 2026-03-09
---

# Phase 32 Plan 01: Agent Lifecycle MCP Tools Summary

**5 agent lifecycle MCP tools (register/deregister/list/status/heartbeat) exposing CoordinationDb operations via rmcp tool handlers**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-09T22:13:39Z
- **Completed:** 2026-03-09T22:17:19Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Extended GlassServer with coord_db_path field and 3-arg constructor
- Implemented 5 agent lifecycle MCP tool handlers following existing spawn_blocking pattern
- Added 5 parameter structs with schemars::JsonSchema for automatic MCP schema generation
- Added 7 parameter deserialization tests covering all structs including optional fields
- All 24 glass_mcp tests pass, clippy clean with -D warnings

## Task Commits

Each task was committed atomically:

1. **Task 1: Add coordination dependency and extend GlassServer** - `e9b377d` (feat)
2. **Task 2 RED: Add failing tests for agent lifecycle tools** - `fb0343c` (test)
3. **Task 2 GREEN: Implement 5 agent lifecycle MCP tool handlers** - `7ed8a5b` (feat)

## Files Created/Modified
- `crates/glass_mcp/Cargo.toml` - Added glass_coordination path dependency
- `crates/glass_mcp/src/lib.rs` - Resolve coord_db_path via glass_coordination::resolve_db_path(), pass to GlassServer
- `crates/glass_mcp/src/tools.rs` - coord_db_path field, 5 param structs, 5 tool handlers, 7 new tests

## Decisions Made
- Open-per-call CoordinationDb in spawn_blocking matches existing HistoryDb tool pattern for consistency
- prune_stale(600) called in glass_agent_list matches Phase 31 10-minute stale timeout convention
- AgentInfo serialized directly from types.rs Serialize derive -- no intermediate response type needed

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Agent lifecycle tools complete, ready for 32-02 (file locking and messaging MCP tools)
- GlassServer coord_db_path plumbing established for remaining coordination tools
- All existing MCP tools unaffected (backward compatible)

## Self-Check: PASSED

- All files verified present on disk
- All 3 commits verified in git log (e9b377d, fb0343c, 7ed8a5b)
- 24 glass_mcp tests pass, clippy clean

---
*Phase: 32-mcp-tools*
*Completed: 2026-03-09*

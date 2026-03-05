---
phase: 09-mcp-server
plan: 01
subsystem: api
tags: [mcp, rmcp, json-rpc, stdio, sqlite, tokio]

# Dependency graph
requires:
  - phase: 05-history-database-foundation
    provides: HistoryDb, CommandRecord, QueryFilter, filtered_query
  - phase: 07-history-cli
    provides: glass mcp serve subcommand routing, parse_time()
provides:
  - glass_mcp crate with run_mcp_server() entry point
  - GlassHistory MCP tool (filtered command query)
  - GlassContext MCP tool (aggregate activity summary)
  - Working glass mcp serve command over stdio
affects: [09-mcp-server]

# Tech tracking
tech-stack:
  added: [rmcp 1.1, schemars 1.2, serde_json 1.0, tokio-util]
  patterns: [tool_router macro, spawn_blocking for sync DB in async, stderr-only logging for MCP]

key-files:
  created:
    - crates/glass_mcp/src/tools.rs
    - crates/glass_mcp/src/context.rs
  modified:
    - crates/glass_mcp/Cargo.toml
    - crates/glass_mcp/src/lib.rs
    - Cargo.toml
    - src/main.rs

key-decisions:
  - "rmcp 1.1.0 (not 0.11) -- latest stable version on crates.io, API differs from blog examples"
  - "ServerInfo builder pattern (non-exhaustive struct) with with_server_info/with_instructions"
  - "Per-branch tracing init in main.rs to avoid double-init panic between terminal and MCP modes"
  - "internal_err helper function to reduce boilerplate in McpError conversions"

patterns-established:
  - "rmcp tool_router + tool_handler macros for MCP tool registration"
  - "Parameters<T> wrapper for tool handler function signatures"
  - "spawn_blocking for all HistoryDb operations in async context"
  - "Content::json() returns Result<Content, ErrorData> -- no extra map_err needed"

requirements-completed: [MCP-01, MCP-02, MCP-03]

# Metrics
duration: 15min
completed: 2026-03-05
---

# Phase 9 Plan 1: MCP Server Core Summary

**Two MCP tools (GlassHistory, GlassContext) over stdio using rmcp 1.1, with aggregate SQL queries and 7 unit tests**

## Performance

- **Duration:** 15 min
- **Started:** 2026-03-05T20:04:22Z
- **Completed:** 2026-03-05T20:19:00Z
- **Tasks:** 3
- **Files modified:** 6

## Accomplishments
- glass_mcp crate fully implemented with rmcp 1.1 tool_router/tool_handler macros
- GlassHistory tool queries command history with text, time, exit code, cwd, and limit filters
- GlassContext tool returns aggregate activity summary via SQL (counts, failures, directories)
- main.rs wired with tokio runtime, stderr-only tracing, and run_mcp_server() call
- 7 unit tests covering context aggregation and history entry conversion
- Full workspace passes 151 tests with zero regressions

## Task Commits

Each task was committed atomically:

1. **Task 1: Set up glass_mcp dependencies and response types** - `7b26b1f` (feat)
2. **Task 2: Wire main.rs with MCP server entry point** - `dfb8852` (feat)
3. **Task 3: Unit tests for tool logic and context queries** - `a6cf993` (test)

## Files Created/Modified
- `crates/glass_mcp/Cargo.toml` - Dependencies: rmcp 1.1, schemars, serde_json, glass_history, rusqlite
- `crates/glass_mcp/src/lib.rs` - Module declarations and run_mcp_server() async entry point
- `crates/glass_mcp/src/tools.rs` - GlassServer with glass_history and glass_context tool handlers
- `crates/glass_mcp/src/context.rs` - build_context_summary() aggregate SQL queries with tests
- `Cargo.toml` - Added glass_mcp and tokio dependencies to glass binary
- `src/main.rs` - Replaced MCP stub with working server, per-branch tracing init

## Decisions Made
- Used rmcp 1.1.0 (latest) instead of 0.11 (from research) -- API differs from blog examples (non-exhaustive ServerInfo, Parameters in wrapper module)
- Per-branch tracing initialization to prevent double-init panic when MCP mode needs stderr writer
- Created HistoryEntry response type separate from CommandRecord to control serialization (omit id, truncate output)
- Helper `internal_err()` function to reduce repetitive McpError conversion boilerplate

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed directory query with after filter**
- **Found during:** Task 3 (unit tests)
- **Issue:** The after-filtered directory query used `SELECT DISTINCT cwd ... ORDER BY MAX(started_at) DESC` without GROUP BY, which would not produce correct ordering
- **Fix:** Changed to `GROUP BY cwd ORDER BY last_used DESC` matching the no-filter branch
- **Files modified:** crates/glass_mcp/src/context.rs
- **Verification:** test_recent_directories_distinct_max_10 passes
- **Committed in:** a6cf993 (Task 3 commit)

**2. [Rule 3 - Blocking] Adapted rmcp API for version 1.1.0**
- **Found during:** Task 1 (compilation)
- **Issue:** Research documented rmcp 0.11 API (Parameters in tool module, ServerInfo struct literal). rmcp 1.1.0 has Parameters in wrapper module, non-exhaustive ServerInfo requiring builder pattern, Content::json returning ErrorData not serde_json::Error
- **Fix:** Used correct import paths, builder pattern for ServerInfo, explicit type annotations on error closures
- **Files modified:** crates/glass_mcp/src/tools.rs
- **Verification:** cargo check -p glass_mcp succeeds clean
- **Committed in:** 7b26b1f (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking)
**Impact on plan:** Both fixes necessary for correctness and compilation. No scope creep.

## Issues Encountered
- rmcp 1.1.0 API significantly differs from 0.11 blog examples -- resolved by reading rmcp source code directly

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- MCP server core is complete; Plan 2 (integration testing, error handling) can proceed
- glass mcp serve starts and responds to MCP initialize handshake
- Two tools registered and functional against the history database

---
*Phase: 09-mcp-server*
*Completed: 2026-03-05*

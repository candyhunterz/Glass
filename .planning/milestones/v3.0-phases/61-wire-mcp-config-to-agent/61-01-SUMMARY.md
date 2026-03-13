---
phase: 61-wire-mcp-config-to-agent
plan: 01
subsystem: agent
tags: [mcp, agent-runtime, cli, flush-collapsed]

# Dependency graph
requires:
  - phase: 56-agent-runtime
    provides: "AgentRuntime spawn, build_agent_command_args, activity stream"
  - phase: 53-soi-mcp-tools
    provides: "glass_query_trend and glass_query_drill MCP tools"
provides:
  - "Working MCP config JSON generation for agent subprocess"
  - "Defensive --mcp-config guard in build_agent_command_args"
  - "flush_collapsed at all agent shutdown sites"
  - "Default allowed_tools includes SOI query tools"
affects: [agent-runtime, mcp-tools]

# Tech tracking
tech-stack:
  added: []
  patterns: ["Closure-based graceful degradation for MCP config generation"]

key-files:
  created: []
  modified:
    - crates/glass_core/src/agent_runtime.rs
    - src/main.rs

key-decisions:
  - "MCP config uses closure returning Option<String> for graceful degradation -- unwrap_or_default yields empty string which build_agent_command_args now handles"
  - "flush_collapsed added at ConfigReloaded and AgentCrashed shutdown sites -- Drop impl cannot access Processor fields"

patterns-established:
  - "Conditional CLI flag emission: guard with !path.is_empty() before pushing flag+value pairs"

requirements-completed: [AGTR-03, SOIM-01, SOIM-02, SOIM-03]

# Metrics
duration: 3min
completed: 2026-03-13
---

# Phase 61 Plan 01: Wire MCP Config to Agent Summary

**Conditional --mcp-config emission with agent-mcp.json generation, flush_collapsed at shutdown sites, and SOI query tools in default allowed_tools**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-13T19:30:26Z
- **Completed:** 2026-03-13T19:33:25Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Fixed dangling --mcp-config flag bug by making emission conditional on non-empty path
- Agent subprocess now receives valid MCP config pointing to Glass MCP server (agent-mcp.json)
- flush_collapsed called before agent_runtime = None at ConfigReloaded and AgentCrashed shutdown paths
- Default allowed_tools updated to include glass_query_trend and glass_query_drill

## Task Commits

Each task was committed atomically:

1. **Task 1: Fix build_agent_command_args and update default allowed_tools** - `88836e2` (feat, TDD)
2. **Task 2: Write MCP config JSON and call flush_collapsed at shutdown sites** - `4d47505` (feat)

## Files Created/Modified
- `crates/glass_core/src/agent_runtime.rs` - Conditional --mcp-config emission, updated default allowed_tools, new test
- `src/main.rs` - MCP config JSON generation in try_spawn_agent, flush_collapsed at two shutdown sites

## Decisions Made
- MCP config generation uses a closure returning Option<String> for graceful degradation -- if current_exe() fails or write fails, empty path is used (no --mcp-config flag)
- flush_collapsed added at ConfigReloaded and AgentCrashed limit-exceeded sites only -- the Drop impl for AgentRuntime cannot access Processor's activity_filter

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Agent subprocess can now discover and invoke all Glass MCP tools including SOI query tools
- All workspace tests pass (1026 tests), zero clippy warnings

---
*Phase: 61-wire-mcp-config-to-agent*
*Completed: 2026-03-13*

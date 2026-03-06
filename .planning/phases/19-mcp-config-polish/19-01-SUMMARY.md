---
phase: 19-mcp-config-polish
plan: 01
subsystem: mcp, config
tags: [rmcp, mcp-tools, toml-config, sqlite-aggregate, pipeline-stats, schemars]

# Dependency graph
requires:
  - phase: 18-storage-retention
    provides: pipe_stages table, insert_pipe_stages/get_pipe_stages DB methods, PipeStageRow type
provides:
  - GlassPipeInspect MCP tool (5th tool) for AI pipeline stage inspection
  - Pipeline statistics in GlassContext response (count, avg stages, failure rate)
  - PipesSection config struct with enabled, max_capture_mb, auto_expand fields
  - Config-driven BufferPolicy, capture gating, and auto-expand override in main.rs
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Config override pattern: handle_event runs first, main.rs overrides after"
    - "PipeStageEntry local serialization struct to avoid cross-crate Serialize dependency"

key-files:
  created: []
  modified:
    - crates/glass_mcp/src/tools.rs
    - crates/glass_mcp/src/context.rs
    - crates/glass_core/src/config.rs
    - src/main.rs

key-decisions:
  - "PipeStageEntry local struct in tools.rs for Serialize instead of deriving on PipeStageRow in glass_history"
  - "auto_expand override in main.rs after handle_event (option b) keeps BlockManager config-agnostic"
  - "pipes.enabled=false only skips temp file reading, shell scripts still emit OSC sequences"

patterns-established:
  - "Config override pattern: BlockManager logic runs first, main.rs overrides based on config after handle_event"

requirements-completed: [MCP-01, MCP-02, CONF-01]

# Metrics
duration: 7min
completed: 2026-03-06
---

# Phase 19 Plan 01: MCP + Config + Polish Summary

**GlassPipeInspect MCP tool, pipeline stats in GlassContext, and [pipes] config section with 3 main.rs wiring points**

## Performance

- **Duration:** 7 min
- **Started:** 2026-03-06T18:12:25Z
- **Completed:** 2026-03-06T18:19:53Z
- **Tasks:** 3
- **Files modified:** 4

## Accomplishments
- Added GlassPipeInspect as 5th MCP tool with command_id + optional stage filter
- Extended GlassContext with pipeline_count, avg_pipeline_stages, and pipeline_failure_rate
- Added PipesSection config with enabled, max_capture_mb, auto_expand and serde defaults
- Wired all 3 config integration points: capture gate, BufferPolicy, auto-expand override
- All 376 workspace tests pass with zero regressions
- Clean release build

## Task Commits

Each task was committed atomically:

1. **Task 1: GlassPipeInspect MCP tool** - `1ea06ff` (test) + `3c5055d` (feat)
2. **Task 2: GlassContext pipeline statistics** - `26d3402` (test) + `15abbde` (feat)
3. **Task 3: PipesSection config and main.rs wiring** - `a34e41d` (test) + `a008dc4` (feat)

_Note: TDD tasks have separate RED (test) and GREEN (feat) commits._

## Files Created/Modified
- `crates/glass_mcp/src/tools.rs` - Added PipeInspectParams, PipeStageEntry, glass_pipe_inspect handler, updated module docs
- `crates/glass_mcp/src/context.rs` - Added pipeline_count, avg_pipeline_stages, pipeline_failure_rate fields and SQL queries
- `crates/glass_core/src/config.rs` - Added PipesSection struct with 3 fields and serde defaults, added to GlassConfig
- `src/main.rs` - Wired pipes.enabled capture gate, pipes.max_capture_mb BufferPolicy, pipes.auto_expand override

## Decisions Made
- Created local PipeStageEntry struct in tools.rs instead of adding Serialize to PipeStageRow in glass_history. Avoids cross-crate coupling.
- Used option (b) for auto_expand wiring: override pipeline_expanded in main.rs after handle_event, keeping BlockManager config-agnostic.
- pipes.enabled=false skips temp file reading in main.rs. Shell scripts still emit OSC sequences but Glass ignores them. Simplest approach.
- Used NULLIF in SQL for division-by-zero safety, with Rust-side 0.0 default for empty pipeline counts.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed empty array syntax in serde_json::json! macro**
- **Found during:** Task 1 (GlassPipeInspect MCP tool)
- **Issue:** `[] as [PipeStageEntry; 0]` is invalid syntax inside serde_json::json! macro
- **Fix:** Changed to `Vec::<PipeStageEntry>::new()` for empty stages response
- **Files modified:** crates/glass_mcp/src/tools.rs
- **Verification:** cargo test -p glass_mcp passes
- **Committed in:** 3c5055d (part of Task 1 feat commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Minor syntax fix for json! macro compatibility. No scope creep.

## Issues Encountered
None - all tasks executed cleanly after the macro syntax fix.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- v1.3 Pipe Visualization milestone complete (all 5 phases: 15-19)
- All MCP tools operational (5 total)
- Config system covers history, snapshot, and pipes sections
- 376 workspace tests green

## Self-Check: PASSED

All 5 modified/created files verified present. All 6 commit hashes verified in git log.

---
*Phase: 19-mcp-config-polish*
*Completed: 2026-03-06*

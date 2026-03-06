---
phase: 20-config-gate-dead-code-cleanup
plan: 01
subsystem: config
tags: [pty, shell-integration, env-var, config-gate, pipes]

# Dependency graph
requires:
  - phase: 19-mcp-config-polish
    provides: pipes.enabled config field and initial temp file reading gate
provides:
  - Three-layer pipes.enabled gating (PTY env var, shell script early-return, event loop skip)
  - GLASS_PIPES_DISABLED env var injected into PTY child environment
affects: [shell-integration, block-manager, pty]

# Tech tracking
tech-stack:
  added: []
  patterns: [env-var-based feature gating between terminal and shell process]

key-files:
  created: []
  modified:
    - crates/glass_terminal/src/pty.rs
    - src/main.rs
    - shell-integration/glass.bash
    - shell-integration/glass.ps1

key-decisions:
  - "Env var GLASS_PIPES_DISABLED=1 as IPC mechanism between terminal and shell (shells cannot read TOML config)"
  - "Gate at __glass_accept_line entry point in bash (single chokepoint covers all pipeline rewriting)"
  - "Pipeline events skipped before block_manager.handle_event to prevent empty stage accumulation"

patterns-established:
  - "Env var feature gating: terminal injects env vars into PTY child, shell scripts check them for behavior control"

requirements-completed: [CONF-01]

# Metrics
duration: 2min
completed: 2026-03-06
---

# Phase 20 Plan 01: Config Gate Summary

**Three-layer pipes.enabled gating via GLASS_PIPES_DISABLED env var: PTY injection, shell script early-return, and event loop pipeline skip**

## Performance

- **Duration:** 2 min
- **Started:** 2026-03-06T19:06:27Z
- **Completed:** 2026-03-06T19:08:27Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- spawn_pty now accepts pipes_enabled parameter and injects GLASS_PIPES_DISABLED=1 into PTY child environment when false
- main.rs skips PipelineStart/PipelineStage event processing when pipes.enabled=false, preventing empty BlockManager accumulation
- Bash shell integration returns early from __glass_accept_line when GLASS_PIPES_DISABLED=1 is set
- PowerShell shell integration wraps pipeline rewrite logic in env var check while preserving AcceptLine and OSC 133;C emission

## Task Commits

Each task was committed atomically:

1. **Task 1: Add pipes_enabled parameter to spawn_pty and inject env var** - `a1ebe1b` (feat)
2. **Task 2: Gate shell scripts with GLASS_PIPES_DISABLED env var check** - `f8a3d5a` (feat)

## Files Created/Modified
- `crates/glass_terminal/src/pty.rs` - Added pipes_enabled parameter, GLASS_PIPES_DISABLED env var injection
- `src/main.rs` - Pass pipes_enabled to spawn_pty, skip pipeline events when disabled
- `shell-integration/glass.bash` - Early return in __glass_accept_line when GLASS_PIPES_DISABLED=1
- `shell-integration/glass.ps1` - Wrap pipeline rewrite in env var check

## Decisions Made
- Env var GLASS_PIPES_DISABLED=1 chosen as IPC mechanism (shell scripts cannot read TOML config directly)
- Gate at __glass_accept_line in bash rather than individual functions (single chokepoint is simpler and sufficient)
- Pipeline events skipped before block_manager.handle_event() to prevent empty CapturedStage entries from accumulating

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Config gate complete, ready for 20-02 dead code cleanup plan
- All three gating layers verified present via grep

---
*Phase: 20-config-gate-dead-code-cleanup*
*Completed: 2026-03-06*

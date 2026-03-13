---
phase: 60-agent-configuration-and-polish
plan: 02
subsystem: agent-runtime
tags: [rust, agent, config, hot-reload, permission-matrix, quiet-rules, coordination]

# Dependency graph
requires:
  - phase: 60-agent-configuration-and-polish
    plan: 01
    provides: PermissionLevel, PermissionKind, PermissionMatrix, QuietRules, classify_proposal, should_quiet

provides:
  - Agent config hot-reload restart when [agent] section changes in ConfigReloaded arm
  - Quiet rules filter suppressing matching activity events in SoiReady arm
  - Permission matrix enforcement in AgentProposal arm (Never/Approve/Auto)
  - Coordination registration/deregistration in try_spawn_agent and Drop
  - Degradation hint ConfigError when claude binary missing and mode != Off

affects: [src/main.rs, agent UX, coordination integration]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Coordination soft errors: if let Ok(mut db) = open_default() -- failures warn! but never prevent agent start/stop"
    - "Config hot-reload creates fresh (tx, rx) channel pair -- old rx was consumed by previous writer thread"
    - "Permission matrix defaults to Approve when no [agent.permissions] section configured"
    - "Auto permission: create worktree then immediately apply before pushing to worktree list"
    - "Quiet rules gate: boolean flag gates activity_filter.process() call only -- SOI display unaffected"

key-files:
  created: []
  modified:
    - src/main.rs

key-decisions:
  - "coordination soft errors use if let Ok() wrapping -- never block agent lifecycle on DB availability"
  - "project_root field marked #[allow(dead_code)] -- stored for future coordination use but not yet read"
  - "Auto-applied toast uses proposal_idx = worktrees.len() (not pushed) -- toast still shows but no overlay entry"
  - "Config hot-reload resets activity_filter with new ActivityStreamConfig -- avoids stale window/dedup state"
  - "Quiet rules check happens before activity_filter.process() -- suppressed events never enter dedup window"

patterns-established:
  - "Coordination registration: try in try_spawn_agent, deregister in Drop -- mirrors AgentSessionDb pattern"
  - "Permission matrix: classify -> match level -> Never early-return, Auto inline-apply, Approve push to list"

requirements-completed: [AGTC-01, AGTC-04, AGTC-05]

# Metrics
duration: 12min
completed: 2026-03-13
---

# Phase 60 Plan 02: Agent Configuration Wiring Summary

**Five event handler wires in main.rs completing v3.0 agent configuration: coordination registration/deregistration, degradation hint, quiet rules filter, permission matrix enforcement, and config hot-reload restart**

## Performance

- **Duration:** 12 min
- **Started:** 2026-03-13T18:35:00Z
- **Completed:** 2026-03-13T18:47:00Z
- **Tasks:** 2
- **Files modified:** 1

## Accomplishments

- Added `agent_id` and `project_root` fields to `AgentRuntime` struct
- Wired `glass_coordination::CoordinationDb::register` + `lock_files` into `try_spawn_agent` after successful spawn
- Wired `unlock_all` + `deregister` into `AgentRuntime::Drop` impl before child kill
- Added AGTC-04 degradation hint: when mode != Off but `try_spawn_agent` returns None, sets `config_error` with install hint
- Added AGTC-03 quiet rules filter in `SoiReady` arm: `should_quiet` gates `activity_filter.process()` call
- Added AGTC-02 permission matrix in `AgentProposal` arm: Never drops, Approve existing behavior, Auto creates + immediately applies worktree
- Added AGTC-01 agent config hot-reload in `ConfigReloaded` arm: detects `agent_config_changed`, drops runtime, creates fresh channel, respawns
- All coordination failures are soft errors (warn! log only)
- Full workspace: build clean, clippy -D warnings clean, 421+ tests passing, fmt clean

## Task Commits

Each task was committed atomically:

1. **Task 1: Wire coordination, degradation hint, and add agent_id to AgentRuntime** - `5aff4c4` (feat)
2. **Task 2: Wire quiet rules, permission matrix, and config hot-reload restart** - `d484ef7` (feat)

## Files Created/Modified

- `src/main.rs` - AgentRuntime struct extended; Drop impl updated; try_spawn_agent coordination wiring; resumed() degradation hint; SoiReady quiet filter; AgentProposal permission matrix; ConfigReloaded hot-reload restart

## Decisions Made

- Coordination soft errors use `if let Ok(mut db) = open_default()` wrapping -- failures warn! but never block agent lifecycle
- `project_root` field marked `#[allow(dead_code)]` -- stored for future coordination features but not yet read-back
- Auto-applied toast uses `proposal_idx = worktrees.len()` (not pushed to list) -- toast renders but no overlay entry created
- Config hot-reload resets `activity_filter` with fresh `ActivityStreamConfig` -- avoids stale dedup window state from old config
- Quiet rules check placed before `activity_filter.process()` -- suppressed events never enter the dedup/rate-limit pipeline

## Self-Check: PASSED

- FOUND: src/main.rs (modified)
- FOUND commit: 5aff4c4 (Task 1)
- FOUND commit: d484ef7 (Task 2)
- cargo build: clean
- cargo clippy --workspace -- -D warnings: clean
- cargo test --workspace: all passing
- cargo fmt --all -- --check: clean

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Coordination API differs from plan interface spec**
- **Found during:** Task 1
- **Issue:** Plan specified `register_agent`, `deregister_agent`, `acquire_locks` but actual API is `register`, `deregister`, `lock_files` with different signatures
- **Fix:** Used actual API methods: `register(name, agent_type, project, cwd, pid)`, `lock_files(agent_id, &[PathBuf], reason)`, `unlock_all(agent_id)`, `deregister(agent_id)`
- **Files modified:** `src/main.rs`
- **Commit:** `5aff4c4`

**2. [Rule 2 - Formatting] cargo fmt applied after Task 2 changes**
- **Found during:** Task 2 verification
- **Issue:** Several tracing! macro calls exceeded line length and had inconsistent brace style
- **Fix:** Ran `cargo fmt --all` to normalize formatting
- **Files modified:** `src/main.rs`
- **Committed in:** `d484ef7` (format applied before commit)

---

**Total deviations:** 2 auto-fixed (1 API mismatch, 1 formatting)
**Impact on plan:** No scope creep. Both were necessary for correctness/CI compliance.

## User Setup Required

None -- no external service configuration required. Coordination uses existing `~/.glass/agents.db`.

## Next Phase Readiness

- All AGTC requirements satisfied
- v3.0 milestone complete: agent mode is fully configurable via config.toml with hot-reload
- Permission matrix, quiet rules, coordination, and degradation hint all wired and tested

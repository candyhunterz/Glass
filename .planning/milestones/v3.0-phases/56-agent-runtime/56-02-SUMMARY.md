---
phase: 56-agent-runtime
plan: "02"
subsystem: agent-runtime
tags: [agent-runtime, subprocess, status-bar, job-object, orphan-prevention, windows, unix]
dependency_graph:
  requires:
    - phase: 56-01
      provides: AgentMode, AgentRuntimeConfig, CooldownTracker, BudgetTracker, helper functions, AppEvent variants
    - phase: 55-02
      provides: activity_stream_rx field on Processor for writer thread to consume
  provides:
    - AgentRuntime struct with subprocess lifecycle in Processor
    - Reader thread routing AgentProposal/AgentQueryResult/AgentCrashed AppEvents
    - Writer thread draining activity_stream_rx with mode filter and cooldown gate
    - Crash restart with exponential backoff (5s/15s/45s, max 3)
    - Windows Job Object (KILL_ON_JOB_CLOSE) for orphan prevention
    - prctl(PR_SET_PDEATHSIG, SIGKILL) in unix spawn path
    - AgentSection in GlassConfig for TOML configuration
    - agent_cost_text/agent_cost_color in StatusLabel for cost display
    - Status bar renders agent cost in green (active) / red (paused)
  affects: [57-agent-worktree, 58-agent-ui, glass_renderer, glass_core]
tech_stack:
  added:
    - libc 0.2 (unix target dep for prctl)
    - Win32_System_Threading feature for windows-sys (GetCurrentProcess)
  patterns:
    - AgentRuntime struct owned by Processor (single-threaded, no Arc/Mutex needed)
    - Reader/writer threads use EventLoopProxy to send AppEvents back to winit loop
    - try_spawn_agent() returns Option<AgentRuntime> for graceful degradation (AGTR-04)
    - Agent cost display follows same positioning pattern as coordination_text
key-files:
  created: []
  modified:
    - src/main.rs
    - crates/glass_core/src/config.rs
    - crates/glass_renderer/src/status_bar.rs
    - crates/glass_renderer/src/frame.rs
    - Cargo.toml
key-decisions:
  - "AgentSection added to GlassConfig with mode/max_budget_usd/cooldown_secs/allowed_tools -- defaults to Off"
  - "try_spawn_agent() checks claude binary via Command::new('claude').arg('--version') before spawning"
  - "Writer thread manages its own inline cooldown (local Instant) to avoid Arc<Mutex> complexity"
  - "libc dep added as target.'cfg(unix)' to avoid unused-dep warnings on Windows"
  - "Windows Job Object stored as Option<isize> on Processor with #[allow(dead_code)] -- lifetime anchor"
  - "agent_cost_text positioned left of coordination_text using additive right-margin offsets"
  - "build_status_text accepts agent_paused: bool to toggle cost color green/red without extra field"
requirements-completed: [AGTR-01, AGTR-02, AGTR-04, AGTR-05, AGTR-06, AGTR-07]
duration: ~20 minutes
completed: 2026-03-13
---

# Phase 56 Plan 02: Agent Runtime Integration Summary

**Claude subprocess spawned in Processor with stdin/stdout pipes, reader/writer threads, exponential-backoff restart, Windows Job Object orphan prevention, and green/red cost display in the status bar.**

## Performance

- **Duration:** ~20 minutes
- **Started:** 2026-03-13T11:00:00Z
- **Completed:** 2026-03-13T11:20:00Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments

- Agent subprocess lifecycle wired into Processor: check binary, write system prompt, spawn claude CLI with piped stdin/stdout
- Reader thread parses JSON lines from claude stdout and routes AgentProposal/AgentQueryResult/AgentCrashed AppEvents via EventLoopProxy
- Writer thread drains activity_stream_rx with mode gate (should_send_in_mode) and inline cooldown, forwards JSON messages to claude stdin
- Crash handler with exponential backoff (5s/15s/45s) and max 3 restart attempts; creates new activity channel on each restart
- Windows Job Object with JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE prevents orphaned claude processes when Glass crashes
- Unix prctl(PR_SET_PDEATHSIG, SIGKILL) in #[cfg(unix)] pre_exec block kills child when parent dies
- AgentSection added to GlassConfig enabling [agent] TOML config (mode, max_budget_usd, cooldown_secs, allowed_tools)
- StatusLabel gains agent_cost_text and agent_cost_color; rendered in both single-pane and multi-pane status bar paths

## Task Commits

Each task was committed atomically:

1. **Task 1: AgentRuntime struct and subprocess spawn** - `2749a2e` (feat)
2. **Task 2: Platform orphan prevention and status bar cost display** - `1869c5c` (feat)

## Files Created/Modified

- `src/main.rs` - AgentRuntime struct, Drop impl, try_spawn_agent(), setup_windows_job_object(), agent event handlers, cost display in render paths
- `crates/glass_core/src/config.rs` - AgentSection struct and GlassConfig.agent field
- `crates/glass_renderer/src/status_bar.rs` - agent_cost_text/agent_cost_color fields in StatusLabel, updated build_status_text signature
- `crates/glass_renderer/src/frame.rs` - agent_cost_text parameter in draw_frame/draw_multi_pane_frame, rendering block in both functions
- `Cargo.toml` - libc workspace dep, unix target dep, Win32_System_Threading feature for windows-sys

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| `AgentSection` in `GlassConfig` | Follows pattern of HistorySection, SnapshotSection, etc. -- optional TOML section with defaults |
| Writer thread inline cooldown | Avoids Arc<Mutex<CooldownTracker>> across thread boundary; cooldown field on AgentRuntime struct is a conceptual anchor but writer owns its own Instant |
| `libc` as `target.'cfg(unix)'` dep | Windows is primary dev platform; unconditional dep causes "unused" warnings |
| System prompt written to `~/.glass/agent-system-prompt.txt` | Follows Glass's ~/.glass/ convention for runtime-generated files |
| No MCP config in Phase 56 | `--mcp-config` omitted until MCP server path is reliably available at spawn time (deferred to later phase) |
| `agent_cost_text` positioned left of `coordination_text` | Extends the right-to-left stacking pattern: git > coordination > agent cost |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Added AgentSection to GlassConfig**
- **Found during:** Task 1 (agent spawn logic)
- **Issue:** Plan assumed `self.config.agent` existed but `GlassConfig` had no `agent` field -- code would not compile
- **Fix:** Added `AgentSection` struct with mode/budget/cooldown/tools fields and optional `agent: Option<AgentSection>` to `GlassConfig`
- **Files modified:** `crates/glass_core/src/config.rs`
- **Verification:** Build passes, existing config tests unaffected
- **Committed in:** 2749a2e (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (Rule 2 - missing critical field)
**Impact on plan:** The AgentSection is directly required for the spawn logic to read configuration. No scope creep.

## Issues Encountered

- `CreateJobObjectW` returns `*mut c_void` (HANDLE) not `usize` -- comparison `job == 0` failed, fixed to `job.is_null()` (Rule 1 auto-fix)
- `build_status_text` exceeded clippy's 7-arg limit after adding agent parameters -- suppressed with `#[allow(clippy::too_many_arguments)]`
- `cargo fmt` required after initial implementation -- ran `cargo fmt --all` before final clippy pass

## User Setup Required

None - agent is disabled by default (mode = Off). To enable, users add to `~/.glass/config.toml`:
```toml
[agent]
mode = "Watch"  # or Assist, Autonomous
max_budget_usd = 1.0
```

## Next Phase Readiness

- Agent subprocess lifecycle is fully wired; Phase 58 (approval UI) can surface `agent_pending_proposals` Vec
- `try_spawn_agent` is ready for Phase 57 to wire git worktree creation into spawn path
- Status bar cost display is live and will update as soon as first `AgentQueryResult` arrives

## Self-Check: PASSED

- `src/main.rs` contains `AgentRuntime` struct -- FOUND
- `src/main.rs` contains `try_spawn_agent` function -- FOUND
- `src/main.rs` contains `setup_windows_job_object` function -- FOUND
- `crates/glass_core/src/config.rs` contains `AgentSection` -- FOUND
- `crates/glass_renderer/src/status_bar.rs` contains `agent_cost_text` -- FOUND
- `crates/glass_renderer/src/frame.rs` contains `agent_cost_text` parameter -- FOUND
- Task 1 commit `2749a2e` -- FOUND
- Task 2 commit `1869c5c` -- FOUND
- All 24 test suites pass, clippy clean, fmt clean

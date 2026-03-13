---
phase: 55-agent-activity-stream
plan: 01
subsystem: agent
tags: [activity-stream, rate-limiter, token-budget, noise-filter, mpsc, glass_core]

# Dependency graph
requires:
  - phase: 48-soi-core
    provides: "Severity strings (Error/Warning/Info/Success) used as collapse-candidate filter"
  - phase: 50-soi-integration
    provides: "SessionId from glass_core::event used as ActivityEvent field"
provides:
  - "ActivityEvent struct with command_id, session_id, summary, severity, token_cost, collapsed_count"
  - "ActivityStreamConfig with channel_capacity, token_budget, max_rate_per_sec defaults"
  - "ActivityFilter: token-bucket rate limiter, Success/Info collapse, rolling budget window"
  - "create_channel() factory returning bounded SyncSender/Receiver pair"
  - "estimate_tokens() for LLM cost estimation"
affects: [56-agent-runtime, glass_core]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Token-bucket rate limiter: bucket starts full at max_rate_per_sec, refilled by elapsed*rate, capped at max"
    - "Noise collapse: identical Success/Info events accumulate pending_collapsed; retroactively updates last window event on fingerprint change"
    - "Rolling budget window: VecDeque with cumulative token_cost; pop_front until within budget after each push"
    - "Bounded channel via std::sync::mpsc::sync_channel; callers MUST use try_send not send"

key-files:
  created:
    - crates/glass_core/src/activity_stream.rs
  modified:
    - crates/glass_core/src/lib.rs

key-decisions:
  - "ActivityFilter.process() collapses only Success and Info severity -- Error and Warning always pass through as actionable"
  - "pending_collapsed counter retroactively updates last window event's collapsed_count when fingerprint changes (lazy collapse)"
  - "Rate bucket starts full so first N calls in a burst pass, matching expected agent startup behavior"
  - "estimate_tokens uses (len/4)+8 -- fixed overhead of 8 ensures empty summaries still count toward budget"

patterns-established:
  - "flush_collapsed() helper provided for Phase 56 runtime shutdown drain path"
  - "window_events() returns iterator over &ActivityEvent (no cloning for read-only consumers)"

requirements-completed: [AGTA-01, AGTA-02, AGTA-03, AGTA-04]

# Metrics
duration: 2min
completed: 2026-03-13
---

# Phase 55 Plan 01: Agent Activity Stream Core Summary

**ActivityFilter in glass_core with token-bucket rate limiter (5/sec), Success/Info collapse, and 4096-token rolling budget window backed by bounded sync_channel**

## Performance

- **Duration:** 2 min
- **Started:** 2026-03-13T10:22:40Z
- **Completed:** 2026-03-13T10:24:30Z
- **Tasks:** 1 (TDD: implement + test in single pass)
- **Files modified:** 2

## Accomplishments
- `ActivityEvent` struct with all required fields including `collapsed_count` and `token_cost`
- `ActivityFilter::process()` implementing noise collapse (Success/Info only), token-bucket rate limiter, and rolling budget window
- `create_channel()` factory wrapping `sync_channel` with documented `try_send` requirement
- 8 unit tests covering all behaviors specified in the plan

## Task Commits

1. **Task 1: ActivityFilter implementation + 8 unit tests** - `5507fb4` (feat)

## Files Created/Modified
- `crates/glass_core/src/activity_stream.rs` - Full module: types, filter logic, channel factory, 8 tests
- `crates/glass_core/src/lib.rs` - Added `pub mod activity_stream;`

## Decisions Made
- Collapse strategy: pending_collapsed counter increments silently; when fingerprint changes the last window event is retroactively updated. This "lazy collapse" avoids emitting a placeholder event and keeps the window count accurate.
- Rate bucket starts full: first N calls in a burst (N = max_rate_per_sec) pass, matching how agents start fresh. Tests verify exactly 5 of 10 rapid-fire events pass with max_rate_per_sec=5.0.
- `flush_collapsed()` added as optional helper for Phase 56 agent runtime shutdown path (not tested here, covered by Phase 56).

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
- `cargo fmt` reformatted several lines (long method chains and assert_eq! with messages). Applied `cargo fmt` before committing. No logic changes.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- `ActivityFilter` and `create_channel()` are ready for Phase 56 agent runtime consumption
- All 4 AGTA requirements (AGTA-01 through AGTA-04) satisfied by the filter logic
- No blockers

---
*Phase: 55-agent-activity-stream*
*Completed: 2026-03-13*

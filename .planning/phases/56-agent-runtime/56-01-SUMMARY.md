---
phase: 56-agent-runtime
plan: "01"
subsystem: glass_core
tags: [agent-runtime, types, events, unit-tests]
dependency_graph:
  requires: [55-01, 55-02]
  provides: [56-02]
  affects: [glass_core]
tech_stack:
  added: []
  patterns: [pure-functions, tdd, derive-default-enum]
key_files:
  created:
    - crates/glass_core/src/agent_runtime.rs
  modified:
    - crates/glass_core/src/event.rs
    - crates/glass_core/src/lib.rs
    - Cargo.toml
decisions:
  - "AgentMode derives Default with #[default] on Off variant (clippy::derivable_impls)"
  - "CooldownTracker uses Option<Instant> + Duration — zero external deps, time-mockable via reset()"
  - "BudgetTracker accumulates f64 directly — no atomic needed since it lives on the Processor struct (single thread)"
  - "extract_proposal uses brace-depth walker instead of regex — avoids regex dep in glass_core"
  - "windows-sys features extended to Win32_System_JobObjects + Win32_Foundation now (avoids 2nd Cargo.toml edit in Plan 02)"
metrics:
  duration: ~10 minutes
  completed: 2026-03-13
  tasks_completed: 1
  files_created: 1
  files_modified: 3
---

# Phase 56 Plan 01: Agent Runtime Types and Helpers Summary

Agent runtime types, pure helpers, and AppEvent variants established in glass_core — all pure logic unit-testable without spawning a real subprocess.

## What Was Built

### `crates/glass_core/src/agent_runtime.rs` (483 lines)

**Types:**
- `AgentMode` — `Off | Watch | Assist | Autonomous`, derives Default with `#[default]` on `Off`
- `AgentRuntimeConfig` — `mode`, `max_budget_usd=1.0`, `cooldown_secs=30`, `allowed_tools` — all with documented defaults
- `AgentProposalData` — 5 fields: `description`, `action`, `severity`, `command_id`, `raw_response`
- `CooldownTracker` — wraps `Option<Instant>` + `Duration`; `check_and_update()` / `reset()`
- `BudgetTracker` — `accumulated: f64` + `max_budget: f64`; `add_cost()`, `is_exceeded()`, `cost_text()`, `paused_text()`

**Pure functions:**
- `should_send_in_mode(mode, severity)` — severity gate logic for all four modes
- `format_activity_as_user_message(event)` — produces valid Claude CLI stream-json user message
- `parse_cost_from_result(line)` — extracts `cost_usd` from result JSON lines
- `extract_proposal(text)` — finds `GLASS_PROPOSAL: {...}` marker and parses structured proposal
- `build_agent_command_args(config, prompt_path, mcp_config_path)` — CLI arg list for subprocess

**Tests:** 22 unit tests covering every type and function.

### `crates/glass_core/src/event.rs`

Added three new `AppEvent` variants:
- `AgentProposal(crate::agent_runtime::AgentProposalData)` — proposal ready for user review
- `AgentQueryResult { cost_usd: f64 }` — agent query completed, cost reported
- `AgentCrashed` — agent subprocess exited unexpectedly

### `crates/glass_core/src/lib.rs`

Added `pub mod agent_runtime;`.

### `Cargo.toml` (workspace)

Extended `windows-sys` features: `Win32_System_Console` + `Win32_System_JobObjects` + `Win32_Foundation` (needed by Plan 02 subprocess job control).

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| `AgentMode` derives `Default` with `#[default]` | Clippy `derivable_impls` catches manual `impl Default` — cleaner derive form |
| `CooldownTracker` uses `Option<Instant>` | No external deps; `reset()` enables deterministic testing |
| `BudgetTracker` uses plain `f64` | Lives on Processor struct (single-threaded); no atomic overhead needed |
| `extract_proposal` uses brace-depth walker | Avoids pulling `regex` crate into `glass_core` |
| `windows-sys` features extended in Plan 01 | Avoids a second `Cargo.toml` churn in Plan 02 |

## Verification Results

```
cargo test -p glass_core -- agent_runtime
  22 passed; 0 failed
cargo clippy -p glass_core -- -D warnings
  Finished (0 warnings)
cargo fmt --all -- --check
  (clean)
```

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Clippy flagged manual `impl Default` on `AgentMode`**
- **Found during:** Task 1 (clippy run)
- **Issue:** `clippy::derivable_impls` — manual `impl Default` is redundant when enum can derive it
- **Fix:** Replaced manual `impl Default` with `#[derive(Default)]` and `#[default]` attribute on `Off` variant
- **Files modified:** `crates/glass_core/src/agent_runtime.rs`
- **Commit:** 8f66628

None beyond the clippy fix above.

## Self-Check: PASSED

- `crates/glass_core/src/agent_runtime.rs` — FOUND (483 lines, >= 150 required)
- `crates/glass_core/src/event.rs` — FOUND, contains `AgentProposal`
- `crates/glass_core/src/lib.rs` — FOUND, contains `pub mod agent_runtime`
- Commit `8f66628` — FOUND
- All 22 tests pass, clippy clean, fmt clean

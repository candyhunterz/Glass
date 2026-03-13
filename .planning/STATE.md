---
gsd_state_version: 1.0
milestone: v3.0
milestone_name: SOI & Agent Mode
status: ready_to_plan
stopped_at: null
last_updated: "2026-03-12T00:00:00.000Z"
progress:
  total_phases: 13
  completed_phases: 0
  total_plans: 0
  completed_plans: 0
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-12)

**Core value:** A terminal that looks and feels normal but passively watches, indexes, and snapshots everything -- surfacing intelligence only when you need it.
**Current focus:** Phase 48 -- SOI Classifier and Parser Crate

## Current Position

Phase: 48 of 60 (SOI Classifier and Parser Crate)
Plan: 0 of TBD in current phase
Status: Ready to plan
Last activity: 2026-03-12 -- v3.0 roadmap created (phases 48-60)

Progress: [░░░░░░░░░░] 0% (v3.0: 0/13 phases)

## Performance Metrics

**Velocity (cumulative):**
- v1.0: 12 plans in ~1.8 hours (~9 min/plan)
- v1.1: 12 plans in ~4.5 hours (~20 min/plan)
- v1.2: 13 plans in ~6 hours (~28 min/plan)
- v1.3: 11 plans in ~2 hours (~11 min/plan)
- v2.0: 12 plans in ~23 min (~4 min/plan)
- v2.1: 11 plans in ~23 min (~3 min/plan)
- v2.2: 8 plans in ~30 min (~4 min/plan)
- v2.3: 9 plans in ~35 min (~4 min/plan)
- v2.4: 7 plans in ~25 min (~4 min/plan)
- v2.5: 6 plans in ~10 min (~2 min/plan)
- Total: 101 plans across 47 phases in 8 days

## Accumulated Context

### Decisions

See PROJECT.md Key Decisions table for full history.

Recent decisions relevant to v3.0:
- SOI summaries rendered as block decorations (NOT injected into PTY stream) to avoid OSC 133 race condition
- SOI parsing runs in spawn_blocking off main thread -- criterion input_latency benchmark must not regress
- Agent runtime is a struct in Processor (not a separate process) -- matches existing coordination poller pattern
- Approval UI is non-modal (toast + hotkeys) -- never captures keyboard focus from terminal
- max_budget_usd = 1.0 USD default is non-negotiable -- ships in Phase 56, not deferred
- git worktree registered in SQLite BEFORE creation -- crash recovery pattern from opencode PR #14649
- New crates needed: glass_soi, glass_agent; new deps: uuid 1.22, git2 0.20

### Pending Todos

1 pending (Mouse drag-and-select for copy paste).

### Blockers/Concerns

- Claude CLI JSON wire protocol schema needs validation before Phase 56 (may be moving target)
- git2 0.20 Windows path handling with spaces/non-ASCII not explicitly tested (Phase 57 risk)
- MCP tool token footprint of 25 existing tools unmeasured -- audit required before Phase 53 adds more
- macOS/Windows code signing still deferred
- pruner.rs max_size_mb not enforced (minor)

## Session Continuity

Last session: 2026-03-12
Stopped at: v3.0 roadmap created -- 13 phases (48-60), 62 requirements mapped, ready to plan Phase 48
Resume file: None

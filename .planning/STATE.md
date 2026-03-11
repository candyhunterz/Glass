---
gsd_state_version: 1.0
milestone: v2.5
milestone_name: UI Controls
status: unknown
stopped_at: Completed 47-02-PLAN.md
last_updated: "2026-03-11T04:18:07.270Z"
progress:
  total_phases: 3
  completed_phases: 3
  total_plans: 6
  completed_plans: 6
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-11)

**Core value:** A terminal that looks and feels normal but passively watches, indexes, and snapshots everything -- surfacing intelligence only when you need it.
**Current focus:** v2.5 UI Controls — scrollbar, tab bar buttons, tab drag reorder

## Current Position

Milestone: v2.5 UI Controls — IN PROGRESS
Phase 45: Scrollbar — COMPLETE (Plan 01 renderer + Plan 02 mouse interactions)
Phase 46: Tab Bar Controls — COMPLETE (Plan 01 renderer + Plan 02 event wiring)
Phase 47: Tab Drag Reorder — COMPLETE (Plan 01 core logic + Plan 02 event wiring)

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
- [Phase 46-tab-bar-controls]: TabHitResult enum for multi-target tab bar hit-testing (Tab, CloseButton, NewTabButton)
- [Phase 46-tab-bar-controls]: Hover-clear-on-close pattern: always reset tab_bar_hovered_tab after closing tabs
- [Phase 47]: to index is final position (post-removal) for reorder_tab semantics
- [Phase 47]: 5px horizontal threshold before drag activates to prevent accidental drags
- [Phase 47]: CursorMoved early return during drag prevents hover/selection interference

### Pending Todos

1 pending (Mouse drag-and-select for copy paste).

### Blockers/Concerns

- macOS/Windows code signing still deferred
- pruner.rs max_size_mb not enforced (minor)
- Nyquist validation partial across most phases

## Session Continuity

Last session: 2026-03-11T04:13:51Z
Stopped at: Completed 47-02-PLAN.md
Resume file: None

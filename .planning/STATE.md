---
gsd_state_version: 1.0
milestone: v2.0
milestone_name: Cross-Platform & Tabs
status: executing
stopped_at: Completed 24-02-PLAN.md
last_updated: "2026-03-07T03:14:29.072Z"
last_activity: 2026-03-07 -- Completed 24-02 (Tab Restructure and Per-Pane Rendering)
progress:
  total_phases: 4
  completed_phases: 3
  total_plans: 11
  completed_plans: 10
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-06)

**Core value:** A terminal that looks and feels normal but passively watches, indexes, and snapshots everything -- surfacing intelligence only when you need it.
**Current focus:** Phase 24 in progress -- Split Panes

## Current Position

Phase: 24 of 24 -- Split Panes
Plan: 2 of 3
Status: In Progress
Last activity: 2026-03-07 -- Completed 24-02 (Tab Restructure and Per-Pane Rendering)

## Performance Metrics

**Velocity (cumulative):**
- v1.0: 12 plans in ~1.8 hours (~9 min/plan)
- v1.1: 12 plans in ~4.5 hours (~20 min/plan)
- v1.2: 13 plans in ~6 hours (~28 min/plan)
- v1.3: 11 plans in ~2 hours (~11 min/plan)
- v2.0: 6 plans in ~23 min (~4 min/plan)
- Total: 54 plans across 24 phases in 3 days

## Accumulated Context

### Decisions

See PROJECT.md Key Decisions table for full history.

- [21-01] Copied SearchOverlay into glass_mux for per-session ownership
- [21-01] SessionId/TabId use u64 wrapper (no uuid dependency needed)
- [21-01] Platform helpers use cfg-gated function definitions per OS
- [21-02] SessionId defined in glass_core::event (not glass_mux) to avoid circular crate dependency
- [21-02] TerminalDirty excluded from session_id (any dirty triggers full redraw)
- [21-03] glass_mux re-exports glass_core::event::SessionId (unified type, no duplication)
- [21-03] Clone visible blocks and StatusState in render path for borrow-checker compliance
- [21-03] OverlayAction enum pattern for search overlay key handling
- [22-01] Inline default_shell_program() in pty.rs to avoid glass_terminal depending on glass_mux
- [22-01] Use not(any(windows, macos)) for Linux font default to cover other Unix-likes
- [22-01] Resolve effective shell via glass_mux::platform::default_shell() before find_shell_integration
- [22-02] Local PTY token constants matching upstream per-platform values (avoids pub(crate) dependency)
- [22-02] ScaleFactorChanged logs warning only (FrameRenderer lacks dynamic DPI update)
- [22-02] Cross-compilation validated per-crate (C deps need target CC toolchain, CI handles natively)
- [23-01] Tab title cloned from session.title at creation (not live-linked)
- [23-01] add_tab inserts after active tab (browser convention)
- [23-01] Test helper uses synthetic tabs without real Session instances
- [23-02] Tab bar color hierarchy: bar bg 30/255, inactive 35/255, active 50/255
- [23-02] 1px gap between tab rects for visual separation
- [23-02] cell_width stored on TabBarRenderer for future text centering
- [23-03] Subtract 2 lines for terminal size (status bar + tab bar)
- [23-03] Resize all sessions on window resize (not just active tab)
- [23-03] TerminalExit closes only affected tab by session_id lookup
- [23-03] create_session/cleanup_session extracted as reusable helpers
- [24-01] Usable-space-first gap accounting: subtract 2px gap before ratio split
- [24-01] Manhattan distance for find_neighbor across nested splits
- [24-01] resize_ratio finds nearest ancestor Split matching direction
- [24-02] Single-pane path uses existing draw_frame for zero regression risk
- [24-02] Multi-pane rendering uses viewport offsets + TextBounds clipping (not wgpu scissor_rect)
- [24-02] SplitNode.split_leaf in-place mutation for splitting focused pane
- [24-02] close_pane sets focused_pane to first_leaf of remaining tree
- [24-02] Divider rects computed by pairwise gap detection between pane viewports

### Pending Todos

None.

### Blockers/Concerns

- Research flag: Bash DEBUG trap reliability across bash versions needs testing
- Research flag: Expanded stage output for long captures may need virtual scrolling
- Known tech debt: pruner.rs max_size_mb not enforced
- Known tech debt: PipeStage.is_tty vestigial after classify.rs removal

## Session Continuity

Last session: 2026-03-07T03:14:29Z
Stopped at: Completed 24-02-PLAN.md
Resume file: None

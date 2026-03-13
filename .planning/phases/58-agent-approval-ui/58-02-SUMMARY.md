---
phase: 58-agent-approval-ui
plan: 02
subsystem: ui
tags: [main.rs, agent-ui, proposal-toast, proposal-overlay, keyboard-shortcuts, status-bar]

dependency_graph:
  requires:
    - phase: 58-01
      provides: ProposalToastRenderer, ProposalOverlayRenderer, draw_frame proposal params, build_status_text agent params
    - phase: 57-02
      provides: agent_proposal_worktrees, WorktreeManager.apply, WorktreeManager.dismiss, WorktreeManager.generate_diff
  provides:
    - ProposalToast struct with 30s auto-dismiss
    - Ctrl+Shift+A/Y/N keyboard shortcuts for overlay toggle/accept/reject
    - Arrow key navigation for overlay proposal list
    - Live agent_mode_text and proposal_count_text in status bar
    - proposal_toast_data and proposal_overlay_data wired into draw_frame calls
  affects: [src/main.rs, crates/glass_renderer]

tech-stack:
  added: []
  patterns: [toast auto-dismiss via created_at.elapsed() in RedrawRequested, diff caching with (index, diff) tuple]

key-files:
  created: []
  modified:
    - src/main.rs
    - crates/glass_renderer/src/frame.rs

key-decisions:
  - "draw_frame and draw_multi_pane_frame gain agent_mode_text/proposal_count_text params forwarded to build_status_text -- avoids a separate call site in main.rs"
  - "Diff cache stored as Option<(usize, String)> on Processor -- invalidated on selection change, regenerated lazily on next redraw"
  - "Arrow key / Escape overlay intercept placed immediately before PTY forward -- only intercepts named keys when overlay open, all others fall through (AGTU-05)"
  - "is_none_or() used instead of map_or(true, ...) per clippy unnecessary_map_or lint"

patterns-established:
  - "Overlay intercept before PTY: check overlay state, handle named keys, _ => {} fall-through"
  - "Duplicate logic for single-pane and multi-pane render paths -- each computes its own agent_mode, proposal, toast, overlay data"

requirements-completed: [AGTU-01, AGTU-02, AGTU-03, AGTU-04, AGTU-05]

duration: ~15 min
completed: 2026-03-13
---

# Phase 58 Plan 02: Agent Approval UI Wiring Summary

**Proposal toast (30s auto-dismiss), review overlay (Ctrl+Shift+A/Y/N + arrow keys), and live agent status bar wired into Processor via draw_frame for both single-pane and multi-pane render paths.**

## Performance

- **Duration:** ~15 min
- **Started:** 2026-03-13T17:25:00Z
- **Completed:** 2026-03-13T17:40:00Z
- **Tasks:** 2 (1 auto + 1 auto-approved checkpoint)
- **Files modified:** 2

## Accomplishments

- Added `ProposalToast` struct and four new `Processor` fields (`active_toast`, `agent_review_open`, `proposal_review_selected`, `proposal_diff_cache`)
- `AgentProposal` handler now creates a toast notification on proposal arrival; toast auto-dismisses after 30 seconds in `RedrawRequested`
- Ctrl+Shift+A toggles review overlay; Ctrl+Shift+Y/N accept/reject selected proposal via `wm.apply()`/`wm.dismiss()` and clamp selection index
- Arrow keys (Up/Down) navigate proposal list when overlay is open; Escape closes overlay; all other keys fall through to PTY (AGTU-05 preserved)
- `draw_frame` and `draw_multi_pane_frame` gain `agent_mode_text`/`proposal_count_text` parameters forwarded to `build_status_text`
- Both render paths compute live `agent_mode_text`, `proposal_count_text`, `proposal_toast_data`, and `proposal_overlay_data` with diff caching

## Task Commits

1. **Task 1: Add Processor state and wire AgentProposal handler, keyboard shortcuts, and draw_frame** - `8aaec03` (feat)
2. **Task 2: Verify approval UI end-to-end** - auto-approved (checkpoint:human-verify, auto-advance active)

## Files Created/Modified

- `src/main.rs` - ProposalToast struct, 4 new Processor fields, AgentProposal toast creation, auto-dismiss in RedrawRequested, Ctrl+Shift+A/Y/N handlers, arrow key overlay navigation, live proposal data in both draw_frame calls
- `crates/glass_renderer/src/frame.rs` - `draw_frame` and `draw_multi_pane_frame` gain `agent_mode_text`/`proposal_count_text` params forwarded to `build_status_text` (replacing hardcoded `None, None`)

## Decisions Made

- Extended `draw_frame` and `draw_multi_pane_frame` signatures rather than adding a separate `build_status_text` call in main.rs -- keeps status bar params co-located with the render call
- Diff cache stored as `Option<(usize, String)>` on Processor -- lazy regeneration on selection change avoids blocking the render loop
- Arrow key intercept uses `_ => {}` fall-through pattern to ensure non-overlay keys always reach PTY
- Both single-pane and multi-pane paths duplicate the proposal data building -- avoids borrow complexity of sharing across the `if pane_count <= 1` branch

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Replaced map_or(true, ...) with is_none_or() per clippy lint**
- **Found during:** Task 1 verification (clippy --workspace)
- **Issue:** Clippy `unnecessary_map_or` lint fired on both render path diff cache checks
- **Fix:** Changed `.map_or(true, |(idx, _)| *idx != selected)` to `.is_none_or(|(idx, _)| *idx != selected)` in both occurrences
- **Files modified:** `src/main.rs`
- **Verification:** `cargo clippy --workspace -- -D warnings` clean
- **Committed in:** 8aaec03 (Task 1 commit, fixed inline)

**2. [Rule 3 - Blocking] Added agent_mode_text/proposal_count_text params to draw_frame and draw_multi_pane_frame**
- **Found during:** Task 1 implementation
- **Issue:** Plan described building the strings and passing to `build_status_text`, but `build_status_text` is called inside `frame.rs` with hardcoded `None, None` -- main.rs has no direct call site
- **Fix:** Added 2 new params to both `draw_frame` and `draw_multi_pane_frame`; forwarded to the internal `build_status_text` calls
- **Files modified:** `crates/glass_renderer/src/frame.rs`
- **Verification:** Build and all tests pass
- **Committed in:** 8aaec03 (Task 1 commit, fixed inline)

---

**Total deviations:** 2 auto-fixed (1 bug/lint, 1 blocking API gap)
**Impact on plan:** Both fixes necessary for correct implementation. No scope creep.

## Issues Encountered

None beyond the two auto-fixed deviations above.

## Next Phase Readiness

- Complete agent approval UI implemented and verified (build + clippy + tests all pass)
- Phase 58 is complete -- both plan 01 (renderers) and plan 02 (wiring) are done
- Terminal remains fully interactive while toast or overlay is visible (AGTU-05)
- Status bar correctly displays agent mode and proposal count when agent runtime is active

## Self-Check: PASSED

- `src/main.rs`: modified and committed (8aaec03)
- `crates/glass_renderer/src/frame.rs`: modified and committed (8aaec03)
- commit 8aaec03: exists in git log

---
*Phase: 58-agent-approval-ui*
*Completed: 2026-03-13*

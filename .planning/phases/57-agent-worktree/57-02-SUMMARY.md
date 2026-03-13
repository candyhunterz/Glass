---
phase: 57-agent-worktree
plan: 02
subsystem: agent
tags: [glass_agent, WorktreeManager, worktree, AgentProposalData, file_changes]

# Dependency graph
requires:
  - phase: 57-01
    provides: "glass_agent crate with WorktreeManager, WorktreeHandle, WorktreeDb, and all lifecycle methods"

provides:
  - "AgentProposalData.file_changes field populated from GLASS_PROPOSAL JSON files array"
  - "extract_proposal parses optional files array (backward compatible)"
  - "Processor.worktree_manager initialized on startup with orphan pruning"
  - "Processor.agent_proposal_worktrees pairs proposals with worktree handles"
  - "AgentProposal events with file_changes create git/temp worktrees via WorktreeManager"

affects:
  - "58-agent-approval-ui: reads agent_proposal_worktrees for approval/dismiss operations"

# Tech tracking
tech-stack:
  added: ["glass_agent crate dependency in root Cargo.toml"]
  patterns:
    - "TDD RED-GREEN for struct field extension with backward-compatible optional JSON parsing"
    - "Processor field pairs data with its derived resource (proposal, Option<handle>)"
    - "Worktree manager initialized with prune_orphans on startup for crash recovery"
    - "Active session CWD as project root with std::env::current_dir() fallback"

key-files:
  created: []
  modified:
    - "crates/glass_core/src/agent_runtime.rs"
    - "src/main.rs"
    - "Cargo.toml"

key-decisions:
  - "agent_pending_proposals replaced by agent_proposal_worktrees: Vec<(AgentProposalData, Option<WorktreeHandle>)> -- pairs proposal with its worktree handle for Phase 58 approval UI"
  - "file_changes field defaults to empty Vec when files key absent in JSON -- backward compatible with existing proposals"
  - "CWD sourced from focused session status.cwd(), fallback to std::env::current_dir() for project root detection"
  - "WorktreeManager init failure is non-fatal (Some vs None) -- app starts regardless, logs warn"

patterns-established:
  - "Proposal-worktree pairing: store (AgentProposalData, Option<WorktreeHandle>) together so Phase 58 can apply/dismiss without a separate lookup"

requirements-completed: [AGTW-01, AGTW-02, AGTW-03, AGTW-04, AGTW-05]

# Metrics
duration: 3min
completed: 2026-03-13
---

# Phase 57 Plan 02: Wire WorktreeManager into Application Summary

**AgentProposalData extended with file_changes field and WorktreeManager wired into Processor -- proposals with file changes now create isolated git/temp worktrees on receipt, with crash recovery on startup.**

## Performance

- **Duration:** ~3 min
- **Started:** 2026-03-13T16:42:29Z
- **Completed:** 2026-03-13T16:45:39Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Extended AgentProposalData with `file_changes: Vec<(String, String)>` and updated extract_proposal to parse optional `files` JSON array (backward compatible)
- Replaced `agent_pending_proposals` with `agent_proposal_worktrees: Vec<(AgentProposalData, Option<WorktreeHandle>)>` in Processor
- WorktreeManager initialized on startup with `prune_orphans()` for crash recovery; non-fatal if initialization fails
- AgentProposal events now create worktrees for proposals that carry file changes; proposals without file changes store None handle

## Task Commits

Each task was committed atomically:

1. **Task 1: Extend AgentProposalData with file_changes and update extract_proposal** - `0a7fa65` (feat, TDD)
2. **Task 2: Wire WorktreeManager into Processor with proposal handling and orphan pruning** - `b76e256` (feat)

**Plan metadata:** (docs commit below)

_Note: Task 1 used TDD RED-GREEN: failing tests committed, then struct + parser updated to pass all 25 tests._

## Files Created/Modified
- `crates/glass_core/src/agent_runtime.rs` - Added `file_changes` field to `AgentProposalData`, updated `extract_proposal` to parse `files` JSON array, added 4 new tests (3 new + updated existing)
- `src/main.rs` - Added `worktree_manager` and `agent_proposal_worktrees` fields to Processor; updated initialization and `AgentProposal` event handler; removed `agent_pending_proposals`
- `Cargo.toml` - Added `glass_agent = { path = "crates/glass_agent" }` to root dependencies

## Decisions Made
- `agent_pending_proposals` replaced by `agent_proposal_worktrees` pairing proposals with `Option<WorktreeHandle>` -- Phase 58 approval UI needs both pieces together without a separate lookup
- `file_changes` defaults to empty Vec when `files` key absent -- full backward compatibility with Phase 56 proposals
- CWD sourced from `focused_session().status.cwd()` first, falls back to `std::env::current_dir()` for project root
- WorktreeManager init failure is non-fatal (logs warn, stores None) -- app remains functional without worktree isolation

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Phase 58 (agent approval UI) can now read `agent_proposal_worktrees` to render proposals and call `worktree_manager.apply()` or `dismiss()` on user action
- WorktreeHandle carries `worktree_path` and `changed_files` for diff display in Phase 58

## Self-Check: PASSED

All files found: agent_runtime.rs, main.rs, Cargo.toml, 57-02-SUMMARY.md
All commits found: 0a7fa65, b76e256

---
*Phase: 57-agent-worktree*
*Completed: 2026-03-13*

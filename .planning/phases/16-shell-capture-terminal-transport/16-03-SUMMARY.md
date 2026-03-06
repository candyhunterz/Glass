---
phase: 16-shell-capture-terminal-transport
plan: 03
subsystem: shell-integration
tags: [bash, powershell, tee, pipeline, osc-133, bind-x, psreadline, tee-object]

# Dependency graph
requires:
  - phase: 16-01
    provides: "OSC 133;S/P parsing in OscScanner and CapturedStage type"
provides:
  - "Bash pipeline rewriting with tee capture and OSC 133;S/P emission"
  - "PowerShell pipeline rewriting with Tee-Object capture and OSC 133;S/P emission"
  - "Enter key interception in both shells for transparent pipeline rewriting"
  - "Temp file lifecycle management (create on pipeline, cleanup on next prompt)"
affects: [16-02, phase-17]

# Tech tracking
tech-stack:
  added: []
  patterns: ["bind -x Enter interception for bash pipeline rewriting", "PSReadLine GetBufferState/Replace for PowerShell pipeline rewriting", "tee/Tee-Object insertion between pipe stages for output capture"]

key-files:
  created: []
  modified:
    - shell-integration/glass.bash
    - shell-integration/glass.ps1

key-decisions:
  - "Two-step bind for Enter: custom escape sequence triggers rewrite, then \\C-j accepts line"
  - "ST terminator (ESC \\) for S/P sequences in both shells to match OscScanner expectations"
  - "PIPESTATUS captured immediately after rewritten pipeline to preserve exit codes"
  - "Temp files cleaned up on next prompt cycle (terminal reads them before prompt returns)"

patterns-established:
  - "Quote-aware character walking for pipe detection (shared pattern in bash and PowerShell)"
  - "Pipeline rewriting pattern: insert tee/Tee-Object between stages, append emission call"

requirements-completed: [CAPT-01, CAPT-02]

# Metrics
duration: 2min
completed: 2026-03-06
---

# Phase 16 Plan 03: Shell Pipeline Capture Summary

**Bash and PowerShell pipeline rewriting with tee/Tee-Object capture and OSC 133;S/P emission for intermediate pipe stage output**

## Performance

- **Duration:** 2 min
- **Started:** 2026-03-06T07:05:49Z
- **Completed:** 2026-03-06T07:08:18Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Bash pipeline rewriting via bind -x Enter interception with tee insertion between pipe stages
- PowerShell pipeline rewriting via PSReadLine GetBufferState with Tee-Object insertion
- Both shells emit OSC 133;S (pipeline start with stage count) and 133;P (per-stage data with file path and size)
- Temp file lifecycle: created per-pipeline in TMPDIR, cleaned up on next prompt cycle
- Quote-aware pipe detection that skips logical OR (||), internal functions, and --no-glass commands

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement bash pipeline rewriting with tee capture and OSC emission** - `2e8ca15` (feat)
2. **Task 2: Implement PowerShell pipeline rewriting with Tee-Object and OSC emission** - `390b21f` (feat)

## Files Created/Modified
- `shell-integration/glass.bash` - Added __glass_has_pipes, __glass_tee_rewrite, __glass_emit_stages, __glass_cleanup_stages, __glass_accept_line with bind -x Enter interception
- `shell-integration/glass.ps1` - Added __Glass-Rewrite-Pipeline, __Glass-Emit-Stages, __Glass-Cleanup-Stages, modified PSReadLine Enter handler and prompt function

## Decisions Made
- Two-step bind approach for Enter key: `\e[glass-pre` triggers __glass_accept_line, then `\C-j` (accept-line) executes -- avoids recursion issues with direct bind -x on `\C-m`
- ST terminator (`ESC \`) used for S and P sequences (matching OscScanner parsing), while existing A/B/C/D sequences retain their original BEL terminators
- PIPESTATUS captured immediately after the rewritten pipeline executes, before __glass_emit_stages runs, to prevent any intervening command from overwriting it
- Temp files cleaned on next prompt cycle rather than immediately after emission -- the terminal has already read the file contents by then

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Shell-side capture is complete -- bash and PowerShell can now rewrite pipelines and emit OSC 133;S/P with temp file paths
- Plan 16-02 (wiring) can now connect terminal-side parsing (Plan 01) to shell-side emission (this plan)
- Temp file reading and stage buffer population in the terminal still needs implementation in Plan 02

## Self-Check: PASSED

- [x] shell-integration/glass.bash exists
- [x] shell-integration/glass.ps1 exists
- [x] 16-03-SUMMARY.md exists
- [x] Commit 2e8ca15 (Task 1) found
- [x] Commit 390b21f (Task 2) found

---
*Phase: 16-shell-capture-terminal-transport*
*Completed: 2026-03-06*

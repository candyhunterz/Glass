---
phase: 11-command-parser
plan: 02
subsystem: snapshot
tags: [powershell, command-parsing, cmdlet-dispatch, named-parameters]

# Dependency graph
requires:
  - phase: 11-command-parser
    plan: 01
    provides: parse_command, tokenize, resolve_path, dispatch_command, ParseResult/Confidence types
provides:
  - PowerShell cmdlet detection via Verb-Noun pattern heuristic
  - Named parameter extraction (-Path, -LiteralPath, -Destination)
  - PowerShell alias recognition (ri, mi, ci, del, etc.)
  - Read-only PowerShell cmdlet classification (Get-Content, Get-ChildItem, etc.)
affects: [12-fs-watcher, pre-exec-snapshot-integration]

# Tech tracking
tech-stack:
  added: []
  patterns: [powershell-cmdlet-dispatch, named-parameter-extraction, verb-noun-heuristic]

key-files:
  created: []
  modified: [crates/glass_snapshot/src/command_parser.rs]

key-decisions:
  - "PowerShell aliases (del, move, copy) routed to PS parser when detected, shadowing POSIX dispatch entries"
  - "Verb-Noun pattern heuristic: hyphen between alphabetic segments detects arbitrary cmdlets"
  - "tokenize_powershell uses simple quote-aware splitter without backslash escaping (PS uses backtick)"
  - "Unknown Verb-Noun cmdlets return Low confidence rather than attempting generic parsing"

patterns-established:
  - "PowerShell-first dispatch: is_powershell_cmdlet check before POSIX dispatch_command"
  - "Named parameter extraction: scan for -Path/-LiteralPath/-Destination with case-insensitive matching"
  - "Positional fallback: first non-flag arg used as path when no named parameter found"

requirements-completed: [SNAP-03]

# Metrics
duration: 3min
completed: 2026-03-05
---

# Phase 11 Plan 02: PowerShell Command Parser Summary

**PowerShell cmdlet parsing with Verb-Noun detection, named parameter extraction (-Path, -Destination), alias recognition, and 8 unit tests covering destructive/read-only cmdlets**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-05T22:48:26Z
- **Completed:** 2026-03-05T22:51:08Z
- **Tasks:** 1 (TDD: RED + GREEN)
- **Files modified:** 1

## Accomplishments
- PowerShell cmdlet detection via Verb-Noun pattern heuristic (Remove-Item, Get-Content, etc.)
- Named parameter extraction for -Path, -LiteralPath, -Destination with case-insensitive matching
- 16 PowerShell aliases recognized (ri, mi, ci, del, erase, rd, rmdir, move, copy, gc, gci, gl, gi, sc, clc, sls)
- Destructive cmdlets (Remove-Item, Move-Item, Copy-Item, Set-Content, Clear-Content) return High with file targets
- Read-only cmdlets (Get-Content, Get-ChildItem, Get-Location, Get-Item, Test-Path, Select-String) return ReadOnly
- All 22 command_parser tests pass, 234 workspace tests green with zero regressions

## Task Commits

Each task was committed atomically (TDD):

1. **Task 1 RED: PowerShell test scaffold** - `71b57c0` (test)
2. **Task 1 GREEN: Implement PowerShell parser** - `30cdde8` (feat)

## Files Created/Modified
- `crates/glass_snapshot/src/command_parser.rs` - Added PowerShell cmdlet detection, tokenizer, dispatch, named parameter extraction, destination extraction, and 8 new test cases

## Decisions Made
- PowerShell aliases (del, move, copy) are routed to the PowerShell parser when detected, shadowing their POSIX dispatch entries -- this is correct because these aliases behave differently in PowerShell context
- Verb-Noun heuristic uses simple alphabetic-hyphen-alphabetic pattern rather than maintaining an exhaustive cmdlet list
- tokenize_powershell is a standalone function (not shlex) since PowerShell uses backtick escaping instead of backslash
- Unknown Verb-Noun cmdlets (e.g., Invoke-CustomScript) return Low confidence to avoid false positives

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed borrow-after-move in PowerShell empty-token path**
- **Found during:** Task 1 GREEN phase
- **Issue:** redirect_targets moved into ParseResult before is_empty() check on same value
- **Fix:** Extract confidence before moving the Vec
- **Files modified:** crates/glass_snapshot/src/command_parser.rs
- **Verification:** Compiles and all tests pass
- **Committed in:** 30cdde8 (Task 1 GREEN commit)

---

**Total deviations:** 1 auto-fixed (1 bug fix)
**Impact on plan:** Trivial Rust borrow checker fix. No scope creep.

## Issues Encountered
- Implementation line count exceeds plan's soft 500-line target (817 lines impl) due to existing POSIX code being ~600 lines. PowerShell additions are ~170 lines, well-proportioned.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- parse_command now handles both POSIX and PowerShell commands
- PowerShell destructive commands will trigger pre-exec snapshots
- Parser remains a pure function with no state or DB access
- Ready for Phase 12 (FS watcher) integration

---
*Phase: 11-command-parser*
*Completed: 2026-03-05*

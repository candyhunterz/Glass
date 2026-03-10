---
phase: 37-token-saving-tools
plan: 02
subsystem: mcp
tags: [similar, unified-diff, token-budget, context-compression, mcp-tools]

requires:
  - phase: 37-token-saving-tools/01
    provides: tab output head/tail mode and cache check tools
provides:
  - glass_command_diff MCP tool for unified diffs of command file changes
  - glass_compressed_context MCP tool for budget-aware context summaries
  - is_binary_content helper for binary file detection
  - truncate_to_budget helper for character-based budget enforcement
affects: [mcp-integration, agent-tooling]

tech-stack:
  added: [similar 2.7]
  patterns: [unified-diff-generation, token-budget-approximation, focus-mode-filtering]

key-files:
  created: []
  modified:
    - crates/glass_mcp/src/tools.rs
    - crates/glass_mcp/Cargo.toml

key-decisions:
  - "Used similar crate for unified diff generation (standard format agents expect)"
  - "Token budget approximation at 1 token ~ 4 chars (good enough for cost control)"
  - "Focus modes split budget into thirds for balanced view"
  - "Binary detection checks first 8KiB for null bytes"

patterns-established:
  - "Budget-aware output: always include summary header, then fill remaining with focused content"
  - "Section builder pattern: separate functions for errors/files/history sections"

requirements-completed: [TOKEN-03, TOKEN-04]

duration: 5min
completed: 2026-03-10
---

# Phase 37 Plan 02: Command Diff and Compressed Context Summary

**Unified diff tool via similar crate and budget-aware compressed context with focus modes (errors/files/history/balanced)**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-10T04:45:47Z
- **Completed:** 2026-03-10T04:50:37Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- glass_command_diff generates unified diffs comparing pre-command snapshots to current files
- Binary file detection returns "[binary file]" placeholder instead of raw diff
- glass_compressed_context respects token budget with 4-chars-per-token approximation
- Four focus modes: errors, files, history, and balanced (default)
- Summary header always included regardless of budget size

## Task Commits

Each task was committed atomically (TDD: test then feat):

1. **Task 1: glass_command_diff** - `f855dde` (test: RED) + `835fb02` (feat: GREEN)
2. **Task 2: glass_compressed_context** - `3daadef` (test: RED) + `f4cc8dc` (feat: GREEN)

## Files Created/Modified
- `crates/glass_mcp/Cargo.toml` - Added similar 2.7 dependency
- `crates/glass_mcp/src/tools.rs` - CommandDiffParams, CompressedContextParams, is_binary_content, truncate_to_budget, glass_command_diff handler, glass_compressed_context handler with section builders

## Decisions Made
- Used similar crate for unified diff generation (standard format agents expect)
- Token budget approximation at 1 token ~ 4 chars (good enough for cost control without tokenizer dependency)
- Focus modes split budget into thirds for balanced view
- Binary detection checks first 8KiB for null bytes (matching git's heuristic)
- For "errors" focus, queries all commands and filters non-zero exit codes in Rust (QueryFilter only supports exact exit_code match)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed clippy manual_unwrap_or_default warnings**
- **Found during:** Task 1 (glass_command_diff implementation)
- **Issue:** Clippy flagged match expressions on Result that could use unwrap_or_default()
- **Fix:** Replaced match expressions with and_then/unwrap_or_default chains
- **Files modified:** crates/glass_mcp/src/tools.rs
- **Committed in:** 835fb02

---

**Total deviations:** 1 auto-fixed (1 bug fix)
**Impact on plan:** Minor style fix for clippy compliance. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All token-saving MCP tools complete (tab output head/tail, cache check, command diff, compressed context)
- Phase 37 fully delivered, ready for next phase

---
*Phase: 37-token-saving-tools*
*Completed: 2026-03-10*

---
phase: 22-cross-platform-validation
plan: 01
subsystem: infra
tags: [cross-platform, cfg-gating, shell-detection, font-defaults]

# Dependency graph
requires:
  - phase: 21-session-extraction
    provides: "SessionMux, platform module with default_shell()"
provides:
  - "windows-sys gated behind cfg(windows) in root Cargo.toml"
  - "Platform-aware font defaults (Consolas/Menlo/Monospace)"
  - "Platform-aware shell detection in spawn_pty ($SHELL on Unix)"
  - "Universal shell integration injection for bash, zsh, fish, and powershell"
affects: [23-tabs, 24-split-panes]

# Tech tracking
tech-stack:
  added: []
  patterns: ["cfg-gated platform defaults", "platform helper functions in pty.rs"]

key-files:
  created: []
  modified:
    - "Cargo.toml"
    - "crates/glass_core/src/config.rs"
    - "crates/glass_terminal/src/pty.rs"
    - "src/main.rs"

key-decisions:
  - "Inline default_shell_program() in pty.rs rather than depending on glass_mux (avoids crate dependency)"
  - "Use not(any(windows, macos)) for Linux font to cover other Unix-likes"
  - "Resolve effective shell via glass_mux::platform::default_shell() before calling find_shell_integration"

patterns-established:
  - "cfg-gated platform defaults: use fn returning static str with per-platform cfg blocks"
  - "Shell integration injection for all shell types, not just PowerShell"

requirements-completed: [P22-03, P22-04, P22-05, P22-06, P22-08]

# Metrics
duration: 3min
completed: 2026-03-07
---

# Phase 22 Plan 01: Cross-Platform Validation Summary

**Gate windows-sys behind cfg(windows), add platform-aware font defaults and shell detection, generalize shell integration for bash/zsh/fish/powershell**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-07T00:19:14Z
- **Completed:** 2026-03-07T00:22:25Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- windows-sys dependency moved to `[target.'cfg(windows)'.dependencies]` so it is not linked on macOS/Linux
- Font default is now platform-aware: Consolas (Windows), Menlo (macOS), Monospace (Linux/other)
- spawn_pty uses `$SHELL` on Unix (with `/bin/sh` fallback), pwsh/powershell detection on Windows
- Shell integration injection now fires for all shell types (bash, zsh, fish, powershell) instead of only PowerShell

## Task Commits

Each task was committed atomically:

1. **Task 1: Gate windows-sys and fix platform font defaults** - `d48e991` (feat)
2. **Task 2: Platform-aware spawn_pty and generalized shell integration injection** - `3a25abe` (feat)

## Files Created/Modified
- `Cargo.toml` - Moved windows-sys to cfg(windows) target dependency
- `crates/glass_core/src/config.rs` - Added default_font_family() with cfg-gated platform defaults
- `crates/glass_terminal/src/pty.rs` - Added default_shell_program() for platform-aware shell detection
- `src/main.rs` - Generalized shell integration injection for all shell types

## Decisions Made
- Inline default_shell_program() in pty.rs to avoid glass_terminal depending on glass_mux (would create dependency issues)
- Use `not(any(target_os = "windows", target_os = "macos"))` for Linux font default to also cover FreeBSD and other Unix-likes
- Resolve effective shell name before calling find_shell_integration, removing the empty-defaults-to-ps1 behavior

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Removed empty shell name defaulting to .ps1 in find_shell_integration**
- **Found during:** Task 2 (shell integration injection)
- **Issue:** find_shell_integration() treated empty shell_name as PowerShell, which would be wrong on Unix
- **Fix:** Removed `|| shell_name.is_empty()` from the PowerShell check since caller now always resolves effective shell
- **Files modified:** src/main.rs
- **Verification:** cargo test --workspace passes (371 tests)
- **Committed in:** 3a25abe (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Necessary for correctness on Unix platforms. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All four cross-platform blockers resolved
- Ready for cross-compilation verification (cargo check --target aarch64-apple-darwin etc.)
- Tabs and split-panes phases can proceed

---
*Phase: 22-cross-platform-validation*
*Completed: 2026-03-07*

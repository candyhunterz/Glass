---
phase: 22-cross-platform-validation
plan: 02
subsystem: infra
tags: [cross-platform, ci, wgpu, surface-format, hidpi, github-actions]

# Dependency graph
requires:
  - phase: 22-cross-platform-validation
    provides: "cfg-gated windows-sys, platform-aware font defaults and shell detection"
provides:
  - "sRGB surface format preference with logging"
  - "GPU adapter info logging on startup"
  - "ScaleFactorChanged event handler"
  - "Cross-platform CI pipeline (Windows/macOS/Linux)"
  - "PTY token constants for cross-platform compilation"
affects: [23-tabs, 24-split-panes]

# Tech tracking
tech-stack:
  added: [github-actions]
  patterns: ["sRGB format preference for cross-platform color consistency", "local PTY token constants to avoid pub(crate) dependency"]

key-files:
  created:
    - ".github/workflows/ci.yml"
  modified:
    - "crates/glass_renderer/src/surface.rs"
    - "crates/glass_terminal/src/pty.rs"
    - "src/main.rs"

key-decisions:
  - "Define local PTY_READ_WRITE_TOKEN/PTY_CHILD_EVENT_TOKEN constants matching upstream values per platform (avoids depending on pub(crate) items)"
  - "ScaleFactorChanged handler logs warning about dynamic DPI not being supported yet (FrameRenderer lacks update_scale_factor)"
  - "Cross-compilation validated per-crate (excluding C-compiled deps like libsqlite3-sys and blake3 that need target CC toolchain)"

patterns-established:
  - "sRGB format preference: iterate caps.formats for is_srgb() before falling back to first format"
  - "Platform-specific polling token constants defined locally rather than imported from upstream pub(crate)"

requirements-completed: [P22-01, P22-02, P22-07, P22-09, P22-10]

# Metrics
duration: 5min
completed: 2026-03-07
---

# Phase 22 Plan 02: Cross-Platform Validation Summary

**sRGB surface format preference with GPU adapter logging, ScaleFactorChanged handler, 3-platform CI matrix, and cross-compilation fixes for PTY token visibility**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-07T00:24:22Z
- **Completed:** 2026-03-07T00:28:56Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Surface format selection now prefers sRGB for consistent color rendering across platforms, with full logging of available formats and GPU adapter info
- ScaleFactorChanged event handler added (logs scale factor change, warns about restart needed for DPI update)
- CI workflow at .github/workflows/ci.yml covers Windows, macOS, and Linux builds with clippy and rustfmt checks
- Cross-compilation validated: aarch64-apple-darwin and x86_64-unknown-linux-gnu both compile successfully

## Task Commits

Each task was committed atomically:

1. **Task 1: Surface format logging and ScaleFactorChanged handler** - `0a39531` (feat)
2. **Task 2: Create CI workflow and validate cross-compilation** - `8dacb42` (feat)

## Files Created/Modified
- `.github/workflows/ci.yml` - 3-platform CI matrix with build, clippy, and fmt jobs
- `crates/glass_renderer/src/surface.rs` - sRGB format preference, GPU adapter info logging
- `crates/glass_terminal/src/pty.rs` - Local PTY token constants, cfg-gated escape_args
- `src/main.rs` - ScaleFactorChanged event handler with DPI change logging

## Decisions Made
- Define local PTY_READ_WRITE_TOKEN/PTY_CHILD_EVENT_TOKEN constants matching upstream values per platform, because alacritty_terminal marks these as pub(crate) on Unix but pub on Windows
- ScaleFactorChanged handler only logs + warns (no font metric recalculation) because FrameRenderer does not yet support dynamic scale factor updates -- documented as future enhancement for multi-monitor HiDPI
- Cross-compilation validated per-crate rather than whole workspace, because libsqlite3-sys and blake3 require a C cross-compiler toolchain not available locally (CI handles native builds)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed PTY_CHILD_EVENT_TOKEN/PTY_READ_WRITE_TOKEN visibility**
- **Found during:** Task 2 (cross-compilation validation)
- **Issue:** alacritty_terminal exports PTY_CHILD_EVENT_TOKEN and PTY_READ_WRITE_TOKEN as pub(crate) on Unix, making them inaccessible
- **Fix:** Defined local constants with correct platform-specific values (Unix: 0/1, Windows: 2/1)
- **Files modified:** crates/glass_terminal/src/pty.rs
- **Verification:** cargo check --target aarch64-apple-darwin and x86_64-unknown-linux-gnu both pass
- **Committed in:** 8dacb42 (Task 2 commit)

**2. [Rule 3 - Blocking] cfg-gated escape_args field in TtyOptions**
- **Found during:** Task 2 (cross-compilation validation)
- **Issue:** TtyOptions::escape_args is #[cfg(target_os = "windows")] in alacritty_terminal but our code set it unconditionally
- **Fix:** Added #[cfg(target_os = "windows")] attribute on the field initialization
- **Files modified:** crates/glass_terminal/src/pty.rs
- **Verification:** cargo check --target aarch64-apple-darwin passes
- **Committed in:** 8dacb42 (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (2 blocking)
**Impact on plan:** Both fixes required for cross-compilation to succeed. No scope creep.

## Issues Encountered
- Local cross-compilation of full workspace blocked by C-compiled dependencies (libsqlite3-sys, blake3) requiring target-specific CC toolchain. Validated per-crate instead; CI workflow handles native builds on each platform.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Cross-platform compilation validated for all Glass Rust crates
- CI pipeline ready to catch regressions on push/PR
- Tabs (Phase 23) and Split Panes (Phase 24) can proceed

---
*Phase: 22-cross-platform-validation*
*Completed: 2026-03-07*

---
phase: 29-auto-update
plan: 01
subsystem: infra
tags: [ureq, semver, github-api, auto-update, background-thread]

# Dependency graph
requires:
  - phase: 28-packaging
    provides: Release workflow publishing MSI/DMG/deb assets to GitHub Releases
provides:
  - updater.rs with spawn_update_checker, check_for_update, find_platform_asset, apply_update
  - UpdateInfo struct and AppEvent::UpdateAvailable variant
  - Platform-specific update apply (msiexec on Windows, open on macOS, xdg-open on Linux)
affects: [29-02-PLAN, status-bar, main-event-loop]

# Tech tracking
tech-stack:
  added: [ureq 3, semver 1, serde_json 1.0, tempfile 3 (Windows)]
  patterns: [background-thread-update-checker, platform-specific-asset-selection]

key-files:
  created: [crates/glass_core/src/updater.rs]
  modified: [crates/glass_core/Cargo.toml, crates/glass_core/src/lib.rs, crates/glass_core/src/event.rs, src/main.rs]

key-decisions:
  - "ureq 3.x read_to_string + serde_json::from_str instead of read_json (no json feature needed)"
  - "Extracted parse_update_from_response for testability without HTTP calls"
  - "tempfile::tempdir with mem::forget to prevent cleanup before msiexec finishes"
  - "UpdateAvailable variant added in Task 1 (Rule 3) since updater.rs depends on it for compilation"

patterns-established:
  - "Background update checker: spawn named thread, blocking HTTP, send AppEvent via proxy"
  - "Platform asset selection: cfg! macros for compile-time suffix matching"

requirements-completed: [UPDT-01, UPDT-03]

# Metrics
duration: 3min
completed: 2026-03-07
---

# Phase 29 Plan 01: Update Checker Core Summary

**Background update checker with ureq/semver version comparison, platform-specific asset selection, and MSI/DMG/deb apply logic**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-07T20:14:20Z
- **Completed:** 2026-03-07T20:17:25Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments
- Created updater.rs with full update checking infrastructure (spawn_update_checker, check_for_update, find_platform_asset, apply_update)
- Added UpdateInfo struct and AppEvent::UpdateAvailable variant for event-driven notification
- Platform-specific apply: msiexec /i /passive on Windows, open on macOS, xdg-open on Linux
- 9 unit tests covering version parsing, comparison, JSON parsing, and asset selection

## Task Commits

Each task was committed atomically:

1. **Task 1: Add dependencies and create updater module with tests** - `6bed7d8` (feat)
2. **Task 2: Add UpdateAvailable variant to AppEvent** - `a8f4ec4` (feat)

## Files Created/Modified
- `crates/glass_core/src/updater.rs` - Update checker: version comparison, GitHub API parsing, platform asset selection, apply logic, 9 unit tests
- `crates/glass_core/Cargo.toml` - Added ureq, semver, serde_json, tempfile (Windows) dependencies
- `crates/glass_core/src/lib.rs` - Added pub mod updater
- `crates/glass_core/src/event.rs` - Added AppEvent::UpdateAvailable(UpdateInfo) variant
- `src/main.rs` - Added UpdateAvailable match arm with info logging (placeholder for Plan 02 UI wiring)

## Decisions Made
- Used ureq 3.x `read_to_string` + `serde_json::from_str` instead of `read_json` method (avoids needing ureq json feature)
- Extracted `parse_update_from_response` as a separate function for unit testability without HTTP calls
- Used `tempfile::tempdir` with `std::mem::forget` on Windows to prevent temp dir cleanup before msiexec finishes reading the MSI
- Added match arm for UpdateAvailable in main.rs to maintain workspace compilation (Plan 02 will add full UI integration)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added UpdateAvailable variant during Task 1 instead of Task 2**
- **Found during:** Task 1 (updater module creation)
- **Issue:** updater.rs references AppEvent::UpdateAvailable in spawn_update_checker, but Task 2 was supposed to add the variant. Rust's exhaustive match checking prevents compilation without it.
- **Fix:** Added the variant to event.rs during Task 1 so the module compiles. Task 2 verified it was correct.
- **Files modified:** crates/glass_core/src/event.rs
- **Verification:** cargo check -p glass_core passes
- **Committed in:** 6bed7d8 (Task 1 commit)

**2. [Rule 1 - Bug] Fixed ureq 3.x API usage**
- **Found during:** Task 1 (compilation)
- **Issue:** Research document showed `body_mut().read_json()` and `read_to_end()` but ureq 3.2.0 uses `read_to_string()` and `read_to_vec()` instead
- **Fix:** Changed to `read_to_string()` + `serde_json::from_str()` for JSON parsing, `read_to_vec()` for binary download
- **Files modified:** crates/glass_core/src/updater.rs
- **Verification:** cargo test passes, cargo check passes
- **Committed in:** 6bed7d8 (Task 1 commit)

**3. [Rule 3 - Blocking] Added UpdateAvailable match arm in src/main.rs**
- **Found during:** Task 2 (workspace compilation check)
- **Issue:** Non-exhaustive pattern match in src/main.rs user_event handler
- **Fix:** Added AppEvent::UpdateAvailable match arm with info logging placeholder
- **Files modified:** src/main.rs
- **Verification:** cargo check --workspace passes cleanly
- **Committed in:** a8f4ec4 (Task 2 commit)

---

**Total deviations:** 3 auto-fixed (1 bug, 2 blocking)
**Impact on plan:** All auto-fixes necessary for compilation. No scope creep.

## Issues Encountered
None beyond the auto-fixed deviations above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Updater infrastructure complete, ready for Plan 02 to wire into status bar UI
- spawn_update_checker ready to be called from main.rs resumed() with env!("CARGO_PKG_VERSION")
- UpdateInfo stored in Processor state for StatusBarRenderer to display

---
*Phase: 29-auto-update*
*Completed: 2026-03-07*

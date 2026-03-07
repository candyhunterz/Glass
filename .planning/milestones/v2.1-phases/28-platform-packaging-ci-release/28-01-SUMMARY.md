---
phase: 28-platform-packaging-ci-release
plan: 01
subsystem: infra
tags: [cargo-wix, msi, dmg, deb, packaging, installer]

requires:
  - phase: none
    provides: n/a
provides:
  - Windows MSI installer definition via cargo-wix with stable UpgradeCode
  - macOS DMG build script with .app bundle creation
  - Linux .deb packaging metadata via cargo-deb
  - MIT LICENSE file for all installers
affects: [28-02-ci-release-workflow]

tech-stack:
  added: [cargo-wix, cargo-deb, hdiutil]
  patterns: [platform-specific packaging directories under packaging/]

key-files:
  created:
    - LICENSE
    - wix/main.wxs
    - wix/License.rtf
    - packaging/macos/Info.plist
    - packaging/macos/build-dmg.sh
    - packaging/linux/glass.desktop
  modified:
    - Cargo.toml

key-decisions:
  - "UpgradeCode GUID D5F79758-7183-4EBE-9B63-DADD19B1D42C is permanent for Windows MSI upgrades"
  - "Install directory is 'Glass Terminal' under ProgramFiles64Folder"
  - "macOS minimum version set to 11.0 (Big Sur)"
  - "Bundle identifier com.glass.terminal for macOS"

patterns-established:
  - "packaging/ directory structure: packaging/{platform}/ for platform-specific files"
  - "WiX PATH component pattern for system-wide CLI access on Windows"

requirements-completed: [PKG-01, PKG-02, PKG-03]

duration: 5min
completed: 2026-03-07
---

# Phase 28 Plan 01: Platform Packaging Summary

**Windows MSI via cargo-wix, macOS DMG via hdiutil .app bundle, and Linux .deb via cargo-deb metadata with MIT LICENSE**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-07T18:33:07Z
- **Completed:** 2026-03-07T18:38:00Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments
- Windows MSI installer definition with stable UpgradeCode GUID and PATH environment variable component
- macOS .app bundle creation script with DMG packaging via hdiutil
- Linux desktop entry and cargo-deb metadata with binary and .desktop asset mappings
- MIT LICENSE file at repo root for all platform installers

## Task Commits

Each task was committed atomically:

1. **Task 1: Create LICENSE file and Windows MSI packaging via cargo-wix** - `6163412` (feat)
2. **Task 2: Create macOS DMG packaging and Linux deb metadata** - `826cf19` (feat)

## Files Created/Modified
- `LICENSE` - MIT license for Glass Contributors
- `wix/main.wxs` - WiX installer definition with UpgradeCode, PATH env var, Glass Terminal product name
- `wix/License.rtf` - RTF license for Windows installer EULA dialog
- `packaging/macos/Info.plist` - macOS app bundle metadata with com.glass.terminal identifier
- `packaging/macos/build-dmg.sh` - Shell script to create .app bundle and DMG via hdiutil
- `packaging/linux/glass.desktop` - Linux desktop entry for application menus
- `Cargo.toml` - Added authors, license, description fields and [package.metadata.deb] section

## Decisions Made
- Used cargo-wix generated UpgradeCode GUID as permanent identifier (D5F79758-7183-4EBE-9B63-DADD19B1D42C)
- Added authors/license/description to root Cargo.toml (required by cargo-wix init)
- macOS minimum version 11.0 (Big Sur) for broad compatibility
- Bundle identifier com.glass.terminal follows reverse-domain convention

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added authors/license/description to Cargo.toml**
- **Found during:** Task 1 (cargo wix init)
- **Issue:** cargo-wix init requires 'authors' field in Cargo.toml manifest
- **Fix:** Added authors, license, and description fields to [package] section
- **Files modified:** Cargo.toml
- **Verification:** cargo wix init succeeded after adding fields
- **Committed in:** 6163412 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Required for cargo-wix to function. No scope creep.

## Issues Encountered
- cargo-wix init requires workspace package specification (`-p glass`) in workspace projects

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All three platform packaging configs are committed and ready for CI automation in Plan 02
- Windows: `cargo wix --no-build` can produce MSI (after release build)
- macOS: `bash packaging/macos/build-dmg.sh` can produce DMG (after release build)
- Linux: `cargo deb --no-build` can produce .deb (after release build)
- macOS code signing remains deferred (unsigned DMG triggers Gatekeeper)

---
*Phase: 28-platform-packaging-ci-release*
*Completed: 2026-03-07*

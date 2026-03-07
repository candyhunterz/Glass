---
phase: 30-documentation-distribution
plan: 03
subsystem: packaging
tags: [winget, homebrew, package-manager, distribution, yaml, ruby]

requires:
  - phase: 28-release-pipeline
    provides: "Release workflow producing MSI and DMG artifacts"
provides:
  - "Winget multi-file manifest (version, installer, locale) for Windows distribution"
  - "Homebrew cask formula for macOS distribution"
affects: [release-pipeline, distribution]

tech-stack:
  added: []
  patterns: ["Multi-file winget manifest (v1.6.0)", "Homebrew tap cask formula with livecheck"]

key-files:
  created:
    - packaging/winget/Glass.Terminal.yaml
    - packaging/winget/Glass.Terminal.installer.yaml
    - packaging/winget/Glass.Terminal.locale.en-US.yaml
    - packaging/homebrew/glass.rb
  modified: []

key-decisions:
  - "Winget manifest uses multi-file format (v1.6.0) with separate version, installer, and locale files"
  - "Homebrew formula targets custom tap (not official homebrew-cask) due to notarization requirement"
  - "Both manifests use <GITHUB_USER> and <SHA256> placeholders for release-time substitution"

patterns-established:
  - "Package manager manifests live in packaging/{manager}/ directory"
  - "Template placeholders (<GITHUB_USER>, <SHA256>) with inline comments explaining update steps"

requirements-completed: [PKG-05, PKG-06]

duration: 1min
completed: 2026-03-07
---

# Phase 30 Plan 03: Package Manager Manifests Summary

**Winget multi-file manifest and Homebrew cask formula with GitHub Releases URL patterns and livecheck**

## Performance

- **Duration:** 1 min
- **Started:** 2026-03-07T20:46:24Z
- **Completed:** 2026-03-07T20:47:44Z
- **Tasks:** 2
- **Files created:** 4

## Accomplishments
- Created winget three-file manifest (version, installer, defaultLocale) following v1.6.0 format
- Created Homebrew cask formula with livecheck block for automatic version detection
- Both manifests point to correct artifact URL patterns (MSI for Windows, DMG for macOS)
- UpgradeCode GUID included in winget installer manifest for seamless MSI upgrades

## Task Commits

Each task was committed atomically:

1. **Task 1: Create winget multi-file manifest** - `0219435` (feat)
2. **Task 2: Create Homebrew cask formula** - `ed9bc19` (feat)

## Files Created/Modified
- `packaging/winget/Glass.Terminal.yaml` - Winget version manifest
- `packaging/winget/Glass.Terminal.installer.yaml` - Winget installer manifest with MSI URL and UpgradeCode
- `packaging/winget/Glass.Terminal.locale.en-US.yaml` - Winget default locale with full description and tags
- `packaging/homebrew/glass.rb` - Homebrew cask formula with livecheck and macOS Big Sur dependency

## Decisions Made
- Used winget multi-file format (v1.6.0) for clean separation of concerns
- Homebrew formula targets a custom tap rather than official homebrew-cask (notarization deferred as PKG-F04)
- Consistent placeholder format (<GITHUB_USER>, <SHA256>) across both package managers

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Package manager manifests ready for use once first release is published
- For winget: compute SHA256 from MSI, replace placeholders, submit PR to microsoft/winget-pkgs
- For Homebrew: create homebrew-glass tap repo, place formula in Casks/glass.rb

## Self-Check: PASSED

All 4 created files verified on disk. Both task commits (0219435, ed9bc19) verified in git log.

---
*Phase: 30-documentation-distribution*
*Completed: 2026-03-07*

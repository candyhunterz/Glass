---
phase: 28-platform-packaging-ci-release
verified: 2026-03-07T19:00:00Z
status: passed
score: 8/8 must-haves verified
---

# Phase 28: Platform Packaging & CI Release Verification Report

**Phase Goal:** Users on Windows, macOS, and Linux can install Glass through platform-native installers downloaded from GitHub Releases
**Verified:** 2026-03-07T19:00:00Z
**Status:** passed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Running cargo wix --no-build on Windows produces an MSI installer in target/wix/ | VERIFIED | `wix/main.wxs` exists (230 lines) with stable UpgradeCode GUID, Product Name "Glass Terminal", PATH env var component, and binary reference via `$(var.CargoTargetBinDir)\glass.exe` |
| 2 | Running bash packaging/macos/build-dmg.sh produces a DMG in target/macos/ | VERIFIED | `packaging/macos/build-dmg.sh` exists (22 lines), copies `target/release/glass` into .app bundle, generates Info.plist with version substitution, calls `hdiutil create` to produce DMG |
| 3 | Running cargo deb --no-build produces a .deb package in target/debian/ | VERIFIED | `Cargo.toml` has `[package.metadata.deb]` section (lines 105-119) with assets mapping `target/release/glass` to `usr/bin/` and desktop entry to `usr/share/applications/` |
| 4 | Pushing a v* git tag triggers the release workflow | VERIFIED | `.github/workflows/release.yml` has `on: push: tags: ["v*"]` trigger (line 4-5) |
| 5 | The workflow builds Glass on Windows, macOS, and Linux in parallel | VERIFIED | Three parallel jobs defined: `build-windows` (windows-latest), `build-macos` (macos-latest), `build-linux` (ubuntu-latest) with no `needs:` dependencies |
| 6 | Each platform job packages the build into its native installer format | VERIFIED | Windows: `cargo wix --no-build`, macOS: `bash packaging/macos/build-dmg.sh`, Linux: `cargo deb --no-build` |
| 7 | All three installers are uploaded as GitHub Release assets | VERIFIED | All three jobs use `softprops/action-gh-release@v2` with correct file globs: `target/wix/*.msi`, `target/macos/*.dmg`, `target/debian/*.deb` |
| 8 | The workflow verifies Cargo.toml version matches the git tag | VERIFIED | Version verification step present in all three jobs comparing `CARGO_VERSION` from Cargo.toml with `TAG_VERSION` from `GITHUB_REF_NAME` |

**Score:** 8/8 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `wix/main.wxs` | WiX installer definition with stable UpgradeCode and PATH entry | VERIFIED | 230 lines, UpgradeCode D5F79758-7183-4EBE-9B63-DADD19B1D42C, Environment PATH component with GUID 6BC95DAA-3AF7-4CDF-AA9E-F4356B92D523 |
| `packaging/macos/Info.plist` | macOS app bundle metadata | VERIFIED | 24 lines, CFBundleIdentifier=com.glass.terminal, LSMinimumSystemVersion=11.0 |
| `packaging/macos/build-dmg.sh` | DMG creation script | VERIFIED | 22 lines, uses hdiutil create, copies binary into .app bundle |
| `packaging/linux/glass.desktop` | Linux desktop entry for application menus | VERIFIED | 9 lines, Terminal=true, Type=Application, Categories=System;TerminalEmulator |
| `Cargo.toml` | deb packaging metadata | VERIFIED | [package.metadata.deb] section at lines 105-119, assets map binary to usr/bin/ |
| `LICENSE` | MIT license file required by installers | VERIFIED | 21 lines, MIT license, copyright 2026 Glass Contributors |
| `.github/workflows/release.yml` | GitHub Actions release workflow triggered on v* tags | VERIFIED | 152 lines, three parallel jobs, softprops/action-gh-release, version verification |
| `wix/License.rtf` | RTF license for Windows installer EULA dialog | VERIFIED | File exists (bonus artifact, not in plan but referenced by main.wxs) |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `wix/main.wxs` | `target/release/glass.exe` | Source attribute in File element | WIRED | Line 127: `Source='$(var.CargoTargetBinDir)\glass.exe'` |
| `packaging/macos/build-dmg.sh` | `target/release/glass` | cp command copying binary | WIRED | Line 12: `cp target/release/glass "${BUNDLE_DIR}/Contents/MacOS/glass"` |
| `Cargo.toml` | `target/release/glass` | cargo-deb assets mapping | WIRED | Line 117: `["target/release/glass", "usr/bin/", "755"]` |
| `.github/workflows/release.yml` | `wix/main.wxs` | cargo wix in Windows job | WIRED | Line 45: `cargo wix --no-build --nocapture` |
| `.github/workflows/release.yml` | `packaging/macos/build-dmg.sh` | bash script in macOS job | WIRED | Line 86: `bash packaging/macos/build-dmg.sh "${GITHUB_REF_NAME#v}"` |
| `.github/workflows/release.yml` | `Cargo.toml` | cargo deb in Linux job | WIRED | Line 135: `cargo deb --no-build` |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| PKG-01 | 28-01 | MSI installer for Windows via cargo-wix with stable UpgradeCode | SATISFIED | `wix/main.wxs` with UpgradeCode, PATH component, binary reference, and License.rtf |
| PKG-02 | 28-01 | DMG bundle for macOS with proper Info.plist and app structure | SATISFIED | `packaging/macos/Info.plist` + `build-dmg.sh` creating .app bundle with hdiutil |
| PKG-03 | 28-01 | .deb package for Linux (Debian/Ubuntu) | SATISFIED | `[package.metadata.deb]` in Cargo.toml with assets and desktop entry |
| PKG-04 | 28-02 | GitHub Releases CI workflow building and publishing installers on tag | SATISFIED | `.github/workflows/release.yml` with 3 parallel jobs, version guard, and softprops upload |

No orphaned requirements found -- all four PKG-0x requirements are claimed by plans and satisfied.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| -- | -- | No anti-patterns found | -- | -- |

No TODO, FIXME, PLACEHOLDER, or stub patterns detected in any of the phase artifacts.

### Human Verification Required

### 1. Windows MSI Installation Test

**Test:** Build release binary on Windows, run `cargo wix --no-build`, execute the generated MSI installer
**Expected:** Glass installs to `C:\Program Files\Glass Terminal\bin\glass.exe`, PATH environment variable is updated, `glass` command works from a new terminal
**Why human:** Requires Windows environment with WiX Toolset installed, actual MSI execution

### 2. macOS DMG Installation Test

**Test:** Build release binary on macOS, run `bash packaging/macos/build-dmg.sh 0.1.0`, open the DMG, drag Glass.app to Applications
**Expected:** DMG mounts showing Glass.app, app launches from Applications folder (after Gatekeeper override via `xattr -cr`)
**Why human:** Requires macOS environment, actual DMG mount and app launch

### 3. Linux deb Installation Test

**Test:** Build release binary on Linux, run `cargo deb --no-build`, install with `sudo dpkg -i target/debian/glass_*.deb`
**Expected:** `glass` command available system-wide, desktop entry appears in application menus
**Why human:** Requires Linux environment, actual package installation

### 4. CI Release Pipeline End-to-End

**Test:** Push a `v0.1.0` tag to main branch on GitHub
**Expected:** Release workflow triggers, all three jobs succeed, GitHub Release is created with MSI, DMG, and deb assets
**Why human:** Requires GitHub repository access, actual CI execution, and artifact inspection

### Gaps Summary

No gaps found. All eight observable truths are verified through codebase inspection. All artifacts exist, are substantive (not stubs), and are properly wired together. All four PKG requirements are satisfied.

The phase deliverables are complete: platform-native packaging configurations for Windows (MSI), macOS (DMG), and Linux (deb), plus a GitHub Actions release workflow that builds and publishes all three when a version tag is pushed.

---

_Verified: 2026-03-07T19:00:00Z_
_Verifier: Claude (gsd-verifier)_

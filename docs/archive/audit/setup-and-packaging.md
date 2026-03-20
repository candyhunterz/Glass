# Setup & Packaging Audit

**Audited**: 2026-03-18
**Scope**: Installation methods, first-run experience, build-from-source, configuration defaults, platform-specific setup, shell integration, and install documentation.
**Verdict**: Solid foundation with several critical gaps that would block or frustrate new users.

---

## Summary

Glass has a well-structured release pipeline (MSI for Windows, DMG for macOS, DEB for Linux), a working `cargo install --git` path, sensible config defaults with graceful fallback, and automatic shell integration injection. However, the shell integration scripts are not bundled into any of the three platform installers, which means the core feature set (command blocks, undo, pipe visualization) will silently not work for anyone who installs via a release binary. Several other issues -- incomplete Linux deps in the README, no Scoop manifest for Windows, placeholder Homebrew/winget manifests, missing MSRV declaration, and unhelpful GPU failure messages -- round out the findings.

---

## 1. Installation Methods

### [CRITICAL] Shell integration scripts not included in any installer

**Current state**: The `find_shell_integration()` function in `src/main.rs:9499` searches for scripts at:
1. `<exe_dir>/shell-integration/<script>` (installed layout)
2. `<exe_dir>/../../shell-integration/<script>` (dev layout)

However, none of the three platform packaging pipelines include the `shell-integration/` directory:
- **Windows MSI** (`wix/main.wxs`): Only installs `glass.exe` and `License.rtf`. No `shell-integration/` directory.
- **macOS DMG** (`packaging/macos/build-dmg.sh`): Only copies `target/release/glass` into the `.app` bundle. No shell integration scripts.
- **Linux DEB** (`Cargo.toml [package.metadata.deb]`): Assets list only includes the binary and `.desktop` file. No shell integration scripts.

**Impact**: Without shell integration, Glass cannot detect command boundaries. Users get a plain terminal with no command blocks, no exit codes, no undo, no pipe visualization -- essentially none of the differentiating features work. The failure is silent (no error, no warning), making it extremely confusing.

**Recommendation**:
- MSI: Add a `Directory` and `Component` for `shell-integration/` in `wix/main.wxs`, placing the 4 scripts alongside the binary.
- DMG: Add `cp -r shell-integration "${BUNDLE_DIR}/Contents/Resources/shell-integration"` to `build-dmg.sh`, and update `find_shell_integration()` to search `../Resources/shell-integration/` relative to the executable inside `.app` bundles.
- DEB: Add `["shell-integration/*", "usr/share/glass/shell-integration/", "644"]` to the `[package.metadata.deb]` assets and update the search path.

### [HIGH] No Scoop manifest for Windows

**Current state**: Windows has a WiX MSI installer and winget manifest templates, but no Scoop manifest. Scoop is the most common developer-oriented package manager on Windows.

**Recommendation**: Create `packaging/scoop/glass.json` with the standard Scoop manifest format pointing to the MSI or a standalone zip of the binary + shell-integration scripts.

### [MEDIUM] Homebrew and winget manifests are placeholder templates

**Current state**:
- `packaging/homebrew/glass.rb`: Contains `<GITHUB_USER>` and `<SHA256>` placeholders. Version hardcoded to `0.1.0`.
- `packaging/winget/Glass.Terminal.installer.yaml`: Contains `<GITHUB_USER>` and `<SHA256>` placeholders. Version hardcoded to `0.1.0`.

These are not automatically updated by the release workflow.

**Recommendation**: Either automate manifest generation in the release workflow (e.g., compute SHA256 and substitute placeholders), or document the manual release checklist for updating these files per release.

### [MEDIUM] No `cargo install glass` from crates.io

**Current state**: `Cargo.toml` has `name`, `version`, `license`, and `description` but is missing `repository`, `homepage`, `categories`, `keywords`, and `readme` fields needed for crates.io publishing. There is no `publish` field (which means publishing is not explicitly blocked), but the package has never been published.

**Recommendation**: Add the missing metadata fields and consider publishing to crates.io. Even if the binary is primarily distributed via installers, `cargo install glass` is the standard way Rust developers expect to install Rust tools.

### [LOW] No AUR package for Arch Linux

**Current state**: Only Debian-based Linux is covered by the DEB package. Arch Linux users have no package.

**Recommendation**: Create a PKGBUILD in `packaging/aur/` for community submission. Low priority since `cargo install --git` works as a fallback.

---

## 2. First-Run Experience

### [HIGH] No warning when shell integration scripts are not found

**Current state**: In `src/main.rs:711`, if `find_shell_integration()` returns `None`, the code silently continues. No log warning, no status bar message, no user notification. The user sees a working terminal but none of the Glass-specific features function.

**Recommendation**: Add a `tracing::warn!("Shell integration scripts not found -- command blocks, undo, and pipe features will not work")` and consider showing a one-time toast or status bar warning on the first session.

### [HIGH] GPU initialization panics with unhelpful messages

**Current state**: In `crates/glass_renderer/src/surface.rs`, GPU initialization uses `expect()` at three points:
- Line 32: `expect("Failed to create wgpu surface")`
- Line 41: `expect("No compatible GPU adapter found")`
- Line 49: `expect("Failed to create wgpu device")`

On systems without GPU support (headless servers, old machines, VMs), Glass panics with a raw Rust panic message. The crash handler writes to `~/.glass/crash.log` and tries to open a GitHub issue, but the user never sees a clear "Glass requires a GPU" message.

**Recommendation**: Replace `expect()` with `match` and print a user-friendly error message to stderr before exiting, e.g., "Glass requires a GPU with DX12/Vulkan/Metal support. Run `glass --check-gpu` to diagnose."

### [MEDIUM] Config file is never created on first run

**Current state**: `GlassConfig::load()` (in `crates/glass_core/src/config.rs:420`) silently falls back to defaults if `~/.glass/config.toml` does not exist. It never creates a default config file. The comment explicitly states: "Does NOT create the config file if it does not exist."

While the defaults are sensible and Glass works fine without a config file, new users have no easy way to discover what's configurable. They must read the README or stumble upon the Settings overlay (Ctrl+Shift+,).

**Recommendation**: Consider creating a commented-out default config file on first launch (common pattern in terminal emulators like Alacritty and Kitty), or at minimum, log an info message telling the user where to create a config file.

### [LOW] No `--check` or `--version` diagnostic mode

**Current state**: `glass --version` works (clap `version` attribute), but there is no diagnostic command to verify that the GPU, shell, and shell integration are all working correctly.

**Recommendation**: Add a `glass check` subcommand that reports: GPU adapter info, detected shell, shell integration script path (found/not found), and config file path (exists/not exists).

---

## 3. Build from Source

### [HIGH] README Linux deps list is incomplete

**Current state**: The README instructs Linux users to install:
```
sudo apt install libxkbcommon-dev libwayland-dev libx11-dev libxi-dev
```

But both the CI workflow (`.github/workflows/ci.yml:37`) and the release workflow (`.github/workflows/release.yml:133`) also install `libxtst-dev`. The build will fail on a fresh Ubuntu system following the README instructions alone.

**Recommendation**: Update the README to include `libxtst-dev` in the Linux dependency list.

### [MEDIUM] No MSRV (Minimum Supported Rust Version) declared

**Current state**: `Cargo.toml` specifies `edition = "2021"` but does not declare `rust-version`. There is no `rust-toolchain.toml` file. The CI uses `dtolnay/rust-toolchain@stable` which always uses the latest stable. A user with an older Rust toolchain may get confusing build errors.

**Recommendation**: Add `rust-version = "1.80"` (or whatever the actual minimum is) to `Cargo.toml` so Cargo gives a clear error message on too-old toolchains. Optionally add a `rust-toolchain.toml`.

### [MEDIUM] No documented Fedora/RHEL/Arch build dependencies

**Current state**: Linux build deps are only listed for Debian/Ubuntu (`apt install`). Users on Fedora, RHEL, openSUSE, or Arch need to figure out the equivalent package names themselves.

**Recommendation**: Add equivalent `dnf install` and `pacman -S` commands in the README or a BUILDING.md file.

### [LOW] Build warnings without icon asset

**Current state**: `build.rs` checks for `assets/icon.ico` and prints a cargo warning if missing: `cargo:warning=assets/icon.ico not found`. The file exists in the repo, but users building from a tarball or shallow clone might not have it. The warning is non-fatal but noisy.

**Recommendation**: This is fine as-is. The warning is informational only.

---

## 4. Configuration Defaults

### [LOW] All defaults are sensible and well-tested

**Current state**: The `GlassConfig::default()` implementation provides:
- Platform-appropriate default fonts: Consolas (Windows), Menlo (macOS), Monospace (Linux)
- Shell auto-detection: pwsh > PowerShell (Windows), `$SHELL` > `/bin/sh` (Unix)
- Reasonable defaults for all optional sections (history, snapshot, pipes, SOI, agent, scripting)
- Agent mode defaults to Off
- Orchestrator defaults to disabled

The `#[serde(default)]` attribute on `GlassConfig` means a completely empty or missing config file works correctly. Malformed TOML also falls back to defaults without crashing. These behaviors are well-tested with 30+ unit tests.

**Recommendation**: No changes needed. This is one of the strongest areas of the codebase.

---

## 5. Platform-Specific Setup

### [MEDIUM] Windows: `#![windows_subsystem = "windows"]` suppresses CLI subcommand output

**Current state**: `src/main.rs:3` sets `#![windows_subsystem = "windows"]` which suppresses the console window. The comment says "CLI subcommands (history, undo, mcp) still work when launched from an existing terminal." This is true -- but if a user double-clicks `glass.exe` and tries `glass history search "foo"` from the Windows Run dialog or a shortcut, they get no output because there is no attached console.

**Recommendation**: This is a known Windows terminal emulator trade-off (Alacritty has the same behavior). Consider documenting it: "CLI subcommands must be run from an existing terminal, not by double-clicking glass.exe."

### [MEDIUM] macOS: No code signing or notarization

**Current state**: The DMG build script (`packaging/macos/build-dmg.sh`) creates an unsigned, unnotarized DMG. macOS Gatekeeper will block it by default. The release notes mention `xattr -cr /Applications/Glass.app`, but this is a poor user experience.

The Homebrew formula comment notes: "The official homebrew-cask repo requires notarization (deferred -- PKG-F04)."

**Recommendation**: This is a known deferred item. For launch, at minimum ensure the Gatekeeper workaround is prominently documented in the release notes and README installation section.

### [MEDIUM] macOS: DMG only builds for aarch64 (Apple Silicon)

**Current state**: `packaging/macos/build-dmg.sh` hardcodes `aarch64` in the output filename: `Glass-${VERSION}-aarch64.dmg`. The release workflow runs on `macos-latest` which is Apple Silicon. Intel Mac users have no pre-built binary.

**Recommendation**: Add a second macOS build job targeting `x86_64-apple-darwin`, or build a universal binary.

### [LOW] Linux: Desktop entry uses generic icon

**Current state**: `packaging/linux/glass.desktop` sets `Icon=utilities-terminal` (a generic system icon) rather than a Glass-specific icon.

**Recommendation**: Include a Glass icon (SVG or PNG) in the DEB package and reference it in the desktop entry.

---

## 6. Shell Integration

### [CRITICAL] Shell integration scripts not distributed with installers

(See finding #1 above -- this is the same issue viewed from the shell integration perspective.)

### [LOW] Shell integration injection is visible in terminal output

**Current state**: Shell integration is injected by typing `source '<path>'` into the PTY (`src/main.rs:711-724`). This command appears in the terminal scrollback on startup. Most users will not notice, but it's visible if they scroll up.

**Recommendation**: This is a minor cosmetic issue. Some terminals (e.g., WezTerm) inject via environment variables before shell startup to avoid visible commands. Consider `BASH_ENV`, `ZDOTDIR`, or `ENV` injection as a future improvement.

### [LOW] No fallback or warning for unsupported shells

**Current state**: `find_shell_integration()` maps shells to scripts: pwsh/powershell -> glass.ps1, zsh -> glass.zsh, fish -> glass.fish, everything else -> glass.bash. If a user runs a shell not in this list (e.g., nushell, elvish, tcsh), they get the bash integration which will likely not work.

**Recommendation**: Log a warning when the detected shell does not match any known integration script.

---

## 7. README / Install Documentation

### [HIGH] README missing Linux dependency (`libxtst-dev`)

(See finding in Build from Source section above.)

### [MEDIUM] README does not mention package manager installation (Homebrew, winget, Scoop)

**Current state**: The Installation section only covers "Pre-built binaries" (download from releases), "Build from source", and "Cargo install". The Homebrew formula, winget manifests, and lack of Scoop are not mentioned. Once these are set up, the README should include them.

**Recommendation**: Add package manager sections once the manifests are finalized and published.

### [MEDIUM] No CONTRIBUTING.md

**Current state**: There is no CONTRIBUTING.md file. Contributors have to read CLAUDE.md (which is an AI context file, not a human contributor guide) to find build commands and conventions.

**Recommendation**: Create a CONTRIBUTING.md with build instructions, test commands, code style expectations, and PR process.

### [LOW] "The binary is self-contained" claim is misleading

**Current state**: The README states: "The binary is self-contained. Shell integration scripts are embedded and auto-injected at PTY spawn time." This is incorrect -- the shell integration scripts are NOT embedded in the binary. They are separate files loaded at runtime from paths relative to the executable. The `find_shell_integration()` function reads them from disk, not from `include_str!()` or similar.

**Recommendation**: Either embed the scripts in the binary using `include_str!()` (which would make the claim true and also fix the packaging issue), or correct the README statement to: "Shell integration scripts must be co-located with the binary in a `shell-integration/` directory."

---

## Priority Fix List

Ordered by impact on a new user trying Glass for the first time:

| Priority | Severity | Finding | Effort |
|----------|----------|---------|--------|
| 1 | CRITICAL | Shell integration scripts not included in any installer (MSI, DMG, DEB) | Medium -- update 3 packaging configs + verify |
| 2 | HIGH | No warning when shell integration scripts not found at runtime | Low -- add tracing::warn + optional toast |
| 3 | HIGH | README Linux deps missing `libxtst-dev` | Trivial -- one-line fix |
| 4 | HIGH | GPU init panics with unhelpful message on systems without GPU | Medium -- replace 3 expect() with user-friendly errors |
| 5 | MEDIUM | Homebrew/winget manifests are untested placeholders | Medium -- automate in release workflow |
| 6 | MEDIUM | macOS DMG is unsigned/unnotarized (Gatekeeper blocks) | High -- requires Apple Developer account |
| 7 | MEDIUM | macOS DMG only for aarch64, no Intel build | Medium -- add CI matrix job |
| 8 | MEDIUM | No MSRV declared in Cargo.toml | Trivial -- add rust-version field |
| 9 | MEDIUM | "Binary is self-contained" README claim is false | Low -- either embed scripts or fix the text |
| 10 | MEDIUM | Config file not created on first run (discoverability) | Low-Medium |
| 11 | MEDIUM | No Scoop manifest for Windows | Low -- create JSON manifest |
| 12 | MEDIUM | No Fedora/Arch build dep docs | Low -- add equivalent package names |
| 13 | LOW | No `glass check` diagnostic subcommand | Medium |
| 14 | LOW | No CONTRIBUTING.md | Low |
| 15 | LOW | Linux desktop entry uses generic icon | Low |

**Recommendation for launch**: Fix items 1-4 before any public release. Items 5-9 should be addressed before broad promotion. Items 10-15 are nice-to-haves that improve polish.

**Highest-leverage single fix**: Embed shell integration scripts in the binary via `include_str!()` and write them to a temp directory at startup. This simultaneously fixes the critical packaging issue (#1), the silent failure (#2), and the misleading README claim (#9).

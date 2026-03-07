# Feature Research

**Domain:** Packaging, distribution, auto-update, config hot-reload, performance profiling, and documentation for a Rust GPU-accelerated terminal emulator
**Researched:** 2026-03-07
**Confidence:** MEDIUM-HIGH (patterns well-established across Alacritty/WezTerm/Kitty/Ghostty; specific tooling versions verified)

## Feature Landscape

### Table Stakes (Users Expect These)

Features users assume exist when downloading a terminal emulator for daily use. Missing these = "not ready for production."

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Platform-native installer (MSI on Windows) | Users expect double-click install, not manual binary placement | MEDIUM | cargo-wix generates MSI via WiX Toolset; supports code signing. Windows-primary so this is P0 |
| macOS .dmg with .app bundle | Standard macOS distribution; drag-to-Applications flow | MEDIUM | cargo-packager supports DMG output; needs Info.plist, icon.icns, code signing for Gatekeeper |
| Linux .deb package | Debian/Ubuntu is majority Linux desktop; apt install flow | LOW | cargo-packager or cargo-deb; straightforward for single binary |
| Linux AppImage | Distribution-agnostic portable binary | LOW | cargo-packager supports this; single-file, no root needed |
| Config validation with clear error messages | Users currently get silent fallback to defaults on bad TOML -- should report what went wrong | LOW | Already has `load_from_str` with `tracing::warn`; upgrade to structured error reporting to user (not just log) |
| Config hot-reload for visual settings | Alacritty does this -- it is the baseline expectation for modern terminals. Font size, colors should apply without restart | MEDIUM | Watch `~/.glass/config.toml` with notify 8.2 (already a dependency in glass_snapshot). Debounce, re-parse, send event to renderer |
| README with build instructions | Open source project needs at minimum: what it is, screenshots, how to build, how to install | LOW | Markdown in repo root; straightforward |
| GitHub Releases with binaries | Users expect downloadable binaries for each platform on the Releases page | LOW | CI already has 3-platform matrix; add artifact upload step |

### Differentiators (Competitive Advantage)

Features that set Glass apart from Alacritty/WezTerm/Kitty. Not required but valuable for positioning.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Auto-update with in-app notification | Alacritty has NO auto-update. WezTerm has NO auto-update. This is a genuine gap in the Rust terminal space. Users must manually check GitHub | MEDIUM | self_update crate (GitHub Releases backend). Check on startup, notify in status bar, user-initiated download. NOT silent background install |
| Config validation overlay | Instead of just logging bad config, show an in-terminal warning overlay like Alacritty does ("Config error: line 5: unknown key 'fontt_family'") | LOW | toml crate already gives line/column on parse error; surface in renderer as overlay text |
| Hot-reload for ALL settings (not just visual) | Alacritty only hot-reloads some settings (font, colors); shell, history, snapshot settings require restart. Glass can reload everything except shell path | MEDIUM | Config sections like history.max_output_capture_kb and snapshot.enabled can be pushed to running subsystems via channels |
| Documentation site with interactive examples | Most Rust terminals have man pages or GitHub wiki at best. A proper mdBook site with config reference, shell integration guide, MCP usage docs would stand out | MEDIUM | mdBook with GitHub Pages deployment. Glass has unique features (MCP, undo, pipes) that need proper docs |
| Winget/Homebrew/AUR package manager listings | Discoverable via standard package managers on each platform | LOW-MEDIUM | Winget: winget-releaser GitHub Action. Homebrew: custom tap with cask formula. AUR: PKGBUILD in separate repo |
| Performance profiling dashboard (internal) | Not user-facing, but enables the "performance optimization pass" goal. Instrument key paths with Tracy or flamegraph | MEDIUM | cargo-flamegraph for CPU. Tracy for GPU/wgpu render pass profiling. Results inform optimization, not shipped to users |
| Portable mode (config next to binary) | Power users want to carry Glass on USB or sync config in dotfiles without XDG/AppData paths | LOW | Check for config.toml adjacent to binary before falling back to ~/.glass/ |

### Anti-Features (Commonly Requested, Often Problematic)

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| Silent background auto-update | "Just keep it updated" | Security risk (unsigned binary replacement), breaks user trust, can interrupt active terminal sessions, Windows Defender flags silent self-modification | Check-and-notify only; user clicks "Download" to open GitHub Releases or runs `glass update` |
| Built-in package manager integration (apt/brew/winget update) | Seems convenient | Requires maintaining package manifests in 5+ ecosystems, each with different review cycles. One broken manifest blocks all platforms | Provide packages but let package managers handle their own update flow. Auto-update via GitHub Releases is orthogonal |
| Live theme marketplace / remote config | "Download themes from community" | Network calls from terminal emulator are a security concern. Cloud sync explicitly out of scope | Ship 2 built-in themes (dark/light). Config is a local file users can share via gists |
| Automatic crash reporting / telemetry | Useful for debugging | Violates "no telemetry" constraint. Trust issue for terminal emulators that see all user input | Structured panic handler that writes crash log to ~/.glass/crash.log. User can optionally share |
| Custom installer UI (wizard-style) | "Professional feel" | Massive platform-specific complexity for marginal UX gain. MSI/DMG already have standard UX | Standard platform installer UX. MSI with WiX gives Add/Remove Programs integration. DMG gives drag-to-Applications |
| Config GUI / settings panel | "I don't want to edit TOML" | Huge surface area, must stay in sync with config schema, not how terminal power users work | Excellent config reference docs + validation errors that point to exact line. `glass config --validate` CLI command |
| Flatpak/Snap packaging | Broader Linux reach | Sandbox restrictions break PTY spawn, shell integration injection, and file system access. Terminal emulators are notoriously difficult to sandbox correctly | .deb + AppImage + AUR cover 95% of Linux users. Document why Flatpak is not supported |

## Feature Dependencies

```
[GitHub Releases CI]
    |
    +--enables--> [MSI installer] --enables--> [Winget listing]
    +--enables--> [DMG installer] --enables--> [Homebrew cask]
    +--enables--> [deb/AppImage]  --enables--> [AUR package]
    |
    +--enables--> [Auto-update check] (needs release API to query)

[Config validation]
    +--requires--> [Structured error types from toml parse]
    +--enables--> [Config validation overlay in renderer]
    +--enables--> [Config hot-reload] (must validate before applying)

[Config hot-reload]
    +--requires--> [notify file watcher on config.toml]
    +--requires--> [Config validation] (reject bad reloads gracefully)
    +--requires--> [Event channel to renderer/subsystems]

[Performance profiling]
    +--independent-- (developer tool, no user-facing dependency)
    +--informs--> [Optimization pass]

[Documentation site]
    +--requires--> [Feature-complete config reference]
    +--enhanced-by--> [Installers exist] (can document install steps)
```

### Dependency Notes

- **Auto-update requires GitHub Releases CI:** The self_update crate queries the GitHub Releases API. CI must produce tagged releases with platform binaries first.
- **Config hot-reload requires config validation:** Cannot hot-reload without validating the new config first. A bad reload must show an error and keep the previous config, not crash or reset to defaults.
- **MSI enables Winget, DMG enables Homebrew:** Package manager listings reference installer URLs from GitHub Releases. Build installers first, then register with package managers.
- **Documentation enhanced by everything else:** Docs describe how to install, configure, and use. Write docs last (or in parallel with final features) so content is accurate.
- **Flatpak/Snap conflicts with terminal functionality:** PTY access, shell injection, and filesystem access are sandboxed. Explicitly an anti-feature.

## Milestone Scope Definition

This is v2.1 (not v1). The terminal is already functional and daily-drivable. The milestone is about polish, distribution, and documentation.

### v2.1 Launch With

These are the features needed to consider v2.1 complete.

- [ ] **MSI installer for Windows** -- primary platform, must be installable without cargo
- [ ] **DMG installer for macOS** -- second platform, must pass Gatekeeper
- [ ] **deb + AppImage for Linux** -- third platform, two formats cover most distros
- [ ] **GitHub Releases CI** -- automated binary upload on git tag
- [ ] **Config validation with user-visible errors** -- show parse errors in-terminal, not just tracing
- [ ] **Config hot-reload for visual settings** -- font_family, font_size at minimum, using notify watcher
- [ ] **Performance profiling pass** -- identify and fix bottlenecks, document baseline metrics
- [ ] **Documentation site** -- mdBook on GitHub Pages with install guide, config reference, feature docs
- [ ] **README overhaul** -- screenshots, feature summary, install instructions, links to docs site

### v2.1 Should-Have (if time permits)

- [ ] **Auto-update check** -- query GitHub Releases API on startup, show notification in status bar
- [ ] **Config hot-reload for ALL settings** -- history, snapshot, pipes sections reload without restart
- [ ] **`glass config --validate` CLI** -- validate config file from command line
- [ ] **Winget listing** -- submit manifest to winget-pkgs repository
- [ ] **Homebrew tap** -- create homebrew-glass tap with cask formula

### Future Consideration (v2.2+)

- [ ] **AUR package** -- community can maintain PKGBUILD
- [ ] **Portable mode** -- config adjacent to binary
- [ ] **Crash log handler** -- structured panic output to ~/.glass/crash.log
- [ ] **Code signing** -- MSI and DMG signing for trust/SmartScreen bypass

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| GitHub Releases CI | HIGH | LOW | P1 |
| MSI installer (Windows) | HIGH | MEDIUM | P1 |
| DMG installer (macOS) | HIGH | MEDIUM | P1 |
| deb + AppImage (Linux) | HIGH | LOW | P1 |
| Config validation errors to user | HIGH | LOW | P1 |
| Config hot-reload (visual) | HIGH | MEDIUM | P1 |
| README overhaul | HIGH | LOW | P1 |
| Documentation site (mdBook) | HIGH | MEDIUM | P1 |
| Performance profiling pass | MEDIUM | MEDIUM | P1 |
| Auto-update check | MEDIUM | MEDIUM | P2 |
| Full config hot-reload | MEDIUM | MEDIUM | P2 |
| `glass config --validate` | LOW | LOW | P2 |
| Winget listing | MEDIUM | LOW | P2 |
| Homebrew tap | MEDIUM | LOW | P2 |
| Code signing | MEDIUM | HIGH | P3 |
| Portable mode | LOW | LOW | P3 |

## Competitor Feature Analysis

| Feature | Alacritty | WezTerm | Kitty | Ghostty | Glass (planned) |
|---------|-----------|---------|-------|---------|-----------------|
| **Installer formats** | None (build from source or package manager) | .deb, .rpm, AppImage, Flatpak, homebrew | .dmg, .deb, homebrew, pip | .dmg, .deb, homebrew | MSI, DMG, deb, AppImage |
| **Auto-update** | None | None | None (relies on package manager) | Built-in (macOS Sparkle) | GitHub Releases check + notification |
| **Config format** | TOML (was YAML) | Lua | Plain text (custom) | Custom format | TOML |
| **Config hot-reload** | Yes (visual settings only, default on) | Yes (Lua re-evaluation) | Yes (partial, SIGUSR1) | Yes (partial) | Yes (planned: visual + subsystem settings) |
| **Config validation** | Error overlay in terminal | Lua error output | Error messages on load | Diagnostic output | Error overlay in terminal (planned) |
| **Documentation** | Man page + GitHub | Dedicated website (wezfurlong.org/wezterm) | Comprehensive website (sw.kovidgoyal.net/kitty) | ghostty.org | mdBook on GitHub Pages (planned) |
| **Package managers** | brew, apt, pacman, snap, scoop, winget | brew, apt, flatpak | brew, apt, pip | brew, apt | winget, brew (planned) |

### Key Observations

1. **Auto-update is a genuine gap** in the Rust terminal space. Only Ghostty (Zig, not Rust) has it via macOS Sparkle framework. Glass implementing auto-update via self_update crate would be a real differentiator.

2. **Config hot-reload is table stakes.** Every modern terminal does it. Alacritty set this expectation in 2018. Glass must support it.

3. **Documentation quality varies wildly.** Kitty and WezTerm have excellent dedicated sites. Alacritty has minimal docs. Glass has unique features (MCP server, command undo, pipe visualization) that genuinely need documentation to be discoverable.

4. **Installer packaging is solved.** cargo-packager or cargo-wix + platform-specific tools handle this. The hard part is CI automation, not the packaging itself.

5. **No competitor hot-reloads ALL config.** Everyone has "some settings require restart." Glass can aim higher by making most settings live-reloadable, since the architecture already uses event channels between subsystems.

## Implementation Complexity Notes

### Packaging (MEDIUM overall)
- **cargo-wix** for MSI: Well-documented, Windows-only tool. Needs WiX Toolset installed on CI runner. Supports code signing if certificate available.
- **cargo-packager** for DMG/deb/AppImage: Single tool handles multiple formats. v0.11.8 is latest. Maintained by CrabNebula team (Tauri ecosystem).
- **CI integration**: GitHub Actions matrix already exists for 3 platforms. Add artifact upload steps and release workflow on tag push.

### Config Hot-Reload (MEDIUM)
- **notify 8.2 already in workspace** (glass_snapshot dependency). Can reuse in glass_core.
- **Debouncing is critical**: Editors like vim write to temp file then rename. Must handle rename events, not just modify events. notify-debouncer-mini handles this.
- **Alacritty pattern**: Watch config file, debounce ~300ms, re-parse, diff against current config, apply only changed fields. If parse fails, show error overlay, keep old config.
- **Hot-reloadable fields**: font_family, font_size (trigger renderer font rebuild), shell (NOT hot-reloadable -- affects running PTY), history/snapshot/pipes settings (push to subsystems via channels).
- **Current config architecture**: `GlassConfig::load()` reads once at startup, returns owned struct. Must change to: load once, watch file, on change re-parse and send `ConfigChanged(GlassConfig)` event through existing `AppEvent` system.
- **Imported config limitation**: Alacritty has a known issue where changes to imported files don't trigger reload. Glass uses single file -- not an issue.

### Auto-Update (MEDIUM)
- **self_update crate**: Mature (v0.42.0), supports GitHub Releases backend. Downloads asset matching platform, replaces binary.
- **Pattern**: Check on startup (async, non-blocking). Compare semver. If newer release exists, show one-line notification in status bar. User runs `glass update` or clicks to download.
- **Windows complication**: Running binary cannot be replaced on Windows. self_update handles this with rename-and-replace pattern, but may need restart.
- **Security**: Verify SHA256 checksum of downloaded binary. GitHub Releases provides checksums.

### Performance Profiling (MEDIUM)
- **cargo-flamegraph**: CPU profiling. Works on all platforms. Generates SVG flamegraph.
- **Tracy**: GPU/wgpu profiling. wgpu has built-in Tracy integration behind a feature flag. Shows render pass timing, GPU memory.
- **Key paths to profile**: Cold startup (currently 360ms), key-to-screen latency (currently 3-7us), scrollback rendering with many blocks, FTS5 search performance with large history.
- **This is a developer activity**, not a user-facing feature. Output is "identified and fixed N bottlenecks" + documented baseline metrics.

### Documentation Site (MEDIUM)
- **mdBook**: Rust ecosystem standard. Used by The Rust Programming Language book. v0.6.2 current. Markdown input, static HTML output. Built-in search. GitHub Pages deployment.
- **Structure needed**: Installation guide (per platform), configuration reference (every TOML key documented), shell integration guide (4 shells), MCP server usage, command undo guide, pipe visualization guide, FAQ/troubleshooting.
- **CI deployment**: GitHub Action builds mdBook on push to main, deploys to GitHub Pages. Well-documented pattern in mdBook docs.

## Sources

- [cargo-wix - WiX MSI installer for Rust](https://github.com/volks73/cargo-wix) -- HIGH confidence, official repo
- [cargo-packager - Multi-format Rust packaging](https://github.com/crabnebula-dev/cargo-packager) -- HIGH confidence, official repo
- [cargo-packager PackageFormat docs](https://docs.rs/cargo-packager/latest/cargo_packager/enum.PackageFormat.html) -- HIGH confidence, docs.rs
- [self_update crate - Self-updating Rust executables](https://docs.rs/self_update/latest/self_update/) -- HIGH confidence, docs.rs
- [self_update GitHub repo](https://github.com/jaemk/self_update) -- HIGH confidence, official repo
- [Alacritty config reference (hot-reload docs)](https://alacritty.org/config-alacritty.html) -- HIGH confidence, official docs
- [Alacritty config watcher restart issue #7981](https://github.com/alacritty/alacritty/issues/7981) -- HIGH confidence, official issue tracker
- [mdBook documentation](https://rust-lang.github.io/mdBook/) -- HIGH confidence, official docs
- [mdBook CI deployment guide](https://rust-lang.github.io/mdBook/continuous-integration.html) -- HIGH confidence, official docs
- [notify crate - File system watcher](https://docs.rs/notify/) -- HIGH confidence, docs.rs
- [winget-releaser GitHub Action](https://github.com/vedantmgoyal9/winget-releaser) -- MEDIUM confidence, community tool
- [Homebrew tap documentation](https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap) -- HIGH confidence, official docs
- [Rust Performance Book - Profiling](https://nnethercote.github.io/perf-book/profiling.html) -- HIGH confidence, community reference
- [flamegraph-rs](https://github.com/flamegraph-rs/flamegraph) -- HIGH confidence, official repo
- [Tracy GPU profiling for wgpu](https://lib.rs/crates/tracy_full) -- MEDIUM confidence, lib.rs

---
*Feature research for: Glass v2.1 Packaging & Polish*
*Researched: 2026-03-07*

# Project Research Summary

**Project:** Glass v2.1 -- Packaging & Polish
**Domain:** Platform packaging, auto-update, config hot-reload, performance profiling, and documentation for a 12-crate Rust GPU-accelerated terminal emulator
**Researched:** 2026-03-07
**Confidence:** HIGH

## Executive Summary

Glass v2.1 is a polish milestone for an already functional and daily-drivable terminal emulator (17,868 LOC, 436 tests, 12 crates). The goal is distribution readiness: platform-native installers (MSI, DMG, deb), automatic update checking via GitHub Releases, config hot-reload (following the pattern Alacritty established as baseline expectation), a performance profiling pass with real benchmarks, and a documentation site. The existing architecture is well-suited for these additions -- the event-driven `AppEvent`/`EventLoopProxy` pattern already handles cross-thread communication, `notify` 8.2 is already a workspace dependency, and `tracing` spans are already throughout the codebase.

The recommended approach is: use `cargo-packager` as the primary cross-platform packaging tool (from the Tauri team, handles MSI/DMG/deb/AppImage), `self_update` 0.42 for GitHub Releases-based auto-update (no custom update server needed), reuse the existing `notify` watcher with debouncing for config hot-reload, and layer `tracing-flame` behind a cargo feature flag for profiling. Only 2 new runtime crates are needed (`self_update`, `notify-debouncer-mini`); everything else is dev tooling or CI-only CLI tools. mdBook handles the documentation site, deployed to GitHub Pages.

The key risks are: (1) config hot-reload causing font rebuild cascades without proper debouncing and diff-based application -- editors generate 2-5 filesystem events per save, and rebuilding glyphon's FontSystem on each one causes flicker and potential panics; (2) MSI UpgradeCode must be hardcoded and committed from the very first release or all future Windows upgrades break irreparably; (3) macOS Gatekeeper blocks unsigned DMGs downloaded from the internet, requiring an Apple Developer account and CI-integrated code signing; (4) Windows file locking prevents replacing the running executable, requiring MSI-based upgrade flow rather than direct binary replacement. All of these are well-understood problems with documented solutions.

## Key Findings

### Recommended Stack

The v2.1 stack adds minimal new runtime dependencies to the existing validated 12-crate workspace. See [STACK.md](STACK.md) for full details.

**Core technologies:**
- **cargo-packager 0.11.8** (CLI, not runtime): Cross-platform installer generation (MSI, DMG, deb, AppImage) from Cargo.toml metadata. From CrabNebula/Tauri team.
- **self_update 0.42.0** (runtime): GitHub Releases-based binary self-update. Handles platform detection, archive extraction, binary replacement, version comparison. Dramatically simpler than hosting a custom update server.
- **notify 8.2 + notify-debouncer-mini 0.7** (runtime): Config file watching with proper debouncing. notify is already in the workspace; only the debouncer is new.
- **tracing-chrome 0.7.2** (optional, dev): Chrome/Perfetto trace output from existing tracing spans. Zero overhead when feature-disabled.
- **criterion 0.5** (dev-only): Statistical microbenchmarks for cold start, key latency, FTS5 queries.
- **mdBook 0.5.2** (CLI): Rust ecosystem standard for documentation sites. GitHub Pages deployment.
- **cargo-generate-rpm 0.20.0** (CLI): RPM generation (cargo-packager does not cover RPM).

**Total new runtime crates: 2.** Binary size impact: ~200-220 KB (mostly from reqwest in self_update).

### Expected Features

See [FEATURES.md](FEATURES.md) for full feature landscape and competitor analysis.

**Must have (table stakes):**
- Platform-native installers: MSI (Windows), DMG (macOS), deb + AppImage (Linux)
- GitHub Releases CI with automated binary upload on tag push
- Config validation with user-visible error messages (not just tracing logs)
- Config hot-reload for visual settings (font_family, font_size) -- Alacritty set this expectation in 2018
- README overhaul with screenshots and install instructions
- Documentation site (mdBook on GitHub Pages)
- Performance profiling pass with documented baseline metrics

**Should have (differentiators):**
- Auto-update check from GitHub Releases -- genuine gap in the Rust terminal space (Alacritty, WezTerm have none)
- Hot-reload for ALL settings (history, snapshot, pipes) -- no competitor does this fully
- `glass config --validate` CLI command
- Winget and Homebrew listings

**Defer (v2.2+):**
- AUR package, portable mode, crash log handler
- Code signing (MSI + DMG) -- budget Apple Developer account, defer implementation
- Flatpak/Snap (sandbox breaks PTY access -- explicitly an anti-feature for terminals)

### Architecture Approach

The v2.1 features integrate into the existing event-driven architecture with minimal structural change. See [ARCHITECTURE.md](ARCHITECTURE.md) for component diagrams and data flows.

**Major components:**
1. **Config hot-reload** (modify `glass_core/config.rs`) -- ConfigWatcher thread using notify, sends `AppEvent::ConfigChanged` through EventLoopProxy, diff-based application to avoid unnecessary font rebuilds
2. **Auto-update** (new crate `glass_update`) -- UpdateChecker backed by self_update, background tokio check on startup, `AppEvent::UpdateAvailable` for status bar notification, `glass update` CLI subcommand
3. **Packaging** (CI/build infrastructure only) -- `release.yml` workflow triggered by `v*` tags, `packaging/` directory with WiX XML, Info.plist, .desktop files
4. **Performance profiling** (scattered instrumentation) -- `#[tracing::instrument]` on hot paths, `tracing-flame` behind `perf` cargo feature, criterion benchmarks in dev-dependencies

**Key architectural patterns to follow:**
- Event-driven config propagation (extend existing AppEvent pattern)
- Feature-gated optional components (profiling behind cargo features)
- Background check with user-triggered action (updates)
- Diff-then-apply for config changes (avoid rebuilding FontSystem when only history thresholds change)

### Critical Pitfalls

See [PITFALLS.md](PITFALLS.md) for full pitfall analysis with recovery strategies.

1. **Config reload cascade without debouncing** -- Editors generate 2-5 FS events per save. Without debouncing, each triggers a FontSystem rebuild causing flicker and panics. Fix: 300-500ms debounce window + diff old vs new config to skip font rebuild when only non-visual fields changed.
2. **MSI UpgradeCode not locked from day one** -- Windows Installer uses this GUID to detect previous installations. If it changes between versions, users get duplicate entries in Add/Remove Programs with no retroactive fix. Fix: Hardcode GUID in `wix/main.wxs`, commit to git, never change it.
3. **Windows file locking prevents binary replacement** -- Running `.exe` holds a file lock. Direct overwrite fails. Fix: Use MSI upgrade path on Windows (download MSI, launch msiexec, exit Glass). Only use direct binary replacement on Linux standalone.
4. **macOS Gatekeeper blocks unsigned DMGs** -- Downloaded unsigned apps show "damaged" error. Fix: Apple Developer account + rcodesign in CI for signing and notarization alongside DMG creation.
5. **Font change only updates focused pane** -- Glass has tabs with split panes. Naive implementation updates only the active pane, leaving others with stale font metrics. Fix: Iterate ALL sessions in SessionMux, resize every pane's terminal grid, send resize to every PTY.

## Implications for Roadmap

Based on research, suggested phase structure:

### Phase 1: Performance Profiling Instrumentation
**Rationale:** Must establish baselines BEFORE any optimization work. Zero new crates -- just adding tracing spans and an optional subscriber. Lowest risk, highest information value.
**Delivers:** Flamegraph capability, criterion benchmark suite, documented baseline metrics (cold start, key latency, PTY throughput, FTS5 query time)
**Addresses:** Performance profiling pass (P1 feature)
**Uses:** tracing (existing), tracing-flame (optional dep), criterion (dev dep), cargo-flamegraph (CLI)
**Avoids:** "Optimizing the wrong thing" pitfall -- profiling without baselines

### Phase 2: Config Validation and Hot-Reload
**Rationale:** Modifies `glass_core` which is a shared dependency -- do it before adding new crates. Config validation is prerequisite for safe hot-reload. High user value (table stakes feature).
**Delivers:** Structured config validation errors shown to user, file watcher with debouncing, diff-based config application, font rebuild across all panes
**Addresses:** Config validation (P1), config hot-reload for visual settings (P1), full config hot-reload (P2)
**Uses:** notify 8.2 (existing), notify-debouncer-mini 0.7 (new), existing AppEvent/EventLoopProxy
**Avoids:** Reload cascade pitfall, torn reads pitfall, multi-pane miss pitfall

### Phase 3: Packaging and CI Release Workflow
**Rationale:** Pure infrastructure -- no runtime code changes. Must exist before auto-update (which queries GitHub Releases). Unblocks distribution.
**Delivers:** MSI installer, DMG bundle, deb package, tar.gz archives, automated release workflow on git tag
**Addresses:** Platform installers (P1), GitHub Releases CI (P1)
**Uses:** cargo-packager 0.11.8, cargo-wix 0.3.9 (fallback), cargo-deb 3.6.3 (fallback), cargo-generate-rpm 0.20.0
**Avoids:** UpgradeCode pitfall (lock GUID immediately), macOS notarization pitfall (sign alongside DMG creation)

### Phase 4: Auto-Update Mechanism
**Rationale:** Depends on Phase 3 (needs GitHub Releases with downloadable artifacts). New crate `glass_update`. Genuine competitive differentiator.
**Delivers:** `glass update` CLI command, background version check on startup, status bar notification, install-method-aware update flow
**Addresses:** Auto-update check (P2 feature, but high differentiator value)
**Uses:** self_update 0.42.0 (new runtime dep), existing tokio runtime, existing AppEvent system
**Avoids:** Windows file locking pitfall (MSI upgrade path), startup blocking pitfall (async check with 24hr cooldown)

### Phase 5: Documentation and README
**Rationale:** Documents the finished product. Can partially overlap with Phase 4. Content is most accurate when features are complete.
**Delivers:** mdBook site on GitHub Pages (install guide, config reference, shell integration, MCP docs), README overhaul with screenshots
**Addresses:** Documentation site (P1), README overhaul (P1)
**Uses:** mdBook 0.5.2, GitHub Pages deployment

### Phase 6: Package Manager Listings (Optional)
**Rationale:** Depends on Phase 3 (installer URLs from GitHub Releases). Low effort, medium value. Can be deferred.
**Delivers:** Winget manifest, Homebrew tap
**Addresses:** Winget listing (P2), Homebrew tap (P2)

### Phase Ordering Rationale

- **Profiling before everything:** Data-driven decisions for the rest of the milestone. If profiling reveals a critical bottleneck, it informs architecture decisions in subsequent phases.
- **Config hot-reload before packaging:** Modifies glass_core (shared dependency). Better to stabilize internal changes before adding external distribution infrastructure.
- **Packaging before auto-update:** Auto-update queries GitHub Releases for artifacts that packaging produces. Hard dependency.
- **Docs last:** Content accuracy requires features to be complete. Shell integration docs need installer paths. Config reference needs hot-reload behavior documented.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 2 (Config Hot-Reload):** Multi-pane propagation is complex -- need to trace all config access sites (15+ in main.rs) and ensure each handles runtime changes. The renderer font rebuild path needs careful design.
- **Phase 3 (Packaging):** WiX template customization, macOS code signing secrets in CI, and cross-platform CI matrix each have platform-specific gotchas. Test MSI upgrade flow end-to-end.
- **Phase 4 (Auto-Update):** Windows-specific binary replacement strategy, install-method detection logic, and GitHub API rate limiting (60/hr unauthenticated) need detailed design.

Phases with standard patterns (skip research-phase):
- **Phase 1 (Profiling):** tracing + tracing-flame + criterion is a thoroughly documented pattern. Just add spans and benchmarks.
- **Phase 5 (Documentation):** mdBook + GitHub Pages is boilerplate. Content writing, not technical research.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | All crate versions verified on crates.io. Minimal new runtime dependencies (2 crates). Existing workspace deps reused. |
| Features | MEDIUM-HIGH | Feature landscape well-mapped against Alacritty/WezTerm/Kitty/Ghostty. Auto-update as differentiator is validated (no Rust terminal has it). |
| Architecture | HIGH | Extends existing event-driven patterns. No fundamental architecture changes. Build order dependencies are clear. |
| Pitfalls | HIGH | Based on codebase analysis (specific line numbers in main.rs/config.rs), platform behavior (Windows file locking, macOS Gatekeeper), and ecosystem experience. |

**Overall confidence:** HIGH

### Gaps to Address

- **wgpu-profiler compatibility with wgpu 28.0:** Need to verify before committing to GPU-specific profiling. If incompatible, CPU profiling via tracing-flame is sufficient for v2.1.
- **macOS code signing budget:** Requires $99/year Apple Developer account. Decision needed on whether to defer (ship with "xattr -d" workaround instructions) or invest now.
- **Windows code signing:** Unsigned MSI triggers SmartScreen warnings. SSL.com eSigner is an option but adds cost and CI complexity. Acceptable to defer.
- **GitHub API rate limiting for update checks:** 60 requests/hr unauthenticated. Need a cooldown cache strategy (check at most once per 24hrs, cache result to disk).
- **ScaleFactorChanged handler:** Currently log-only (known tech debt). Config hot-reload implementation should address DPI-change font recalculation as part of the font rebuild path.

## Sources

### Primary (HIGH confidence)
- [cargo-packager (crates.io)](https://crates.io/crates/cargo-packager) -- v0.11.8, multi-format installer generation
- [self_update (GitHub)](https://github.com/jaemk/self_update) -- v0.42.0, GitHub Releases auto-update backend
- [notify (crates.io)](https://crates.io/crates/notify) -- v8.2.0, already in Glass workspace
- [notify-debouncer-mini (crates.io)](https://crates.io/crates/notify-debouncer-mini) -- v0.7.0, debounced file watching
- [tracing-chrome (crates.io)](https://crates.io/crates/tracing-chrome) -- v0.7.2, Chrome/Perfetto trace output
- [mdBook (crates.io)](https://crates.io/crates/mdbook) -- v0.5.2, documentation site generator
- [cargo-wix (GitHub)](https://github.com/volks73/cargo-wix) -- v0.3.9, MSI generation
- [cargo-deb (crates.io)](https://crates.io/crates/cargo-deb) -- v3.6.3, Debian package generation
- [cargo-generate-rpm (crates.io)](https://crates.io/crates/cargo-generate-rpm) -- v0.20.0, RPM generation
- [Microsoft MSI UpgradeCode docs](https://learn.microsoft.com/en-us/windows/win32/msi/changing-the-product-code) -- MSI upgrade semantics
- [Alacritty (GitHub)](https://github.com/alacritty/alacritty) -- Reference terminal for packaging and config hot-reload patterns
- Glass codebase: `crates/glass_core/src/config.rs`, `src/main.rs`, `crates/glass_core/src/event.rs`

### Secondary (MEDIUM confidence)
- [apple-codesign (rcodesign)](https://gregoryszorc.com/blog/2022/08/08/achieving-a-completely-open-source-implementation-of-apple-code-signing-and-notarization/) -- Pure-Rust macOS code signing
- [wgpu-profiler (docs.rs)](https://docs.rs/wgpu-profiler) -- GPU timing for wgpu (compatibility with wgpu 28.0 unverified)
- [winget-releaser (GitHub)](https://github.com/vedantmgoyal9/winget-releaser) -- Winget manifest automation
- [Flatpak Rust packaging](https://belmoussaoui.com/blog/8-how-to-flatpak-a-rust-application/) -- Manual manifest approach (not recommended for terminals)

---
*Research completed: 2026-03-07*
*Ready for roadmap: yes*

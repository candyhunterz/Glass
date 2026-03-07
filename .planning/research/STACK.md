# Stack Research: v2.1 Packaging & Polish

**Project:** Glass v2.1 -- Platform Installers, Auto-Update, Config Hot-Reload, Profiling, Docs
**Researched:** 2026-03-07
**Confidence:** HIGH (well-established Rust packaging ecosystem, verified versions via crates.io)

## Scope

This document covers ONLY what is needed for v2.1. The existing validated stack (12 crates, 17,868 LOC, 436 tests) is unchanged. This research addresses five new capability areas:

1. Platform installers (MSI, DMG, deb, rpm, AppImage, Flatpak)
2. Auto-update mechanism
3. Config hot-reload
4. Performance profiling tooling
5. Documentation site

---

## Recommended Stack

### Packaging & Distribution (Build-Time Tools -- NOT Runtime Dependencies)

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| cargo-packager (CLI) | 0.11.8 | Cross-platform installer generation | Single tool generates MSI (WiX), DMG, deb, AppImage, NSIS. From CrabNebula (Tauri team). Configured via `[package.metadata.packager]` in Cargo.toml -- no separate config file needed. Supports code signing. |
| cargo-wix (CLI) | 0.3.9 | MSI-specific generation (fallback) | More mature MSI tooling than cargo-packager's WiX support. Use only if cargo-packager's MSI output is insufficient. |
| cargo-deb (CLI) | 3.6.3 | Debian package generation (fallback) | Most mature deb packager in Rust ecosystem. Use if cargo-packager's deb output needs customization beyond what it supports. |
| cargo-generate-rpm (CLI) | 0.20.0 | RPM package generation | cargo-packager does not generate RPM. This is the standard Rust RPM generator. |

**Key decision: Use cargo-packager as primary, with per-format fallbacks.**

cargo-packager handles MSI + DMG + deb + AppImage in one tool. For RPM, add cargo-generate-rpm. For Flatpak, use flatpak-builder directly (no Rust crate needed -- it's a manifest + build system, not a library).

**These are all `cargo install` CLI tools, NOT Cargo.toml dependencies.** They run in CI and are invoked as build steps, not compiled into Glass.

### Auto-Update (Runtime Dependency)

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| self_update | 0.42.0 | In-place binary self-update from GitHub Releases | Most mature Rust self-update crate (0.42.0, actively maintained). Supports GitHub Releases backend natively. Handles: version comparison, asset download by platform/target triple, binary replacement, optional progress callbacks. 2.5M+ downloads. |

**Why self_update over cargo-packager-updater:**

cargo-packager-updater (0.2.3) is designed to work with cargo-packager's signing infrastructure and update server. It requires hosting your own update endpoint that serves signed update manifests. self_update works directly with GitHub Releases -- just upload platform-specific archives to a release and self_update finds and downloads the right one by naming convention.

For a project distributing via GitHub Releases, self_update is dramatically simpler. No update server, no signing infrastructure, no custom manifest format. Just: tag a release, upload binaries, self_update handles the rest.

**Integration point:** Add a `glass update` CLI subcommand (via existing clap infrastructure) that checks GitHub Releases for a newer version and replaces the binary in-place.

### Config Hot-Reload (Runtime Dependency)

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| notify | 8.2.0 | File system watching for config.toml changes | **Already in the project** (glass_snapshot uses it for FS watching). Reuse the same dependency. Provides cross-platform file watching: ReadDirectoryChangesW (Windows), FSEvents (macOS), inotify (Linux). |
| notify-debouncer-mini | 0.7.0 | Debounced file watch events | Prevents multiple reloads from a single save operation. Text editors often write-rename-delete which triggers 2-4 events. Debouncer collapses these into one event. Built on top of notify, same maintainers. |

**No new watcher crate needed.** notify 8.2.0 is already a workspace dependency. The only addition is notify-debouncer-mini for debouncing (the snapshot watcher does its own ad-hoc debouncing -- config watching should use the proper debouncer).

**Integration pattern:**
1. Watch `~/.glass/config.toml` via notify-debouncer-mini (100ms debounce)
2. On change: re-parse TOML, validate, diff against current config
3. Apply hot-reloadable fields (font_family, font_size, colors) via `AppEvent::ConfigChanged`
4. Non-hot-reloadable fields (shell) log a warning: "restart required"
5. Invalid config: log error, keep current config (never crash on bad config)

**What is hot-reloadable vs not:**

| Field | Hot-Reloadable | Why |
|-------|---------------|-----|
| font_family | YES | Rebuild glyphon font system, recompute metrics |
| font_size | YES | Rebuild glyphon font system, recompute metrics |
| shell | NO | Requires new PTY process -- restart needed |
| history.* | YES | Runtime parameters, no structural change |
| snapshot.enabled | YES | Gate check is per-command |
| snapshot.max_count | YES | Pruner reads on next run |
| pipes.enabled | YES | Gate check is per-command |

### Performance Profiling (Dev Dependencies Only)

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| tracing-chrome | 0.7.2 | Generate chrome://tracing JSON for frame-level profiling | Glass already uses tracing 0.1.44 and tracing-subscriber 0.3. tracing-chrome is a Layer that outputs to Chrome's trace format. Open in chrome://tracing or Perfetto for per-frame GPU/CPU visualization. Zero production overhead when not enabled. |
| criterion | 0.5 | Statistical microbenchmarks | Standard Rust benchmarking framework. Use for cold-start time, key-to-screen latency, config parse time, FTS5 query time. Handles warmup, statistical analysis, regression detection. |
| cargo-flamegraph (CLI) | 0.6 | CPU flame graphs | `cargo install flamegraph`. Wraps perf (Linux) / DTrace (macOS) / ETW (Windows via cargo-xctrace). Visualize where CPU time is spent during rendering, PTY I/O, etc. |
| memory-stats | 1.2 | Runtime memory reporting | **Already in the project.** Used for idle memory measurement. Continue using for profiling pass. |

**Profiling is dev-tooling, not shipped to users.** tracing-chrome should be behind a cargo feature flag (`profiling`) so it adds zero overhead in release builds. criterion goes in `[dev-dependencies]`.

**Integration pattern for tracing-chrome:**
```toml
[features]
profiling = ["tracing-chrome"]

[dependencies]
tracing-chrome = { version = "0.7.2", optional = true }
```

```rust
#[cfg(feature = "profiling")]
let _guard = {
    let (chrome_layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
        .file("glass_trace.json")
        .build();
    tracing_subscriber::registry().with(chrome_layer).init();
    guard
};
```

### Documentation Site (External Tooling)

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| mdBook | 0.5.2 | Documentation site from Markdown | Official Rust documentation tool. Used by The Rust Programming Language book, rustc docs, wasm-bindgen docs. Generates static HTML from Markdown with search, theming, and sidebar navigation. Deployed to GitHub Pages with zero config. |

**Why mdBook over alternatives:**

| Alternative | Why Not |
|-------------|---------|
| Docusaurus | JavaScript/React -- wrong ecosystem for a Rust project. Heavyweight. |
| Hugo/Zola | General-purpose static site generators. mdBook is purpose-built for technical documentation with code blocks, admonitions, and search. |
| rustdoc | For API docs (already generated by `cargo doc`). mdBook is for user-facing documentation: installation, configuration, keybindings, shell integration. |

**Structure:**
```
docs/
  book.toml          # mdBook config
  src/
    SUMMARY.md        # Table of contents
    installation.md   # Platform-specific install instructions
    configuration.md  # config.toml reference
    keybindings.md    # Keyboard shortcuts
    shell-integration.md  # bash/zsh/fish/pwsh setup
    history.md        # History & search features
    snapshots.md      # Undo/snapshot features
    mcp.md            # MCP server for AI integration
    tabs-panes.md     # Tabs and split panes
```

**CI integration:** `mdbook build` in GitHub Actions, deploy to GitHub Pages on release tags.

---

## New Cargo.toml Dependencies (Runtime)

```toml
[workspace.dependencies]
# Auto-update
self_update = { version = "0.42", default-features = false, features = ["archive-tar", "archive-zip", "compression-flate2", "backends-github"] }

# Config hot-reload debouncing
notify-debouncer-mini = "0.7"

# Performance profiling (optional)
tracing-chrome = { version = "0.7.2", optional = true }
```

```toml
# In glass_core or root binary Cargo.toml
[dependencies]
self_update = { workspace = true }
notify-debouncer-mini = { workspace = true }
# notify already available via workspace

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[features]
profiling = ["tracing-chrome"]
```

**Total new runtime crates: 2** (self_update, notify-debouncer-mini). Everything else is dev tooling or CLI tools installed separately.

---

## CI/CD Pipeline Additions

### Release Workflow (new: `.github/workflows/release.yml`)

```yaml
# Trigger: push tag v*
# Matrix: windows-latest, macos-latest, ubuntu-latest
# Steps per platform:
#   1. cargo build --release
#   2. Platform-specific packaging:
#      - Windows: cargo packager --formats msi,nsis
#      - macOS: cargo packager --formats dmg,app
#      - Linux: cargo packager --formats deb,appimage
#      - Linux: cargo generate-rpm
#   3. Upload artifacts to GitHub Release
```

### Documentation Workflow (new: `.github/workflows/docs.yml`)

```yaml
# Trigger: push to main (docs/** changed)
# Steps:
#   1. mdbook build docs/
#   2. Deploy to GitHub Pages
```

### Existing CI (unchanged)

The existing `ci.yml` continues to run `cargo build --release` and `cargo test` on all three platforms. No changes needed.

---

## What NOT to Add

| Temptation | Why Not |
|------------|---------|
| **cargo-packager-updater** (0.2.3) | Requires hosting an update server with signed manifests. Overkill for GitHub Releases distribution. self_update works directly with GitHub Releases. |
| **Tauri** | Glass is not a webview app. Tauri's packaging is tightly coupled to its webview + IPC model. cargo-packager (extracted from Tauri) gives packaging without the webview baggage. |
| **Squirrel/Sparkle** | Native update frameworks for Windows/macOS. Would require FFI bindings, platform-specific code, and a separate update server. self_update is pure Rust and cross-platform. |
| **config-rs crate** | Over-engineered for Glass's needs. Glass loads a single TOML file. serde + toml (already in the project) handles deserialization. Adding config-rs introduces layered config, environment variable overrides, and 12-factor patterns that add complexity without value. |
| **hot-lib-reloader** | For hot-reloading Rust code (dylib swapping). Glass needs config hot-reload, not code hot-reload. Totally different problem. |
| **pprof-rs** | Linux-only CPU profiler. cargo-flamegraph + tracing-chrome cover all platforms. |
| **Tracy profiler** | Excellent but requires building a separate C++ viewer application and linking a C library. tracing-chrome outputs to Perfetto (browser-based) which is zero-install. |
| **Snap packaging** | Snap's confinement model restricts filesystem and PTY access. Terminal emulators notoriously break under Snap confinement. Flatpak or AppImage are better choices for sandboxed Linux distribution. |

---

## Alternatives Considered

| Category | Recommended | Alternative | Why Not Alternative |
|----------|-------------|-------------|---------------------|
| Cross-platform packaging | cargo-packager 0.11.8 | cargo-bundle 0.6 | cargo-bundle is unmaintained (last release 2021). cargo-packager is its spiritual successor from the Tauri team. |
| MSI generation | cargo-packager (primary) + cargo-wix (fallback) | WiX directly | cargo-packager wraps WiX. Only fall back to cargo-wix if advanced MSI customization is needed (custom dialog UI, merge modules). |
| RPM generation | cargo-generate-rpm 0.20.0 | rpmbuild directly | cargo-generate-rpm reads metadata from Cargo.toml. rpmbuild requires writing a separate .spec file. |
| Auto-update | self_update 0.42.0 | Custom reqwest + GitHub API | self_update handles: platform detection, archive extraction, binary replacement, version comparison. Reimplementing this is 500+ LOC of error-prone code. |
| Config watching | notify 8.2 + debouncer-mini 0.7 | Polling loop | Polling wastes CPU. notify uses OS-native file watching (inotify/FSEvents/ReadDirectoryChangesW) -- zero CPU when idle. |
| Profiling | tracing-chrome 0.7.2 | Tracy | Tracy is more powerful but requires C++ viewer and native library linking. tracing-chrome outputs JSON for Perfetto (browser). Good enough for an optimization pass; switch to Tracy only if deeper GPU analysis is needed. |
| Docs site | mdBook 0.5.2 | Zola | Zola is a general static site generator. mdBook is purpose-built for documentation with search, code highlighting, and sidebar TOC out of the box. Less config, better fit. |

---

## Version Compatibility

| Package | Compatible With | Notes |
|---------|-----------------|-------|
| self_update 0.42.0 | reqwest (async HTTP), zip/tar/flate2 | Pulls in reqwest as a dependency. Uses tokio runtime (already in project). |
| notify-debouncer-mini 0.7.0 | notify 8.x | Built on top of notify. Must use compatible notify version -- 8.2.0 matches. |
| tracing-chrome 0.7.2 | tracing 0.1.x, tracing-subscriber 0.3.x | Compatible with existing tracing stack. Implements tracing_subscriber::Layer. |
| criterion 0.5 | Stable Rust | No compatibility concerns. Dev-dependency only. |
| cargo-packager 0.11.8 | WiX Toolset v3/v4 (Windows), create-dmg (macOS) | CLI tool. Requires WiX installed on Windows CI runner (`choco install wixtoolset`). |
| cargo-generate-rpm 0.20.0 | RPM 4.x | CLI tool. Requires rpmbuild on Linux CI runner. |
| mdBook 0.5.2 | Stable Rust | CLI tool. `cargo install mdbook` or download pre-built binary. |

---

## Compile & Dependency Impact

| Addition | New Transitive Deps | Compile Impact | Binary Size | Notes |
|----------|---------------------|----------------|-------------|-------|
| self_update | reqwest, hyper, rustls, zip, tar, flate2 | MODERATE (~15s) | ~200 KB | reqwest is the heavy one. Consider feature-gating behind `update` feature if cold compile time matters. |
| notify-debouncer-mini | None (notify already present) | MINIMAL (~1s) | ~5 KB | Thin wrapper over notify. |
| tracing-chrome (optional) | None | MINIMAL (~2s) | ~10 KB | Only compiled with `--features profiling`. |
| criterion (dev-only) | plotters, statistical libs | MODERATE (~10s) | N/A | Dev-dependency only, not in release binary. |
| **Total runtime addition** | ~30 new transitive crates (mostly from reqwest) | ~15-20s first compile | ~200-220 KB | Acceptable for auto-update capability. |

**Mitigation for reqwest bloat:** Use `default-features = false` on self_update to avoid pulling in unnecessary backends. Only enable `backends-github`, `archive-tar`, `archive-zip`, and `compression-flate2`.

---

## Integration Points with Existing Architecture

### Auto-Update Integration

```
glass update (clap subcommand)
  -> self_update::backends::github::Update::configure()
  -> Check latest GitHub Release tag vs current version
  -> Download platform-specific archive (glass-v{version}-{target}.tar.gz)
  -> Replace binary in-place
  -> Print "Updated to vX.Y.Z. Restart Glass to use new version."
```

**Version embedding:** Use `env!("CARGO_PKG_VERSION")` (already available) for current version comparison.

**Asset naming convention:** `glass-v{version}-x86_64-pc-windows-msvc.zip`, `glass-v{version}-aarch64-apple-darwin.tar.gz`, `glass-v{version}-x86_64-unknown-linux-gnu.tar.gz`

### Config Hot-Reload Integration

```
App::new()
  -> Spawn config watcher thread (notify-debouncer-mini)
  -> Watch ~/.glass/config.toml
  -> On change: parse + validate + diff
  -> Send AppEvent::ConfigChanged(ConfigDelta) via EventProxy
  -> App::handle_event matches ConfigChanged:
     -> Update font system (glyphon rebuild)
     -> Update renderer parameters
     -> Log non-hot-reloadable changes
```

**Existing event infrastructure:** Glass already has `AppEvent` enum and `EventProxy` for cross-thread communication. Config changes slot into this pattern naturally.

### Profiling Integration

```
GLASS_PROFILE=1 glass
  -> Activates tracing-chrome layer
  -> Writes glass_trace.json on exit
  -> Open in chrome://tracing or Perfetto
```

**Existing tracing infrastructure:** Glass already uses `tracing::info!`, `tracing::debug!` etc. throughout the codebase. tracing-chrome captures these spans automatically -- no instrumentation code changes needed. Add `#[tracing::instrument]` to hot-path functions (render loop, PTY read, event dispatch) for detailed breakdown.

---

## Sources

- [cargo-packager (crates.io)](https://crates.io/crates/cargo-packager) -- v0.11.8 verified, format support confirmed (HIGH confidence)
- [cargo-packager (GitHub)](https://github.com/crabnebula-dev/cargo-packager) -- README documents MSI, DMG, deb, AppImage, NSIS support (HIGH confidence)
- [self_update (crates.io)](https://crates.io/crates/self_update) -- v0.42.0 verified (HIGH confidence)
- [self_update (GitHub)](https://github.com/jaemk/self_update) -- GitHub Releases backend documented (HIGH confidence)
- [cargo-packager-updater (crates.io)](https://crates.io/crates/cargo-packager-updater) -- v0.2.3, requires update server (HIGH confidence)
- [notify-debouncer-mini (crates.io)](https://crates.io/crates/notify-debouncer-mini) -- v0.7.0, compatible with notify 8.x (HIGH confidence)
- [notify (crates.io)](https://crates.io/crates/notify) -- v8.2.0 latest stable (HIGH confidence)
- [tracing-chrome (crates.io)](https://crates.io/crates/tracing-chrome) -- v0.7.2 verified (HIGH confidence)
- [mdBook (crates.io)](https://crates.io/crates/mdbook) -- v0.5.2 verified (HIGH confidence)
- [cargo-deb (crates.io)](https://crates.io/crates/cargo-deb) -- v3.6.3, fallback for deb (HIGH confidence)
- [cargo-generate-rpm (crates.io)](https://crates.io/crates/cargo-generate-rpm) -- v0.20.0, needed for RPM format (HIGH confidence)
- [cargo-wix (crates.io)](https://crates.io/crates/cargo-wix) -- v0.3.9, MSI fallback (HIGH confidence)
- [Flatpak Rust packaging (belmoussaoui.com)](https://belmoussaoui.com/blog/8-how-to-flatpak-a-rust-application/) -- Manual manifest approach documented (MEDIUM confidence)

---
*Stack research for: Glass v2.1 Packaging & Polish*
*Researched: 2026-03-07*

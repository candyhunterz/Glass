# Architecture Research

**Domain:** Packaging, distribution, auto-update, config hot-reload, and performance profiling for a 12-crate Rust terminal emulator
**Researched:** 2026-03-07
**Confidence:** HIGH

## System Overview

Current 12-crate architecture with new v2.1 components marked:

```
                         ┌─────────────────────────────────────────┐
                         │              glass (binary)             │
                         │  main.rs -- Processor / ApplicationHandler  │
                         │  Owns: EventLoop, WindowContext, config │
                         └────┬────────────┬──────────┬────────────┘
                              │            │          │
         ┌────────────────────┤            │          ├────────────────┐
         │                    │            │          │                │
    ┌────▼────┐         ┌────▼────┐  ┌────▼────┐  ┌──▼───────┐  ┌────▼────┐
    │glass_mux│         │glass_   │  │glass_   │  │glass_    │  │glass_   │
    │         │         │terminal │  │renderer │  │history   │  │snapshot │
    └────┬────┘         └─────────┘  └─────────┘  └──────────┘  └─────────┘
         │
    ┌────▼────┐         ┌─────────┐  ┌─────────┐  ┌──────────┐
    │glass_   │         │glass_   │  │glass_   │  │glass_    │
    │core     │         │protocol │  │pipes    │  │mcp       │
    │(config) │         └─────────┘  └─────────┘  └──────────┘
    └─────────┘

    NEW COMPONENTS (v2.1):

    ┌──────────────┐
    │ glass_update │    (new crate -- auto-update via GitHub Releases)
    └──────────────┘

    MODIFIED COMPONENTS:
    - glass_core/config.rs   -- hot-reload watcher, validation, ConfigDiff
    - glass_core/event.rs    -- AppEvent::ConfigChanged, AppEvent::UpdateAvailable
    - glass (binary)/main.rs -- handle ConfigChanged, startup update check, profiling
    - glass_renderer         -- accept font changes at runtime (rebuild font system)
    - CI (.github/workflows) -- release workflow producing platform installers
```

## New vs Modified Components

| Component | Status | What Changes |
|-----------|--------|-------------|
| `glass_core/config.rs` | **MODIFY** | Add validation, hot-reload watcher, `ConfigDiff` type |
| `glass_core/event.rs` | **MODIFY** | Add `AppEvent::ConfigChanged` and `AppEvent::UpdateAvailable` variants |
| `glass (binary)/main.rs` | **MODIFY** | Handle ConfigChanged, startup update check, profiling gates |
| `glass_renderer` | **MODIFY** | Accept font changes at runtime (rebuild FontSystem) |
| `glass_update` | **NEW CRATE** | Auto-update check/download/replace via GitHub Releases |
| `.github/workflows/release.yml` | **NEW FILE** | Release workflow producing MSI/DMG/deb/tar.gz |
| `packaging/` | **NEW DIR** | Installer configs (WiX XML, Info.plist, .desktop) |
| Profiling instrumentation | **MODIFY (scattered)** | `tracing` spans in hot paths across crates |

---

## Component 1: Config Hot-Reload

### Where It Lives

Modify `glass_core/config.rs` -- NOT a new crate. The config module already handles loading; hot-reload extends the same responsibility. The `notify` crate is already a workspace dependency (v8.2, used by `glass_snapshot` for FS watching).

### Architecture

```
~/.glass/config.toml
        │
        │ (notify crate -- file watcher, already in workspace)
        ▼
┌─────────────────────┐
│ ConfigWatcher        │  spawned on dedicated std::thread
│  - notify::Watcher   │  manual debounce: 500ms via Instant
│  - EventLoopProxy    │  sends AppEvent::ConfigChanged
└─────────┬───────────┘
          │
          ▼ AppEvent::ConfigChanged { changes: Vec<ConfigChange> }
┌─────────────────────┐
│ Processor (main.rs)  │
│  - diff old vs new   │
│  - apply what changed │
└─────────────────────┘
          │
          ├──▶ font_family/font_size changed → rebuild FontSystem, resize terminals
          ├──▶ history.* changed → update HistoryDb config thresholds
          ├──▶ snapshot.* changed → update SnapshotStore config thresholds
          ├──▶ pipes.* changed → update pipe capture settings
          └──▶ shell changed → log warning "requires restart"
```

### Key Design Decisions

**Reuse `notify 8.2` (already a workspace dependency).** The snapshot crate already depends on it. No new dependency needed. Use the `RecommendedWatcher` with a polling fallback (same as snapshot's FS watcher).

**Debounce at 500ms with manual `Instant` check.** Editors like vim write to temp files then rename; VS Code does atomic saves. A 500ms debounce window absorbs all intermediate filesystem events. Use a simple `Instant::elapsed()` check in the callback rather than adding `notify-debouncer-mini` as a new dependency.

**Diff-based application via ConfigChange enum.** Parse the new config, diff against the stored `GlassConfig`, and only apply changed fields. This avoids rebuilding the expensive FontSystem on every save when only a history threshold changed.

```rust
// In glass_core/config.rs
pub enum ConfigChange {
    FontFamily(String),
    FontSize(f32),
    HistorySection(HistorySection),
    SnapshotSection(Option<SnapshotSection>),
    PipesSection(Option<PipesSection>),
    // shell: not hot-reloadable
}

// In glass_core/event.rs
AppEvent::ConfigChanged {
    window_id: WindowId,  // broadcast to all windows
    changes: Vec<ConfigChange>,
}
```

**What is NOT hot-reloadable:** Shell override (`shell = "bash"`) requires PTY respawn -- log a warning: "Shell change requires restart." This is the same approach Alacritty uses.

**Validation on reload.** Currently, malformed TOML silently falls back to all defaults. For hot-reload, preserve the previous working config and log specific errors:
- Font size must be 6.0..=72.0
- Unknown TOML keys produce a warning (not an error)
- Invalid sections preserve the previous value and log what went wrong

### Integration Points

| Touches | What Changes |
|---------|-------------|
| `glass_core/config.rs` | Add `ConfigWatcher::new(proxy)`, `GlassConfig::diff(&self, &other)`, validation |
| `glass_core/event.rs` | Add `AppEvent::ConfigChanged` variant |
| `glass_core/Cargo.toml` | Add `notify` and `winit` dependencies (for EventLoopProxy type) |
| `main.rs` Processor | Store `GlassConfig` as mutable, spawn ConfigWatcher, handle ConfigChanged events |
| `glass_renderer` | Add `FontSystem::rebuild(family, size)` or equivalent method |

### Config Flow Detail

```
1. User edits ~/.glass/config.toml and saves
2. notify fires Create/Modify/Rename event on config.toml
3. ConfigWatcher callback checks Instant::elapsed() > 500ms since last reload
4. If debounce passed: re-read file, parse TOML via GlassConfig::load_from_str()
5. If parse fails: log error, keep existing config, do nothing
6. If parse succeeds: compute diff vs current config
7. If no changes: do nothing
8. If changes: send AppEvent::ConfigChanged { changes } via EventLoopProxy
9. Processor::user_event() matches ConfigChanged:
   - FontFamily/FontSize → call renderer.rebuild_font_system()
   - HistorySection → update session history_db thresholds
   - SnapshotSection → update session snapshot_store thresholds
   - PipesSection → update pipe capture settings
10. Store new config as current
```

---

## Component 2: Auto-Update

### Where It Lives

**New crate: `glass_update`** in `crates/glass_update/`. Isolated because update logic is complex, platform-specific, and optional.

### Architecture

```
┌─────────────────────────────────────┐
│ glass_update                         │
│                                      │
│  UpdateChecker                       │
│    - check() -> Option<Release>      │
│    - download_and_replace() -> Result│
│                                      │
│  Backed by: self_update crate        │
│  Backend: GitHub Releases API        │
│  Format: .tar.gz (Linux/macOS),      │
│          .zip (Windows)              │
└──────────────┬──────────────────────┘
               │
               ▼ called from main.rs
┌─────────────────────────────────────┐
│ Startup check (background tokio)     │
│  1. Compare CARGO_PKG_VERSION to     │
│     latest GitHub release tag        │
│  2. If newer: send AppEvent::        │
│     UpdateAvailable { version }      │
│  3. User triggers: Ctrl+Shift+U or   │
│     `glass update` CLI               │
└─────────────────────────────────────┘
```

### Key Design Decisions

**Use `self_update` crate.** It handles the download-extract-replace-binary workflow, supports GitHub Releases as a backend, works cross-platform, and uses `self_replace` for atomic binary replacement on Windows (where the running exe cannot be directly overwritten). Well-maintained with 400+ GitHub stars.

**Background check, manual apply.** Check for updates on startup in a background tokio task. Do NOT auto-apply. Show a non-intrusive status bar notification ("Update v2.2 available"). The user explicitly triggers via keyboard shortcut or CLI. This preserves trust -- users do not expect their terminal to restart itself.

**CLI subcommand for headless update:**
```
glass update          # Check and apply update
glass update --check  # Check only, print result
```

**Install-method detection.** When Glass was installed via MSI/DMG/package manager, `self_update` binary replacement would conflict with the package manager's tracking. Detect install method by checking the binary's path:
- `/usr/bin/glass` or `C:\Program Files\Glass\` = installer-managed, tell user to update via their package manager
- `~/.glass/bin/glass` or portable location = self-managed, binary replacement is safe

**Use `rustls` not native-tls.** Avoids OpenSSL dependency on Linux. Pure Rust TLS.

### Crate Structure

```
crates/glass_update/
  src/
    lib.rs        -- pub UpdateChecker, UpdateStatus
    checker.rs    -- version comparison, GitHub API call
    installer.rs  -- download, verify, replace binary
```

### Crate Dependencies

```toml
[dependencies]
self_update = { version = "0.41", default-features = false, features = ["rustls", "archive-tar", "archive-zip", "compression-flate2"] }
semver = "1"
tokio = { workspace = true }
tracing = { workspace = true }
anyhow = { workspace = true }
```

### Integration Points

| Touches | What Changes |
|---------|-------------|
| New `crates/glass_update/` | UpdateChecker struct, version comparison, download logic |
| `Cargo.toml` (workspace) | Add `self_update` and `semver` to workspace deps |
| `main.rs` CLI | Add `Commands::Update { check_only: bool }` subcommand |
| `main.rs` Processor | Spawn background update check on startup, handle UpdateAvailable |
| `glass_core/event.rs` | Add `AppEvent::UpdateAvailable { version: String, url: String }` |
| Status bar rendering | Show "[v2.2 available]" indicator when update exists |

---

## Component 3: Packaging and Distribution

### Where It Lives

**No new Rust crate.** This is entirely CI/build infrastructure plus static installer config files.

### Architecture

```
Git tag push (v2.1.0)
        │
        ▼
┌──────────────────────────────────────────────────┐
│ .github/workflows/release.yml                     │
│                                                    │
│  ┌──────────┐  ┌──────────┐  ┌──────────────┐    │
│  │ Windows  │  │  macOS   │  │    Linux      │    │
│  │          │  │          │  │               │    │
│  │ cargo    │  │ cargo    │  │ cargo build   │    │
│  │ build    │  │ build    │  │ --release     │    │
│  │ --release│  │ --release│  │               │    │
│  │          │  │          │  │               │    │
│  │ ┌──────┐ │  │ ┌──────┐ │  │ ┌───────────┐ │    │
│  │ │ .msi │ │  │ │ .dmg │ │  │ │ .deb      │ │    │
│  │ │ .zip │ │  │ │.tar.gz│ │  │ │ .tar.gz   │ │    │
│  │ └──────┘ │  │ └──────┘ │  │ └───────────┘ │    │
│  └──────────┘  └──────────┘  └──────────────┘    │
│                                                    │
│  Upload all artifacts to GitHub Release            │
└──────────────────────────────────────────────────┘
```

### Platform Installer Details

**Windows (MSI via cargo-wix / WiX 4):**
- WiX Toolset 4.x generates `.msi` installer
- Installs to `C:\Program Files\Glass\`
- Adds `glass.exe` to system PATH
- Includes shell integration scripts in install dir
- Also produce a portable `.zip` (binary + scripts)
- Config file: `packaging/windows/main.wxs`
- Reference: Alacritty uses the same approach (WiX MSI + portable exe)

**macOS (DMG with .app bundle):**
- Create `.app` bundle with `Info.plist` and icon
- Use `hdiutil` in CI to produce `.dmg`
- Separate builds per architecture (aarch64 primary, x86_64 secondary) -- `lipo` universal binary optional
- Also produce `.tar.gz` for Homebrew-style installs
- Config files: `packaging/macos/Info.plist`, `packaging/macos/glass.icns`

**Linux (deb + tar.gz):**
- `cargo-deb` for `.deb` package (Ubuntu/Debian)
- Tar.gz with binary + shell integration scripts for other distros
- Install binary to `/usr/bin/glass`, shell scripts to `/usr/share/glass/`
- Config files: `packaging/linux/glass.desktop`, deb metadata in `Cargo.toml`
- Skip rpm/Flatpak/AppImage initially -- deb covers the largest desktop Linux base, tar.gz covers everything else

### Directory Layout

```
packaging/
├── windows/
│   └── main.wxs              # WiX installer definition
├── macos/
│   ├── Info.plist             # .app bundle metadata
│   ├── glass.icns             # Application icon
│   └── create-dmg.sh          # DMG creation script
├── linux/
│   ├── glass.desktop          # XDG desktop entry
│   └── postinst               # Post-install script (optional)
└── scripts/
    └── install.sh             # curl-pipe installer for quick installs
```

### Integration Points

| Touches | What Changes |
|---------|-------------|
| `.github/workflows/release.yml` | New workflow triggered by `v*` tags |
| `packaging/` directory | New -- all installer config files |
| `Cargo.toml` | Add `[package.metadata.wix]` and `[package.metadata.deb]` sections |
| Existing `ci.yml` | No change -- remains for PR/push CI |

### Release Workflow Trigger

```yaml
on:
  push:
    tags: ['v*']  # Triggered by: git tag v2.1.0 && git push --tags
```

The workflow matrix mirrors the existing CI matrix (windows-latest, macos-latest, ubuntu-latest) but adds packaging steps after `cargo build --release`.

---

## Component 4: Performance Profiling

### Where It Lives

**No new crate.** Add instrumentation spans across existing crates using `tracing` (already a workspace dependency everywhere). Add `tracing-flame` as an optional dependency for flamegraph output. Feature-gated behind `--features perf`.

### Architecture

```
┌──────────────────────────────────────────────────────────┐
│ Compile-time feature: "perf"                              │
│                                                            │
│  When enabled:                                             │
│    - tracing-flame subscriber layer → flamegraph.folded    │
│    - Optional: wgpu-profiler → GPU timing data             │
│    - memory-stats logging (already available)              │
│                                                            │
│  Instrumented hot paths:                                   │
│    glass_terminal:  PTY read loop, VTE byte processing     │
│    glass_renderer:  frame render, glyph shaping, GPU pass  │
│    glass_core:      config load, config diff               │
│    glass (binary):  event dispatch, keyboard encode        │
│    glass_snapshot:  blob write, BLAKE3 hash                │
│    glass_history:   DB insert, FTS5 search queries         │
└──────────────────────────────────────────────────────────┘
```

### Key Design Decisions

**Use `tracing` spans (already everywhere) + `tracing-flame` for flamegraphs.** The project already uses `tracing` and `tracing-subscriber` throughout. Adding `tracing-flame` as an optional subscriber produces folded stack traces consumable by `inferno` (Rust flamegraph tool). Zero new instrumentation API to learn.

**Feature-gated, not always-on.** Profiling adds overhead even when spans are "inactive" because `tracing` still checks the subscriber filter. Gate behind a Cargo feature:
```toml
[features]
perf = ["dep:tracing-flame"]

[dependencies]
tracing-flame = { version = "0.2", optional = true }
```

**Instrumentation targets (key metrics):**

| Metric | Location | Instrumentation |
|--------|----------|-----------------|
| Cold start time | `main.rs` | Already measured (360ms baseline), keep as-is |
| Key-to-screen latency | `main.rs` event loop | `#[tracing::instrument]` on keyboard handler |
| Frame render time | `glass_renderer` | Span around `render_frame()` and sub-passes |
| PTY read throughput | `glass_terminal` | Span in PTY reader thread loop, bytes/iteration |
| Glyph cache hit rate | `glass_renderer` | Counter in FontSystem, logged periodically |
| Memory usage | `main.rs` | Already have `memory-stats`, log periodically |
| DB query time | `glass_history` | Span around `insert_command()`, `search()` |
| Config parse time | `glass_core` | Span around `GlassConfig::load()` |

**CLI subcommand for on-demand profiling (optional, defer if time-constrained):**
```
glass profile --duration 10   # Profile for 10s, write flamegraph.folded
```

**GPU profiling via `wgpu-profiler`.** This is a MEDIUM confidence recommendation -- need to verify compatibility with wgpu 28.0. Feature-gate separately if added:
```toml
[features]
gpu-profile = ["dep:wgpu-profiler"]
```

### Integration Points

| Touches | What Changes |
|---------|-------------|
| Root `Cargo.toml` | Add `perf` feature, `tracing-flame` optional dep |
| `main.rs` | Conditional `tracing-flame` subscriber layer when `perf` feature enabled |
| `glass_renderer/src/*.rs` | Add `#[tracing::instrument(skip_all)]` on render hot paths |
| `glass_terminal/src/pty.rs` | Add span in PTY read loop |
| `glass_history/src/db.rs` | Add spans around DB operations |
| `glass_snapshot/src/*.rs` | Add spans around blob store operations |

---

## Data Flow Changes Summary

### Config Hot-Reload Flow

```
1. User saves ~/.glass/config.toml
2. notify::Watcher fires filesystem event
3. ConfigWatcher thread checks debounce (500ms)
4. Re-parses TOML via GlassConfig::load_from_str()
5. If valid: compute diff vs current config
6. Send AppEvent::ConfigChanged { changes } via EventLoopProxy
7. Processor::user_event() matches ConfigChanged
8. Per-change dispatch:
   - FontFamily/FontSize → renderer.rebuild_font_system()
   - Sections → update thresholds on active sessions
   - Shell → log "requires restart"
9. Store new config
```

### Auto-Update Flow

```
1. App starts → spawn tokio task: glass_update::check()
2. HTTP GET GitHub Releases API (rate limit: 60/hr unauthenticated)
3. Compare latest tag semver to env!("CARGO_PKG_VERSION")
4. If newer: send AppEvent::UpdateAvailable { version }
5. Processor renders "[v2.2 available]" in status bar
6. User presses Ctrl+Shift+U or runs `glass update`
7. glass_update::download_and_apply()
8. Binary replaced atomically → prompt user to restart
```

### Packaging Flow (CI only, not runtime)

```
1. Developer pushes tag v2.1.0
2. GitHub Actions release.yml triggers on tag
3. Matrix: Windows (MSI+zip), macOS (DMG+tar.gz), Linux (deb+tar.gz)
4. Artifacts uploaded to GitHub Release
5. self_update checks this same Release endpoint for newer versions
```

---

## Build Order (Dependency-Aware)

These four features have minimal interdependencies but the following order respects logical prerequisites.

### Phase 1: Performance Profiling Instrumentation
**Why first:** Profiling should be in place BEFORE the optimization pass so you can measure impact. Zero new crates, just adding spans and an optional subscriber.
- Add `tracing::instrument` to render, PTY, and DB hot paths
- Add `tracing-flame` optional subscriber behind `perf` feature
- Run profiling, capture baseline flamegraph
- Optimize bottlenecks based on data (not guesses)

### Phase 2: Config Validation and Hot-Reload
**Why second:** Config validation is a prerequisite for safe hot-reload. Modifies `glass_core` which is a shared dependency -- do it before adding new crates.
- Add validation rules to `GlassConfig` (range checks, unknown key warnings)
- Add `GlassConfig::diff()` method
- Add `ConfigWatcher` using `notify` (already in workspace)
- Add `AppEvent::ConfigChanged` variant
- Implement per-field application in Processor
- Add renderer font rebuild path (`FontSystem::rebuild()`)

### Phase 3: Packaging and CI Release Workflow
**Why third:** Pure infrastructure -- no runtime code changes. Must exist before auto-update (which needs GitHub Releases with downloadable artifacts).
- Create `packaging/` directory with installer configs
- Write `release.yml` GitHub Actions workflow
- Add `[package.metadata.wix]` and `[package.metadata.deb]` to Cargo.toml
- Test MSI, DMG, and deb builds in CI
- Produce first tagged release with platform installers

### Phase 4: Auto-Update Mechanism
**Why last:** Depends on packaging (Phase 3) because it downloads artifacts from GitHub Releases. New crate `glass_update`.
- Create `crates/glass_update/` with `UpdateChecker`
- Integrate `self_update` crate with GitHub Releases backend
- Add `Commands::Update` CLI subcommand
- Add background check on startup (tokio task)
- Add `AppEvent::UpdateAvailable` and status bar notification
- Add install-method detection (MSI vs portable)

### Phase 5: Documentation
**Why last:** Documents the finished product. Can overlap with Phase 4.
- README rewrite with installation instructions per platform
- Config reference (all TOML sections, hot-reloadable vs restart-required)
- Keyboard shortcuts reference
- Contributing/building guide

---

## Architectural Patterns

### Pattern 1: Event-Driven Config Propagation (extend existing pattern)

**What:** Config changes flow through the same `AppEvent` / `EventLoopProxy` mechanism already used for PTY events, shell events, git status, and terminal dirty notifications.
**When to use:** Any background thread/watcher needs to notify the main event loop.
**Trade-offs:** Consistent with existing architecture. Slightly more latency than direct mutation (goes through event loop queue), but thread-safe and maintains the single-writer pattern on the main thread.

### Pattern 2: Feature-Gated Optional Components

**What:** Use Cargo features to gate heavy optional dependencies (`tracing-flame`, `wgpu-profiler`).
**When to use:** Development/debugging tools that should not increase binary size or runtime overhead in release builds.
**Trade-offs:** Conditional compilation adds `#[cfg(feature = "perf")]` annotations but keeps release binary lean.

### Pattern 3: Background Check with User-Triggered Action

**What:** Background async task checks for updates/information but never applies disruptive changes automatically.
**When to use:** Updates, migrations, or any operation that disrupts the user's workflow.
**Trade-offs:** Slower update adoption vs. user trust and session stability.

### Pattern 4: Diff-Then-Apply for Config Changes

**What:** Compute a diff between old and new config, then apply only the changed fields rather than rebuilding everything.
**When to use:** When config changes have heterogeneous costs (font rebuild is expensive, threshold update is cheap).
**Trade-offs:** More code to maintain the diff logic, but avoids visible flicker from unnecessary font rebuilds.

---

## Anti-Patterns

### Anti-Pattern 1: Polling Config File on a Timer

**What people do:** `loop { sleep(1s); reload_config(); }` instead of filesystem events.
**Why it's wrong:** Wastes CPU, misses rapid saves, adds 0-1s latency to config changes.
**Do this instead:** Use `notify` crate with debounce. Glass already uses `notify 8.2` for snapshot FS watching -- reuse it.

### Anti-Pattern 2: Auto-Applying Updates Without Consent

**What people do:** Download and replace binary silently on startup.
**Why it's wrong:** User's terminal disappears mid-session. Binary replacement can fail and corrupt the install. Breaks trust.
**Do this instead:** Check in background, notify in status bar, user explicitly triggers apply + restart.

### Anti-Pattern 3: Rebuilding Everything on Config Change

**What people do:** Tear down renderer, fonts, and terminal state when any config field changes.
**Why it's wrong:** Causes visible flicker, ~35ms FontSystem rebuild, drops scroll position.
**Do this instead:** Diff config, apply only changed fields. Font change rebuilds FontSystem but preserves terminal grids. History thresholds update in-place.

### Anti-Pattern 4: Shipping Only a Bare Binary

**What people do:** Upload `glass.exe` to GitHub Releases with no installer.
**Why it's wrong:** No PATH setup, no shell integration scripts installed, no Start Menu entry, no uninstaller. Users must manually manage everything.
**Do this instead:** Provide both an installer (MSI/DMG/deb) for system integration AND a portable archive for power users.

### Anti-Pattern 5: Always-On Profiling in Release Builds

**What people do:** Leave profiling spans always active.
**Why it's wrong:** Even inactive tracing spans have ~1ns overhead per call. In a 60fps render loop processing thousands of cells, this adds up across hundreds of span sites.
**Do this instead:** Feature-gate profiling behind `--features perf`. Ship release builds without the feature.

---

## Integration Point Summary

### Internal Boundaries

| Boundary | Communication | Notes |
|----------|---------------|-------|
| ConfigWatcher -> Processor | `AppEvent::ConfigChanged` via EventLoopProxy | Same pattern as GitInfo, Shell, CommandOutput events |
| UpdateChecker -> Processor | `AppEvent::UpdateAvailable` via EventLoopProxy | Background tokio task on startup |
| Processor -> Renderer | Direct method call (`rebuild_font_system()`) | Synchronous, main thread only |
| CI -> GitHub Releases | Tag-triggered workflow upload | No runtime component |
| glass_update -> GitHub API | HTTPS via reqwest (inside self_update) | rustls backend, no OpenSSL |

### External Services

| Service | Integration Pattern | Notes |
|---------|---------------------|-------|
| GitHub Releases API | self_update crate handles HTTP + JSON | Rate limited 60/hr unauthenticated; cache result |
| Filesystem (notify) | notify 8.2 RecommendedWatcher | Platform: ReadDirectoryChangesW / FSEvents / inotify |
| WiX Toolset 4 | CI-only via cargo-wix | Windows MSI generation |
| cargo-deb | CI-only | Debian package generation |
| hdiutil | CI-only (macOS runner) | DMG creation |

---

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Config hot-reload architecture | HIGH | `notify` already in workspace, pattern well-established in Rust ecosystem |
| Auto-update via self_update | HIGH | Crate is mature, GitHub Releases backend well-documented |
| Packaging (MSI/DMG/deb) | HIGH | Alacritty uses identical approach (WiX + hdiutil + cargo-deb) |
| Performance profiling via tracing-flame | HIGH | tracing already in workspace, tracing-flame is a standard layer |
| GPU profiling via wgpu-profiler | MEDIUM | Need to verify wgpu 28.0 compatibility |
| Build order | HIGH | Dependency chain is clear (profiling before optimization, packaging before auto-update) |

## Open Questions

- **wgpu-profiler version:** Does the latest wgpu-profiler support wgpu 28.0? Needs verification before committing to GPU profiling.
- **macOS code signing:** DMG distribution may trigger Gatekeeper warnings without signing. Apple Developer Program ($99/yr) needed for proper code signing. Can defer to a future milestone.
- **Windows SmartScreen:** Unsigned MSI triggers SmartScreen warning. cargo-dist supports SSL.com eSigner for code signing, but adds cost/complexity. Can defer.
- **Update check rate limiting:** GitHub API allows 60 requests/hr unauthenticated. For daily-driver use, checking once per startup (with a 24hr cooldown cache) is sufficient.

## Sources

- [self_update crate](https://github.com/jaemk/self_update) -- GitHub Releases backend for auto-update (HIGH confidence)
- [cargo-wix](https://github.com/volks73/cargo-wix) -- MSI installer generation for Rust projects (HIGH confidence)
- [cargo-deb](https://crates.io/crates/cargo-deb) -- Debian package generation (HIGH confidence)
- [notify crate](https://crates.io/crates/notify) -- Already in workspace v8.2 for FS watching (HIGH confidence)
- [tracing-flame](https://lib.rs/crates/tracing-flame) -- Flamegraph generation from tracing spans (HIGH confidence)
- [wgpu-profiler](https://docs.rs/wgpu-profiler) -- GPU timing for wgpu pipelines (MEDIUM confidence)
- [Alacritty packaging](https://github.com/alacritty/alacritty) -- Reference implementation for terminal MSI/DMG distribution (HIGH confidence)
- [cargo-dist](https://github.com/axodotdev/cargo-dist) -- Alternative to manual release workflow; considered but manual preferred for control (MEDIUM confidence)
- [Rust hot-reloader patterns](https://github.com/junkurihara/rust-hot-reloader) -- Reference for notify-based config reload (MEDIUM confidence)
- [profiling crate](https://github.com/aclysma/profiling) -- Abstraction over profiling backends (MEDIUM confidence)

---
*Architecture research for: Glass v2.1 Packaging & Polish*
*Researched: 2026-03-07*

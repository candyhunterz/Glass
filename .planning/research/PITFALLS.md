# Pitfalls Research

**Domain:** Adding packaging, auto-update, config hot-reload, and performance profiling to an existing Rust terminal emulator (Glass v2.1)
**Researched:** 2026-03-07
**Confidence:** HIGH (based on codebase analysis of config.rs/main.rs/event.rs, ecosystem research, and known platform behaviors)

## Critical Pitfalls

### Pitfall 1: Config hot-reload triggers font/renderer rebuild cascade without debouncing

**What goes wrong:**
A single file save in most editors (VS Code, vim, nano) generates 2-5 filesystem events (write, close, modify metadata, rename-then-write). Without debouncing, each event triggers a full config reload. Since Glass uses `glyphon` for text rendering with per-line `cosmic_text::Buffer` objects, a font family or font size change requires rebuilding the entire `FontSystem`, recalculating all glyph metrics, resizing the terminal grid, and re-laying-out every visible line. Triggering this 3-5 times in 50ms causes visible flickering, frame drops, and potential panics if the renderer is mid-frame when the font system is replaced.

**Why it happens:**
Developers test hot-reload with manual saves and see one event. In production, editors use atomic-write patterns (write temp file, rename over original) that generate multiple events. The `notify` crate (already a Glass dependency at v8.2) faithfully reports each one. Linux inotify generates 3-5 events per save; Windows `ReadDirectoryChangesW` generates 2-3; macOS FSEvents batches them ambiguously.

**How to avoid:**
Use `notify-debouncer-mini` or implement a 300-500ms debounce window. On receiving any config file event, start a timer. Only reload after 500ms of silence. When reloading, diff the old and new `GlassConfig` structs to determine what actually changed -- skip font rebuild if only `[history]` or `[pipes]` sections changed. Never rebuild fonts for non-visual config changes.

**Warning signs:**
- Testing only on one OS (events behave differently per platform)
- No debounce timer in the file watcher setup
- Font flickering when saving config in VS Code
- Multiple "Config reloaded" log lines per save

**Phase to address:**
Config hot-reload phase. Debouncing must be the first thing implemented, before any reload logic.

---

### Pitfall 2: Config is owned directly by App -- no shared-state pattern for multi-consumer reload

**What goes wrong:**
Currently `GlassConfig` is a plain `Clone` struct stored as `config: GlassConfig` on the `App` struct in `main.rs` (line 156). Config values are read directly via `self.config.font_family`, `self.config.pipes.as_ref()`, `self.config.snapshot.clone()`, etc. across 15+ call sites spanning session creation (line 328, 352), rendering (lines 468-469), history capture (line 1795), snapshot management (line 500, 1529), and pipe processing (lines 1432, 1450, 1465). Hot-reload requires atomically swapping the config while multiple consumers read it. Naively mutating `self.config` in a file watcher callback while a render frame is in progress creates torn reads -- a frame that starts with `font_size = 14.0` but switches to `font_size = 18.0` halfway through.

**Why it happens:**
The "load once at startup" design (line 1935: `let config = GlassConfig::load()`) was correct for v1.0-v2.0. Adding hot-reload is a retroactive architectural change that touches every config consumer.

**How to avoid:**
Since Glass uses winit's event loop with `EventLoopProxy`, send a `UserEvent::ConfigReloaded(GlassConfig)` through the proxy when the file watcher detects a change. The reload happens synchronously in the event loop -- the same thread that reads config. This is the simplest correct approach for Glass's architecture because it avoids `Arc<RwLock<>>` complexity and guarantees config is consistent within a single frame. Add a `ConfigReloaded` variant to `AppEvent` in `glass_core::event`.

**Warning signs:**
- Adding `&mut self.config` writes in a file watcher callback running on a different thread
- No `ConfigReloaded` event variant in `AppEvent`
- Config reads scattered across async boundaries without synchronization
- File watcher calling `GlassConfig::load()` directly instead of through event loop

**Phase to address:**
Config hot-reload phase. The event-based reload pattern must be established before implementing any individual reload handlers (font, colors, history settings, etc.).

---

### Pitfall 3: MSI UpgradeCode not set from day one -- breaks all future auto-updates on Windows

**What goes wrong:**
Windows Installer uses a stable `UpgradeCode` GUID to identify that two MSI packages are versions of the same product. If the first MSI release ships without a fixed UpgradeCode (or if it changes between versions), Windows Installer cannot detect the previous installation during `FindRelatedProducts`. The user ends up with two entries in "Add/Remove Programs," potentially two copies of the executable, and `ALLUSERS` mismatch between versions causes the old install to be invisible to the new installer. There is no way to retroactively fix this for users who installed with the wrong UpgradeCode without asking them to manually uninstall.

**Why it happens:**
`cargo-wix` generates a new random GUID in the WiX template on `cargo wix init`. Developers build the first MSI, ship it, then only discover the UpgradeCode problem when they ship version 2. By then, early adopters have the wrong GUID installed.

**How to avoid:**
Run `cargo wix init` once, then immediately hardcode a permanent UpgradeCode GUID in `wix/main.wxs`. Commit this to version control. Never change it. The ProductCode should change with each version (use `*` for auto-generation), but the UpgradeCode is forever. Add a CI check that verifies the UpgradeCode has not changed from the committed value.

**Warning signs:**
- `UpgradeCode` not committed to version control
- WiX template regenerated per release instead of edited
- No upgrade testing (install v1, then install v2, verify v1 is removed)
- Different `ALLUSERS` value between versions

**Phase to address:**
Packaging phase (first phase of v2.1). Must be locked down before the first MSI is distributed to anyone.

---

### Pitfall 4: Auto-updater tries to replace the running Glass.exe on Windows

**What goes wrong:**
On Windows, a running executable holds a file lock. The auto-update mechanism downloads the new binary and tries to overwrite `glass.exe` -- Windows denies the write because the file is locked by the running process. Naive workarounds: (1) kill the process then replace -- leaves the user with no terminal and possibly lost work; (2) rename the running exe (Windows allows this), place new binary, prompt restart -- works but leaves orphaned `.old` files; (3) download new MSI and invoke `msiexec /i` -- correct but requires MSI infrastructure.

**Why it happens:**
Linux and macOS allow replacing a running binary's file (the running process keeps its inode reference). Developers who test on Linux first don't encounter this. Windows is the odd one out, and it is Glass's primary platform.

**How to avoid:**
Use the MSI upgrade path for Windows: download the new MSI, prompt "Update available, restart Glass to install," and when the user agrees, launch `msiexec /i new_version.msi` and exit Glass. The MSI handles file replacement, PATH updates, and Start Menu shortcuts. For macOS, download new DMG and prompt relaunch. For Linux, use system package manager updates (apt/dnf) rather than self-update. Only implement direct binary replacement on platforms that support it (Linux standalone binary).

**Warning signs:**
- Auto-update code using `std::fs::copy` or `std::fs::rename` to overwrite the current exe path
- No restart-after-update flow
- No `.old` file cleanup on startup
- Testing auto-update only on Linux/macOS

**Phase to address:**
Auto-update phase. Must be designed specifically for Windows first since it is the primary platform.

---

### Pitfall 5: macOS notarization blocks distribution of unsigned DMGs

**What goes wrong:**
macOS Gatekeeper quarantines unsigned applications downloaded from the internet. Users see "Glass is damaged and can't be opened" or "Glass can't be opened because it is from an unidentified developer." macOS Sequoia (15+) has tightened these restrictions further -- users need `xattr -d com.apple.quarantine` from the command line, which is a dealbreaker for non-technical users. Hardened runtime must be enabled (`codesign --options=runtime`), and notarization requires submitting the binary to Apple's servers which can have multi-hour delays.

**Why it happens:**
Apple code signing requires a $99/year Apple Developer account and a specific certificate type (Developer ID Application). The apple-codesign (rcodesign) Rust crate can sign and notarize from any platform, but the certificate management and GitHub Actions secrets setup is complex. Developers skip it "for now" and never come back.

**How to avoid:**
Budget for an Apple Developer account. Use the `apple-codesign` crate (rcodesign) to sign and notarize from CI without needing macOS for the signing step. Set up code signing in the CI pipeline alongside DMG creation -- not as a follow-up task. Store the signing certificate as a GitHub Actions secret. Test by downloading the DMG from GitHub releases on a clean Mac, not by running a locally-built binary.

**Warning signs:**
- DMG works when built locally (no quarantine flag) but fails when downloaded
- No `codesign --verify` step in CI
- No Apple Developer account budgeted
- Notarization treated as "we'll do it later"

**Phase to address:**
Packaging phase. Code signing setup must happen alongside DMG creation, not after.

---

### Pitfall 6: Performance profiling without baseline benchmarks -- optimizing the wrong thing

**What goes wrong:**
Developers start a "performance optimization pass" by guessing what is slow (usually the GPU renderer) and spending days optimizing draw calls when the actual bottleneck is font shaping, config parsing, or SQLite queries. Glass already has good ad-hoc metrics (360ms cold start, 3-7us key latency, 86MB idle) but no automated benchmark suite. Without baselines, there is no way to know if an optimization helped or if a regression was introduced. The existing `ScaleFactorChanged` handler is log-only (known tech debt) -- performance work might trigger DPI-related regressions that go undetected.

**Why it happens:**
Performance intuition is almost always wrong. GPU rendering feels expensive, but Glass's instanced quad pipeline is efficient. Real bottlenecks in terminal emulators are typically: (1) PTY read loop throughput for large outputs (`cat large_file.txt`), (2) VTE parsing for complex escape sequences, (3) glyph cache misses when new Unicode characters appear, (4) SQLite writes blocking the event loop during history logging, (5) `notify` watcher overhead with many watchers active.

**How to avoid:**
Before optimizing anything: (1) Add `criterion` benchmarks for the critical path: PTY read throughput, VTE parse rate, grid snapshot lock duration, glyph shaping latency. (2) Profile with `cargo flamegraph` or `Tracy` to find actual hotspots. (3) Measure before and after every change. (4) For GPU-specific profiling, use `wgpu-profiler` to measure GPU-side frame timing. (5) Add a CI benchmark job that detects regressions > 5%.

**Warning signs:**
- Optimization PRs without before/after numbers
- Spending time on the renderer when no rendering benchmark exists
- "It feels faster" as the success criterion
- No `criterion` or `divan` dependency in `dev-dependencies`

**Phase to address:**
Performance profiling phase. Establish baselines and benchmarks before any optimization work begins.

---

### Pitfall 7: Config hot-reload changes font but only updates the focused pane

**What goes wrong:**
Glass v2.0 has tabs with split panes. Each tab owns a `SplitNode` tree, and each leaf pane has its own session with independent terminal grid, viewport, and render state. When font_family or font_size changes via hot-reload, the naive implementation updates the `FontSystem` and resizes only the active/focused pane's terminal grid. The other 5+ panes in other tabs continue rendering with stale font metrics, causing misaligned text, wrong column counts, and eventual panics when the renderer tries to draw glyphs with the old font metrics but the new font system.

**Why it happens:**
The config reload handler processes the event in the context of the focused pane. Developers test with a single pane and it works. Multi-pane testing is skipped because it requires opening multiple tabs and splits.

**How to avoid:**
On font change, iterate ALL sessions in `SessionMux`, and for each session, iterate ALL panes in the `SplitTree`. For each pane: recalculate cell dimensions, resize the terminal grid (columns/rows may change), send resize to the PTY (which triggers `SIGWINCH` on Unix / `ResizePseudoConsole` on Windows), and invalidate the glyph cache. Use a helper method like `apply_font_change_to_all_panes()` that the config reload handler calls.

**Warning signs:**
- Config reload handler only calls resize on `self.active_session()` or similar
- No test with multiple tabs and splits open during config change
- Text misalignment in background tabs after font change

**Phase to address:**
Config hot-reload phase. The multi-pane propagation must be part of the core reload logic, not an afterthought.

---

## Technical Debt Patterns

Shortcuts that seem reasonable but create long-term problems.

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Reload entire config on any file change | Simple implementation | Unnecessary font rebuilds, frame drops when only `[history]` changed | Never -- always diff old vs new config |
| Ship unsigned binaries "to start" | Faster initial release | macOS users blocked by Gatekeeper; Windows SmartScreen warns "Unknown publisher" | Never for macOS; acceptable for early Windows testing only |
| Self-contained update binary (no MSI) | No WiX dependency | Cannot cleanly replace running exe on Windows, no uninstall entry, no Start Menu shortcuts | Never on Windows; acceptable on Linux standalone |
| Skip CI benchmarks | Faster CI runs | Performance regressions ship undetected | Only before first public release; add benchmarks as part of v2.1 |
| Polling-based config reload (check every N seconds) | No additional `notify` watcher needed | Unnecessary CPU usage, delayed feedback (up to N seconds), wastes battery on laptops | Never -- Glass already depends on `notify` |
| `GlassConfig::load()` in watcher thread | Simple hot-reload | File read on watcher thread, potential race with main thread reading config | Never -- parse in watcher, send via `EventLoopProxy` |
| Hardcoding update check URL in binary | Simple update mechanism | Cannot change update server without shipping new binary | Acceptable if URL points to a stable redirect endpoint |

## Integration Gotchas

Common mistakes when connecting these new features to the existing Glass system.

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| Config reload + GPU renderer | Rebuilding the entire wgpu surface/pipeline on font change | Only rebuild `FontSystem` and glyph atlas; the pipeline, shaders, and surface are font-independent |
| Config reload + split panes | Applying new font size to focused pane only | Iterate all sessions in `SessionMux`, resize every pane's terminal grid and viewport |
| Config reload + existing `notify` watcher | Two `notify` watchers competing (one for snapshots in `glass_snapshot`, one for config) | Use separate watchers with clearly distinct event channels, or a shared watcher with path-based demuxing |
| Config reload + shell setting | Changing `shell = "fish"` restarts current session | Shell changes should only affect new tabs/panes, not running sessions |
| Config reload + TOML errors | Parse error resets all config to defaults | On reload error, keep previous working config and show error in status bar |
| Auto-update + MCP server | Update kills `glass mcp serve` subprocess | MCP server is a separate process launched by AI clients, not managed by Glass terminal -- auto-update only affects the terminal binary |
| MSI installer + shell integration | Installer doesn't add `glass.exe` to PATH | Include PATH modification in WiX template (`Environment` element); shell integration scripts need `glass` on PATH for `glass history` commands |
| MSI installer + config migration | New version expects config in new location, old config orphaned | First-run migration: check old location, copy to new, log warning |
| Performance benchmarks + multi-pane | Profiling single-session and declaring "done" | Benchmark with 4+ split panes rendering simultaneously -- GPU and memory pressure scales with pane count |
| Update check + startup time | Synchronous HTTP request blocks terminal launch | Check for updates asynchronously after terminal is rendered; never block startup |

## Performance Traps

Patterns that work at small scale but fail as usage grows.

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Re-shaping all glyphs on font size change | 200ms+ freeze on config reload with large scrollback | Only reshape visible lines; invalidate scrollback glyph cache lazily | Scrollback > 5000 lines |
| Synchronous config file read on watcher event | Main thread blocks for 1-5ms reading/parsing TOML | Read and parse in watcher thread, send parsed result via `EventLoopProxy` | Config file on network drive or slow disk |
| Downloading update on main thread | Terminal freezes during multi-MB download | Download in background tokio task, show progress in status bar | Any network latency > 100ms |
| `criterion` benchmarks in default `cargo test` | CI time doubles, developers skip running tests | Put benchmarks behind `--bench` flag (criterion default), separate CI job | Benchmark suite > 30 seconds |
| Full scrollback re-render on any config change | Multi-second freeze | Only re-render visible viewport; scrollback re-rendered on scroll | Scrollback > 10000 lines |
| Update check on every Glass launch | Cold start goes from 360ms to 2000ms+ | Check at most once per day; cache result; never block startup | Always, even on fast networks |
| Profiling with debug builds | Numbers meaningless, 10-50x slower than release | Always profile with `--release`; `criterion` does this by default | Always |

## Security Mistakes

Domain-specific security issues for packaging, updates, and config reload.

| Mistake | Risk | Prevention |
|---------|------|------------|
| Auto-updater downloads over HTTP without hash verification | MITM replaces glass.exe with malware | HTTPS only + SHA256/BLAKE3 hash verification against a signed manifest |
| Storing code signing certificates in the repo | Certificate leaked, attacker signs malicious updates | GitHub Actions secrets only; never commit `.pfx`, `.p12`, or private keys |
| Config hot-reload allows `exec` or `shell_command` fields | Malicious config file runs arbitrary code | `GlassConfig` only contains data fields (strings, numbers, bools) -- maintain this invariant |
| Auto-updater runs with elevated privileges unnecessarily | Privilege escalation if update mechanism is compromised | Download and verification as normal user; only MSI installation elevates via UAC prompt |
| Update manifest URL configurable via config.toml | Attacker changes config to point to malicious update server | Hardcode the update check URL in the binary; never allow config to override it |
| Self-update binary not verified before execution | Corrupted download replaces working binary | Download to temp file, verify checksum, then rename into place; keep backup of old binary |

## UX Pitfalls

Common user experience mistakes in this domain.

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| Silent config reload with no feedback | User changes font_size, nothing happens (typo in field name), thinks hot-reload is broken | Flash status bar message: "Config reloaded" or "Config error: unknown field 'fontt_size'" |
| Auto-update interrupts active terminal session | User loses work in running commands | Never auto-restart; show notification "Update available, restart to apply" and let user choose when |
| Config error drops all settings to defaults on reload | User makes one typo, entire config resets | On reload error, keep previous config and show error in status bar |
| Installer requires admin privileges when not needed | Users in corporate/locked-down environments can't install | Per-user install option (install to `%LOCALAPPDATA%`) in addition to system-wide install |
| No config validation error messages | User sets `font_size = -5` and gets cryptic rendering bug | Validate config values on load/reload; report specific errors: "font_size must be between 6.0 and 72.0" |
| Update download shows no progress | User thinks Glass is frozen during update download | Show download progress in status bar with percentage and ETA |
| Config changes apply inconsistently across panes | Some panes have new font, others have old font | Apply changes to all panes atomically before next render frame |

## "Looks Done But Isn't" Checklist

Things that appear complete but are missing critical pieces.

- [ ] **Config hot-reload:** Often missing validation error reporting -- verify user sees what went wrong when TOML is malformed
- [ ] **Config hot-reload:** Often missing shell change handling -- verify that changing `shell = "fish"` does NOT affect existing sessions (only new tabs)
- [ ] **Config hot-reload:** Often missing multi-pane propagation -- verify font change applies to ALL panes in ALL tabs, not just focused one
- [ ] **Config hot-reload:** Often missing debounce -- save config 5 times rapidly, verify only one reload occurs
- [ ] **Config hot-reload:** Often missing diff -- change only `[history].max_output_capture_kb`, verify fonts are NOT rebuilt
- [ ] **MSI installer:** Often missing UpgradeCode stability -- verify same GUID across v2.1.0, v2.1.1, v2.2.0 builds
- [ ] **MSI installer:** Often missing PATH entry -- verify `glass` command works from any terminal after install
- [ ] **MSI installer:** Often missing per-user install option -- verify non-admin users can install
- [ ] **Auto-update:** Often missing rollback -- verify that if new version crashes on startup, user can recover (old binary kept as backup)
- [ ] **Auto-update:** Often missing proxy support -- verify update checks work behind corporate HTTP proxies
- [ ] **Auto-update:** Often missing offline handling -- verify Glass starts normally when update server is unreachable
- [ ] **macOS DMG:** Often missing notarization -- verify DMG works when downloaded from internet (not just built locally)
- [ ] **Linux packages:** Often missing desktop file and icon -- verify Glass appears in application launchers
- [ ] **Performance benchmarks:** Often missing throughput test -- verify `cat /dev/urandom | head -c 10M` doesn't crash or freeze
- [ ] **Performance benchmarks:** Often missing multi-pane test -- benchmark with 4 split panes, not just 1
- [ ] **ScaleFactorChanged:** Currently log-only (known tech debt) -- verify DPI change triggers font metric recalculation after hot-reload is implemented

## Recovery Strategies

When pitfalls occur despite prevention, how to recover.

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Wrong UpgradeCode shipped in MSI | HIGH | Must ask users to manually uninstall old version; add detection in new MSI for orphaned installs |
| Config reload crashes terminal | MEDIUM | Add `--safe-mode` CLI flag that skips config file and uses defaults; add crash handler that logs which config field caused issue |
| Unsigned macOS binary shipped | MEDIUM | Re-sign and notarize the DMG; existing users must re-download; provide `xattr -d` instructions as interim fix |
| Performance regression shipped | LOW | Revert the optimization commit; this is why benchmarks in CI matter |
| Auto-updater corrupts binary | HIGH | Ship standalone recovery tool or provide manual download link; store previous binary hash for verification |
| Font rebuild causes frame tear | LOW | Increase debounce window; add a "config applying..." overlay that hides the torn frame |
| Config reload ignores multi-pane | LOW | Fix to iterate all sessions/panes; no data loss, just visual inconsistency until restarted |
| Update check blocks startup | LOW | Move to async check; users experience slow startup until fix ships |

## Pitfall-to-Phase Mapping

How roadmap phases should address these pitfalls.

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| UpgradeCode not set | Packaging (MSI) | `grep UpgradeCode wix/main.wxs` shows hardcoded GUID committed to git |
| macOS not notarized | Packaging (DMG) | CI includes `codesign --verify` and notarization; download DMG from releases on clean Mac |
| Config reload cascade/flicker | Config hot-reload | Save config 5 times rapidly in VS Code; terminal shows one "Config reloaded" message, no flicker |
| Config torn reads | Config hot-reload | `ConfigReloaded` variant exists in `AppEvent`; config updated once per event loop iteration |
| Config multi-pane miss | Config hot-reload | Open 3 tabs with 2 splits each; change font_size; verify all 6 panes update |
| Config error drops to defaults | Config hot-reload | Introduce typo in config.toml; verify old config preserved, error shown in status bar |
| Shell config affects running sessions | Config hot-reload | Change `shell = "fish"` while bash is running; existing session stays bash, new tab opens fish |
| Auto-update blocks startup | Auto-update | Measure cold start with and without network; difference < 10ms |
| Auto-update crashes on Windows | Auto-update | Start Glass, trigger update, verify Glass doesn't crash; verify cleanup on next launch |
| Profiling without baselines | Performance | `cargo bench` produces reproducible numbers; CI job fails if metrics regress > 5% |
| Installer missing PATH | Packaging (MSI) | Install MSI, open cmd.exe, run `glass --version`; it works |
| Linux missing desktop integration | Packaging (Linux) | Install .deb, verify Glass appears in GNOME/KDE launcher with icon |

## Sources

- Glass codebase analysis: `crates/glass_core/src/config.rs` (config struct with 6 sections, load-once pattern), `src/main.rs` (15+ config access sites, line 156 ownership, line 1935 load), `crates/glass_core/src/event.rs` (AppEvent enum, EventLoopProxy integration)
- [cargo-wix](https://github.com/volks73/cargo-wix) -- WiX template generation and MSI building for Rust
- [self_update crate](https://github.com/jaemk/self_update) -- Rust self-update library for GitHub releases
- [Microsoft: Changing the Product Code](https://learn.microsoft.com/en-us/windows/win32/msi/changing-the-product-code) -- MSI UpgradeCode/ProductCode semantics
- [notify-debouncer-mini](https://crates.io/crates/notify-debouncer-mini) -- Debounced file watching for notify crate
- [File watcher debouncing in Rust](https://oneuptime.com/blog/post/2026-01-25-file-watcher-debouncing-rust/view) -- Debounce patterns, atomic counter approach
- [apple-codesign (rcodesign)](https://gregoryszorc.com/blog/2022/08/08/achieving-a-completely-open-source-implementation-of-apple-code-signing-and-notarization/) -- Pure-Rust macOS code signing from any platform
- [wgpu-profiler](https://github.com/Wumpf/wgpu-profiler) -- GPU timer query profiling for wgpu
- [Alacritty config hot-reload](https://alacritty.org/config-alacritty.html) -- Reference for terminal config live reload behavior (most settings reload, fonts require restart)
- [Advanced Installer: MSI Upgrades](https://www.advancedinstaller.com/application-packaging-training/msi-advanced/ebook/msi-upgrades-and-patches.html) -- Major vs minor upgrade semantics
- [Packaging Rust for end users](https://rust-cli.github.io/book/tutorial/packaging.html) -- Official Rust CLI book packaging guide
- [cargo-bundle](https://github.com/burtonageo/cargo-bundle) -- Cross-platform app bundle creation

---
*Pitfalls research for: Glass v2.1 -- Packaging, Auto-Update, Config Hot-Reload, Performance*
*Researched: 2026-03-07*

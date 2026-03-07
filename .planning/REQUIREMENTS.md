# Requirements: Glass

**Defined:** 2026-03-07
**Core Value:** A terminal that looks and feels normal but passively watches, indexes, and snapshots everything -- surfacing intelligence only when you need it.

## v2.1 Requirements

Requirements for Packaging & Polish milestone. Each maps to roadmap phases.

### Performance

- [x] **PERF-01**: Automated criterion benchmarks for cold start, input latency, and idle memory
- [x] **PERF-02**: Tracing instrumentation on hot paths (PTY read, render loop, event dispatch)
- [x] **PERF-03**: Performance optimization pass based on profiling results (startup time, memory, rendering throughput)

### Configuration

- [x] **CONF-01**: Config validation with actionable error messages on malformed config.toml
- [x] **CONF-02**: Config hot-reload watching config.toml for changes and applying without restart
- [x] **CONF-03**: In-terminal error overlay displaying config parse errors instead of silent failure

### Packaging

- [x] **PKG-01**: MSI installer for Windows via cargo-wix with stable UpgradeCode
- [x] **PKG-02**: DMG bundle for macOS with proper Info.plist and app structure
- [x] **PKG-03**: .deb package for Linux (Debian/Ubuntu)
- [x] **PKG-04**: GitHub Releases CI workflow building and publishing installers on tag
- [x] **PKG-05**: Winget package manifest for Windows package manager
- [x] **PKG-06**: Homebrew cask formula for macOS package manager

### Auto-Update

- [x] **UPDT-01**: Background update check on startup against GitHub Releases
- [x] **UPDT-02**: Status bar notification when update is available
- [x] **UPDT-03**: One-click update apply (MSI upgrade on Windows, DMG replacement on macOS, notify on Linux)

### Documentation

- [x] **DOCS-01**: mdBook documentation site with feature guides (undo, pipes, MCP, search, config)
- [x] **DOCS-02**: GitHub Pages deployment for docs site
- [x] **DOCS-03**: README overhaul with screenshots, install instructions, feature overview, and badges

## Future Requirements

Deferred to future milestones.

### Packaging

- **PKG-F01**: AppImage for Linux (portable format)
- **PKG-F02**: RPM package for Fedora/RHEL
- **PKG-F03**: Flatpak (risky for terminal emulators due to sandbox restrictions on PTY/filesystem)
- **PKG-F04**: macOS code signing and notarization
- **PKG-F05**: Windows code signing (SmartScreen)

### Performance

- **PERF-F01**: CI performance regression detection (automated benchmark comparison on PRs)
- **PERF-F02**: GPU profiling with wgpu-profiler

## Out of Scope

| Feature | Reason |
|---------|--------|
| Snap package | Snap sandbox conflicts with PTY access and filesystem operations |
| Flatpak (v2.1) | Portal API restrictions on terminal emulator core operations; needs runtime investigation |
| Built-in theme engine | One dark/one light theme sufficient; theme marketplace explicitly out of scope |
| Cloud-based update server | GitHub Releases sufficient; avoids hosting costs and complexity |
| Automatic silent updates | Users must consent to updates; no silent binary replacement |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| PERF-01 | Phase 26 | Complete |
| PERF-02 | Phase 26 | Complete |
| PERF-03 | Phase 26 | Complete |
| CONF-01 | Phase 27 | Complete |
| CONF-02 | Phase 27 | Complete |
| CONF-03 | Phase 27 | Complete |
| PKG-01 | Phase 28 | Complete |
| PKG-02 | Phase 28 | Complete |
| PKG-03 | Phase 28 | Complete |
| PKG-04 | Phase 28 | Complete |
| PKG-05 | Phase 30 | Complete |
| PKG-06 | Phase 30 | Complete |
| UPDT-01 | Phase 29 | Complete |
| UPDT-02 | Phase 29 | Complete |
| UPDT-03 | Phase 29 | Complete |
| DOCS-01 | Phase 30 | Complete |
| DOCS-02 | Phase 30 | Complete |
| DOCS-03 | Phase 30 | Complete |

**Coverage:**
- v2.1 requirements: 18 total
- Mapped to phases: 18
- Unmapped: 0

---
*Requirements defined: 2026-03-07*
*Last updated: 2026-03-07 after roadmap creation*

# Roadmap: Glass

## Milestones

- [x] **v1.0 MVP** -- Phases 1-4 (shipped 2026-03-05)
- [x] **v1.1 Structured Scrollback + MCP Server** -- Phases 5-9 (shipped 2026-03-05)
- [x] **v1.2 Command-Level Undo** -- Phases 10-14 (shipped 2026-03-06)
- [x] **v1.3 Pipe Visualization** -- Phases 15-20 (shipped 2026-03-06)
- [x] **v2.0 Cross-Platform & Tabs** -- Phases 21-25 (shipped 2026-03-07)
- [ ] **v2.1 Packaging & Polish** -- Phases 26-30 (in progress)

## Phases

<details>
<summary>v1.0 MVP (Phases 1-4) -- SHIPPED 2026-03-05</summary>

- [x] Phase 1: Scaffold (3/3 plans) -- completed 2026-03-05
- [x] Phase 2: Terminal Core (3/3 plans) -- completed 2026-03-05
- [x] Phase 3: Shell Integration and Block UI (4/4 plans) -- completed 2026-03-05
- [x] Phase 4: Configuration and Performance (2/2 plans) -- completed 2026-03-05

</details>

<details>
<summary>v1.1 Structured Scrollback + MCP Server (Phases 5-9) -- SHIPPED 2026-03-05</summary>

- [x] Phase 5: History Database Foundation (2/2 plans) -- completed 2026-03-05
- [x] Phase 6: Output Capture + Writer Integration (4/4 plans) -- completed 2026-03-05
- [x] Phase 7: CLI Query Interface (2/2 plans) -- completed 2026-03-05
- [x] Phase 8: Search Overlay (2/2 plans) -- completed 2026-03-05
- [x] Phase 9: MCP Server (2/2 plans) -- completed 2026-03-05

</details>

<details>
<summary>v1.2 Command-Level Undo (Phases 10-14) -- SHIPPED 2026-03-06</summary>

- [x] Phase 10: Content Store + DB Schema (2/2 plans) -- completed 2026-03-05
- [x] Phase 11: Command Parser (2/2 plans) -- completed 2026-03-05
- [x] Phase 12: FS Watcher Engine (2/2 plans) -- completed 2026-03-06
- [x] Phase 13: Integration + Undo Engine (4/4 plans) -- completed 2026-03-06
- [x] Phase 14: UI + CLI + MCP + Pruning (3/3 plans) -- completed 2026-03-06

</details>

<details>
<summary>v1.3 Pipe Visualization (Phases 15-20) -- SHIPPED 2026-03-06</summary>

- [x] Phase 15: Pipe Parsing Core (2/2 plans) -- completed 2026-03-06
- [x] Phase 16: Shell Capture + Terminal Transport (3/3 plans) -- completed 2026-03-06
- [x] Phase 17: Pipeline UI (2/2 plans) -- completed 2026-03-06
- [x] Phase 18: Storage + Retention (1/1 plan) -- completed 2026-03-06
- [x] Phase 19: MCP + Config + Polish (1/1 plan) -- completed 2026-03-06
- [x] Phase 20: Config Gate + Dead Code Cleanup (2/2 plans) -- completed 2026-03-06

</details>

<details>
<summary>v2.0 Cross-Platform & Tabs (Phases 21-25) -- SHIPPED 2026-03-07</summary>

- [x] Phase 21: Session Extraction & Platform Foundation (3/3 plans) -- completed 2026-03-06
- [x] Phase 22: Cross-Platform Validation (2/2 plans) -- completed 2026-03-07
- [x] Phase 23: Tabs (3/3 plans) -- completed 2026-03-07
- [x] Phase 24: Split Panes (3/3 plans) -- completed 2026-03-07
- [x] Phase 25: TerminalExit Multi-Pane Fix (1/1 plan) -- completed 2026-03-07

</details>

### v2.1 Packaging & Polish (In Progress)

**Milestone Goal:** Production-ready distribution, performance tuning, config polish, and public documentation across all three platforms.

- [x] **Phase 26: Performance Profiling & Optimization** - Establish baselines, instrument hot paths, and optimize based on data (completed 2026-03-07)
- [x] **Phase 27: Config Validation & Hot-Reload** - Config error reporting and live config changes without restart (completed 2026-03-07)
- [x] **Phase 28: Platform Packaging & CI Release** - Platform-native installers and automated release workflow (completed 2026-03-07)
- [x] **Phase 29: Auto-Update** - Background update checking and one-click update from GitHub Releases (completed 2026-03-07)
- [ ] **Phase 30: Documentation & Distribution** - Public docs site, README overhaul, and package manager listings

## Phase Details

### Phase 26: Performance Profiling & Optimization
**Goal**: Users benefit from a measurably fast terminal with documented performance characteristics
**Depends on**: Phase 25 (v2.0 complete -- profile the finished product)
**Requirements**: PERF-01, PERF-02, PERF-03
**Success Criteria** (what must be TRUE):
  1. Running `cargo bench` produces statistical benchmarks for cold start time, input-to-render latency, and idle memory usage
  2. Enabling the `perf` cargo feature produces a flamegraph/trace file showing time spent in PTY read, render loop, and event dispatch
  3. Cold start, input latency, and idle memory meet or exceed documented targets (<500ms, <5ms, <120MB) after optimization
  4. Performance baseline numbers are recorded and committed for future regression comparison
**Plans:** 2/2 plans complete

Plans:
- [ ] 26-01-PLAN.md -- Criterion benchmark infrastructure and feature-gated tracing instrumentation
- [ ] 26-02-PLAN.md -- Optimization pass and PERFORMANCE.md baseline documentation

### Phase 27: Config Validation & Hot-Reload
**Goal**: Users get immediate feedback on config errors and see config changes applied live without restarting Glass
**Depends on**: Phase 26 (stabilize core before modifying glass_core shared dependency)
**Requirements**: CONF-01, CONF-02, CONF-03
**Success Criteria** (what must be TRUE):
  1. A malformed config.toml produces a specific, actionable error message telling the user what is wrong and where
  2. Editing font_family or font_size in config.toml while Glass is running applies the change to all open panes within 1 second
  3. A config parse error during hot-reload displays an in-terminal overlay instead of silently failing or crashing
  4. Non-visual config changes (history thresholds, snapshot settings) are applied without triggering a font rebuild
**Plans:** 2/2 plans complete

Plans:
- [ ] 27-01-PLAN.md -- Config validation with structured errors, config diffing, and notify dependency
- [ ] 27-02-PLAN.md -- Config watcher, font hot-reload, error overlay, and end-to-end wiring

### Phase 28: Platform Packaging & CI Release
**Goal**: Users on Windows, macOS, and Linux can install Glass through platform-native installers downloaded from GitHub Releases
**Depends on**: Phase 27 (stabilize runtime code before packaging)
**Requirements**: PKG-01, PKG-02, PKG-03, PKG-04
**Success Criteria** (what must be TRUE):
  1. Running the MSI installer on Windows installs Glass to Program Files and adds it to PATH
  2. Opening the DMG on macOS provides a drag-to-Applications bundle that launches correctly
  3. Installing the .deb package on Ubuntu/Debian makes `glass` available system-wide
  4. Pushing a `v*` git tag triggers a CI workflow that builds all three installers and publishes them as GitHub Release assets
**Plans:** 2/2 plans complete

Plans:
- [ ] 28-01-PLAN.md -- Platform packaging configs (WiX MSI, macOS DMG bundle, Linux deb metadata)
- [ ] 28-02-PLAN.md -- GitHub Actions release workflow with version verification

### Phase 29: Auto-Update
**Goal**: Users are notified of new versions and can update with minimal friction
**Depends on**: Phase 28 (needs GitHub Releases with downloadable artifacts)
**Requirements**: UPDT-01, UPDT-02, UPDT-03
**Success Criteria** (what must be TRUE):
  1. Glass checks GitHub Releases for newer versions in the background on startup without blocking the terminal
  2. When a newer version exists, the status bar shows a visible notification with the available version number
  3. On Windows, the user can trigger an MSI-based upgrade from the notification; on macOS, a DMG download; on Linux, a notification with instructions
**Plans:** 2/2 plans complete

Plans:
- [ ] 29-01-PLAN.md -- Core updater module with version checking, asset selection, and apply logic
- [ ] 29-02-PLAN.md -- Status bar notification UI and main.rs wiring

### Phase 30: Documentation & Distribution
**Goal**: New users can discover, install, learn, and configure Glass through public documentation and package managers
**Depends on**: Phase 29 (document the complete feature set; installer URLs needed for package manifests)
**Requirements**: DOCS-01, DOCS-02, DOCS-03, PKG-05, PKG-06
**Success Criteria** (what must be TRUE):
  1. A public documentation site covers installation, configuration reference, shell integration setup, undo system, pipe inspection, MCP server usage, and search
  2. The docs site is deployed to GitHub Pages and accessible at the project URL
  3. The GitHub README includes screenshots, install instructions for all three platforms, feature overview, and CI badges
  4. Running `winget install glass` on Windows installs the latest release
  5. Running `brew install --cask glass` on macOS installs the latest release
**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 26 -> 27 -> 28 -> 29 -> 30

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. Scaffold | v1.0 | 3/3 | Complete | 2026-03-05 |
| 2. Terminal Core | v1.0 | 3/3 | Complete | 2026-03-05 |
| 3. Shell Integration and Block UI | v1.0 | 4/4 | Complete | 2026-03-05 |
| 4. Configuration and Performance | v1.0 | 2/2 | Complete | 2026-03-05 |
| 5. History Database Foundation | v1.1 | 2/2 | Complete | 2026-03-05 |
| 6. Output Capture + Writer Integration | v1.1 | 4/4 | Complete | 2026-03-05 |
| 7. CLI Query Interface | v1.1 | 2/2 | Complete | 2026-03-05 |
| 8. Search Overlay | v1.1 | 2/2 | Complete | 2026-03-05 |
| 9. MCP Server | v1.1 | 2/2 | Complete | 2026-03-05 |
| 10. Content Store + DB Schema | v1.2 | 2/2 | Complete | 2026-03-05 |
| 11. Command Parser | v1.2 | 2/2 | Complete | 2026-03-05 |
| 12. FS Watcher Engine | v1.2 | 2/2 | Complete | 2026-03-06 |
| 13. Integration + Undo Engine | v1.2 | 4/4 | Complete | 2026-03-06 |
| 14. UI + CLI + MCP + Pruning | v1.2 | 3/3 | Complete | 2026-03-06 |
| 15. Pipe Parsing Core | v1.3 | 2/2 | Complete | 2026-03-06 |
| 16. Shell Capture + Terminal Transport | v1.3 | 3/3 | Complete | 2026-03-06 |
| 17. Pipeline UI | v1.3 | 2/2 | Complete | 2026-03-06 |
| 18. Storage + Retention | v1.3 | 1/1 | Complete | 2026-03-06 |
| 19. MCP + Config + Polish | v1.3 | 1/1 | Complete | 2026-03-06 |
| 20. Config Gate + Dead Code Cleanup | v1.3 | 2/2 | Complete | 2026-03-06 |
| 21. Session Extraction & Platform Foundation | v2.0 | 3/3 | Complete | 2026-03-06 |
| 22. Cross-Platform Validation | v2.0 | 2/2 | Complete | 2026-03-07 |
| 23. Tabs | v2.0 | 3/3 | Complete | 2026-03-07 |
| 24. Split Panes | v2.0 | 3/3 | Complete | 2026-03-07 |
| 25. TerminalExit Multi-Pane Fix | v2.0 | 1/1 | Complete | 2026-03-07 |
| 26. Performance Profiling & Optimization | v2.1 | 2/2 | Complete | 2026-03-07 |
| 27. Config Validation & Hot-Reload | v2.1 | 2/2 | Complete | 2026-03-07 |
| 28. Platform Packaging & CI Release | v2.1 | 2/2 | Complete | 2026-03-07 |
| 29. Auto-Update | 2/2 | Complete    | 2026-03-07 | - |
| 30. Documentation & Distribution | v2.1 | 0/? | Not started | - |

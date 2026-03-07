# Project Retrospective

*A living document updated after each milestone. Lessons feed forward into future planning.*

## Milestone: v2.1 -- Packaging & Polish

**Shipped:** 2026-03-07
**Phases:** 5 | **Plans:** 11 | **Sessions:** ~2

### What Was Built
- Criterion benchmark infrastructure with feature-gated tracing instrumentation and PERFORMANCE.md baselines (522ms cold start, 3-7us latency, 86MB memory)
- Config validation with structured errors (line/column/snippet from toml span() API) and load_validated() replacing unwrap-style config loading
- Config hot-reload via notify file watcher: ConfigReloaded event, font rebuild (update_font), error overlay renderer
- Platform-native installers: Windows MSI (cargo-wix with stable UpgradeCode), macOS DMG (hdiutil with Info.plist), Linux .deb (cargo-deb)
- GitHub Actions release workflow with parallel cross-platform builds triggered on v* tag push
- Background auto-update checker (ureq + semver against GitHub Releases) with status bar notification and Ctrl+Shift+U update apply
- mdBook documentation site (16 pages) with GitHub Pages deployment, project README with badges
- Winget multi-file manifest (v1.6.0) and Homebrew cask formula for package manager distribution

### What Worked
- Config watcher reused notify 8.2 already in workspace (from FS watcher in v1.2) -- zero new dependency friction
- Update checker followed config_watcher pattern (named thread + EventLoopProxy) -- consistent architecture
- Error overlay followed SearchOverlayRenderer pattern -- architectural consistency across overlay types
- Release workflow parallel jobs with no inter-job dependencies worked cleanly with softprops/action-gh-release
- Audit found only low-severity issues (doc URL hardcoding, DMG filename pattern) -- no blockers
- Recording cold start honestly at 522ms (vs 500ms target) set a good precedent for transparent baselines

### What Was Inefficient
- Installation docs hardcoded `anthropics/glass` as repo owner instead of actual user -- caught by audit but should have been parameterized from the start
- Package manager manifests contain multiple placeholders (<GITHUB_USER>, <SHA256>) requiring manual substitution at publish time -- could have been templated
- README screenshot placeholder left in -- documentation published without visual content
- Nyquist validation still partial across all 5 phases -- 6th consecutive milestone with this gap

### Patterns Established
- Feature-gated instrumentation: cfg_attr(feature = "perf") for zero-overhead tracing in release builds
- Watch parent directory (not config file) to handle atomic saves from editors (vim, VSCode)
- Box<T> in enum variants to keep AppEvent size reasonable when carrying large config structs
- Version verification in CI release jobs to prevent Cargo.toml/tag mismatch
- tempfile with mem::forget for platform-specific download-then-execute flows (Windows MSI)
- Center-text status bar rendering for transient notifications

### Key Lessons
1. Reuse existing patterns (config_watcher, SearchOverlay) when adding new features -- consistency reduces bugs and review cost
2. Parameterize repository-specific values from day one -- hardcoded owner/URL breaks on fork or transfer
3. Package manager manifests should use build-time substitution, not manual placeholders
4. Performance targets should include measurement methodology alongside numbers -- PERFORMANCE.md as single source of truth
5. Feature-gated instrumentation is the right pattern for profiling infrastructure -- zero cost when disabled

### Cost Observations
- Model mix: predominantly opus for execution, balanced profile
- Sessions: ~2 sessions in 1 day
- Notable: 11 plans in ~23 min, averaging ~3 min/plan (fastest per-plan velocity; packaging/docs plans are largely config/content generation)

---

## Milestone: v2.0 -- Cross-Platform & Tabs

**Shipped:** 2026-03-07
**Phases:** 5 | **Plans:** 12 | **Sessions:** ~3

### What Was Built
- glass_mux crate with SessionMux multiplexer, Session struct (15 fields from WindowContext), and platform cfg-gated helpers
- SessionId newtype routing through all AppEvent variants and EventProxy for multi-session event dispatch
- Cross-platform compilation (Windows/macOS/Linux) with 3-platform CI matrix, platform-aware shell detection and font defaults
- Shell integration auto-injection for bash, zsh, fish, and PowerShell via find_shell_integration()
- Tab system with GPU-rendered tab bar (TabBarRenderer), Ctrl+Shift+T/W shortcuts, mouse click activation, CWD inheritance
- Binary tree split pane layout engine (SplitTree) with compute_layout, remove_leaf, find_neighbor, resize_ratio (26 TDD tests)
- Per-pane scissor-clipped rendering with viewport offsets, focus accent borders, and divider drawing
- Pane-aware TerminalExit handler routing PTY exit to close_pane or close_tab based on pane count

### What Worked
- SessionMux extraction (Phase 21) as separate crate kept glass_terminal untouched -- clean boundary
- Phase 25 gap closure pattern (again!) caught TerminalExit pane-awareness gap that audit identified
- Binary tree TDD approach (26 tests before integration) made split pane rendering integration smooth
- Single-pane rendering path preserved for zero regression risk -- multi-pane only activates when splits exist
- Platform cfg-gating compiled correctly on first try for all 3 platforms (CI validated)
- fish shell integration gap caught during re-audit and fixed before milestone completion

### What Was Inefficient
- default_shell_program() duplicated in pty.rs and platform.rs -- glass_terminal can't depend on glass_mux (circular), so inline copy was necessary but is tech debt
- config_dir() and data_dir() built speculatively in platform.rs but never consumed -- orphaned API
- ScaleFactorChanged handler is log-only -- FrameRenderer lacks dynamic DPI recalculation
- glass.fish not created during Phase 22 despite code referencing it -- caught only on re-audit
- ROADMAP heading said "Phases 21-24" even though Phase 25 existed -- heading accuracy drifted

### Patterns Established
- SessionId newtype with Copy + Hash for zero-cost routing through event dispatch
- SplitNode binary tree with in-place split_leaf mutation and first_leaf fallback on close
- Viewport offset + TextBounds clipping for multi-pane rendering (not wgpu scissor rects)
- Pairwise gap detection for divider rect computation between adjacent pane viewports
- fish event handlers (fish_prompt, fish_preexec) for shell integration without precmd/preexec

### Key Lessons
1. Gap closure is now a 5-milestone proven pattern -- Phase 25 and glass.fish fix both caught by audit
2. Asset existence should be verified alongside code paths -- glass.fish code existed but the file didn't
3. Binary tree TDD before integration is the right split pane strategy -- 26 tests gave confidence for the rendering work
4. Viewport offset rendering is simpler than GPU scissor rects for terminal panes
5. Platform cfg-gating works well at compile time -- prefer cfg attributes over runtime checks

### Cost Observations
- Model mix: predominantly opus for execution, balanced profile
- Sessions: ~3 sessions across 2 days
- Notable: 12 plans in ~23 min averaged (~4 min/plan), fastest milestone yet due to well-established patterns and smaller individual plan scope

---

## Milestone: v1.2 -- Command-Level Undo

**Shipped:** 2026-03-06
**Phases:** 5 | **Plans:** 13 | **Sessions:** ~4

### What Was Built
- Content-addressed blob store (glass_snapshot crate) with BLAKE3 hashing, deduplication, and SQLite snapshot metadata
- POSIX + PowerShell command parser with whitelist dispatch identifying file targets for pre-exec snapshot
- Filesystem watcher engine with .glassignore pattern matching and notify-based event monitoring
- UndoEngine with conflict detection, file restoration, confidence tracking, and one-shot semantics
- [undo] label on command blocks, Ctrl+Shift+Z keybinding, CLI undo subcommand, GlassUndo + GlassFileDiff MCP tools
- Storage pruning with age/count/orphan cleanup on startup background thread

### What Worked
- Dual mechanism design (pre-exec parser + FS watcher) gave honest confidence levels without over-promising
- Clean phase layering: storage foundation (10) -> independent parser (11) + watcher (12) -> integration (13) -> UI/CLI/MCP (14)
- Parallel phase execution for 11+12 (both depended only on 10) saved time
- Single glass_snapshot crate API surface kept consumers (main.rs, glass_mcp) clean
- Gap closure plan (13-04) caught confidence display and config gating gaps before moving to Phase 14
- Re-verification pattern (Phase 13 verified twice: before and after gap closure) caught real issues

### What Was Inefficient
- Phases 11 and 12 could have been more tightly integrated during planning -- watcher events needed to map to parser confidence levels, which required Phase 13 to reconcile
- VALIDATION.md files created as drafts for all phases but never completed -- Nyquist validation remains a persistent gap across all milestones
- 13 plans across 5 phases for a single feature (undo) may have been over-decomposed -- some plans had very narrow scope

### Patterns Established
- SnapshotStore coordinator pattern (BlobStore + SnapshotDb behind single API)
- Whitelist dispatch command parser with per-command extractors
- HashMap deduplication for watcher events (keep last event per path)
- Safety margin pattern for pruning (protect N most recent from age-based deletion)
- Config gating pattern: Option<Section> with enabled field, absent = default enabled
- Per-request store opening in spawn_blocking for !Send types in async MCP handlers

### Key Lessons
1. Dual mechanism designs (parser + watcher) are more honest than trying to make one approach perfect
2. Gap closure plans are a reliable pattern -- plan for them rather than hoping everything is wired first time
3. Nyquist validation needs to be integrated into phase execution, not deferred -- it's been skipped across 3 milestones now
4. One-shot undo is the right V1 semantics -- simpler mental model for users before adding undo chains
5. Separate SQLite databases (history.db, snapshots.db) enable independent lifecycle management

### Cost Observations
- Model mix: predominantly opus for execution, balanced profile
- Sessions: ~4 sessions across 2 days
- Notable: 13 plans in ~6 hours, averaging ~28 min/plan (slower than v1.1 due to complex cross-phase integration)

---

## Milestone: v1.1 -- Structured Scrollback + MCP Server

**Shipped:** 2026-03-05
**Phases:** 5 | **Plans:** 12 | **Sessions:** ~5

### What Was Built
- SQLite history database (glass_history crate) with FTS5 search, per-project storage, retention policies
- PTY output capture pipeline with alt-screen detection, binary filtering, ANSI stripping, schema migration
- CLI query interface (`glass history search/list`) with combined filters and formatted table output
- Search overlay (Ctrl+Shift+F) with live incremental search, debounce, and scroll-to-block navigation
- MCP server (glass_mcp crate) with GlassHistory and GlassContext tools over stdio JSON-RPC
- Clap subcommand routing and display_offset scrollback fix

### What Worked
- Gap closure pattern (Phase 6 plan 04) effectively caught deferred wiring work before moving on
- Cross-phase integration was clean -- all 11 glass_history exports wired correctly by downstream phases
- rmcp SDK for MCP server eliminated JSON-RPC boilerplate and provided reliable stdio transport
- Epoch timestamp matching for scroll-to-block was more reliable than index-position heuristics
- PRAGMA user_version migration pattern scaled cleanly for v0->v1 schema change

### What Was Inefficient
- Research documentation for rmcp was based on v0.11; actual v1.1.0 API differed significantly -- required runtime discovery
- Phase 6 needed 4 plans (including gap closure) where 3 were originally scoped -- deferred DB wiring created a gap
- Command text extraction was deferred in Phase 6 then solved ad-hoc in Phase 8 -- could have been planned earlier
- Roadmap checkbox state drifted again (Phases 6, 8, 9 showed incomplete in ROADMAP.md despite being done)

### Patterns Established
- OutputBuffer accumulate-then-flush pattern for PTY output capture
- AppEvent-based cross-thread communication (PTY thread -> main thread -> DB)
- Alt-screen detection via raw byte scanning (avoids locking TermMode)
- Content FTS5 tables (not external content) for simpler, safer full-text search
- McpTestClient with reader thread + mpsc channel for non-blocking process testing
- SearchOverlay state module with debounced search execution via request_redraw polling

### Key Lessons
1. Always verify SDK versions against installed crate, not documentation -- rmcp 0.11 vs 1.1.0 had breaking API changes
2. Deferred wiring creates gaps -- better to wire end-to-end in the same phase than split across phases
3. Roadmap checkbox state needs automated verification -- manual updates drift consistently
4. Content FTS5 tables are simpler than external content tables for most use cases
5. Epoch timestamps are more reliable than index positions for cross-system record matching

### Cost Observations
- Model mix: predominantly opus for execution, balanced profile
- Sessions: ~5 sessions across 1 day
- Notable: 12 plans in ~4.5 hours, averaging 20 min/plan (2x slower than v1.0 due to larger crate integration)

---

## Milestone: v1.0 -- MVP

**Shipped:** 2026-03-05
**Phases:** 4 | **Plans:** 12 | **Sessions:** ~4

### What Was Built
- GPU-accelerated terminal emulator with wgpu DX12 rendering pipeline
- Full VTE terminal: 24-bit color, keyboard modifiers, clipboard, scrollback, bracketed paste
- Shell integration: OscScanner, BlockManager, StatusState with OSC 133/7 parsing
- Block UI: visual command blocks with exit code badges and duration labels
- Status bar with CWD and git branch/dirty count
- TOML configuration and performance-tuned cold start (360ms)

### What Worked
- TDD approach for shell integration layer (27 tests) caught edge cases early
- Exact version pinning (alacritty_terminal =0.25.1) avoided semver surprises
- Custom PTY read loop decision enabled clean OscScanner integration
- Parallel GPU + font init optimization yielded significant cold start improvement
- Wave-based plan execution kept phases focused and independently verifiable

### What Was Inefficient
- Research documentation sometimes diverged from actual API (winit can_create_surfaces vs resumed(), wgpu request_device signature) -- required runtime discovery
- Performance targets (200ms cold start, 50MB memory) were set without measuring hardware baselines -- had to revise mid-milestone
- Phase 3 roadmap showed 3/4 plans but all 4 were actually completed -- roadmap checkbox state drifted

### Patterns Established
- GridSnapshot lock-minimizing pattern for PTY reader/renderer coordination
- Two-phase overlay buffer pattern for cosmic_text borrow-checker safety
- ShellEvent enum mirroring in glass_core to avoid circular crate dependencies
- ASCII badge text (OK/X) over Unicode for font compatibility
- DX12 forced backend on Windows (33% faster than Vulkan auto-select)

### Key Lessons
1. Always measure hardware baselines before setting performance targets -- GPU driver init and memory are non-negotiable floors
2. Pin exact crate versions for unstable APIs -- alacritty_terminal has no semver guarantee
3. Verify API surfaces against installed crate source, not documentation -- docs can be wrong or outdated
4. Custom PTY read loops enable features that library abstractions prevent (OscScanner pre-scanning)
5. Per-line cosmic_text Buffers with set_rich_text are the right granularity for terminal rendering

### Cost Observations
- Model mix: predominantly opus for execution, balanced profile
- Sessions: ~4 sessions across 1 day
- Notable: 12 plans in ~1.8 hours total execution time, averaging 9 min/plan

---

## Milestone: v1.3 -- Pipe Visualization

**Shipped:** 2026-03-06
**Phases:** 6 | **Plans:** 11 | **Sessions:** ~3

### What Was Built
- Byte-level pipe parser (glass_pipes crate) with shell quoting awareness, TTY detection, opt-out flag, and buffer sampling
- Shell capture via tee rewriting (bash) and Tee-Object (PowerShell) with OSC 133;S/P protocol transport
- Multi-row pipeline UI with auto-expand on failure, click/keyboard stage expansion, and sampled output rendering
- pipe_stages DB table with schema v2 migration, FK cascade, and retention policy integration
- GlassPipeInspect MCP tool and GlassContext pipeline stats for AI integration
- Three-layer pipes.enabled config gate (PTY env var, shell scripts, main.rs event processing)

### What Worked
- Clean phase layering: parsing core (15) -> transport (16) -> UI (17) + storage (18) -> MCP/config (19) -> gap closure (20)
- Audit-driven gap closure: `/gsd:audit-milestone` identified config gate and dead code gaps, Phase 20 closed them cleanly
- Re-audit after gap closure confirmed all 16/16 requirements satisfied with no remaining gaps
- OSC protocol reuse (133;S/P extending existing 133;A/B/C/D) kept shell integration clean
- Single glass_pipes crate for all parsing types kept downstream consumers (glass_core, glass_terminal, glass_mcp) clean

### What Was Inefficient
- classify.rs (TTY detection, opt-out) built in Phase 15 then entirely removed in Phase 20 -- the runtime never consumed classification results, only the parser's stage splitting
- PipeStage.is_tty field populated but never read at runtime -- vestigial from removed classify module
- Phase 20 was added post-audit; could have been caught during Phase 19 planning if config gating was scoped earlier
- VALIDATION.md still only completed for Phase 17 (human-verified); Nyquist validation remains partial for 4th consecutive milestone

### Patterns Established
- OSC protocol extension pattern (133;S/P) for shell-to-terminal structured data transport
- Three-layer config gating: env var IPC for shell scripts, parameter threading for Rust, event filtering in main loop
- FinalizedBuffer-to-row conversion at crate boundary to avoid coupling (glass_pipes/glass_history)
- Pipeline overlay rendering (not grid row insertion) for sub-block UI elements
- Dead code removal as explicit audit-driven phase rather than ad-hoc cleanup

### Key Lessons
1. Audit-driven gap closure is now a proven 3-milestone pattern -- always run audit before marking complete
2. Build only what the runtime consumes -- classify.rs was speculative infrastructure that got removed entirely
3. Config gating needs to be planned from Phase 1, not bolted on after audit
4. Three-layer gating (env var + code + event filter) is the right pattern when crossing process boundaries
5. Schema migrations with hardcoded version numbers are more reliable than const-based versioning

### Cost Observations
- Model mix: predominantly opus for execution, balanced profile
- Sessions: ~3 sessions across 2 days
- Notable: 11 plans in ~2 hours, averaging ~11 min/plan (faster than v1.1/v1.2 due to well-established patterns)

---

## Cross-Milestone Trends

### Process Evolution

| Milestone | Sessions | Phases | Key Change |
|-----------|----------|--------|------------|
| v1.0 | ~4 | 4 | Established GSD workflow with TDD, wave execution |
| v1.1 | ~5 | 5 | Added gap closure pattern, cross-crate integration testing |
| v1.2 | ~4 | 5 | Dual mechanism design, re-verification after gap closure, parallel phase execution |
| v1.3 | ~3 | 6 | Audit-driven gap closure phase, dead code removal as explicit phase, three-layer config gating |
| v2.0 | ~3 | 5 | Multi-session architecture, binary tree TDD, asset existence verification |
| v2.1 | ~2 | 5 | Feature-gated instrumentation, pattern reuse (watcher/overlay), honest baselines |

### Cumulative Quality

| Milestone | Tests | Coverage | Tech Debt Items |
|-----------|-------|----------|-----------------|
| v1.0 | 27+ | Partial (Nyquist gaps in phases 2-4) | 3 |
| v1.1 | 88+ (phase 5 alone) | Partial (Nyquist gaps in phases 5-9) | 4 |
| v1.2 | 234+ (full workspace) | Partial (Nyquist gaps in phases 10-14) | 2 |
| v1.3 | 376+ (full workspace) | Partial (Phase 17 verified, 5 phases partial) | 2 |
| v2.0 | 436 (full workspace) | Partial (Nyquist partial across phases 21-25) | 4 |
| v2.1 | 436 (full workspace) | Partial (Nyquist partial across phases 26-30) | 6 |

### Top Lessons (Verified Across Milestones)

1. Always verify API/SDK versions against installed source, not documentation -- confirmed in v1.0 (winit/wgpu), v1.1 (rmcp)
2. Roadmap checkbox state drifts consistently -- needs automated verification
3. Pin exact crate versions for unstable APIs -- confirmed across all milestones
4. Measure hardware/system baselines before setting targets -- GPU floors (v1.0), throughput benchmarks (v1.1)
5. Gap closure plans are reliable -- confirmed in v1.1, v1.2, v1.3, v2.0; plan for them proactively
6. Nyquist validation is persistently skipped -- 6 milestones with partial coverage; needs workflow integration, not just reminders
7. Audit-driven gap closure is a proven pattern across v1.1, v1.2, v1.3, v2.0, v2.1 -- always run audit before marking milestone complete
8. Speculative infrastructure gets removed -- build only what the runtime consumes (classify.rs in v1.3, config_dir/data_dir in v2.0)
9. Verify asset existence alongside code paths -- code referencing non-existent files passes compilation but fails at runtime (glass.fish in v2.0)
10. Reuse existing architectural patterns when adding new features -- consistency reduces bugs (v2.1: watcher/overlay/event patterns)

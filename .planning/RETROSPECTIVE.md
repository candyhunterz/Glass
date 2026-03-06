# Project Retrospective

*A living document updated after each milestone. Lessons feed forward into future planning.*

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

## Cross-Milestone Trends

### Process Evolution

| Milestone | Sessions | Phases | Key Change |
|-----------|----------|--------|------------|
| v1.0 | ~4 | 4 | Established GSD workflow with TDD, wave execution |
| v1.1 | ~5 | 5 | Added gap closure pattern, cross-crate integration testing |
| v1.2 | ~4 | 5 | Dual mechanism design, re-verification after gap closure, parallel phase execution |

### Cumulative Quality

| Milestone | Tests | Coverage | Tech Debt Items |
|-----------|-------|----------|-----------------|
| v1.0 | 27+ | Partial (Nyquist gaps in phases 2-4) | 3 |
| v1.1 | 88+ (phase 5 alone) | Partial (Nyquist gaps in phases 5-9) | 4 |
| v1.2 | 234+ (full workspace) | Partial (Nyquist gaps in phases 10-14) | 2 |

### Top Lessons (Verified Across Milestones)

1. Always verify API/SDK versions against installed source, not documentation -- confirmed in v1.0 (winit/wgpu), v1.1 (rmcp)
2. Roadmap checkbox state drifts consistently -- needs automated verification
3. Pin exact crate versions for unstable APIs -- confirmed across all milestones
4. Measure hardware/system baselines before setting targets -- GPU floors (v1.0), throughput benchmarks (v1.1)
5. Gap closure plans are reliable -- confirmed in v1.1 (Phase 6) and v1.2 (Phase 13); plan for them proactively
6. Nyquist validation is persistently skipped -- 3 milestones with partial coverage; needs workflow integration, not just reminders

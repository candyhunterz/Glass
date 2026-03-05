# Project Retrospective

*A living document updated after each milestone. Lessons feed forward into future planning.*

## Milestone: v1.0 — MVP

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
- Research documentation sometimes diverged from actual API (winit can_create_surfaces vs resumed(), wgpu request_device signature) — required runtime discovery
- Performance targets (200ms cold start, 50MB memory) were set without measuring hardware baselines — had to revise mid-milestone
- Phase 3 roadmap showed 3/4 plans but all 4 were actually completed — roadmap checkbox state drifted

### Patterns Established
- GridSnapshot lock-minimizing pattern for PTY reader/renderer coordination
- Two-phase overlay buffer pattern for cosmic_text borrow-checker safety
- ShellEvent enum mirroring in glass_core to avoid circular crate dependencies
- ASCII badge text (OK/X) over Unicode for font compatibility
- DX12 forced backend on Windows (33% faster than Vulkan auto-select)

### Key Lessons
1. Always measure hardware baselines before setting performance targets — GPU driver init and memory are non-negotiable floors
2. Pin exact crate versions for unstable APIs — alacritty_terminal has no semver guarantee
3. Verify API surfaces against installed crate source, not documentation — docs can be wrong or outdated
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

### Cumulative Quality

| Milestone | Tests | Coverage | Tech Debt Items |
|-----------|-------|----------|-----------------|
| v1.0 | 27+ | Partial (Nyquist gaps in phases 2-4) | 3 |

### Top Lessons (Verified Across Milestones)

1. Measure hardware baselines before setting performance targets
2. Pin exact versions for crates without semver stability guarantees

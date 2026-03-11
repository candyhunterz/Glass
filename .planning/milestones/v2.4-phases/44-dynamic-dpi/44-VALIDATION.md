---
phase: 44
slug: dynamic-dpi
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-10
---

# Phase 44 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test (cargo test) |
| **Config file** | Cargo.toml workspace |
| **Quick run command** | `cargo test -p glass_renderer -- grid_renderer` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_renderer -- grid_renderer`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 44-01-01 | 01 | 1 | DPI-01 | unit | `cargo test -p glass_renderer -- scale_factor_changes_cell_dimensions` | Wave 0 | pending |
| 44-01-02 | 01 | 1 | DPI-01 | unit | `cargo test -p glass_renderer -- scale_factor_preserves_grid_alignment` | Wave 0 | pending |
| 44-01-03 | 01 | 1 | DPI-01, DPI-02 | integration | ScaleFactorChanged handler implementation | N/A | pending |
| 44-01-04 | 01 | 1 | DPI-02 | manual | Drag window between monitors, verify reflow | N/A | pending |

*Status: pending / green / red / flaky*

---

## Wave 0 Requirements

- [ ] `scale_factor_changes_cell_dimensions` test in grid_renderer.rs -- covers DPI-01
- [ ] `scale_factor_preserves_grid_alignment` test in grid_renderer.rs -- covers DPI-01

*Existing test infrastructure covers framework needs. Only new test stubs required.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| PTY receives correct dimensions after DPI change | DPI-02 | Requires physical multi-monitor setup | Drag Glass window from 1x to 2x display, run `tput cols; tput lines` before and after, verify values change |
| No rendering artifacts after DPI change | DPI-02 | Requires visual inspection | After monitor switch, verify no blurry text, clipped glyphs, or misaligned grid |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

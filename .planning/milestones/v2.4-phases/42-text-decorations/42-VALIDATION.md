---
phase: 42
slug: text-decorations
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-10
---

# Phase 42 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (built-in Rust test framework) |
| **Config file** | Cargo.toml workspace |
| **Quick run command** | `cargo test -p glass_renderer` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_renderer`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 42-01-01 | 01 | 1 | DECO-01 | unit | `cargo test -p glass_renderer -- underline` | ❌ W0 | ⬜ pending |
| 42-01-02 | 01 | 1 | DECO-01 | unit | `cargo test -p glass_renderer -- underline_wide` | ❌ W0 | ⬜ pending |
| 42-01-03 | 01 | 1 | DECO-01 | unit | `cargo test -p glass_renderer -- underline_space` | ❌ W0 | ⬜ pending |
| 42-01-04 | 01 | 1 | DECO-02 | unit | `cargo test -p glass_renderer -- strikeout` | ❌ W0 | ⬜ pending |
| 42-01-05 | 01 | 1 | DECO-02 | unit | `cargo test -p glass_renderer -- strikeout_wide` | ❌ W0 | ⬜ pending |
| 42-01-06 | 01 | 1 | DECO-01+02 | unit | `cargo test -p glass_renderer -- both_decorations` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Tests for `build_decoration_rects` in `grid_renderer.rs` — all 6 test cases above
- No new framework install needed — existing cargo test infrastructure covers everything

*Existing infrastructure covers all phase requirements.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Decorations visually correct at different font sizes | DECO-01, DECO-02 | Visual rendering quality | Change font size in config.toml, verify underline/strikethrough position |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

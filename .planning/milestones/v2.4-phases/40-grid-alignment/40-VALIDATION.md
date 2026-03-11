---
phase: 40
slug: grid-alignment
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-10
---

# Phase 40 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (built-in #[test]) |
| **Config file** | None (uses Cargo default test runner) |
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
| 40-01-01 | 01 | 0 | GRID-01 | unit | `cargo test -p glass_renderer grid_alignment` | ❌ W0 | ⬜ pending |
| 40-01-02 | 01 | 0 | GRID-02 | unit | `cargo test -p glass_renderer cell_height_from_metrics` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_renderer/src/grid_renderer.rs` — add #[cfg(test)] mod tests with:
  - Test that cell_height is derived from font metrics (not 1.2x multiplier)
  - Test that cell_width matches "M" glyph advance width
  - Test that build_cell_buffers produces correct number of buffers (skips spaces and spacers)
  - Test that build_cell_text_areas positions cells at exact grid coordinates
- [ ] Need to create a FontSystem in tests — may require test helper that loads a bundled test font or uses system default

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Box-drawing borders connect seamlessly | GRID-01, GRID-02 | Visual rendering output | Run `vim` or `htop`, verify no vertical gaps between lines |
| No horizontal drift in TUI apps | GRID-01 | Visual rendering output | Open tmux status bar, verify characters stay aligned to grid columns |
| Renders identically to Alacritty | GRID-01, GRID-02 | Cross-application comparison | Compare same font/size in Glass vs Alacritty side-by-side |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

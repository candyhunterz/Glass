---
phase: 41
slug: wide-character-support
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-10
---

# Phase 41 — Validation Strategy

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
| 41-01-01 | 01 | 1 | WIDE-01 | unit | `cargo test -p glass_renderer wide_char_buffer` | ❌ W0 | ⬜ pending |
| 41-01-02 | 01 | 1 | WIDE-01 | unit | `cargo test -p glass_renderer spacer_skipped` | ❌ W0 | ⬜ pending |
| 41-01-03 | 01 | 1 | WIDE-02 | unit | `cargo test -p glass_renderer wide_char_bg_rect` | ❌ W0 | ⬜ pending |
| 41-01-04 | 01 | 1 | WIDE-02 | unit | `cargo test -p glass_renderer wide_char_cursor` | ❌ W0 | ⬜ pending |
| 41-01-05 | 01 | 1 | WIDE-02 | unit | `cargo test -p glass_renderer wide_char_selection` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_renderer/src/grid_renderer.rs` — add unit tests for wide char Buffer creation (2 * cell_width)
- [ ] `crates/glass_renderer/src/grid_renderer.rs` — add unit tests for LEADING_WIDE_CHAR_SPACER skip
- [ ] `crates/glass_renderer/src/grid_renderer.rs` — add unit tests for wide char background rect double-width
- [ ] `crates/glass_renderer/src/grid_renderer.rs` — add unit tests for cursor double-width on WIDE_CHAR
- [ ] Update existing `build_cell_buffers_skips_spaces_and_spacers` test to include LEADING_WIDE_CHAR_SPACER

*Existing infrastructure covers framework needs — only test stubs required.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| CJK text visually renders at correct size | WIDE-01 | GPU rendering requires visual inspection | Run Glass, type `echo 漢字テスト`, verify chars span 2 cells |
| Mixed ASCII/CJK alignment | WIDE-01 | Visual alignment check | Run Glass, type mixed text, verify column alignment |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

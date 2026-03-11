---
phase: 46
slug: tab-bar-controls
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-10
---

# Phase 46 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in (#[cfg(test)] mod tests) + Criterion benches |
| **Config file** | Cargo.toml workspace |
| **Quick run command** | `cargo test -p glass_renderer tab_bar` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_renderer tab_bar`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 46-01-01 | 01 | 1 | TAB-01 | unit | `cargo test -p glass_renderer tab_bar::tests::test_new_tab_button` | ❌ W0 | ⬜ pending |
| 46-01-02 | 01 | 1 | TAB-02 | unit | `cargo test -p glass_renderer tab_bar::tests::test_close_button_hovered` | ❌ W0 | ⬜ pending |
| 46-01-03 | 01 | 1 | TAB-03 | unit | `cargo test -p glass_renderer tab_bar::tests::test_hit_new_tab_button` | ❌ W0 | ⬜ pending |
| 46-01-04 | 01 | 1 | TAB-04 | unit | `cargo test -p glass_renderer tab_bar::tests::test_hit_close_button` | ❌ W0 | ⬜ pending |
| 46-01-05 | 01 | 1 | TAB-05 | unit | `cargo test -p glass_renderer tab_bar::tests::test_min_tab_width` | ❌ W0 | ⬜ pending |
| 46-01-06 | 01 | 1 | TAB-06 | unit | `cargo test -p glass_renderer tab_bar::tests::test_title_truncation_with_close` | ❌ W0 | ⬜ pending |
| 46-01-07 | 01 | 1 | TAB-07 | unit | `cargo test -p glass_renderer tab_bar::tests::test_hit_test_correct_index` | ✅ | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] New test cases for `TabHitResult` enum variants (NewTabButton, CloseButton)
- [ ] New test cases for variable-width tab layout with minimum width clamping
- [ ] New test cases for close button rect positioning within hovered tab
- [ ] Update existing tests to match new `hit_test()` return type (`TabHitResult` enum)

*Existing infrastructure covers framework needs — only new test stubs required.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Hover highlight visual appearance | TAB-02 | Visual rendering quality | Hover over tabs, verify subtle circular highlight behind "x" glyph |
| "+" button visual appearance | TAB-01 | Visual rendering quality | Check "+" button has correct size/position after last tab |
| Tab overflow title truncation | TAB-06 | Visual correctness | Open 10+ tabs, verify titles truncate with "..." |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

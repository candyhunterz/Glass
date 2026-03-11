---
phase: 47
slug: tab-drag-reorder
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-10
---

# Phase 47 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[cfg(test)]` + cargo test |
| **Config file** | None needed |
| **Quick run command** | `cargo test -p glass_mux -p glass_renderer` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_mux -p glass_renderer`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 47-01-01 | 01 | 1 | reorder_tab moves tab | unit | `cargo test -p glass_mux -- reorder` | ❌ W0 | ⬜ pending |
| 47-01-02 | 01 | 1 | reorder_tab adjusts active_tab | unit | `cargo test -p glass_mux -- reorder` | ❌ W0 | ⬜ pending |
| 47-01-03 | 01 | 1 | reorder_tab no-op from==to | unit | `cargo test -p glass_mux -- reorder` | ❌ W0 | ⬜ pending |
| 47-01-04 | 01 | 1 | drag_drop_index correct slot | unit | `cargo test -p glass_renderer -- drag_drop` | ❌ W0 | ⬜ pending |
| 47-01-05 | 01 | 1 | drop indicator renders | unit | `cargo test -p glass_renderer -- drag_indicator` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `reorder_tab` tests in `session_mux.rs` — covers reorder logic and active_tab adjustment
- [ ] `drag_drop_index` test in `tab_bar.rs` — covers drop position calculation
- [ ] `build_tab_rects` with drag indicator test in `tab_bar.rs` — covers visual indicator

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Drag visual feedback smooth | UX quality | Requires visual inspection | Drag a tab, verify smooth motion and insertion indicator |
| Drop at edges works | Edge case | Requires interactive test | Drag tab to leftmost/rightmost position |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

---
phase: 24
slug: split-panes
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-06
---

# Phase 24 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[test]` + `cargo test` |
| **Config file** | Cargo.toml workspace |
| **Quick run command** | `cargo test -p glass_mux` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_mux`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| SPLIT-01 | 01 | 1 | SplitNode tree construction | unit | `cargo test -p glass_mux split_tree` | Needs creation W0 | pending |
| SPLIT-02 | 01 | 1 | compute_layout pixel rects | unit | `cargo test -p glass_mux split_tree::tests::layout` | Needs creation W0 | pending |
| SPLIT-03 | 01 | 1 | Horizontal split width+gap | unit | `cargo test -p glass_mux split_tree::tests::horizontal` | Needs creation W0 | pending |
| SPLIT-04 | 01 | 1 | Vertical split height+gap | unit | `cargo test -p glass_mux split_tree::tests::vertical` | Needs creation W0 | pending |
| SPLIT-05 | 01 | 1 | remove_leaf collapses parent | unit | `cargo test -p glass_mux split_tree::tests::remove` | Needs creation W0 | pending |
| SPLIT-06 | 01 | 1 | find_neighbor direction nav | unit | `cargo test -p glass_mux split_tree::tests::neighbor` | Needs creation W0 | pending |
| SPLIT-07 | 01 | 1 | Resize ratio clamping | unit | `cargo test -p glass_mux split_tree::tests::resize_ratio` | Needs creation W0 | pending |
| SPLIT-08 | 02 | 2 | Tab SplitNode + focused_pane | unit | `cargo test -p glass_mux session_mux::tests` | Existing, needs extension | pending |
| SPLIT-09 | 03 | 3 | PTY resize per-pane cells | integration | Manual -- requires PTY | Manual only | pending |
| SPLIT-10 | 02 | 2 | Scissor rect clipping | integration | Manual -- requires GPU | Manual only | pending |
| SPLIT-11 | 03 | 3 | Last pane close = tab close | unit | `cargo test -p glass_mux session_mux::tests` | Needs creation | pending |

*Status: pending / green / red / flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_mux/src/split_tree.rs` -- full test module for SplitNode (SPLIT-01 through SPLIT-07)
- [ ] Tests for layout computation, tree manipulation, focus navigation, ratio resize

*Existing test infrastructure (cargo test) covers framework needs.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| PTY resize sends correct per-pane cell dimensions | SPLIT-09 | Requires active PTY process | Split pane, run `tput cols` in each pane, verify different column counts |
| Scissor rect clipping renders correctly | SPLIT-10 | Requires GPU rendering | Split pane, scroll content in one pane, verify no bleed into adjacent pane |
| Mouse click changes pane focus | SPLIT-10 | Requires mouse input | Click in non-focused pane, verify focus border moves |
| Visual dividers render between panes | SPLIT-10 | Requires visual inspection | Split pane, verify 1-2px divider line visible |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

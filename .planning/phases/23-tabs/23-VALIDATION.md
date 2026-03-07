---
phase: 23
slug: tabs
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-06
---

# Phase 23 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (built-in Rust testing) |
| **Config file** | Cargo.toml workspace |
| **Quick run command** | `cargo test -p glass_mux && cargo test -p glass_renderer` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_mux && cargo test -p glass_renderer`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 23-01-01 | 01 | 1 | TAB-01 | unit | `cargo test -p glass_mux -- session_mux` | Partial | pending |
| 23-01-02 | 01 | 1 | TAB-02 | unit | `cargo test -p glass_mux -- tab_cycle` | W0 | pending |
| 23-01-03 | 01 | 1 | TAB-03 | unit | `cargo test -p glass_mux -- close_tab` | W0 | pending |
| 23-02-01 | 02 | 1 | TAB-04 | unit | `cargo test -p glass_renderer -- tab_bar` | W0 | pending |
| 23-03-01 | 03 | 2 | TAB-05 | integration | Manual stress test | N/A | pending |

*Status: pending / green / red / flaky*

---

## Wave 0 Requirements

- [ ] `glass_mux/src/session_mux.rs` — unit tests for add_tab, close_tab, activate_tab, next_tab, prev_tab
- [ ] `glass_renderer/src/tab_bar.rs` — unit tests for tab bar rect/text generation

*Existing infrastructure covers framework setup (cargo test already configured).*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| 50-tab rapid create/close stress test | TAB-05 | Requires PTY and real window | Create/close 50 tabs rapidly, check for zombie processes and resource leaks |
| Tab bar click hit-testing | TAB-04 | Requires mouse input + GPU rendering | Click each tab, verify correct activation |
| CWD inheritance on new tab | TAB-01 | Requires shell and filesystem | cd to a directory, open new tab, verify CWD matches |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

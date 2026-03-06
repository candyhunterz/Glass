---
phase: 13
slug: integration-undo-engine
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-05
---

# Phase 13 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test framework (cargo test) |
| **Config file** | Cargo.toml (workspace, already configured) |
| **Quick run command** | `cargo test -p glass_snapshot --lib && cargo test -p glass_core --lib` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_snapshot --lib && cargo test -p glass_core --lib`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 13-01-01 | 01 | 1 | STOR-03 | unit | `cargo test -p glass_core --lib -- config::tests::test_snapshot_config` | W0 | pending |
| 13-01-02 | 01 | 1 | SNAP-01 | unit | `cargo test -p glass_snapshot --lib -- undo::tests::test_pre_exec_snapshot` | W0 | pending |
| 13-02-01 | 02 | 1 | UNDO-01 | unit | `cargo test -p glass_snapshot --lib -- undo::tests::test_undo_latest` | W0 | pending |
| 13-02-02 | 02 | 1 | UNDO-02 | unit | `cargo test -p glass_snapshot --lib -- undo::tests::test_restore_file` | W0 | pending |
| 13-02-03 | 02 | 1 | UNDO-03 | unit | `cargo test -p glass_snapshot --lib -- undo::tests::test_conflict_detection` | W0 | pending |
| 13-02-04 | 02 | 1 | UNDO-04 | unit | `cargo test -p glass_snapshot --lib -- undo::tests::test_confidence_level` | W0 | pending |
| 13-03-01 | 03 | 2 | UNDO-01 | integration | `cargo test -p glass_snapshot --lib -- undo::tests::test_keybinding_undo` | W0 | pending |

*Status: pending / green / red / flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_snapshot/src/undo.rs` — new module with UndoEngine struct and test stubs
- [ ] `crates/glass_core/src/config.rs` — add SnapshotSection tests
- [ ] `crates/glass_snapshot/src/db.rs` — add query methods for latest snapshot with parser files

*Wave 0 creates test stubs and infrastructure before implementation begins.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Ctrl+Shift+Z triggers undo in running terminal | UNDO-01 | Requires live terminal with keybinding processing | 1. Run Glass, 2. Execute `echo test > /tmp/undo_test.txt`, 3. Press Ctrl+Shift+Z, 4. Verify file is removed |
| Undo confidence displayed per command | UNDO-04 | Requires visual terminal output inspection | 1. Run Glass, 2. Execute a known command (e.g., `cp`), 3. Verify confidence indicator appears |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

---
phase: 12
slug: fs-watcher-engine
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-05
---

# Phase 12 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (built-in) |
| **Config file** | Cargo.toml (workspace) |
| **Quick run command** | `cargo test -p glass_snapshot` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_snapshot`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 12-01-01 | 01 | 1 | SNAP-04 | unit | `cargo test -p glass_snapshot watcher::tests::test_watcher_detects_create` | W0 | pending |
| 12-01-02 | 01 | 1 | SNAP-04 | unit | `cargo test -p glass_snapshot watcher::tests::test_event_kinds` | W0 | pending |
| 12-01-03 | 01 | 1 | SNAP-04 | unit | `cargo test -p glass_snapshot watcher::tests::test_rename_detection` | W0 | pending |
| 12-01-04 | 01 | 1 | SNAP-04 | unit | `cargo test -p glass_snapshot watcher::tests::test_drain_events` | W0 | pending |
| 12-02-01 | 02 | 1 | STOR-02 | unit | `cargo test -p glass_snapshot ignore_rules::tests::test_hardcoded_ignores` | W0 | pending |
| 12-02-02 | 02 | 1 | STOR-02 | unit | `cargo test -p glass_snapshot ignore_rules::tests::test_glassignore_file` | W0 | pending |
| 12-02-03 | 02 | 1 | STOR-02 | unit | `cargo test -p glass_snapshot watcher::tests::test_ignore_filtering` | W0 | pending |

*Status: pending · green · red · flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_snapshot/src/watcher.rs` — test stubs for SNAP-04
- [ ] `crates/glass_snapshot/src/ignore_rules.rs` — test stubs for STOR-02
- [ ] Add `notify = "8.2"` and `ignore = "0.4"` to glass_snapshot/Cargo.toml

*Wave 0 creates test stubs that compile but fail, ensuring test infrastructure exists before implementation.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Watcher starts/stops with command lifecycle | SNAP-04 | Requires shell integration (PTY + event loop) | Run a command in Glass, verify watcher events appear in snapshot_files table |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

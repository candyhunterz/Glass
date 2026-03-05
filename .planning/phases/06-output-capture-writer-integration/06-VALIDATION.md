---
phase: 6
slug: output-capture-writer-integration
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-05
---

# Phase 6 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test framework (cargo test) |
| **Config file** | Cargo.toml (workspace) |
| **Quick run command** | `cargo test -p glass_terminal -p glass_history` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_terminal -p glass_history`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 06-01-01 | 01 | 1 | HIST-02 | unit | `cargo test -p glass_terminal output_buffer` | No - W0 | pending |
| 06-01-02 | 01 | 1 | HIST-02 | unit | `cargo test -p glass_terminal alt_screen` | No - W0 | pending |
| 06-01-03 | 01 | 1 | HIST-02 | unit | `cargo test -p glass_history binary_detect` | No - W0 | pending |
| 06-01-04 | 01 | 1 | HIST-02 | unit | `cargo test -p glass_history truncate` | No - W0 | pending |
| 06-01-05 | 01 | 1 | HIST-02 | unit | `cargo test -p glass_history ansi_strip` | No - W0 | pending |
| 06-02-01 | 02 | 1 | HIST-02 | unit | `cargo test -p glass_history migration` | No - W0 | pending |
| 06-02-02 | 02 | 1 | HIST-02 | unit | `cargo test -p glass_history output_roundtrip` | No - W0 | pending |
| 06-02-03 | 02 | 1 | HIST-02 | unit | `cargo test -p glass_history config` | Partial | pending |
| 06-03-01 | 03 | 2 | INFR-02 | manual | Visual verification | No - manual | pending |

*Status: pending / green / red / flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_terminal/src/output_capture.rs` — OutputBuffer struct with unit tests (HIST-02a, HIST-02b)
- [ ] `crates/glass_history/src/output.rs` or additions to `db.rs` — truncation, binary detection, ANSI stripping tests (HIST-02c, HIST-02d, HIST-02e)
- [ ] Schema migration tests in `db.rs` (HIST-02f)
- [ ] Updated `insert_command` / `get_command` tests with output field (HIST-02g)
- [ ] `max_output_capture_kb` in HistoryConfig test (HIST-02h)

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Block decorations scroll correctly with display_offset | INFR-02 | Visual rendering requires GPU context | 1. Run Glass, execute several commands. 2. Scroll up through history. 3. Verify separator lines and exit code badges stay aligned with their commands. |
| PTY throughput not regressed | INFR-02 | Performance requires real PTY interaction | 1. Run `time seq 1 100000` in Glass. 2. Compare wall time with v1.0 baseline. 3. Verify no measurable regression. |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

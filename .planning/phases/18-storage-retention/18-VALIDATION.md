---
phase: 18
slug: storage-retention
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-06
---

# Phase 18 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[test]` + tempfile for database fixtures |
| **Config file** | None (Cargo.toml `[dev-dependencies]` only) |
| **Quick run command** | `cargo test -p glass_history` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_history`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 18-01-01 | 01 | 1 | STOR-01 | unit | `cargo test -p glass_history -- test_migration_v1_to_v2 -x` | W0 | pending |
| 18-01-02 | 01 | 1 | STOR-01 | unit | `cargo test -p glass_history -- test_existing_records_survive_v2_migration -x` | W0 | pending |
| 18-01-03 | 01 | 1 | STOR-01 | unit | `cargo test -p glass_history -- test_insert_and_get_pipe_stages -x` | W0 | pending |
| 18-01-04 | 01 | 1 | STOR-01 | unit | `cargo test -p glass_history -- test_no_pipe_stages_for_simple_command -x` | W0 | pending |
| 18-01-05 | 01 | 1 | STOR-01 | unit | `cargo test -p glass_history -- test_pipe_stage_buffer_variants -x` | W0 | pending |
| 18-02-01 | 02 | 1 | STOR-02 | unit | `cargo test -p glass_history -- test_prune_cascades_to_pipe_stages -x` | W0 | pending |
| 18-02-02 | 02 | 1 | STOR-02 | unit | `cargo test -p glass_history -- test_size_prune_cascades_to_pipe_stages -x` | W0 | pending |
| 18-02-03 | 02 | 1 | STOR-02 | unit | `cargo test -p glass_history -- test_delete_command_cascades_pipe_stages -x` | W0 | pending |

*Status: pending / green / red / flaky*

---

## Wave 0 Requirements

- [ ] Test stubs for STOR-01 migration and insert/get tests
- [ ] Test stubs for STOR-02 pruning cascade tests
- [ ] Shared test fixtures (tempfile db creation, sample CommandRecord + CapturedStage helpers)

*Existing infrastructure (tempfile, in-memory SQLite) covers all needs. No new dev-dependencies.*

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

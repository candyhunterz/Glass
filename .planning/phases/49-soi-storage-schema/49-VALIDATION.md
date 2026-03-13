---
phase: 49
slug: soi-storage-schema
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-12
---

# Phase 49 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[test]` + cargo test |
| **Config file** | none — inline `#[cfg(test)] mod tests` |
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
| 49-01-01 | 01 | 1 | SOIS-02 | unit | `cargo test -p glass_history -- test_fresh_db_has_output_column_and_version` | ✅ update | ⬜ pending |
| 49-01-02 | 01 | 1 | SOIS-02 | unit | `cargo test -p glass_history -- test_migration_v2_to_v3` | ❌ W0 | ⬜ pending |
| 49-01-03 | 01 | 1 | SOIS-01 | unit | `cargo test -p glass_history -- soi` | ❌ W0 | ⬜ pending |
| 49-01-04 | 01 | 1 | SOIS-03 | unit | `cargo test -p glass_history -- soi` | ❌ W0 | ⬜ pending |
| 49-01-05 | 01 | 1 | SOIS-04 | unit | `cargo test -p glass_history -- prune_cascades_to_soi` | ❌ W0 | ⬜ pending |
| 49-01-06 | 01 | 1 | SOIS-04 | unit | `cargo test -p glass_history -- size_prune_cascades_to_soi` | ❌ W0 | ⬜ pending |
| 49-01-07 | 01 | 1 | SOIS-04 | unit | `cargo test -p glass_history -- delete_command_cascades_soi` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_history/src/soi.rs` — new module with `OutputRecordRow`, `CommandOutputSummaryRow`, insert/query functions + inline tests
- [ ] `test_migration_v2_to_v3` in `db.rs` tests
- [ ] `test_prune_cascades_to_soi` in `retention.rs` tests
- [ ] `test_size_prune_cascades_to_soi` in `retention.rs` tests
- [ ] `test_delete_command_cascades_soi` in `db.rs` tests
- [ ] Update `SCHEMA_VERSION` constant from `2` to `3` in `db.rs`

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

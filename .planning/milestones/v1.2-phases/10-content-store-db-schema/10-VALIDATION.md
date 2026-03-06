---
phase: 10
slug: content-store-db-schema
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-05
---

# Phase 10 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[cfg(test)]` + tempfile 3 |
| **Config file** | None — Rust's built-in test harness |
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
| 10-01-01 | 01 | 1 | SNAP-02 | unit | `cargo test -p glass_snapshot -- blob_store::tests::test_dedup` | W0 | pending |
| 10-01-02 | 01 | 1 | SNAP-02 | unit | `cargo test -p glass_snapshot -- blob_store::tests::test_hash_correctness` | W0 | pending |
| 10-01-03 | 01 | 1 | SNAP-02 | unit | `cargo test -p glass_snapshot -- blob_store::tests::test_store_and_read` | W0 | pending |
| 10-01-04 | 01 | 1 | SNAP-06 | unit | `cargo test -p glass_snapshot -- db::tests::test_schema_creation` | W0 | pending |
| 10-01-05 | 01 | 1 | SNAP-06 | unit | `cargo test -p glass_snapshot -- db::tests::test_persistence` | W0 | pending |
| 10-01-06 | 01 | 1 | SNAP-06 | unit | `cargo test -p glass_snapshot -- db::tests::test_command_id_link` | W0 | pending |
| 10-02-01 | 02 | 2 | SNAP-05 | integration | Manual — requires terminal + shell integration | Manual | pending |

*Status: pending · green · red · flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_snapshot/src/blob_store.rs` — BlobStore implementation with tests
- [ ] `crates/glass_snapshot/src/db.rs` — SnapshotDb implementation with tests
- [ ] `crates/glass_snapshot/src/types.rs` — Shared types
- [ ] `crates/glass_snapshot/src/lib.rs` — Module declarations and re-exports
- [ ] `crates/glass_snapshot/Cargo.toml` — Add blake3, rusqlite, anyhow, tracing, dirs dependencies
- [ ] Root `Cargo.toml` — Add `blake3 = "1.8.3"` to workspace dependencies
- [ ] tempfile dev-dependency already available in workspace

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Command text extracted at CommandExecuted time is non-empty | SNAP-05 | Requires terminal + shell integration — cannot unit test grid extraction | 1. Launch Glass terminal 2. Run a command (e.g. `echo hello`) 3. Check logs for non-empty command text at CommandExecuted time |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

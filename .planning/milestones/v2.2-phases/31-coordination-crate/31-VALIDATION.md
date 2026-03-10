---
phase: 31
slug: coordination-crate
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-09
---

# Phase 31 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test framework (cargo test) |
| **Config file** | None — standard `#[cfg(test)] mod tests` pattern |
| **Quick run command** | `cargo test -p glass_coordination` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~10 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_coordination`
- **After every plan wave:** Run `cargo test --workspace && cargo clippy --workspace -- -D warnings`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 10 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 31-01-01 | 01 | 1 | COORD-01 | unit | `cargo test -p glass_coordination -- test_register` | ❌ W0 | ⬜ pending |
| 31-01-02 | 01 | 1 | COORD-02 | unit | `cargo test -p glass_coordination -- test_deregister` | ❌ W0 | ⬜ pending |
| 31-01-03 | 01 | 1 | COORD-03 | unit | `cargo test -p glass_coordination -- test_heartbeat` | ❌ W0 | ⬜ pending |
| 31-01-04 | 01 | 1 | COORD-04 | unit | `cargo test -p glass_coordination -- test_prune_stale` | ❌ W0 | ⬜ pending |
| 31-02-01 | 02 | 1 | COORD-05 | unit | `cargo test -p glass_coordination -- test_lock_files` | ❌ W0 | ⬜ pending |
| 31-02-02 | 02 | 1 | COORD-06 | unit | `cargo test -p glass_coordination -- test_canonicalize` | ❌ W0 | ⬜ pending |
| 31-02-03 | 02 | 1 | COORD-07 | unit | `cargo test -p glass_coordination -- test_unlock` | ❌ W0 | ⬜ pending |
| 31-03-01 | 03 | 1 | COORD-08 | unit | `cargo test -p glass_coordination -- test_broadcast` | ❌ W0 | ⬜ pending |
| 31-03-02 | 03 | 1 | COORD-09 | unit | `cargo test -p glass_coordination -- test_send_message` | ❌ W0 | ⬜ pending |
| 31-03-03 | 03 | 1 | COORD-10 | unit | `cargo test -p glass_coordination -- test_read_messages` | ❌ W0 | ⬜ pending |
| 31-03-04 | 03 | 1 | COORD-11 | unit | `cargo test -p glass_coordination -- test_project_scoping` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_coordination/Cargo.toml` — crate manifest
- [ ] `crates/glass_coordination/src/lib.rs` — public API and re-exports
- [ ] `crates/glass_coordination/src/db.rs` — schema + all SQL operations + tests
- [ ] `crates/glass_coordination/src/types.rs` — data structures
- [ ] `crates/glass_coordination/src/pid.rs` — platform PID liveness + tests
- [ ] Workspace `Cargo.toml` members includes `glass_coordination`

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| PID liveness on Windows | COORD-04 | Requires real process handles | Run test binary, check `OpenProcess` returns valid handle |

*All other phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 10s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

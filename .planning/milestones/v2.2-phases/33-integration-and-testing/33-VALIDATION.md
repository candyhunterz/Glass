---
phase: 33
slug: integration-and-testing
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-09
---

# Phase 33 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (built-in Rust test harness) |
| **Config file** | Cargo.toml (workspace) |
| **Quick run command** | `cargo test -p glass_coordination` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_coordination`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 33-01-01 | 01 | 1 | INTG-01 | manual | Visual inspection of CLAUDE.md | N/A (doc) | ⬜ pending |
| 33-01-02 | 01 | 1 | INTG-02 | integration | `cargo test -p glass_coordination test_cross_connection` | ❌ W0 | ⬜ pending |
| 33-01-03 | 01 | 1 | INTG-03 | integration | `cargo test -p glass_coordination test_cross_connection_lock_conflict` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Cross-connection integration tests in `crates/glass_coordination/src/db.rs` — covers INTG-02, INTG-03

*Existing infrastructure covers INTG-01 (documentation change only).*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| CLAUDE.md contains coordination protocol section | INTG-01 | Documentation content verification | Inspect CLAUDE.md for coordination protocol with register/lock/message/deregister instructions |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

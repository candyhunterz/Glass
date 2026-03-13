---
phase: 54
slug: soi-extended-parsers
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-13
---

# Phase 54 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[cfg(test)] mod tests` (co-located) |
| **Config file** | none — existing infrastructure |
| **Quick run command** | `cargo test -p glass_soi` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_soi`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 54-01-01 | 01 | 1 | SOIX-01 | unit | `cargo test -p glass_soi git` | ❌ W0 | ⬜ pending |
| 54-01-02 | 01 | 1 | SOIX-02 | unit | `cargo test -p glass_soi docker` | ❌ W0 | ⬜ pending |
| 54-01-03 | 01 | 1 | SOIX-03 | unit | `cargo test -p glass_soi kubectl` | ❌ W0 | ⬜ pending |
| 54-02-01 | 02 | 1 | SOIX-04 | unit | `cargo test -p glass_soi tsc` | ❌ W0 | ⬜ pending |
| 54-02-02 | 02 | 1 | SOIX-05 | unit | `cargo test -p glass_soi go` | ❌ W0 | ⬜ pending |
| 54-02-03 | 02 | 1 | SOIX-06 | unit | `cargo test -p glass_soi json_lines` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

*Existing infrastructure covers all phase requirements.* All parsers use co-located `#[cfg(test)] mod tests` following the established pattern (cargo_build.rs, cargo_test.rs, npm.rs, pytest.rs, jest.rs).

---

## Manual-Only Verifications

*All phase behaviors have automated verification.* Parser output is deterministic given input text.

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

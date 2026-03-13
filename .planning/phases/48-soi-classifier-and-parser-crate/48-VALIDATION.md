---
phase: 48
slug: soi-classifier-and-parser-crate
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-12
---

# Phase 48 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[test]` via `cargo test` |
| **Config file** | none (workspace-level `cargo test --workspace`) |
| **Quick run command** | `cargo test -p glass_soi` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~5 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_soi`
- **After every plan wave:** Run `cargo test --workspace && cargo clippy --workspace -- -D warnings`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 10 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 48-01-01 | 01 | 1 | SOIP-01 | unit | `cargo test -p glass_soi classifier` | ❌ W0 | ⬜ pending |
| 48-01-02 | 01 | 1 | SOIP-02 | unit | `cargo test -p glass_soi cargo_build` | ❌ W0 | ⬜ pending |
| 48-01-03 | 01 | 1 | SOIP-03 | unit | `cargo test -p glass_soi cargo_test` | ❌ W0 | ⬜ pending |
| 48-01-04 | 01 | 1 | SOIP-04 | unit | `cargo test -p glass_soi npm` | ❌ W0 | ⬜ pending |
| 48-01-05 | 01 | 1 | SOIP-05 | unit | `cargo test -p glass_soi pytest` | ❌ W0 | ⬜ pending |
| 48-01-06 | 01 | 1 | SOIP-06 | unit | `cargo test -p glass_soi jest` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_soi/` — entire crate scaffold with Cargo.toml
- [ ] `crates/glass_soi/src/lib.rs` — public API declaration
- [ ] `crates/glass_soi/src/types.rs` — OutputType, OutputRecord, ParsedOutput, Severity, TestStatus
- [ ] `crates/glass_soi/src/classifier.rs` — classifier with test stubs per tool
- [ ] `crates/glass_soi/src/cargo_build.rs` — Rust compiler parser with SOIP-02 test stubs
- [ ] `crates/glass_soi/src/cargo_test.rs` — cargo test parser with SOIP-03 test stubs
- [ ] `crates/glass_soi/src/npm.rs` — npm parser with SOIP-04 test stubs
- [ ] `crates/glass_soi/src/pytest.rs` — pytest parser with SOIP-05 test stubs
- [ ] `crates/glass_soi/src/jest.rs` — jest parser with SOIP-06 test stubs
- [ ] `crates/glass_soi/src/ansi.rs` — ANSI strip utility
- [ ] `Cargo.toml` workspace members update — add `crates/glass_soi` to `members`

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 10s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

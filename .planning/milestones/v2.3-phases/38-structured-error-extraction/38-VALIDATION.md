---
phase: 38
slug: structured-error-extraction
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-09
---

# Phase 38 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (built-in) |
| **Config file** | Cargo.toml (workspace member) |
| **Quick run command** | `cargo test --package glass_errors` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --package glass_errors && cargo test --package glass_mcp`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 38-01-01 | 01 | 1 | ERR-01 | unit | `cargo test --package glass_errors` | ❌ W0 | ⬜ pending |
| 38-01-02 | 01 | 1 | ERR-02 | unit | `cargo test --package glass_errors` | ❌ W0 | ⬜ pending |
| 38-01-03 | 01 | 1 | ERR-02 | unit | `cargo test --package glass_errors` | ❌ W0 | ⬜ pending |
| 38-01-04 | 01 | 1 | ERR-03 | unit | `cargo test --package glass_errors` | ❌ W0 | ⬜ pending |
| 38-01-05 | 01 | 1 | ERR-04 | unit | `cargo test --package glass_errors` | ❌ W0 | ⬜ pending |
| 38-02-01 | 02 | 2 | ERR-01 | unit | `cargo test --package glass_mcp` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_errors/Cargo.toml` — new crate manifest
- [ ] `crates/glass_errors/src/lib.rs` — public types and entry point with tests
- [ ] `crates/glass_errors/src/rust_json.rs` — Rust JSON parser with tests
- [ ] `crates/glass_errors/src/rust_human.rs` — Rust human parser with tests
- [ ] `crates/glass_errors/src/generic.rs` — Generic parser with tests
- [ ] `crates/glass_errors/src/detect.rs` — Auto-detection with tests

*All test files are created by the plans themselves (Wave 0 integrated into Wave 1).*

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

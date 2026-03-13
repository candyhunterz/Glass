---
phase: 60
slug: agent-configuration-and-polish
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-13
---

# Phase 60 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in (`#[cfg(test)] mod tests`) + cargo test |
| **Config file** | none (inline tests) |
| **Quick run command** | `cargo test -p glass_core --lib 2>&1` |
| **Full suite command** | `cargo test --workspace 2>&1` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_core --lib 2>&1`
- **After every plan wave:** Run `cargo test --workspace 2>&1`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 60-01-01 | 01 | 1 | AGTC-01 | unit | `cargo test -p glass_core --lib config 2>&1` | ✅ | ⬜ pending |
| 60-01-02 | 01 | 1 | AGTC-01 | unit | `cargo test -p glass_core --lib config 2>&1` | ✅ | ⬜ pending |
| 60-01-03 | 01 | 1 | AGTC-02 | unit | `cargo test -p glass_core --lib 2>&1` | ❌ W0 | ⬜ pending |
| 60-01-04 | 01 | 1 | AGTC-03 | unit | `cargo test -p glass_core --lib 2>&1` | ❌ W0 | ⬜ pending |
| 60-02-01 | 02 | 2 | AGTC-02 | unit | `cargo test -p glass_core --lib 2>&1` | ❌ W0 | ⬜ pending |
| 60-02-02 | 02 | 2 | AGTC-03 | unit | `cargo test -p glass_core --lib 2>&1` | ❌ W0 | ⬜ pending |
| 60-02-03 | 02 | 2 | AGTC-04 | unit | `cargo test -p glass_core --lib config 2>&1` | ✅ | ⬜ pending |
| 60-02-04 | 02 | 2 | AGTC-05 | integration | `cargo test -p glass_coordination --lib 2>&1` | ✅ | ⬜ pending |
| 60-02-05 | 02 | 2 | AGTC-01 | unit | `cargo test -p glass_core --lib 2>&1` | ✅ | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Tests for `PermissionMatrix` parsing in `crates/glass_core/src/config.rs` tests block
- [ ] Tests for `QuietRules` parsing in `crates/glass_core/src/config.rs` tests block
- [ ] Unit tests for `classify_proposal` helper function
- [ ] Unit tests for quiet_rules filter logic (pure function)

*Existing coordination tests cover CoordinationDb — no new fixtures needed for lock/unlock operations.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Config hot-reload restarts agent | AGTC-01 | Requires running Glass + editing config.toml | 1. Start Glass with agent.mode="off" 2. Edit config.toml to mode="assist" 3. Verify agent starts |
| User sees binary-not-found hint | AGTC-04 | Visual verification of config error display | 1. Set agent.enabled=true 2. Remove claude from PATH 3. Start Glass 4. Verify hint shown |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

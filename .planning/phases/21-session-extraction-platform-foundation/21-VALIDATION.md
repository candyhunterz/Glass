---
phase: 21
slug: session-extraction-platform-foundation
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-06
---

# Phase 21 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test (`#[cfg(test)]` + `cargo test`) |
| **Config file** | None (uses Cargo.toml test config) |
| **Quick run command** | `cargo test -p glass_mux` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_mux && cargo check -p glass`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 21-01-01 | 01 | 1 | P21-01 | compilation | `cargo check -p glass_mux` | ❌ W0 | ⬜ pending |
| 21-01-02 | 01 | 1 | P21-02 | unit | `cargo test -p glass_mux -- session` | ❌ W0 | ⬜ pending |
| 21-01-03 | 01 | 1 | P21-04 | unit | `cargo test -p glass_mux -- session_mux` | ❌ W0 | ⬜ pending |
| 21-01-04 | 01 | 1 | P21-06 | unit | `cargo test -p glass_mux -- platform` | ❌ W0 | ⬜ pending |
| 21-01-05 | 01 | 1 | P21-07 | unit | `cargo test -p glass_mux -- platform::config` | ❌ W0 | ⬜ pending |
| 21-01-06 | 01 | 1 | P21-08 | unit | `cargo test -p glass_mux -- platform::modifier` | ❌ W0 | ⬜ pending |
| 21-02-01 | 02 | 1 | P21-03 | unit | `cargo test -p glass_core -- app_event` | ✅ partial | ⬜ pending |
| 21-02-02 | 02 | 1 | P21-05 | compilation | `cargo check -p glass` | ✅ | ⬜ pending |
| 21-03-01 | 03 | 2 | P21-09 | smoke | `test -f shell-integration/glass.zsh` | ❌ W0 | ⬜ pending |
| 21-03-02 | 03 | 2 | P21-10 | integration | `cargo test --workspace` | ✅ | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_mux/Cargo.toml` — new crate manifest
- [ ] `crates/glass_mux/src/lib.rs` — crate root with module declarations
- [ ] `crates/glass_mux/src/session.rs` — Session struct with unit tests
- [ ] `crates/glass_mux/src/session_mux.rs` — SessionMux with unit tests
- [ ] `crates/glass_mux/src/platform.rs` — platform helpers with unit tests
- [ ] `crates/glass_mux/src/types.rs` — SessionId, TabId types
- [ ] `shell-integration/glass.zsh` — zsh shell integration script
- [ ] Update `crates/glass_core/src/event.rs` tests for SessionId in AppEvent variants

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Glass launches and runs identically to v1.3 | P21-10 | Requires visual + interactive verification | Launch Glass, run commands, verify blocks, search overlay, undo all work |
| Shell integration loads on zsh/bash | P21-09 | Requires macOS/Linux environment | Start Glass on macOS/Linux, verify OSC 133 sequences emitted |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

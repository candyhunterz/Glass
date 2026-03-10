---
phase: 39
slug: live-command-awareness
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-10
---

# Phase 39 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (built-in Rust test framework) |
| **Config file** | Cargo.toml workspace |
| **Quick run command** | `cargo test -p glass_mcp --lib` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_mcp --lib && cargo test -p glass_terminal --lib`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 39-01-01 | 01 | 1 | LIVE-01 | unit | `cargo test -p glass_mcp --lib test_has_running_command_params` | ❌ W0 | ⬜ pending |
| 39-01-02 | 01 | 1 | LIVE-01 | unit | `cargo test -p glass_mcp --lib test_has_running_command_no_gui` | ❌ W0 | ⬜ pending |
| 39-01-03 | 01 | 1 | LIVE-02 | unit | `cargo test -p glass_mcp --lib test_cancel_command_params` | ❌ W0 | ⬜ pending |
| 39-01-04 | 01 | 1 | LIVE-02 | unit | `cargo test -p glass_mcp --lib test_cancel_command_no_gui` | ❌ W0 | ⬜ pending |
| 39-01-05 | 01 | 1 | LIVE-01 | unit | `cargo test -p glass_terminal --lib test_block_elapsed` | ✅ | ⬜ pending |
| 39-01-06 | 01 | 1 | LIVE-02 | unit | `cargo test -p glass_mcp --lib test_etx_byte` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `HasRunningCommandParams` deserialization test in tools.rs
- [ ] `CancelCommandParams` deserialization test in tools.rs
- [ ] No-GUI error tests for both new tools in tools.rs
- [ ] ETX byte constant test in tools.rs

*Existing infrastructure covers block state and elapsed time testing.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Cancel actually interrupts a running command in GUI | LIVE-02 | Requires running Glass GUI + active command | 1. Start Glass, run `sleep 60`, call cancel_command, verify command exits |
| Elapsed time updates in real time | LIVE-01 | Requires live command execution | 1. Start Glass, run `sleep 60`, call has_running_command multiple times, verify elapsed increases |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

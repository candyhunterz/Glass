---
phase: 20
slug: config-gate-dead-code-cleanup
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-06
---

# Phase 20 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust built-in) |
| **Config file** | Cargo.toml (workspace) |
| **Quick run command** | `cargo test -p glass_pipes && cargo test -p glass_terminal` |
| **Full suite command** | `cargo test` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_pipes && cargo test -p glass_terminal`
- **After every plan wave:** Run `cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 20-01-01 | 01 | 1 | CONF-01 | unit | `cargo test -p glass_terminal -- test_spawn_pty_pipes_disabled` | Wave 0 | pending |
| 20-01-02 | 01 | 1 | CONF-01 | manual | Manual: set GLASS_PIPES_DISABLED=1, verify bash skips rewrite | N/A | pending |
| 20-01-03 | 01 | 1 | CONF-01 | manual | Manual: set GLASS_PIPES_DISABLED=1, verify PowerShell skips rewrite | N/A | pending |
| 20-01-04 | 01 | 1 | CONF-01 | integration | Verified by: no pipeline_stages in block when pipes disabled | Wave 0 | pending |
| 20-02-01 | 02 | 1 | PIPE-02 | build | `cargo build` | N/A (compiler) | pending |
| 20-02-02 | 02 | 1 | PIPE-02 | unit | `cargo test` | Existing | pending |

*Status: pending / green / red / flaky*

---

## Wave 0 Requirements

- [ ] Optional: unit test for spawn_pty env var injection if signature change makes it testable
- [ ] Existing test infrastructure covers dead code removal verification (cargo build + cargo test)

*Existing infrastructure covers most phase requirements. Compiler verification (cargo build succeeds) is the primary gate for dead code removal.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Bash script skips pipeline rewrite when GLASS_PIPES_DISABLED=1 | CONF-01 | Shell integration requires live terminal with env var set | 1. Set GLASS_PIPES_DISABLED=1 2. Run `echo hello | cat` 3. Verify no tee rewriting in command |
| PowerShell script skips pipeline rewrite when GLASS_PIPES_DISABLED=1 | CONF-01 | Shell integration requires live terminal with env var set | 1. Set $env:GLASS_PIPES_DISABLED="1" 2. Run `echo hello | cat` 3. Verify no Tee-Object insertion |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

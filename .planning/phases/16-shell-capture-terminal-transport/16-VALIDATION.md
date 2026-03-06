---
phase: 16
slug: shell-capture-terminal-transport
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-05
---

# Phase 16 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[cfg(test)]` + cargo test; shell script manual validation |
| **Config file** | none — existing workspace config |
| **Quick run command** | `cargo test -p glass_terminal -- osc_scanner && cargo test -p glass_core` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_terminal && cargo test -p glass_core`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green + manual shell integration test
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 16-01-01 | 01 | 1 | CAPT-01/02 | unit | `cargo test -p glass_core -- event::pipeline` | No — W0 | pending |
| 16-01-02 | 01 | 1 | CAPT-01/02 | unit | `cargo test -p glass_terminal -- osc_scanner::pipeline` | No — W0 | pending |
| 16-01-03 | 01 | 1 | CAPT-01/02 | unit | `cargo test -p glass_terminal -- block_manager::pipeline` | No — W0 | pending |
| 16-02-01 | 02 | 2 | CAPT-01 | integration | Manual: source glass.bash; test pipeline rewriting | No — W0 | pending |
| 16-02-02 | 02 | 2 | CAPT-01 | integration | Manual: run pipeline in Glass terminal, check exit code | No — W0 | pending |
| 16-03-01 | 03 | 2 | CAPT-02 | integration | Manual: run pipeline in Glass terminal with PowerShell | No — W0 | pending |

*Status: pending / green / red / flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_terminal/src/osc_scanner.rs` — tests for OSC 133;S and 133;P parsing
- [ ] `crates/glass_terminal/src/block_manager.rs` — tests for PipelineStart/PipelineStage handling
- [ ] `crates/glass_core/src/event.rs` — new ShellEvent variants (compile-time validated)
- [ ] `crates/glass_pipes/src/types.rs` — CapturedStage type tests

*Existing infrastructure covers framework install.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Bash tee rewriting produces correct command | CAPT-01 | Requires live bash shell with glass.bash sourced | `source glass.bash; run piped command; verify tee insertion and OSC emission` |
| PIPESTATUS preserved after tee pipeline | CAPT-01 | Requires live shell environment | `false \| cat; echo $?` should show 1 |
| PowerShell Tee-Object captures stage text | CAPT-02 | Requires live PowerShell with glass.ps1 loaded | Run pipeline in Glass terminal; verify OSC 133;P emitted |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

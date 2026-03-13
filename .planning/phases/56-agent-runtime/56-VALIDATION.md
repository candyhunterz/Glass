---
phase: 56
slug: agent-runtime
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-13
---

# Phase 56 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test (`cargo test`) |
| **Config file** | None (inline `#[cfg(test)]`) |
| **Quick run command** | `cargo test -p glass_core -- agent` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_core -- agent`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 56-01-01 | 01 | 1 | AGTR-01 | unit | `cargo test -p glass -- agent_runtime::tests::test_agent_command_flags` | ❌ W0 | ⬜ pending |
| 56-01-02 | 01 | 1 | AGTR-02 | unit | `cargo test -p glass_core -- agent_runtime::tests::test_format_activity_message` | ❌ W0 | ⬜ pending |
| 56-01-03 | 01 | 1 | AGTR-02 | unit | `cargo test -p glass_core -- agent_runtime::tests::test_parse_cost_from_result` | ❌ W0 | ⬜ pending |
| 56-01-04 | 01 | 1 | AGTR-03 | unit | `cargo test -p glass_core -- agent_runtime::tests::test_autonomy_mode_filter` | ❌ W0 | ⬜ pending |
| 56-01-05 | 01 | 1 | AGTR-06 | unit | `cargo test -p glass_core -- agent_runtime::tests::test_cooldown_filter` | ❌ W0 | ⬜ pending |
| 56-01-06 | 01 | 1 | AGTR-07 | unit | `cargo test -p glass_core -- agent_runtime::tests::test_budget_gate` | ❌ W0 | ⬜ pending |
| 56-02-01 | 02 | 2 | AGTR-04 | unit | `cargo test -p glass -- agent_runtime::tests::test_restart_on_crash` | ❌ W0 | ⬜ pending |
| 56-02-02 | 02 | 2 | AGTR-05 | unit | `cargo test -p glass -- agent_runtime::tests::test_windows_job_object_setup` | ❌ W0 | ⬜ pending |
| 56-02-03 | 02 | 2 | AGTR-07 | unit | `cargo test -p glass_renderer -- status_bar::tests::test_agent_cost_text_format` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_core/src/agent_runtime.rs` — new file: `AgentMode`, `AgentProposalData`, `AgentRuntimeConfig`, helper functions, unit test stubs
- [ ] `crates/glass_core/src/event.rs` — add `AppEvent::AgentProposal`, `AppEvent::AgentQueryResult`, `AppEvent::AgentCrashed` variants
- [ ] `crates/glass_renderer/src/status_bar.rs` — add `agent_cost_text` field to `StatusLabel`
- [ ] Windows `windows-sys` features: add `Win32_System_JobObjects`, `Win32_Foundation`, `Win32_Security` to workspace `Cargo.toml`

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Claude CLI process spawns and receives activity events on stdin | AGTR-01 | Requires actual Claude CLI binary and API key | Start Glass with `agent.mode = "watch"`, run a command, verify agent process receives event in logs |
| Killing Glass doesn't leave orphaned process | AGTR-05 | Requires process kill simulation | Start Glass with agent, `taskkill /F /PID <glass>`, verify no `claude` process remains |
| Status bar shows real-time cost | AGTR-07 | Visual verification | Run Glass with agent, trigger proposals, verify cost updates in status bar |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

---
phase: 59
slug: agent-session-continuity
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-13
---

# Phase 59 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test (`cargo test`) |
| **Config file** | None (inline `#[cfg(test)]`) |
| **Quick run command** | `cargo test -p glass_agent -- session && cargo test -p glass_core -- agent_runtime` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_agent -- session && cargo test -p glass_core -- agent_runtime`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 59-01-01 | 01 | 1 | AGTS-01 | unit | `cargo test -p glass_core -- agent_runtime::tests::extract_handoff_parses_valid_marker` | Wave 0 | ⬜ pending |
| 59-01-02 | 01 | 1 | AGTS-01 | unit | `cargo test -p glass_core -- agent_runtime::tests::extract_handoff_returns_none_without_marker` | Wave 0 | ⬜ pending |
| 59-01-03 | 01 | 1 | AGTS-02 | unit | `cargo test -p glass_agent -- session_db::tests::test_insert_and_list` | Wave 0 | ⬜ pending |
| 59-01-04 | 01 | 1 | AGTS-02 | unit | `cargo test -p glass_agent -- session_db::tests::test_session_survives_restart` | Wave 0 | ⬜ pending |
| 59-01-05 | 01 | 1 | AGTS-02 | unit | `cargo test -p glass_agent -- session_db::tests::test_migration_version_3` | Wave 0 | ⬜ pending |
| 59-01-06 | 01 | 1 | AGTS-03 | unit | `cargo test -p glass_agent -- session_db::tests::test_load_prior_handoff_most_recent` | Wave 0 | ⬜ pending |
| 59-01-07 | 01 | 1 | AGTS-03 | unit | `cargo test -p glass_agent -- session_db::tests::test_load_prior_handoff_empty` | Wave 0 | ⬜ pending |
| 59-01-08 | 01 | 1 | AGTS-03 | unit | `cargo test -p glass_core -- agent_runtime::tests::format_handoff_produces_valid_json` | Wave 0 | ⬜ pending |
| 59-01-09 | 01 | 1 | AGTS-04 | unit | `cargo test -p glass_agent -- session_db::tests::test_session_chain_three_records` | Wave 0 | ⬜ pending |
| 59-01-10 | 01 | 1 | AGTS-04 | unit | `cargo test -p glass_agent -- worktree_db::tests::test_migration_version_2` | ✅ | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_agent/src/session_db.rs` — new file: `AgentSessionDb`, migration version 3, `insert_session`, `load_prior_handoff`, tests
- [ ] `crates/glass_agent/src/types.rs` — add `HandoffData`, `AgentSessionRecord` structs
- [ ] `crates/glass_core/src/agent_runtime.rs` — add `extract_handoff()`, `format_handoff_as_user_message()`
- [ ] `crates/glass_core/src/event.rs` — add `AppEvent::AgentHandoff` variant

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Agent emits GLASS_HANDOFF on context exhaustion | AGTS-01 | Requires live agent session hitting context limit | Run a long agent session, verify handoff JSON appears in output |
| New session loads prior handoff automatically | AGTS-03 | Requires full agent spawn cycle | Start agent, end session, start new agent, verify handoff context injected |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

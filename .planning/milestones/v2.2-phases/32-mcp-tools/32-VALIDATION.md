---
phase: 32
slug: mcp-tools
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-09
---

# Phase 32 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (built-in Rust test framework) |
| **Config file** | None needed (standard Cargo workspace) |
| **Quick run command** | `cargo test --package glass_mcp` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --package glass_mcp`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 32-01-01 | 01 | 1 | MCP-01 | unit | `cargo test --package glass_mcp -- glass_agent_register` | ❌ W0 | ⬜ pending |
| 32-01-02 | 01 | 1 | MCP-02 | unit | `cargo test --package glass_mcp -- deregister` | ❌ W0 | ⬜ pending |
| 32-01-03 | 01 | 1 | MCP-03 | unit | `cargo test --package glass_mcp -- agent_list` | ❌ W0 | ⬜ pending |
| 32-01-04 | 01 | 1 | MCP-04 | unit | `cargo test --package glass_mcp -- agent_status` | ❌ W0 | ⬜ pending |
| 32-01-05 | 01 | 1 | MCP-05 | unit | `cargo test --package glass_mcp -- agent_lock` | ❌ W0 | ⬜ pending |
| 32-01-06 | 01 | 1 | MCP-06 | unit | `cargo test --package glass_mcp -- agent_unlock` | ❌ W0 | ⬜ pending |
| 32-01-07 | 01 | 1 | MCP-07 | unit | `cargo test --package glass_mcp -- agent_locks` | ❌ W0 | ⬜ pending |
| 32-01-08 | 01 | 1 | MCP-08 | unit | `cargo test --package glass_mcp -- agent_broadcast` | ❌ W0 | ⬜ pending |
| 32-01-09 | 01 | 1 | MCP-09 | unit | `cargo test --package glass_mcp -- agent_send` | ❌ W0 | ⬜ pending |
| 32-01-10 | 01 | 1 | MCP-10 | unit | `cargo test --package glass_mcp -- agent_messages` | ❌ W0 | ⬜ pending |
| 32-01-11 | 01 | 1 | MCP-11 | unit | `cargo test --package glass_mcp -- agent_heartbeat` | ❌ W0 | ⬜ pending |
| 32-01-12 | 01 | 1 | MCP-12 | unit | `cargo test --package glass_mcp -- implicit_heartbeat` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Parameter struct deserialization tests (RegisterParams, DeregisterParams, LockParams, UnlockParams, BroadcastParams, SendParams, MessagesParams, HeartbeatParams, StatusParams, ListAgentsParams, ListLocksParams)
- [ ] Response JSON structure validation tests
- [ ] Direct DB operation verification tests (open tempfile DB, call handler logic, check DB state)

*Tests should be synchronous unit tests matching existing pattern in tools.rs — no tokio runtime needed.*

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

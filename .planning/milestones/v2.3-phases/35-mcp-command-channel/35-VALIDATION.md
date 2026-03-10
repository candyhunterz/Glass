---
phase: 35
slug: mcp-command-channel
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-09
---

# Phase 35 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[test]` + `#[cfg(test)]` modules |
| **Config file** | Cargo.toml test configuration |
| **Quick run command** | `cargo test -p glass_core ipc --no-fail-fast` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_core -p glass_mcp --no-fail-fast`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 35-01-01 | 01 | 1 | INFRA-01 | integration | `cargo test -p glass_core ipc_roundtrip` | ❌ W0 | ⬜ pending |
| 35-01-02 | 01 | 1 | INFRA-01 | unit | `cargo test -p glass_core ipc_serde` | ❌ W0 | ⬜ pending |
| 35-01-03 | 01 | 1 | INFRA-02 | unit | Verify oneshot::send is non-blocking (by design) | ❌ W0 | ⬜ pending |
| 35-01-04 | 01 | 1 | INFRA-02 | unit | `cargo test -p glass_core ipc_unknown_method` | ❌ W0 | ⬜ pending |
| 35-02-01 | 02 | 1 | N/A | unit | `cargo test -p glass_mcp ipc_client_no_gui` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_core/src/ipc.rs` — IPC types, listener, connection handler (new file)
- [ ] Unit tests for McpRequest/McpResponse serialization
- [ ] Integration test: IPC round-trip (spawn listener, connect client, exchange JSON)
- [ ] Test: IPC client gets error when no listener is running

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| GUI remains responsive during MCP requests | INFRA-02 | Requires visual confirmation of frame rendering | 1. Start Glass 2. Send MCP request 3. Verify UI doesn't freeze |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

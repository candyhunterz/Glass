---
phase: 9
slug: mcp-server
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-05
---

# Phase 9 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test (cargo test) |
| **Config file** | Cargo.toml (workspace) |
| **Quick run command** | `cargo test -p glass_mcp` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_mcp`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 9-01-01 | 01 | 1 | MCP-01 | integration | `cargo test -p glass_mcp --test integration` | ❌ W0 | ⬜ pending |
| 9-01-02 | 01 | 1 | MCP-02 | unit | `cargo test -p glass_mcp -- glass_history` | ❌ W0 | ⬜ pending |
| 9-01-03 | 01 | 1 | MCP-03 | unit | `cargo test -p glass_mcp -- glass_context` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_mcp/tests/integration.rs` — MCP handshake integration test stub
- [ ] `crates/glass_mcp/src/tools.rs` — unit test stubs for tool parameter parsing
- [ ] Response types with Serialize derive for CommandRecord data

*Existing glass_history test infrastructure (temp DB fixtures) can be reused.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| stdout carries only JSON-RPC | MCP-01 | Requires checking no stray output | Run `glass mcp serve`, send initialize, verify no non-JSON lines on stdout |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

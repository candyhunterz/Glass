---
phase: 36
slug: multi-tab-orchestration
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-09
---

# Phase 36 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[test]` + `#[cfg(test)]` modules |
| **Config file** | Cargo.toml test configuration |
| **Quick run command** | `cargo test -p glass_mcp tab --no-fail-fast` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_mcp -p glass_core --no-fail-fast`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 36-01-01 | 01 | 1 | TAB-01 | unit | `cargo test -p glass_mcp tab_create` | ❌ W0 | ⬜ pending |
| 36-01-02 | 01 | 1 | TAB-02 | unit | `cargo test -p glass_mcp tab_list` | ❌ W0 | ⬜ pending |
| 36-01-03 | 01 | 1 | TAB-03 | unit | `cargo test -p glass_mcp tab_send` | ❌ W0 | ⬜ pending |
| 36-01-04 | 01 | 1 | TAB-04 | unit | `cargo test -p glass_mcp tab_output` | ❌ W0 | ⬜ pending |
| 36-01-05 | 01 | 1 | TAB-05 | unit | `cargo test -p glass_mcp tab_close_last` | ❌ W0 | ⬜ pending |
| 36-01-06 | 01 | 1 | TAB-06 | unit | `cargo test -p glass_mcp tab_target` | ❌ W0 | ⬜ pending |
| 36-01-07 | 01 | 1 | TAB-06 | unit | `cargo test -p glass_mcp tab_target_both` | ❌ W0 | ⬜ pending |
| 36-01-08 | 01 | 1 | TAB-06 | unit | `cargo test -p glass_mcp tab_target_neither` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Tab tool parameter struct serde/schemars tests (verify JSON deserialization)
- [ ] TabTarget resolution unit tests (index lookup, session_id lookup, error cases)
- [ ] IPC method round-trip tests (extend existing TCP-based test pattern from ipc.rs)
- [ ] Regex compilation error handling test for tab_output

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Tab appears in GUI after creation | TAB-01 | Requires running GUI window | Create tab via MCP, verify tab bar shows new tab |
| Tab bar updates on close | TAB-05 | Requires running GUI window | Close tab via MCP, verify tab bar updates |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

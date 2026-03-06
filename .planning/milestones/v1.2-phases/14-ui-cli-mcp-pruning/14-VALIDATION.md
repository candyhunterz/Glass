---
phase: 14
slug: ui-cli-mcp-pruning
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-05
---

# Phase 14 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test + cargo test |
| **Config file** | Cargo.toml (workspace) |
| **Quick run command** | `cargo test -p glass_snapshot --lib && cargo test -p glass_mcp --lib` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_snapshot --lib && cargo test -p glass_mcp --lib`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 14-01-01 | 01 | 1 | UI-01 | unit | `cargo test -p glass_renderer block_renderer` | W0 | pending |
| 14-01-02 | 01 | 1 | UI-02 | unit | `cargo test -p glass_snapshot undo` | W0 | pending |
| 14-02-01 | 02 | 1 | UI-03 | unit + integration | `cargo test -p glass undo` | W0 | pending |
| 14-02-02 | 02 | 1 | STOR-01 | unit | `cargo test -p glass_snapshot pruner` | W0 | pending |
| 14-03-01 | 03 | 1 | MCP-01 | unit | `cargo test -p glass_mcp glass_undo` | W0 | pending |
| 14-03-02 | 03 | 1 | MCP-02 | unit | `cargo test -p glass_mcp glass_file_diff` | W0 | pending |

*Status: pending / green / red / flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_snapshot/src/pruner.rs` — stubs for STOR-01 pruning tests
- [ ] Tests for `undo_command(command_id)` in `undo.rs` — UI-03
- [ ] Tests for pruning DB queries in `db.rs` — STOR-01
- [ ] Tests for MCP undo/diff tools in `glass_mcp/src/tools.rs` — MCP-01, MCP-02
- [ ] Tests for `[undo]` label positioning in `block_renderer.rs` — UI-01
- [ ] Tests for undo visual feedback formatting — UI-02

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| [undo] label visually positioned correctly in block header | UI-01 | Requires GPU rendering pipeline | Launch Glass, run a file-modifying command, verify [undo] label appears |
| Undo feedback overlay displays and auto-dismisses | UI-02 | Requires live terminal rendering | Press Ctrl+Shift+Z after a file-modifying command, verify feedback appears |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

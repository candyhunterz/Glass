---
phase: 37
slug: token-saving-tools
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-10
---

# Phase 37 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in (`#[cfg(test)] mod tests`) + cargo test |
| **Config file** | Cargo workspace (already configured) |
| **Quick run command** | `cargo test -p glass_mcp` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_mcp`
- **After every plan wave:** Run `cargo test --workspace && cargo clippy --workspace -- -D warnings`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 37-01-01 | 01 | 1 | TOKEN-01 | unit | `cargo test -p glass_mcp -- test_output_filter_params -x` | ❌ W0 | ⬜ pending |
| 37-01-02 | 01 | 1 | TOKEN-01 | unit | `cargo test -- test_extract_term_lines_head -x` | ❌ W0 | ⬜ pending |
| 37-01-03 | 01 | 1 | TOKEN-02 | unit | `cargo test -p glass_mcp -- test_cache_check_params -x` | ❌ W0 | ⬜ pending |
| 37-01-04 | 01 | 1 | TOKEN-02 | unit | `cargo test -p glass_mcp -- test_staleness_detection -x` | ❌ W0 | ⬜ pending |
| 37-01-05 | 01 | 1 | TOKEN-02 | unit | `cargo test -p glass_mcp -- test_cache_valid -x` | ❌ W0 | ⬜ pending |
| 37-02-01 | 02 | 2 | TOKEN-03 | unit | `cargo test -p glass_mcp -- test_command_diff_params -x` | ❌ W0 | ⬜ pending |
| 37-02-02 | 02 | 2 | TOKEN-03 | unit | `cargo test -p glass_mcp -- test_unified_diff -x` | ❌ W0 | ⬜ pending |
| 37-02-03 | 02 | 2 | TOKEN-04 | unit | `cargo test -p glass_mcp -- test_compressed_context_params -x` | ❌ W0 | ⬜ pending |
| 37-02-04 | 02 | 2 | TOKEN-04 | unit | `cargo test -p glass_mcp -- test_budget_truncation -x` | ❌ W0 | ⬜ pending |
| 37-02-05 | 02 | 2 | TOKEN-04 | unit | `cargo test -p glass_mcp -- test_focus_mode -x` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Test stubs for all TOKEN-01 through TOKEN-04 param deserialization in `crates/glass_mcp/src/tools.rs`
- [ ] Test stubs for staleness detection logic
- [ ] Test stubs for unified diff generation
- [ ] Test stubs for budget truncation and focus mode
- [ ] `similar` crate added to `glass_mcp/Cargo.toml`

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Head/tail mode returns correct lines from live terminal | TOKEN-01 | Requires running terminal with active PTY | Start Glass, run a command with many output lines, call glass_tab_output with mode="head" and mode="tail", verify correct lines returned |
| IPC round-trip for output_filter | TOKEN-01 | Requires GUI event loop + MCP server | Start Glass with MCP, call tool via MCP client, verify response |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

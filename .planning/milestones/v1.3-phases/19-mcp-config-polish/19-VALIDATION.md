---
phase: 19
slug: mcp-config-polish
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-06
---

# Phase 19 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust built-in) |
| **Config file** | Cargo.toml workspace + per-crate Cargo.toml |
| **Quick run command** | `cargo test -p glass_mcp && cargo test -p glass_core` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_mcp && cargo test -p glass_core`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 19-01-01 | 01 | 1 | MCP-01 | unit | `cargo test -p glass_mcp -- pipe_inspect` | Wave 0 | pending |
| 19-01-02 | 01 | 1 | MCP-01 | unit | `cargo test -p glass_mcp -- pipe_inspect_params` | Wave 0 | pending |
| 19-01-03 | 01 | 1 | MCP-01 | unit | `cargo test -p glass_mcp -- pipe_inspect` | Wave 0 | pending |
| 19-01-04 | 01 | 1 | MCP-02 | unit | `cargo test -p glass_mcp -- context` | Wave 0 | pending |
| 19-01-05 | 01 | 1 | MCP-02 | unit | `cargo test -p glass_mcp -- context` | Wave 0 | pending |
| 19-01-06 | 01 | 1 | CONF-01 | unit | `cargo test -p glass_core -- pipes` | Wave 0 | pending |
| 19-01-07 | 01 | 1 | CONF-01 | unit | `cargo test -p glass_core -- pipes` | Wave 0 | pending |
| 19-01-08 | 01 | 1 | CONF-01 | integration | `cargo test --workspace` | Wave 0 | pending |

*Status: pending / green / red / flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_mcp/src/tools.rs` — add PipeInspectParams and glass_pipe_inspect handler tests in existing `#[cfg(test)] mod tests`
- [ ] `crates/glass_mcp/src/context.rs` — add pipeline_count/avg_stages/failure_rate tests in existing test module
- [ ] `crates/glass_core/src/config.rs` — add PipesSection tests in existing `#[cfg(test)] mod tests`
- [ ] No new test files needed — all tests go in existing test modules

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| max_capture_mb wired to BufferPolicy | CONF-01 | Requires running Glass with config and observing buffer behavior | Set `max_capture_mb = 1` in config.toml, run a pipeline producing >1MB, verify truncation |
| auto_expand=false disables expansion | CONF-01 | Requires TUI observation | Set `auto_expand = false`, run a failed pipeline, verify it stays collapsed |
| enabled=false skips pipeline capture | CONF-01 | Requires observing temp file processing | Set `enabled = false`, run a pipeline, verify no stage data in DB |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

---
phase: 53
slug: soi-mcp-tools
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-13
---

# Phase 53 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in (`cargo test`) |
| **Config file** | `Cargo.toml` workspace — no separate test config |
| **Quick run command** | `cargo test -p glass_mcp -p glass_history` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_mcp -p glass_history`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 53-01-01 | 01 | 1 | SOIM-01 | unit | `cargo test -p glass_mcp tests::test_glass_query` | ❌ W0 | ⬜ pending |
| 53-01-02 | 01 | 1 | SOIM-01 | unit | `cargo test -p glass_mcp tests::test_glass_query_no_soi` | ❌ W0 | ⬜ pending |
| 53-01-03 | 01 | 1 | SOIM-02 | unit | `cargo test -p glass_history soi::tests::test_get_last_n_run_ids` | ❌ W0 | ⬜ pending |
| 53-01-04 | 01 | 1 | SOIM-02 | unit | `cargo test -p glass_mcp tests::test_glass_query_trend_regression` | ❌ W0 | ⬜ pending |
| 53-01-05 | 01 | 1 | SOIM-03 | unit | `cargo test -p glass_mcp tests::test_glass_query_drill_found` | ❌ W0 | ⬜ pending |
| 53-01-06 | 01 | 1 | SOIM-03 | unit | `cargo test -p glass_mcp tests::test_glass_query_drill_not_found` | ❌ W0 | ⬜ pending |
| 53-01-07 | 01 | 1 | SOIM-04 | unit | `cargo test -p glass_mcp context::tests::test_context_soi_summaries` | ❌ W0 | ⬜ pending |
| 53-01-08 | 01 | 1 | SOIM-04 | unit | `cargo test -p glass_mcp tests::test_compressed_context_soi_section` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Test helpers for creating HistoryDb with SOI data (reuse pattern from `glass_history/src/soi.rs` test module)
- [ ] `crates/glass_mcp/src/tools.rs` tests section — needs tempfile + HistoryDb + insert_parsed_output setup fixtures
- [ ] `get_last_n_run_ids()` method on HistoryDb in `glass_history/src/db.rs`

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Tool schema token footprint audit | SOIM-01 | One-time measurement, not a regression test | Serialize tool_router() tool list to JSON, measure bytes before/after |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

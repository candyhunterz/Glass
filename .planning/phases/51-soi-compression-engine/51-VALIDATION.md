---
phase: 51
slug: soi-compression-engine
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-13
---

# Phase 51 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in (`#[test]`) |
| **Config file** | None — tests inline per project convention |
| **Quick run command** | `cargo test -p glass_history -- compress` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_history -- compress`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 51-01-01 | 01 | 1 | SOIC-01 | unit | `cargo test -p glass_history -- compress_one_line_failed_build` | ❌ W0 | ⬜ pending |
| 51-01-02 | 01 | 1 | SOIC-01 | unit | `cargo test -p glass_history -- compress_summary_budget` | ❌ W0 | ⬜ pending |
| 51-01-03 | 01 | 1 | SOIC-01 | unit | `cargo test -p glass_history -- compress_detailed_budget` | ❌ W0 | ⬜ pending |
| 51-01-04 | 01 | 1 | SOIC-01 | unit | `cargo test -p glass_history -- compress_full_budget_no_truncation` | ❌ W0 | ⬜ pending |
| 51-01-05 | 01 | 1 | SOIC-02 | unit | `cargo test -p glass_history -- compress_errors_before_warnings` | ❌ W0 | ⬜ pending |
| 51-01-06 | 01 | 1 | SOIC-03 | unit | `cargo test -p glass_history -- compress_drill_down_record_ids` | ❌ W0 | ⬜ pending |
| 51-02-01 | 02 | 1 | SOIC-04 | unit | `cargo test -p glass_history -- diff_compress_second_run` | ❌ W0 | ⬜ pending |
| 51-02-02 | 02 | 1 | SOIC-04 | unit | `cargo test -p glass_history -- diff_compress_first_run_no_prior` | ❌ W0 | ⬜ pending |
| 51-02-03 | 02 | 1 | SOIC-04 | unit | `cargo test -p glass_history -- diff_compress_empty_previous` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_history/src/compress.rs` — new module with `TokenBudget`, `CompressedOutput`, `DiffSummary`, `RecordFingerprint`, `compress()`, `diff_compress()` and all tests
- [ ] `crates/glass_history/src/soi.rs` — add `get_previous_run_records()` function
- [ ] `crates/glass_history/src/db.rs` — add `get_previous_run_records()` delegation method
- [ ] `crates/glass_history/src/lib.rs` — re-export `compress` module

*No test framework install needed — Rust built-in tests already in use project-wide.*

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

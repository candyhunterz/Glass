---
phase: 17
slug: pipeline-ui
status: validated
nyquist_compliant: true
wave_0_complete: true
created: 2026-03-05
validated: 2026-03-06
---

# Phase 17 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test (cargo test) |
| **Config file** | Cargo.toml workspace |
| **Quick run command** | `cargo test -p glass_terminal -p glass_renderer --lib` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_terminal -p glass_renderer --lib`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | Status |
|---------|------|------|-------------|-----------|-------------------|--------|
| 17-01-01 | 01 | 1 | UI-01 | unit | `cargo test -p glass_terminal --lib block_manager::tests::pipeline_stage_commands_stored` | green |
| 17-01-02 | 01 | 1 | UI-01 | unit | `cargo test -p glass_renderer --lib block_renderer::tests` | green |
| 17-01-03 | 01 | 1 | UI-02 | unit | `cargo test -p glass_terminal --lib block_manager::tests::pipeline_auto_expand` | green |
| 17-01-04 | 01 | 1 | UI-02 | unit | `cargo test -p glass_terminal --lib block_manager::tests::pipeline_auto_collapse` | green |
| 17-01-05 | 01 | 1 | UI-03 | unit | `cargo test -p glass_terminal --lib block_manager::tests::set_expanded_stage` | green |
| 17-01-06 | 01 | 1 | UI-04 | unit | `cargo test -p glass_terminal --lib block_manager::tests::pipeline_hit_test` | green |

---

## Wave 0 Requirements

- [x] `block_manager::tests::pipeline_auto_expand_on_failure` — covers UI-02 auto-expand logic
- [x] `block_manager::tests::pipeline_auto_expand_on_many_stages` — covers UI-02 auto-expand logic
- [x] `block_manager::tests::pipeline_auto_collapse_simple_success` — covers UI-02 auto-collapse logic
- [x] `block_manager::tests::pipeline_stage_commands_stored` — covers UI-01 command text storage
- [x] `block_manager::tests::set_expanded_stage_sets_and_clears` — covers UI-03 stage expansion toggle
- [x] `block_manager::tests::toggle_pipeline_expanded_clears_expanded_stage` — covers UI-03/UI-04 toggle
- [x] `block_renderer::tests::test_pipeline_rects_*` (3 tests) — covers UI-01 rect generation
- [x] `block_renderer::tests::test_pipeline_text_*` (5 tests) — covers UI-01 label generation
- [x] `block_renderer::tests::test_line_count_*` (5 tests) — covers UI-01 line counting
- [x] `block_renderer::tests::test_format_bytes_*` (3 tests) — covers UI-01 byte formatting
- [x] `block_manager::tests::pipeline_hit_test_*` (4 tests) — covers UI-04 click detection

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Visual rendering of pipeline rows | UI-01 | GPU rendering output cannot be unit tested | Run `cat file \| grep foo \| wc -l` and verify multi-row block appears with stage info |
| Click to expand/collapse | UI-04 | Requires mouse interaction with rendered window | Click pipeline stage row, verify expansion toggles |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 15s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** validated

## Validation Audit 2026-03-06

| Metric | Count |
|--------|-------|
| Gaps found | 2 |
| Resolved | 2 |
| Escalated | 0 |

---
phase: 17
slug: pipeline-ui
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-05
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

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 17-01-01 | 01 | 1 | UI-01 | unit | `cargo test -p glass_terminal --lib block_manager::tests::pipeline_stage_command_text` | Wave 0 | pending |
| 17-01-02 | 01 | 1 | UI-01 | unit | `cargo test -p glass_renderer --lib block_renderer` | Wave 0 | pending |
| 17-01-03 | 01 | 1 | UI-02 | unit | `cargo test -p glass_terminal --lib block_manager::tests::pipeline_auto_expand` | Wave 0 | pending |
| 17-01-04 | 01 | 1 | UI-02 | unit | `cargo test -p glass_terminal --lib block_manager::tests::pipeline_auto_collapse` | Wave 0 | pending |
| 17-01-05 | 01 | 1 | UI-03 | unit | `cargo test -p glass_terminal --lib block_manager::tests::pipeline_stage_expand_toggle` | Wave 0 | pending |
| 17-01-06 | 01 | 1 | UI-04 | unit | `cargo test -p glass_renderer --lib block_renderer::tests::pipeline_hit_test` | Wave 0 | pending |

*Status: pending / green / red / flaky*

---

## Wave 0 Requirements

- [ ] `block_manager::tests::pipeline_auto_expand` — covers UI-02 auto-expand logic
- [ ] `block_manager::tests::pipeline_auto_collapse` — covers UI-02 auto-collapse logic
- [ ] `block_manager::tests::pipeline_stage_command_text` — covers UI-01 command text storage
- [ ] `block_manager::tests::pipeline_stage_expand_toggle` — covers UI-03 stage expansion toggle
- [ ] `block_renderer` pipeline rendering tests — covers UI-01 rect/label generation
- [ ] Hit test helper tests — covers UI-04 click detection

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Visual rendering of pipeline rows | UI-01 | GPU rendering output cannot be unit tested | Run `cat file \| grep foo \| wc -l` and verify multi-row block appears with stage info |
| Click to expand/collapse | UI-04 | Requires mouse interaction with rendered window | Click pipeline stage row, verify expansion toggles |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

---
phase: 57
slug: agent-worktree
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-13
---

# Phase 57 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test (`cargo test`) |
| **Config file** | None (inline `#[cfg(test)]`) |
| **Quick run command** | `cargo test -p glass_agent` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_agent`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 57-01-01 | 01 | 1 | AGTW-01 | unit | `cargo test -p glass_agent -- worktree_manager::tests::test_create_worktree_git` | ❌ W0 | ⬜ pending |
| 57-01-02 | 01 | 1 | AGTW-01 | unit | `cargo test -p glass_agent -- worktree_manager::tests::test_write_files_to_worktree` | ❌ W0 | ⬜ pending |
| 57-01-03 | 01 | 1 | AGTW-02 | unit | `cargo test -p glass_agent -- worktree_manager::tests::test_generate_diff` | ❌ W0 | ⬜ pending |
| 57-01-04 | 01 | 1 | AGTW-02 | unit | `cargo test -p glass_agent -- worktree_manager::tests::test_diff_binary_file` | ❌ W0 | ⬜ pending |
| 57-01-05 | 01 | 1 | AGTW-03 | unit | `cargo test -p glass_agent -- worktree_manager::tests::test_apply_copies_files` | ❌ W0 | ⬜ pending |
| 57-01-06 | 01 | 1 | AGTW-04 | unit | `cargo test -p glass_agent -- worktree_manager::tests::test_cleanup_removes_dir` | ❌ W0 | ⬜ pending |
| 57-01-07 | 01 | 1 | AGTW-04 | unit | `cargo test -p glass_agent -- worktree_manager::tests::test_dismiss_no_working_tree_change` | ❌ W0 | ⬜ pending |
| 57-01-08 | 01 | 1 | AGTW-05 | unit | `cargo test -p glass_agent -- worktree_manager::tests::test_prune_orphans` | ❌ W0 | ⬜ pending |
| 57-01-09 | 01 | 1 | AGTW-05 | unit | `cargo test -p glass_agent -- worktree_db::tests::test_pending_row_survives_restart` | ❌ W0 | ⬜ pending |
| 57-01-10 | 01 | 1 | AGTW-06 | unit | `cargo test -p glass_agent -- worktree_manager::tests::test_non_git_fallback` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_agent/` — new crate directory with `Cargo.toml` and `src/lib.rs`
- [ ] `crates/glass_agent/src/types.rs` — `WorktreeHandle`, `WorktreeKind`, `PendingWorktree`
- [ ] `crates/glass_agent/src/worktree_manager.rs` — `WorktreeManager` struct with test stubs
- [ ] `crates/glass_agent/src/worktree_db.rs` — `pending_worktrees` table helpers with test stubs
- [ ] Workspace `Cargo.toml` — add `git2 = "0.20"` and `diffy` to `[workspace.dependencies]`
- [ ] Verify `diffy` published version: `cargo search diffy`

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Startup orphan pruning after crash | AGTW-05 | Requires simulating crash mid-creation | 1. Create worktree + pending row, 2. Kill Glass, 3. Restart, 4. Verify worktree pruned |
| Non-git fallback in real non-git dir | AGTW-06 | Needs a real non-git project directory | 1. Open Glass in non-git dir, 2. Trigger agent proposal, 3. Verify temp dir used |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

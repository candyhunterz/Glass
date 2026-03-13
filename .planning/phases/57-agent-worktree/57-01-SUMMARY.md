---
phase: 57-agent-worktree
plan: "01"
subsystem: glass_agent
tags: [worktree, git2, sqlite, crash-recovery, diff]
dependency_graph:
  requires: []
  provides: [glass_agent::WorktreeManager, glass_agent::WorktreeDb, glass_agent::WorktreeHandle]
  affects: [workspace Cargo.toml, Cargo.lock]
tech_stack:
  added: [git2 0.20.4, diffy 0.4.2, uuid 1 (workspace)]
  patterns: [register-before-create crash recovery, RefCell interior mutability for DB, git2 worktree API, diffy unified diff]
key_files:
  created:
    - crates/glass_agent/Cargo.toml
    - crates/glass_agent/src/lib.rs
    - crates/glass_agent/src/types.rs
    - crates/glass_agent/src/worktree_db.rs
    - crates/glass_agent/src/worktree_manager.rs
  modified:
    - Cargo.toml
    - Cargo.lock
decisions:
  - "WorktreeDb uses &mut self for write methods (BEGIN IMMEDIATE transaction), WorktreeManager wraps it in RefCell for interior mutability from &self callers"
  - "create_worktree_inner creates base_dir before git worktree add (git2 requires parent to exist)"
  - "init_git_repo test helper drops tree before returning repo to satisfy borrow checker"
metrics:
  duration_seconds: 394
  completed_date: "2026-03-13"
  tasks_completed: 2
  files_created: 5
  files_modified: 2
  tests_added: 15
requirements-completed: [AGTW-06]
---

# Phase 57 Plan 01: glass_agent Crate - WorktreeManager and SQLite Crash Recovery Summary

**One-liner:** New `glass_agent` crate with `WorktreeManager` isolating agent proposals in git worktrees (or plain-dir fallback) with SQLite "register-before-create" crash recovery and unified diff via `diffy`.

## What Was Built

Created the `glass_agent` crate from scratch with a complete worktree lifecycle: create, diff, apply, dismiss, and prune orphans. The crate is the foundation for Phase 58's approval UI.

### Files Created

- `crates/glass_agent/Cargo.toml` — Crate manifest with git2, diffy, rusqlite, uuid, dirs, anyhow, tracing, serde_json workspace deps
- `crates/glass_agent/src/types.rs` — `WorktreeKind` (Git/TempDir), `WorktreeHandle`, `PendingWorktree`
- `crates/glass_agent/src/worktree_db.rs` — `WorktreeDb` with migration to version 2 (adds `pending_worktrees` table to `~/.glass/agents.db`); CRUD: insert/list/delete with BEGIN IMMEDIATE transactions
- `crates/glass_agent/src/worktree_manager.rs` — `WorktreeManager` with `create_worktree`, `generate_diff`, `apply`, `dismiss`, `cleanup`, `prune_orphans`; stores DB in `RefCell<WorktreeDb>` for interior mutability
- `crates/glass_agent/src/lib.rs` — Public re-exports of `WorktreeManager`, `WorktreeDb`, `WorktreeHandle`, `WorktreeKind`, `PendingWorktree`

### Workspace Changes

- Added `git2 = "0.20"`, `diffy = "0.4"`, `uuid = { version = "1", features = ["v4"] }`, `serde_json = "1.0"` to `[workspace.dependencies]` in root `Cargo.toml`
- Updated root package `serde_json` references to use `workspace = true`

## Tests

All 15 tests pass:

**WorktreeDb (6 tests):**
- `test_open_creates_table` — DB opens, pending_worktrees table created
- `test_insert_and_list` — Row stored and retrievable
- `test_delete_removes_row` — Row removed after delete
- `test_list_empty_on_fresh_db` — Empty Vec on fresh DB
- `test_pending_row_survives_restart` — Row survives connection close+reopen
- `test_migration_version_2` — PRAGMA user_version = 2 after migration

**WorktreeManager (9 tests):**
- `test_create_worktree_git` — Creates git linked worktree, returns WorktreeKind::Git
- `test_create_worktree_writes_file_changes` — File content written into worktree
- `test_create_worktree_non_git_fallback` — Plain directory used for non-git project
- `test_generate_diff_text` — Unified diff with --- a/ +++ b/ headers and change lines
- `test_generate_diff_binary_placeholder` — "Binary file" emitted for non-UTF-8 files
- `test_apply_copies_files_to_working_tree` — Files copied to project root, worktree removed
- `test_dismiss_removes_worktree_without_touching_working_tree` — Worktree gone, project unchanged
- `test_prune_orphans` — Simulated crash orphan dir + DB row pruned on startup
- `test_register_before_create_invariant` — DB row present after create, gone after dismiss

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Added base_dir creation before git worktree add**
- **Found during:** Task 2 test run
- **Issue:** `repo.worktree(id, worktree_path, None)` fails with "failed to make directory" when the parent `~/.glass/worktrees/` doesn't exist yet
- **Fix:** Added `std::fs::create_dir_all(parent)` for `worktree_path.parent()` at the start of `create_worktree_inner`
- **Files modified:** `crates/glass_agent/src/worktree_manager.rs`
- **Commit:** cb786b4

**2. [Rule 1 - Bug] RefCell interior mutability for WorktreeDb in WorktreeManager**
- **Found during:** Task 1 compile phase
- **Issue:** `WorktreeDb` write methods (`insert_pending_worktree`, `delete_pending_worktree`) require `&mut self` due to rusqlite transaction API; `WorktreeManager` public API should take `&self`
- **Fix:** Wrapped `WorktreeDb` in `RefCell<WorktreeDb>` in `WorktreeManager`, changed all DB calls to `self.db.borrow_mut().method()` and `self.db.borrow().method()`
- **Files modified:** `crates/glass_agent/src/worktree_manager.rs`
- **Commit:** 884b73c

**3. [Rule 1 - Bug] init_git_repo test helper borrow issue**
- **Found during:** Task 2 compile phase
- **Issue:** Returning `repo` while `tree` (borrowed from `repo`) is still in scope causes E0505
- **Fix:** Wrapped `tree` creation and commit in an inner block; changed return type to `()` since callers don't need the repo handle
- **Files modified:** `crates/glass_agent/src/worktree_manager.rs`
- **Commit:** 884b73c

## Self-Check: PASSED

- All 5 created files exist
- Commits 884b73c and cb786b4 exist in git log
- 15/15 tests pass, workspace tests fully green

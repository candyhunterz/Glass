# Phase 57: Agent Worktree - Research

**Researched:** 2026-03-13
**Domain:** Git worktree isolation, SQLite crash-recovery, unified diff generation, file copy apply/dismiss, non-git fallback
**Confidence:** HIGH

## Summary

Phase 57 adds a `WorktreeManager` that isolates every agent code-change proposal in a dedicated git worktree (under `~/.glass/worktrees/<uuid>/`) so the user's working tree is never touched until they explicitly approve. The SQLite "register before create" pattern (sourced from the opencode PR #14649 crash-recovery precedent, noted in STATE.md) ensures that even a mid-creation crash leaves a recoverable `pending_worktree` row that gets pruned on next Glass startup.

The standard mechanism is `git2` crate for all git operations (worktree add, list, prune) and `std::fs` for the apply (copy) and dismiss (remove_dir_all) operations. Unified diff is produced by the `diffy` crate, which operates purely on in-memory strings and requires no external `diff` binary. Non-git projects fall back to a `tempfile::TempDir` (or a manually managed dir under `~/.glass/worktrees/`) that mirrors the working tree files the agent intends to change, with the same apply/dismiss interface.

The hardest technical problems are: (1) correctly using `git2`'s `Repository::worktree` / `worktrees()` / `find_worktree()` API, which differs subtly from the CLI `git worktree add` command; (2) the "register-before-create" ordering discipline in SQLite to survive crashes; (3) generating a clean per-file unified diff that covers only the files the agent changed (not the entire working tree); and (4) Windows path handling with `git2` — the STATE.md explicitly flags this as a known risk.

**Primary recommendation:** Add `glass_agent` as a new crate containing `WorktreeManager`. Use `git2` for worktree operations, `diffy` for unified diff, `rusqlite` for `pending_worktrees` registration (in the existing `~/.glass/agents.db`), and `std::fs::copy` + `std::fs::remove_dir_all` for apply/dismiss. The `WorktreeManager` is instantiated in `Processor` alongside `AgentRuntime` and called when a proposal is accepted or dismissed.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| AGTW-01 | WorktreeManager creates isolated git worktrees for agent code changes | `git2::Repository::worktree(name, path, opts)` creates a linked worktree; `glass_agent::worktree_manager::WorktreeManager` struct owns the lifecycle |
| AGTW-02 | Unified diff generated between worktree and main working tree for review | `diffy::create_patch(original, modified)` produces a unified diff string; iterate changed files, read both versions, call diffy per file |
| AGTW-03 | Apply copies changed files from worktree to working tree on user approval | `std::fs::copy(worktree_path, working_tree_path)` for each changed file; call `cleanup_worktree` after |
| AGTW-04 | Cleanup removes worktree after apply or dismiss | `git2::Worktree::prune(opts)` (with `lock: false`); fall back to `std::fs::remove_dir_all` + `git worktree prune` for the non-git case |
| AGTW-05 | Crash recovery via SQLite-registered pending worktrees pruned on startup | INSERT `pending_worktrees` row BEFORE `git worktree add`; on startup scan rows and call `prune_orphan` for each; delete row after successful prune |
| AGTW-06 | Non-git projects fall back to temp directory with file copies | Detect non-git with `git2::Repository::discover(path).is_err()`; use a manually managed dir under `~/.glass/worktrees/<uuid>/` with copies of the target files |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `git2` | 0.20 | Git worktree add, list, find, prune via libgit2 C binding | Rust-native; no subprocess; cross-platform; already planned in STATE.md decisions |
| `diffy` | 0.4 | Pure-Rust unified diff generation | No external `diff` binary dependency; produces standard unified diff format; works on &str |
| `rusqlite` | 0.38 (workspace) | `pending_worktrees` table in `~/.glass/agents.db` for crash recovery | Already workspace dep; established migration pattern via `PRAGMA user_version` |
| `uuid` | 1 | Unique worktree directory names under `~/.glass/worktrees/` | Already in `glass_coordination/Cargo.toml`; needs promoting to workspace or adding to `glass_agent` |
| `std::fs` | stdlib | File copy (apply) and remove_dir_all (dismiss/cleanup) | No dep; correct tool for the job |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `tempfile` | 3 (dev-deps) | TempDir in tests only | Tests that need ephemeral directories without touching `~/.glass` |
| `dirs` | 6 (workspace) | Resolve `~/.glass/worktrees/` path | Consistent with every other crate using `dirs::home_dir` |
| `tracing` | workspace | Log worktree create/apply/dismiss/prune events | Project convention |
| `anyhow` | workspace | `Result` propagation in WorktreeManager | Project convention |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `git2` worktree API | `std::process::Command("git worktree add")` | subprocess approach works but `git2` is safer on Windows paths with spaces; no PATH dependency; avoids output parsing |
| `diffy` | `similar` crate | `similar` is more feature-rich but heavier; `diffy` is minimal and produces standard unified diff; sufficient for read-only display |
| `diffy` | shell-out to `diff -u` | `diff` not guaranteed available on Windows; pure Rust is correct choice |
| separate DB for worktrees | reuse `~/.glass/agents.db` | agents.db already has WAL mode and migration infrastructure; no new DB needed |

**Installation — add to workspace `Cargo.toml` `[workspace.dependencies]`:**
```toml
git2  = "0.20"
diffy = "0.4"
```

**New crate `Cargo.toml` (crates/glass_agent/Cargo.toml):**
```toml
[package]
name = "glass_agent"
version = "0.1.0"
edition = "2021"

[dependencies]
git2     = { workspace = true }
diffy    = { workspace = true }
rusqlite = { workspace = true }
uuid     = { version = "1", features = ["v4"] }
dirs     = { workspace = true }
anyhow   = { workspace = true }
tracing  = { workspace = true }
```

## Architecture Patterns

### Recommended Module Structure
```
crates/glass_agent/
├── Cargo.toml
└── src/
    ├── lib.rs              # pub use worktree_manager::WorktreeManager; pub use types::*;
    ├── types.rs            # WorktreeHandle, WorktreeKind, WorktreeStatus, PendingWorktree
    ├── worktree_manager.rs # WorktreeManager struct: create, diff, apply, dismiss, prune_orphans
    └── worktree_db.rs      # pending_worktrees SQLite table: insert, list, delete, migrate

src/main.rs
└── Processor
    ├── worktree_manager: glass_agent::WorktreeManager
    └── handle_proposal_accept(proposal_id) / handle_proposal_dismiss(proposal_id)
```

### Pattern 1: SQLite "Register Before Create" (AGTW-05)
**What:** INSERT a `pending_worktrees` row BEFORE calling `git worktree add`. On success, the row is deleted after the worktree is confirmed created. On startup, any surviving rows indicate orphaned worktrees from a crash.
**When to use:** Every worktree creation. Non-negotiable — this is the crash-safety invariant.
**Source:** STATE.md key decision: "git worktree registered in SQLite BEFORE creation -- crash recovery pattern from opencode PR #14649"

```rust
// Source: pattern from opencode PR #14649 (crash recovery invariant)
pub fn create_worktree(&self, project_root: &Path, proposal_id: &str) -> Result<WorktreeHandle> {
    let id = uuid::Uuid::new_v4().to_string();
    let worktree_path = self.base_dir.join(&id);

    // STEP 1: Register BEFORE creating -- crash safety invariant
    self.db.insert_pending_worktree(&id, &worktree_path, proposal_id)?;

    // STEP 2: Create worktree (may fail or crash here)
    let result = self.create_git_worktree(project_root, &worktree_path, &id);

    match result {
        Ok(handle) => {
            // STEP 3: On success, DO NOT delete the row yet --
            // it stays until apply or dismiss completes
            Ok(handle)
        }
        Err(e) => {
            // Creation failed: delete row immediately (not a crash orphan)
            let _ = self.db.delete_pending_worktree(&id);
            Err(e)
        }
    }
}
```

### Pattern 2: git2 Worktree Creation (AGTW-01)
**What:** Use `git2::Repository::worktree()` to create a linked worktree at a new path. The worktree gets a detached HEAD so agent changes don't affect the main branch.
**When to use:** When the project root is a git repository.

```rust
// Source: git2 crate docs — Repository::worktree()
use git2::{Repository, WorktreeAddOptions};

fn create_git_worktree(
    repo: &Repository,
    worktree_path: &Path,
    name: &str,
) -> Result<()> {
    let mut opts = WorktreeAddOptions::new();
    // No --detach needed: linked worktrees get their own HEAD automatically
    // Do NOT set a branch -- let git create a detached HEAD
    repo.worktree(name, worktree_path, Some(&opts))?;
    Ok(())
}
```

**CRITICAL git2 API note:** `Repository::worktree(name, path, opts)` takes:
- `name`: unique short name for the worktree (used in `.git/worktrees/<name>/`)
- `path`: filesystem path where the worktree should be created
- `opts`: `Option<&WorktreeAddOptions>` — pass `Some` or `None`

The worktree is created with a detached HEAD pointing to the current HEAD commit of the main worktree.

### Pattern 3: Copying Agent Changes into Worktree (AGTW-01)
**What:** After the worktree is created, the agent's proposed file changes (provided as `(relative_path, new_content)` pairs from the proposal) are written into the worktree directory. The worktree is NOT modified by running the agent inside it — Glass writes the files directly.
**When to use:** Immediately after worktree creation, before returning to the proposal review flow.

```rust
// Write proposed file changes into the worktree
for (rel_path, content) in &proposal.file_changes {
    let dest = worktree_path.join(rel_path);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, content)?;
}
```

### Pattern 4: Unified Diff Generation (AGTW-02)
**What:** For each file the agent changed, read the current working tree version and the worktree version, then call `diffy::create_patch` to produce a unified diff string.
**When to use:** When the user triggers a review (Phase 58 UI, or for the proposal data structure).

```rust
// Source: diffy crate docs — diffy::create_patch
use diffy::create_patch;

pub fn generate_diff(&self, handle: &WorktreeHandle) -> Result<String> {
    let mut full_diff = String::new();
    for rel_path in &handle.changed_files {
        let working_path = handle.project_root.join(rel_path);
        let worktree_path = handle.worktree_path.join(rel_path);

        let original = if working_path.exists() {
            std::fs::read_to_string(&working_path).unwrap_or_default()
        } else {
            String::new()  // new file added by agent
        };
        let modified = std::fs::read_to_string(&worktree_path).unwrap_or_default();

        let patch = create_patch(&original, &modified);
        full_diff.push_str(&format!("--- a/{}\n+++ b/{}\n", rel_path.display(), rel_path.display()));
        full_diff.push_str(&patch.to_string());
        full_diff.push('\n');
    }
    Ok(full_diff)
}
```

### Pattern 5: Apply (AGTW-03)
**What:** Copy changed files from the worktree to the working tree, then clean up the worktree.
**When to use:** User accepts the proposal.

```rust
pub fn apply(&self, handle: WorktreeHandle) -> Result<()> {
    for rel_path in &handle.changed_files {
        let src = handle.worktree_path.join(rel_path);
        let dst = handle.project_root.join(rel_path);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&src, &dst)?;
    }
    self.cleanup(handle)
}
```

### Pattern 6: Dismiss and Cleanup (AGTW-04)
**What:** Remove the worktree directory and prune the git worktree reference, then delete the `pending_worktrees` row.
**When to use:** User dismisses a proposal, OR after a successful apply.

```rust
pub fn cleanup(&self, handle: WorktreeHandle) -> Result<()> {
    match &handle.kind {
        WorktreeKind::Git { repo_path } => {
            let repo = git2::Repository::open(repo_path)?;
            // Find the worktree by name and prune it
            if let Ok(wt) = repo.find_worktree(&handle.id) {
                let mut prune_opts = git2::WorktreePruneOptions::new();
                prune_opts.valid(true);   // prune even if path still exists
                wt.prune(Some(&mut prune_opts))?;
            }
            // Belt-and-suspenders: remove dir if prune left it
            if handle.worktree_path.exists() {
                std::fs::remove_dir_all(&handle.worktree_path)?;
            }
        }
        WorktreeKind::TempDir => {
            // Non-git fallback: just remove the directory
            if handle.worktree_path.exists() {
                std::fs::remove_dir_all(&handle.worktree_path)?;
            }
        }
    }
    self.db.delete_pending_worktree(&handle.id)?;
    Ok(())
}
```

### Pattern 7: Startup Orphan Pruning (AGTW-05)
**What:** On Glass startup, query all rows in `pending_worktrees` and prune any directories/git-worktree-refs that still exist.
**When to use:** Once, during `Processor` initialization before the event loop starts.

```rust
pub fn prune_orphans(&self) -> Result<()> {
    let pending = self.db.list_pending_worktrees()?;
    for row in pending {
        tracing::warn!("WorktreeManager: pruning orphan worktree {}", row.id);
        if row.worktree_path.exists() {
            // Try git prune first; fall through to rm on error
            if let Ok(repo) = git2::Repository::discover(&row.project_root) {
                if let Ok(wt) = repo.find_worktree(&row.id) {
                    let mut opts = git2::WorktreePruneOptions::new();
                    opts.valid(true);
                    let _ = wt.prune(Some(&mut opts));
                }
            }
            let _ = std::fs::remove_dir_all(&row.worktree_path);
        }
        self.db.delete_pending_worktree(&row.id)?;
    }
    Ok(())
}
```

### Pattern 8: Non-Git Fallback (AGTW-06)
**What:** When the project is not a git repository, create a plain directory under `~/.glass/worktrees/<uuid>/` and copy the relevant files there. The apply and dismiss operations are identical to the git case except no git prune step.
**When to use:** `git2::Repository::discover(project_root).is_err()`

```rust
pub fn create_worktree(&self, project_root: &Path, proposal: &AgentProposal) -> Result<WorktreeHandle> {
    let id = uuid::Uuid::new_v4().to_string();
    let worktree_path = self.base_dir.join(&id);
    std::fs::create_dir_all(&worktree_path)?;

    let kind = match git2::Repository::discover(project_root) {
        Ok(_repo) => WorktreeKind::Git { repo_path: project_root.to_path_buf() },
        Err(_) => {
            tracing::info!("WorktreeManager: non-git project, using plain directory fallback");
            WorktreeKind::TempDir
        }
    };
    // ... register in DB, copy files, return handle
}
```

### Pattern 9: pending_worktrees SQLite Schema
**What:** A new table in `~/.glass/agents.db` (via a new migration).

```sql
CREATE TABLE IF NOT EXISTS pending_worktrees (
    id             TEXT PRIMARY KEY,      -- UUID, also used as git worktree name
    worktree_path  TEXT NOT NULL,         -- absolute path to ~/.glass/worktrees/<id>/
    project_root   TEXT NOT NULL,         -- absolute path to the project being edited
    proposal_id    TEXT NOT NULL,         -- links back to AgentProposalData
    created_at     INTEGER NOT NULL       -- unixepoch()
);
```

Migration: bump `PRAGMA user_version` from current (existing in `CoordinationDb::migrate`) to the next version. Follow the same `if version < N { ... conn.pragma_update(None, "user_version", N)?; }` pattern used in `glass_history/src/db.rs` and `glass_coordination/src/db.rs`.

### Anti-Patterns to Avoid
- **Creating the worktree directory before registering in SQLite:** This inverts the crash-safety invariant. A crash after mkdir but before INSERT leaves an untracked directory.
- **Using `git2::Repository::init` in the worktree:** The worktree is already a linked working tree; calling init would create a new independent repo.
- **Running `std::process::Command("git worktree prune")` instead of `git2::Worktree::prune`:** Fragile on Windows (git may not be on PATH), output parsing required; use the git2 API.
- **Calling `remove_dir_all` without first calling `git2::Worktree::prune`:** Leaves stale `.git/worktrees/<name>/` metadata; subsequent `git worktree list` will show locked/invalid entries.
- **Storing diff text in SQLite:** Diffs can be large and are ephemeral; compute on demand from worktree vs. working tree.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Unified diff | Custom line-by-line differ | `diffy::create_patch` | Edge cases: CRLF, no trailing newline, binary files, context lines — all handled by diffy |
| Worktree management | Shell-out to `git worktree add/rm` | `git2` crate | No PATH dependency, no output parsing, cross-platform, tested against libgit2 |
| UUID generation | Timestamp + random | `uuid::Uuid::new_v4()` | Already in codebase (`glass_coordination`); collision-free |
| Crash recovery tracking | In-memory Vec | `pending_worktrees` SQLite table | Survives crashes by definition; in-memory doesn't |
| Non-git fallback isolation | Modifying files in-place | Plain directory copy | Must not touch working tree until approval, even for non-git projects |

**Key insight:** The crash-recovery problem is the crux of this phase. The correct solution (register-before-create in SQLite) is simple to implement but easy to get wrong by reversing the order. The git2 API for worktrees is also the second subtlety — `Repository::worktree()` vs `Worktree::prune()` vs `WorktreePruneOptions` — there are no "convenience" wrappers; you work with the raw libgit2 bindings.

## Common Pitfalls

### Pitfall 1: git2 worktree name vs. directory name
**What goes wrong:** `repo.worktree(name, path, opts)` — the `name` must be unique across all worktrees for this repo and cannot contain slashes. Using a full UUID as the name works. Using the path as the name fails.
**Why it happens:** libgit2 stores the worktree metadata under `.git/worktrees/<name>/` — slashes would be interpreted as directory separators.
**How to avoid:** Use the UUID string directly as the name (e.g., `"wt-550e8400-e29b-41d4-a716"`). Verify `!name.contains('/')`.
**Warning signs:** `git2::Error` with "invalid worktree name" or "directory already exists in .git/worktrees/".

### Pitfall 2: git2 WorktreePruneOptions `valid` flag
**What goes wrong:** Calling `wt.prune(None)` on a worktree whose path still exists on disk does nothing — git2/libgit2 considers the worktree "valid" (path exists) and refuses to prune it.
**Why it happens:** Default prune only removes worktrees whose path is gone (stale). For cleanup, we need to prune even when the path exists.
**How to avoid:** Always set `WorktreePruneOptions::valid(true)` to force-prune regardless of path existence. Then remove the directory with `remove_dir_all`.
**Warning signs:** `wt.prune(None)` returns `Ok(())` but the `.git/worktrees/<name>/` entry remains.

### Pitfall 3: Windows path handling with git2
**What goes wrong:** git2 0.20 on Windows may fail with paths containing non-ASCII characters or spaces when calling `Repository::worktree()`. The STATE.md explicitly flags this: "git2 0.20 Windows path handling with spaces/non-ASCII not explicitly tested (Phase 57 risk)".
**Why it happens:** libgit2's Windows path normalization may not handle all edge cases for linked worktrees (distinct from regular repo open).
**How to avoid:** Use `~/.glass/worktrees/` (within the user's home dir, typically ASCII) for the worktree path. Do NOT place worktrees inside the project directory. Test on Windows CI with a path containing a space.
**Warning signs:** `git2::Error` with "path not found" or "failed to resolve path" on Windows only.

### Pitfall 4: `diffy` on binary files
**What goes wrong:** `diffy::create_patch` reads the file as a UTF-8 string. Binary files (images, compiled artifacts) will cause `std::fs::read_to_string` to fail with an invalid UTF-8 error.
**Why it happens:** diffy operates on `&str`, not `&[u8]`.
**How to avoid:** Before calling `read_to_string`, check if the file is likely binary (e.g., via extension allowlist or by attempting `read_to_string` and matching `Err(InvalidUtf8)`). For binary files, emit a placeholder diff line: `"Binary file changed (review in worktree)"`.
**Warning signs:** `std::fs::read_to_string` returns `Err(...)` on `apply` paths or diff generation panics.

### Pitfall 5: Registering in the wrong DB
**What goes wrong:** Adding `pending_worktrees` to `~/.glass/history.db` or creating a new separate DB, rather than `~/.glass/agents.db`.
**Why it happens:** Multiple DBs exist in `~/.glass/`; easy to pick the wrong one.
**How to avoid:** Add `pending_worktrees` table to `CoordinationDb::migrate()` in `glass_coordination/src/db.rs` as a new version bump (e.g., version 2). Alternatively, create the table in a new `WorktreeDb` that opens `agents.db` directly. Either way, same physical file.
**Warning signs:** Orphan recovery doesn't fire because the DB being opened on startup is not the same file used during creation.

### Pitfall 6: Deleting the pending_worktrees row too early
**What goes wrong:** Deleting the `pending_worktrees` row immediately after `git worktree add` succeeds (before apply/dismiss completes). If the apply crashes mid-copy, the worktree is left untracked and leaks.
**Why it happens:** Treating "worktree created" as the terminal event rather than "worktree finalized (applied or dismissed)".
**How to avoid:** The row is deleted ONLY inside `cleanup()` after either `apply` or `dismiss` completes. The row persists for the entire lifetime of the pending proposal.
**Warning signs:** After apply crash, Glass restarts but doesn't prune the leftover worktree directory.

## Code Examples

Verified patterns from official sources:

### git2 worktree creation
```rust
// Source: git2 0.20 crate docs — Repository::worktree
use git2::{Repository, WorktreeAddOptions};
use std::path::Path;

fn add_worktree(repo: &Repository, name: &str, path: &Path) -> Result<(), git2::Error> {
    // opts: None uses defaults (detached HEAD at current HEAD)
    repo.worktree(name, path, None)?;
    Ok(())
}
```

### git2 worktree prune
```rust
// Source: git2 0.20 crate docs — Worktree::prune, WorktreePruneOptions
use git2::{Repository, WorktreePruneOptions};

fn prune_worktree(repo: &Repository, name: &str) -> Result<(), git2::Error> {
    let wt = repo.find_worktree(name)?;
    let mut opts = WorktreePruneOptions::new();
    opts.valid(true);   // force-prune even when path still exists on disk
    wt.prune(Some(&mut opts))?;
    Ok(())
}
```

### diffy unified diff
```rust
// Source: diffy 0.4 crate docs — diffy::create_patch
use diffy::create_patch;

fn diff_file(original: &str, modified: &str) -> String {
    let patch = create_patch(original, modified);
    patch.to_string()
}
```

### pending_worktrees SQLite migration
```rust
// Source: glass_history/src/db.rs migrate() pattern -- PRAGMA user_version versioning
fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    // ... existing version guards ...

    if version < N {  // N = next version after current CoordinationDb version
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS pending_worktrees (
                id             TEXT PRIMARY KEY,
                worktree_path  TEXT NOT NULL,
                project_root   TEXT NOT NULL,
                proposal_id    TEXT NOT NULL,
                created_at     INTEGER NOT NULL DEFAULT (unixepoch())
            );"
        )?;
        conn.pragma_update(None, "user_version", N)?;
    }
    Ok(())
}
```

### WorktreeHandle and types
```rust
// crates/glass_agent/src/types.rs
use std::path::PathBuf;

/// Whether the worktree is a git linked worktree or a plain directory copy.
#[derive(Debug, Clone)]
pub enum WorktreeKind {
    /// Project is a git repo; linked worktree was created.
    Git { repo_path: PathBuf },
    /// Project is not a git repo; plain directory copy was created.
    TempDir,
}

/// A live handle to a pending agent worktree.
#[derive(Debug)]
pub struct WorktreeHandle {
    /// UUID — used as both the worktree name and directory name.
    pub id: String,
    /// Absolute path to the worktree directory.
    pub worktree_path: PathBuf,
    /// Absolute path to the project root.
    pub project_root: PathBuf,
    /// Whether this is a git worktree or plain copy.
    pub kind: WorktreeKind,
    /// Project-relative paths of files the agent changed.
    pub changed_files: Vec<PathBuf>,
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Apply agent changes directly to working tree | Isolate in git worktree until user approves | Phase 57 (new) | Working tree never modified without consent |
| Shell-out `git worktree add` | `git2::Repository::worktree()` | N/A (first implementation) | No PATH dependency, cross-platform, no output parsing |
| No crash recovery | Register-before-create in SQLite | Phase 57 (from opencode PR #14649 pattern) | Orphaned worktrees cleaned on next startup |
| No non-git fallback | Plain directory copy under `~/.glass/worktrees/` | Phase 57 (new) | Phase works on any project, not just git repos |

**Deprecated/outdated:**
- Storing proposals only in memory (`agent_pending_proposals: Vec<AgentProposalData>`): Phase 57 attaches a `WorktreeHandle` to each pending proposal, changing the proposal lifecycle. The in-memory Vec stays but each entry now carries a worktree path.

## Open Questions

1. **Where does `AgentProposalData` carry file change information?**
   - What we know: Current `AgentProposalData` (in `glass_core/src/agent_runtime.rs`) has `action: String`, `description: String`, `severity: String`, `command_id: i64`, `raw_response: String`. There is no `file_changes: Vec<(PathBuf, String)>` field.
   - What's unclear: Phase 57 requires the proposal to carry a list of `(relative_path, new_content)` pairs so WorktreeManager can write them into the worktree. Phase 56 doesn't add this because it predates worktrees. Phase 57 must extend `AgentProposalData` or add a new type.
   - Recommendation: Add `file_changes: Vec<(String, String)>` (relative path, new content) to `AgentProposalData` in Phase 57 Plan 01. The agent system prompt already instructs the agent to emit `GLASS_PROPOSAL:` JSON; extend that JSON schema to include a `files` array: `[{"path": "src/main.rs", "content": "..."}]`. The `extract_proposal` function in `agent_runtime.rs` must be updated.

2. **CoordinationDb migration version**
   - What we know: `CoordinationDb::migrate` in `glass_coordination/src/db.rs` exists but only has a schema creation block (no explicit version 1 yet based on the code read). The current schema has `agents`, `file_locks`, and `messages` tables.
   - What's unclear: What is the current `PRAGMA user_version` value in `agents.db`? Is there a version 1 migration guard?
   - Recommendation: Read `glass_coordination/src/db.rs` fully in Wave 0 of planning to confirm current version number, then add `pending_worktrees` as the next version increment.

3. **Worktree HEAD branch policy**
   - What we know: `git2::Repository::worktree(name, path, None)` creates a worktree with the HEAD of the main worktree, detached.
   - What's unclear: Should the worktree be on a named branch (e.g., `glass-agent-proposal/<uuid>`) for user visibility in `git branch`, or detached HEAD?
   - Recommendation: Detached HEAD is sufficient for Phase 57. Named branches are in scope for future phase AGTE-02 (PR/branch creation). Detached HEAD is simpler and doesn't pollute the branch list.

4. **`diffy` version availability**
   - What we know: `diffy` is a commonly used Rust diff crate. Claimed version `0.4` from training knowledge.
   - What's unclear: Whether `diffy 0.4` is the current published version or whether `0.3` is the latest stable.
   - Recommendation: Verify with `cargo search diffy` in Wave 0. If `0.3` is the latest, use `0.3`. The API (`create_patch` returning a `Patch` with `to_string()`) has been stable across minor versions.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test (`cargo test`) |
| Config file | None (inline `#[cfg(test)]`) |
| Quick run command | `cargo test -p glass_agent` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| AGTW-01 | WorktreeManager::create_worktree creates dir and git worktree | unit | `cargo test -p glass_agent -- worktree_manager::tests::test_create_worktree_git` | Wave 0 |
| AGTW-01 | WorktreeManager writes agent file changes into worktree | unit | `cargo test -p glass_agent -- worktree_manager::tests::test_write_files_to_worktree` | Wave 0 |
| AGTW-02 | generate_diff returns unified diff string for changed file | unit | `cargo test -p glass_agent -- worktree_manager::tests::test_generate_diff` | Wave 0 |
| AGTW-02 | generate_diff emits placeholder for binary file | unit | `cargo test -p glass_agent -- worktree_manager::tests::test_diff_binary_file` | Wave 0 |
| AGTW-03 | apply copies worktree files to working tree | unit | `cargo test -p glass_agent -- worktree_manager::tests::test_apply_copies_files` | Wave 0 |
| AGTW-04 | cleanup removes worktree dir and git worktree ref | unit | `cargo test -p glass_agent -- worktree_manager::tests::test_cleanup_removes_dir` | Wave 0 |
| AGTW-04 | dismiss removes worktree without touching working tree | unit | `cargo test -p glass_agent -- worktree_manager::tests::test_dismiss_no_working_tree_change` | Wave 0 |
| AGTW-05 | prune_orphans removes leftover worktree from crashed session | unit | `cargo test -p glass_agent -- worktree_manager::tests::test_prune_orphans` | Wave 0 |
| AGTW-05 | pending_worktrees row persists across restart (simulated) | unit | `cargo test -p glass_agent -- worktree_db::tests::test_pending_row_survives_restart` | Wave 0 |
| AGTW-06 | non-git project uses TempDir fallback | unit | `cargo test -p glass_agent -- worktree_manager::tests::test_non_git_fallback` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_agent`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_agent/` — new crate directory with `Cargo.toml` and `src/lib.rs`
- [ ] `crates/glass_agent/src/types.rs` — `WorktreeHandle`, `WorktreeKind`, `PendingWorktree`
- [ ] `crates/glass_agent/src/worktree_manager.rs` — `WorktreeManager` struct
- [ ] `crates/glass_agent/src/worktree_db.rs` — `pending_worktrees` table helpers
- [ ] Workspace `Cargo.toml` — add `git2 = "0.20"` and `diffy = "0.4"` (or `0.3`) to `[workspace.dependencies]`
- [ ] `glass_coordination/src/db.rs` — read full file to confirm current migration version before adding `pending_worktrees` migration
- [ ] Verify `diffy` published version: `cargo search diffy` in Wave 0
- [ ] `glass_core/src/agent_runtime.rs` — extend `AgentProposalData` with `file_changes: Vec<(String, String)>`; update `extract_proposal` to parse `files` array from `GLASS_PROPOSAL:` JSON

## Sources

### Primary (HIGH confidence)
- `crates/glass_core/src/agent_runtime.rs` — Phase 56 completed implementation. Confirmed `AgentProposalData`, `extract_proposal`, `AgentRuntimeConfig`, `build_agent_command_args`. Read directly.
- `crates/glass_core/src/event.rs` — Confirmed `AppEvent::AgentProposal`, `AppEvent::AgentCrashed`, `AppEvent::AgentQueryResult`. Read directly.
- `src/main.rs` (Processor) — Confirmed `agent_pending_proposals: Vec<AgentProposalData>`, `agent_runtime: Option<AgentRuntime>`, worktree cleanup is `TODO Phase 58` comment at line 3568. Read directly.
- `Cargo.toml` (workspace) — Confirmed `git2` and `uuid` are NOT in `[workspace.dependencies]`; `uuid` exists in `glass_coordination/Cargo.toml` only; `rusqlite`, `dirs`, `anyhow`, `tracing` all available at workspace level. Read directly.
- `crates/glass_history/src/db.rs` — Confirmed `PRAGMA user_version` migration pattern. Read directly.
- `crates/glass_coordination/src/db.rs` — Confirmed `CoordinationDb::migrate` structure and WAL mode pattern. Read directly (first 80 lines; need full read in Wave 0 to confirm current version number).
- `.planning/STATE.md` — Confirmed key decisions: "git worktree registered in SQLite BEFORE creation -- crash recovery pattern from opencode PR #14649"; "New crates needed: glass_soi, glass_agent"; "new deps: uuid 1.22, git2 0.20". Read directly.
- `crates/glass_snapshot/src/undo.rs` — Confirmed `std::fs::write` / `std::fs::remove_file` patterns for file restoration. The apply/dismiss pattern mirrors this. Read directly.

### Secondary (MEDIUM confidence)
- git2 crate docs (training knowledge, version 0.20): `Repository::worktree(name, path, opts)`, `Repository::find_worktree(name)`, `Worktree::prune(opts)`, `WorktreePruneOptions::valid(bool)`. Requires verification against published docs in Wave 0.
- diffy crate docs (training knowledge): `create_patch(original, modified) -> Patch`, `Patch::to_string()`. Version needs verification.

### Tertiary (LOW confidence)
- opencode PR #14649 (referenced in STATE.md) — crash-recovery pattern origin. Not directly verified; trusted based on team decision already encoded in STATE.md.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — workspace deps confirmed, git2/diffy identified from STATE.md decisions; versions need Wave 0 cargo search verification
- Architecture: HIGH — follows established crate/module patterns exactly; new `glass_agent` crate already planned in STATE.md
- Pitfalls: HIGH — git2 WorktreePruneOptions `valid` flag and Windows path risk are verified from git2 API knowledge and STATE.md explicit risk note
- `AgentProposalData` extension: MEDIUM — the need is clear but the exact JSON schema extension has no external precedent; chosen approach (add `files` array to GLASS_PROPOSAL JSON) is the natural fit

**Research date:** 2026-03-13
**Valid until:** 2026-04-13 (git2 0.20 API stable; diffy API stable; version numbers need re-check if planning is delayed)

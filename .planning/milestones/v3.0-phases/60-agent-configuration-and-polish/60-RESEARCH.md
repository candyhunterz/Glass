# Phase 60: Agent Configuration and Polish - Research

**Researched:** 2026-03-13
**Domain:** Rust config extension, permission matrix, quiet rules, graceful degradation, glass_coordination integration
**Confidence:** HIGH

## Summary

Phase 60 is a pure polish-and-wiring phase with no new architectural concepts. Every component it touches already exists and is tested. The phase adds three new semantic layers on top of the Phase 56-59 agent foundation: a permission matrix that gates proposal types, quiet rules that suppress activity events for matching command patterns, and a config-reload path that re-evaluates the agent runtime when the `[agent]` section changes.

The most complex requirement is AGTC-05 (glass_coordination lock management). The `CoordinationDb` and `AgentInfo`/`LockResult` types are fully implemented in `glass_coordination`. The agent already receives a `project_root` string at spawn time (Phase 59). What's missing is a call to `CoordinationDb::acquire_locks` on session start and a matching `release_locks` on shutdown. This must be done inside `try_spawn_agent` (for acquire) and in `AgentRuntime::drop` (for release), or via a dedicated `AppEvent` round-trip.

AGTC-04 (graceful degradation) is already 90% done: `try_spawn_agent` returns `None` when the `claude` binary is absent. The gap is surfacing a user-visible config hint instead of only a `tracing::warn!` log line, and ensuring `agent.enabled = true` (mode != Off) with a missing binary logs a clear actionable message.

**Primary recommendation:** Extend `AgentSection` in `glass_core/config.rs` with `permissions` and `quiet_rules` sub-tables, add enforcement at the activity-stream gate and proposal-emission point, wire coordination lock acquire/release around `try_spawn_agent`, and promote the existing binary-not-found log to a user-visible toast or status bar hint.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| AGTC-01 | Full [agent] config section in config.toml with hot-reload support | `AgentSection` exists with mode/budget/cooldown/allowed_tools. Hot-reload path (`AppEvent::ConfigReloaded`) already swaps `self.config` but does NOT restart the agent runtime on mode change. Need to add agent-restart logic in the ConfigReloaded arm. |
| AGTC-02 | Permission matrix: approve/auto/never per action type (edit_files, run_commands, git_operations) | New `PermissionMatrix` sub-struct in `AgentSection`. Enforcement point: `extract_proposal` already classifies proposals; need a filter step between proposal receipt and `agent_proposal_worktrees.push()`. |
| AGTC-03 | Quiet rules: ignore specific commands, ignore successful commands | New `QuietRules` sub-struct in `AgentSection`. Enforcement point: the `SoiReady` arm in main.rs, before `activity_filter.process()` and before `activity_stream_tx.try_send()`. |
| AGTC-04 | Graceful degradation when Claude CLI is unavailable | `try_spawn_agent` already returns `None` silently. Need to emit `AppEvent::AgentDisabledMissingBinary` (or reuse existing toast system) so user sees actionable message. |
| AGTC-05 | Agent integrates with glass_coordination for advisory lock management on session start/stop | `CoordinationDb::acquire_locks` / `release_locks` exist. Need: agent registration (register_agent), lock acquire on session start, release on session end / drop. Project root already threaded through. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `glass_core::config` | in-tree | `AgentSection` extension for permissions + quiet_rules | All config lives here; TOML serde derive pattern established |
| `glass_coordination::CoordinationDb` | in-tree | Advisory lock acquire/release for AGTC-05 | Fully implemented; `acquire_locks`, `release_locks`, `register_agent`, `deregister_agent` all exist |
| `glass_core::event::AppEvent` | in-tree | New variants for binary-not-found notification | All user-visible events route through this enum |
| `notify` crate | already dep | Config hot-reload watcher | Already wired via `spawn_config_watcher` |
| `serde + toml` | 1.0 / 1.0.4 | TOML deserialization of new sub-tables | Already in workspace |

### No New Dependencies Required
All libraries needed for Phase 60 are already in the workspace. No `Cargo.toml` changes should be needed.

## Architecture Patterns

### Recommended Project Structure

Changes are spread across three existing files plus one new struct location:

```
crates/glass_core/src/config.rs       -- extend AgentSection with PermissionMatrix + QuietRules
crates/glass_core/src/event.rs        -- add AgentDisabledMissingBinary variant (optional)
src/main.rs
  try_spawn_agent()                   -- add coordination register+lock on start
  AgentRuntime::drop()                -- add coordination deregister+unlock on stop
  AppEvent::ConfigReloaded arm        -- add agent restart when mode or config changes
  AppEvent::SoiReady arm              -- add quiet_rules filter before activity_filter.process()
  AppEvent::AgentProposal arm         -- add permission matrix check before push
```

### Pattern 1: Extending AgentSection with Sub-Tables

**What:** TOML supports inline tables and regular sub-sections. The existing `AgentSection` uses flat fields. Add `PermissionMatrix` and `QuietRules` as optional nested structs with `#[serde(default)]`.

**When to use:** Whenever config data has logical grouping that benefits from namespace isolation in the TOML file.

**Example:**
```toml
[agent]
mode = "Assist"
max_budget_usd = 2.0

[agent.permissions]
edit_files = "approve"      # approve | auto | never
run_commands = "approve"
git_operations = "never"

[agent.quiet_rules]
ignore_exit_zero = false
ignore_patterns = ["cargo check", "git status"]
```

**TOML deserialization pattern (from existing codebase):**
```rust
// Source: crates/glass_core/src/config.rs -- SoiSection / PipesSection pattern
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PermissionMatrix {
    #[serde(default = "default_permission_approve")]
    pub edit_files: PermissionLevel,
    #[serde(default = "default_permission_approve")]
    pub run_commands: PermissionLevel,
    #[serde(default = "default_permission_approve")]
    pub git_operations: PermissionLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionLevel {
    Approve,  // show to user for approval (current default behavior)
    Auto,     // auto-apply without approval
    Never,    // never produce proposals of this type
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct QuietRules {
    #[serde(default)]
    pub ignore_exit_zero: bool,
    #[serde(default)]
    pub ignore_patterns: Vec<String>,
}
```

Add to `AgentSection`:
```rust
#[serde(default)]
pub permissions: Option<PermissionMatrix>,
#[serde(default)]
pub quiet_rules: Option<QuietRules>,
```

### Pattern 2: Config Hot-Reload Agent Restart

**What:** The `AppEvent::ConfigReloaded` arm in main.rs currently updates `self.config` and restarts the font pipeline. It does NOT restart the agent runtime when the `[agent]` section changes.

**Current state:** `self.config = new_config` is the only agent-related action in ConfigReloaded. The agent runtime keeps running with old config until process restart.

**What to add:** After swapping config, compare old vs new agent section. If mode changed or any field changed, drop the old runtime and call `try_spawn_agent` with new config.

```rust
// In AppEvent::ConfigReloaded arm, after self.config = new_config:
let old_agent_mode = old_config.agent.as_ref().map(|a| a.mode);
let new_agent_mode = new_config.agent.as_ref().map(|a| a.mode);
let agent_config_changed = old_config.agent != new_config.agent;

if agent_config_changed {
    // Drop old runtime (triggers AgentRuntime::drop -> kill child)
    self.agent_runtime = None;
    // Re-create channel
    let (tx, rx) = glass_core::activity_stream::create_channel(&...);
    self.activity_stream_tx = Some(tx);
    // Re-spawn if new mode is not Off
    let new_agent_cfg = ...; // map from new_config.agent
    if new_agent_cfg.mode != AgentMode::Off {
        self.agent_runtime = try_spawn_agent(new_agent_cfg, rx, ...);
    } else {
        self.activity_stream_rx = Some(rx);
    }
}
```

**Pitfall:** The `activity_stream_rx` is taken (`.take()`) by the writer thread in `try_spawn_agent`. On restart, a new channel must be created. The old `tx` sender is replaced so the old channel is dropped naturally.

### Pattern 3: Quiet Rules Filtering

**What:** Before forwarding `SoiReady` events to the activity stream, check if the command matches any quiet rule.

**Enforcement point:** The `AppEvent::SoiReady` arm in main.rs, which calls `activity_filter.process()` and then `activity_stream_tx.try_send()`.

**Data available at that point:** `summary` (SOI one-line), `severity` ("Success"/"Error"/etc), and the command text is NOT directly in SoiReady -- it's accessible via the session's `block_manager`.

**Implementation approach for `ignore_patterns`:** The simplest approach that matches the requirement is to match `summary` against patterns using `contains()` or glob matching, since the SOI summary includes the command context. Alternatively, the `command_id` can be used to look up the command text in the history DB -- but that adds a synchronous DB read on the main thread (avoid). Match against the `summary` field using `ignore_patterns` as substring checks.

**Implementation approach for `ignore_exit_zero`:** SOI severity "Success" maps to exit code 0. Filtering `ignore_exit_zero = true` means dropping all "Success" severity events from reaching the agent. This is already partially handled by `AgentMode::Watch` (only Error passes), but quiet rules are more granular -- they suppress at the activity stream level regardless of mode.

```rust
// In SoiReady arm, before activity_filter.process():
if let Some(agent_cfg) = self.config.agent.as_ref() {
    if let Some(quiet) = agent_cfg.quiet_rules.as_ref() {
        if quiet.ignore_exit_zero && severity == "Success" {
            return; // swallow event
        }
        for pattern in &quiet.ignore_patterns {
            if summary.contains(pattern.as_str()) {
                return; // swallow event
            }
        }
    }
}
```

### Pattern 4: Permission Matrix Enforcement

**What:** When an `AppEvent::AgentProposal` arrives, classify the proposal's action type and check the permission matrix before adding to `agent_proposal_worktrees`.

**Proposal classification:** The `AgentProposalData` has `action` (shell command string) and `file_changes` (non-empty = edit_files proposal). A heuristic classification:
- Non-empty `file_changes` -> `edit_files` permission
- `action` starts with `git ` -> `git_operations` permission
- otherwise -> `run_commands` permission

**Permission levels:**
- `Never`: Drop the proposal entirely (no toast, no overlay entry)
- `Approve`: Current behavior (toast + overlay for user review) -- default
- `Auto`: Apply immediately without user approval (skip the overlay)

```rust
// In AppEvent::AgentProposal arm:
fn classify_proposal(proposal: &AgentProposalData) -> PermissionKind {
    if !proposal.file_changes.is_empty() {
        PermissionKind::EditFiles
    } else if proposal.action.starts_with("git ") {
        PermissionKind::GitOperations
    } else {
        PermissionKind::RunCommands
    }
}

let permission = if let Some(cfg) = self.config.agent.as_ref() {
    cfg.permissions.as_ref().map(|p| match classify_proposal(&proposal) {
        PermissionKind::EditFiles => p.edit_files,
        PermissionKind::RunCommands => p.run_commands,
        PermissionKind::GitOperations => p.git_operations,
    }).unwrap_or(PermissionLevel::Approve)
} else {
    PermissionLevel::Approve
};

match permission {
    PermissionLevel::Never => { /* drop proposal */ }
    PermissionLevel::Approve => { /* existing behavior: push to worktrees, show toast */ }
    PermissionLevel::Auto => { /* apply immediately, skip overlay */ }
}
```

### Pattern 5: glass_coordination Lock Integration (AGTC-05)

**What:** On agent session start, call `CoordinationDb::register_agent` + `CoordinationDb::acquire_locks`. On session end, call `release_locks` + `deregister_agent`.

**Existing API in `glass_coordination::CoordinationDb`:**
```rust
// Source: crates/glass_coordination/src/db.rs (confirmed in codebase)
pub fn register_agent(&mut self, info: &AgentInfo) -> Result<String> { ... }
pub fn deregister_agent(&mut self, agent_id: &str) -> Result<()> { ... }
pub fn acquire_locks(&mut self, agent_id: &str, paths: &[&str], reason: Option<&str>) -> Result<LockResult> { ... }
pub fn release_locks(&mut self, agent_id: &str, paths: &[&str]) -> Result<()> { ... }
```

**What files to lock:** The agent doesn't know in advance which files it will modify. The standard pattern (from CLAUDE.md protocol) is to lock files "before editing". For Glass Agent, the intent is to register a session-level lock on the project root or on any file in `file_changes` when a proposal arrives. The simplest correct implementation for AGTC-05: acquire a session-level advisory lock on the project root directory when the agent starts (indicating "this project is under active agent session"), and release on session end.

**Where to call:** In `try_spawn_agent` after successful spawn, before returning `Some(AgentRuntime)`. The `project_root` is already passed in. The `CoordinationDb` can be opened inline.

**Agent ID storage:** The `AgentRuntime` struct needs an `agent_id: Option<String>` field to store the registered ID for deregistration in `Drop`.

```rust
// In try_spawn_agent, after child.spawn() succeeds:
let agent_id = {
    match glass_coordination::CoordinationDb::open_default() {
        Ok(mut db) => {
            let info = glass_coordination::AgentInfo {
                id: uuid::Uuid::new_v4().to_string(),
                name: "glass-agent".to_string(),
                agent_type: "claude-code".to_string(),
                project: project_root.clone(),
                cwd: project_root.clone(),
                pid: None, // child.id() is Option<u32>
                status: "active".to_string(),
                task: Some("monitoring terminal activity".to_string()),
                registered_at: 0, // set by DB
                last_heartbeat: 0,
            };
            match db.register_agent(&info) {
                Ok(id) => {
                    // Advisory lock on project root
                    let _ = db.acquire_locks(&id, &[&project_root], Some("agent session"));
                    Some(id)
                }
                Err(e) => {
                    tracing::warn!("AgentRuntime: failed to register with coordination: {}", e);
                    None
                }
            }
        }
        Err(e) => {
            tracing::warn!("AgentRuntime: failed to open coordination db: {}", e);
            None
        }
    }
};
```

```rust
// In AgentRuntime::drop():
if let Some(ref agent_id) = self.agent_id {
    if let Ok(mut db) = glass_coordination::CoordinationDb::open_default() {
        let _ = db.release_locks(agent_id, &[]);  // release all
        let _ = db.deregister_agent(agent_id);
    }
}
```

**Pitfall:** `CoordinationDb::open_default()` can fail if `~/.glass/` doesn't exist or agents.db is locked. All coordination failures must be soft errors -- they must NEVER prevent the agent from starting or stopping.

### Pattern 6: AGTC-04 Graceful Degradation with User Hint

**What:** Currently `try_spawn_agent` returns `None` with a `tracing::warn!` when `claude` is not found. The requirement says "shows a clear config hint" -- the user needs to see something, not just a log line.

**Implementation:** Add an `AppEvent` variant (or reuse the existing `config_error` display path) to surface the hint. The simplest approach: use the existing `config_error: Option<ConfigError>` field on Processor. The ConfigError struct has a `message` field. After `try_spawn_agent` returns `None` when mode != Off, set a synthetic `config_error` with the hint message. This reuses the existing error overlay rendering with zero new rendering code.

```rust
// In resumed() after try_spawn_agent returns None:
if agent_config.mode != AgentMode::Off && self.agent_runtime.is_none() {
    self.config_error = Some(glass_core::config::ConfigError {
        message: "'claude' CLI not found on PATH. Install from https://claude.ai/download, or set agent.mode = \"off\" in ~/.glass/config.toml".to_string(),
        line: None,
        column: None,
        snippet: None,
    });
}
```

### Anti-Patterns to Avoid

- **Blocking DB call in SoiReady arm:** Opening `CoordinationDb` for every SOI event is too expensive. Coordination calls belong only at agent session start/stop.
- **Forgetting to create a new channel on agent restart:** `activity_stream_rx` is taken by the writer thread. ConfigReloaded must create a fresh channel, not reuse the consumed one.
- **Acquiring locks on specific files at start:** The agent doesn't know future files. Lock the project root as a "session marker", not specific files. File-level locks are for the MCP `glass_agent_lock` tool (already separate).
- **Making PermissionLevel::Auto apply files unsafely:** Auto-apply should still use the existing `apply_worktree_changes` path (which copies files from worktree), just skip the overlay step.
- **Using `#[serde(flatten)]` for sub-tables:** TOML uses `[agent.permissions]` syntax which maps to a nested struct field, not flattened. Use a named field `permissions: Option<PermissionMatrix>`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Config parsing with line numbers | Custom TOML parser | `toml::from_str` + `ConfigError` (already exists) | `load_validated()` is already implemented with span-based line/col extraction |
| File watching | `inotify`/`ReadDirectoryChanges` direct | `notify` crate + `spawn_config_watcher` | Already wired; just handle the new agent fields in the existing ConfigReloaded arm |
| Agent process tracking | PID file or signal handlers | Existing `AgentRuntime::drop` + `child.kill()` | Pattern already established in Phase 56 |
| Coordination locking | Custom file lock | `glass_coordination::CoordinationDb::acquire_locks` | Already implemented with SQLite WAL + IMMEDIATE transactions |
| Pattern matching for quiet rules | Regex engine | String `contains()` for simple substring match | Requirements say "command pattern" not "regex"; substring is sufficient and avoids a `regex` dependency in glass_core |

**Key insight:** Phase 60 is almost entirely wiring existing components together. No new Rust crates or major new modules are needed.

## Common Pitfalls

### Pitfall 1: Config Reload Does Not Restart Agent
**What goes wrong:** After editing `config.toml` to change `agent.mode` from `Off` to `Assist`, the agent doesn't start because `ConfigReloaded` only swaps `self.config` without checking if agent section changed.
**Why it happens:** The existing `ConfigReloaded` arm handles font reload but has no agent restart logic.
**How to avoid:** After `self.config = new_config`, compare old vs new agent section (`PartialEq` already derived on `AgentSection`) and conditionally restart.
**Warning signs:** Agent mode shows "Off" in status bar even after config edit.

### Pitfall 2: Double-Consumed activity_stream_rx on Restart
**What goes wrong:** On second agent start (after config reload), `try_spawn_agent` panics or silently fails because `activity_stream_rx` was already taken by the previous writer thread.
**Why it happens:** `activity_stream_rx` is a one-shot `mpsc::Receiver`. Once moved into the writer thread closure, it's gone.
**How to avoid:** Always create a fresh `(tx, rx)` pair before each call to `try_spawn_agent`. The old `tx` is simply dropped when `self.activity_stream_tx` is overwritten.
**Warning signs:** Second agent spawn produces no activity events; writer thread exits immediately.

### Pitfall 3: Coordination DB Failures Blocking Agent Start
**What goes wrong:** `CoordinationDb::open_default()` returns `Err` (e.g., permission issue, locked DB) and the agent fails to start entirely.
**Why it happens:** Treating coordination as a hard dependency.
**How to avoid:** Wrap all coordination calls in `if let Ok(mut db) = ...` and log warnings. The agent MUST start even if coordination is unavailable.
**Warning signs:** Agent never starts on CI or in Docker containers where `~/.glass/` may not be writable.

### Pitfall 4: Quiet Rules Matching on Wrong Field
**What goes wrong:** `ignore_patterns` is matched against `summary` (SOI output text) but user expects to match the original command text (e.g., "cargo check").
**Why it happens:** The `SoiReady` event carries `summary` (SOI one-liner), not the original command text. The command text IS in the history DB but requires a DB lookup.
**How to avoid:** Document clearly in config.toml comments that `ignore_patterns` matches against SOI summary text. Alternatively, store command text in `SoiReady` -- but that requires changing the event struct, which is fine if done carefully. Check `AppEvent::SoiReady` fields to see if command text is available.
**Warning signs:** Pattern "cargo check" doesn't suppress events even though user expects it to.

**Research note:** Check `AppEvent::SoiReady` definition in `glass_core/src/event.rs` -- if it only carries `summary` and not `command_text`, the planner must decide: add `command_text` to `SoiReady` (preferred for correctness) or document the limitation.

### Pitfall 5: PermissionLevel::Auto Bypassing Worktree Creation
**What goes wrong:** `Auto` mode applies file changes directly without creating a worktree, meaning there's no undo path.
**Why it happens:** Trying to skip the worktree for "auto-approve" proposals.
**How to avoid:** `Auto` still creates a worktree and calls `apply_worktree_changes` immediately -- it just skips the user-facing overlay step. This preserves the crash-recovery path and undo capability.

## Code Examples

### Verified: AgentSection Serde Pattern
```rust
// Source: crates/glass_core/src/config.rs (existing code, Phase 56)
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AgentSection {
    #[serde(default)]
    pub mode: crate::agent_runtime::AgentMode,
    #[serde(default = "default_agent_max_budget_usd")]
    pub max_budget_usd: f64,
    // ... add new fields here with #[serde(default)]
    #[serde(default)]
    pub permissions: Option<PermissionMatrix>,
    #[serde(default)]
    pub quiet_rules: Option<QuietRules>,
}
```

### Verified: CoordinationDb API
```rust
// Source: crates/glass_coordination/src/db.rs (confirmed in codebase)
// open_default() -> Result<CoordinationDb>
// register_agent(&mut self, info: &AgentInfo) -> Result<String>  (returns agent_id)
// deregister_agent(&mut self, agent_id: &str) -> Result<()>
// acquire_locks(&mut self, agent_id: &str, paths: &[&str], reason: Option<&str>) -> Result<LockResult>
// release_locks(&mut self, agent_id: &str, paths: &[&str]) -> Result<()>  (empty slice = all locks)
```

### Verified: ConfigReloaded Arm Location
```rust
// Source: src/main.rs line 3782
AppEvent::ConfigReloaded { config, error } => {
    // ... existing font reload logic ...
    self.config = new_config;  // line 3818 -- insert agent restart check AFTER this line
}
```

### Verified: SoiReady Arm Location
```rust
// Source: src/main.rs line 3871 (approximate)
// Insert quiet_rules check BEFORE activity_filter.process() call
AppEvent::SoiReady { ... summary, severity, ... } => {
    // NEW: quiet rules check here
    // EXISTING: activity_filter.process() + activity_stream_tx.try_send()
}
```

### Verified: AgentRuntime Struct Extension
```rust
// Source: src/main.rs line 218 (AgentRuntime struct)
struct AgentRuntime {
    child: Option<std::process::Child>,
    cooldown: glass_core::agent_runtime::CooldownTracker,
    budget: glass_core::agent_runtime::BudgetTracker,
    config: glass_core::agent_runtime::AgentRuntimeConfig,
    restart_count: u32,
    last_crash: Option<std::time::Instant>,
    // NEW for AGTC-05:
    agent_id: Option<String>,  // coordination DB agent_id, None if registration failed
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Agent config: mode only | Full section (mode, budget, cooldown, tools) | Phase 56 | Foundation for Phase 60 extension |
| No permission gating | Permission matrix (approve/auto/never per action type) | Phase 60 (new) | Users can prevent file edits from agent |
| All commands forwarded to agent | Quiet rules suppress matching patterns | Phase 60 (new) | Reduces noise for repetitive commands |
| Binary-not-found: silent warn | Clear config hint to user | Phase 60 (new) | Actionable degradation |
| No coordination on agent sessions | CoordinationDb lock on project root | Phase 60 (new) | Multi-agent safety when Claude CLI + cursor both active |

**Not yet in codebase (for planner's awareness):**
- `AppEvent::SoiReady` may or may not carry `command_text` -- verify in `glass_core/src/event.rs` before coding AGTC-03
- `CoordinationDb::release_locks` with empty slice `&[]` -- verify semantics: does it release ALL locks for that agent_id? Check db.rs implementation.

## Open Questions

1. **Does SoiReady carry command_text for quiet_rules matching?**
   - What we know: `SoiReady` carries `summary` and `severity`. The command text that triggered the SOI is in the history DB row identified by `command_id`.
   - What's unclear: Is command text in the `SoiReady` event payload, or must it be fetched?
   - Recommendation: Check `glass_core/src/event.rs` `AppEvent::SoiReady` definition. If command_text is absent, add it (as an `Option<String>` for backward compat) so quiet_rules can match on the original command string.

2. **release_locks empty-slice semantics**
   - What we know: `CoordinationDb::release_locks(agent_id, paths)` exists.
   - What's unclear: Passing `&[]` to release ALL locks vs. needing to pass explicit paths.
   - Recommendation: Read db.rs `release_locks` implementation before coding Drop. If `&[]` doesn't release all, store the locked paths in `AgentRuntime.locked_paths: Vec<String>`.

3. **Config hot-reload timing: should agent restart be immediate or deferred?**
   - What we know: Config reload fires from `notify` watcher thread via proxy, processed on main thread synchronously.
   - What's unclear: Is there a race where the old agent is killed but a new one spawns before old stdout is fully drained?
   - Recommendation: Drop old `agent_runtime` (which kills child), then spawn new one. The old reader thread will exit naturally when stdout closes. No explicit join needed (threads are detached).

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in (`#[cfg(test)] mod tests`) + cargo test |
| Config file | none (inline tests) |
| Quick run command | `cargo test -p glass_core --lib 2>&1` |
| Full suite command | `cargo test --workspace 2>&1` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| AGTC-01 | [agent.permissions] parses from TOML | unit | `cargo test -p glass_core --lib config 2>&1` | ✅ config.rs tests exist |
| AGTC-01 | [agent.quiet_rules] parses from TOML | unit | `cargo test -p glass_core --lib config 2>&1` | ✅ config.rs tests exist |
| AGTC-01 | Agent section with all fields parses correctly | unit | `cargo test -p glass_core --lib config 2>&1` | ✅ config.rs tests exist |
| AGTC-02 | PermissionLevel::Never drops proposal | unit | `cargo test -p glass_core --lib 2>&1` | ❌ Wave 0 |
| AGTC-02 | PermissionLevel::Approve is default | unit | `cargo test -p glass_core --lib 2>&1` | ❌ Wave 0 |
| AGTC-03 | ignore_exit_zero suppresses Success events | unit | `cargo test -p glass_core --lib 2>&1` | ❌ Wave 0 |
| AGTC-03 | ignore_patterns substring match works | unit | `cargo test -p glass_core --lib 2>&1` | ❌ Wave 0 |
| AGTC-04 | ConfigError displays binary-not-found hint | unit | `cargo test -p glass_core --lib config 2>&1` | ✅ config.rs error display tests |
| AGTC-05 | AgentRuntime registers/deregisters via CoordinationDb | integration | `cargo test -p glass_coordination --lib 2>&1` | ✅ coordination db tests exist |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_core --lib 2>&1`
- **Per wave merge:** `cargo test --workspace 2>&1`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] Tests for `PermissionMatrix` parsing in `crates/glass_core/src/config.rs` tests block
- [ ] Tests for `QuietRules` parsing in `crates/glass_core/src/config.rs` tests block
- [ ] Unit tests for `classify_proposal` helper function (can live in config.rs or agent_runtime.rs)
- [ ] Unit tests for quiet_rules filter logic (pure function, easy to extract)

*(Existing coordination tests cover CoordinationDb -- no new fixtures needed for lock/unlock operations)*

## Sources

### Primary (HIGH confidence)
- `crates/glass_core/src/config.rs` - Full `AgentSection`, `GlassConfig`, existing serde patterns, `ConfigError` struct
- `crates/glass_core/src/agent_runtime.rs` - `AgentMode`, `AgentRuntimeConfig`, `CooldownTracker`, `BudgetTracker`, `extract_proposal`, `build_agent_command_args`
- `crates/glass_core/src/activity_stream.rs` - `ActivityFilter::process()`, channel, `ActivityEvent` fields
- `crates/glass_core/src/config_watcher.rs` - `spawn_config_watcher`, `AppEvent::ConfigReloaded` dispatch
- `crates/glass_coordination/src/db.rs` - `CoordinationDb` API (register_agent, acquire_locks, release_locks, deregister_agent)
- `crates/glass_coordination/src/types.rs` - `AgentInfo`, `LockResult`, `FileLock`, `LockConflict`
- `src/main.rs` - `Processor`, `AgentRuntime` struct, `try_spawn_agent`, `ConfigReloaded` arm, `AgentProposal` arm, startup in `resumed()`

### Secondary (MEDIUM confidence)
- `.planning/STATE.md` - Accumulated decisions from phases 56-59 (patterns and pitfalls logged)
- `.planning/REQUIREMENTS.md` - AGTC-01 through AGTC-05 spec language

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - all libraries are in-tree, no new deps required
- Architecture: HIGH - all extension points identified with exact file/line references
- Pitfalls: HIGH - sourced from in-codebase patterns and STATE.md accumulated decisions

**Research date:** 2026-03-13
**Valid until:** 2026-04-13 (stable internal codebase, 30-day window)

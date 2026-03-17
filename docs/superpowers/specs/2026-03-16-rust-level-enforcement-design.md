# Rust-Level Rule Enforcement — Design Spec

**Date**: 2026-03-16
**Status**: Draft
**Scope**: Convert 6 text-injection feedback rules into Rust-level enforcement in `main.rs`, update `RuleAction` enum in `glass_feedback`
**Depends on**: `docs/superpowers/specs/2026-03-16-orchestrator-feedback-loop-design.md`

---

## Problem

The orchestrator feedback loop produces behavioral rules, but 6 of 9 actions are text injections — suggestions appended to the agent's context that depend on LLM compliance. If the LLM ignores "commit your changes" or "give ONE instruction," the rule has no effect. This makes the feedback loop partially self-suggesting rather than genuinely self-improving.

## Solution

Convert the 6 text-injection actions into Rust-level enforcement that the orchestrator executes directly. The orchestrator runs git commands, splits agent responses, reverts out-of-scope files, and blocks forward progress — all without depending on LLM behavior.

---

## Updated RuleAction Enum

Replace text injection variants with typed enforcement variants in `glass_feedback/src/types.rs`:

```rust
pub enum RuleAction {
    // Rust-level enforcement (orchestrator executes directly)
    ForceCommit,
    IsolateCommit { file: String },
    SplitInstructions,
    RevertOutOfScope { files: Vec<String> },
    BlockUntilResolved { message: String },
    ExtendSilence { extra_secs: u64 },
    RunVerifyTwice,
    EarlyStuck { threshold: u32 },

    // Text injection (kept only for verify_progress)
    TextInjection(String),
}
```

Only `verify_progress` remains as `TextInjection` — the orchestrator cannot determine what "progress" means for an arbitrary task.

---

## Enforcement Implementations

### ForceCommit

**Trigger**: `iterations_since_last_commit >= 5` (from `RunState`)

Note: `RunState.iterations_since_last_commit` is a new field that tracks iterations since the last detected git commit. It is reset to 0 whenever a new commit is detected (by comparing `git rev-parse HEAD` against the stored SHA). This replaces the incorrect `iterations_since_last_commit` computation from the feedback loop spec.

**Enforcement**:
1. Only fire when the last verification result was not a regression (check `last_verified_iteration` and whether a revert occurred — prevents committing broken code)
2. Run background command: `git add -A && git commit -m "glass: auto-checkpoint iter {N}"`
3. Capture new SHA from `git rev-parse HEAD`
4. Update `last_good_commit` to new SHA — ensures metric guard reverts go to this checkpoint, not before it
5. Reset `iterations_since_last_commit` counter
6. Inject `[GLASS_AUTO_COMMIT] Glass committed {sha} due to uncommitted drift` into next context send

**Pattern**: Same as metric guard — `std::process::Command` with `.current_dir(&cwd)`, runs synchronously before context send.

### IsolateCommit { file }

**Trigger**: `recent_reverted_files` contains the file in `action_params["file"]`

**Enforcement**:
1. Only fire when the last verification result was not a regression (same guard as ForceCommit)
2. Check if file appears in `git diff --stat` (already captured at every silence event)
3. If modified: `git add <file> && git commit -m "glass: isolate {file}"`
4. Update `last_good_commit` to new SHA — metric guard revert won't touch this commit
5. Inject `[GLASS_AUTO_COMMIT] Glass isolated {file} in commit {sha}` into context

**Safety**: Only commits the specific hot file, not the entire working tree. Only fires when verification has not detected a regression.

### SplitInstructions

**Trigger**: `smaller_instructions` rule is active (confirmed/provisional/pinned)

**New state on OrchestratorState**:
```rust
pub instruction_buffer: Vec<String>,
```

**Enforcement**:
1. Intercept in the `OrchestratorResponse` handler where `AgentResponse::TypeText` is processed
2. If rule is active, parse response for numbered list items (lines matching `^\d+[.)]\s`)
3. If 2+ items found: type only the first into PTY, push rest into `instruction_buffer`
4. On subsequent silence triggers: if `instruction_buffer` is non-empty, pop next instruction and type into PTY — skip the agent entirely (don't send context, don't wait for response)
5. When buffer is empty, resume normal agent loop

**Parsing heuristic**: Split on lines matching `^\d+[.)]\s`. If no numbered items found, fall through to normal behavior (don't split prose responses).

**Edge cases**:
- If a buffered instruction is stale (agent already completed it), it will produce no change. The waste detector catches this on the next run and the `smaller_instructions` rule may get demoted by the regression guard.
- The `instruction_buffer` is cleared when a checkpoint cycle begins (`checkpoint_phase` transitions from Idle to WaitingForCheckpoint) or when the agent is respawned. After respawn, the new agent has fresh context and buffered instructions from the old context are invalid.

### RevertOutOfScope { files }

**Trigger**: `restrict_scope` rule is active AND files outside PRD deliverables appear in `git diff --stat`

**New state on OrchestratorState**:
```rust
pub prd_deliverable_files: Vec<String>,  // cached at orchestrator activation
```

**Enforcement**:
1. On orchestrator activation: parse PRD deliverables via `parse_prd_deliverables()`, cache in `prd_deliverable_files`
2. On each silence event: parse file paths from `git diff --stat` (already captured)
3. Compute out-of-scope files: files in diff but not in deliverables
4. For each out-of-scope file: `git checkout -- <file>`
5. Inject `[GLASS_SCOPE_GUARD] Reverted out-of-scope files: {list}` into context

**Scope matching**: Deliverables are matched by prefix — if a deliverable is `crates/glass_feedback/`, any file under that path is in-scope. This prevents false positives on support files (Cargo.toml, types.rs) when the PRD lists a directory or main file.

**Untracked files**: `git diff --stat` only shows tracked modified files. To catch new out-of-scope files, also run `git ls-files --others --exclude-standard` and apply the same scope check. Untracked out-of-scope files are removed with `rm`.

**Safety**: Only fires when `prd_deliverable_files` is non-empty. PRDs without a `## Deliverables` section skip this entirely. A threshold of 3+ out-of-scope files prevents reverting incidental single-file changes (e.g., a lockfile update).

### BlockUntilResolved { message }

**Trigger**: `build_dependency_first` rule is active AND dependency error pattern detected in iteration log

**New state on OrchestratorState**:
```rust
pub dependency_block: Option<String>,  // block message, None = not blocked
```

**Enforcement**:
1. When rule fires: set `dependency_block = Some(message)`
2. On next silence trigger, if blocked:
   - Don't send terminal context to the Glass Agent
   - Type the block message directly into PTY: `"STOP current task. {message}"`
   - Set `response_pending = true`
3. On subsequent silence triggers: check terminal output for dependency error keywords ("dependency", "not found", "undefined", "import" followed by backtrack pattern)
4. If error pattern absent: clear `dependency_block`, resume normal loop
5. If error persists after 3 blocked iterations: clear block anyway, let normal stuck detection handle it

**Resolution detection**: Check the exit code of the last completed command block (via `BlockManager`), not raw terminal text. If the last command exited 0 (success), the dependency is likely resolved. This avoids false matches from stale keywords in scrollback. Falls back to keyword absence in the last completed block's output (not the full 80-line scrollback) if exit code is unavailable.

### ExtendSilence { extra_secs }

**Trigger**: `extend_silence` rule is active

**Enforcement**: Already Rust-level. Set a flag on `OrchestratorState` that the silence handler checks to dynamically add seconds to the threshold.

### RunVerifyTwice

**Trigger**: `run_verify_twice` rule is active AND `verify_alternations >= 2`

**Enforcement**: Already Rust-level. Set a flag that the `VerifyComplete` handler checks. If set, re-run verification before declaring regression.

### EarlyStuck { threshold }

**Trigger**: `early_stuck` rule is active

**Enforcement**: Already Rust-level. Temporarily lower `max_retries` on `OrchestratorState`.

### TextInjection (verify_progress only)

**Trigger**: `verify_progress` rule is active AND `waste_rate > 0.15`

**Enforcement**: Append `"Verify progress before continuing"` to `[FEEDBACK_RULES]` section of context. This is the only remaining text injection — kept because the orchestrator cannot determine what "progress" means for an arbitrary task.

---

## Rule Engine Updates

The `RuleEngine::check_rules()` method in `glass_feedback/src/rules.rs` needs to return the new `RuleAction` variants instead of `TextInjection` for the converted actions:

| Action name | Old return | New return |
|---|---|---|
| `force_commit` | `TextInjection("Commit current changes...")` | `ForceCommit` |
| `isolate_commits` | `TextInjection("Commit {file} separately...")` | `IsolateCommit { file }` |
| `smaller_instructions` | `TextInjection("Give ONE instruction...")` | `SplitInstructions` |
| `restrict_scope` | `TextInjection("Only modify PRD files...")` | `RevertOutOfScope { files }` |
| `build_dependency_first` | `TextInjection("Build {dep}...")` | `BlockUntilResolved { message }` |
| `verify_progress` | `TextInjection("Verify progress...")` | `TextInjection("Verify progress...")` (unchanged) |

**API boundary**: The rule engine returns *signals*, not computed file lists. The silence handler in `main.rs` has access to `git diff --stat` and `prd_deliverable_files` and computes the actual enforcement:

- `check_rules(state, run_state)` → returns `RevertOutOfScope { files: vec![] }` as a signal that the restrict_scope rule is active
- The silence handler computes the actual out-of-scope files using `git diff --stat` and `prd_deliverable_files` from `OrchestratorState`
- This keeps the rule engine pure (no git/filesystem dependencies)

Similarly, `SplitInstructions` is returned by `check_rules()` as a signal. The `OrchestratorResponse` handler checks `is_rule_active("smaller_instructions")` — a new method on `RuleEngine` — to decide whether to split the response. `SplitInstructions` is not processed in the silence handler's action loop.

```rust
impl RuleEngine {
    /// Check if a specific rule action is active (confirmed/provisional/pinned).
    pub fn is_rule_active(&self, action_name: &str) -> bool { ... }
}
```

---

## OrchestratorState Changes

New fields added to `OrchestratorState` in `orchestrator.rs`:

```rust
pub instruction_buffer: Vec<String>,           // buffered split instructions
pub dependency_block: Option<String>,          // active dependency block message
pub dependency_block_iterations: u32,          // iterations while blocked (max DEPENDENCY_BLOCK_MAX_ITERATIONS)
pub prd_deliverable_files: Vec<String>,        // cached PRD deliverables
```

All initialized to empty/None/0 in `new()` and reset on activation.

```rust
const DEPENDENCY_BLOCK_MAX_ITERATIONS: u32 = 3;
```

---

## Enforcement Flow in OrchestratorSilence Handler

The enforcement runs at the existing feedback rules evaluation point (line ~7355), replacing the text injection loop:

```
for action in check_rules(feedback_state, &run_state):
    match action:
        ForceCommit →
            git commit -am "glass: auto-checkpoint"
            inject notification into context

        IsolateCommit { file } →
            if file in git diff: git add + commit
            update last_good_commit
            inject notification into context

        RevertOutOfScope { files } →
            for file: git checkout -- file
            inject notification into context

        BlockUntilResolved { message } →
            set dependency_block
            (handler exits early on next silence if blocked)

        ExtendSilence { extra_secs } →
            set extend flag on state

        RunVerifyTwice →
            set verify-twice flag

        EarlyStuck { threshold } →
            temporarily lower max_retries

        TextInjection(text) →
            append to [FEEDBACK_RULES] section (verify_progress only)
```

`SplitInstructions` is handled separately in the `OrchestratorResponse` handler, not in the silence handler, because it intercepts the agent's response before it's typed into the PTY.

---

## Instruction Buffer Flow in OrchestratorResponse Handler

At the point where `AgentResponse::TypeText(text)` is about to be typed into the PTY:

```
if smaller_instructions rule is active:
    items = parse_numbered_items(text)
    if items.len() >= 2:
        type items[0] into PTY
        push items[1..] into instruction_buffer
        return  // don't type the full response
    // else: fall through, type normally
```

In the `OrchestratorSilence` handler, before sending context to the agent:

```
if instruction_buffer is non-empty:
    pop next instruction
    type into PTY
    set response_pending = true
    return  // skip agent, use buffered instruction
```

---

## Context Notifications

All enforcement actions inject a notification into the next context send so the Glass Agent knows what happened:

| Action | Notification |
|---|---|
| `ForceCommit` | `[GLASS_AUTO_COMMIT] Glass committed {sha} due to uncommitted drift` |
| `IsolateCommit` | `[GLASS_AUTO_COMMIT] Glass isolated {file} in commit {sha}` |
| `RevertOutOfScope` | `[GLASS_SCOPE_GUARD] Reverted out-of-scope files: {list}` |
| `BlockUntilResolved` | `[GLASS_DEPENDENCY_BLOCK] Blocked: {message}` |
| `SplitInstructions` | `[GLASS_SPLIT] Sending instruction {N} of {total} from buffered response` |

These are informational — they tell the agent what Glass did, not what the agent should do. The enforcement already happened.

---

## Enforcement Summary

| Action | Enforcement mechanism | LLM needed? |
|---|---|---|
| `ForceCommit` | Background `git commit -am` | No |
| `IsolateCommit` | Background `git add <file> && git commit` | No |
| `SplitInstructions` | Parse response, buffer, send one at a time | No |
| `RevertOutOfScope` | `git checkout --` per out-of-scope file | No |
| `BlockUntilResolved` | Block context send, type message into PTY | No |
| `ExtendSilence` | Set flag on OrchestratorState | No |
| `RunVerifyTwice` | Set flag for verify handler | No |
| `EarlyStuck` | Lower threshold on OrchestratorState | No |
| `TextInjection` (verify_progress) | Append to context | Yes |

**Result: 8 of 9 actions are fully enforced in Rust. The feedback loop is genuinely self-improving.**

---

## Testing Strategy

- **ForceCommit / IsolateCommit**: test with a temp git repo — verify commit is created, SHA captured, last_good_commit updated
- **SplitInstructions**: test parsing heuristic with various response formats (numbered, lettered, prose). Test buffer pop/resume flow.
- **RevertOutOfScope**: test with temp git repo + modified files — verify only out-of-scope files reverted
- **BlockUntilResolved**: test block/unblock state transitions, 3-iteration safety limit
- **Rule engine**: update existing tests to verify new RuleAction variants are returned instead of TextInjection
- **Integration**: end-to-end test with a simulated run that triggers enforcement actions

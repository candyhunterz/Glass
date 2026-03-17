# Rust-Level Rule Enforcement Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert 6 text-injection feedback rules into Rust-level enforcement so the orchestrator self-improves through code execution, not LLM compliance.

**Architecture:** Update `RuleAction` enum with typed enforcement variants, update `RuleEngine::check_rules()` to return them, add `is_rule_active()` method, add enforcement state to `OrchestratorState`, implement enforcement handlers in `main.rs` at the existing feedback rules evaluation point and the `OrchestratorResponse` handler.

**Tech Stack:** Rust, existing `std::process::Command` for git ops, existing `PtyMsg::Input` for PTY writes.

**Spec:** `docs/superpowers/specs/2026-03-16-rust-level-enforcement-design.md`

---

## File Map

| File | Changes |
|---|---|
| `crates/glass_feedback/src/types.rs` | Update `RuleAction` enum, add `iterations_since_last_commit` to `RunState` |
| `crates/glass_feedback/src/rules.rs` | Update `check_rules()` to return new variants, add `is_rule_active()` |
| `src/orchestrator.rs` | Add enforcement state fields, add `DEPENDENCY_BLOCK_MAX_ITERATIONS` constant |
| `src/main.rs` | Add enforcement handlers in `OrchestratorSilence` and `OrchestratorResponse`, update `RunState` construction, add `prd_deliverable_files` caching |

---

## Chunk 1: Type Changes & Rule Engine Updates

### Task 1: Update RuleAction enum

**Files:**
- Modify: `crates/glass_feedback/src/types.rs`

- [ ] **Step 1: Write tests for new RuleAction variants**

Add to the existing test module in `types.rs`:

```rust
#[test]
fn rule_action_force_commit() {
    let action = RuleAction::ForceCommit;
    assert!(matches!(action, RuleAction::ForceCommit));
}

#[test]
fn rule_action_isolate_commit() {
    let action = RuleAction::IsolateCommit { file: "src/main.rs".to_string() };
    if let RuleAction::IsolateCommit { file } = &action {
        assert_eq!(file, "src/main.rs");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn rule_action_split_instructions() {
    let action = RuleAction::SplitInstructions;
    assert!(matches!(action, RuleAction::SplitInstructions));
}

#[test]
fn rule_action_revert_out_of_scope() {
    let action = RuleAction::RevertOutOfScope { files: vec!["foo.rs".to_string()] };
    if let RuleAction::RevertOutOfScope { files } = &action {
        assert_eq!(files.len(), 1);
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn rule_action_block_until_resolved() {
    let action = RuleAction::BlockUntilResolved { message: "Build dep first".to_string() };
    assert!(matches!(action, RuleAction::BlockUntilResolved { .. }));
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p glass_feedback
```

- [ ] **Step 3: Update the RuleAction enum**

Replace the current `RuleAction` enum with:

```rust
/// Actions returned by the rule engine at runtime.
#[derive(Debug, Clone)]
pub enum RuleAction {
    /// Rust-level: run git commit -am to checkpoint.
    ForceCommit,
    /// Rust-level: git add + commit a specific hot file in isolation.
    IsolateCommit { file: String },
    /// Rust-level: signal that instruction splitting is active.
    /// Handled in OrchestratorResponse, not in silence handler.
    SplitInstructions,
    /// Rust-level: signal that scope guard is active.
    /// Silence handler computes actual files and reverts them.
    RevertOutOfScope { files: Vec<String> },
    /// Rust-level: block forward progress until dependency resolved.
    BlockUntilResolved { message: String },
    /// Rust-level: extend silence threshold by N seconds.
    ExtendSilence { extra_secs: u64 },
    /// Rust-level: run verification twice before reverting.
    RunVerifyTwice,
    /// Rust-level: lower stuck detection threshold.
    EarlyStuck { threshold: u32 },
    /// Text injection (kept only for verify_progress).
    TextInjection(String),
}
```

- [ ] **Step 4: Add `iterations_since_last_commit` to RunState**

Replace `uncommitted_iterations` with `iterations_since_last_commit` in `RunState`:

```rust
pub struct RunState {
    pub iteration: u32,
    pub iterations_since_last_commit: u32,
    pub revert_rate: f64,
    pub stuck_rate: f64,
    pub waste_rate: f64,
    pub recent_reverted_files: Vec<String>,
    pub verify_alternations: u32,
}
```

- [ ] **Step 5: Fix any tests that reference old RuleAction variants or `uncommitted_iterations`**

Search for `TextInjection` in test assertions and `uncommitted_iterations` references. Update to match new variants and field name.

- [ ] **Step 6: Run tests**

```bash
cargo test -p glass_feedback
```
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/glass_feedback/src/types.rs
git commit -m "feat(feedback): update RuleAction enum with Rust-level enforcement variants"
```

---

### Task 2: Update RuleEngine to return new variants

**Files:**
- Modify: `crates/glass_feedback/src/rules.rs`

- [ ] **Step 1: Write tests for updated check_rules and is_rule_active**

Add to existing test module:

```rust
#[test]
fn check_rules_force_commit_returns_variant() {
    let engine = RuleEngine {
        rules: vec![make_test_rule("r1", "force_commit", RuleStatus::Confirmed)],
    };
    let state = RunState {
        iterations_since_last_commit: 6,
        ..Default::default()
    };
    let actions = engine.check_rules(&state);
    assert!(actions.iter().any(|a| matches!(a, RuleAction::ForceCommit)));
}

#[test]
fn check_rules_isolate_commit_returns_variant() {
    let mut rule = make_test_rule("r1", "isolate_commits", RuleStatus::Confirmed);
    rule.action_params.insert("file".to_string(), "src/main.rs".to_string());
    let engine = RuleEngine { rules: vec![rule] };
    let state = RunState {
        recent_reverted_files: vec!["src/main.rs".to_string()],
        ..Default::default()
    };
    let actions = engine.check_rules(&state);
    assert!(actions.iter().any(|a| matches!(a, RuleAction::IsolateCommit { .. })));
}

#[test]
fn check_rules_smaller_instructions_returns_split() {
    let engine = RuleEngine {
        rules: vec![make_test_rule("r1", "smaller_instructions", RuleStatus::Confirmed)],
    };
    let state = RunState::default();
    let actions = engine.check_rules(&state);
    assert!(actions.iter().any(|a| matches!(a, RuleAction::SplitInstructions)));
}

#[test]
fn check_rules_restrict_scope_returns_revert() {
    let engine = RuleEngine {
        rules: vec![make_test_rule("r1", "restrict_scope", RuleStatus::Confirmed)],
    };
    let state = RunState::default();
    let actions = engine.check_rules(&state);
    assert!(actions.iter().any(|a| matches!(a, RuleAction::RevertOutOfScope { .. })));
}

#[test]
fn check_rules_build_dependency_returns_block() {
    let engine = RuleEngine {
        rules: vec![make_test_rule("r1", "build_dependency_first", RuleStatus::Confirmed)],
    };
    let state = RunState::default();
    let actions = engine.check_rules(&state);
    assert!(actions.iter().any(|a| matches!(a, RuleAction::BlockUntilResolved { .. })));
}

#[test]
fn check_rules_verify_progress_still_text() {
    let engine = RuleEngine {
        rules: vec![make_test_rule("r1", "verify_progress", RuleStatus::Confirmed)],
    };
    let state = RunState { waste_rate: 0.2, ..Default::default() };
    let actions = engine.check_rules(&state);
    assert!(actions.iter().any(|a| matches!(a, RuleAction::TextInjection(_))));
}

#[test]
fn is_rule_active_true() {
    let engine = RuleEngine {
        rules: vec![make_test_rule("r1", "smaller_instructions", RuleStatus::Confirmed)],
    };
    assert!(engine.is_rule_active("smaller_instructions"));
}

#[test]
fn is_rule_active_false_rejected() {
    let engine = RuleEngine {
        rules: vec![make_test_rule("r1", "smaller_instructions", RuleStatus::Rejected)],
    };
    assert!(!engine.is_rule_active("smaller_instructions"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Update check_rules() to return new variants**

In the match block for each action:
- `"force_commit"` → return `RuleAction::ForceCommit` (trigger: `state.iterations_since_last_commit >= 5`)
- `"isolate_commits"` → return `RuleAction::IsolateCommit { file }` (trigger: file in reverted_files)
- `"smaller_instructions"` → return `RuleAction::SplitInstructions` (always)
- `"restrict_scope"` → return `RuleAction::RevertOutOfScope { files: vec![] }` (always, files computed by caller)
- `"build_dependency_first"` → return `RuleAction::BlockUntilResolved { message }` (always)
- `"verify_progress"` → keep as `RuleAction::TextInjection(...)` (trigger: waste_rate > 0.15)
- `"extend_silence"` → keep as `RuleAction::ExtendSilence` (already correct)
- `"run_verify_twice"` → keep as `RuleAction::RunVerifyTwice` (already correct)
- `"early_stuck"` → keep as `RuleAction::EarlyStuck` (already correct)

- [ ] **Step 4: Add is_rule_active() method**

```rust
impl RuleEngine {
    /// Check if a specific rule action is active (confirmed/provisional/pinned).
    pub fn is_rule_active(&self, action_name: &str) -> bool {
        self.rules.iter().any(|r| {
            r.action == action_name
                && matches!(
                    r.status,
                    RuleStatus::Confirmed | RuleStatus::Provisional | RuleStatus::Pinned
                )
        })
    }
}
```

- [ ] **Step 5: Fix any broken tests from the variant changes**

Update existing tests that asserted `TextInjection` for actions that now return typed variants.

- [ ] **Step 6: Run tests**

```bash
cargo test -p glass_feedback
```
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/glass_feedback/src/rules.rs
git commit -m "feat(feedback): rule engine returns Rust-level enforcement variants"
```

---

## Chunk 2: OrchestratorState & Enforcement Handlers

### Task 3: Add enforcement state to OrchestratorState

**Files:**
- Modify: `src/orchestrator.rs`

- [ ] **Step 1: Add new fields to OrchestratorState**

```rust
/// Buffered split instructions (one-at-a-time enforcement).
pub instruction_buffer: Vec<String>,
/// Active dependency block message (None = not blocked).
pub dependency_block: Option<String>,
/// Iterations spent while dependency-blocked (safety limit).
pub dependency_block_iterations: u32,
/// Cached PRD deliverable file paths (for scope guard).
pub prd_deliverable_files: Vec<String>,
/// Iterations since the last detected git commit.
pub iterations_since_last_commit: u32,
/// Last known git HEAD SHA (for commit detection).
pub last_known_head: Option<String>,
```

- [ ] **Step 2: Add constant**

```rust
pub const DEPENDENCY_BLOCK_MAX_ITERATIONS: u32 = 3;
```

- [ ] **Step 3: Initialize in new()**

All set to empty/None/0.

- [ ] **Step 4: Run tests**

```bash
cargo test --workspace
```
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/orchestrator.rs
git commit -m "feat(orchestrator): add enforcement state fields for feedback loop"
```

---

### Task 4: Implement enforcement in OrchestratorSilence handler

**Files:**
- Modify: `src/main.rs`

This is the largest task. READ the existing feedback rules evaluation section (around line 7355) and replace the text injection loop with enforcement handlers.

- [ ] **Step 1: Cache PRD deliverables on orchestrator activation**

In both activation paths (Ctrl+Shift+O and config hot-reload), after setting `self.orchestrator.active = true`:

```rust
// Cache PRD deliverables for scope guard
let prd_rel = self.config.agent.as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .map(|o| o.prd_path.as_str())
    .unwrap_or("PRD.md");
let prd_path = std::path::Path::new(&current_cwd).join(prd_rel);
if let Ok(content) = std::fs::read_to_string(&prd_path) {
    self.orchestrator.prd_deliverable_files = orchestrator::parse_prd_deliverables(&content);
}
```

- [ ] **Step 2: Add commit detection to track iterations_since_last_commit**

In the `OrchestratorSilence` handler, after capturing `git diff --stat`, add:

```rust
// Detect new commits for iterations_since_last_commit tracking
let current_head = std::process::Command::new("git")
    .args(["rev-parse", "HEAD"])
    .current_dir(&cwd)
    .output()
    .ok()
    .and_then(|o| if o.status.success() {
        String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
    } else { None });

if let Some(ref head) = current_head {
    if self.orchestrator.last_known_head.as_ref() != Some(head) {
        self.orchestrator.iterations_since_last_commit = 0;
        self.orchestrator.last_known_head = Some(head.clone());
    } else {
        self.orchestrator.iterations_since_last_commit += 1;
    }
}
```

- [ ] **Step 3: Add dependency block check**

Before the existing feedback rules evaluation, add:

```rust
// Dependency block enforcement
if let Some(ref block_msg) = self.orchestrator.dependency_block {
    self.orchestrator.dependency_block_iterations += 1;

    // Check if resolved: last command exited 0
    let resolved = /* check last block exit code via block_manager */;

    if resolved || self.orchestrator.dependency_block_iterations
        >= orchestrator::DEPENDENCY_BLOCK_MAX_ITERATIONS
    {
        self.orchestrator.dependency_block = None;
        self.orchestrator.dependency_block_iterations = 0;
        tracing::info!("Orchestrator: dependency block cleared");
    } else {
        // Type block message into PTY
        let msg = format!("STOP current task. {}\n", block_msg);
        if let Some(session) = /* get focused session */ {
            let _ = session.pty_sender.send(PtyMsg::Input(
                std::borrow::Cow::Owned(msg.into_bytes())
            ));
            self.orchestrator.mark_pty_write();
        }
        self.orchestrator.response_pending = true;
        return;
    }
}
```

- [ ] **Step 4: Add instruction buffer check**

Before sending context to agent:

```rust
// Instruction buffer: send next buffered instruction instead of asking agent
if !self.orchestrator.instruction_buffer.is_empty() {
    let next_instruction = self.orchestrator.instruction_buffer.remove(0);
    let remaining = self.orchestrator.instruction_buffer.len();

    let notification = format!(
        "[GLASS_SPLIT] Sending instruction {} of {} from buffered response\n",
        /* current */ , /* total */
    );

    if let Some(session) = /* get focused session */ {
        let msg = format!("{}\n", next_instruction);
        let _ = session.pty_sender.send(PtyMsg::Input(
            std::borrow::Cow::Owned(msg.into_bytes())
        ));
        self.orchestrator.mark_pty_write();
    }
    self.orchestrator.response_pending = true;
    return;
}
```

- [ ] **Step 5: Replace text injection loop with enforcement handlers**

Replace the existing `for action in &actions` loop with:

```rust
let mut feedback_notifications = Vec::new();

for action in &actions {
    match action {
        glass_feedback::RuleAction::ForceCommit => {
            // Only if last verify was not a regression
            let last_was_regression = /* check */;
            if !last_was_regression {
                let output = std::process::Command::new("git")
                    .args(["commit", "-am", &format!("glass: auto-checkpoint iter {}", self.orchestrator.iteration)])
                    .current_dir(&cwd)
                    .output();
                if let Ok(o) = output {
                    if o.status.success() {
                        // Update last_good_commit
                        if let Some(sha) = /* get new HEAD */ {
                            self.orchestrator.last_good_commit = Some(sha.clone());
                            self.orchestrator.iterations_since_last_commit = 0;
                            self.orchestrator.last_known_head = Some(sha.clone());
                            feedback_notifications.push(
                                format!("[GLASS_AUTO_COMMIT] Glass committed {} due to uncommitted drift", sha)
                            );
                        }
                    }
                }
            }
        }
        glass_feedback::RuleAction::IsolateCommit { file } => {
            // Similar to ForceCommit but only for the specific file
            let last_was_regression = /* check */;
            if !last_was_regression {
                if let Some(ref diff) = git_diff {
                    if diff.contains(file) {
                        let _ = std::process::Command::new("git")
                            .args(["add", file])
                            .current_dir(&cwd)
                            .output();
                        let output = std::process::Command::new("git")
                            .args(["commit", "-m", &format!("glass: isolate {}", file)])
                            .current_dir(&cwd)
                            .output();
                        if let Ok(o) = output {
                            if o.status.success() {
                                if let Some(sha) = /* get new HEAD */ {
                                    self.orchestrator.last_good_commit = Some(sha.clone());
                                    feedback_notifications.push(
                                        format!("[GLASS_AUTO_COMMIT] Glass isolated {} in commit {}", file, sha)
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        glass_feedback::RuleAction::RevertOutOfScope { .. } => {
            // Compute actual out-of-scope files
            if !self.orchestrator.prd_deliverable_files.is_empty() {
                if let Some(ref diff) = git_diff {
                    let changed: Vec<String> = /* parse file paths from diff --stat */;
                    let out_of_scope: Vec<String> = changed.iter()
                        .filter(|f| !self.orchestrator.prd_deliverable_files.iter()
                            .any(|d| f.starts_with(d) || f == d))
                        .cloned()
                        .collect();

                    if out_of_scope.len() >= 3 {
                        for file in &out_of_scope {
                            let _ = std::process::Command::new("git")
                                .args(["checkout", "--", file])
                                .current_dir(&cwd)
                                .output();
                        }
                        feedback_notifications.push(
                            format!("[GLASS_SCOPE_GUARD] Reverted {} out-of-scope files: {}",
                                out_of_scope.len(),
                                out_of_scope.join(", "))
                        );
                    }
                }
            }
        }
        glass_feedback::RuleAction::BlockUntilResolved { message } => {
            if self.orchestrator.dependency_block.is_none() {
                self.orchestrator.dependency_block = Some(message.clone());
                self.orchestrator.dependency_block_iterations = 0;
            }
        }
        glass_feedback::RuleAction::ExtendSilence { .. } => {
            // Already handled — flag-based
        }
        glass_feedback::RuleAction::RunVerifyTwice => {
            // Already handled — flag-based
        }
        glass_feedback::RuleAction::EarlyStuck { .. } => {
            // Already handled — flag-based
        }
        glass_feedback::RuleAction::SplitInstructions => {
            // Handled in OrchestratorResponse, not here
        }
        glass_feedback::RuleAction::TextInjection(text) => {
            feedback_notifications.push(format!("[FEEDBACK_RULES] {}", text));
        }
    }
}

// Append notifications to context
if !feedback_notifications.is_empty() {
    for note in &feedback_notifications {
        content.push_str(&format!("\n{}\n", note));
    }
}
```

- [ ] **Step 6: Update RunState construction**

Update the `RunState` construction to use the new field name:

```rust
let run_state = glass_feedback::RunState {
    iteration: self.orchestrator.iteration,
    iterations_since_last_commit: self.orchestrator.iterations_since_last_commit,
    // ... rest unchanged
};
```

- [ ] **Step 7: Build and test**

```bash
cargo build
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

- [ ] **Step 8: Commit**

```bash
git add src/main.rs
git commit -m "feat(orchestrator): Rust-level enforcement — commit, isolate, scope guard, block"
```

---

### Task 5: Implement instruction splitting in OrchestratorResponse handler

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Find the OrchestratorResponse handler**

Search for `AgentResponse::TypeText` in the `OrchestratorResponse` handler (around line 6705).

- [ ] **Step 2: Add instruction splitting before PTY write**

Before the line that types text into the PTY, add:

```rust
// Instruction splitting enforcement
let text_to_type = if self.feedback_state.as_ref()
    .map(|fs| fs.engine.is_rule_active("smaller_instructions"))
    .unwrap_or(false)
{
    // Parse numbered list items
    let items: Vec<String> = parse_numbered_instructions(&text);
    if items.len() >= 2 {
        // Buffer items 2..N, type only item 1
        let first = items[0].clone();
        self.orchestrator.instruction_buffer = items[1..].to_vec();
        tracing::info!(
            "Orchestrator: split {} instructions, buffering {}",
            items.len(),
            items.len() - 1
        );
        first
    } else {
        text.clone()
    }
} else {
    text.clone()
};
```

Then use `text_to_type` instead of `text` when writing to PTY.

- [ ] **Step 3: Write the parse_numbered_instructions helper**

```rust
/// Parse numbered list items from an agent response.
/// Matches lines starting with `1.`, `2)`, etc.
fn parse_numbered_instructions(text: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        let trimmed = line.trim();
        // Check if line starts a new numbered item
        if trimmed.len() >= 2 {
            let first_char = trimmed.chars().next().unwrap_or(' ');
            let second_char = trimmed.chars().nth(1).unwrap_or(' ');
            if first_char.is_ascii_digit() && (second_char == '.' || second_char == ')') {
                if !current.is_empty() {
                    items.push(current.trim().to_string());
                }
                current = trimmed.to_string();
                continue;
            }
        }
        if !current.is_empty() {
            current.push('\n');
            current.push_str(line);
        } else {
            current.push_str(line);
            current.push('\n');
        }
    }
    if !current.is_empty() {
        items.push(current.trim().to_string());
    }

    // Only return as split items if we found numbered items
    if items.len() >= 2 {
        items
    } else {
        vec![text.to_string()]
    }
}
```

- [ ] **Step 4: Clear instruction buffer on checkpoint/respawn**

In `respawn_orchestrator_agent()`, add:
```rust
self.orchestrator.instruction_buffer.clear();
```

Also clear when checkpoint phase transitions to WaitingForCheckpoint.

- [ ] **Step 5: Build and test**

```bash
cargo build
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat(orchestrator): instruction splitting enforcement — buffer and send one at a time"
```

---

### Task 6: Reset enforcement state on activation and add helper

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Reset enforcement fields on activation**

In both Ctrl+Shift+O and config hot-reload activation paths, add:

```rust
self.orchestrator.instruction_buffer.clear();
self.orchestrator.dependency_block = None;
self.orchestrator.dependency_block_iterations = 0;
self.orchestrator.iterations_since_last_commit = 0;
self.orchestrator.last_known_head = None;
```

- [ ] **Step 2: Add a helper to parse file paths from git diff --stat**

```rust
/// Parse file paths from `git diff --stat` output.
/// Each line looks like: " src/main.rs | 5 ++---"
fn parse_diff_stat_files(diff_stat: &str) -> Vec<String> {
    diff_stat.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.contains('|') {
                Some(trimmed.split('|').next()?.trim().to_string())
            } else {
                None
            }
        })
        .collect()
}
```

- [ ] **Step 3: Build, test, clippy**

```bash
cargo build
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat(orchestrator): reset enforcement state on activation, add diff stat parser"
```

---

## Chunk 3: Tests & Final Verification

### Task 7: Add enforcement-specific tests

**Files:**
- Modify: `crates/glass_feedback/src/rules.rs` (update existing tests)
- Modify: `src/main.rs` (add parse_numbered_instructions tests)

- [ ] **Step 1: Add parse_numbered_instructions tests to main.rs**

In the `#[cfg(test)] mod tests` section at the end of `main.rs` (or in `src/tests.rs` if it exists):

```rust
#[test]
fn parse_numbered_instructions_splits() {
    let text = "1. Build the API endpoint\n2. Write unit tests\n3. Update the docs";
    let items = parse_numbered_instructions(text);
    assert_eq!(items.len(), 3);
    assert!(items[0].contains("Build the API"));
    assert!(items[1].contains("Write unit tests"));
    assert!(items[2].contains("Update the docs"));
}

#[test]
fn parse_numbered_instructions_no_numbers() {
    let text = "Just do the thing and make it work";
    let items = parse_numbered_instructions(text);
    assert_eq!(items.len(), 1);
}

#[test]
fn parse_numbered_instructions_single_item() {
    let text = "1. Only one instruction here";
    let items = parse_numbered_instructions(text);
    assert_eq!(items.len(), 1); // single item = don't split
}

#[test]
fn parse_numbered_instructions_multiline_items() {
    let text = "1. Build the API\n   with proper error handling\n2. Write tests";
    let items = parse_numbered_instructions(text);
    assert_eq!(items.len(), 2);
    assert!(items[0].contains("error handling"));
}

#[test]
fn parse_diff_stat_files_parses() {
    let diff = " src/main.rs     | 15 +++---\n crates/foo/lib.rs | 3 +\n 2 files changed";
    let files = parse_diff_stat_files(diff);
    assert_eq!(files.len(), 2);
    assert_eq!(files[0], "src/main.rs");
    assert_eq!(files[1], "crates/foo/lib.rs");
}

#[test]
fn parse_diff_stat_files_empty() {
    let diff = "";
    let files = parse_diff_stat_files(diff);
    assert!(files.is_empty());
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test --workspace
```
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "test(orchestrator): add enforcement helper tests — instruction splitting, diff parsing"
```

---

### Task 8: Final workspace verification

**Files:**
- All

- [ ] **Step 1: Full build**

```bash
cargo build
```

- [ ] **Step 2: Full test suite**

```bash
cargo test --workspace
```

- [ ] **Step 3: Clippy**

```bash
cargo clippy --workspace -- -D warnings
```

- [ ] **Step 4: Format check**

```bash
cargo fmt --all -- --check
```

All must pass with zero warnings and zero failures.

- [ ] **Step 5: Final commit if any formatting fixes needed**

```bash
git add -A
git commit -m "style: formatting fixes for enforcement implementation"
```

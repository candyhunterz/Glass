# Scripting Lifecycle Wiring Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the lifecycle loop so scripts are promoted, rejected, and aged based on run outcomes — making the self-improvement system actually self-correcting.

**Architecture:** The bridge (`src/script_bridge.rs`) becomes the coordination point between `glass_feedback` regression results and `glass_scripting::lifecycle` functions. ScriptSystem gains run-tracking state. The ConfigReload action loop is broken by a guard flag.

**Tech Stack:** Rust, glass_scripting lifecycle module, glass_feedback regression results

**Spec:** `docs/superpowers/specs/2026-03-18-self-improvement-scripting-design.md`

---

## File Map

| File | Change |
|------|--------|
| `src/script_bridge.rs` | Add lifecycle wiring: record failures/triggers per run, promote/reject on run end, ConfigReload loop guard |
| `src/main.rs` | Call bridge lifecycle methods after `on_run_end`, pass regression result |
| `crates/glass_scripting/src/lib.rs` | Expose script manifest paths from ScriptSystem, add run tracking |
| `crates/glass_scripting/src/hooks.rs` | Expose manifest_path on scripts_for results |

---

## Task 1: Track Script Execution Per Run

**Files:**
- Modify: `src/script_bridge.rs`

The bridge needs to track which scripts fired and which errored during a run, so it can call lifecycle functions at run end.

- [ ] **Step 1: Add per-run tracking state to ScriptBridge**

```rust
use std::collections::{HashMap, HashSet};

pub struct ScriptBridge {
    system: ScriptSystem,
    tool_registry: ScriptToolRegistry,
    enabled: bool,
    project_root: Option<String>,
    // Per-run tracking (reset on each orchestrator run start)
    scripts_triggered: HashSet<String>,    // script names that fired successfully
    scripts_errored: HashMap<String, u32>, // script name -> consecutive error count this run
    config_reload_guard: bool,             // prevents ConfigReload -> SetConfig -> ConfigReload loop
}
```

- [ ] **Step 2: Initialize tracking in new() and reset on run start**

In `new()`:
```rust
scripts_triggered: HashSet::new(),
scripts_errored: HashMap::new(),
config_reload_guard: false,
```

Add a `reset_run_tracking(&mut self)` method:
```rust
pub fn reset_run_tracking(&mut self) {
    self.scripts_triggered.clear();
    self.scripts_errored.clear();
}
```

- [ ] **Step 3: Record triggers and errors in run_hook**

Update the private `run_hook` method to track outcomes. After running the hook and before returning actions:

```rust
fn run_hook(&mut self, hook: HookPoint, context: &HookContext, event_data: &HookEventData) -> Vec<Action> {
    if !self.enabled {
        return Vec::new();
    }
    let result = self.system.run_hook(hook.clone(), context, event_data);

    // Track which scripts fired successfully
    let scripts = self.system.scripts_for_hook(&hook);
    for script in &scripts {
        if !result.errors.iter().any(|(name, _)| name == &script.name) {
            self.scripts_triggered.insert(script.name.clone());
        }
    }

    // Track errors
    for (name, err) in &result.errors {
        tracing::warn!("ScriptBridge: script '{name}' error on {hook:?}: {err}");
        *self.scripts_errored.entry(name.clone()).or_insert(0) += 1;
    }

    result.actions
}
```

Note: `run_hook` currently takes `&self` — it needs to become `&mut self`. This means ALL hook methods that call it (`on_command_complete`, `on_orchestrator_iteration`, etc.) also need `&mut self`. Update their signatures. Then update all call sites in main.rs from `self.script_bridge.on_X(...)` — these should already work since `self` is `&mut self` in event handlers.

- [ ] **Step 4: Add scripts_for_hook to ScriptSystem**

In `crates/glass_scripting/src/lib.rs`, add a method that returns script names + manifest paths for a hook:

```rust
pub struct ScriptInfo {
    pub name: String,
    pub manifest_path: std::path::PathBuf,
    pub status: ScriptStatus,
    pub origin: ScriptOrigin,
}

pub fn scripts_for_hook(&self, hook: &HookPoint) -> Vec<ScriptInfo> {
    self.registry.scripts_for(hook.clone())
        .iter()
        .map(|s| ScriptInfo {
            name: s.manifest.name.clone(),
            manifest_path: s.manifest_path.clone(),
            status: s.manifest.status.clone(),
            origin: s.manifest.origin.clone(),
        })
        .collect()
}
```

- [ ] **Step 5: Verify and commit**

Run: `cargo build && cargo test --workspace`

```bash
git add src/script_bridge.rs crates/glass_scripting/src/lib.rs
git commit -m "feat(scripting): add per-run script tracking in ScriptBridge"
```

---

## Task 2: Wire Lifecycle on Run End

**Files:**
- Modify: `src/script_bridge.rs`
- Modify: `src/main.rs`

After `on_run_end()` returns a `FeedbackResult`, the bridge should promote/reject scripts based on the regression result.

- [ ] **Step 1: Add on_feedback_run_end method to ScriptBridge**

```rust
/// Process lifecycle updates for all scripts after an orchestrator run.
///
/// - If regression detected: reject all provisional scripts
/// - If improved/neutral: promote provisional scripts that fired
/// - Record failures for scripts that errored 3+ times
/// - Increment stale_runs for scripts that didn't fire
pub fn on_feedback_run_end(&mut self, regressed: bool) {
    let all_scripts = self.system.scripts_for_all();

    for script in &all_scripts {
        // Skip user-written scripts — they don't go through lifecycle
        if script.origin == glass_scripting::ScriptOrigin::User {
            continue;
        }

        let name = &script.name;
        let path = &script.manifest_path;

        // Check if script errored enough times for auto-reject
        if let Some(&error_count) = self.scripts_errored.get(name) {
            if error_count >= 3 {
                tracing::info!("[lifecycle] auto-rejecting script '{name}' ({error_count} errors)");
                let _ = glass_scripting::lifecycle::record_failure(path);
                continue;
            }
        }

        if script.status == glass_scripting::ScriptStatus::Provisional {
            if regressed {
                // Regression: reject all provisional scripts
                tracing::info!("[lifecycle] rejecting provisional script '{name}' (regression)");
                let _ = glass_scripting::lifecycle::reject_script(path);
            } else if self.scripts_triggered.contains(name) {
                // Improved/neutral and script fired: promote
                tracing::info!("[lifecycle] promoting provisional script '{name}'");
                let _ = glass_scripting::lifecycle::promote_script(path);
            }
            // Provisional script that didn't fire: leave as provisional for next run
        } else if script.status == glass_scripting::ScriptStatus::Confirmed
            || script.status == glass_scripting::ScriptStatus::Stale
        {
            if self.scripts_triggered.contains(name) {
                // Record successful trigger (resets failure count, resets stale)
                let _ = glass_scripting::lifecycle::record_trigger(path);
            } else {
                // Script didn't fire this run — increment staleness
                let _ = glass_scripting::lifecycle::increment_stale(path, 5, 10);
            }
        }
    }

    // Reset tracking for next run
    self.reset_run_tracking();

    // Reload scripts to pick up status changes
    self.reload();
}
```

- [ ] **Step 2: Add scripts_for_all to ScriptSystem**

In `crates/glass_scripting/src/lib.rs`:

```rust
/// Return info for all loaded scripts (for lifecycle processing).
pub fn scripts_for_all(&self) -> Vec<ScriptInfo> {
    self.all_scripts()
        .iter()
        .map(|s| ScriptInfo {
            name: s.manifest.name.clone(),
            manifest_path: s.manifest_path.clone(),
            status: s.manifest.status.clone(),
            origin: s.manifest.origin.clone(),
        })
        .collect()
}
```

- [ ] **Step 3: Call on_feedback_run_end from main.rs**

In `src/main.rs`, find where `on_run_end` result is processed (around line 1861). After the existing logging and before the LLM prompt handling, add:

```rust
// Lifecycle: promote/reject scripts based on regression result
let regressed = matches!(
    result.regression,
    Some(glass_feedback::regression::RegressionResult::Regressed { .. })
);
self.script_bridge.on_feedback_run_end(regressed);
```

Note: Check if `glass_feedback::regression::RegressionResult` is publicly accessible. If it's behind a `pub(crate)` or the `regression` module isn't public, you may need to expose it or check the `regression` field differently.

- [ ] **Step 4: Reset tracking on run start**

In `src/main.rs`, find where `OrchestratorRunStart` hook is fired (in the Ctrl+Shift+O toggle-on path). Before or after the hook call, add:

```rust
self.script_bridge.reset_run_tracking();
```

- [ ] **Step 5: Verify and commit**

Run: `cargo build && cargo test --workspace`

```bash
git add src/script_bridge.rs src/main.rs crates/glass_scripting/src/lib.rs
git commit -m "feat(scripting): wire lifecycle promotion/rejection on run end"
```

---

## Task 3: Break the ConfigReload Loop

**Files:**
- Modify: `src/script_bridge.rs`
- Modify: `src/main.rs`

If a script on the `ConfigReload` hook calls `glass.set_config()`, and `execute_actions` writes to config.toml, the hot-reload watcher fires `ConfigReloaded`, which fires the hook again — infinite loop.

- [ ] **Step 1: Add guard flag**

The `config_reload_guard` field already added in Task 1. Now use it.

In the `on_config_reload` method:

```rust
pub fn on_config_reload(&mut self, ctx: &HookContext) -> Vec<Action> {
    if self.config_reload_guard {
        tracing::debug!("[script] config reload guard active, skipping hook");
        return Vec::new();
    }
    self.run_hook(HookPoint::ConfigReload, ctx, &HookEventData::new())
}
```

- [ ] **Step 2: Set guard during action execution**

In `execute_actions`, before processing `SetConfig`:

```rust
Action::SetConfig { key, value } => {
    self.config_reload_guard = true;
    tracing::info!("[script] set_config({key}, {value:?})");
    // Write to config.toml would go here
    // The guard prevents the resulting ConfigReloaded event from re-triggering scripts
    // Guard auto-clears on next non-config-reload hook invocation
}
```

- [ ] **Step 3: Clear guard on non-config hooks**

At the top of `run_hook`, clear the guard if this isn't a ConfigReload:

```rust
fn run_hook(&mut self, hook: HookPoint, ...) -> Vec<Action> {
    if hook != HookPoint::ConfigReload {
        self.config_reload_guard = false;
    }
    // ... rest of method
}
```

- [ ] **Step 4: Verify and commit**

Run: `cargo build && cargo test --workspace`

```bash
git add src/script_bridge.rs
git commit -m "fix(scripting): break ConfigReload action loop with guard flag"
```

---

## Task 4: Deduplicate Tier 4 Script Generation

**Files:**
- Modify: `src/main.rs` (ScriptGeneration response handler)

If multiple runs generate scripts addressing the same pattern, you get duplicate provisional scripts.

- [ ] **Step 1: Check for name collision before writing**

In the `EphemeralPurpose::ScriptGeneration` handler, after parsing the script name, check if a script with that name already exists:

```rust
Some((name, hooks, source)) => {
    let scripts_dir = std::path::Path::new(&project_root)
        .join(".glass").join("scripts").join("feedback");
    let manifest_path = scripts_dir.join(format!("{name}.toml"));

    // Deduplicate: skip if a script with this name already exists
    if manifest_path.exists() {
        tracing::info!("Tier 4: script '{name}' already exists, skipping");
        return;
    }

    // ... rest of write logic
}
```

- [ ] **Step 2: Add timestamp suffix for generic names**

If the LLM generates a generic name like "fix_stuck", append a short timestamp:

```rust
let name = if scripts_dir.join(format!("{name}.toml")).exists() {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{name}_{ts}")
} else {
    name
};
```

Actually, the simpler approach from Step 1 (skip if exists) is better. If the previous script was rejected, it'll have `status = "archived"` and the loader will skip it, but the file still exists. So check for the file but also check if it's archived:

```rust
if manifest_path.exists() {
    // Check if existing script is archived (effectively deleted)
    if let Ok(existing) = glass_scripting::lifecycle::read_manifest(&manifest_path) {
        if existing.status != glass_scripting::ScriptStatus::Archived {
            tracing::info!("Tier 4: script '{name}' already exists (status: {:?}), skipping", existing.status);
            return;
        }
        // Archived — safe to overwrite with new version
    }
}
```

- [ ] **Step 3: Verify and commit**

Run: `cargo build && cargo test --workspace`

```bash
git add src/main.rs
git commit -m "fix(scripting): deduplicate Tier 4 script generation"
```

---

## Task 5: Handle Persistent LLM Format Failures

**Files:**
- Modify: `src/script_bridge.rs` (or `src/main.rs`)

If the Tier 4 LLM keeps producing unparseable responses, it wastes ephemeral agent calls silently.

- [ ] **Step 1: Add failure counter**

Add to Processor in main.rs:

```rust
script_gen_parse_failures: u32,
```

Initialize to 0.

- [ ] **Step 2: Increment on parse failure, skip after threshold**

In the `ScriptGeneration` handler:

```rust
None => {
    self.script_gen_parse_failures += 1;
    tracing::warn!(
        "Tier 4: could not parse script from LLM response (failure {}/3)",
        self.script_gen_parse_failures
    );
}
```

Before spawning the ephemeral agent:

```rust
if self.script_gen_parse_failures >= 3 {
    tracing::info!("Tier 4: suppressed — {} consecutive parse failures", self.script_gen_parse_failures);
    // Don't spawn the agent. Reset counter on next successful parse.
} else {
    // ... spawn agent
}
```

- [ ] **Step 3: Reset on successful parse**

In the success path:

```rust
Some((name, hooks, source)) => {
    self.script_gen_parse_failures = 0;
    // ... rest of write logic
}
```

- [ ] **Step 4: Verify and commit**

Run: `cargo build && cargo test --workspace`

```bash
git add src/main.rs
git commit -m "fix(scripting): suppress Tier 4 after 3 consecutive parse failures"
```

---

## Summary

| Task | What It Fixes |
|------|--------------|
| 1 | Track which scripts fire/error per run |
| 2 | Promote provisional scripts on improvement, reject on regression, age stale scripts |
| 3 | Prevent ConfigReload → SetConfig → ConfigReload infinite loop |
| 4 | Prevent duplicate Tier 4 scripts from piling up |
| 5 | Suppress Tier 4 agent spawn after repeated parse failures |

After these 5 tasks, the lifecycle is fully closed: scripts are born (Tier 4), validated (run metrics), promoted or rejected (lifecycle), and aged out (staleness). The self-improvement loop is self-correcting.

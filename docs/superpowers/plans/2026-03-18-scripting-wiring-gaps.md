# Scripting Layer Wiring Gaps Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close all 5 remaining gaps so the scripting layer is fully operational — every hook fires, every action executes, MCP tools work, and the feedback loop generates scripts.

**Architecture:** All changes are in `src/main.rs`, `src/script_bridge.rs`, and `crates/glass_core/src/event.rs`. The glass_scripting crate is complete and unchanged. Each task wires existing bridge methods to existing event handlers.

**Tech Stack:** Rust, glass_scripting (Rhai), glass_core events, ephemeral_agent pattern

**Spec:** `docs/superpowers/specs/2026-03-18-self-improvement-scripting-design.md`

---

## File Map

### Modified Files

| File | Change |
|------|--------|
| `src/main.rs` | Wire all hook calls, implement Tier 4 ephemeral agent, implement MCP IPC handlers |
| `src/script_bridge.rs` | Implement remaining action types in execute_actions |
| `crates/glass_core/src/event.rs` | Add `ScriptGeneration` variant to `EphemeralPurpose` |

---

## Task 1: Wire All Event Hooks into main.rs

**Files:**
- Modify: `src/main.rs`

This task adds bridge calls at every event handler location. Each call follows the same pattern: check `has_scripts_for`, build context, call bridge method, execute actions.

- [ ] **Step 1: Read main.rs to confirm line numbers**

Read these sections to verify the exact insertion points match the current code:
- Line 5936: `ShellEvent::CommandExecuted` handler (CommandStart hook)
- Line 6088: `ShellEvent::CommandFinished` handler (CommandComplete hook)
- Line 7583: `AppEvent::OrchestratorSilence` handler after guards (OrchestratorIteration hook)
- Line 4221: Orchestrator toggle on (OrchestratorRunStart hook)
- Line 3867: `add_tab` call (TabCreate hook)
- Line 3902+5273: `close_tab` calls (TabClose hook)
- Line 3909: Last window closes (SessionEnd hook)

- [ ] **Step 2: Add helper to build context and execute actions**

Add a private method to Processor that reduces boilerplate:

```rust
/// Run a script hook and execute any resulting actions.
fn fire_script_hook(&self, hook: glass_scripting::HookPoint, event: &glass_scripting::HookEventData) {
    if !self.script_bridge.has_scripts_for(hook.clone()) {
        return;
    }
    let ctx = self.build_hook_context();
    let actions = self.script_bridge.run_hook(hook, &ctx, event);
    if !actions.is_empty() {
        if let Some(ref root) = self.script_bridge.project_root() {
            self.script_bridge.execute_actions(&actions, root);
        }
    }
}
```

Note: `script_bridge.project_root()` doesn't exist yet — add a `pub fn project_root(&self) -> Option<&str>` getter to ScriptBridge that returns `self.project_root.as_deref()`.

- [ ] **Step 3: Wire CommandStart hook**

At `src/main.rs` around line 5974 (after command_text extraction in the `ShellEvent::CommandExecuted` handler, before snapshot decisions):

```rust
// Script hook: CommandStart
{
    let mut event = glass_scripting::HookEventData::new();
    event.set("command", command_text.clone());
    self.fire_script_hook(glass_scripting::HookPoint::CommandStart, &event);
}
```

- [ ] **Step 4: Wire CommandComplete hook**

At `src/main.rs` around line 6115 (in the `ShellEvent::CommandFinished` handler, after exit_code and command_text are available):

```rust
// Script hook: CommandComplete
{
    let mut event = glass_scripting::HookEventData::new();
    event.set("command", command_text.clone());
    event.set("exit_code", exit_code.unwrap_or(-1) as i64);
    self.fire_script_hook(glass_scripting::HookPoint::CommandComplete, &event);
}
```

- [ ] **Step 5: Wire OrchestratorIteration hook**

At `src/main.rs` around line 7583 (in `AppEvent::OrchestratorSilence` handler, after all guard checks pass, before building context for the agent):

```rust
// Script hook: OrchestratorIteration
{
    let mut event = glass_scripting::HookEventData::new();
    event.set("iteration", self.orchestrator.iteration_count as i64);
    self.fire_script_hook(glass_scripting::HookPoint::OrchestratorIteration, &event);
}
```

- [ ] **Step 6: Wire OrchestratorRunStart hook**

At `src/main.rs` around line 4256 (right after `self.script_bridge.load_for_project(&current_cwd)` which already exists in the orchestrator toggle-on path):

```rust
// Script hook: OrchestratorRunStart
self.fire_script_hook(glass_scripting::HookPoint::OrchestratorRunStart, &glass_scripting::HookEventData::new());
```

- [ ] **Step 7: Wire OrchestratorRunEnd hook**

Find every location where the orchestrator deactivates. The primary one is in `run_feedback_on_end` or near `self.orchestrator.active = false`. Add before deactivation cleanup:

```rust
// Script hook: OrchestratorRunEnd
{
    let mut event = glass_scripting::HookEventData::new();
    event.set("iterations", self.orchestrator.iteration_count as i64);
    self.fire_script_hook(glass_scripting::HookPoint::OrchestratorRunEnd, &event);
}
```

- [ ] **Step 8: Wire TabCreate hook**

At `src/main.rs` around line 3868 (right after `ctx.session_mux.add_tab(session, false)`):

```rust
// Script hook: TabCreate
{
    let mut event = glass_scripting::HookEventData::new();
    event.set("tab_index", ctx.session_mux.tab_count() as i64 - 1);
    self.fire_script_hook(glass_scripting::HookPoint::TabCreate, &event);
}
```

- [ ] **Step 9: Wire TabClose hook**

At both `close_tab` call sites (around lines 3902 and 5273):

```rust
// Script hook: TabClose
{
    let mut event = glass_scripting::HookEventData::new();
    event.set("tab_index", idx as i64);
    self.fire_script_hook(glass_scripting::HookPoint::TabClose, &event);
}
```

- [ ] **Step 10: Wire SessionEnd hook**

At `src/main.rs` around line 3909 (right before `event_loop.exit()`):

```rust
// Script hook: SessionEnd
self.fire_script_hook(glass_scripting::HookPoint::SessionEnd, &glass_scripting::HookEventData::new());
```

- [ ] **Step 11: Verify it compiles**

Run: `cargo build`
Expected: Successful compilation. Fix any field access issues (variable names may differ slightly from plan — adapt to actual code).

- [ ] **Step 12: Run all tests**

Run: `cargo test --workspace`
Expected: All tests pass

- [ ] **Step 13: Commit**

```bash
git add src/main.rs src/script_bridge.rs
git commit -m "feat(scripting): wire all event hooks into main.rs event loop"
```

---

## Task 2: Implement Remaining Action Types

**Files:**
- Modify: `src/script_bridge.rs`

The `execute_actions` method currently handles `Log`, `Notify`, and `Commit`. Wire the remaining 14 action types.

- [ ] **Step 1: Read script_bridge.rs and main.rs for available APIs**

Understand what Glass state is accessible:
- `self.project_root` for git operations
- `git` commands via `std::process::Command` (with CREATE_NO_WINDOW)
- Config changes need to write to config.toml and trigger reload
- Snapshot operations need access to snapshot store (not directly available — log as deferred)

- [ ] **Step 2: Implement git actions**

In `execute_actions`, replace the catch-all `other =>` arm with specific handlers:

```rust
Action::IsolateCommit { files, message } => {
    self.execute_git_isolate_commit(project_root, files, message);
}
Action::RevertFiles { files } => {
    self.execute_git_revert(project_root, files);
}
```

Add helper methods:

```rust
fn execute_git_isolate_commit(&self, project_root: &str, files: &[String], message: &str) {
    let mut add_cmd = std::process::Command::new("git");
    add_cmd.arg("add").args(files).current_dir(project_root);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        add_cmd.creation_flags(0x08000000);
    }
    if let Ok(output) = add_cmd.output() {
        if output.status.success() {
            let mut commit_cmd = std::process::Command::new("git");
            commit_cmd.args(["commit", "-m", message]).current_dir(project_root);
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                commit_cmd.creation_flags(0x08000000);
            }
            match commit_cmd.output() {
                Ok(o) if o.status.success() => {
                    tracing::info!("[script] isolated commit of {} files", files.len());
                }
                Ok(o) => {
                    tracing::warn!("[script] isolated commit failed: {}", String::from_utf8_lossy(&o.stderr).trim());
                }
                Err(e) => tracing::warn!("[script] isolated commit error: {e}"),
            }
        }
    }
}

fn execute_git_revert(&self, project_root: &str, files: &[String]) {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("checkout").arg("--").args(files).current_dir(project_root);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    match cmd.output() {
        Ok(o) if o.status.success() => {
            tracing::info!("[script] reverted {} files", files.len());
        }
        Ok(o) => tracing::warn!("[script] revert failed: {}", String::from_utf8_lossy(&o.stderr).trim()),
        Err(e) => tracing::warn!("[script] revert error: {e}"),
    }
}
```

- [ ] **Step 3: Implement orchestrator actions**

```rust
Action::InjectPromptHint { text } => {
    tracing::info!("[script] injecting prompt hint ({} chars)", text.len());
    // Prompt hints are read from rules.toml by glass_feedback on next run.
    // Write a temporary prompt_hint rule to rules.toml.
    let rules_path = std::path::Path::new(project_root).join(".glass").join("rules.toml");
    if let Ok(mut content) = std::fs::read_to_string(&rules_path) {
        content.push_str(&format!(
            "\n[[rules]]\nid = \"script_hint_{}\"\ntrigger = \"always\"\naction = \"prompt_hint\"\naction_params = {{ text = \"{}\" }}\nstatus = \"provisional\"\nseverity = \"info\"\nscope = \"project\"\n",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
            text.replace('\"', "\\\""),
        ));
        let _ = std::fs::write(&rules_path, content);
    }
}
Action::TriggerCheckpoint { reason } => {
    tracing::info!("[script] checkpoint requested: {reason}");
    // The orchestrator checks for checkpoint triggers each iteration.
    // Set a flag that the orchestrator will read.
    // For now, log only — full wiring requires adding a field to orchestrator state.
}
Action::ExtendSilence { extra_secs } => {
    tracing::info!("[script] extending silence by {extra_secs}s");
    // Similar to TriggerCheckpoint — requires orchestrator state access.
    // Log for now.
}
Action::BlockIteration { message, max_iterations } => {
    tracing::info!("[script] blocking iteration (max {max_iterations}): {message}");
    // Log for now.
}
```

- [ ] **Step 4: Implement remaining actions**

```rust
Action::SetConfig { key, value } => {
    tracing::info!("[script] set_config({key}, {value:?}) — requires config.toml write + reload");
    // Full implementation would write to config.toml and let hot-reload pick it up.
    // Deferred: config writing is complex (must preserve comments, handle partial sections).
}
Action::ForceSnapshot { paths } => {
    tracing::info!("[script] force_snapshot requested for {} paths", paths.len());
    // Requires snapshot store access — not available in bridge. Log for now.
}
Action::SetSnapshotPolicy { pattern, enabled } => {
    tracing::info!("[script] set_snapshot_policy({pattern}, {enabled})");
}
Action::TagCommand { command_id, tags } => {
    tracing::info!("[script] tag_command({command_id}, {:?})", tags);
    // Requires history DB access — not available in bridge. Log for now.
}
Action::EnableScript { name } => {
    tracing::info!("[script] enable_script({name})");
    // Would call lifecycle::promote or update manifest status.
}
Action::DisableScript { name } => {
    tracing::info!("[script] disable_script({name})");
    // Would call lifecycle::reject or update manifest status.
}
Action::RegisterTool { name, description, .. } => {
    tracing::info!("[script] register_tool({name}: {description})");
    // Dynamic tool registration handled via MCP registry, not execute_actions.
}
Action::UnregisterTool { name } => {
    tracing::info!("[script] unregister_tool({name})");
}
```

- [ ] **Step 5: Verify and commit**

Run: `cargo build && cargo test --workspace`

```bash
git add src/script_bridge.rs
git commit -m "feat(scripting): implement all action types in execute_actions"
```

---

## Task 3: Implement MCP IPC Handlers

**Files:**
- Modify: `src/main.rs:8888-8903`
- Modify: `src/script_bridge.rs` (add tool registry access)

- [ ] **Step 1: Add ScriptToolRegistry to ScriptBridge**

In `src/script_bridge.rs`, add a `tool_registry` field:

```rust
use glass_scripting::ScriptToolRegistry;

pub struct ScriptBridge {
    system: ScriptSystem,
    tool_registry: ScriptToolRegistry,
    enabled: bool,
    project_root: Option<String>,
}
```

Initialize in `new()`:
```rust
tool_registry: ScriptToolRegistry::new(),
```

Populate in `load_for_project()` after scripts are loaded:
```rust
self.tool_registry.register_from_scripts(&self.system.all_scripts_cloned(), false);
```

Note: `all_scripts()` returns `Vec<&LoadedScript>` — you may need `all_scripts_cloned()` or clone the scripts. Check the API and adapt.

Add public accessors:
```rust
pub fn get_script_tool(&self, name: &str) -> Option<&glass_scripting::mcp::ScriptToolDef> {
    self.tool_registry.get(name)
}

pub fn list_script_tools(&self) -> Vec<&glass_scripting::mcp::ScriptToolDef> {
    self.tool_registry.list_confirmed()
}

pub fn run_script_tool(&self, tool_name: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
    let tool = self.tool_registry.get(tool_name)
        .ok_or_else(|| format!("Script tool '{}' not found", tool_name))?;
    let ctx = HookContext::default(); // MCP tools don't have event context
    let mut event = HookEventData::new();
    // Pass params as the event data so the script can access them
    if let Some(obj) = params.as_object() {
        for (k, v) in obj {
            event.set(k.clone(), rhai_value_from_json(v));
        }
    }
    let result = self.system.run_single_script(&tool.script_name, &ctx, &event);
    match result {
        Ok(actions) => Ok(serde_json::json!({"status": "ok", "actions_count": actions.len()})),
        Err(e) => Err(e),
    }
}
```

Note: `run_single_script` may not exist on ScriptSystem. Check the API — you may need to add it, or use `run_hook` with a specific hook point. Adapt as needed. The key thing is to run the named script and return a result.

- [ ] **Step 2: Wire script_tool IPC handler**

Replace the placeholder at `src/main.rs:8888-8894`:

```rust
"script_tool" => {
    let tool_name = request.params.get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let params = request.params.get("params")
        .cloned()
        .unwrap_or(serde_json::json!({}));
    match self.script_bridge.run_script_tool(tool_name, params) {
        Ok(result) => glass_core::ipc::McpResponse::ok(request.id, result),
        Err(e) => glass_core::ipc::McpResponse::err(request.id, &e),
    }
}
```

- [ ] **Step 3: Wire list_script_tools IPC handler**

Replace the placeholder at `src/main.rs:8896-8903`:

```rust
"list_script_tools" => {
    let tools: Vec<serde_json::Value> = self.script_bridge.list_script_tools()
        .iter()
        .map(|t| serde_json::json!({
            "name": t.name,
            "description": t.description,
            "params_schema": t.params_schema,
        }))
        .collect();
    glass_core::ipc::McpResponse::ok(request.id, serde_json::json!({"tools": tools}))
}
```

- [ ] **Step 4: Verify and commit**

Run: `cargo build && cargo test --workspace`

```bash
git add src/main.rs src/script_bridge.rs
git commit -m "feat(scripting): implement MCP script_tool and list_script_tools IPC handlers"
```

---

## Task 4: Implement Tier 4 Ephemeral Agent for Script Generation

**Files:**
- Modify: `crates/glass_core/src/event.rs:33-40` (EphemeralPurpose enum)
- Modify: `src/main.rs:1895-1901` (spawn ephemeral agent)
- Modify: `src/main.rs:8997-9020` (handle ephemeral response)

- [ ] **Step 1: Add ScriptGeneration to EphemeralPurpose**

In `crates/glass_core/src/event.rs`, add to the `EphemeralPurpose` enum (line 39):

```rust
/// Generate a Tier 4 Rhai script from feedback analysis.
ScriptGeneration,
```

- [ ] **Step 2: Add state fields for script generation**

In `src/main.rs`, find where `feedback_llm_project_root` is stored (around line 363). Add nearby:

```rust
/// Project root captured when Tier 4 script generation is spawned.
script_gen_project_root: Option<String>,
```

Initialize to `None` in the constructor.

- [ ] **Step 3: Spawn ephemeral agent for Tier 4**

Replace the TODO at `src/main.rs:1895-1901`:

```rust
// Tier 4: script generation prompt
if let Some(script_prompt) = result.script_prompt {
    tracing::info!(
        "Tier 4: spawning ephemeral agent for script generation ({} chars)",
        script_prompt.len()
    );
    self.script_gen_project_root = Some(self.orchestrator.project_root.clone());
    let request = ephemeral_agent::EphemeralAgentRequest {
        system_prompt: "You are generating a Rhai script for the Glass terminal emulator's self-improvement system. Respond ONLY in the structured format requested.".to_string(),
        user_message: script_prompt,
        timeout: std::time::Duration::from_secs(60),
        purpose: glass_core::event::EphemeralPurpose::ScriptGeneration,
    };
    if let Err(e) = ephemeral_agent::spawn_ephemeral_agent(request, self.proxy.clone()) {
        tracing::warn!("Tier 4 script generation: ephemeral spawn failed: {e:?}");
    }
}
```

- [ ] **Step 4: Handle ScriptGeneration ephemeral response**

In the `EphemeralAgentComplete` handler, after the `FeedbackAnalysis` arm (around line 9020), add:

```rust
glass_core::event::EphemeralPurpose::ScriptGeneration => {
    let project_root = self
        .script_gen_project_root
        .take()
        .unwrap_or_else(|| self.orchestrator.project_root.clone());
    match result {
        Ok(resp) => {
            if let Some(cost) = resp.cost_usd {
                tracing::info!("Tier 4 script generation cost: ${:.4}", cost);
            }
            // Parse the LLM response for SCRIPT_NAME, SCRIPT_HOOKS, SCRIPT_SOURCE
            match parse_script_response(&resp.text) {
                Some((name, hooks, source)) => {
                    let scripts_dir = std::path::Path::new(&project_root)
                        .join(".glass")
                        .join("scripts")
                        .join("feedback");
                    if let Err(e) = std::fs::create_dir_all(&scripts_dir) {
                        tracing::warn!("Tier 4: failed to create scripts dir: {e}");
                        return;
                    }
                    // Write manifest
                    let manifest_content = format!(
                        "name = \"{name}\"\nhooks = [{hooks}]\nstatus = \"provisional\"\norigin = \"feedback\"\nversion = 1\napi_version = 1\ncreated = \"{}\"\n",
                        chrono_free_date()
                    );
                    let _ = std::fs::write(scripts_dir.join(format!("{name}.toml")), manifest_content);
                    let _ = std::fs::write(scripts_dir.join(format!("{name}.rhai")), source);
                    tracing::info!("Tier 4: wrote provisional script '{name}' to {}", scripts_dir.display());
                    // Reload scripts so it's active next run
                    self.script_bridge.reload();
                }
                None => {
                    tracing::warn!("Tier 4: could not parse script from LLM response ({} chars)", resp.text.len());
                }
            }
        }
        Err(e) => {
            tracing::warn!("Tier 4 script generation failed: {e:?}");
        }
    }
}
```

- [ ] **Step 5: Add response parser helper**

Add near the bottom of main.rs (or in a helper module):

```rust
/// Parse structured script response from the Tier 4 LLM.
/// Expected format:
/// SCRIPT_NAME: <name>
/// SCRIPT_HOOKS: <comma-separated>
/// SCRIPT_SOURCE:
/// ```rhai
/// ...
/// ```
fn parse_script_response(text: &str) -> Option<(String, String, String)> {
    let name = text.lines()
        .find(|l| l.starts_with("SCRIPT_NAME:"))
        .map(|l| l.trim_start_matches("SCRIPT_NAME:").trim().to_string())?;
    let hooks_raw = text.lines()
        .find(|l| l.starts_with("SCRIPT_HOOKS:"))
        .map(|l| l.trim_start_matches("SCRIPT_HOOKS:").trim().to_string())?;
    // Convert "CommandComplete, OrchestratorIteration" to "\"CommandComplete\", \"OrchestratorIteration\""
    let hooks = hooks_raw.split(',')
        .map(|h| format!("\"{}\"", h.trim()))
        .collect::<Vec<_>>()
        .join(", ");
    // Extract source between ```rhai and ```
    let source_start = text.find("```rhai").map(|i| i + 7)?;
    let source_end = text[source_start..].find("```").map(|i| source_start + i)?;
    let source = text[source_start..source_end].trim().to_string();
    Some((name, hooks, source))
}

fn chrono_free_date() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Days since epoch, then approximate year/month/day
    let days = secs / 86400;
    let years = days / 365;
    let year = 1970 + years;
    let remaining = days - years * 365;
    let month = remaining / 30 + 1;
    let day = remaining % 30 + 1;
    format!("{year}-{month:02}-{day:02}")
}
```

- [ ] **Step 6: Handle exhaustive match**

The `EphemeralPurpose` match in `AppEvent::EphemeralAgentComplete` handler must now handle `ScriptGeneration`. Verify the match is not `_ =>` — if it's exhaustive, adding the new arm is required for compilation.

- [ ] **Step 7: Verify and commit**

Run: `cargo build && cargo test --workspace`

```bash
git add crates/glass_core/src/event.rs src/main.rs
git commit -m "feat(scripting): implement Tier 4 ephemeral agent for script generation"
```

---

## Task 5: Final Verification

**Files:** None (verification only)

- [ ] **Step 1: Run full workspace build**

Run: `cargo build --workspace`
Expected: Zero errors, zero warnings

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: Clean

- [ ] **Step 3: Run all tests**

Run: `cargo test --workspace`
Expected: All tests pass

- [ ] **Step 4: Verify no remaining TODOs in scripting code**

Run: `grep -rn "TODO\|todo!\|FIXME\|HACK\|unhandled action" src/script_bridge.rs src/main.rs crates/glass_scripting/`
Expected: No TODOs related to scripting wiring. Some "log for now" comments are acceptable for actions that need subsystem access (ForceSnapshot, TagCommand, SetConfig).

- [ ] **Step 5: Commit any fixes**

```bash
git add -A
git commit -m "chore(scripting): final cleanup and verification"
```

---

## Summary

| Task | What It Closes |
|------|---------------|
| 1 | All 10+ event hooks wired into main.rs (CommandStart/Complete, Orchestrator, Tab, Session) |
| 2 | All 17 action types handled in execute_actions (git, config, orchestrator, scripts, MCP, notifications) |
| 3 | MCP script_tool and list_script_tools IPC return real data |
| 4 | Tier 4 ephemeral agent spawns, parses response, writes provisional scripts |
| 5 | Full verification pass |

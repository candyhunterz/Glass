// ---------------------------------------------------------------------------
// ScriptBridge — connects glass_scripting to the Glass main event loop
// ---------------------------------------------------------------------------
//
// Many hook methods are defined but not yet wired into the event loop --
// they will be connected incrementally in future tasks.

use std::collections::{HashMap, HashSet};

use glass_core::config::GlassConfig;
use glass_scripting::{
    Action, HookContext, HookEventData, HookPoint, LogLevel, ScriptSystem, ScriptToolRegistry,
};

/// Bridge between the Glass event loop and the scripting engine.
///
/// Owns a [`ScriptSystem`] and provides typed methods for each hook point.
/// The main event loop calls these methods at the appropriate moments; the
/// bridge builds the [`HookContext`] / [`HookEventData`] and delegates to
/// `ScriptSystem::run_hook`, then returns the resulting [`Action`] list for
/// the caller to execute.
pub struct ScriptBridge {
    system: ScriptSystem,
    tool_registry: ScriptToolRegistry,
    enabled: bool,
    project_root: Option<String>,
    /// Guard to break ConfigReload -> SetConfig -> ConfigReloaded -> ConfigReload loops.
    /// Set to `true` when a `SetConfig` action is executed; while true, the
    /// `on_config_reload` hook is suppressed. Cleared when any other hook fires.
    config_reload_guard: bool,
    /// Names of scripts that fired successfully (no errors) during the current run.
    scripts_triggered: HashSet<String>,
    /// Script name -> error count during the current run.
    scripts_errored: HashMap<String, u32>,
}

impl ScriptBridge {
    /// Create a new `ScriptBridge` from the application config.
    ///
    /// Reads the `[scripting]` section to determine whether scripting is
    /// enabled and to build the sandbox config. No scripts are loaded yet --
    /// call [`load_for_project`] once the project root is known.
    pub fn new(config: &GlassConfig) -> Self {
        let (enabled, sandbox) = match config.scripting.as_ref() {
            Some(section) => (
                section.enabled,
                glass_scripting::SandboxConfig::from_config(section),
            ),
            None => (false, glass_scripting::SandboxConfig::default()),
        };
        Self {
            system: ScriptSystem::new(sandbox),
            tool_registry: ScriptToolRegistry::new(),
            enabled,
            project_root: None,
            config_reload_guard: false,
            scripts_triggered: HashSet::new(),
            scripts_errored: HashMap::new(),
        }
    }

    /// Load scripts for the given project root directory.
    ///
    /// Stores the project root for later [`reload`] calls.
    pub fn load_for_project(&mut self, project_root: &str) {
        self.project_root = Some(project_root.to_string());
        if !self.enabled {
            tracing::debug!("ScriptBridge: scripting disabled, skipping script load");
            return;
        }
        let errors = self.system.load_all(project_root);
        if errors.is_empty() {
            let count = self.system.all_scripts().len();
            tracing::info!("ScriptBridge: loaded {count} scripts for {project_root}");
        } else {
            for (name, err) in &errors {
                tracing::warn!("ScriptBridge: compile error in script '{name}': {err}");
            }
        }

        // Rebuild the MCP tool registry from loaded scripts.
        self.tool_registry = ScriptToolRegistry::new();
        let all_scripts: Vec<glass_scripting::LoadedScript> = self
            .system
            .all_scripts()
            .into_iter()
            .cloned()
            .collect();
        self.tool_registry
            .register_from_scripts(&all_scripts, false);
        let tool_count = self.tool_registry.list_confirmed().len();
        if tool_count > 0 {
            tracing::info!("ScriptBridge: registered {tool_count} MCP script tool(s)");
        }
    }

    /// Reload scripts from the stored project root.
    pub fn reload(&mut self) {
        if let Some(ref root) = self.project_root.clone() {
            self.load_for_project(root);
        }
    }

    /// Update the enabled flag from a new config (e.g. after hot-reload).
    pub fn update_config(&mut self, config: &GlassConfig) {
        let was_enabled = self.enabled;
        self.enabled = config
            .scripting
            .as_ref()
            .map(|s| s.enabled)
            .unwrap_or(false);
        if self.enabled && !was_enabled {
            tracing::info!("ScriptBridge: scripting enabled via config reload");
            self.reload();
        } else if !self.enabled && was_enabled {
            tracing::info!("ScriptBridge: scripting disabled via config reload");
        }
    }

    /// Check whether any scripts are registered for the given hook.
    pub fn has_scripts_for(&self, hook: HookPoint) -> bool {
        self.enabled && self.system.has_scripts_for(hook)
    }

    /// Return the stored project root, if any.
    pub fn project_root(&self) -> Option<&str> {
        self.project_root.as_deref()
    }

    /// Reset per-run script tracking counters.
    ///
    /// Call this at the start of each orchestrator run so that lifecycle
    /// decisions are based only on the current run's data.
    pub fn reset_run_tracking(&mut self) {
        self.scripts_triggered.clear();
        self.scripts_errored.clear();
    }

    // -------------------------------------------------------------------
    // MCP script tool methods
    // -------------------------------------------------------------------

    /// Return all registered script tool definitions as JSON values.
    pub fn list_script_tools(&self) -> Vec<serde_json::Value> {
        self.tool_registry
            .list_confirmed()
            .iter()
            .map(|def| {
                serde_json::json!({
                    "name": def.name,
                    "description": def.description,
                    "params_schema": def.params_schema,
                    "script_name": def.script_name,
                })
            })
            .collect()
    }

    /// Look up and run a script-backed MCP tool by name.
    ///
    /// The `params` JSON object is flattened into the `event` map so the
    /// script can access individual parameters via `event.param_name`.
    pub fn run_script_tool(
        &self,
        name: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let tool_def = self
            .tool_registry
            .get(name)
            .ok_or_else(|| format!("Unknown script tool: {name}"))?;

        // Build a HookContext with whatever we have.
        let ctx = HookContext {
            cwd: self
                .project_root
                .clone()
                .unwrap_or_else(|| ".".to_string()),
            ..Default::default()
        };

        // Build event data from the JSON params so the script can read them.
        let mut event_data = HookEventData::new();
        event_data.set("tool_name", name.to_string());
        if let serde_json::Value::Object(map) = &params {
            for (key, value) in map {
                set_json_field(&mut event_data, key, value);
            }
        }

        // Run the tool's backing script via the engine.
        let script_name = &tool_def.script_name;
        match self
            .system
            .run_hook(HookPoint::McpRequest, &ctx, &event_data)
        {
            ref result if !result.errors.is_empty() => {
                let errs: Vec<String> = result
                    .errors
                    .iter()
                    .map(|(n, e)| format!("{n}: {e}"))
                    .collect();
                Err(format!(
                    "Script tool '{script_name}' error: {}",
                    errs.join("; ")
                ))
            }
            ref result => {
                // Convert actions to a JSON summary for the MCP response.
                let action_summaries: Vec<serde_json::Value> = result
                    .actions
                    .iter()
                    .map(action_to_json)
                    .collect();
                Ok(serde_json::json!({
                    "tool": name,
                    "actions": action_summaries,
                    "action_count": result.actions.len(),
                }))
            }
        }
    }

    // -------------------------------------------------------------------
    // Per-hook convenience methods
    // -------------------------------------------------------------------

    /// Fire when a command completes (exit code known).
    pub fn on_command_complete(
        &mut self,
        ctx: &HookContext,
        command: &str,
        exit_code: Option<i32>,
        duration_ms: i64,
    ) -> Vec<Action> {
        let mut event = HookEventData::new();
        event.set("command", command.to_string());
        event.set("exit_code", exit_code.unwrap_or(-1) as i64);
        event.set("duration_ms", duration_ms);
        self.run_hook(HookPoint::CommandComplete, ctx, &event)
    }

    /// Fire when a command starts executing.
    pub fn on_command_start(&mut self, ctx: &HookContext, command: &str) -> Vec<Action> {
        let mut event = HookEventData::new();
        event.set("command", command.to_string());
        self.run_hook(HookPoint::CommandStart, ctx, &event)
    }

    /// Fire on each orchestrator iteration.
    pub fn on_orchestrator_iteration(&mut self, ctx: &HookContext, iteration: u32) -> Vec<Action> {
        let mut event = HookEventData::new();
        event.set("iteration", iteration as i64);
        self.run_hook(HookPoint::OrchestratorIteration, ctx, &event)
    }

    /// Fire when an orchestrator run starts.
    pub fn on_orchestrator_run_start(&mut self, ctx: &HookContext) -> Vec<Action> {
        self.run_hook(HookPoint::OrchestratorRunStart, ctx, &HookEventData::new())
    }

    /// Fire when an orchestrator run ends.
    pub fn on_orchestrator_run_end(&mut self, ctx: &HookContext, iterations: u32) -> Vec<Action> {
        let mut event = HookEventData::new();
        event.set("iterations", iterations as i64);
        self.run_hook(HookPoint::OrchestratorRunEnd, ctx, &event)
    }

    /// Fire when the config is reloaded.
    ///
    /// Suppressed when `config_reload_guard` is set (i.e. when a script's
    /// `SetConfig` action just wrote to config.toml and the hot-reload
    /// callback fired). This prevents infinite loops.
    pub fn on_config_reload(&mut self, ctx: &HookContext) -> Vec<Action> {
        if self.config_reload_guard {
            tracing::debug!("ScriptBridge: suppressing ConfigReload hook (guard active)");
            return Vec::new();
        }
        self.run_hook(HookPoint::ConfigReload, ctx, &HookEventData::new())
    }

    /// Fire when a session starts.
    pub fn on_session_start(&mut self, ctx: &HookContext) -> Vec<Action> {
        self.run_hook(HookPoint::SessionStart, ctx, &HookEventData::new())
    }

    /// Fire when a session ends.
    pub fn on_session_end(&mut self, ctx: &HookContext) -> Vec<Action> {
        self.run_hook(HookPoint::SessionEnd, ctx, &HookEventData::new())
    }

    /// Fire when a tab is created.
    pub fn on_tab_create(&mut self, ctx: &HookContext, tab_index: usize) -> Vec<Action> {
        let mut event = HookEventData::new();
        event.set("tab_index", tab_index as i64);
        self.run_hook(HookPoint::TabCreate, ctx, &event)
    }

    /// Fire when a tab is closed.
    pub fn on_tab_close(&mut self, ctx: &HookContext, tab_index: usize) -> Vec<Action> {
        let mut event = HookEventData::new();
        event.set("tab_index", tab_index as i64);
        self.run_hook(HookPoint::TabClose, ctx, &event)
    }

    /// Fire before a snapshot is taken. Returns `true` if the snapshot
    /// should proceed, `false` if any script vetoed it.
    ///
    /// Note: uses `system.run_hook` directly (not `self.run_hook`) because
    /// it needs access to `filter_result`, but still tracks errors/triggers.
    pub fn on_snapshot_before(&mut self, ctx: &HookContext, command: &str) -> bool {
        if !self.has_scripts_for(HookPoint::SnapshotBefore) {
            return true;
        }
        let mut event = HookEventData::new();
        event.set("command", command.to_string());

        let script_names = self.system.scripts_for_hook(HookPoint::SnapshotBefore);
        let result = self.system.run_hook(HookPoint::SnapshotBefore, ctx, &event);

        // Track errors and triggers the same way run_hook does.
        let errored_names: HashSet<&str> = result
            .errors
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        for (name, err) in &result.errors {
            tracing::warn!("ScriptBridge: script '{name}' error on SnapshotBefore: {err}");
            *self.scripts_errored.entry(name.clone()).or_insert(0) += 1;
        }
        for name in &script_names {
            if !errored_names.contains(name.as_str()) {
                self.scripts_triggered.insert(name.clone());
            }
        }

        // filter_result is Some(true/false) for SnapshotBefore, default true
        result.filter_result.unwrap_or(true)
    }

    // -------------------------------------------------------------------
    // Action execution
    // -------------------------------------------------------------------

    /// Execute a list of actions produced by scripts.
    ///
    /// Handles all [`Action`] variants: `Log`, `Notify`, `Commit`,
    /// `IsolateCommit`, `RevertFiles` run git commands directly; orchestrator,
    /// config, script-management, and MCP actions are logged with context
    /// (full subsystem wiring is deferred to later phases).
    pub fn execute_actions(&mut self, actions: &[Action], project_root: &str) {
        for action in actions {
            match action {
                Action::Log { level, message } => match level {
                    LogLevel::Debug => tracing::debug!("[script] {}", message),
                    LogLevel::Info => tracing::info!("[script] {}", message),
                    LogLevel::Warn => tracing::warn!("[script] {}", message),
                    LogLevel::Error => tracing::error!("[script] {}", message),
                },
                Action::Notify { message, title } => {
                    let prefix = title
                        .as_ref()
                        .map(|t| format!("[{t}] "))
                        .unwrap_or_default();
                    tracing::info!("[script notify] {prefix}{message}");
                }
                Action::Commit { message } => {
                    self.execute_git_commit(project_root, message);
                }

                // -- Git actions --
                Action::IsolateCommit { message, files } => {
                    self.execute_git_isolate_commit(project_root, files, message);
                }
                Action::RevertFiles { paths } => {
                    self.execute_git_revert(project_root, paths);
                }

                // -- Orchestrator actions (log with context) --
                Action::InjectPromptHint { hint } => {
                    tracing::info!("[script] inject prompt hint: {hint}");
                }
                Action::TriggerCheckpoint { reason } => {
                    let reason_str = reason.as_deref().unwrap_or("(no reason)");
                    tracing::info!("[script] trigger checkpoint: {reason_str}");
                }
                Action::ExtendSilence { duration_ms } => {
                    tracing::info!("[script] extend silence by {duration_ms}ms");
                }
                Action::BlockIteration { reason } => {
                    let reason_str = reason.as_deref().unwrap_or("(no reason)");
                    tracing::info!("[script] block iteration: {reason_str}");
                }

                // -- Config/storage actions (log -- need subsystem access) --
                Action::SetConfig { key, value } => {
                    tracing::info!("[script] set config {key} = {value:?}");
                    // Arm the guard so the resulting ConfigReloaded event
                    // does not re-trigger scripts (breaking the loop).
                    self.config_reload_guard = true;
                }
                Action::ForceSnapshot { paths } => {
                    tracing::info!("[script] force snapshot for {} path(s)", paths.len());
                }
                Action::SetSnapshotPolicy { pattern, policy } => {
                    tracing::info!("[script] set snapshot policy: pattern={pattern} policy={policy}");
                }
                Action::TagCommand { block_id, tag } => {
                    let id_str = block_id.as_deref().unwrap_or("current");
                    tracing::info!("[script] tag command {id_str}: {tag}");
                }

                // -- Script management actions --
                Action::EnableScript { name } => {
                    tracing::info!("[script] enable script: {name}");
                }
                Action::DisableScript { name } => {
                    tracing::info!("[script] disable script: {name}");
                }

                // -- MCP actions --
                Action::RegisterTool { name, description } => {
                    let desc = description.as_deref().unwrap_or("(no description)");
                    tracing::info!("[script] register MCP tool: {name} - {desc}");
                }
                Action::UnregisterTool { name } => {
                    tracing::info!("[script] unregister MCP tool: {name}");
                }
            }
        }
    }

    // -------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------

    /// Run a hook and return the resulting actions. Returns empty if
    /// scripting is disabled or an error occurs.
    ///
    /// Also updates per-run tracking: scripts that execute without errors
    /// are recorded in `scripts_triggered`; scripts that error are tallied
    /// in `scripts_errored`.
    pub(crate) fn run_hook(
        &mut self,
        hook: HookPoint,
        context: &HookContext,
        event_data: &HookEventData,
    ) -> Vec<Action> {
        if !self.enabled {
            return Vec::new();
        }
        // Clear the config reload guard when any non-ConfigReload hook fires.
        // The guard only needs to survive long enough to suppress the single
        // ConfigReloaded event triggered by the SetConfig action.
        if hook != HookPoint::ConfigReload {
            self.config_reload_guard = false;
        }

        // Collect script names that will be attempted for this hook *before*
        // running them, so we can attribute successes vs errors.
        let script_names = self.system.scripts_for_hook(hook.clone());

        let result = self.system.run_hook(hook.clone(), context, event_data);

        // Build a set of errored script names from this invocation.
        let errored_names: HashSet<&str> = result
            .errors
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();

        for (name, err) in &result.errors {
            tracing::warn!("ScriptBridge: script '{name}' error on {hook:?}: {err}");
            *self.scripts_errored.entry(name.clone()).or_insert(0) += 1;
        }

        // Scripts that were attempted and did NOT error are considered triggered.
        for name in &script_names {
            if !errored_names.contains(name.as_str()) {
                self.scripts_triggered.insert(name.clone());
            }
        }

        result.actions
    }

    /// Execute a git commit in the project directory.
    fn execute_git_commit(&self, project_root: &str, message: &str) {
        let mut cmd = std::process::Command::new("git");
        cmd.args(["commit", "-am", message])
            .current_dir(project_root);

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        match cmd.output() {
            Ok(output) => {
                if output.status.success() {
                    tracing::info!(
                        "[script] git commit succeeded: {}",
                        String::from_utf8_lossy(&output.stdout).trim()
                    );
                } else {
                    tracing::warn!(
                        "[script] git commit failed: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    );
                }
            }
            Err(e) => {
                tracing::warn!("[script] failed to run git commit: {e}");
            }
        }
    }

    /// Stage specific files and commit them in the project directory.
    fn execute_git_isolate_commit(&self, project_root: &str, files: &[String], message: &str) {
        if files.is_empty() {
            tracing::warn!("[script] isolate commit with no files, skipping");
            return;
        }

        // Stage the specified files
        let mut add_cmd = std::process::Command::new("git");
        add_cmd.arg("add").args(files).current_dir(project_root);

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            add_cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        match add_cmd.output() {
            Ok(output) if !output.status.success() => {
                tracing::warn!(
                    "[script] git add failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
                return;
            }
            Err(e) => {
                tracing::warn!("[script] failed to run git add: {e}");
                return;
            }
            _ => {}
        }

        // Commit the staged files
        let mut commit_cmd = std::process::Command::new("git");
        commit_cmd
            .args(["commit", "-m", message])
            .current_dir(project_root);

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            commit_cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        match commit_cmd.output() {
            Ok(output) => {
                if output.status.success() {
                    tracing::info!(
                        "[script] isolate commit succeeded ({} file(s)): {}",
                        files.len(),
                        String::from_utf8_lossy(&output.stdout).trim()
                    );
                } else {
                    tracing::warn!(
                        "[script] isolate commit failed: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    );
                }
            }
            Err(e) => {
                tracing::warn!("[script] failed to run git commit: {e}");
            }
        }
    }

    /// Revert specified files using `git checkout`.
    fn execute_git_revert(&self, project_root: &str, paths: &[String]) {
        if paths.is_empty() {
            tracing::warn!("[script] revert with no paths, skipping");
            return;
        }

        let mut cmd = std::process::Command::new("git");
        cmd.args(["checkout", "--"]).args(paths).current_dir(project_root);

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        match cmd.output() {
            Ok(output) => {
                if output.status.success() {
                    tracing::info!("[script] reverted {} file(s)", paths.len());
                } else {
                    tracing::warn!(
                        "[script] git revert failed: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    );
                }
            }
            Err(e) => {
                tracing::warn!("[script] failed to run git checkout: {e}");
            }
        }
    }
}

/// Set a JSON value as a field on `HookEventData`.
///
/// Primitive types (string, int, float, bool) are set natively so scripts
/// can use them directly. Complex types (arrays, objects, null) are
/// serialized to a JSON string.
fn set_json_field(event_data: &mut HookEventData, key: &str, value: &serde_json::Value) {
    match value {
        serde_json::Value::String(s) => event_data.set(key, s.clone()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                event_data.set(key, i);
            } else if let Some(f) = n.as_f64() {
                event_data.set(key, f);
            } else {
                event_data.set(key, n.to_string());
            }
        }
        serde_json::Value::Bool(b) => event_data.set(key, *b),
        // Null, arrays, and objects are serialized as JSON strings.
        _ => event_data.set(key, value.to_string()),
    }
}

/// Convert a script [`Action`] to a JSON value for MCP responses.
fn action_to_json(action: &Action) -> serde_json::Value {
    match action {
        Action::Log { level, message } => {
            serde_json::json!({"type": "log", "level": format!("{level:?}").to_lowercase(), "message": message})
        }
        Action::Notify { message, title } => {
            serde_json::json!({"type": "notify", "message": message, "title": title})
        }
        Action::Commit { message } => {
            serde_json::json!({"type": "commit", "message": message})
        }
        Action::InjectPromptHint { hint } => {
            serde_json::json!({"type": "inject_prompt_hint", "hint": hint})
        }
        Action::SetConfig { key, value } => {
            serde_json::json!({"type": "set_config", "key": key, "value": format!("{value:?}")})
        }
        _ => {
            serde_json::json!({"type": format!("{action:?}").split('{').next().unwrap_or("unknown").trim()})
        }
    }
}

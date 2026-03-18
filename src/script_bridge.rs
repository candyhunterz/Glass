// ---------------------------------------------------------------------------
// ScriptBridge — connects glass_scripting to the Glass main event loop
// ---------------------------------------------------------------------------
//
// Many hook methods are defined but not yet wired into the event loop --
// they will be connected incrementally in future tasks.

use glass_core::config::GlassConfig;
use glass_scripting::{Action, HookContext, HookEventData, HookPoint, LogLevel, ScriptSystem};

/// Bridge between the Glass event loop and the scripting engine.
///
/// Owns a [`ScriptSystem`] and provides typed methods for each hook point.
/// The main event loop calls these methods at the appropriate moments; the
/// bridge builds the [`HookContext`] / [`HookEventData`] and delegates to
/// `ScriptSystem::run_hook`, then returns the resulting [`Action`] list for
/// the caller to execute.
pub struct ScriptBridge {
    system: ScriptSystem,
    enabled: bool,
    project_root: Option<String>,
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
            enabled,
            project_root: None,
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

    // -------------------------------------------------------------------
    // Per-hook convenience methods
    // -------------------------------------------------------------------

    /// Fire when a command completes (exit code known).
    pub fn on_command_complete(
        &self,
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
    pub fn on_command_start(&self, ctx: &HookContext, command: &str) -> Vec<Action> {
        let mut event = HookEventData::new();
        event.set("command", command.to_string());
        self.run_hook(HookPoint::CommandStart, ctx, &event)
    }

    /// Fire on each orchestrator iteration.
    pub fn on_orchestrator_iteration(&self, ctx: &HookContext, iteration: u32) -> Vec<Action> {
        let mut event = HookEventData::new();
        event.set("iteration", iteration as i64);
        self.run_hook(HookPoint::OrchestratorIteration, ctx, &event)
    }

    /// Fire when an orchestrator run starts.
    pub fn on_orchestrator_run_start(&self, ctx: &HookContext) -> Vec<Action> {
        self.run_hook(HookPoint::OrchestratorRunStart, ctx, &HookEventData::new())
    }

    /// Fire when an orchestrator run ends.
    pub fn on_orchestrator_run_end(&self, ctx: &HookContext, iterations: u32) -> Vec<Action> {
        let mut event = HookEventData::new();
        event.set("iterations", iterations as i64);
        self.run_hook(HookPoint::OrchestratorRunEnd, ctx, &event)
    }

    /// Fire when the config is reloaded.
    pub fn on_config_reload(&self, ctx: &HookContext) -> Vec<Action> {
        self.run_hook(HookPoint::ConfigReload, ctx, &HookEventData::new())
    }

    /// Fire when a session starts.
    pub fn on_session_start(&self, ctx: &HookContext) -> Vec<Action> {
        self.run_hook(HookPoint::SessionStart, ctx, &HookEventData::new())
    }

    /// Fire when a session ends.
    pub fn on_session_end(&self, ctx: &HookContext) -> Vec<Action> {
        self.run_hook(HookPoint::SessionEnd, ctx, &HookEventData::new())
    }

    /// Fire when a tab is created.
    pub fn on_tab_create(&self, ctx: &HookContext, tab_index: usize) -> Vec<Action> {
        let mut event = HookEventData::new();
        event.set("tab_index", tab_index as i64);
        self.run_hook(HookPoint::TabCreate, ctx, &event)
    }

    /// Fire when a tab is closed.
    pub fn on_tab_close(&self, ctx: &HookContext, tab_index: usize) -> Vec<Action> {
        let mut event = HookEventData::new();
        event.set("tab_index", tab_index as i64);
        self.run_hook(HookPoint::TabClose, ctx, &event)
    }

    /// Fire before a snapshot is taken. Returns `true` if the snapshot
    /// should proceed, `false` if any script vetoed it.
    pub fn on_snapshot_before(&self, ctx: &HookContext, command: &str) -> bool {
        if !self.has_scripts_for(HookPoint::SnapshotBefore) {
            return true;
        }
        let mut event = HookEventData::new();
        event.set("command", command.to_string());
        let result = self.system.run_hook(HookPoint::SnapshotBefore, ctx, &event);
        // filter_result is Some(true/false) for SnapshotBefore, default true
        result.filter_result.unwrap_or(true)
    }

    // -------------------------------------------------------------------
    // Action execution
    // -------------------------------------------------------------------

    /// Execute a list of actions produced by scripts.
    ///
    /// Handles `Log`, `Notify`, and `Commit` directly. Other action types
    /// are logged as debug (to be wired in future phases).
    pub fn execute_actions(&self, actions: &[Action], project_root: &str) {
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
                other => {
                    tracing::debug!("[script] unhandled action: {:?}", other);
                }
            }
        }
    }

    // -------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------

    /// Run a hook and return the resulting actions. Returns empty if
    /// scripting is disabled or an error occurs.
    fn run_hook(
        &self,
        hook: HookPoint,
        context: &HookContext,
        event_data: &HookEventData,
    ) -> Vec<Action> {
        if !self.enabled {
            return Vec::new();
        }
        let result = self.system.run_hook(hook.clone(), context, event_data);
        for (name, err) in &result.errors {
            tracing::warn!("ScriptBridge: script '{name}' error on {hook:?}: {err}");
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
}

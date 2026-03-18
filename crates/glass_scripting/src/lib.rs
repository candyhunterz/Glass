pub mod actions;
pub mod context;
pub mod engine;
pub mod hooks;
pub mod lifecycle;
pub mod loader;
pub mod mcp;
pub mod profile;
pub mod sandbox;
pub mod types;

pub use actions::{Action, ConfigValue, LogLevel};
pub use context::{CommandSnapshot, HookContext, HookEventData};
pub use engine::{ScriptEngine, ScriptRunResult};
pub use hooks::HookRegistry;
pub use loader::{load_all_scripts, load_scripts_from_dir};
pub use mcp::ScriptToolRegistry;
pub use sandbox::*;
pub use types::{HookPoint, LoadedScript, ScriptManifest, ScriptOrigin, ScriptStatus};

use std::path::Path;

// ---------------------------------------------------------------------------
// ScriptSystem -- top-level orchestrator that wires engine + registry + sandbox
// ---------------------------------------------------------------------------

/// The top-level scripting orchestrator.
///
/// `ScriptSystem` owns the Rhai [`ScriptEngine`], the [`HookRegistry`], and the
/// [`SandboxConfig`]. It provides a single entry point (`run_hook`) that
/// executes every registered script for a hook point, aggregates their actions
/// and errors, and applies hook-specific semantics:
///
/// - **`SnapshotBefore`** -- AND aggregation. `filter_result` defaults to
///   `true`; any `Confirmed` or `User`-origin script that errors sets it to
///   `false` (veto).
/// - **`McpRequest`** -- first-responder-wins. The first script whose returned
///   actions are non-empty is considered the responder and remaining scripts are
///   skipped.
pub struct ScriptSystem {
    engine: engine::ScriptEngine,
    registry: hooks::HookRegistry,
    sandbox: sandbox::SandboxConfig,
}

impl ScriptSystem {
    /// Create a new `ScriptSystem` with the given sandbox config.
    ///
    /// Starts with an empty registry (no scripts loaded).
    pub fn new(sandbox: sandbox::SandboxConfig) -> Self {
        let engine = engine::ScriptEngine::new(&sandbox);
        // Empty registry -- no scripts, default per-hook limit from sandbox.
        let registry = hooks::HookRegistry::new(Vec::new(), sandbox.max_scripts_per_hook);
        Self {
            engine,
            registry,
            sandbox,
        }
    }

    /// Load scripts from a single directory, compile them, build the registry,
    /// and return any compile errors as `(script_name, error_message)` pairs.
    pub fn load_from_dir(&mut self, dir: &Path) -> Vec<(String, String)> {
        let scripts = loader::load_scripts_from_dir(dir);
        self.ingest(scripts)
    }

    /// Load scripts from both project-local and global directories, compile
    /// them, build the registry, and return any compile errors.
    pub fn load_all(&mut self, project_root: &str) -> Vec<(String, String)> {
        let scripts = loader::load_all_scripts(project_root);
        self.ingest(scripts)
    }

    /// Run all scripts registered for `hook`, aggregating their actions and
    /// errors into a single [`ScriptRunResult`].
    ///
    /// # Hook-specific semantics
    ///
    /// **`SnapshotBefore`** -- `filter_result` starts as `Some(true)`. If any
    /// script with `Confirmed` status or `User` origin errors during execution,
    /// `filter_result` is set to `Some(false)` (veto). Scripts that are merely
    /// `Provisional` or `Stale` cannot veto.
    ///
    /// **`McpRequest`** -- first-responder-wins. The loop stops as soon as a
    /// script produces at least one action (indicating it handled the request).
    pub fn run_hook(
        &self,
        hook: HookPoint,
        context: &context::HookContext,
        event_data: &context::HookEventData,
    ) -> ScriptRunResult {
        let scripts = self.registry.scripts_for(hook.clone());

        let mut result = ScriptRunResult::default();

        // Snapshot-before: AND aggregation with default true.
        let is_snapshot_before = hook == HookPoint::SnapshotBefore;
        let is_mcp_request = hook == HookPoint::McpRequest;

        if is_snapshot_before {
            result.filter_result = Some(true);
        }

        for script in scripts {
            match self.engine.run_script(&script.manifest.name, context, event_data) {
                Ok(actions) => {
                    // McpRequest: first script with non-empty actions wins.
                    if is_mcp_request && !actions.is_empty() {
                        result.actions.extend(actions);
                        break;
                    }
                    result.actions.extend(actions);
                }
                Err(e) => {
                    result
                        .errors
                        .push((script.manifest.name.clone(), e));

                    // SnapshotBefore veto: confirmed or user-origin script
                    // errors count as a veto.
                    if is_snapshot_before && can_veto(script) {
                        result.filter_result = Some(false);
                    }
                }
            }
        }

        result
    }

    /// Check whether any scripts are registered for `hook`.
    pub fn has_scripts_for(&self, hook: HookPoint) -> bool {
        self.registry.has_scripts_for(hook)
    }

    /// Return references to all unique scripts across every hook.
    pub fn all_scripts(&self) -> Vec<&types::LoadedScript> {
        self.registry.all_scripts()
    }

    // -- private helpers -----------------------------------------------------

    /// Compile scripts, rebuild the registry, return compile errors.
    fn ingest(&mut self, scripts: Vec<types::LoadedScript>) -> Vec<(String, String)> {
        let errors = self.engine.compile_all(&scripts);
        self.registry =
            hooks::HookRegistry::new(scripts, self.sandbox.max_scripts_per_hook);
        errors
    }
}

/// A script can veto a `SnapshotBefore` filter if it has `Confirmed` status or
/// `User` origin.
fn can_veto(script: &types::LoadedScript) -> bool {
    script.manifest.status == ScriptStatus::Confirmed
        || script.manifest.origin == ScriptOrigin::User
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    /// Create a minimal temp script directory with one hook script.
    fn setup_script_dir() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let hooks_dir = tmp.path().join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Manifest
        let manifest = r#"name = "test-hook"
hooks = ["command_complete"]
status = "confirmed"
origin = "feedback"
version = 1
api_version = "1"
type = "hook"
"#;
        fs::write(hooks_dir.join("test-hook.toml"), manifest).unwrap();

        // Source -- logs a message using the glass API
        let source = r#"glass.log("info", "hook fired");"#;
        fs::write(hooks_dir.join("test-hook.rhai"), source).unwrap();

        tmp
    }

    fn default_context() -> HookContext {
        HookContext {
            cwd: "/project".to_string(),
            git_branch: "main".to_string(),
            git_dirty_files: Vec::new(),
            recent_commands: Vec::new(),
            active_rules: Vec::new(),
            config_values: HashMap::new(),
        }
    }

    #[test]
    fn script_system_loads_and_runs() {
        let tmp = setup_script_dir();

        let mut system = ScriptSystem::new(SandboxConfig::default());
        let errors = system.load_from_dir(tmp.path());
        assert!(errors.is_empty(), "compile errors: {errors:?}");

        // Verify the script was loaded
        assert!(system.has_scripts_for(HookPoint::CommandComplete));
        assert_eq!(system.all_scripts().len(), 1);

        // Run the hook
        let ctx = default_context();
        let event = HookEventData::new();
        let result = system.run_hook(HookPoint::CommandComplete, &ctx, &event);

        assert!(result.errors.is_empty(), "run errors: {:?}", result.errors);
        assert_eq!(result.actions.len(), 1);
        match &result.actions[0] {
            Action::Log { level, message } => {
                assert_eq!(*level, LogLevel::Info);
                assert_eq!(message, "hook fired");
            }
            other => panic!("Expected Action::Log, got {other:?}"),
        }
        // Non-filter hook -- filter_result should be None
        assert!(result.filter_result.is_none());
    }

    #[test]
    fn snapshot_before_default_true_no_veto() {
        let tmp = TempDir::new().unwrap();
        let hooks_dir = tmp.path().join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        let manifest = r#"name = "snap-guard"
hooks = ["snapshot_before"]
status = "confirmed"
origin = "feedback"
version = 1
api_version = "1"
type = "hook"
"#;
        fs::write(hooks_dir.join("snap-guard.toml"), manifest).unwrap();
        fs::write(
            hooks_dir.join("snap-guard.rhai"),
            r#"glass.log("debug", "snapshot ok");"#,
        )
        .unwrap();

        let mut system = ScriptSystem::new(SandboxConfig::default());
        let errors = system.load_from_dir(tmp.path());
        assert!(errors.is_empty());

        let result = system.run_hook(
            HookPoint::SnapshotBefore,
            &default_context(),
            &HookEventData::new(),
        );

        // Script succeeded, so filter_result stays true
        assert_eq!(result.filter_result, Some(true));
        assert!(result.errors.is_empty());
    }

    #[test]
    fn snapshot_before_veto_on_confirmed_error() {
        let tmp = TempDir::new().unwrap();
        let hooks_dir = tmp.path().join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        let manifest = r#"name = "bad-guard"
hooks = ["snapshot_before"]
status = "confirmed"
origin = "feedback"
version = 1
api_version = "1"
type = "hook"
"#;
        fs::write(hooks_dir.join("bad-guard.toml"), manifest).unwrap();
        // Script that will error at runtime
        fs::write(
            hooks_dir.join("bad-guard.rhai"),
            "let x = missing_fn();",
        )
        .unwrap();

        let mut system = ScriptSystem::new(SandboxConfig::default());
        let errors = system.load_from_dir(tmp.path());
        assert!(errors.is_empty(), "should compile fine");

        let result = system.run_hook(
            HookPoint::SnapshotBefore,
            &default_context(),
            &HookEventData::new(),
        );

        // Confirmed script errored -> veto
        assert_eq!(result.filter_result, Some(false));
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn mcp_request_first_responder_wins() {
        let tmp = TempDir::new().unwrap();
        let hooks_dir = tmp.path().join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // First script: produces an action
        let m1 = r#"name = "handler-a"
hooks = ["mcp_request"]
status = "confirmed"
origin = "user"
version = 1
api_version = "1"
type = "hook"
"#;
        fs::write(hooks_dir.join("handler-a.toml"), m1).unwrap();
        fs::write(
            hooks_dir.join("handler-a.rhai"),
            r#"glass.log("info", "handled by A");"#,
        )
        .unwrap();

        // Second script: also produces an action
        let m2 = r#"name = "handler-b"
hooks = ["mcp_request"]
status = "confirmed"
origin = "feedback"
version = 1
api_version = "1"
type = "hook"
"#;
        fs::write(hooks_dir.join("handler-b.toml"), m2).unwrap();
        fs::write(
            hooks_dir.join("handler-b.rhai"),
            r#"glass.log("info", "handled by B");"#,
        )
        .unwrap();

        let mut system = ScriptSystem::new(SandboxConfig::default());
        let errors = system.load_from_dir(tmp.path());
        assert!(errors.is_empty());

        let result = system.run_hook(
            HookPoint::McpRequest,
            &default_context(),
            &HookEventData::new(),
        );

        // First responder wins -- only one script's actions
        assert_eq!(result.actions.len(), 1);
        // McpRequest ordering: User first, so handler-a runs first
        match &result.actions[0] {
            Action::Log { message, .. } => {
                assert_eq!(message, "handled by A");
            }
            other => panic!("Expected Action::Log, got {other:?}"),
        }
    }

    #[test]
    fn has_scripts_for_empty_registry() {
        let system = ScriptSystem::new(SandboxConfig::default());
        assert!(!system.has_scripts_for(HookPoint::CommandComplete));
        assert!(system.all_scripts().is_empty());
    }

    #[test]
    fn load_from_dir_returns_compile_errors() {
        let tmp = TempDir::new().unwrap();
        let hooks_dir = tmp.path().join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        let manifest = r#"name = "broken"
hooks = ["command_start"]
status = "confirmed"
origin = "feedback"
version = 1
api_version = "1"
type = "hook"
"#;
        fs::write(hooks_dir.join("broken.toml"), manifest).unwrap();
        fs::write(hooks_dir.join("broken.rhai"), "let x = ;").unwrap();

        let mut system = ScriptSystem::new(SandboxConfig::default());
        let errors = system.load_from_dir(tmp.path());
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, "broken");
    }
}

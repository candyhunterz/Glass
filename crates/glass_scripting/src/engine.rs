use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rhai::{Array, Dynamic, Engine, Map, Scope, AST};

use crate::actions::{Action, ConfigValue, LogLevel};
use crate::context::{HookContext, HookEventData};
use crate::sandbox::SandboxConfig;
use crate::types::LoadedScript;

// ---------------------------------------------------------------------------
// GlassApi — the custom Rhai type exposed as the `glass` variable in scripts
// ---------------------------------------------------------------------------

/// The `glass` object available to every Rhai script.
///
/// Read-only accessors return cloned snapshots of Glass state.
/// Action methods push [`Action`] values into a shared collector that the
/// host reads after the script finishes.
#[derive(Debug, Clone)]
pub struct GlassApi {
    cwd: String,
    git_branch: String,
    git_dirty_files: Vec<String>,
    config_values: HashMap<String, String>,
    active_rules: Vec<String>,
    actions: Arc<Mutex<Vec<Action>>>,
}

impl GlassApi {
    /// Build a new `GlassApi` from the hook context snapshot.
    fn from_context(ctx: &HookContext, actions: Arc<Mutex<Vec<Action>>>) -> Self {
        Self {
            cwd: ctx.cwd.clone(),
            git_branch: ctx.git_branch.clone(),
            git_dirty_files: ctx.git_dirty_files.clone(),
            config_values: ctx.config_values.clone(),
            active_rules: ctx.active_rules.clone(),
            actions,
        }
    }

    // -- read-only accessors ------------------------------------------------

    fn cwd(&mut self) -> String {
        self.cwd.clone()
    }

    fn git_branch(&mut self) -> String {
        self.git_branch.clone()
    }

    fn git_dirty_files(&mut self) -> Array {
        self.git_dirty_files
            .iter()
            .map(|s| Dynamic::from(s.clone()))
            .collect()
    }

    fn config(&mut self, key: String) -> Dynamic {
        self.config_values
            .get(&key)
            .map(|v| Dynamic::from(v.clone()))
            .unwrap_or(Dynamic::UNIT)
    }

    fn active_rules(&mut self) -> Array {
        self.active_rules
            .iter()
            .map(|s| Dynamic::from(s.clone()))
            .collect()
    }

    // -- action methods (push into the shared collector) --------------------

    fn commit(&mut self, message: String) {
        self.push_action(Action::Commit { message });
    }

    fn log(&mut self, level: String, message: String) {
        let level = match level.to_lowercase().as_str() {
            "debug" => LogLevel::Debug,
            "warn" | "warning" => LogLevel::Warn,
            "error" => LogLevel::Error,
            _ => LogLevel::Info,
        };
        self.push_action(Action::Log { level, message });
    }

    fn notify(&mut self, message: String) {
        self.push_action(Action::Notify {
            message,
            title: None,
        });
    }

    fn set_config(&mut self, key: String, value: Dynamic) {
        let config_value = dynamic_to_config_value(&value);
        self.push_action(Action::SetConfig {
            key,
            value: config_value,
        });
    }

    fn inject_prompt_hint(&mut self, text: String) {
        self.push_action(Action::InjectPromptHint { hint: text });
    }

    fn force_snapshot(&mut self, paths: Array) {
        let paths: Vec<String> = paths
            .into_iter()
            .filter_map(|d| d.into_string().ok())
            .collect();
        self.push_action(Action::ForceSnapshot { paths });
    }

    fn trigger_checkpoint(&mut self, reason: String) {
        let reason = if reason.is_empty() {
            None
        } else {
            Some(reason)
        };
        self.push_action(Action::TriggerCheckpoint { reason });
    }

    fn extend_silence(&mut self, extra_secs: i64) {
        let duration_ms = (extra_secs.max(0) as u64) * 1000;
        self.push_action(Action::ExtendSilence { duration_ms });
    }

    // -- internal -----------------------------------------------------------

    fn push_action(&self, action: Action) {
        if let Ok(mut actions) = self.actions.lock() {
            actions.push(action);
        }
    }
}

/// Convert a Rhai `Dynamic` value to our `ConfigValue` enum.
fn dynamic_to_config_value(value: &Dynamic) -> ConfigValue {
    if let Ok(b) = value.as_bool() {
        return ConfigValue::Bool(b);
    }
    if let Ok(i) = value.as_int() {
        return ConfigValue::Int(i);
    }
    if let Ok(f) = value.as_float() {
        return ConfigValue::Float(f);
    }
    // Fall back to string representation.
    ConfigValue::String(value.to_string())
}

// ---------------------------------------------------------------------------
// ScriptRunResult
// ---------------------------------------------------------------------------

/// The result of running a single script through the engine.
#[derive(Debug, Default)]
pub struct ScriptRunResult {
    pub actions: Vec<Action>,
    pub errors: Vec<(String, String)>,
    pub filter_result: Option<bool>,
    pub mcp_response: Option<Dynamic>,
}

// ---------------------------------------------------------------------------
// ScriptEngine
// ---------------------------------------------------------------------------

/// The Rhai-based script execution engine.
///
/// Holds the configured [`Engine`], sandbox limits, and a cache of compiled
/// ASTs keyed by script name.
pub struct ScriptEngine {
    engine: Engine,
    ast_cache: HashMap<String, AST>,
}

impl ScriptEngine {
    /// Create a new engine with the given sandbox configuration.
    pub fn new(sandbox: &SandboxConfig) -> Self {
        let mut engine = Engine::new();

        // Apply sandbox limits.
        engine.set_max_operations(sandbox.max_operations);
        engine.set_max_expr_depths(64, 32);
        engine.set_max_call_levels(32);
        engine.set_max_string_size(1_048_576); // 1 MiB
        engine.set_max_array_size(10_000);
        engine.set_max_map_size(10_000);

        // Register the GlassApi custom type and its methods.
        engine.register_type_with_name::<GlassApi>("GlassApi");

        // Read-only accessors
        engine.register_fn("cwd", GlassApi::cwd);
        engine.register_fn("git_branch", GlassApi::git_branch);
        engine.register_fn("git_dirty_files", GlassApi::git_dirty_files);
        engine.register_fn("config", GlassApi::config);
        engine.register_fn("active_rules", GlassApi::active_rules);

        // Action methods
        engine.register_fn("commit", GlassApi::commit);
        engine.register_fn("log", GlassApi::log);
        engine.register_fn("notify", GlassApi::notify);
        engine.register_fn("set_config", GlassApi::set_config);
        engine.register_fn("inject_prompt_hint", GlassApi::inject_prompt_hint);
        engine.register_fn("force_snapshot", GlassApi::force_snapshot);
        engine.register_fn("trigger_checkpoint", GlassApi::trigger_checkpoint);
        engine.register_fn("extend_silence", GlassApi::extend_silence);

        Self {
            engine,
            ast_cache: HashMap::new(),
        }
    }

    /// Compile a single script and cache its AST.
    ///
    /// Returns `Ok(())` on success or an error message describing the parse
    /// failure.
    pub fn compile(&mut self, script: &LoadedScript) -> Result<(), String> {
        let ast = self
            .engine
            .compile(&script.source)
            .map_err(|e| format!("{e}"))?;
        self.ast_cache.insert(script.manifest.name.clone(), ast);
        Ok(())
    }

    /// Compile all scripts, returning a list of `(name, error)` for any that
    /// failed to compile.
    pub fn compile_all(&mut self, scripts: &[LoadedScript]) -> Vec<(String, String)> {
        let mut errors = Vec::new();
        for script in scripts {
            if let Err(e) = self.compile(script) {
                errors.push((script.manifest.name.clone(), e));
            }
        }
        errors
    }

    /// Execute a previously compiled script.
    ///
    /// The script receives two scope variables:
    /// - `event`: a Rhai `Map` built from `event_data`
    /// - `glass`: a [`GlassApi`] instance with read-only state + action methods
    ///
    /// Returns the collected [`Action`]s on success or an error message.
    pub fn run_script(
        &self,
        name: &str,
        context: &HookContext,
        event_data: &HookEventData,
    ) -> Result<Vec<Action>, String> {
        let ast = self
            .ast_cache
            .get(name)
            .ok_or_else(|| format!("script '{name}' not compiled"))?;

        // Build the shared action collector.
        let actions: Arc<Mutex<Vec<Action>>> = Arc::new(Mutex::new(Vec::new()));

        // Build the `glass` API object from the context snapshot.
        let glass_api = GlassApi::from_context(context, Arc::clone(&actions));

        // Build the `event` map from the event data.
        let mut event_map = Map::new();
        for (key, value) in &event_data.fields {
            event_map.insert(key.as_str().into(), value.clone());
        }

        // Set up scope.
        let mut scope = Scope::new();
        scope.push("glass", glass_api);
        scope.push("event", event_map);

        // Run the script.
        self.engine
            .run_ast_with_scope(&mut scope, ast)
            .map_err(|e| format!("{e}"))?;

        // Collect actions from the shared mutex.
        let collected = actions
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();

        Ok(collected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::SandboxConfig;
    use crate::types::{HookPoint, LoadedScript, ScriptManifest, ScriptOrigin, ScriptStatus};
    use std::path::PathBuf;

    /// Helper to build a LoadedScript from inline source code.
    fn make_script(name: &str, source: &str) -> LoadedScript {
        LoadedScript {
            manifest: ScriptManifest {
                name: name.to_string(),
                hooks: vec![HookPoint::CommandComplete],
                status: ScriptStatus::Confirmed,
                origin: ScriptOrigin::Feedback,
                version: 1,
                api_version: "1".to_string(),
                created: None,
                failure_count: 0,
                trigger_count: 0,
                stale_runs: 0,
                script_type: "hook".to_string(),
                description: None,
                params: None,
            },
            source: source.to_string(),
            manifest_path: PathBuf::from(format!("/test/{name}.toml")),
            source_path: PathBuf::from(format!("/test/{name}.rhai")),
        }
    }

    fn default_context() -> HookContext {
        HookContext {
            cwd: "/home/user/project".to_string(),
            git_branch: "main".to_string(),
            git_dirty_files: vec!["src/lib.rs".to_string()],
            recent_commands: Vec::new(),
            active_rules: vec!["rule-a".to_string()],
            config_values: {
                let mut m = HashMap::new();
                m.insert("snapshot.enabled".to_string(), "true".to_string());
                m
            },
        }
    }

    #[test]
    fn compile_valid_script() {
        let mut engine = ScriptEngine::new(&SandboxConfig::default());
        let script = make_script("valid", "let x = 42;");
        assert!(engine.compile(&script).is_ok());
    }

    #[test]
    fn compile_invalid_script() {
        let mut engine = ScriptEngine::new(&SandboxConfig::default());
        let script = make_script("invalid", "let x = ;");
        let result = engine.compile(&script);
        assert!(result.is_err());
        assert!(
            !result.unwrap_err().is_empty(),
            "error message should be non-empty"
        );
    }

    #[test]
    fn run_script_accesses_event_data() {
        let mut engine = ScriptEngine::new(&SandboxConfig::default());
        let script = make_script(
            "event-reader",
            r#"
                let code = event.exit_code;
                if code != 0 {
                    glass.log("warn", "non-zero exit");
                }
            "#,
        );
        engine.compile(&script).unwrap();

        let ctx = default_context();
        let mut event = HookEventData::new();
        event.set("exit_code", 1_i64);

        let actions = engine.run_script("event-reader", &ctx, &event).unwrap();
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::Log { level, message } => {
                assert_eq!(*level, LogLevel::Warn);
                assert_eq!(message, "non-zero exit");
            }
            other => panic!("Expected Action::Log, got {other:?}"),
        }
    }

    #[test]
    fn sandbox_limits_excessive_operations() {
        let sandbox = SandboxConfig::new(1_000, 2_000, 10, 100, 20);
        let mut engine = ScriptEngine::new(&sandbox);
        let script = make_script(
            "infinite",
            r#"
                let x = 0;
                loop {
                    x += 1;
                }
            "#,
        );
        engine.compile(&script).unwrap();

        let ctx = default_context();
        let event = HookEventData::new();
        let result = engine.run_script("infinite", &ctx, &event);
        assert!(result.is_err(), "should hit operation limit");
    }

    #[test]
    fn script_calls_glass_commit() {
        let mut engine = ScriptEngine::new(&SandboxConfig::default());
        let script = make_script("committer", r#"glass.commit("test commit");"#);
        engine.compile(&script).unwrap();

        let ctx = default_context();
        let event = HookEventData::new();
        let actions = engine.run_script("committer", &ctx, &event).unwrap();

        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::Commit { message } => {
                assert_eq!(message, "test commit");
            }
            other => panic!("Expected Action::Commit, got {other:?}"),
        }
    }

    #[test]
    fn script_calls_glass_log() {
        let mut engine = ScriptEngine::new(&SandboxConfig::default());
        let script = make_script("logger", r#"glass.log("info", "hello world");"#);
        engine.compile(&script).unwrap();

        let ctx = default_context();
        let event = HookEventData::new();
        let actions = engine.run_script("logger", &ctx, &event).unwrap();

        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::Log { level, message } => {
                assert_eq!(*level, LogLevel::Info);
                assert_eq!(message, "hello world");
            }
            other => panic!("Expected Action::Log, got {other:?}"),
        }
    }

    #[test]
    fn script_reads_glass_state() {
        let mut engine = ScriptEngine::new(&SandboxConfig::default());
        let script = make_script(
            "state-reader",
            r#"
                let branch = glass.git_branch();
                let dir = glass.cwd();
                let dirty = glass.git_dirty_files();
                let rules = glass.active_rules();
                let snap_enabled = glass.config("snapshot.enabled");

                if branch == "main" && dirty.len() > 0 {
                    glass.log("info", "dirty main branch at " + dir);
                }
                if snap_enabled == "true" {
                    glass.log("debug", "snapshots on");
                }
                if rules.len() > 0 {
                    glass.log("debug", "has rules");
                }
            "#,
        );
        engine.compile(&script).unwrap();

        let ctx = default_context();
        let event = HookEventData::new();
        let actions = engine.run_script("state-reader", &ctx, &event).unwrap();

        // Should get 3 log actions
        assert_eq!(actions.len(), 3);
        match &actions[0] {
            Action::Log { level, message } => {
                assert_eq!(*level, LogLevel::Info);
                assert!(message.contains("dirty main branch"));
            }
            other => panic!("Expected Action::Log, got {other:?}"),
        }
    }

    #[test]
    fn script_not_compiled_returns_error() {
        let engine = ScriptEngine::new(&SandboxConfig::default());
        let ctx = default_context();
        let event = HookEventData::new();
        let result = engine.run_script("nonexistent", &ctx, &event);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not compiled"));
    }

    #[test]
    fn compile_all_collects_errors() {
        let mut engine = ScriptEngine::new(&SandboxConfig::default());
        let scripts = vec![
            make_script("good", "let x = 42;"),
            make_script("bad", "let x = ;"),
            make_script("also-good", "let y = 10;"),
        ];
        let errors = engine.compile_all(&scripts);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, "bad");
    }

    #[test]
    fn script_calls_multiple_actions() {
        let mut engine = ScriptEngine::new(&SandboxConfig::default());
        let script = make_script(
            "multi-action",
            r#"
                glass.log("info", "starting");
                glass.commit("auto-commit");
                glass.notify("done!");
                glass.force_snapshot(["src/main.rs", "Cargo.toml"]);
                glass.trigger_checkpoint("test reason");
                glass.extend_silence(30);
                glass.set_config("snapshot.enabled", true);
                glass.inject_prompt_hint("remember to test");
            "#,
        );
        engine.compile(&script).unwrap();

        let ctx = default_context();
        let event = HookEventData::new();
        let actions = engine.run_script("multi-action", &ctx, &event).unwrap();

        assert_eq!(actions.len(), 8);

        // Verify each action type in order
        assert!(matches!(&actions[0], Action::Log { .. }));
        assert!(matches!(&actions[1], Action::Commit { .. }));
        assert!(matches!(&actions[2], Action::Notify { .. }));
        assert!(matches!(&actions[3], Action::ForceSnapshot { paths } if paths.len() == 2));
        assert!(
            matches!(&actions[4], Action::TriggerCheckpoint { reason } if reason.as_deref() == Some("test reason"))
        );
        assert!(
            matches!(&actions[5], Action::ExtendSilence { duration_ms } if *duration_ms == 30_000)
        );
        assert!(
            matches!(&actions[6], Action::SetConfig { key, value } if key == "snapshot.enabled" && *value == ConfigValue::Bool(true))
        );
        assert!(
            matches!(&actions[7], Action::InjectPromptHint { hint } if hint == "remember to test")
        );
    }
}

use std::collections::HashMap;

/// Snapshot of Glass state taken once per hook invocation.
#[derive(Debug, Clone, Default)]
pub struct HookContext {
    pub cwd: String,
    pub git_branch: String,
    pub git_dirty_files: Vec<String>,
    pub recent_commands: Vec<CommandSnapshot>,
    pub active_rules: Vec<String>,
    pub config_values: HashMap<String, String>,
}

/// A snapshot of a single command's metadata.
#[derive(Debug, Clone)]
pub struct CommandSnapshot {
    pub command: String,
    pub exit_code: i32,
    pub cwd: String,
    pub duration_ms: u64,
}

/// Event-specific data passed to scripts as the `event` variable.
#[derive(Debug, Clone, Default)]
pub struct HookEventData {
    pub fields: HashMap<String, rhai::Dynamic>,
}

impl HookEventData {
    pub fn new() -> Self {
        Self {
            fields: HashMap::new(),
        }
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<rhai::Dynamic>) {
        self.fields.insert(key.into(), value.into());
    }
}

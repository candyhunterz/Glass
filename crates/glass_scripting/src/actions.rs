use serde::{Deserialize, Serialize};

/// Log severity level for script-emitted log messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// A dynamically-typed configuration value that scripts can set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfigValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
}

/// Actions that scripts can return to affect Glass behavior.
///
/// Each variant represents a side effect the scripting engine
/// will request Glass to perform after script execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    /// Commit current changes (standard git commit).
    Commit {
        message: String,
    },
    /// Commit using an isolated worktree.
    IsolateCommit {
        message: String,
        #[serde(default)]
        files: Vec<String>,
    },
    /// Revert specified files to their last snapshot state.
    RevertFiles {
        paths: Vec<String>,
    },
    /// Set a configuration value at a dotted key path.
    SetConfig {
        key: String,
        value: ConfigValue,
    },
    /// Force an immediate snapshot of specified paths.
    ForceSnapshot {
        #[serde(default)]
        paths: Vec<String>,
    },
    /// Change the snapshot policy for a path pattern.
    SetSnapshotPolicy {
        pattern: String,
        policy: String,
    },
    /// Tag a command block with metadata.
    TagCommand {
        #[serde(default)]
        block_id: Option<String>,
        tag: String,
    },
    /// Inject a hint into the next orchestrator prompt.
    InjectPromptHint {
        hint: String,
    },
    /// Trigger an orchestrator checkpoint.
    TriggerCheckpoint {
        #[serde(default)]
        reason: Option<String>,
    },
    /// Extend the silence timeout by a duration.
    ExtendSilence {
        duration_ms: u64,
    },
    /// Block the current orchestrator iteration (e.g., rate limiting).
    BlockIteration {
        #[serde(default)]
        reason: Option<String>,
    },
    /// Enable a script by name.
    EnableScript {
        name: String,
    },
    /// Disable a script by name.
    DisableScript {
        name: String,
    },
    /// Register a new MCP tool.
    RegisterTool {
        name: String,
        #[serde(default)]
        description: Option<String>,
    },
    /// Unregister an MCP tool by name.
    UnregisterTool {
        name: String,
    },
    /// Emit a log message at the given level.
    Log {
        level: LogLevel,
        message: String,
    },
    /// Display a notification to the user.
    Notify {
        message: String,
        #[serde(default)]
        title: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_action_roundtrip() {
        let action = Action::SetConfig {
            key: "snapshot.enabled".to_string(),
            value: ConfigValue::Bool(true),
        };
        let json = serde_json::to_string(&action).unwrap();
        let parsed: Action = serde_json::from_str(&json).unwrap();
        match parsed {
            Action::SetConfig { key, value } => {
                assert_eq!(key, "snapshot.enabled");
                assert_eq!(value, ConfigValue::Bool(true));
            }
            _ => panic!("Expected SetConfig variant"),
        }
    }

    #[test]
    fn config_value_untagged_deserialization() {
        let bool_val: ConfigValue = serde_json::from_str("true").unwrap();
        assert_eq!(bool_val, ConfigValue::Bool(true));

        let int_val: ConfigValue = serde_json::from_str("42").unwrap();
        assert_eq!(int_val, ConfigValue::Int(42));

        let float_val: ConfigValue = serde_json::from_str("3.14").unwrap();
        assert_eq!(float_val, ConfigValue::Float(3.14));

        let str_val: ConfigValue = serde_json::from_str(r#""hello""#).unwrap();
        assert_eq!(str_val, ConfigValue::String("hello".to_string()));
    }
}

use serde::{Deserialize, Serialize};

/// Points in the Glass lifecycle where scripts can be triggered.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookPoint {
    CommandStart,
    CommandComplete,
    BlockStateChange,
    SnapshotBefore,
    SnapshotAfter,
    HistoryQuery,
    HistoryInsert,
    PipelineComplete,
    ConfigReload,
    OrchestratorRunStart,
    OrchestratorRunEnd,
    OrchestratorIteration,
    OrchestratorCheckpoint,
    OrchestratorStuck,
    McpRequest,
    McpResponse,
    TabCreate,
    TabClose,
    SessionStart,
    SessionEnd,
}

/// Lifecycle status of a script in the registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptStatus {
    Provisional,
    Confirmed,
    Rejected,
    Stale,
    Archived,
}

/// Where a script originated from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptOrigin {
    Feedback,
    User,
}

/// TOML manifest describing a script's metadata and configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptManifest {
    pub name: String,
    pub hooks: Vec<HookPoint>,
    #[serde(default = "default_status")]
    pub status: ScriptStatus,
    #[serde(default = "default_origin")]
    pub origin: ScriptOrigin,
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default = "default_api_version")]
    pub api_version: String,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub failure_count: u32,
    #[serde(default)]
    pub trigger_count: u64,
    #[serde(default)]
    pub stale_runs: u32,
    /// Script type (e.g. "hook", "mcp_tool"). Renamed from "type" to avoid keyword.
    #[serde(rename = "type", default = "default_script_type")]
    pub script_type: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub params: Option<toml::Value>,
}

fn default_status() -> ScriptStatus {
    ScriptStatus::Provisional
}

fn default_origin() -> ScriptOrigin {
    ScriptOrigin::Feedback
}

fn default_version() -> u32 {
    1
}

fn default_api_version() -> String {
    "1".to_string()
}

fn default_script_type() -> String {
    "hook".to_string()
}

/// A fully loaded script: manifest + source code + file paths.
#[derive(Debug, Clone)]
pub struct LoadedScript {
    pub manifest: ScriptManifest,
    pub source: String,
    pub manifest_path: std::path::PathBuf,
    pub source_path: std::path::PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_hook_manifest() {
        let toml_str = r#"
            name = "auto-snapshot"
            hooks = ["command_start", "command_complete"]
            status = "confirmed"
            origin = "feedback"
            version = 2
            api_version = "1"
            type = "hook"
            description = "Takes snapshots around commands"
        "#;
        let manifest: ScriptManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.name, "auto-snapshot");
        assert_eq!(manifest.hooks.len(), 2);
        assert_eq!(manifest.hooks[0], HookPoint::CommandStart);
        assert_eq!(manifest.hooks[1], HookPoint::CommandComplete);
        assert_eq!(manifest.status, ScriptStatus::Confirmed);
        assert_eq!(manifest.origin, ScriptOrigin::Feedback);
        assert_eq!(manifest.version, 2);
        assert_eq!(manifest.script_type, "hook");
        assert_eq!(
            manifest.description.as_deref(),
            Some("Takes snapshots around commands")
        );
    }

    #[test]
    fn deserialize_mcp_tool_manifest() {
        let toml_str = r#"
            name = "custom-tool"
            hooks = ["mcp_request"]
            type = "mcp_tool"
            description = "A custom MCP tool"

            [params]
            input_schema = "string"
            required = true
            timeout_ms = 5000
        "#;
        let manifest: ScriptManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.name, "custom-tool");
        assert_eq!(manifest.hooks, vec![HookPoint::McpRequest]);
        assert_eq!(manifest.script_type, "mcp_tool");
        assert!(manifest.params.is_some());

        let params = manifest.params.unwrap();
        let table = params.as_table().expect("params should be a table");
        assert_eq!(
            table.get("input_schema").and_then(|v| v.as_str()),
            Some("string")
        );
        assert_eq!(
            table.get("required").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            table.get("timeout_ms").and_then(|v| v.as_integer()),
            Some(5000)
        );
        // Defaults should apply
        assert_eq!(manifest.status, ScriptStatus::Provisional);
        assert_eq!(manifest.origin, ScriptOrigin::Feedback);
        assert_eq!(manifest.version, 1);
    }
}

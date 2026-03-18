//! Dynamic MCP tool registry for script-defined tools.
//!
//! Scripts with `script_type == "mcp_tool"` can expose themselves as MCP tools.
//! The [`ScriptToolRegistry`] scans loaded scripts, builds tool definitions, and
//! provides lookup for the MCP server to route dynamic tool calls.

use std::collections::HashMap;

use crate::types::{LoadedScript, ScriptStatus};

/// Definition of a script-backed MCP tool.
#[derive(Debug, Clone)]
pub struct ScriptToolDef {
    /// Tool name exposed to MCP clients.
    pub name: String,
    /// Human-readable description of what the tool does.
    pub description: String,
    /// JSON Schema describing the tool's parameters.
    pub params_schema: serde_json::Value,
    /// Name of the backing script (matches `ScriptManifest::name`).
    pub script_name: String,
}

/// Registry of script-defined MCP tools.
///
/// Built by scanning [`LoadedScript`]s for those with `script_type == "mcp_tool"`.
/// Provides lookup by tool name and listing of confirmed tools.
pub struct ScriptToolRegistry {
    tools: HashMap<String, ScriptToolDef>,
}

impl ScriptToolRegistry {
    /// Create an empty registry with no tools.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Scan `scripts` and register any with `script_type == "mcp_tool"`.
    ///
    /// If `include_provisional` is `false`, scripts with
    /// [`ScriptStatus::Provisional`] are skipped. Confirmed, User-origin, and
    /// other non-provisional scripts are always included.
    pub fn register_from_scripts(&mut self, scripts: &[LoadedScript], include_provisional: bool) {
        for script in scripts {
            if script.manifest.script_type != "mcp_tool" {
                continue;
            }
            if !include_provisional && script.manifest.status == ScriptStatus::Provisional {
                continue;
            }

            let description = script
                .manifest
                .description
                .clone()
                .unwrap_or_else(|| format!("Script tool: {}", script.manifest.name));

            // Build a JSON Schema from the manifest params (if any).
            // Each TOML param key becomes a property with its type inferred from
            // the TOML value type.
            let params_schema = build_params_schema(&script.manifest.params);

            let tool_name = script.manifest.name.clone();
            let def = ScriptToolDef {
                name: tool_name.clone(),
                description,
                params_schema,
                script_name: script.manifest.name.clone(),
            };
            self.tools.insert(tool_name, def);
        }
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<&ScriptToolDef> {
        self.tools.get(name)
    }

    /// List all registered tools (all statuses that passed the provisional filter).
    pub fn list_confirmed(&self) -> Vec<&ScriptToolDef> {
        self.tools.values().collect()
    }
}

impl Default for ScriptToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert optional TOML `params` table into a JSON Schema object.
///
/// Each key becomes a property. The type is inferred from the TOML value:
/// - String -> `"string"`
/// - Integer -> `"integer"`
/// - Float -> `"number"`
/// - Boolean -> `"boolean"`
///
/// If `params` is `None`, returns an empty object schema.
fn build_params_schema(params: &Option<toml::Value>) -> serde_json::Value {
    let table = match params {
        Some(toml::Value::Table(t)) => t,
        _ => {
            return serde_json::json!({
                "type": "object",
                "properties": {}
            });
        }
    };

    let mut properties = serde_json::Map::new();
    for (key, value) in table {
        let type_str = match value {
            toml::Value::String(_) => "string",
            toml::Value::Integer(_) => "integer",
            toml::Value::Float(_) => "number",
            toml::Value::Boolean(_) => "boolean",
            _ => "string",
        };
        properties.insert(
            key.clone(),
            serde_json::json!({ "type": type_str }),
        );
    }

    serde_json::json!({
        "type": "object",
        "properties": properties
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{LoadedScript, ScriptManifest, ScriptOrigin, ScriptStatus};
    use std::path::PathBuf;

    fn make_mcp_tool_script(name: &str, status: ScriptStatus) -> LoadedScript {
        LoadedScript {
            manifest: ScriptManifest {
                name: name.to_string(),
                hooks: vec![crate::types::HookPoint::McpRequest],
                status,
                origin: ScriptOrigin::Feedback,
                version: 1,
                api_version: "1".to_string(),
                created: None,
                failure_count: 0,
                trigger_count: 0,
                stale_runs: 0,
                script_type: "mcp_tool".to_string(),
                description: Some(format!("Tool: {}", name)),
                params: Some(toml::Value::Table({
                    let mut t = toml::map::Map::new();
                    t.insert("input".to_string(), toml::Value::String("string".to_string()));
                    t
                })),
            },
            source: String::new(),
            manifest_path: PathBuf::from("/tmp/test.toml"),
            source_path: PathBuf::from("/tmp/test.rhai"),
        }
    }

    fn make_hook_script(name: &str) -> LoadedScript {
        LoadedScript {
            manifest: ScriptManifest {
                name: name.to_string(),
                hooks: vec![crate::types::HookPoint::CommandComplete],
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
            source: String::new(),
            manifest_path: PathBuf::from("/tmp/hook.toml"),
            source_path: PathBuf::from("/tmp/hook.rhai"),
        }
    }

    #[test]
    fn register_mcp_tool_from_script() {
        let scripts = vec![
            make_mcp_tool_script("my-tool", ScriptStatus::Confirmed),
            make_hook_script("regular-hook"),
        ];

        let mut registry = ScriptToolRegistry::new();
        registry.register_from_scripts(&scripts, false);

        // The mcp_tool script should be registered
        assert!(registry.get("my-tool").is_some());
        let def = registry.get("my-tool").unwrap();
        assert_eq!(def.name, "my-tool");
        assert_eq!(def.description, "Tool: my-tool");
        assert_eq!(def.script_name, "my-tool");

        // The hook script should NOT be registered (wrong script_type)
        assert!(registry.get("regular-hook").is_none());

        // list_confirmed should return exactly one tool
        assert_eq!(registry.list_confirmed().len(), 1);
    }

    #[test]
    fn skip_provisional_when_not_included() {
        let scripts = vec![
            make_mcp_tool_script("provisional-tool", ScriptStatus::Provisional),
            make_mcp_tool_script("confirmed-tool", ScriptStatus::Confirmed),
        ];

        let mut registry = ScriptToolRegistry::new();
        registry.register_from_scripts(&scripts, false);

        // Provisional should be skipped
        assert!(registry.get("provisional-tool").is_none());
        // Confirmed should be present
        assert!(registry.get("confirmed-tool").is_some());
        assert_eq!(registry.list_confirmed().len(), 1);
    }

    #[test]
    fn include_provisional_when_requested() {
        let scripts = vec![
            make_mcp_tool_script("provisional-tool", ScriptStatus::Provisional),
            make_mcp_tool_script("confirmed-tool", ScriptStatus::Confirmed),
        ];

        let mut registry = ScriptToolRegistry::new();
        registry.register_from_scripts(&scripts, true);

        // Both should be present
        assert!(registry.get("provisional-tool").is_some());
        assert!(registry.get("confirmed-tool").is_some());
        assert_eq!(registry.list_confirmed().len(), 2);
    }

    #[test]
    fn empty_registry() {
        let registry = ScriptToolRegistry::new();
        assert!(registry.get("anything").is_none());
        assert!(registry.list_confirmed().is_empty());
    }

    #[test]
    fn params_schema_from_toml() {
        let scripts = vec![make_mcp_tool_script("schema-test", ScriptStatus::Confirmed)];
        let mut registry = ScriptToolRegistry::new();
        registry.register_from_scripts(&scripts, false);

        let def = registry.get("schema-test").unwrap();
        let schema = &def.params_schema;
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["input"].is_object());
    }
}

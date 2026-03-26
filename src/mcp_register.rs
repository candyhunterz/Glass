//! MCP auto-registration — detects installed AI tools and registers
//! Glass's MCP server in their configuration files.

use std::path::{Path, PathBuf};

/// Result of attempting to register with one AI tool.
#[derive(Debug)]
#[allow(dead_code)]
pub struct RegistrationResult {
    pub tool: &'static str,
    pub path: PathBuf,
    pub action: RegistrationAction,
}

#[derive(Debug, PartialEq)]
pub enum RegistrationAction {
    Registered,
    AlreadyExists,
    NotInstalled,
    Error(String),
}

/// Known AI tools and their global MCP config paths relative to home.
struct ToolConfig {
    name: &'static str,
    /// Path segments relative to home directory.
    config_segments: &'static [&'static str],
}

const KNOWN_TOOLS: &[ToolConfig] = &[
    ToolConfig {
        name: "Claude Code",
        config_segments: &[".claude", "settings.local.json"],
    },
    ToolConfig {
        name: "Cursor",
        config_segments: &[".cursor", "mcp.json"],
    },
    ToolConfig {
        name: "Windsurf",
        config_segments: &[".codeium", "windsurf", "mcp_config.json"],
    },
];

/// Resolve the absolute path to the Glass binary.
fn glass_binary_path() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// Register Glass MCP entry in a single tool's config file.
///
/// Returns the action taken. Creates parent directories and the file
/// if they don't exist. Merges into existing `mcpServers` without
/// touching other keys.
fn register_tool_config(config_path: &Path, glass_binary: &str) -> RegistrationAction {
    // Check if parent directory exists (tool is installed)
    if let Some(parent) = config_path.parent() {
        if !parent.exists() {
            return RegistrationAction::NotInstalled;
        }
    }

    // Read existing config or start with empty object
    let mut root: serde_json::Value = if config_path.exists() {
        match std::fs::read_to_string(config_path) {
            Ok(content) if content.trim().is_empty() => serde_json::json!({}),
            Ok(content) => match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(e) => return RegistrationAction::Error(format!("parse error: {e}")),
            },
            Err(e) => return RegistrationAction::Error(format!("read error: {e}")),
        }
    } else {
        serde_json::json!({})
    };

    // Check if glass entry already exists
    if root
        .get("mcpServers")
        .and_then(|s| s.get("glass"))
        .is_some()
    {
        return RegistrationAction::AlreadyExists;
    }

    // Merge glass entry
    let mcp_servers = root
        .as_object_mut()
        .unwrap()
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    mcp_servers["glass"] = serde_json::json!({
        "command": glass_binary,
        "args": ["mcp", "serve"]
    });

    // Write back
    match std::fs::write(config_path, serde_json::to_string_pretty(&root).unwrap()) {
        Ok(()) => RegistrationAction::Registered,
        Err(e) => RegistrationAction::Error(format!("write error: {e}")),
    }
}

/// Auto-register Glass MCP server with all detected AI tools.
///
/// Resolves the Glass binary path, iterates known tools, and writes
/// config entries where the tool is installed and Glass is not yet
/// registered. Returns results for logging/diagnostics.
pub fn auto_register() -> Vec<RegistrationResult> {
    let binary = match glass_binary_path() {
        Some(p) => p.to_string_lossy().to_string(),
        None => {
            tracing::warn!("MCP auto-register: could not resolve glass binary path");
            return vec![];
        }
    };

    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            tracing::warn!("MCP auto-register: could not determine home directory");
            return vec![];
        }
    };

    let mut results = Vec::new();

    for tool in KNOWN_TOOLS {
        let mut config_path = home.clone();
        for segment in tool.config_segments {
            config_path = config_path.join(segment);
        }

        let action = register_tool_config(&config_path, &binary);
        tracing::info!(
            tool = tool.name,
            path = %config_path.display(),
            action = ?action,
            "MCP auto-register"
        );
        results.push(RegistrationResult {
            tool: tool.name,
            path: config_path,
            action,
        });
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_register_creates_config_when_absent() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join(".claude").join("settings.local.json");
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();

        let result = register_tool_config(&config_path, "/usr/bin/glass");

        assert_eq!(result, RegistrationAction::Registered);
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert!(content["mcpServers"]["glass"].is_object());
        assert_eq!(content["mcpServers"]["glass"]["command"], "/usr/bin/glass");
    }

    #[test]
    fn test_register_merges_into_existing() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("settings.local.json");
        let existing = serde_json::json!({
            "mcpServers": {
                "other-tool": { "command": "other", "args": [] }
            },
            "someOtherKey": true
        });
        std::fs::write(
            &config_path,
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        let result = register_tool_config(&config_path, "/usr/bin/glass");

        assert_eq!(result, RegistrationAction::Registered);
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert!(content["mcpServers"]["glass"].is_object());
        assert!(content["mcpServers"]["other-tool"].is_object());
        assert_eq!(content["someOtherKey"], true);
    }

    #[test]
    fn test_register_skips_when_present() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("settings.local.json");
        let existing = serde_json::json!({
            "mcpServers": {
                "glass": { "command": "/old/path/glass", "args": ["mcp", "serve"] }
            }
        });
        std::fs::write(
            &config_path,
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        let result = register_tool_config(&config_path, "/new/path/glass");

        assert_eq!(result, RegistrationAction::AlreadyExists);
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(content["mcpServers"]["glass"]["command"], "/old/path/glass");
    }

    #[test]
    fn test_register_skips_when_not_installed() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("nonexistent_tool_dir").join("config.json");

        let result = register_tool_config(&config_path, "/usr/bin/glass");

        assert_eq!(result, RegistrationAction::NotInstalled);
    }
}

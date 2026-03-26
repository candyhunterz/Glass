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

/// Hint block appended to AI tool instruction files.
#[allow(dead_code)]
const CONTEXT_HINT_MARKDOWN: &str = "\n\n## Glass Terminal Integration\n\n\
    Glass terminal history and context are available via MCP tools. \
    Use `glass_history` to search past commands and output across sessions. \
    Use `glass_context` for a summary of recent activity.\n";

/// Plain text variant for files that don't use markdown headers.
#[allow(dead_code)]
const CONTEXT_HINT_PLAIN: &str = "\n\nGlass Terminal Integration\n\n\
    Glass terminal history and context are available via MCP tools. \
    Use glass_history to search past commands and output across sessions. \
    Use glass_context for a summary of recent activity.\n";

/// Known AI instruction files and whether they use markdown.
#[allow(dead_code)]
const HINT_FILES: &[(&str, bool)] = &[
    ("CLAUDE.md", true),
    (".cursorrules", false),
    ("AGENTS.md", true),
    ("GEMINI.md", true),
];

/// Append Glass MCP context hints to existing AI instruction files.
///
/// Only appends to files that already exist and don't already mention
/// `glass_history`. Never creates new files.
#[allow(dead_code)]
pub fn inject_context_hints(project_root: &Path) {
    for &(filename, is_markdown) in HINT_FILES {
        let file_path = project_root.join(filename);
        if !file_path.exists() {
            continue;
        }

        let content = match std::fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if content.contains("glass_history") {
            continue;
        }

        let hint = if is_markdown {
            CONTEXT_HINT_MARKDOWN
        } else {
            CONTEXT_HINT_PLAIN
        };

        if let Err(e) = std::fs::write(&file_path, format!("{content}{hint}")) {
            tracing::warn!(
                "Failed to append context hint to {}: {e}",
                file_path.display()
            );
        }
    }
}

/// Append `.mcp.json` to `.gitignore` if not already listed.
#[allow(dead_code)]
fn update_gitignore(project_root: &Path) {
    let gitignore_path = project_root.join(".gitignore");
    let content = std::fs::read_to_string(&gitignore_path).unwrap_or_default();

    if content.lines().any(|line| line.trim() == ".mcp.json") {
        return;
    }

    let separator = if content.ends_with('\n') || content.is_empty() {
        ""
    } else {
        "\n"
    };

    if let Err(e) = std::fs::write(&gitignore_path, format!("{content}{separator}.mcp.json\n")) {
        tracing::warn!("Failed to update .gitignore: {e}");
    }
}

/// Write `.mcp.json` in the project root and update `.gitignore`.
///
/// Merges if the file already exists. Appends `.mcp.json` to
/// `.gitignore` if not already listed. Also injects context hints
/// into existing AI instruction files.
#[allow(dead_code)]
pub fn register_project_mcp(project_root: &Path, glass_binary: &str) -> RegistrationAction {
    let mcp_path = project_root.join(".mcp.json");
    let action = register_tool_config(&mcp_path, glass_binary);

    if matches!(action, RegistrationAction::Registered) {
        update_gitignore(project_root);
        inject_context_hints(project_root);
    }

    action
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

/// Print MCP registration diagnostics to stdout (for `glass check`).
pub fn print_diagnostics(results: &[RegistrationResult]) {
    println!("\nMCP Auto-Registration:");
    for result in results {
        let status = match &result.action {
            RegistrationAction::Registered => "registered",
            RegistrationAction::AlreadyExists => "already registered",
            RegistrationAction::NotInstalled => "not installed",
            RegistrationAction::Error(e) => {
                println!(
                    "  {:<14} {:<45} [error: {}]",
                    result.tool,
                    result.path.display(),
                    e
                );
                continue;
            }
        };
        println!(
            "  {:<14} {:<45} [{}]",
            result.tool,
            result.path.display(),
            status
        );
    }
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

    #[test]
    fn test_project_mcp_json_created() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path();

        let result = register_project_mcp(project_root, "/usr/bin/glass");

        assert_eq!(result, RegistrationAction::Registered);
        let mcp_path = project_root.join(".mcp.json");
        assert!(mcp_path.exists());
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&mcp_path).unwrap()).unwrap();
        assert_eq!(content["mcpServers"]["glass"]["command"], "/usr/bin/glass");
    }

    #[test]
    fn test_project_mcp_json_merges() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path();
        let mcp_path = project_root.join(".mcp.json");
        let existing = serde_json::json!({
            "mcpServers": {
                "existing-server": { "command": "existing", "args": [] }
            }
        });
        std::fs::write(&mcp_path, serde_json::to_string_pretty(&existing).unwrap()).unwrap();

        let result = register_project_mcp(project_root, "/usr/bin/glass");

        assert_eq!(result, RegistrationAction::Registered);
        let content: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&mcp_path).unwrap()).unwrap();
        assert!(content["mcpServers"]["glass"].is_object());
        assert!(content["mcpServers"]["existing-server"].is_object());
    }

    #[test]
    fn test_gitignore_updated() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path();
        std::fs::write(project_root.join(".gitignore"), "node_modules/\n").unwrap();

        register_project_mcp(project_root, "/usr/bin/glass");

        let gitignore = std::fs::read_to_string(project_root.join(".gitignore")).unwrap();
        assert!(gitignore.contains(".mcp.json"));
        assert!(gitignore.contains("node_modules/"));
    }

    #[test]
    fn test_gitignore_not_duplicated() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path();
        std::fs::write(project_root.join(".gitignore"), ".mcp.json\n").unwrap();

        register_project_mcp(project_root, "/usr/bin/glass");

        let gitignore = std::fs::read_to_string(project_root.join(".gitignore")).unwrap();
        assert_eq!(gitignore.matches(".mcp.json").count(), 1);
    }

    #[test]
    fn test_context_hint_appended() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path();
        std::fs::write(
            project_root.join("CLAUDE.md"),
            "# My Project\n\nSome instructions.\n",
        )
        .unwrap();

        inject_context_hints(project_root);

        let content = std::fs::read_to_string(project_root.join("CLAUDE.md")).unwrap();
        assert!(content.contains("glass_history"));
        assert!(content.contains("glass_context"));
    }

    #[test]
    fn test_context_hint_skipped_when_present() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path();
        let original = "# My Project\n\nUse glass_history to search.\n";
        std::fs::write(project_root.join("CLAUDE.md"), original).unwrap();

        inject_context_hints(project_root);

        let content = std::fs::read_to_string(project_root.join("CLAUDE.md")).unwrap();
        assert_eq!(content, original, "file should not be modified");
    }

    #[test]
    fn test_context_hint_skips_nonexistent_files() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path();

        inject_context_hints(project_root);

        assert!(!project_root.join("CLAUDE.md").exists());
        assert!(!project_root.join("AGENTS.md").exists());
    }
}

//! Parser for .glass/agent-instructions.md files.
//!
//! Supports optional YAML-like frontmatter with `context_files` list,
//! plus a free-form body for agent steering instructions.

use std::path::Path;

/// Parsed agent instructions file.
#[derive(Debug, Default)]
pub struct AgentInstructions {
    /// File paths listed in frontmatter `context_files:` section.
    pub context_files: Vec<String>,
    /// Free-form instruction body (everything after frontmatter).
    pub body: String,
}

/// Parse an agent-instructions.md file.
///
/// Returns `None` if the file doesn't exist or can't be read.
pub fn parse_agent_instructions(path: &Path) -> Option<AgentInstructions> {
    let content = std::fs::read_to_string(path).ok()?;
    Some(parse_content(&content))
}

/// Parse the content string (separated for testability).
fn parse_content(content: &str) -> AgentInstructions {
    // Normalize \r\n to \n for consistent parsing on Windows
    let normalized = content.replace("\r\n", "\n");
    let trimmed = normalized.trim();
    if !trimmed.starts_with("---") {
        return AgentInstructions {
            context_files: Vec::new(),
            body: content.trim().to_string(),
        };
    }

    // Find closing --- (search in the raw remainder which starts with \n)
    let after_open = &trimmed[3..]; // starts with "\n" or is empty
    let closing = after_open.find("\n---");
    let Some(closing_pos) = closing else {
        // No closing --- found, treat entire content as body
        return AgentInstructions {
            context_files: Vec::new(),
            body: content.trim().to_string(),
        };
    };

    // frontmatter is what's between the two --- markers (strip leading \n)
    let frontmatter = after_open[..closing_pos].trim_start_matches('\n');
    let body = after_open[closing_pos + 4..].trim().to_string(); // skip "\n---"

    // Hand-parse context_files from frontmatter
    let mut context_files = Vec::new();
    let mut in_context_files = false;
    for line in frontmatter.lines() {
        let line = line.trim();
        if line.starts_with("context_files:") {
            in_context_files = true;
            continue;
        }
        if in_context_files {
            if let Some(entry) = line.strip_prefix("- ") {
                context_files.push(entry.trim().to_string());
            } else if !line.is_empty() {
                in_context_files = false;
            }
        }
    }

    AgentInstructions {
        context_files,
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_no_frontmatter() {
        let content = "Focus on UI first.\nUse vanilla JS.";
        let result = parse_content(content);
        assert!(result.context_files.is_empty());
        assert_eq!(result.body, content);
    }

    #[test]
    fn parse_with_frontmatter() {
        let content = "---\ncontext_files:\n  - PRD.md\n  - docs/plan.md\n---\n\nFocus on UI.";
        let result = parse_content(content);
        assert_eq!(result.context_files, vec!["PRD.md", "docs/plan.md"]);
        assert_eq!(result.body, "Focus on UI.");
    }

    #[test]
    fn parse_frontmatter_no_context_files() {
        let content = "---\nother_field: value\n---\n\nJust instructions.";
        let result = parse_content(content);
        assert!(result.context_files.is_empty());
        assert_eq!(result.body, "Just instructions.");
    }

    #[test]
    fn parse_empty_frontmatter() {
        let content = "---\n---\n\nBody only.";
        let result = parse_content(content);
        assert!(result.context_files.is_empty());
        assert_eq!(result.body, "Body only.");
    }

    #[test]
    fn parse_no_closing_frontmatter() {
        let content = "---\ncontext_files:\n  - PRD.md\nno closing";
        let result = parse_content(content);
        assert!(result.context_files.is_empty());
        assert_eq!(result.body, content.trim());
    }

    #[test]
    fn parse_empty_body() {
        let content = "---\ncontext_files:\n  - PRD.md\n---";
        let result = parse_content(content);
        assert_eq!(result.context_files, vec!["PRD.md"]);
        assert!(result.body.is_empty());
    }

    #[test]
    fn parse_windows_crlf_line_endings() {
        let content = "---\r\ncontext_files:\r\n  - PRD.md\r\n---\r\n\r\nWindows instructions.";
        let result = parse_content(content);
        assert_eq!(result.context_files, vec!["PRD.md"]);
        assert_eq!(result.body, "Windows instructions.");
    }

    #[test]
    fn parse_file_not_found() {
        let result = parse_agent_instructions(Path::new("/nonexistent/path.md"));
        assert!(result.is_none());
    }
}

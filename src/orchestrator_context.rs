//! Context assembly for the orchestrator's initial message.
//!
//! Gathers project context from multiple sources: agent-instructions.md,
//! a configured PRD file, recently modified markdown files, and git status.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Assembled context for the orchestrator's kickoff message.
#[derive(Debug, Default)]
pub struct OrchestratorContext {
    /// Agent steering instructions (from agent-instructions.md or config fallback).
    pub instructions: String,
    /// Priority-ordered list of (path, content) pairs.
    pub files: Vec<(String, String)>,
    /// Recent terminal output lines.
    pub terminal_lines: Vec<String>,
    /// Output of `git log --oneline -10`, if in a git repo.
    pub git_log: Option<String>,
    /// Output of `git diff --stat`, if in a git repo.
    pub git_diff: Option<String>,
}

impl OrchestratorContext {
    /// Returns true if the files vec is empty.
    pub fn has_no_files(&self) -> bool {
        self.files.is_empty()
    }

    /// Formats the context into a structured message starting with `[ORCHESTRATOR_START]`.
    ///
    /// Combined file content is capped at 8,000 words. Files are included in priority
    /// order. The first file is always included in full. Subsequent files that push over
    /// budget are truncated. Files beyond the cutoff are omitted entirely.
    pub fn build_initial_message(&self) -> String {
        const WORD_BUDGET: usize = 8_000;

        let mut out = String::new();
        out.push_str("[ORCHESTRATOR_START]\n\n");

        // --- Agent Instructions ---
        if !self.instructions.is_empty() {
            out.push_str("## Agent Instructions\n\n");
            out.push_str(&self.instructions);
            out.push_str("\n\n");
        }

        // --- Project Context Files ---
        if !self.files.is_empty() {
            out.push_str("## Project Context Files\n\n");

            let mut words_used: usize = 0;
            for (idx, (path, content)) in self.files.iter().enumerate() {
                let word_count = content.split_whitespace().count();

                if idx == 0 {
                    // First file always included in full.
                    out.push_str(&format!("### {path}\n\n"));
                    out.push_str(content);
                    out.push_str("\n\n");
                    words_used += word_count;
                } else if words_used >= WORD_BUDGET {
                    // Budget exhausted — omit remaining files entirely.
                    break;
                } else {
                    let remaining = WORD_BUDGET - words_used;
                    if word_count <= remaining {
                        // Fits within budget.
                        out.push_str(&format!("### {path}\n\n"));
                        out.push_str(content);
                        out.push_str("\n\n");
                        words_used += word_count;
                    } else {
                        // Truncate to remaining budget.
                        let truncated: String = content
                            .split_whitespace()
                            .take(remaining)
                            .collect::<Vec<_>>()
                            .join(" ");
                        out.push_str(&format!("### {path}\n\n"));
                        out.push_str(&truncated);
                        out.push_str(&format!(
                            "\n\n[TRUNCATED — read full file at {path}]\n\n"
                        ));
                        words_used += remaining;
                    }
                }
            }
        }

        // --- Terminal Context ---
        if !self.terminal_lines.is_empty() {
            out.push_str("## Terminal Context\n\n");
            out.push_str("```\n");
            for line in &self.terminal_lines {
                out.push_str(line);
                out.push('\n');
            }
            out.push_str("```\n\n");
        }

        // --- Git Status ---
        let has_git = self.git_log.is_some() || self.git_diff.is_some();
        if has_git {
            out.push_str("## Git Status\n\n");
            if let Some(log) = &self.git_log {
                out.push_str("### Recent Commits\n\n```\n");
                out.push_str(log);
                out.push_str("```\n\n");
            }
            if let Some(diff) = &self.git_diff {
                out.push_str("### Diff Stat\n\n```\n");
                out.push_str(diff);
                out.push_str("```\n\n");
            }
        }

        out
    }
}

/// Gather orchestrator context from multiple sources.
///
/// Priority order for files:
/// 1. Files listed in `.glass/agent-instructions.md` frontmatter (`context_files`).
/// 2. `config_prd_path` if provided.
/// 3. Recently modified `*.md` files in project root (last 30 minutes, newest first).
///
/// Git status is only collected when a `.git` directory exists at `project_root`.
pub fn gather_context(
    project_root: &str,
    terminal_lines: Vec<String>,
    config_prd_path: Option<&str>,
    config_instructions: Option<&str>,
) -> OrchestratorContext {
    let root = Path::new(project_root);

    // --- 1. Agent instructions ---
    let instructions_path = root.join(".glass").join("agent-instructions.md");
    let parsed_instructions =
        crate::agent_instructions::parse_agent_instructions(&instructions_path);

    let instructions = if let Some(ref parsed) = parsed_instructions {
        parsed.body.clone()
    } else {
        config_instructions.unwrap_or("").to_string()
    };

    // --- Collect files in priority order, deduplicating by absolute path ---
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut files: Vec<(String, String)> = Vec::new();

    let maybe_add = |path: &Path, files: &mut Vec<(String, String)>, seen: &mut HashSet<PathBuf>| {
        let abs = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                // canonicalize fails if file doesn't exist; skip
                return;
            }
        };
        if !seen.insert(abs) {
            return; // already included
        }
        if let Ok(content) = std::fs::read_to_string(path) {
            files.push((path.to_string_lossy().into_owned(), content));
        }
    };

    // Priority 1: context_files from agent-instructions frontmatter.
    if let Some(ref parsed) = parsed_instructions {
        for cf in &parsed.context_files {
            let cf_path = if Path::new(cf).is_absolute() {
                PathBuf::from(cf)
            } else {
                root.join(cf)
            };
            maybe_add(&cf_path, &mut files, &mut seen);
        }
    }

    // Priority 2: config PRD path (caller already checked existence).
    if let Some(prd) = config_prd_path {
        let prd_path = if Path::new(prd).is_absolute() {
            PathBuf::from(prd)
        } else {
            root.join(prd)
        };
        maybe_add(&prd_path, &mut files, &mut seen);
    }

    // Priority 3: recently modified *.md files in project root (non-recursive, last 30 min).
    let recent_cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(30 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let mut recent_mds: Vec<(SystemTime, PathBuf)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(root) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if let Ok(meta) = std::fs::metadata(&p) {
                if let Ok(modified) = meta.modified() {
                    if modified >= recent_cutoff {
                        recent_mds.push((modified, p));
                    }
                }
            }
        }
    }
    // Sort newest first.
    recent_mds.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, p) in recent_mds {
        maybe_add(&p, &mut files, &mut seen);
    }

    // --- Git status ---
    let (git_log, git_diff) = if root.join(".git").exists() {
        let log = run_git_command(project_root, &["log", "--oneline", "-10"]);
        let diff = run_git_command(project_root, &["diff", "--stat"]);
        (log, diff)
    } else {
        (None, None)
    };

    OrchestratorContext {
        instructions,
        files,
        terminal_lines,
        git_log,
        git_diff,
    }
}

/// Run a git subcommand in the given directory, returning stdout on success.
fn run_git_command(cwd: &str, args: &[&str]) -> Option<String> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(args).current_dir(cwd);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd.output().ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).into_owned();
        if s.trim().is_empty() {
            None
        } else {
            Some(s)
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_initial_message_empty_context() {
        let ctx = OrchestratorContext::default();
        assert!(ctx.has_no_files());
        let msg = ctx.build_initial_message();
        assert!(msg.starts_with("[ORCHESTRATOR_START]"));
    }

    #[test]
    fn build_initial_message_with_files() {
        let ctx = OrchestratorContext {
            instructions: "Do the thing.".to_string(),
            files: vec![
                ("PRD.md".to_string(), "Product requirements.".to_string()),
                ("ARCHITECTURE.md".to_string(), "System design.".to_string()),
            ],
            terminal_lines: vec!["$ cargo build".to_string(), "   Compiling glass v0.1.0".to_string()],
            git_log: Some("abc1234 feat: initial commit".to_string()),
            git_diff: Some(" src/main.rs | 5 ++++".to_string()),
        };

        let msg = ctx.build_initial_message();
        assert!(msg.starts_with("[ORCHESTRATOR_START]"));
        assert!(msg.contains("## Agent Instructions"));
        assert!(msg.contains("## Project Context Files"));
        assert!(msg.contains("## Terminal Context"));
        assert!(msg.contains("## Git Status"));
        assert!(msg.contains("PRD.md"));
        assert!(msg.contains("ARCHITECTURE.md"));
        assert!(msg.contains("$ cargo build"));
        assert!(msg.contains("abc1234 feat: initial commit"));
    }

    #[test]
    fn build_initial_message_truncates_large_files() {
        // Generate a file with ~9000 words.
        let big_content: String = (0..9000).map(|i| format!("word{i} ")).collect();
        let ctx = OrchestratorContext {
            instructions: String::new(),
            files: vec![
                ("first.md".to_string(), "short content".to_string()),
                ("second.md".to_string(), big_content),
            ],
            terminal_lines: vec![],
            git_log: None,
            git_diff: None,
        };

        let msg = ctx.build_initial_message();
        assert!(msg.contains("[TRUNCATED — read full file at second.md]"));
    }

    #[test]
    fn has_no_files_true_when_empty() {
        let ctx = OrchestratorContext {
            instructions: "Some instructions.".to_string(),
            files: vec![],
            terminal_lines: vec![],
            git_log: None,
            git_diff: None,
        };
        assert!(ctx.has_no_files());
    }
}

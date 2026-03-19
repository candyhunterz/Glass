# Orchestrator Streamline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the orchestrator kickoff phase, auto-discover project context from multiple sources, add `.glass/agent-instructions.md` for user customization, and add a centered toast notification.

**Architecture:** The activation flow becomes: Ctrl+Shift+O → gather context (terminal + recent .md files + agent-instructions.md + prd_path) → validate context exists → spawn agent with full context as initial message. System prompt core stays hardcoded; dynamic content moves to initial message. A new centered toast renderer handles activation-blocking notifications.

**Tech Stack:** Rust, wgpu (rect renderer), glyphon (text), existing `try_spawn_agent` infrastructure.

**Spec:** `docs/superpowers/specs/2026-03-19-orchestrator-streamline-design.md`

---

### Task 1: Remove kickoff state from OrchestratorState

**Files:**
- Modify: `src/orchestrator.rs:479-482` (remove fields)
- Modify: `src/orchestrator.rs:540-545` (remove from constructor)
- Modify: `src/orchestrator.rs:590-600` (remove methods)

- [ ] **Step 1: Remove `kickoff_complete` and `last_user_keypress` fields**

In `src/orchestrator.rs`, remove from the struct:
```rust
// DELETE these two fields:
pub last_user_keypress: Option<std::time::Instant>,
pub kickoff_complete: bool,
```

And from `new()`:
```rust
// DELETE these two initializers:
last_user_keypress: None,
kickoff_complete: false,
```

- [ ] **Step 2: Remove `mark_user_keypress` and `user_recently_active` methods**

Delete lines 590-600 in `src/orchestrator.rs`:
```rust
// DELETE entire methods:
pub fn mark_user_keypress(&mut self) { ... }
pub fn user_recently_active(&self, threshold: std::time::Duration) -> bool { ... }
```

- [ ] **Step 3: Build to find all remaining references**

Run: `cargo build 2>&1`
Expected: Compiler errors at every call site referencing the removed fields/methods. Note all error locations for the next tasks.

- [ ] **Step 4: Commit**

```bash
git add src/orchestrator.rs
git commit -m "refactor: remove kickoff state from OrchestratorState"
```

---

### Task 2: Remove kickoff code paths from main.rs

**Files:**
- Modify: `src/main.rs:5559-5561` (keypress tracking)
- Modify: `src/main.rs:8083-8086` (deferred TypeText kickoff check)
- Modify: `src/main.rs:8278-8303` (kickoff guard in silence handler)

- [ ] **Step 1: Remove keypress tracking in keyboard handler**

At `src/main.rs:5559-5561`, delete:
```rust
// DELETE:
if self.orchestrator.active && !self.orchestrator.kickoff_complete {
    self.orchestrator.mark_user_keypress();
}
```

- [ ] **Step 2: Remove deferred TypeText kickoff check**

At `src/main.rs:8083-8086`, remove the kickoff branch. Keep the block-executing deferral. The code currently has two deferral conditions — remove only the kickoff one:
```rust
// DELETE this branch:
if !self.orchestrator.kickoff_complete {
    tracing::debug!("Orchestrator: deferring TypeText — kickoff not complete");
    self.orchestrator.deferred_type_text.push(text_to_type.clone());
} else if ...
```
Change the `else if` to a plain `if` so the block-executing check remains.

- [ ] **Step 3: Remove kickoff guard block in silence handler**

At `src/main.rs:8278-8303`, delete the entire kickoff block:
```rust
// DELETE entire block:
if !self.orchestrator.kickoff_complete {
    // ... all the kickoff threshold checking ...
    // ... kickoff_complete = true ...
}
```

- [ ] **Step 4: Build and verify**

Run: `cargo build 2>&1`
Expected: Clean build (no more references to removed fields).

- [ ] **Step 5: Run tests**

Run: `cargo test --workspace 2>&1`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "refactor: remove kickoff code paths from main event loop"
```

---

### Task 3: Remove KICKOFF MODE from system prompt and handoff.md

**Files:**
- Modify: `src/main.rs:1107-1134` (KICKOFF MODE text blocks)
- Modify: `src/main.rs:4814-4894` (handoff.md code)
- Modify: `src/main.rs:7306-7307` (settings overlay toggle)

- [ ] **Step 1: Remove KICKOFF MODE text from system prompt builder**

In `try_spawn_agent`, find the `kickoff_instructions` variable (around line 1107) and delete both KICKOFF MODE text blocks and the variable assignment. The system prompt should end after the RESPONSE FORMAT section — no kickoff instructions appended.

- [ ] **Step 2: Remove handoff.md read-and-delete**

At `src/main.rs:4814-4817`, delete:
```rust
// DELETE:
let handoff_path = std::path::Path::new(&current_cwd)
    .join(".glass")
    .join("handoff.md");
let handoff_note = std::fs::read_to_string(&handoff_path).ok();
```

Also delete the handoff content insertion into the handoff string (around line 4871-4874) and the file deletion (lines 4891-4894):
```rust
// DELETE:
if let Some(ref note) = handoff_note { ... }
// DELETE:
if handoff_note.is_some() && self.agent_runtime.is_some() {
    let _ = std::fs::remove_file(&handoff_path);
}
```

- [ ] **Step 3: Remove kickoff reset from settings overlay toggle**

At `src/main.rs:7306-7307`, delete:
```rust
// DELETE:
self.orchestrator.kickoff_complete = false;
self.orchestrator.last_user_keypress = None;
```

- [ ] **Step 4: Build and verify**

Run: `cargo build 2>&1`
Expected: Clean build.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "refactor: remove KICKOFF MODE prompt text and handoff.md support"
```

---

### Task 4: Add `agent_instructions` config field

**Files:**
- Modify: `crates/glass_core/src/config.rs:146-200` (OrchestratorSection struct)

- [ ] **Step 1: Add the field to OrchestratorSection**

In the `OrchestratorSection` struct, after the `ablation_sweep_interval` field, add:
```rust
/// Fallback agent instructions when .glass/agent-instructions.md doesn't exist.
#[serde(default)]
pub agent_instructions: Option<String>,
```

- [ ] **Step 2: Build and verify**

Run: `cargo build 2>&1`
Expected: Clean build. The field is `Option` with `serde(default)` so existing configs parse fine.

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace 2>&1`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/glass_core/src/config.rs
git commit -m "feat: add agent_instructions config field to OrchestratorSection"
```

---

### Task 5: Implement agent-instructions.md parser

**Files:**
- Create: `src/agent_instructions.rs`
- Modify: `src/main.rs` (add `mod agent_instructions;`)

- [ ] **Step 1: Write tests for the parser**

Create `src/agent_instructions.rs` with test module:
```rust
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

    // Find closing ---
    let after_first = &trimmed[3..];
    let after_first = after_first.trim_start_matches('\n');
    let closing = after_first.find("\n---");
    let Some(closing_pos) = closing else {
        // No closing --- found, treat entire content as body
        return AgentInstructions {
            context_files: Vec::new(),
            body: content.trim().to_string(),
        };
    };

    let frontmatter = &after_first[..closing_pos];
    let body = after_first[closing_pos + 4..].trim().to_string(); // skip "\n---"

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
            if line.starts_with("- ") {
                context_files.push(line[2..].trim().to_string());
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
        assert_eq!(result.body, content);
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
```

- [ ] **Step 2: Add module declaration**

In `src/main.rs`, add near the top with other `mod` declarations:
```rust
mod agent_instructions;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace 2>&1`
Expected: All tests pass (7 new tests).

- [ ] **Step 4: Commit**

```bash
git add src/agent_instructions.rs src/main.rs
git commit -m "feat: add agent-instructions.md parser with frontmatter support"
```

---

### Task 6: Implement context assembly function

**Files:**
- Create: `src/orchestrator_context.rs`
- Modify: `src/main.rs` (add `mod orchestrator_context;`)

- [ ] **Step 1: Create the context assembly module with tests**

Create `src/orchestrator_context.rs`:
```rust
//! Context assembly for orchestrator activation.
//!
//! Gathers project context from multiple sources: agent-instructions.md,
//! configured prd_path, auto-scanned recent .md files, terminal context,
//! and git status.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::agent_instructions;

/// Assembled orchestrator context ready for the agent's initial message.
pub struct OrchestratorContext {
    /// Agent instruction body (from .glass/agent-instructions.md or config fallback).
    pub instructions: String,
    /// Discovered context files with their contents: (relative_path, content).
    pub files: Vec<(String, String)>,
    /// Terminal context lines.
    pub terminal_lines: Vec<String>,
    /// Git log output (None if not a git repo).
    pub git_log: Option<String>,
    /// Git diff --stat output (None if not a git repo).
    pub git_diff: Option<String>,
}

impl OrchestratorContext {
    /// Returns true if no context files were discovered.
    pub fn has_no_files(&self) -> bool {
        self.files.is_empty()
    }

    /// Build the initial message string for the agent.
    pub fn build_initial_message(&self) -> String {
        let mut msg = String::from("[ORCHESTRATOR_START]\n\n");

        if !self.instructions.is_empty() {
            msg.push_str("## Agent Instructions\n");
            msg.push_str(&self.instructions);
            msg.push_str("\n\n");
        }

        if !self.files.is_empty() {
            msg.push_str("## Project Context Files\n");
            let mut word_count = 0usize;
            let budget = 8000;
            for (path, content) in &self.files {
                let file_words = content.split_whitespace().count();
                msg.push_str(&format!("### {}\n", path));
                if word_count + file_words > budget && word_count > 0 {
                    // Truncate this file
                    let remaining = budget.saturating_sub(word_count);
                    let truncated: String = content
                        .split_whitespace()
                        .take(remaining)
                        .collect::<Vec<_>>()
                        .join(" ");
                    msg.push_str(&truncated);
                    msg.push_str(&format!(
                        "\n[TRUNCATED — read full file at {}]\n\n",
                        path
                    ));
                    break; // Budget exhausted
                } else {
                    msg.push_str(content);
                    msg.push_str("\n\n");
                    word_count += file_words;
                }
            }
        }

        if !self.terminal_lines.is_empty() {
            msg.push_str("## Terminal Context (last 200 lines)\n");
            for line in &self.terminal_lines {
                msg.push_str(line);
                msg.push('\n');
            }
            msg.push('\n');
        }

        if self.git_log.is_some() || self.git_diff.is_some() {
            msg.push_str("## Git Status\n");
            if let Some(ref log) = self.git_log {
                msg.push_str("Recent commits:\n");
                msg.push_str(log.trim());
                msg.push_str("\n\n");
            }
            if let Some(ref diff) = self.git_diff {
                msg.push_str("Uncommitted changes:\n");
                msg.push_str(diff.trim());
                msg.push('\n');
            }
        }

        msg
    }
}

/// Create a `git` command with `CREATE_NO_WINDOW` on Windows.
fn git_cmd() -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd
}

/// Gather context from all sources.
///
/// `terminal_lines` should be pre-captured by the caller (requires terminal lock).
/// `config_prd_path` is the optional `prd_path` from config.
/// `config_instructions` is the optional fallback `agent_instructions` from config.
pub fn gather_context(
    project_root: &str,
    terminal_lines: Vec<String>,
    config_prd_path: Option<&str>,
    config_instructions: Option<&str>,
) -> OrchestratorContext {
    let root = Path::new(project_root);
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();
    let mut files: Vec<(String, String)> = Vec::new();
    let mut instructions = String::new();

    // 1. Read .glass/agent-instructions.md
    let instructions_path = root.join(".glass").join("agent-instructions.md");
    if let Some(parsed) = agent_instructions::parse_agent_instructions(&instructions_path) {
        instructions = parsed.body;

        // Read explicitly listed context_files (highest priority)
        for rel_path in &parsed.context_files {
            let abs_path = root.join(rel_path);
            if seen_paths.contains(&abs_path) {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&abs_path) {
                files.push((rel_path.clone(), content));
                seen_paths.insert(abs_path);
            }
        }
    } else if let Some(fallback) = config_instructions {
        instructions = fallback.to_string();
    }

    // 2. Read configured prd_path
    if let Some(prd_rel) = config_prd_path {
        let prd_abs = root.join(prd_rel);
        if !seen_paths.contains(&prd_abs) {
            if let Ok(content) = std::fs::read_to_string(&prd_abs) {
                files.push((prd_rel.to_string(), content));
                seen_paths.insert(prd_abs);
            }
        }
    }

    // 3. Auto-scan recent .md files (modified in last 30 minutes)
    let cutoff = std::time::SystemTime::now()
        - std::time::Duration::from_secs(30 * 60);
    if let Ok(entries) = std::fs::read_dir(root) {
        let mut recent: Vec<(PathBuf, std::time::SystemTime)> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "md")
                    .unwrap_or(false)
            })
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                let modified = meta.modified().ok()?;
                if modified > cutoff {
                    Some((e.path(), modified))
                } else {
                    None
                }
            })
            .collect();

        // Sort by modification time, newest first
        recent.sort_by(|a, b| b.1.cmp(&a.1));

        for (path, _) in recent {
            if seen_paths.contains(&path) {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                files.push((rel, content));
                seen_paths.insert(path);
            }
        }
    }

    // 4. Git status (only if .git exists)
    let git_dir = root.join(".git");
    let (git_log, git_diff) = if git_dir.exists() {
        let log = git_cmd()
            .args(["log", "--oneline", "-10"])
            .current_dir(project_root)
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout).ok()
                } else {
                    None
                }
            });
        let diff = git_cmd()
            .args(["diff", "--stat"])
            .current_dir(project_root)
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout).ok()
                } else {
                    None
                }
            });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_initial_message_empty_context() {
        let ctx = OrchestratorContext {
            instructions: String::new(),
            files: Vec::new(),
            terminal_lines: Vec::new(),
            git_log: None,
            git_diff: None,
        };
        assert!(ctx.has_no_files());
        let msg = ctx.build_initial_message();
        assert!(msg.starts_with("[ORCHESTRATOR_START]"));
    }

    #[test]
    fn build_initial_message_with_files() {
        let ctx = OrchestratorContext {
            instructions: "Focus on UI.".to_string(),
            files: vec![("PRD.md".to_string(), "Build a thing.".to_string())],
            terminal_lines: vec!["$ ls".to_string()],
            git_log: Some("abc123 initial commit".to_string()),
            git_diff: None,
        };
        assert!(!ctx.has_no_files());
        let msg = ctx.build_initial_message();
        assert!(msg.contains("## Agent Instructions"));
        assert!(msg.contains("Focus on UI."));
        assert!(msg.contains("### PRD.md"));
        assert!(msg.contains("Build a thing."));
        assert!(msg.contains("## Terminal Context"));
        assert!(msg.contains("## Git Status"));
    }

    #[test]
    fn build_initial_message_truncates_large_files() {
        let large_content = "word ".repeat(9000); // 9000 words, over 8000 budget
        let ctx = OrchestratorContext {
            instructions: String::new(),
            files: vec![("big.md".to_string(), large_content)],
            terminal_lines: Vec::new(),
            git_log: None,
            git_diff: None,
        };
        let msg = ctx.build_initial_message();
        assert!(msg.contains("[TRUNCATED"));
    }

    #[test]
    fn has_no_files_true_when_empty() {
        let ctx = OrchestratorContext {
            instructions: "some instructions".to_string(),
            files: Vec::new(),
            terminal_lines: vec!["$ pwd".to_string()],
            git_log: None,
            git_diff: None,
        };
        assert!(ctx.has_no_files());
    }
}
```

- [ ] **Step 2: Add module declaration**

In `src/main.rs`, add:
```rust
mod orchestrator_context;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace 2>&1`
Expected: All tests pass (4 new tests).

- [ ] **Step 4: Commit**

```bash
git add src/orchestrator_context.rs src/main.rs
git commit -m "feat: add orchestrator context assembly with multi-source discovery"
```

---

### Task 7: Add centered toast renderer

**Files:**
- Modify: `crates/glass_renderer/src/frame.rs` (new `draw_centered_toast` method)
- Modify: `src/main.rs` (Processor field + call site after draw_frame/draw_multi_pane_frame)

**Pattern:** Follow the existing overlay pattern used by `draw_config_error_overlay` (frame.rs:2225) and `draw_conflict_overlay` (frame.rs:2338). Separate method called AFTER draw_frame, using `LoadOp::Load`, single encoder + single submit for both rect and text.

- [ ] **Step 1: Add `draw_centered_toast` method to FrameRenderer**

In `crates/glass_renderer/src/frame.rs`, after `draw_conflict_overlay`, add:
```rust
/// Draw a centered toast notification on top of existing frame content.
///
/// Renders a semi-transparent dark backdrop with white text, centered on screen.
/// Must be called AFTER draw_frame/draw_multi_pane_frame (reuses rect_renderer).
pub fn draw_centered_toast(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &wgpu::TextureView,
    width: u32,
    height: u32,
    text: &str,
) {
    let w = width as f32;
    let h = height as f32;
    let (cell_width, cell_height) = self.grid_renderer.cell_size();
    let font_family = &self.grid_renderer.font_family;
    let physical_font_size = self.grid_renderer.font_size * self.grid_renderer.scale_factor;
    let metrics = Metrics::new(physical_font_size, cell_height);

    let text_width = text.len() as f32 * cell_width;
    let padding_x = cell_width * 3.0;
    let padding_y = cell_height * 0.5;
    let box_w = text_width + padding_x * 2.0;
    let box_h = cell_height + padding_y * 2.0;
    let box_x = (w - box_w) / 2.0;
    let box_y = (h - box_h) / 2.0;

    // Backdrop rect
    let toast_rects = vec![crate::rect_renderer::RectInstance {
        pos: [box_x, box_y, box_w, box_h],
        color: [0.1, 0.1, 0.1, 0.85],
    }];
    self.rect_renderer.prepare(device, queue, &toast_rects, width, height);

    // Text buffer
    let mut buffer = glyphon::Buffer::new(&mut self.glyph_cache.font_system, metrics);
    buffer.set_size(&mut self.glyph_cache.font_system, Some(box_w), Some(cell_height));
    buffer.set_text(
        &mut self.glyph_cache.font_system,
        text,
        &Attrs::new().family(Family::Name(font_family)),
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);

    let text_x = box_x + padding_x;
    let text_y = box_y + padding_y;
    let toast_areas = vec![TextArea {
        buffer: &buffer,
        left: text_x,
        top: text_y,
        scale: 1.0,
        bounds: TextBounds {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        },
        default_color: GlyphonColor::rgb(255, 255, 255),
        custom_glyphs: &[],
    }];

    self.glyph_cache.viewport.update(queue, Resolution { width, height });

    if let Err(e) = self.glyph_cache.text_renderer.prepare(
        device,
        queue,
        &mut self.glyph_cache.font_system,
        &mut self.glyph_cache.atlas,
        &self.glyph_cache.viewport,
        toast_areas,
        &mut self.glyph_cache.swash_cache,
    ) {
        tracing::warn!("Centered toast text prepare error: {:?}", e);
    }

    // Single encoder, single pass for both rect and text
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("centered_toast_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.rect_renderer.render(&mut pass, toast_rects.len() as u32);
        if let Err(e) = self.glyph_cache.text_renderer.render(
            &self.glyph_cache.atlas,
            &self.glyph_cache.viewport,
            &mut pass,
        ) {
            tracing::warn!("Centered toast text render error: {:?}", e);
        }
    }
    queue.submit([encoder.finish()]);
}
```

- [ ] **Step 2: Add `centered_toast` field to Processor**

In `src/main.rs` Processor struct:
```rust
/// Centered toast message, auto-dismisses after 5 seconds.
centered_toast: Option<(String, std::time::Instant)>,
```

Initialize as `None` in the constructor.

- [ ] **Step 3: Call `draw_centered_toast` after draw_frame/draw_multi_pane_frame**

In `src/main.rs`, at each render call site (after `draw_frame` / `draw_multi_pane_frame`, alongside the existing `draw_config_error_overlay` / `draw_conflict_overlay` calls), add:
```rust
// Centered toast (auto-dismiss after 5 seconds)
if let Some((ref msg, at)) = self.centered_toast {
    if at.elapsed().as_secs() < 5 {
        ctx.frame_renderer.draw_centered_toast(
            &ctx.renderer.device,
            &ctx.renderer.queue,
            &view,
            size.width,
            size.height,
            msg,
        );
    } else {
        self.centered_toast = None;
    }
}
```

- [ ] **Step 4: Build and verify**

Run: `cargo build 2>&1`
Expected: Clean build.

- [ ] **Step 5: Commit**

```bash
git add crates/glass_renderer/src/frame.rs src/main.rs
git commit -m "feat: add centered toast notification renderer"
```

---

### Task 8: Rewrite Ctrl+Shift+O activation flow

**Files:**
- Modify: `src/main.rs:4660-4910` (Ctrl+Shift+O handler)

This is the main integration task. Replace the entire activation block with the new context-gathering flow.

- [ ] **Step 1: Replace the orchestrator activation block**

Replace the body of `if self.orchestrator.active {` (the enable branch) with:

```rust
tracing::info!("Orchestrator: enabled by user");
self.orchestrator.reset_stuck();
self.orchestrator.iterations_since_checkpoint = 0;
self.orchestrator.bounded_stop_pending = false;
self.orchestrator.max_iterations = self
    .config
    .agent
    .as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .and_then(|o| o.max_iterations);

// Capture project root from terminal CWD
let current_cwd = ctx
    .session_mux
    .focused_session()
    .map(|s| s.status.cwd().to_string())
    .unwrap_or_else(|| {
        std::env::current_dir()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    });
self.orchestrator.project_root = current_cwd.clone();

// Load scripts for the project
self.script_bridge.load_for_project(&current_cwd);
self.script_bridge.reset_run_tracking();

// Fire scripting hook
fire_hook_on_bridge(
    &mut self.script_bridge,
    &self.orchestrator.project_root,
    glass_scripting::HookPoint::OrchestratorRunStart,
    &glass_scripting::HookEventData::new(),
);

// Capture terminal context (requires session borrow)
let terminal_lines = ctx
    .session_mux
    .focused_session()
    .map(|session| extract_term_lines(&session.term, 200))
    .unwrap_or_default();

// Gather context from all sources
// prd_path has a serde default of "PRD.md" — only pass it if the file actually exists
let config_prd_path = self
    .config
    .agent
    .as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .map(|o| o.prd_path.as_str())
    .filter(|p| std::path::Path::new(&current_cwd).join(p).exists());
let config_instructions = self
    .config
    .agent
    .as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .and_then(|o| o.agent_instructions.as_deref());

let context = orchestrator_context::gather_context(
    &current_cwd,
    terminal_lines,
    config_prd_path,
    config_instructions,
);

// Gate: require at least one context file
if context.has_no_files() {
    self.orchestrator.active = false;
    self.centered_toast = Some((
        "No project context found -- create a plan first".to_string(),
        std::time::Instant::now(),
    ));
    tracing::info!("Orchestrator: blocked — no context files found");
    ctx.mark_dirty_and_redraw();
    return;
}

self.orchestrator_activated_at = Some(std::time::Instant::now());

// Auto-detect mode (in-memory only, no config write)
let prd_content = context.files.first().map(|(_, c)| c.as_str());
let (detected_mode, detected_verify, _detected_files) =
    orchestrator::auto_detect_orchestrator_config(
        &current_cwd,
        prd_content,
    );
tracing::info!(
    "Orchestrator auto-detect: mode={}, verify={}",
    detected_mode,
    detected_verify,
);

// Build initial message
let initial_message = context.build_initial_message();

// Drop ctx borrow, then respawn
let _ = ctx;
self.respawn_orchestrator_agent(&current_cwd, initial_message);
```

Keep the existing metric guard initialization, verify command detection, and artifact watcher code that follows.

**Also remove from the old activation block:**
- The `config_write_suppress_until` guard and all three `update_config_field` calls (lines ~4760-4800) — no longer needed since we don't write to config.toml.
- The auto-generated checkpoint block (lines ~4835-4868) — context is now in the initial message.
- The handoff.md code if not already removed in Task 3.

- [ ] **Step 2: Remove inline PRD/checkpoint/iteration content from system prompt**

In `try_spawn_agent`, the orchestrator system prompt currently inlines `prd_truncated`, `checkpoint_content`, and `iterations_content`. Remove these sections — they now go in the initial message via `OrchestratorContext`. The system prompt should contain ONLY the core rules (identity, mode instructions, critical rules, response format).

- [ ] **Step 3: Apply same changes to settings overlay toggle path**

Update the settings overlay toggle (around line 7294) to use the same context-gathering flow. Extract the shared logic into a helper method if the duplication is significant.

- [ ] **Step 4: Build and verify**

Run: `cargo build 2>&1`
Expected: Clean build.

- [ ] **Step 5: Run tests**

Run: `cargo test --workspace 2>&1`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat: rewrite orchestrator activation with context-based flow"
```

---

### Task 9: Clean up feedback types

**Files:**
- Modify: `crates/glass_feedback/src/types.rs:205,301` (kickoff_duration_secs)
- Modify: `src/main.rs` (RunData construction)

- [ ] **Step 1: Hardcode kickoff_duration_secs to 0**

In `src/main.rs`, find where `RunData` is constructed (the `build_run_data` method) and ensure `kickoff_duration_secs: 0` is set. It likely already is — verify and leave unchanged for backward compat.

- [ ] **Step 2: Build and verify**

Run: `cargo build 2>&1`
Expected: Clean build.

- [ ] **Step 3: Commit** (only if changes were needed)

```bash
git add crates/glass_feedback/src/types.rs src/main.rs
git commit -m "chore: hardcode kickoff_duration_secs to 0 after kickoff removal"
```

---

### Task 10: Update generate_postmortem signature

**Files:**
- Modify: `src/orchestrator.rs:776` (generate_postmortem function)
- Modify: `src/main.rs` (call sites)

- [ ] **Step 1: Change `prd_path: &str` to `context_files: &[String]`**

In `src/orchestrator.rs`, update the `generate_postmortem` function signature:
```rust
// BEFORE:
pub fn generate_postmortem(... prd_path: &str, ...)
// AFTER:
pub fn generate_postmortem(... context_files: &[String], ...)
```

Update the body to list all context file paths in the postmortem report instead of a single PRD path.

- [ ] **Step 2: Update call sites**

Find all `generate_postmortem` call sites in `src/main.rs` and pass the list of discovered context file paths instead of the single `prd_path`.

- [ ] **Step 3: Build and verify**

Run: `cargo build 2>&1`
Expected: Clean build.

- [ ] **Step 4: Commit**

```bash
git add src/orchestrator.rs src/main.rs
git commit -m "refactor: update generate_postmortem to accept context file list"
```

---

### Task 11: Update ORCHESTRATOR.md

**Files:**
- Modify: `ORCHESTRATOR.md`

- [ ] **Step 1: Add new activation flow section**

Add the activation flow diagram from the spec to ORCHESTRATOR.md, replacing any existing kickoff documentation. Include:
- The context gathering cascade (agent-instructions.md → prd_path → auto-scan → terminal)
- The zero-context gate
- The `.glass/agent-instructions.md` format and example
- Note that `handoff.md` is no longer supported

- [ ] **Step 2: Commit**

```bash
git add ORCHESTRATOR.md
git commit -m "docs: update ORCHESTRATOR.md with streamlined activation flow"
```

---

### Task 12: Integration test — manual verification

- [ ] **Step 1: Build release binary**

Run: `cargo build --release 2>&1`

- [ ] **Step 2: Test activation with no context**

Launch Glass, open a fresh shell, press Ctrl+Shift+O. Expected: centered toast "No project context found — create a plan first", orchestrator does NOT activate.

- [ ] **Step 3: Test activation with context**

Create a test `.md` file in a project directory, press Ctrl+Shift+O. Expected: orchestrator activates, agent receives context including the file.

- [ ] **Step 4: Test agent-instructions.md**

Create `.glass/agent-instructions.md` with frontmatter listing a context file. Activate orchestrator. Expected: agent receives both the instructions and the listed file content.

- [ ] **Step 5: Test checkpoint respawn**

Trigger a checkpoint cycle. Expected: agent respawns with minimal "Resume from checkpoint" message, NOT the full context bundle.

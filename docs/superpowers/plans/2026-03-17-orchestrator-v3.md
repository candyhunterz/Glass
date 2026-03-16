# Orchestrator V3 Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the orchestrator general-purpose: auto-generate PRDs from plain language, support non-code tasks with file-based verification, simplify settings, and remove auto-pause on user input.

**Architecture:** Three changes to the existing orchestrator: (1) a kickoff flow in the Ctrl+Shift+O handler that detects missing PRDs and guides generation, (2) a general mode with a task-agnostic system prompt and file-based verification, (3) a simplified settings overlay with auto-detection. All changes build on existing infrastructure — no new crates or major refactors.

**Tech Stack:** Rust, winit, wgpu, serde, toml, regex

---

## Chunk 1: Config & Auto-Detection

### Task 1: Add verify_files to OrchestratorSection

**Files:**
- Modify: `crates/glass_core/src/config.rs:117-156`

- [ ] **Step 1: Add verify_files field to OrchestratorSection**

After `pub orchestrator_mode: String` (line 155), add:

```rust
    /// Files to check for file-based verification. Auto-populated from PRD deliverables.
    #[serde(default)]
    pub verify_files: Vec<String>,
```

- [ ] **Step 2: Add test for verify_files deserialization**

In the test module, add:

```rust
    #[test]
    fn test_orchestrator_verify_files_default_empty() {
        let toml = "[agent.orchestrator]\nenabled = true";
        let config = GlassConfig::load_from_str(toml);
        let orch = config.agent.unwrap().orchestrator.unwrap();
        assert!(orch.verify_files.is_empty());
    }

    #[test]
    fn test_orchestrator_verify_files_custom() {
        let toml = r#"[agent.orchestrator]
enabled = true
verify_files = ["plan.md", "site/index.html"]"#;
        let config = GlassConfig::load_from_str(toml);
        let orch = config.agent.unwrap().orchestrator.unwrap();
        assert_eq!(orch.verify_files, vec!["plan.md", "site/index.html"]);
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p glass_core`
Expected: All tests pass including the 2 new ones.

- [ ] **Step 4: Commit**

```bash
git add crates/glass_core/src/config.rs
git commit -m "feat(config): add verify_files to OrchestratorSection for file-based verification"
```

### Task 2: Add auto-detection and PRD deliverables parser

**Files:**
- Modify: `src/orchestrator.rs`

- [ ] **Step 1: Add PRD deliverables parser**

After the `auto_detect_verify_commands` function, add:

```rust
/// Parse the "## Deliverables" section of a PRD file to extract file paths.
///
/// Looks for markdown list items and extracts the first token that looks like
/// a file path (contains a dot or slash). E.g.:
/// - `vacation-plan.md (itinerary)` → `vacation-plan.md`
/// - `site/index.html` → `site/index.html`
pub fn parse_prd_deliverables(prd_content: &str) -> Vec<String> {
    let mut in_deliverables = false;
    let mut files = Vec::new();

    for line in prd_content.lines() {
        let trimmed = line.trim();

        // Detect section headers
        if trimmed.starts_with("## ") {
            in_deliverables = trimmed.eq_ignore_ascii_case("## deliverables")
                || trimmed.eq_ignore_ascii_case("## Deliverables");
            continue;
        }

        if !in_deliverables {
            continue;
        }

        // Parse list items: "- file.md (description)" or "- file.md"
        if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            // Extract first token that looks like a file path
            let first_token = item.split_whitespace().next().unwrap_or("");
            let first_token = first_token.trim_end_matches(|c: char| c == ',' || c == ')' || c == '(');
            if first_token.contains('.') || first_token.contains('/') || first_token.contains('\\') {
                files.push(first_token.to_string());
            }
        }
    }

    files
}

/// Auto-detect orchestrator mode and verification strategy from project + PRD.
///
/// Returns (orchestrator_mode, verify_mode, verify_files).
pub fn auto_detect_orchestrator_config(
    project_root: &str,
    prd_content: Option<&str>,
) -> (String, String, Vec<String>) {
    // 1. Check for code project markers
    let commands = auto_detect_verify_commands(project_root);
    if !commands.is_empty() {
        // Code project — use test verification
        let mode = "build".to_string();
        return (mode, "floor".to_string(), Vec::new());
    }

    // 2. Parse PRD for deliverables
    if let Some(content) = prd_content {
        let deliverables = parse_prd_deliverables(content);
        if !deliverables.is_empty() {
            return ("general".to_string(), "files".to_string(), deliverables);
        }
    }

    // 3. No markers, no deliverables — general with no verification
    ("general".to_string(), "off".to_string(), Vec::new())
}
```

- [ ] **Step 2: Add tests**

```rust
    #[test]
    fn parse_prd_deliverables_extracts_files() {
        let prd = "# Plan\n\n## Deliverables\n- vacation-plan.md (itinerary)\n- site/index.html\n- research/flights.md (top options)\n\n## Requirements\n- Budget $5000";
        let files = parse_prd_deliverables(prd);
        assert_eq!(files, vec!["vacation-plan.md", "site/index.html", "research/flights.md"]);
    }

    #[test]
    fn parse_prd_deliverables_empty_when_no_section() {
        let prd = "# Plan\n\n## Requirements\n- Do stuff";
        let files = parse_prd_deliverables(prd);
        assert!(files.is_empty());
    }

    #[test]
    fn parse_prd_deliverables_ignores_non_file_items() {
        let prd = "## Deliverables\n- A complete vacation plan\n- output.md\n- At least 3 options";
        let files = parse_prd_deliverables(prd);
        assert_eq!(files, vec!["output.md"]);
    }

    #[test]
    fn auto_detect_code_project() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let (mode, verify, files) = auto_detect_orchestrator_config(dir.path().to_str().unwrap(), None);
        assert_eq!(mode, "build");
        assert_eq!(verify, "floor");
        assert!(files.is_empty());
    }

    #[test]
    fn auto_detect_general_with_deliverables() {
        let dir = tempfile::TempDir::new().unwrap();
        let prd = "## Deliverables\n- plan.md\n- site/index.html";
        let (mode, verify, files) = auto_detect_orchestrator_config(dir.path().to_str().unwrap(), Some(prd));
        assert_eq!(mode, "general");
        assert_eq!(verify, "files");
        assert_eq!(files, vec!["plan.md", "site/index.html"]);
    }

    #[test]
    fn auto_detect_general_no_deliverables() {
        let dir = tempfile::TempDir::new().unwrap();
        let (mode, verify, files) = auto_detect_orchestrator_config(dir.path().to_str().unwrap(), None);
        assert_eq!(mode, "general");
        assert_eq!(verify, "off");
        assert!(files.is_empty());
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p glass orchestrator`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/orchestrator.rs
git commit -m "feat(orchestrator): add PRD deliverables parser and auto-detection"
```

### Task 3: Add file-based verification logic

**Files:**
- Modify: `src/orchestrator.rs`

- [ ] **Step 1: Add FileVerifyBaseline struct and check function**

After `MetricBaseline`, add:

```rust
/// Baseline for file-based verification (general mode).
/// Tracks file existence and sizes to detect regressions.
#[derive(Debug, Clone)]
pub struct FileVerifyBaseline {
    /// Map of file path → last known byte size.
    pub file_sizes: std::collections::HashMap<String, u64>,
}

impl FileVerifyBaseline {
    pub fn new() -> Self {
        Self {
            file_sizes: std::collections::HashMap::new(),
        }
    }
}

/// Check deliverable files for regression.
///
/// Returns (regressed, results_summary).
/// Regression = a file that previously existed is now missing,
/// or a file shrank by more than 50%.
pub fn check_file_verification(
    project_root: &str,
    verify_files: &[String],
    baseline: &mut FileVerifyBaseline,
) -> (bool, String) {
    let root = std::path::Path::new(project_root);
    let mut regressed = false;
    let mut summaries = Vec::new();

    for file in verify_files {
        let path = root.join(file);
        let current_size = std::fs::metadata(&path).map(|m| m.len()).ok();

        match (baseline.file_sizes.get(file), current_size) {
            (Some(&prev_size), None) => {
                // File was present, now missing — regression
                regressed = true;
                summaries.push(format!("{file}: MISSING (was {prev_size}B)"));
            }
            (Some(&prev_size), Some(curr)) if prev_size > 0 && curr < prev_size / 2 => {
                // File shrank by more than 50% — regression
                regressed = true;
                summaries.push(format!("{file}: SHRUNK ({prev_size}B -> {curr}B)"));
            }
            (_, Some(curr)) => {
                baseline.file_sizes.insert(file.clone(), curr);
                summaries.push(format!("{file}: {curr}B"));
            }
            (None, None) => {
                summaries.push(format!("{file}: not yet created"));
            }
        }
    }

    (regressed, summaries.join(", "))
}
```

- [ ] **Step 2: Add tests**

```rust
    #[test]
    fn file_verify_new_file_not_regression() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("plan.md"), "hello world").unwrap();
        let mut baseline = FileVerifyBaseline::new();
        let (regressed, _) = check_file_verification(
            dir.path().to_str().unwrap(),
            &["plan.md".to_string()],
            &mut baseline,
        );
        assert!(!regressed);
        assert_eq!(*baseline.file_sizes.get("plan.md").unwrap(), 11);
    }

    #[test]
    fn file_verify_missing_file_is_regression() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut baseline = FileVerifyBaseline::new();
        baseline.file_sizes.insert("plan.md".to_string(), 100);
        let (regressed, summary) = check_file_verification(
            dir.path().to_str().unwrap(),
            &["plan.md".to_string()],
            &mut baseline,
        );
        assert!(regressed);
        assert!(summary.contains("MISSING"));
    }

    #[test]
    fn file_verify_shrunk_file_is_regression() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("plan.md"), "x").unwrap(); // 1 byte
        let mut baseline = FileVerifyBaseline::new();
        baseline.file_sizes.insert("plan.md".to_string(), 100); // was 100 bytes
        let (regressed, summary) = check_file_verification(
            dir.path().to_str().unwrap(),
            &["plan.md".to_string()],
            &mut baseline,
        );
        assert!(regressed);
        assert!(summary.contains("SHRUNK"));
    }

    #[test]
    fn file_verify_growing_file_not_regression() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("plan.md"), "a".repeat(200)).unwrap();
        let mut baseline = FileVerifyBaseline::new();
        baseline.file_sizes.insert("plan.md".to_string(), 100);
        let (regressed, _) = check_file_verification(
            dir.path().to_str().unwrap(),
            &["plan.md".to_string()],
            &mut baseline,
        );
        assert!(!regressed);
        assert_eq!(*baseline.file_sizes.get("plan.md").unwrap(), 200);
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p glass orchestrator`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/orchestrator.rs
git commit -m "feat(orchestrator): add file-based verification for general mode"
```

## Chunk 2: General Mode System Prompt

### Task 4: Add general mode system prompt

**Files:**
- Modify: `src/main.rs:828-912` (system prompt construction in `try_spawn_agent`)

- [ ] **Step 1: Add general mode branch to system prompt construction**

In `try_spawn_agent`, find the `mode_instructions` variable (line ~828). Add a new branch for "general":

```rust
        let mode_instructions = if orch_mode == "audit" {
            // ... existing audit prompt ...
        } else if orch_mode == "general" {
            r#"ORCHESTRATOR MODE: GENERAL
You are orchestrating a general task (research, planning, design, or mixed work).

ITERATION PROTOCOL:
1. READ the PRD deliverables and requirements
2. INSTRUCT Claude Code on the next deliverable to produce
3. MONITOR progress — is Claude Code making tangible output?
4. REDIRECT if Claude Code goes off-track or stalls
5. CHECK deliverable files exist and have content
6. When all deliverables are complete, respond with GLASS_DONE

Use whatever tools are needed: web search, file creation, shell commands, code.
Track progress by deliverable completion, not test counts.
You CANNOT create files yourself — instruct Claude Code to do it."#
        } else {
            // ... existing build prompt ...
        };
```

- [ ] **Step 2: Verify compilation**

Run: `cargo build`
Expected: Compiles cleanly.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat(orchestrator): add general mode system prompt for non-code tasks"
```

### Task 5: Add kickoff instructions to system prompt

**Files:**
- Modify: `src/main.rs:778-912` (system prompt construction in `try_spawn_agent`)

- [ ] **Step 1: Detect missing PRD and add kickoff instructions**

In `try_spawn_agent`, after reading `prd_content` (line ~786), check if the PRD was not found and append kickoff instructions:

```rust
        let prd_missing = prd_content.starts_with("(PRD not found");
        let kickoff_instructions = if prd_missing {
            "\n\nKICKOFF MODE:\nNo PRD file exists yet. Your FIRST instruction to Claude Code must be:\n\"Generate a detailed PRD file. Name it descriptively based on the project goal (e.g., PRD-japan-vacation.md). Include:\n- ## Deliverables (list each output file)\n- ## Requirements (specific constraints)\n- ## Research Areas (if applicable)\nWrite it to disk, then start executing it.\"\n\nAfter Claude Code writes the PRD, continue with normal orchestration."
        } else {
            ""
        };
```

Then include `{kickoff_instructions}` in the format string after `{mode_instructions}`.

- [ ] **Step 2: Verify compilation**

Run: `cargo build`
Expected: Compiles cleanly.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat(orchestrator): add kickoff instructions when PRD is missing"
```

## Chunk 3: Kickoff Flow & Auto-Pause Removal

### Task 6: Remove auto-pause on user input

**Files:**
- Modify: `src/main.rs:4254-4258` (auto-pause handler)

- [ ] **Step 1: Remove the auto-pause block**

Find the block at line ~4254:
```rust
                        if self.orchestrator.active {
                            self.orchestrator.active = false;
                            tracing::info!("Orchestrator: auto-paused (user typing detected)");
```

Remove the entire `if self.orchestrator.active` block (including the artifact watcher cleanup inside it). Replace with a comment:

```rust
                        // Orchestrator no longer auto-pauses on user input.
                        // Only Ctrl+Shift+O toggles orchestration on/off.
```

- [ ] **Step 2: Verify compilation**

Run: `cargo build`
Expected: Compiles cleanly.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "fix(orchestrator): remove auto-pause on user input, Ctrl+Shift+O is the only toggle"
```

### Task 7: Rewrite Ctrl+Shift+O handler with kickoff flow

**Files:**
- Modify: `src/main.rs:3681-3820` (Ctrl+Shift+O handler)

- [ ] **Step 1: Add PRD existence check and prompt**

At the start of the `if self.orchestrator.active` block (the activation branch), before the existing PRD validation and agent respawn, add:

```rust
                                    // Check if PRD exists for kickoff flow
                                    let prd_rel = self
                                        .config
                                        .agent
                                        .as_ref()
                                        .and_then(|a| a.orchestrator.as_ref())
                                        .map(|o| o.prd_path.as_str())
                                        .unwrap_or("PRD.md");
                                    let prd_exists = std::path::Path::new(&current_cwd)
                                        .join(prd_rel)
                                        .exists();

                                    if prd_exists {
                                        // Prompt user: continue or start fresh?
                                        if let Some(session) = ctx.session_mux.focused_session() {
                                            let msg = format!(
                                                "\r\n[GLASS] Found existing PRD at {}. Continue with it? (y=continue, n=start fresh)\r\n",
                                                prd_rel
                                            );
                                            let bytes = msg.into_bytes();
                                            let _ = session.pty_sender.send(
                                                PtyMsg::Input(std::borrow::Cow::Owned(bytes)),
                                            );
                                        }
                                    }
```

Note: The user's response handling (y/n) happens naturally through the orchestrator's normal context capture — the Glass Agent sees the prompt and the user's response in the terminal context and acts accordingly. No special state machine needed.

For the "start fresh" path, the Glass Agent's kickoff instructions (from Task 5) handle PRD generation. The auto-detection (from Task 2) runs when the agent reports the new PRD filename.

- [ ] **Step 2: Wire auto-detection into activation**

After the agent respawn and metric guard initialization, add auto-detection:

```rust
                                    // Auto-detect orchestrator mode from project + PRD
                                    let prd_content = std::fs::read_to_string(
                                        std::path::Path::new(&current_cwd).join(prd_rel),
                                    )
                                    .ok();
                                    let (detected_mode, detected_verify, detected_files) =
                                        orchestrator::auto_detect_orchestrator_config(
                                            &current_cwd,
                                            prd_content.as_deref(),
                                        );
                                    tracing::info!(
                                        "Orchestrator auto-detect: mode={}, verify={}, files={:?}",
                                        detected_mode, detected_verify, detected_files
                                    );
```

The detected values are used by the system prompt (already reads orchestrator_mode from config) and the verify thread (already reads verify_mode from config). For now, log them; writing them back to config.toml is a follow-up enhancement.

- [ ] **Step 3: Verify compilation**

Run: `cargo build`
Expected: Compiles cleanly.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat(orchestrator): add kickoff flow with PRD existence check and auto-detection"
```

## Chunk 4: File Verification Integration

### Task 8: Wire file verification into the verify thread

**Files:**
- Modify: `src/main.rs` (OrchestratorSilence handler, verify thread section)
- Modify: `src/main.rs` (Processor struct — add file_verify_baseline field)

- [ ] **Step 1: Add FileVerifyBaseline to Processor**

In the `Processor` struct (line ~254), after `orchestrator_activated_at`, add:

```rust
    /// File-based verification baseline for general mode.
    file_verify_baseline: orchestrator::FileVerifyBaseline,
```

Initialize it in the Processor constructor:

```rust
                file_verify_baseline: orchestrator::FileVerifyBaseline::new(),
```

- [ ] **Step 2: Add file verification branch in OrchestratorSilence handler**

In the verify section of the OrchestratorSilence handler (line ~6834), after the `verify_mode == "floor"` block, add:

```rust
                        if verify_mode == "files" && !already_verified {
                            let verify_files = self
                                .config
                                .agent
                                .as_ref()
                                .and_then(|a| a.orchestrator.as_ref())
                                .map(|o| o.verify_files.clone())
                                .unwrap_or_default();
                            if !verify_files.is_empty() {
                                let (regressed, summary) =
                                    orchestrator::check_file_verification(
                                        &cwd,
                                        &verify_files,
                                        &mut self.file_verify_baseline,
                                    );
                                // Log to iterations.tsv
                                orchestrator::append_iteration_log(
                                    &cwd,
                                    self.orchestrator.iteration,
                                    "verify",
                                    if regressed { "revert" } else { "keep" },
                                    &summary,
                                );
                                if regressed {
                                    // Revert via git (same as test verification)
                                    if let Some(ref commit) = self.orchestrator.last_good_commit {
                                        let _ = std::process::Command::new("git")
                                            .args(["reset", "--hard", commit])
                                            .current_dir(&cwd)
                                            .output();
                                        tracing::info!("File verify: reverted to {commit}");
                                    }
                                }
                                self.orchestrator.last_verified_iteration =
                                    Some(self.orchestrator.iteration);
                            }
                        }
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build`
Expected: Compiles cleanly.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat(orchestrator): wire file-based verification into the orchestrator loop"
```

## Chunk 5: Settings UX Cleanup

### Task 9: Simplify orchestrator settings fields

**Files:**
- Modify: `crates/glass_renderer/src/settings_overlay.rs:72-107,912-977`

- [ ] **Step 1: Remove technical fields from SettingsConfigSnapshot**

In `SettingsConfigSnapshot` (line 72), remove these fields:
- `orchestrator_fast_trigger_secs`
- `orchestrator_prompt_pattern`
- `orchestrator_max_retries`
- `orchestrator_verify_command`
- `orchestrator_completion_artifact`

Keep: `orchestrator_enabled`, `orchestrator_silence_secs`, `orchestrator_prd_path`, `orchestrator_verify_mode`, `orchestrator_max_iterations`, `orchestrator_mode`.

Update the `Default` impl to match.

- [ ] **Step 2: Update fields_for_section for orchestrator**

Replace the orchestrator section (index 6) in `fields_for_section` with:

```rust
            6 => vec![
                // Orchestrator — 3 editable + 4 display-only
                (
                    "Enabled",
                    if config.orchestrator_enabled { "ON".to_string() } else { "OFF".to_string() },
                    true,
                ),
                (
                    "Max Iterations",
                    if config.orchestrator_max_iterations == 0 {
                        "unlimited".to_string()
                    } else {
                        format!("{}", config.orchestrator_max_iterations)
                    },
                    false,
                ),
                (
                    "Silence Timeout (sec)",
                    format!("{}", config.orchestrator_silence_secs),
                    false,
                ),
                // Display-only fields below
                ("PRD Path", config.orchestrator_prd_path.clone(), false),
                ("Mode", config.orchestrator_mode.clone(), false),
                ("Verify Mode", config.orchestrator_verify_mode.clone(), false),
            ],
```

- [ ] **Step 3: Update SettingsConfigSnapshot construction in main.rs**

Find where `SettingsConfigSnapshot` is constructed (line ~2902 in main.rs) and remove the deleted fields. Add any missing ones.

- [ ] **Step 4: Verify compilation**

Run: `cargo build`
Expected: Compiles cleanly.

- [ ] **Step 5: Commit**

```bash
git add crates/glass_renderer/src/settings_overlay.rs src/main.rs
git commit -m "feat(settings): simplify orchestrator to 3 editable + 3 display-only fields"
```

### Task 10: Fix settings handlers

**Files:**
- Modify: `src/main.rs:8296-8500` (handle_settings_activate and handle_settings_increment)

- [ ] **Step 1: Update handle_settings_activate for new field indices**

The orchestrator section (6, N) activate handlers need to match the new field layout:
- (6, 0) → Enabled toggle (keep as-is)
- Remove (6, 6) → verify_mode toggle (was causing hang)
- Remove (6, 10) → orchestrator_mode toggle (now auto-detected)

- [ ] **Step 2: Update handle_settings_increment for new field indices**

New field layout:
- (6, 1) → Max Iterations: step 10, min 0 (was step 5 at index 9)
- (6, 2) → Silence Timeout: step 10, min 10, max 300 (was step 5 at index 1)
- Remove: fast_trigger (was 6,2), max_retries (was 6,5)

```rust
        // Orchestrator max_iterations: step 10
        (6, 1) => {
            let current = config
                .agent
                .as_ref()
                .and_then(|a| a.orchestrator.as_ref())
                .and_then(|o| o.max_iterations)
                .unwrap_or(0) as i64;
            let new_val = (current + delta * 10).max(0);
            Some((
                Some("agent.orchestrator"),
                "max_iterations",
                new_val.to_string(),
            ))
        }
        // Orchestrator silence_timeout_secs: step 10
        (6, 2) => {
            let current = config
                .agent
                .as_ref()
                .and_then(|a| a.orchestrator.as_ref())
                .map(|o| o.silence_timeout_secs)
                .unwrap_or(60) as i64;
            let new_val = (current + delta * 10).clamp(10, 300);
            Some((
                Some("agent.orchestrator"),
                "silence_timeout_secs",
                new_val.to_string(),
            ))
        }
```

- [ ] **Step 3: Fix config reload hang**

In the `AppEvent::ConfigReloaded` handler (line ~5731), in the `agent_config_changed` block, before `self.agent_runtime = None;`, add:

```rust
                        // Clear response_pending to prevent hang if a verify thread is in-flight
                        self.orchestrator.response_pending = false;
```

- [ ] **Step 4: Update tests**

Fix any test that references the old settings field indices or removed SettingsConfigSnapshot fields.

- [ ] **Step 5: Run all tests**

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 6: Run clippy and fmt**

Run: `cargo clippy --workspace -- -D warnings && cargo fmt --all -- --check`
Expected: Clean.

- [ ] **Step 7: Commit**

```bash
git add src/main.rs crates/glass_renderer/src/settings_overlay.rs src/tests.rs
git commit -m "fix(settings): update orchestrator field indices, remove broken toggles, fix config reload hang"
```

---

## Summary

| Task | Component | Files |
|------|-----------|-------|
| 1 | Config: verify_files field | `config.rs` |
| 2 | Auto-detection + PRD parser | `orchestrator.rs` |
| 3 | File verification logic | `orchestrator.rs` |
| 4 | General mode system prompt | `main.rs` |
| 5 | Kickoff instructions | `main.rs` |
| 6 | Remove auto-pause | `main.rs` |
| 7 | Kickoff flow in Ctrl+Shift+O | `main.rs` |
| 8 | Wire file verification | `main.rs` |
| 9 | Settings overlay cleanup | `settings_overlay.rs`, `main.rs` |
| 10 | Settings handlers + hang fix | `main.rs`, `settings_overlay.rs`, `tests.rs` |

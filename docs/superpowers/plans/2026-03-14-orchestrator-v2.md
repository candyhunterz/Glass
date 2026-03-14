# Orchestrator V2 Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add metric guard (auto-revert on regression), artifact-based completion, and bounded iteration mode to the Glass orchestrator.

**Architecture:** Three features layered onto the existing orchestrator loop. Metric guard runs verification commands on a background thread after each iteration and auto-reverts via git on regression. Artifact completion uses a `notify` file watcher to trigger the orchestrator instantly. Bounded iterations checkpoint-stop after N iterations with a summary.

**Tech Stack:** Rust, existing `notify` crate, existing SOI pipeline, existing `git` CLI integration, existing `begin_checkpoint()` flow.

**Spec:** `docs/superpowers/specs/2026-03-14-orchestrator-v2-design.md`

---

## Chunk 1: Prerequisite + Config + Data Structures

### Task 0: Fix `update_config_field` Dotted Path Bug

**Files:**
- Modify: `crates/glass_core/src/config.rs:425-460`

This is a pre-existing bug: `update_config_field()` treats `"agent.orchestrator"` as a flat key instead of traversing the nested table path `agent -> orchestrator`. Must be fixed before adding new orchestrator settings.

- [ ] **Step 1: Write failing test**

Add to the tests module in `config.rs`:

```rust
#[test]
fn test_update_config_field_dotted_section() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[agent.orchestrator]\nenabled = true\n").unwrap();

    update_config_field(&path, Some("agent.orchestrator"), "silence_timeout_secs", "15").unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    let config = GlassConfig::load_from_str(&content);
    let orch = config.agent.unwrap().orchestrator.unwrap();
    assert_eq!(orch.silence_timeout_secs, 15);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --package glass_core test_update_config_field_dotted`
Expected: FAIL or incorrect behavior (flat key created instead of nested).

- [ ] **Step 3: Fix `update_config_field` to traverse dotted paths**

In `crates/glass_core/src/config.rs`, replace the section handling block (around line 448-454):

```rust
if let Some(section_name) = section {
    // Traverse dotted section names (e.g., "agent.orchestrator" -> agent -> orchestrator)
    let parts: Vec<&str> = section_name.split('.').collect();
    let mut current = table as &mut toml::map::Map<String, toml::Value>;
    for part in &parts {
        let entry = current
            .entry(part.to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        current = entry
            .as_table_mut()
            .ok_or_else(|| ConfigError {
                message: format!("Config section '{part}' is not a table"),
                line: None,
                column: None,
                snippet: None,
            })?;
    }
    current.insert(key.to_string(), parsed_value);
} else {
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --package glass_core test_update_config_field`
Expected: All config tests PASS.

- [ ] **Step 5: Run clippy**

Run: `cargo clippy --package glass_core -- -D warnings`

- [ ] **Step 6: Commit**

```bash
git add crates/glass_core/src/config.rs
git commit -m "fix(config): traverse dotted section paths in update_config_field

Previously treated 'agent.orchestrator' as a flat key. Now splits on '.'
and traverses nested TOML tables correctly."
```

---

### Task 1: Add V2 Config Fields to OrchestratorSection

**Files:**
- Modify: `crates/glass_core/src/config.rs:117-155`

- [ ] **Step 1: Write tests for new config fields**

```rust
#[test]
fn test_orchestrator_v2_fields_defaults() {
    let toml = "[agent.orchestrator]\nenabled = true";
    let config = GlassConfig::load_from_str(toml);
    let orch = config.agent.unwrap().orchestrator.unwrap();
    assert_eq!(orch.verify_mode, "floor");
    assert!(orch.verify_command.is_none());
    assert_eq!(orch.completion_artifact, ".glass/done");
    assert!(orch.max_iterations.is_none());
}

#[test]
fn test_orchestrator_v2_fields_custom() {
    let toml = r#"[agent.orchestrator]
enabled = true
verify_mode = "disabled"
verify_command = "cargo test"
completion_artifact = ".build/complete"
max_iterations = 25"#;
    let config = GlassConfig::load_from_str(toml);
    let orch = config.agent.unwrap().orchestrator.unwrap();
    assert_eq!(orch.verify_mode, "disabled");
    assert_eq!(orch.verify_command.as_deref(), Some("cargo test"));
    assert_eq!(orch.completion_artifact, ".build/complete");
    assert_eq!(orch.max_iterations, Some(25));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --package glass_core test_orchestrator_v2`
Expected: FAIL — fields don't exist.

- [ ] **Step 3: Add fields to OrchestratorSection**

Add to the struct after `agent_prompt_pattern`:

```rust
/// Verification mode: "floor" (default) or "disabled".
#[serde(default = "default_orch_verify_mode")]
pub verify_mode: String,
/// Optional user-override verification command. Overrides auto-detect + agent discovery.
#[serde(default)]
pub verify_command: Option<String>,
/// File path (relative to CWD) that triggers orchestrator when created. Default ".glass/done".
#[serde(default = "default_orch_completion_artifact")]
pub completion_artifact: String,
/// Maximum iterations before checkpoint-stop. None = unlimited.
#[serde(default)]
pub max_iterations: Option<u32>,
```

Add default functions:

```rust
fn default_orch_verify_mode() -> String {
    "floor".to_string()
}
fn default_orch_completion_artifact() -> String {
    ".glass/done".to_string()
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --package glass_core test_orchestrator`
Expected: All PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/glass_core/src/config.rs
git commit -m "feat(config): add verify_mode, verify_command, completion_artifact, max_iterations to orchestrator config"
```

---

### Task 2: Add Metric Guard Data Structures + Auto-Detect

**Files:**
- Modify: `src/orchestrator.rs`

- [ ] **Step 1: Write tests for data structures and auto-detect**

```rust
#[test]
fn verify_result_no_regression_same_counts() {
    let baseline = vec![VerifyResult {
        command_name: "test".to_string(),
        exit_code: 0,
        tests_passed: Some(10),
        tests_failed: Some(0),
        errors: vec![],
    }];
    let current = vec![VerifyResult {
        command_name: "test".to_string(),
        exit_code: 0,
        tests_passed: Some(10),
        tests_failed: Some(0),
        errors: vec![],
    }];
    assert!(!MetricBaseline::check_regression(&baseline, &current));
}

#[test]
fn verify_result_regression_on_fail_increase() {
    let baseline = vec![VerifyResult {
        command_name: "test".to_string(),
        exit_code: 0,
        tests_passed: Some(10),
        tests_failed: Some(0),
        errors: vec![],
    }];
    let current = vec![VerifyResult {
        command_name: "test".to_string(),
        exit_code: 1,
        tests_passed: Some(8),
        tests_failed: Some(2),
        errors: vec!["error".to_string()],
    }];
    assert!(MetricBaseline::check_regression(&baseline, &current));
}

#[test]
fn verify_result_no_regression_on_added_tests() {
    let baseline = vec![VerifyResult {
        command_name: "test".to_string(),
        exit_code: 0,
        tests_passed: Some(10),
        tests_failed: Some(0),
        errors: vec![],
    }];
    let current = vec![VerifyResult {
        command_name: "test".to_string(),
        exit_code: 0,
        tests_passed: Some(15),
        tests_failed: Some(0),
        errors: vec![],
    }];
    assert!(!MetricBaseline::check_regression(&baseline, &current));
}

#[test]
fn verify_result_regression_on_build_failure() {
    let baseline = vec![VerifyResult {
        command_name: "build".to_string(),
        exit_code: 0,
        tests_passed: None,
        tests_failed: None,
        errors: vec![],
    }];
    let current = vec![VerifyResult {
        command_name: "build".to_string(),
        exit_code: 1,
        tests_passed: None,
        tests_failed: None,
        errors: vec!["compile error".to_string()],
    }];
    assert!(MetricBaseline::check_regression(&baseline, &current));
}

#[test]
fn auto_detect_rust_project() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
    let cmds = auto_detect_verify_commands(dir.path().to_str().unwrap());
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].cmd, "cargo test");
}

#[test]
fn auto_detect_no_project_returns_empty() {
    let dir = tempfile::TempDir::new().unwrap();
    let cmds = auto_detect_verify_commands(dir.path().to_str().unwrap());
    assert!(cmds.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --package glass orchestrator::tests::verify`
Expected: FAIL — types don't exist.

- [ ] **Step 3: Implement data structures**

Add to `src/orchestrator.rs` before `OrchestratorState`:

```rust
/// A verification command with its name and command string.
#[derive(Debug, Clone)]
pub struct VerifyCommand {
    pub name: String,
    pub cmd: String,
}

/// Result of running a verification command.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub command_name: String,
    pub exit_code: i32,
    pub tests_passed: Option<u32>,
    pub tests_failed: Option<u32>,
    pub errors: Vec<String>,
}

/// Tracks verification baseline and results across iterations.
pub struct MetricBaseline {
    pub commands: Vec<VerifyCommand>,
    pub baseline_results: Vec<VerifyResult>,
    pub last_results: Vec<VerifyResult>,
    pub keep_count: u32,
    pub revert_count: u32,
}

impl MetricBaseline {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            baseline_results: Vec::new(),
            last_results: Vec::new(),
            keep_count: 0,
            revert_count: 0,
        }
    }

    /// Check if current results represent a regression from baseline.
    /// Regression = pass count dropped, fail count increased, or exit code went from 0 to non-zero.
    pub fn check_regression(baseline: &[VerifyResult], current: &[VerifyResult]) -> bool {
        for (b, c) in baseline.iter().zip(current.iter()) {
            // Build broke (exit code regressed)
            if b.exit_code == 0 && c.exit_code != 0 {
                return true;
            }
            // Test pass count dropped
            if let (Some(bp), Some(cp)) = (b.tests_passed, c.tests_passed) {
                if cp < bp {
                    return true;
                }
            }
            // Test fail count increased
            if let (Some(bf), Some(cf)) = (b.tests_failed, c.tests_failed) {
                if cf > bf {
                    return true;
                }
            }
        }
        false
    }

    /// Update baseline when tests are added (floor rises).
    pub fn update_baseline_if_improved(&mut self, current: &[VerifyResult]) {
        for (b, c) in self.baseline_results.iter_mut().zip(current.iter()) {
            if let (Some(bp), Some(cp)) = (b.tests_passed, c.tests_passed) {
                // Only raise floor if fail count is also known and didn't increase
                if let (Some(bf), Some(cf)) = (b.tests_failed, c.tests_failed) {
                    if cp > bp && cf <= bf {
                        b.tests_passed = Some(cp);
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 4: Implement auto_detect_verify_commands**

```rust
/// Auto-detect verification commands based on project marker files.
pub fn auto_detect_verify_commands(project_root: &str) -> Vec<VerifyCommand> {
    let root = std::path::Path::new(project_root);

    // Check in priority order, return first match
    if root.join("Cargo.toml").exists() {
        return vec![VerifyCommand {
            name: "cargo test".to_string(),
            cmd: "cargo test".to_string(),
        }];
    }

    if root.join("package.json").exists() {
        // Check if package.json has a "test" script
        if let Ok(content) = std::fs::read_to_string(root.join("package.json")) {
            if content.contains("\"test\"") {
                return vec![VerifyCommand {
                    name: "npm test".to_string(),
                    cmd: "npm test".to_string(),
                }];
            }
        }
    }

    if root.join("pyproject.toml").exists() || root.join("setup.py").exists() {
        return vec![VerifyCommand {
            name: "pytest".to_string(),
            cmd: "pytest".to_string(),
        }];
    }

    if root.join("go.mod").exists() {
        return vec![VerifyCommand {
            name: "go test".to_string(),
            cmd: "go test ./...".to_string(),
        }];
    }

    if root.join("tsconfig.json").exists() {
        return vec![VerifyCommand {
            name: "tsc".to_string(),
            cmd: "npx tsc --noEmit".to_string(),
        }];
    }

    if root.join("Makefile").exists() {
        if let Ok(content) = std::fs::read_to_string(root.join("Makefile")) {
            if content.contains("test:") {
                return vec![VerifyCommand {
                    name: "make test".to_string(),
                    cmd: "make test".to_string(),
                }];
            }
        }
    }

    // No test framework detected
    Vec::new()
}
```

- [ ] **Step 5: Add GLASS_VERIFY parsing to parse_agent_response**

Add the `Verify` variant to `AgentResponse`:

```rust
pub enum AgentResponse {
    TypeText(String),
    Wait,
    Checkpoint { completed: String, next: String },
    Done { summary: String },
    /// Agent discovered additional verification commands.
    Verify { commands: Vec<VerifyCommand> },
}
```

Add parsing in `parse_agent_response()`, before the default TypeText fallback:

```rust
let verify_marker = "GLASS_VERIFY:";
if let Some(start) = trimmed.find(verify_marker) {
    let after = trimmed[start + verify_marker.len()..].trim();
    if let Some(json_start) = after.find('{') {
        let json_slice = &after[json_start..];
        let mut depth = 0usize;
        let mut end = None;
        for (i, ch) in json_slice.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end = Some(i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Some(end_idx) = end {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_slice[..end_idx]) {
                if let Some(cmds) = val.get("commands").and_then(|c| c.as_array()) {
                    let commands: Vec<VerifyCommand> = cmds
                        .iter()
                        .filter_map(|c| {
                            let name = c.get("name")?.as_str()?.to_string();
                            let cmd = c.get("cmd")?.as_str()?.to_string();
                            Some(VerifyCommand { name, cmd })
                        })
                        .collect();
                    if !commands.is_empty() {
                        return AgentResponse::Verify { commands };
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 6: Add MetricBaseline field to OrchestratorState**

Add to `OrchestratorState`:

```rust
/// Metric guard: verification baseline and results tracking.
pub metric_baseline: Option<MetricBaseline>,
/// Git commit SHA at the start of the current iteration (for revert).
pub last_good_commit: Option<String>,
```

Initialize in `new()`:

```rust
metric_baseline: None,
last_good_commit: None,
```

- [ ] **Step 7: Add test for GLASS_VERIFY parsing**

```rust
#[test]
fn parse_verify_commands() {
    let raw = r#"GLASS_VERIFY: {"commands": [{"name": "integration", "cmd": "./test.sh"}]}"#;
    match parse_agent_response(raw) {
        AgentResponse::Verify { commands } => {
            assert_eq!(commands.len(), 1);
            assert_eq!(commands[0].name, "integration");
            assert_eq!(commands[0].cmd, "./test.sh");
        }
        other => panic!("Expected Verify, got {:?}", other),
    }
}
```

- [ ] **Step 8: Run all tests**

Run: `cargo test --package glass orchestrator::tests`
Expected: All PASS.

- [ ] **Step 9: Run clippy**

Run: `cargo clippy --package glass -- -D warnings`

- [ ] **Step 10: Commit**

```bash
git add src/orchestrator.rs
git commit -m "feat(orchestrator): add metric guard data structures, auto-detect, and GLASS_VERIFY parsing

VerifyCommand, VerifyResult, MetricBaseline with regression detection.
auto_detect_verify_commands for Rust, Node, Python, Go, Make projects.
GLASS_VERIFY agent response variant for dynamic command discovery."
```

---

### Task 3: Add Bounded Iteration + Summary Builder to OrchestratorState

**Files:**
- Modify: `src/orchestrator.rs`

- [ ] **Step 1: Write tests**

```rust
#[test]
fn bounded_stop_when_limit_reached() {
    let mut state = OrchestratorState::new(3);
    state.max_iterations = Some(10);
    state.iteration = 9;
    assert!(!state.should_stop_bounded());
    state.iteration = 10;
    assert!(state.should_stop_bounded());
}

#[test]
fn bounded_stop_unlimited() {
    let mut state = OrchestratorState::new(3);
    state.max_iterations = None;
    state.iteration = 1000;
    assert!(!state.should_stop_bounded());
}

#[test]
fn bounded_stop_zero_means_unlimited() {
    let mut state = OrchestratorState::new(3);
    state.max_iterations = Some(0);
    assert!(!state.should_stop_bounded());
}

#[test]
fn summary_with_metric_guard() {
    let mut baseline = MetricBaseline::new();
    baseline.keep_count = 12;
    baseline.revert_count = 3;
    baseline.baseline_results = vec![VerifyResult {
        command_name: "test".to_string(),
        exit_code: 0,
        tests_passed: Some(45),
        tests_failed: Some(0),
        errors: vec![],
    }];
    baseline.last_results = vec![VerifyResult {
        command_name: "test".to_string(),
        exit_code: 0,
        tests_passed: Some(52),
        tests_failed: Some(0),
        errors: vec![],
    }];
    let summary = build_bounded_summary(25, Some(&baseline), ".glass/checkpoint.md");
    assert!(summary.contains("25/25"));
    assert!(summary.contains("12 kept"));
    assert!(summary.contains("3 reverted"));
    assert!(summary.contains("45"));
    assert!(summary.contains("52"));
}

#[test]
fn summary_without_metric_guard() {
    let summary = build_bounded_summary(10, None, ".glass/checkpoint.md");
    assert!(summary.contains("10/10"));
    assert!(!summary.contains("Metric guard"));
    assert!(summary.contains("checkpoint"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --package glass orchestrator::tests::bounded`
Expected: FAIL.

- [ ] **Step 3: Implement bounded iteration + summary**

Add `max_iterations` field to `OrchestratorState`:

```rust
/// Maximum iterations before checkpoint-stop. None or Some(0) = unlimited.
pub max_iterations: Option<u32>,
/// Whether bounded stop has been triggered (deactivate after checkpoint completes).
pub bounded_stop_pending: bool,
```

Initialize in `new()`:

```rust
max_iterations: None,
bounded_stop_pending: false,
```

Add method:

```rust
pub fn should_stop_bounded(&self) -> bool {
    self.max_iterations
        .filter(|&max| max > 0)
        .map(|max| self.iteration >= max)
        .unwrap_or(false)
}
```

Add free function:

```rust
/// Build the summary string for a bounded run completion.
pub fn build_bounded_summary(
    iterations: u32,
    metric_baseline: Option<&MetricBaseline>,
    checkpoint_path: &str,
) -> String {
    let mut summary = format!(
        "[GLASS_ORCHESTRATOR] Bounded run complete ({iterations}/{iterations} iterations)\n"
    );

    if let Some(baseline) = metric_baseline {
        if !baseline.commands.is_empty() {
            summary.push_str(&format!(
                "  Metric guard: {} kept, {} reverted\n",
                baseline.keep_count, baseline.revert_count
            ));
            // Show test counts from first command if available
            if let (Some(b), Some(c)) = (
                baseline.baseline_results.first(),
                baseline.last_results.first(),
            ) {
                if let (Some(bp), Some(cp)) = (b.tests_passed, c.tests_passed) {
                    summary.push_str(&format!(
                        "  Baseline: {} tests \u{2192} Current: {} tests\n",
                        bp, cp
                    ));
                }
            }
        }
    }

    summary.push_str(&format!("  Last checkpoint: {checkpoint_path}\n"));
    summary.push_str("  To resume: enable orchestrator (Ctrl+Shift+O)\n");
    summary
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --package glass orchestrator::tests`
Expected: All PASS.

- [ ] **Step 5: Commit**

```bash
git add src/orchestrator.rs
git commit -m "feat(orchestrator): add bounded iteration mode with summary builder

should_stop_bounded() checks iteration limit. build_bounded_summary()
generates completion summary with optional metric guard stats."
```

---

## Chunk 2: Settings Overlay + Artifact Watcher

### Task 4: Settings Overlay — Add 4 New Fields

**Files:**
- Modify: `crates/glass_renderer/src/settings_overlay.rs`
- Modify: `src/main.rs` (config snapshot + settings handlers)

- [ ] **Step 1: Add fields to SettingsConfigSnapshot**

In `settings_overlay.rs`, add after `orchestrator_prompt_pattern`:

```rust
pub orchestrator_verify_mode: String,
pub orchestrator_verify_command: String,
pub orchestrator_completion_artifact: String,
pub orchestrator_max_iterations: u32,
```

Update `Default` impl:

```rust
orchestrator_verify_mode: "floor".to_string(),
orchestrator_verify_command: String::new(),
orchestrator_completion_artifact: ".glass/done".to_string(),
orchestrator_max_iterations: 0,
```

- [ ] **Step 2: Update fields_for_section index 6**

Replace the Orchestrator section in `fields_for_section()` to add 4 new fields (indices 6-9) after the existing 6 (indices 0-5):

Append to the existing vec:

```rust
(
    "Verify Mode",
    config.orchestrator_verify_mode.clone(),
    false,
),
(
    "Verify Command",
    if config.orchestrator_verify_command.is_empty() {
        "(auto-detect)".to_string()
    } else {
        config.orchestrator_verify_command.clone()
    },
    false,
),
(
    "Completion Artifact",
    config.orchestrator_completion_artifact.clone(),
    false,
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
```

- [ ] **Step 3: Update config snapshot builder in main.rs**

Where `SettingsConfigSnapshot` is constructed, add:

```rust
orchestrator_verify_mode: self
    .config.agent.as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .map(|o| o.verify_mode.clone())
    .unwrap_or_else(|| "floor".to_string()),
orchestrator_verify_command: self
    .config.agent.as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .and_then(|o| o.verify_command.clone())
    .unwrap_or_default(),
orchestrator_completion_artifact: self
    .config.agent.as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .map(|o| o.completion_artifact.clone())
    .unwrap_or_else(|| ".glass/done".to_string()),
orchestrator_max_iterations: self
    .config.agent.as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .and_then(|o| o.max_iterations)
    .unwrap_or(0),
```

- [ ] **Step 4: Add Verify Mode toggle to handle_settings_activate**

Add to `handle_settings_activate()`:

```rust
// Orchestrator: verify_mode (toggle floor <-> disabled)
(6, 6) => {
    let current = config
        .agent.as_ref()
        .and_then(|a| a.orchestrator.as_ref())
        .map(|o| o.verify_mode.as_str())
        .unwrap_or("floor");
    let new_mode = if current == "floor" {
        "\"disabled\""
    } else {
        "\"floor\""
    };
    Some((Some("agent.orchestrator"), "verify_mode", new_mode.to_string()))
}
```

- [ ] **Step 5: Add Max Iterations increment to handle_settings_increment**

Add to `handle_settings_increment()`:

```rust
// Orchestrator max_iterations: step 5
(6, 9) => {
    let current = config
        .agent.as_ref()
        .and_then(|a| a.orchestrator.as_ref())
        .and_then(|o| o.max_iterations)
        .unwrap_or(0) as i64;
    let new_val = (current + delta * 5).max(0);
    if new_val == 0 {
        // 0 means unlimited — remove the key so it defaults to None
        Some((Some("agent.orchestrator"), "max_iterations", "0".to_string()))
    } else {
        Some((Some("agent.orchestrator"), "max_iterations", new_val.to_string()))
    }
}
```

- [ ] **Step 6: Build and test**

Run: `cargo build && cargo test --workspace`
Expected: All pass.

- [ ] **Step 7: Commit**

```bash
git add crates/glass_renderer/src/settings_overlay.rs src/main.rs
git commit -m "feat(settings): add Verify Mode, Verify Command, Completion Artifact, Max Iterations to orchestrator settings"
```

---

### Task 5: Artifact Completion Watcher

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 0: Add `notify` dependency to the binary crate**

The `notify` crate is used by `glass_core` and `glass_snapshot` but not the main binary crate. Add it to the root `Cargo.toml` dependencies:

```toml
notify = "8.0"
```

- [ ] **Step 1: Add `artifact_watcher_thread` field to Processor**

Find the `Processor` struct definition in `main.rs` and add:

```rust
/// Thread handle for the artifact completion watcher (if active).
artifact_watcher_thread: Option<std::thread::JoinHandle<()>>,
```

Initialize to `None` in `Processor`'s constructor.

- [ ] **Step 2: Create `start_artifact_watcher` helper**

Add a method on `Processor` (or a free function) that spawns the artifact watcher thread:

```rust
fn start_artifact_watcher(
    artifact_path: &str,
    cwd: &str,
    proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    window_id: WindowId,
    session_id: SessionId,
) -> Option<std::thread::JoinHandle<()>> {
    if artifact_path.is_empty() {
        return None;
    }
    let full_path = std::path::PathBuf::from(cwd).join(artifact_path);
    let target_filename = full_path.file_name()?.to_owned();
    let watch_dir = full_path.parent()?.to_path_buf();

    // Ensure parent directory exists
    let _ = std::fs::create_dir_all(&watch_dir);

    let handle = std::thread::Builder::new()
        .name("Glass artifact watcher".into())
        .spawn(move || {
            use notify::{recommended_watcher, RecursiveMode, Watcher};
            let proxy_clone = proxy;
            let target = target_filename;
            let mut watcher = match recommended_watcher(move |res: Result<notify::Event, _>| {
                if let Ok(ev) = res {
                    let dominated_by_match = ev.paths.iter().any(|p| {
                        p.file_name().map(|n| n == target.as_os_str()).unwrap_or(false)
                    });
                    if dominated_by_match {
                        let _ = proxy_clone.send_event(AppEvent::OrchestratorSilence {
                            window_id,
                            session_id,
                        });
                    }
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("Failed to create artifact watcher: {e}");
                    return;
                }
            };
            if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
                tracing::warn!("Failed to watch artifact dir: {e}");
                return;
            }
            std::thread::park(); // Keep alive until unparked
        })
        .ok()?;

    Some(handle)
}
```

- [ ] **Step 3: Start watcher when orchestrator is enabled**

Find where `self.orchestrator.active = true` is set (Ctrl+Shift+O handler). After it, start the watcher:

```rust
// Start artifact watcher
let artifact_path = self.config.agent.as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .map(|o| o.completion_artifact.clone())
    .unwrap_or_else(|| ".glass/done".to_string());
let cwd = self.get_focused_cwd();
if let Some(ctx) = self.windows.values().next() {
    if let Some(session) = ctx.session_mux.focused_session() {
        let proxy = /* event loop proxy clone */;
        self.artifact_watcher_thread = start_artifact_watcher(
            &artifact_path, &cwd, proxy, window_id, session.id(),
        );
    }
}
```

- [ ] **Step 4: Stop watcher when orchestrator is disabled**

Where `self.orchestrator.active = false` is set, stop the watcher:

```rust
if let Some(handle) = self.artifact_watcher_thread.take() {
    handle.thread().unpark();
    let _ = handle.join();
}
```

- [ ] **Step 5: Add artifact cleanup in OrchestratorSilence handler**

In the OrchestratorSilence handler, after the context is processed, clean up the artifact file:

```rust
// Clean up artifact file if it exists (one-shot signal)
let artifact_path_cfg = self.config.agent.as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .map(|o| o.completion_artifact.clone())
    .unwrap_or_default();
if !artifact_path_cfg.is_empty() {
    let full = std::path::Path::new(&cwd).join(&artifact_path_cfg);
    if full.exists() {
        let _ = std::fs::remove_file(&full);
    }
}
```

- [ ] **Step 6: Build and test**

Run: `cargo build && cargo test --workspace`
Expected: All pass.

- [ ] **Step 7: Commit**

```bash
git add src/main.rs
git commit -m "feat(orchestrator): add artifact-based completion watcher

Spawns a notify file watcher for completion_artifact path.
File creation/modification triggers OrchestratorSilence instantly.
Watcher lifecycle tied to orchestrator enable/disable."
```

---

## Chunk 3: Metric Guard Integration + Bounded Stop Integration

### Task 6: Metric Guard — Background Verification + Revert

**Files:**
- Modify: `src/main.rs` (OrchestratorSilence handler, new VerifyComplete handler)
- Modify: `crates/glass_core/src/event.rs` (new AppEvent variant)

- [ ] **Step 1: Add AppEvent::VerifyComplete variant**

In `crates/glass_core/src/event.rs`, add to the `AppEvent` enum:

```rust
/// Metric guard verification completed on background thread.
VerifyComplete {
    window_id: WindowId,
    session_id: SessionId,
    results: Vec<crate::verify::VerifyResult>,
}
```

Note: Since `VerifyResult` is defined in the `glass` binary crate (orchestrator.rs), not in `glass_core`, you'll need to either:
- Move `VerifyResult` to `glass_core` so `event.rs` can reference it, OR
- Use a simpler type in the event (e.g., `Vec<(String, i32, Option<u32>, Option<u32>)>`) and convert

The simplest approach: define a lightweight `VerifyEventResult` in `glass_core/src/event.rs`:

```rust
/// Lightweight verification result for cross-crate event passing.
#[derive(Debug, Clone)]
pub struct VerifyEventResult {
    pub command_name: String,
    pub exit_code: i32,
    pub tests_passed: Option<u32>,
    pub tests_failed: Option<u32>,
    pub output: String,
}
```

- [ ] **Step 2: Add `parse_test_counts_from_output` helper**

Add a helper in `src/main.rs` that extracts test pass/fail counts from command output using simple regex patterns. This avoids depending on the full SOI pipeline for verification (which runs async and would complicate the flow).

```rust
/// Extract test pass/fail counts from command output using common patterns.
fn parse_test_counts_from_output(output: &str) -> (Option<u32>, Option<u32>) {
    // Rust: "test result: ok. 45 passed; 2 failed; 0 ignored"
    if let Some(caps) = regex::Regex::new(r"(\d+) passed; (\d+) failed")
        .ok()
        .and_then(|re| re.captures(output))
    {
        let passed = caps.get(1).and_then(|m| m.as_str().parse().ok());
        let failed = caps.get(2).and_then(|m| m.as_str().parse().ok());
        return (passed, failed);
    }
    // Jest/Node: "Tests: 2 failed, 45 passed, 47 total"
    if let Some(caps) = regex::Regex::new(r"Tests:\s*(?:(\d+) failed,\s*)?(\d+) passed")
        .ok()
        .and_then(|re| re.captures(output))
    {
        let failed = caps.get(1).and_then(|m| m.as_str().parse().ok());
        let passed = caps.get(2).and_then(|m| m.as_str().parse().ok());
        return (passed, failed.or(Some(0)));
    }
    // Pytest: "5 passed, 2 failed" or "5 passed"
    if let Some(caps) = regex::Regex::new(r"(\d+) passed")
        .ok()
        .and_then(|re| re.captures(output))
    {
        let passed = caps.get(1).and_then(|m| m.as_str().parse().ok());
        let failed = regex::Regex::new(r"(\d+) failed")
            .ok()
            .and_then(|re| re.captures(output))
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse().ok())
            .or(Some(0));
        return (passed, failed);
    }
    // Go: "ok" or "FAIL" — no counts, exit code only
    (None, None)
}
```

- [ ] **Step 3: Add verification trigger in OrchestratorSilence handler**

In the OrchestratorSilence handler, after context is built but before sending to the agent, check if verification should run:

```rust
// Metric guard: run verification on background thread
let verify_mode = self.config.agent.as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .map(|o| o.verify_mode.as_str())
    .unwrap_or("floor");

if verify_mode == "floor" {
    if let Some(ref baseline) = self.orchestrator.metric_baseline {
        if !baseline.commands.is_empty() {
            let commands = baseline.commands.clone();
            let cwd = cwd.clone();
            let proxy = /* event loop proxy */;
            std::thread::Builder::new()
                .name("Glass verify".into())
                .spawn(move || {
                    let results: Vec<VerifyEventResult> = commands.iter().map(|cmd| {
                        let output = if cfg!(target_os = "windows") {
                            std::process::Command::new("cmd")
                                .args(["/C", &cmd.cmd])
                        } else {
                            std::process::Command::new("sh")
                                .args(["-c", &cmd.cmd])
                        }
                            .current_dir(&cwd)
                            .output();
                        match output {
                            Ok(o) => {
                                let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                                let (passed, failed) = parse_test_counts_from_output(&stdout);
                                VerifyEventResult {
                                    command_name: cmd.name.clone(),
                                    exit_code: o.status.code().unwrap_or(-1),
                                    tests_passed: passed,
                                    tests_failed: failed,
                                    output: stdout,
                                }
                            }
                            Err(e) => VerifyEventResult {
                                command_name: cmd.name.clone(),
                                exit_code: -1,
                                tests_passed: None,
                                tests_failed: None,
                                output: format!("Failed to run: {e}"),
                            },
                        }
                    }).collect();
                    let _ = proxy.send_event(AppEvent::VerifyComplete {
                        window_id,
                        session_id,
                        results,
                    });
                })
                .ok();
            // Block sending context until verification completes
            self.orchestrator.response_pending = true;
            return; // Don't send context yet — wait for VerifyComplete
        }
    }
}
// If no verification needed, proceed with normal context send
```

- [ ] **Step 4: Handle VerifyComplete event**

Add a new match arm in the event handler:

```rust
AppEvent::VerifyComplete { window_id, session_id, results } => {
    // Convert VerifyEventResult to VerifyResult
    let verify_results: Vec<orchestrator::VerifyResult> = results.into_iter().map(|r| {
        orchestrator::VerifyResult {
            command_name: r.command_name,
            exit_code: r.exit_code,
            tests_passed: r.tests_passed,
            tests_failed: r.tests_failed,
            errors: if r.exit_code != 0 {
                vec![r.output.lines().take(10).collect::<Vec<_>>().join("\n")]
            } else {
                vec![]
            },
        }
    }).collect();

    if let Some(ref mut baseline) = self.orchestrator.metric_baseline {
        let regressed = orchestrator::MetricBaseline::check_regression(
            &baseline.baseline_results,
            &verify_results,
        );

        if regressed {
            // Revert via git
            if let Some(ref commit) = self.orchestrator.last_good_commit {
                let cwd = self.get_focused_cwd();
                let _ = std::process::Command::new("git")
                    .args(["reset", "--hard", commit])
                    .current_dir(&cwd)
                    .output();
                tracing::info!("Metric guard: reverted to {commit}");
            }
            baseline.revert_count += 1;
            baseline.last_results = verify_results;

            // Send METRIC_GUARD message to agent
            // ... build and send context with regression info ...
        } else {
            baseline.update_baseline_if_improved(&verify_results);
            baseline.keep_count += 1;
            baseline.last_results = verify_results;

            // Send normal context to agent
            // ... proceed with normal context send ...
        }
    }

    self.orchestrator.response_pending = false;
}
```

- [ ] **Step 5: Record last_good_commit at iteration start**

In the OrchestratorSilence handler, at the start (after backpressure check), record the current commit:

```rust
// Record current commit for metric guard revert
if self.orchestrator.last_good_commit.is_none() || /* new iteration started */ {
    let cwd = self.get_focused_cwd();
    self.orchestrator.last_good_commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&cwd)
        .output()
        .ok()
        .and_then(|o| if o.status.success() {
            String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
        } else {
            None
        });
}
```

- [ ] **Step 6: Initialize MetricBaseline at orchestration start**

Where orchestrator is enabled (Ctrl+Shift+O handler), initialize the baseline:

```rust
// Initialize metric guard
let verify_mode = self.config.agent.as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .map(|o| o.verify_mode.as_str())
    .unwrap_or("floor");

if verify_mode == "floor" {
    let cwd = self.get_focused_cwd();
    let user_cmd = self.config.agent.as_ref()
        .and_then(|a| a.orchestrator.as_ref())
        .and_then(|o| o.verify_command.clone());

    let commands = if let Some(cmd) = user_cmd {
        vec![orchestrator::VerifyCommand { name: cmd.clone(), cmd }]
    } else {
        orchestrator::auto_detect_verify_commands(&cwd)
    };

    if !commands.is_empty() {
        // Run baseline verification synchronously (brief delay on Ctrl+Shift+O is acceptable)
        let baseline_results: Vec<orchestrator::VerifyResult> = commands.iter().map(|cmd| {
            let output = if cfg!(target_os = "windows") {
                std::process::Command::new("cmd")
                    .args(["/C", &cmd.cmd])
            } else {
                std::process::Command::new("sh")
                    .args(["-c", &cmd.cmd])
            }
                .current_dir(&cwd)
                .output();
            match output {
                Ok(o) => {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    let (passed, failed) = parse_test_counts_from_output(&stdout);
                    orchestrator::VerifyResult {
                        command_name: cmd.name.clone(),
                        exit_code: o.status.code().unwrap_or(-1),
                        tests_passed: passed,
                        tests_failed: failed,
                        errors: vec![],
                    }
                }
                Err(_) => orchestrator::VerifyResult {
                    command_name: cmd.name.clone(),
                    exit_code: -1,
                    tests_passed: None,
                    tests_failed: None,
                    errors: vec![],
                },
            }
        }).collect();

        let mut baseline = orchestrator::MetricBaseline::new();
        baseline.commands = commands;
        baseline.baseline_results = baseline_results.clone();
        baseline.last_results = baseline_results;
        self.orchestrator.metric_baseline = Some(baseline);
        tracing::info!("Metric guard initialized with {} commands",
            self.orchestrator.metric_baseline.as_ref().unwrap().commands.len());
    }
}
```

- [ ] **Step 7: Build and test**

Run: `cargo build && cargo test --workspace`
Expected: All pass.

- [ ] **Step 8: Commit**

```bash
git add src/main.rs crates/glass_core/src/event.rs
git commit -m "feat(orchestrator): integrate metric guard with background verification and auto-revert

Runs verify commands on background thread after each iteration.
Compares results against baseline, auto-reverts via git on regression.
Sends METRIC_GUARD message to agent with error details on revert."
```

---

### Task 7: Bounded Iteration — Wire into Main Loop

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Wire should_stop_bounded into OrchestratorSilence handler**

After the iteration counter increment, check bounded stop:

```rust
// Check bounded iteration limit
if self.orchestrator.should_stop_bounded() && !self.orchestrator.bounded_stop_pending {
    self.orchestrator.bounded_stop_pending = true;
    // Trigger checkpoint (reuses existing flow)
    let cwd = self.get_focused_cwd();
    let cp_path = orchestrator::checkpoint_path(
        &cwd,
        self.config.agent.as_ref()
            .and_then(|a| a.orchestrator.as_ref())
            .map(|o| o.checkpoint_path.as_str()),
    );
    let cp_mtime = orchestrator::file_mtime(&cp_path);
    self.orchestrator.begin_checkpoint("bounded limit reached", "N/A", cp_mtime);

    // Send checkpoint request to agent
    // ... same as existing auto-checkpoint flow ...
}
```

- [ ] **Step 2: Handle bounded stop after checkpoint completes**

In the checkpoint completion handler (where `checkpoint_changed` is true), check if bounded stop is pending:

```rust
if self.orchestrator.bounded_stop_pending {
    // Print summary
    let cp_path_str = self.config.agent.as_ref()
        .and_then(|a| a.orchestrator.as_ref())
        .map(|o| o.checkpoint_path.as_str())
        .unwrap_or(".glass/checkpoint.md");
    let summary = orchestrator::build_bounded_summary(
        self.orchestrator.iteration,
        self.orchestrator.metric_baseline.as_ref(),
        cp_path_str,
    );

    // Write summary to terminal
    if let Some(ctx) = self.windows.values().next() {
        if let Some(session) = ctx.session_mux.focused_session() {
            let bytes = format!("\r\n{summary}\r\n").into_bytes();
            let _ = session.pty_sender.send(PtyMsg::Input(std::borrow::Cow::Owned(bytes)));
        }
    }

    // Log to iterations.tsv
    let cwd = self.get_focused_cwd();
    orchestrator::append_iteration_log(
        &cwd,
        self.orchestrator.iteration,
        "bounded-stop",
        "complete",
        &format!("Bounded run complete ({} iterations)", self.orchestrator.iteration),
    );

    // Deactivate orchestrator
    self.orchestrator.active = false;
    self.orchestrator.bounded_stop_pending = false;
    self.orchestrator.checkpoint_phase = orchestrator::CheckpointPhase::Idle;

    // Stop artifact watcher
    if let Some(handle) = self.artifact_watcher_thread.take() {
        handle.thread().unpark();
        let _ = handle.join();
    }

    tracing::info!("Orchestrator: bounded run complete after {} iterations", self.orchestrator.iteration);
    for ctx in self.windows.values() {
        ctx.window.request_redraw();
    }
    return;
}
```

- [ ] **Step 3: Set max_iterations from config when orchestrator starts**

Where orchestrator is enabled, read max_iterations from config:

```rust
self.orchestrator.max_iterations = self.config.agent.as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .and_then(|o| o.max_iterations);
```

- [ ] **Step 4: Build and test**

Run: `cargo build && cargo test --workspace`
Expected: All pass.

- [ ] **Step 5: Run fmt and clippy**

Run: `cargo fmt --all -- --check && cargo clippy --workspace -- -D warnings`

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat(orchestrator): wire bounded iteration mode into main loop

Checks should_stop_bounded() after each iteration. Triggers checkpoint,
prints summary with metric guard stats, and deactivates orchestrator."
```

---

## Final Verification

### Task 8: Update Orchestrator System Prompt

**Files:**
- Modify: `src/main.rs` (where the Glass Agent system prompt is built)

- [ ] **Step 1: Find system prompt construction**

Search for where the Glass Agent system prompt is assembled in `main.rs` (look for the orchestrator system prompt builder, likely near `respawn_orchestrator_agent` or similar).

- [ ] **Step 2: Add artifact, GLASS_VERIFY, and metric guard instructions**

Add to the system prompt:

```
When the implementer is done with a task, have it create the file `{completion_artifact}` to signal completion.

If you discover additional verification commands for this project (custom test scripts, integration tests, etc.), report them:
GLASS_VERIFY: {"commands": [{"name": "description", "cmd": "command to run"}]}

After each iteration, Glass will run verification commands automatically. If changes cause test regressions or build failures, they will be automatically reverted and you will be notified.
```

- [ ] **Step 3: Build and test**

Run: `cargo build && cargo test --workspace`

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat(orchestrator): add artifact, GLASS_VERIFY, and metric guard instructions to agent system prompt"
```

---

## Final Verification

- [ ] **Run full test suite**: `cargo test --workspace`
- [ ] **Run clippy**: `cargo clippy --workspace -- -D warnings`
- [ ] **Run fmt**: `cargo fmt --all -- --check`
- [ ] **Manual smoke test**: Start Glass, open settings (Ctrl+Shift+,), verify Orchestrator section shows all 10 fields.

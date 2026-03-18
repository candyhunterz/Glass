# Feedback LLM Wiring Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the `feedback_llm` config flag to actually spawn an ephemeral claude session at the end of each orchestrator run, using the existing `llm.rs` prompt builder and response parser to produce Tier 3 PromptHint findings.

**Architecture:** `on_run_end` already produces rule-based findings synchronously. We add a new `EphemeralPurpose::FeedbackAnalysis` variant, have `run_feedback_on_end` in main.rs spawn the ephemeral agent when `feedback_llm = true`, and handle the response in the existing `EphemeralAgentComplete` handler — parsing findings and persisting them to `rules.toml`.

**Tech Stack:** Rust, glass_feedback::llm module, ephemeral_agent.rs, glass_core::event::EphemeralPurpose

---

## Design Decision: Why Not Inside `on_run_end`?

`on_run_end` is synchronous and called from the main thread event handler. Spawning a claude subprocess and waiting for it would block the UI. Instead, we follow the same pattern as checkpoint synthesis and quality verification: spawn the ephemeral agent from main.rs, handle the async result in `EphemeralAgentComplete`.

The LLM findings are applied in a second pass after the response arrives. The rule-based analysis (Tier 1 + Tier 2) still runs synchronously in `on_run_end` — the LLM analysis (Tier 3) is additive and async.

---

## File Structure

| File | Changes |
|------|---------|
| `crates/glass_core/src/event.rs` | Add `FeedbackAnalysis` to `EphemeralPurpose` enum |
| `crates/glass_feedback/src/lib.rs` | Add `llm_prompt` field to `FeedbackResult`, populate it when `feedback_llm = true` |
| `src/main.rs` | Spawn ephemeral agent from `run_feedback_on_end`, handle response in `EphemeralAgentComplete` |

---

### Task 1: Add FeedbackAnalysis to EphemeralPurpose

**Files:**
- Modify: `crates/glass_core/src/event.rs:33-38`

- [ ] **Step 1: Add variant**

In `EphemeralPurpose` enum, add:

```rust
/// Qualitative LLM analysis of orchestrator run for Tier 3 findings.
FeedbackAnalysis,
```

- [ ] **Step 2: Build and verify**

Run: `cargo build --workspace`

- [ ] **Step 3: Commit**

```
feat(event): add FeedbackAnalysis variant to EphemeralPurpose
```

---

### Task 2: Return LLM Prompt from on_run_end

**Files:**
- Modify: `crates/glass_feedback/src/lib.rs:46-52` (FeedbackResult struct)
- Modify: `crates/glass_feedback/src/lib.rs:146-300` (on_run_end function)

The idea: `on_run_end` already has the `data` (RunData) and `findings` (rule-based). If `feedback_llm` is enabled, build the LLM prompt and return it in `FeedbackResult` so the caller (main.rs) can spawn the ephemeral agent.

- [ ] **Step 1: Add fields to FeedbackResult**

```rust
pub struct FeedbackResult {
    pub findings: Vec<Finding>,
    pub regression: Option<regression::RegressionResult>,
    pub rules_promoted: Vec<String>,
    pub rules_rejected: Vec<String>,
    pub config_changes: Vec<(String, String, String)>,
    /// LLM analysis prompt — None if feedback_llm is disabled.
    /// The caller should send this to an ephemeral agent and pass
    /// the response to `apply_llm_findings`.
    pub llm_prompt: Option<String>,
}
```

- [ ] **Step 2: Add `feedback_llm` flag to FeedbackState**

In `FeedbackState` struct, add:

```rust
pub feedback_llm: bool,
pub max_prompt_hints: usize,
```

Populate them in `on_run_start`:

```rust
FeedbackState {
    // ... existing fields ...
    feedback_llm: config.feedback_llm,
    max_prompt_hints: config.max_prompt_hints,
}
```

- [ ] **Step 3: Build LLM prompt in on_run_end if enabled**

After step 9 (config_changes extraction) and before step 10 (persist), add:

```rust
// --- Step 9b: build LLM analysis prompt if enabled ---
let llm_prompt = if state.feedback_llm {
    Some(llm::build_analysis_prompt(&data, &findings))
} else {
    None
};
```

Update the `FeedbackResult` return to include `llm_prompt`.

- [ ] **Step 4: Add `apply_llm_findings` public function**

Add a new public function to `lib.rs` that main.rs calls when the ephemeral agent responds:

```rust
/// Apply LLM-generated findings to the project's rules file.
///
/// Called asynchronously after `on_run_end` when the ephemeral agent
/// returns its analysis. Parses the response, deduplicates against
/// existing prompt_hint rules, and persists to rules.toml.
pub fn apply_llm_findings(
    project_root: &str,
    llm_response: &str,
    max_prompt_hints: usize,
) {
    let project_dir = std::path::PathBuf::from(project_root).join(".glass");
    let rules_path = project_dir.join("rules.toml");

    let raw_findings = llm::parse_llm_response(llm_response);

    // Load current rules to get existing prompt_hint rules for dedup
    let rules_file = load_rules_file(&rules_path);
    let existing_hints: Vec<_> = rules_file.rules.iter()
        .filter(|r| r.action == "prompt_hint")
        .cloned()
        .collect();

    let deduped = llm::dedup_findings(raw_findings, &existing_hints, max_prompt_hints);

    if deduped.is_empty() {
        return;
    }

    // Apply as new findings — they enter as Provisional
    let mut rules_file = load_rules_file(&rules_path);
    let run_id = format!("llm-{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0));
    lifecycle::apply_findings(&mut rules_file.rules, &deduped, &run_id, false);
    let _ = save_rules_file(&rules_path, &rules_file);

    tracing::info!(
        "Feedback LLM: applied {} prompt hint(s) to {}",
        deduped.len(),
        rules_path.display()
    );
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p glass_feedback`

- [ ] **Step 6: Commit**

```
feat(feedback): return LLM prompt from on_run_end and add apply_llm_findings
```

---

### Task 3: Spawn Ephemeral Agent and Handle Response in main.rs

**Files:**
- Modify: `src/main.rs` — `run_feedback_on_end()` method
- Modify: `src/main.rs` — `EphemeralAgentComplete` handler

- [ ] **Step 1: Spawn ephemeral agent in run_feedback_on_end**

In `run_feedback_on_end()`, after the existing `on_run_end` call and config change handling, add:

```rust
// Spawn LLM analysis if prompt was generated
if let Some(prompt) = result.llm_prompt {
    let request = ephemeral_agent::EphemeralAgentRequest {
        system_prompt: "You are analyzing an orchestrator run for qualitative issues. Respond ONLY in the structured format requested.".to_string(),
        user_message: prompt,
        timeout: std::time::Duration::from_secs(60),
        purpose: glass_core::event::EphemeralPurpose::FeedbackAnalysis,
    };
    if let Err(e) = ephemeral_agent::spawn_ephemeral_agent(request, self.proxy.clone()) {
        tracing::warn!("Feedback LLM: ephemeral spawn failed: {e:?}");
    }
}
```

Note: `run_feedback_on_end` needs to save `project_root` and `max_prompt_hints` before `feedback_state` is consumed by `on_run_end`. Store them as locals.

- [ ] **Step 2: Handle FeedbackAnalysis in EphemeralAgentComplete**

Find the `EphemeralAgentComplete` handler in main.rs. Add a new match arm:

```rust
glass_core::event::EphemeralPurpose::FeedbackAnalysis => {
    match result {
        Ok(resp) => {
            if let Some(cost) = resp.cost_usd {
                tracing::info!("Feedback LLM cost: ${:.4}", cost);
            }
            // Apply findings to the project's rules file
            glass_feedback::apply_llm_findings(
                &self.orchestrator.project_root,
                &resp.text,
                self.config.agent.as_ref()
                    .and_then(|a| a.orchestrator.as_ref())
                    .map(|o| o.max_prompt_hints)
                    .unwrap_or(10),
            );
        }
        Err(e) => {
            tracing::warn!("Feedback LLM failed: {e:?}");
        }
    }
}
```

- [ ] **Step 3: Build and verify**

Run: `cargo build --workspace`

- [ ] **Step 4: Run all tests**

Run: `cargo test --workspace`

- [ ] **Step 5: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`

- [ ] **Step 6: Commit**

```
feat(feedback): wire feedback_llm to spawn ephemeral agent for Tier 3 prompt hints
```

---

## Summary

| Before | After |
|--------|-------|
| `feedback_llm = true` in config does nothing | Spawns ephemeral claude at end of each run |
| `llm.rs` prompt builder never called | Builds prompt with run metrics, iteration log, PRD, git diff |
| `llm.rs` response parser never called | Parses structured FINDING/SCOPE/SEVERITY blocks |
| `llm.rs` dedup never called | Filters duplicates against existing prompt_hint rules |
| No Tier 3 findings produced | Up to 5 PromptHint findings per run, capped by `max_prompt_hints` |

The LLM analysis is fire-and-forget — it runs in the background after orchestrator deactivation. If it fails or times out, the run's rule-based findings (Tier 1 + 2) are already persisted. The LLM findings (Tier 3) are additive.

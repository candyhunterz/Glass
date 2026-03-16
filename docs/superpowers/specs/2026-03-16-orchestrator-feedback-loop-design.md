# Orchestrator Feedback Loop — Design Spec

**Date**: 2026-03-16
**Status**: Draft
**Scope**: New crate `glass_feedback/`, integration with `orchestrator.rs` and `main.rs`

---

## Problem

The Glass orchestrator runs autonomously but doesn't learn from its runs. The same inefficiencies (bad silence thresholds, instruction overload, hot file reverts) repeat across runs. Post-mortem files are generated but never read back into the next run.

## Solution

A self-improving feedback loop that analyzes each orchestrator run, produces findings, applies changes through a guarded lifecycle, and rolls back changes that cause regressions.

---

## Architecture

### New Crate: `glass_feedback`

Four subsystems composing into the full loop:

```
┌─────────────────────────────────────────────────────────────┐
│                     glass_feedback                           │
│                                                              │
│  ┌──────────────┐  ┌──────────────┐  ┌───────────────────┐  │
│  │  Analyzer     │  │  Rule Engine │  │ Regression Guard  │  │
│  │              │  │              │  │                   │  │
│  │ Rule-based   │  │ Load rules   │  │ Snapshot before   │  │
│  │ + LLM (opt)  │  │ Match triggers│  │ Compare after    │  │
│  │              │  │ Inject actions│  │ Rollback/promote  │  │
│  └──────┬───────┘  └──────┬───────┘  └────────┬──────────┘  │
│         │                 │                    │             │
│  ┌──────┴─────────────────┴────────────────────┴──────────┐  │
│  │                  Lifecycle Manager                      │  │
│  │  proposed → provisional → confirmed/rejected            │  │
│  │  staleness detection, demotion, scope tagging           │  │
│  └────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

### Integration Points

- **`on_run_start(project_root, config)`** — called in `main.rs` when `self.orchestrator.active` is set to `true` (both Ctrl+Shift+O and config hot-reload activation paths). Loads rules from all sources, takes config snapshot, persists snapshot to `.glass/tuning-history.toml` (crash-safe). Returns `FeedbackState` handle stored on `Processor`.
- **`on_run_end(run_data)`** — called in `main.rs` on every orchestration stop: `GLASS_DONE`, user toggle off (Ctrl+Shift+O), bounded stop, usage pause, and crash. Receives a `RunData` struct built from `OrchestratorState`, event buffer, and `MetricBaseline`. Runs analysis, applies changes, handles regression guard. `RunMetrics` are computed from raw counts here (e.g., `revert_rate = metric_baseline.revert_count as f64 / iterations as f64`).
- **`check_rules(run_state)`** — called in the `OrchestratorSilence` handler before each context send. Takes current iteration count, recent revert/stuck state, and returns `Vec<RuleAction>` — text actions to append to context and Rust-level flags (extend_silence, run_verify_twice, early_stuck).
- **`prompt_hints()`** — called in `build_orchestrator_system_prompt()` (in `main.rs`, the function starting around line 780). Returns confirmed and provisional prompt hint strings for inclusion in the Glass Agent's system prompt.

### Data Files

| File | Scope | Purpose |
|---|---|---|
| `.glass/rules.toml` | Per-project | Behavioral rules with lifecycle state |
| `.glass/tuning-history.toml` | Per-project | Config change log and snapshots |
| `.glass/run-metrics.toml` | Per-project | Metrics from recent runs (last 20) |
| `.glass/archived-rules.toml` | Per-project | Pruned stale/rejected rules |
| `~/.glass/global-rules.toml` | Global | Cross-project confirmed rules |
| `~/.glass/default-rules.toml` | Shipped | Battle-tested defaults bundled with binary |

---

## Three Tiers of Findings

### Tier 1: Config Tuning

Findings that map directly to `config.toml` values. Applied automatically, protected by regression guard.

| Finding | Detection | Action |
|---|---|---|
| Silence timeout too short | Fast trigger fired during active output 2+ times | Increase `silence_timeout_secs` by 50% |
| Silence timeout too long | Avg idle between iterations > 2x threshold | Decrease by 25% |
| Stuck threshold too sensitive | Stuck triggered but next iteration made progress | Increase `max_retries_before_stuck` by 1 |
| Stuck threshold too lenient | 5+ identical fingerprints before stuck fired | Decrease by 1 (min 2) |
| Checkpoint too infrequent | Efficiency drops >30% in later iterations | Lower auto-checkpoint interval |
| Checkpoint too frequent | 3+ checkpoints for <15 iterations | Raise interval |

### Tier 2: Behavioral Rules

Runtime rules injected as text instructions in the orchestrator context. The AI agent follows them — model-agnostic.

| Finding | Detection | Action Injected |
|---|---|---|
| Hot file | Same file in 3+ revert events | "Commit {file} in isolation before other changes" |
| Uncommitted drift | 5+ iterations with no git commit | "Commit current changes before continuing" |
| Instruction overload | Agent response has 4+ instructions, next iteration only partially completes | "Give ONE instruction per response" |
| Flaky verification | Same test alternates pass/fail across iterations | Run verification twice before reverting (Rust-level) |
| Ordering failure | Dependency error followed by backtrack | "Build {dependency} before {feature}" |
| Scope creep | Git diff shows files outside PRD deliverables | "Only modify files related to current PRD item" |
| Oscillation | Semantically similar fingerprints across 4+ iterations | Trigger stuck recovery earlier (Rust-level) |
| High revert rate | Revert rate > 0.3 | "Give ONE instruction per response" |
| High waste rate | No-diff iterations > 15% | "Verify progress before continuing" |

Note: Most actions are text injected into agent context. Three actions (`run_verify_twice`, `extend_silence`, `early_stuck`) are handled in Rust code because they affect orchestrator timing, not agent instructions.

### Tier 3: Prompt Hints

Qualitative findings from LLM analysis. Capped at 10 per project, 5 global. Injected into the Glass Agent system prompt.

Examples:
- "This project's integration tests require a clean DB — run migrations first"
- "The auth module uses callbacks, not async — don't refactor to async"
- "API rate limiting tests are timing-dependent — add retry logic"

---

## Rule-Based Analyzer

### Inputs

All data already exists — no new collection needed. Passed to `on_run_end()` as a `RunData` struct:

```rust
pub struct RunData {
    pub project_root: String,
    pub iterations: u32,
    pub duration_secs: u64,
    pub kickoff_duration_secs: u64,
    pub iterations_tsv: String,             // full content of .glass/iterations.tsv
    pub metric_baseline: Option<MetricBaseline>,  // from orchestrator.rs
    pub events: Vec<OrchestratorEventEntry>,      // from OrchestratorEventBuffer in main.rs
    pub completion_reason: String,           // "done" | "bounded" | "user" | "usage" | "crash"
    pub prd_content: Option<String>,
    pub git_log: Option<String>,             // git log --oneline from run start to end
    pub git_diff_stat: Option<String>,       // git diff --stat from run start to end
}
```

The `OrchestratorEventBuffer` (defined in `src/orchestrator_events.rs`) is a 1000-event ring buffer with timestamped entries containing: `AgentText`, `ContextSent`, `AgentRespawn`, `VerifyResult`, `Thinking`, `ToolCall`, `ToolResult` events.

### Finding Structure

```rust
pub enum FindingAction {
    /// Tier 1: mutate a config.toml value
    ConfigTuning {
        field: String,           // e.g., "silence_timeout_secs"
        current_value: String,
        new_value: String,
    },
    /// Tier 2: runtime rule with a named action
    BehavioralRule {
        action: String,          // e.g., "isolate_commits"
        params: HashMap<String, String>,
    },
    /// Tier 3: free-text hint for system prompt
    PromptHint {
        text: String,
    },
}

pub struct Finding {
    pub id: String,                    // deterministic, e.g., "silence-too-short-2026-03-16"
    pub category: FindingCategory,     // ConfigTuning | BehavioralRule | PromptHint
    pub severity: Severity,            // High | Medium | Low
    pub action: FindingAction,         // typed action per tier
    pub evidence: String,              // data that led to this finding
    pub scope: Scope,                  // Project | Global
}
```

### Detectors

15 detector functions, each a pure function: takes run data in, returns `Vec<Finding>`.

1. `detect_silence_mismatch` — fast trigger fired during active output 2+ times
2. `detect_silence_waste` — avg idle between iterations > 2x threshold
3. `detect_stuck_sensitivity` — stuck triggered but next iteration progressed
4. `detect_stuck_leniency` — 5+ identical fingerprints before stuck fired
5. `detect_checkpoint_frequency` — efficiency drops >30% in later iterations
6. `detect_checkpoint_overhead` — 3+ checkpoints for <15 iterations
7. `detect_hot_files` — same file in 3+ revert events
8. `detect_uncommitted_drift` — 5+ iterations with no git commit
9. `detect_instruction_overload` — 4+ instructions, partial completion
10. `detect_flaky_verification` — test alternates pass/fail
11. `detect_ordering_failure` — dependency error then backtrack
12. `detect_scope_creep` — git diff shows files outside PRD deliverables
13. `detect_oscillation` — similar fingerprints across 4+ iterations
14. `detect_revert_rate` — revert rate > 0.3
15. `detect_waste_rate` — no-diff iterations > 15%

### Detector Feasibility Notes

**`detect_instruction_overload`**: The Glass Agent's responses are captured in the event buffer as `AgentText` events. Instruction count is approximated by counting numbered list items (`1.`, `2.`, etc.) or imperative sentences in the response text. Partial completion is detected by comparing the git diff between the iteration where instructions were given and the next — if fewer files changed than instructions given, it's a partial completion. This is a heuristic, not exact.

**`detect_scope_creep`**: Only runs when `parse_prd_deliverables()` returns a non-empty list. If the PRD has no parseable `## Deliverables` section with file paths, this detector is skipped (returns empty findings). No false positives for PRDs that describe deliverables at a higher level.

---

## Config Write Mechanism

Tier 1 findings mutate `config.toml` values. This uses the existing `glass_core::config::update_config_field()` function (already used by the orchestrator to write auto-detected mode and verify settings). This function uses `toml_edit` to preserve comments and formatting — it does not serialize/deserialize the full file.

**Hot-reload interaction**: The config watcher will fire a `ConfigReloaded` event when the feedback loop modifies `config.toml`. To avoid a race, the feedback loop sets a `feedback_write_pending` flag on `Processor` before writing. The `ConfigReloaded` handler checks this flag and skips reloading the orchestrator section if set (the in-memory state is already correct). The flag is cleared after the handler runs.

**Snapshot restore**: On regression, the feedback loop writes the snapshot values back using the same `update_config_field()` mechanism. The same `feedback_write_pending` flag prevents the hot-reload race.

---

## Rule Engine

### Rule Sources (priority order)

1. `.glass/rules.toml` — per-project, highest priority
2. `~/.glass/global-rules.toml` — cross-project
3. `~/.glass/default-rules.toml` — shipped with Glass, lowest priority

Same trigger at multiple levels: most specific (project) wins.

### Trigger Language

Trigger strings are **not a DSL**. They are human-readable labels that map to named Rust detector functions. The rule engine uses the `action` field (an enum variant name) to determine behavior, not the `trigger` string. The `trigger` field is documentation — it describes why the rule was created, not how it's evaluated.

At runtime, rules are matched by their `action` field. The rule engine checks whether the action's preconditions are met using the current `RunState`:

```rust
pub struct RunState {
    pub iteration: u32,
    pub uncommitted_iterations: u32,    // iterations since last git commit
    pub revert_rate: f64,
    pub stuck_rate: f64,
    pub waste_rate: f64,
    pub recent_reverted_files: Vec<String>,
    pub verify_results: Vec<(bool, bool)>,  // (passed_last, passed_now) pairs
}

// Example: the "isolate_commits" action checks RunState.recent_reverted_files
// against rule.action_params.file to decide if it should fire.
```

No expression parser needed. Each action type has a hardcoded check function in Rust.

### Rule Structure

```toml
[[rules]]
id = "rule-001"
trigger = "same_file_reverted >= 3"    # human-readable label, not evaluated
trigger_params = { file = "src/main.rs" }
action = "isolate_commits"             # maps to Rust action handler
action_params = { file = "src/main.rs" }
status = "provisional"               # proposed | provisional | confirmed | rejected | pinned | stale
severity = "high"
scope = "project"                    # project | global
tags = ["rust"]                      # auto-detected from project markers (Cargo.toml → "rust", package.json → "node", etc.)
added_run = "2026-03-16T14:30:00"
added_metric = "3 reverts involving src/main.rs"
confirmed_run = ""
rejected_run = ""
rejected_reason = ""
last_triggered_run = ""              # staleness detection
trigger_count = 0
```

**Tag auto-detection**: When a rule is created, tags are set based on project marker files (same detection used by `auto_detect_verify_commands`). Global rules only apply to projects whose tags overlap with the rule's tags. A rule with no tags applies to all projects.

### Runtime Action Types

| Action | When Checked | Behavior |
|---|---|---|
| `isolate_commits` | Before context send | Appends text instruction to context |
| `force_commit` | Before context send, if threshold met | Appends text instruction to context |
| `smaller_instructions` | On agent response received | Appends text instruction to next context |
| `run_verify_twice` | Before verify decision | Runs verification twice (Rust-level) |
| `extend_silence` | On silence trigger | Adds N seconds to threshold (Rust-level) |
| `restrict_scope` | Before context send | Appends text instruction to context |
| `early_stuck` | On fingerprint check | Lowers stuck threshold (Rust-level) |
| `build_dependency_first` | Before context send | Appends text instruction to context |
| `verify_progress` | Before context send | Appends text instruction to context |

### Injection Mechanism

```
OrchestratorSilence fires
  → glass_feedback::check_rules(run_state) returns Vec<Action>
  → Text actions appended to [TERMINAL_CONTEXT] as [FEEDBACK_RULES] section
  → Rust-level actions applied directly to orchestrator behavior
```

---

## Regression Guard

### Run Metrics

```rust
pub struct RunMetrics {
    pub run_id: String,
    pub project_root: String,
    pub iterations: u32,
    pub duration_secs: u64,
    pub revert_rate: f64,              // reverts / iterations
    pub stuck_rate: f64,               // stuck_triggers / iterations
    pub waste_rate: f64,               // no-diff iterations / iterations
    pub checkpoint_rate: f64,          // checkpoints / iterations
    pub completion: String,            // "complete" | "partial" | "stopped" | "crashed"
    pub prd_items_completed: u32,
    pub prd_items_total: u32,
    pub kickoff_duration_secs: u64,
}
```

### Snapshot & Compare

Before each run:
- Snapshot current `config.toml` orchestrator values and list of provisional rules
- Record baseline metrics from the previous run

After each run:
- Compare metrics against baseline
- Regression = any of:
  - `revert_rate` increased by > 0.1
  - `stuck_rate` increased by > 0.05
  - `waste_rate` increased by > 0.1
  - `completion` went from "complete" to "partial"/"crashed"

If regression:
1. Restore config from snapshot
2. Mark provisional rules as `rejected` with reason
3. Log rollback event

If improved or neutral:
1. Promote provisional → confirmed
2. Update baseline

### Cold Start (First Run)

On the first run in a project, there are no previous metrics to compare against. The regression guard operates in **observation-only mode**:

- Findings are generated and rules are proposed as normal
- Proposed rules are promoted to provisional (if within cap)
- But no regression comparison is performed — there's no baseline
- The run's metrics become the baseline for the next run
- Config tuning findings are proposed but not applied until the second run has a baseline

### Multi-Rule Rejection

If 3 provisional rules are active and the run regresses, all 3 are rejected (the regression guard cannot isolate which rule caused it). To disambiguate:

- On the next run, the analyzer may re-propose the same findings
- Re-proposed rules enter one at a time (the 3-provisional cap still applies, but after a bulk rejection the system is conservative — max 1 re-proposal per run until metrics stabilize)
- This naturally isolates which rule was the problem

### Crash Recovery

The pre-run snapshot is persisted to `.glass/tuning-history.toml` on disk (not just in memory). If Glass crashes mid-run:

- On next startup, `on_run_start()` checks for an incomplete run (snapshot exists but no corresponding entry in `run-metrics.toml`)
- The incomplete run is treated as a no-op — snapshot is preserved, provisional rules remain provisional
- No rollback or promotion occurs for the crashed run

### Safety Constraints

- Max 3 provisional rules per run (reduced to max 1 after a bulk rejection until metrics stabilize)
- Max 1 config value change per run
- Rejected changes get 5-run cooldown before re-proposal
- `status = "pinned"` — user override, never auto-reverted

### Metric Storage

`.glass/run-metrics.toml` stores last 20 runs. Older entries pruned.

---

## Lifecycle Manager

### State Transitions

```
                    ┌──────────────────────────────────────┐
                    │                                      │
proposed ──→ provisional ──→ confirmed ──→ stale ──────────┤
                 │               │           │             │
                 └→ rejected     │           └→ confirmed  │
                      │          │             (re-triggered)
                      └→ cooldown (5 runs)                 │
                            │    │                         │
                            └→ proposed    └→ provisional  │
                             (re-eligible)   (env drift)   │
                                                           │
                                              stale ──→ archived
                                             (5 more runs)
```

### Promotion Criteria

- `proposed → provisional`: finding generated by analyzer, within the 3-rule provisional cap
- `provisional → confirmed`: next run's metrics did not regress
- `confirmed → stale`: rule hasn't triggered in 10 consecutive runs

### Demotion Criteria

- `provisional → rejected`: next run's metrics regressed
- `confirmed → provisional`: metrics that were stable for 3+ runs start regressing (environment drift)
- `stale → archived`: stale for 5 more runs after being flagged, moved to `.glass/archived-rules.toml`

### Staleness Detection

Each rule tracks `last_triggered_run` and `trigger_count`. After every run:

- Rules not triggered in 10 runs → marked `stale`
- Rules stale for 5 additional runs → archived (moved to `archived-rules.toml`)
- Archived rules are not loaded at runtime
- **Staleness reset**: if a stale rule triggers again (the condition it guards against recurs), it is promoted back to `confirmed` and its stale counter resets. This handles intermittent issues that resurface.

### Confirmed Rule Demotion (Environment Drift)

After each run, check if confirmed rules' associated metrics are trending worse over the last 3 runs. If so, demote back to `provisional` for re-evaluation.

### Convergence Behavior

When the system has converged (all rules confirmed, no new findings):
- Analyzer still runs (milliseconds, free) — acts as health check
- Staleness checks still run — prunes unused rules
- Demotion checks still run — catches environment drift
- No new provisional rules generated — system is quiet

---

## Error Handling

All feedback data files (`.glass/rules.toml`, `.glass/tuning-history.toml`, `.glass/run-metrics.toml`, `~/.glass/global-rules.toml`) may become corrupted through partial writes, manual editing errors, or disk issues.

**Strategy**: on any TOML parse failure:
1. Log a warning with the file path and parse error
2. Back up the corrupted file to `{filename}.bak`
3. Continue with an empty/default state for that file
4. The feedback system degrades gracefully — rules are empty (no injection), metrics are empty (cold-start mode), snapshots are empty (observation-only)

No feedback data file is critical to Glass operation. The terminal and orchestrator function normally without the feedback system.

---

## LLM Analyzer (Opt-in)

### Activation

Enabled via `feedback_llm = true` in `[agent.orchestrator]`. Disabled by default.

### Execution

- Runs after rule-based analysis completes
- Spawns a short-lived LLM call using the same agent config (Claude CLI or whatever is configured). This is the primary approach — reusing the live Glass Agent process is unreliable because the agent may already be shutting down when `on_run_end()` fires
- The analysis call is a single prompt/response exchange, not a long-running session
- Timeout: 60 seconds. If the call fails or times out, LLM findings are silently skipped (rule-based findings still apply)

### Prompt

```
[FEEDBACK_ANALYSIS]
Analyze this orchestrator run and identify qualitative issues that
wouldn't be caught by quantitative rules.

RUN METRICS:
iterations: {N}, reverts: {N}, stuck: {N}, duration: {N}min

ITERATION LOG (last 50):
{iterations.tsv content}

EVENT BUFFER (summarized):
{thinking events, tool calls, agent responses — compressed}

PRD SUMMARY:
{first 500 words of PRD}

GIT DIFF SUMMARY:
{git diff --stat from run start to end}

RULE-BASED FINDINGS ALREADY DETECTED:
{list of findings the rule engine already produced}

Respond in this exact format:
FINDING: <description>
SCOPE: project|global
SEVERITY: high|medium|low
---
(repeat for each finding, max 5)
```

### Output Processing

- Parsed into `Finding` structs with `category = PromptHint`
- Deduplicated against existing prompt hints (fuzzy match on description)
- Subject to cap: 10 per project, 5 global
- Goes through the same provisional → confirmed lifecycle

### System Prompt Injection

```
LESSONS FROM PREVIOUS RUNS:
- [confirmed] This project's integration tests require a clean DB — run migrations first
- [confirmed] The auth module uses callbacks, not async — don't refactor to async
- [provisional] API rate limiting tests are timing-dependent — add retry logic
```

Only `confirmed` and `provisional` hints included. `stale` and `rejected` excluded.

### Model Agnostic

- Plain text prompt, no model-specific features
- Simple response format parseable with basic string splitting
- If parsing fails, LLM findings silently dropped (rule-based findings still apply)

---

## Default Rules

### Shipped with Glass

Embedded in binary, written to `~/.glass/default-rules.toml` on first launch or version update.

```toml
[meta]
version = "1.0.0"

[[rules]]
id = "default-uncommitted-drift"
trigger = "uncommitted_iterations >= 5"
action = "force_commit"
severity = "medium"
scope = "global"

[[rules]]
id = "default-hot-file"
trigger = "same_file_reverted >= 3"
action = "isolate_commits"
severity = "high"
scope = "global"

[[rules]]
id = "default-instruction-overload"
trigger = "instruction_count >= 4 && partial_completion"
action = "smaller_instructions"
severity = "medium"
scope = "global"

[[rules]]
id = "default-flaky-verify"
trigger = "verify_alternates_pass_fail >= 2"
action = "run_verify_twice"
severity = "high"
scope = "global"

[[rules]]
id = "default-revert-rate"
trigger = "revert_rate > 0.3"
action = "smaller_instructions"
severity = "high"
scope = "global"

[[rules]]
id = "default-waste-rate"
trigger = "waste_rate > 0.15"
action = "verify_progress"
severity = "medium"
scope = "global"
```

### Lifecycle Integration

- On first run in a project, defaults are copied to `.glass/rules.toml` as `provisional` (they are battle-tested enough to skip the `proposed` stage, but still need to prove they don't regress this specific project)
- Same lifecycle from there: provisional → confirmed/rejected
- Rejected defaults stay rejected for this project — Glass updates don't re-propose them
- New defaults added in a Glass update enter as `provisional` (the `[meta] version` field tracks which defaults have been seen)

### Community Rules (Future-Ready)

Rule format supports import. Not implemented now, but no design changes needed later:

```bash
# Future
glass feedback import community-rules-rust.toml
```

Imported rules enter as `proposed`. Same lifecycle. Regression guard protects against bad community rules.

---

## Configuration

```toml
[agent.orchestrator]
# Existing fields unchanged...

# Feedback loop
feedback_llm = false          # Enable LLM qualitative analysis (opt-in)
max_prompt_hints = 10         # Max Tier 3 hints per project
```

No other config needed. The feedback loop is always active for the rule-based tier (free, instant). Only the LLM tier is opt-in.

---

## Modes

The feedback loop is mode-agnostic. Works identically for `build`, `general`, and `audit` orchestrator modes. The same detectors, rule engine, and regression guard apply. What differs is interpretation:

| Finding | Build mode example | General mode example |
|---|---|---|
| Hot file revert | Test file keeps failing | Output doc keeps being overwritten |
| Instruction overload | 4 code tasks at once | 4 document sections at once |
| Scope creep | Modified files outside PRD | Wrote sections PRD didn't ask for |
| Ordering failure | Built API before DB schema | Wrote conclusion before research |

---

## Testing Strategy

- Each detector is a pure function — unit testable with synthetic run data
- Rule matching tested with fixture rules + simulated run state
- Lifecycle transitions tested as a state machine
- Regression guard tested with metric snapshots and simulated regressions
- Integration test: synthetic multi-run sequence verifying propose → confirm and propose → reject flows
- LLM analyzer: response parsing tested with fixture responses, no live LLM calls in tests

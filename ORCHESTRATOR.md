# Glass Orchestrator — Complete Architecture Reference

Read this file to understand the orchestrator mode before making any changes. This is the authoritative reference — do not ask the user to re-explain what's documented here.

## What It Does

The orchestrator drives Claude Code sessions autonomously. It spawns a separate "Glass Agent" (a claude subprocess) that reviews terminal output, makes product decisions, and types instructions into Claude Code running in the terminal. The flow is: silence detected → capture terminal context → send to Glass Agent → agent responds with instruction → type into PTY → repeat.

## Multi-Provider Backend

The orchestrator supports multiple LLM providers for the Glass Agent (the reviewer/guide). The implementer (the CLI running in the terminal) is independent — controlled by the `implementer` config field.

### Supported Providers

| Provider | Config Value | Auth | Default Model |
|----------|-------------|------|---------------|
| Claude Code CLI | `provider = "claude-code"` | CLI's own OAuth | (CLI decides) |
| Anthropic API | `provider = "anthropic-api"` | `ANTHROPIC_API_KEY` env var | `claude-sonnet-4-6` |
| OpenAI API | `provider = "openai-api"` | `OPENAI_API_KEY` env var | `gpt-4o` |
| Ollama (local) | `provider = "ollama"` | None required | `llama3` |
| Custom endpoint | `provider = "custom"` | Optional `GLASS_API_KEY` | `gpt-4o` |

### Model Mixing

The orchestrator (reviewer) and implementer (code writer) use different models independently:

```toml
[agent]
provider = "openai-api"         # GPT-4o reviews and guides
model = "gpt-4o"

[agent.orchestrator]
implementer = "claude-code"     # Claude Code writes the code
```

### Implementer Configuration

| Implementer | Config Value | Crash Recovery Command |
|-------------|-------------|----------------------|
| Claude Code | `"claude-code"` | `claude --dangerously-skip-permissions -p` |
| Codex | `"codex"` | `codex --full-auto` |
| Aider | `"aider"` | `aider --yes-always` |
| Gemini | `"gemini"` | `gemini` |
| Custom | `"custom"` | Uses `implementer_command` value |

### Persona

The `persona` field customizes the orchestrator agent's behavior without modifying the protocol (GLASS_WAIT/GLASS_DONE). It's inserted as Layer 3 in the prompt:

1. **Protocol** (hardcoded) — GLASS_WAIT, GLASS_DONE, response format
2. **Mode behavior** (hardcoded) — build/audit/general iteration protocol
3. **Persona** (user-editable) — tone, domain expertise, constraints
4. **Project instructions** (.glass/agent-instructions.md)

```toml
[agent.orchestrator]
persona = "You are a senior systems architect. Be concise. Prioritize correctness."
# Or load from file:
# persona = ".glass/agent-persona.md"
```

### Backend Architecture

All providers normalize to the same `AgentEvent` stream. The orchestrator state machine in `main.rs` never sees provider-specific details:

```
Provider JSON/SSE → AgentEvent → AppEvent → Orchestrator State Machine
```

Tool calling for API backends (OpenAI, Anthropic, Ollama) uses Glass IPC to execute MCP tools:

```
API Backend → SyncIpcClient → Glass GUI IPC Listener → MCP Tool Execution → Result
```

The backend crate lives at `crates/glass_agent_backend/` with one file per provider.

## Key Files

| File | Purpose |
|------|---------|
| `src/orchestrator.rs` | State machine, response parsing, constants, metric baseline, context building |
| `src/main.rs` | All event handlers (OrchestratorSilence, OrchestratorResponse, VerifyComplete, toggle, crash recovery) |
| `src/checkpoint_synth.rs` | Checkpoint.md synthesis (ephemeral AI or fallback) |
| `src/orchestrator_events.rs` | Transcript buffer for activity overlay UI |
| `src/ephemeral_agent.rs` | Short-lived claude subprocess for checkpoint synthesis |
| `src/usage_tracker.rs` | OAuth usage polling, auto-pause at 80%/hard-stop at 95% |
| `crates/glass_terminal/src/silence.rs` | SmartTrigger — 4-mode silence detection that drives the loop |
| `crates/glass_feedback/src/` | Self-improvement feedback loop (analyzer, rules, lifecycle, regression) |
| `crates/glass_scripting/src/` | Rhai scripting engine, hook registry, action API, script lifecycle, MCP tools, profiles |
| `src/script_bridge.rs` | Bridge: routes events to scripts, executes actions, tracks lifecycle per run |
| `crates/glass_feedback/src/attribution.rs` | Per-rule metric attribution — correlates rule firings with metric deltas to identify passengers |
| `crates/glass_feedback/src/ablation.rs` | Ablation testing — disables one confirmed rule per run to definitively test if it's needed |
| `crates/glass_core/src/agent_runtime.rs` | Agent command args, system prompt building, activity stream |

## The Main Loop

```
User presses Ctrl+Shift+O
    → Gather context (agent-instructions.md → prd_path → auto-scan .md files → terminal → git status)
    → Validate: at least one context file found (else: centered toast + abort)
    → Glass Agent subprocess spawns with assembled context as first message
    → Autonomous loop begins immediately

SmartTrigger fires (silence detected)
    → OrchestratorSilence event
    → Guard checks: active? agent alive? response_pending?
    → Flush any deferred TypeText
    → Capture terminal context (20-80 lines based on SOI)
    → Compute environment fingerprint (stuck detection)
    → Run metric guard verification (if enabled)
    → Send context to Glass Agent via stdin JSON
    → Set response_pending = true

Glass Agent responds
    → OrchestratorResponse event
    → Parse response (TypeText / Wait / Checkpoint / Done / Verify)
    → Check bounded limit and auto-checkpoint
    → Route response:
        TypeText → type into PTY (or defer if block executing)
        Wait → do nothing, wait for next silence
        Checkpoint → synthesize checkpoint.md, kill/respawn agent
        Done → deactivate orchestrator, final commit
        Verify → register new verification commands

Repeat until Done or user presses Ctrl+Shift+O again
```

## Cancel and Re-enable Flow

When the user cancels mid-run (Ctrl+Shift+O off):
1. `completion_reason` set to `"user_cancelled"`
2. Feedback loop runs full `on_run_end()` analysis with partial data
3. If checkpoint synthesis was in progress, fallback written to `.glass/checkpoint.md`
4. Agent killed, artifact watcher stopped

When the user re-enables (Ctrl+Shift+O on):
1. All feedback counters reset to zero — treated as a fresh run
2. **`checkpoint.md` is regenerated** from current git state (recent commits + uncommitted changes) — NOT the stale checkpoint from the previous run. This prevents the agent from trying to redo work the user did manually between runs.
3. `iterations.tsv` is preserved — the agent sees history from prior runs (truncated to last 50 entries). This helps it avoid repeating failed approaches.
4. `on_run_start()` loads rules (including any promoted from the cancelled run)
5. Metric baseline preserved if it existed — test floor carries over
6. Context is re-gathered and agent spawns immediately (no kickoff delay)

## Orchestrator Modes

Set via `orchestrator_mode` in config. Auto-detected at activation.

| Mode | When | Agent Role |
|------|------|------------|
| **build** | Cargo.toml, package.json, etc. exist | TDD cycle: plan→test first→implement→verify→commit. Uses MCP tools for active verification. |
| **general** | PRD has deliverables but no code project | Deliverable tracking for research/planning/design tasks. |

All modes have full MCP tool access.

## Activation Flow

Ctrl+Shift+O immediately gathers context, validates it, and spawns the agent. There is no kickoff delay.

### Steps

1. **Ctrl+Shift+O pressed** → read current working directory from OSC 7 CWD
2. **Context cascade** (in priority order):
   - `.glass/agent-instructions.md` — primary steering file (frontmatter + body)
   - `prd_path` from config — structured project plan
   - Auto-scan: up to 5 recently modified `.md` files in project root (modified within 30 days)
   - Recent terminal output lines
   - `git log --oneline -10` and `git diff --stat`
3. **Zero-context gate** — if `context.files` is empty after the cascade, activation is **aborted**:
   - A centered toast message is displayed: `"Orchestrator: no project context found (add PRD.md or .glass/agent-instructions.md)"`
   - `orchestrator.active` is reset to false
4. **Agent spawns** → initial message sent as `[ORCHESTRATOR_START]` block containing all gathered context
5. **Autonomous loop begins immediately** — SmartTrigger drives from here

### Zero-Context Gate

The gate blocks activation if no markdown files with actual content are found. This prevents the agent from running blind without any project context.

Files must exist AND have non-empty content. Terminal lines and git output alone are not counted as "context files."

### `.glass/agent-instructions.md` Format

This file is the primary way to steer the agent for a project. It supports optional YAML frontmatter followed by a body with instructions.

```markdown
---
title: My Project
mode: build
verify: cargo test --workspace
---

Build a GPU-accelerated terminal emulator in Rust. Focus on:
- Implement the feature described in PRD.md
- Keep all tests passing
- Commit after each working increment
```

**Frontmatter fields** (all optional):
- `title` — project name (informational)
- `mode` — orchestrator mode override (`build`, `general`, `audit`)
- `verify` — verification command override

The body (everything after the `---` closing fence) is passed verbatim as the agent's instructions. If no frontmatter is present, the entire file is treated as instructions.

**Note:** `handoff.md` is no longer supported. It has been superseded by `agent-instructions.md`, which is persistent (not deleted after reading) and supports frontmatter metadata.

## Silence Detection (SmartTrigger)

Four trigger modes in priority order:

1. **Prompt regex** — instant fire when terminal output matches `agent_prompt_pattern` config
2. **Shell prompt (OSC 133;A)** — instant fire when shell returns to prompt
3. **Fast trigger** — fires `fast_trigger_secs` (default 5) after output stops flowing
4. **Slow fallback** — fires every `silence_timeout_secs` (default 30) periodically

The SmartTrigger lives in the PTY reader thread and sends `AppEvent::OrchestratorSilence` to the main thread. The event carries `silence_duration_ms` (time since last PTY byte) for rendering stall detection. The main thread also captures block executing state at trigger time. Both are logged to `iteration-details.md` — see [Iteration Detail Log](#iteration-detail-log).

Fast trigger requires `min_output_bytes` (default 512) of output before arming, preventing spurious fires on small outputs like prompts.

## Response Parsing

The Glass Agent's text response is parsed into structured actions:

| Response | Parsed As | Action |
|----------|-----------|--------|
| `GLASS_WAIT` (exact) | `Wait` | Reset silence timer, check again later |
| `GLASS_DONE: summary` | `Done { summary }` | Stop orchestration, final commit |
| `GLASS_CHECKPOINT: {"completed": "...", "next": "..."}` | `Checkpoint` | Synthesize checkpoint.md, respawn agent |
| `GLASS_VERIFY: {"commands": [...]}` | `Verify` | Register additional verification commands |
| Anything else | `TypeText(text)` | Type into PTY as Claude Code input |

## Checkpoint Cycle

When a checkpoint fires (agent-requested, auto after `checkpoint_interval` iterations (default 15), or bounded limit):

1. `trigger_checkpoint_synthesis()` gathers git state, iterations log, metric summary
2. Builds fallback checkpoint content (pure Rust, no AI)
3. Spawns ephemeral claude subprocess for AI-synthesized checkpoint (120s timeout)
4. On completion (or timeout/failure): writes `.glass/checkpoint.md`
5. **Clears implementer context** — types `/clear` into the PTY so the implementer (Claude Code, Aider, etc.) starts fresh alongside the reviewer. Without this, the implementer's context window fills up over 15+ iterations even though the Glass Agent has fresh context.
6. If bounded stop: deactivates orchestrator, writes bounded summary, generates post-mortem. Otherwise:
7. Kills current Glass Agent, spawns fresh agent with handoff: "Read .glass/checkpoint.md and continue"
8. Resets stuck detection, iterations_since_checkpoint counter, bounded_stop_pending flag

**Generation-specific files:** Each agent spawn writes `agent-system-prompt-{generation}.txt` and `agent-mcp-{generation}.json` to avoid file-locking conflicts on Windows when the old process hasn't fully exited yet.

**Stale crash filtering:** `AppEvent::AgentCrashed` carries the generation of the agent that died. Old agent crashes (from a killed predecessor) are filtered by comparing against the current `agent_generation`.

**Checkpoint.md contains:** completed work summary, current errors, abandoned approaches, key decisions, git state, next steps.

**Implementer clear commands:**
| Implementer | Clear Command | Notes |
|-------------|--------------|-------|
| claude-code | `/clear` | Clears conversation history, keeps session alive |
| aider | `/clear` | Clears conversation history |
| gemini | `/clear` | Clears conversation history |
| codex | (skipped) | No known clear command — context managed externally |
| custom | (skipped) | User must handle context management |

## Metric Guard (Verification)

Prevents the agent from introducing regressions.

**Modes:**
- `floor` — runs test commands (auto-detected: cargo test, npm test, pytest, etc.)
- `files` — checks deliverable file sizes (for general mode)
- `off` — no verification

**Flow per iteration:**
1. Run verify commands on background thread (5-min timeout)
2. `VerifyComplete` event fires with results
3. If baseline empty: establish baseline (first run)
4. Else: `check_regression(baseline, current)`:
   - Exit code regressed (0 → non-zero) → revert
   - Test pass count dropped → revert
   - Test fail count increased → revert
   - Extra failing command → revert
5. On regression: `git reset --hard` to last_good_commit, notify agent
6. On keep: `update_baseline_if_improved()` raises the floor (pass count can only go up)

## Stuck Detection

Two signals, combined with OR:

1. **Response stuck:** N identical consecutive TypeText responses (default N=3)
2. **Fingerprint stuck:** N identical environment fingerprints (terminal hash + SOI errors + git diff)

When stuck:
- Send "You've tried this same approach multiple times..." message to Claude Code
- Log to iterations.tsv
- Reset stuck detection buffers

## Agent Spawn Details

The Glass Agent is a `claude` subprocess spawned with:
```
claude -p --verbose --output-format stream-json --input-format stream-json
  --system-prompt-file ~/.glass/agent-system-prompt-{generation}.txt
  --mcp-config ~/.glass/agent-mcp-{generation}.json
  --allowedTools <all Glass MCP tools>
  --dangerously-skip-permissions
  --disable-slash-commands
```

- **stdin:** JSON messages (context sends from orchestrator)
- **stdout:** stream-json (parsed by reader thread → AppEvents)
- **stderr:** null (prevents deadlock from stderr buffer fill)
- **Windows:** CREATE_NO_WINDOW flag
- **Crash recovery:** 3 restart attempts with exponential backoff (5s → 15s → 45s)
- **Generation tracking:** Each respawn increments `agent_generation`. `AgentCrashed` events carry the generation so stale crashes from killed predecessors are filtered out.
- **File isolation:** System prompt and MCP config use generation-specific filenames to prevent file-locking conflicts during respawn

## Usage Tracking

Background thread polls Anthropic OAuth usage API every 60 seconds.

| Threshold | Event | Action |
|-----------|-------|--------|
| >= 95% | UsageHardStop | Write emergency checkpoint, deactivate |
| >= 80% | UsagePause | Deactivate, skip ephemeral agents |
| < 20% | UsageResume | Log only — user must re-enable manually |

## Deferred TypeText

TypeText responses are buffered (not typed immediately) when a block is executing (Claude Code is actively running a command). The deferred queue is flushed one item at a time on each silence trigger. Each flush types one deferred message and returns (letting the terminal process it before the next).

**PTY write protocol:** All orchestrator text→PTY writes use a split-write pattern to avoid Claude Code's paste detection:
1. Text (newlines collapsed to spaces) is sent as one write
2. Enter (`\r`) is sent 150ms later via a background thread

Without the split, Claude Code treats `text\r` arriving in a single `read()` as pasted content and shows `[Pasted text #1 +1 lines]`, waiting for manual Enter confirmation.

## Course Correction (Nudge)

While the orchestrator is running, the user can write `.glass/nudge.md` in the project root. On the next silence trigger, the orchestrator reads it, includes it as `[USER_NUDGE]` in the context sent to the Glass Agent, then deletes the file.

---

# Self-Improvement Feedback Loop

## Overview

The feedback loop analyzes each orchestrator run and produces findings that tune future runs. It operates across four tiers:

1. **Tier 1: Config Tuning** — adjusts `config.toml` values (silence timeout, max retries, etc.)
2. **Tier 2: Behavioral Rules** — adds rules to `rules.toml` (force_commit, split_instructions, etc.)
3. **Tier 3: Prompt Hints** — injects text into the agent's context
4. **Tier 4: Rhai Scripts** — LLM-generated scripts that hook into Glass events and execute actions at runtime via the embedded scripting engine (`glass_scripting` crate)

## Files Created

### Per-project (`<project_root>/.glass/`)

| File | Purpose | Created By |
|------|---------|------------|
| `rules.toml` | Active behavioral rules (provisional/confirmed) | `on_run_end()` |
| `run-metrics.toml` | Historical run metrics (one entry per run, includes per-rule firings) | `on_run_end()` |
| `rule-attribution.toml` | Per-rule attribution scores (passenger scores, firing correlations) | `on_run_end()` |
| `tuning-history.toml` | Config snapshots at each run start, pending ConfigTuning changes, per-field cooldowns | `on_run_start()` / `on_run_end()` |
| `archived-rules.toml` | Rules that were rejected or went stale | `on_run_end()` |
| `iterations.tsv` | Per-iteration log (TSV: iteration, commit, feature, metric, status, description). Lean format fed to agent context. | `append_iteration_log()` during run |
| `iteration-details.md` | Rich per-iteration markdown log for post-run analysis: trigger source + silence duration, block executing state, agent instructions, files changed, verification results, errors, rendering stall warnings. Pruned to 2000 lines. | `append_iteration_detail()` during run |
| `checkpoint.md` | Last checkpoint for agent context handoff | Checkpoint synthesis |
| `run-report-YYYYMMDD-HHMMSS.md` | Combined run report: postmortem (summary, metric guard, commits), trigger source breakdown, feedback analysis (tiers, rules, config tuning, ablation, attribution) | `run_feedback_on_end()` |
| `nudge.md` | User course correction (read and deleted per iteration) | User-created |
| `agent-instructions.md` | Persistent agent steering file with optional frontmatter (supersedes handoff.md) | User-created |
| `done` | Completion artifact signal (configurable path) | Agent creates, orchestrator deletes |
| `scripts/hooks/*.toml` | Script manifests (name, hooks, status, origin) | Tier 4 generation or user-created |
| `scripts/hooks/*.rhai` | Rhai script source files | Tier 4 generation or user-created |
| `scripts/tools/*.toml` | MCP tool script manifests | Tier 4 generation or user-created |
| `scripts/tools/*.rhai` | MCP tool script source | Tier 4 generation or user-created |
| `scripts/feedback/*.toml` | Auto-generated script manifests (provisional) | Tier 4 ephemeral agent |
| `scripts/feedback/*.rhai` | Auto-generated script source | Tier 4 ephemeral agent |

### Global (`~/.glass/`)

| File | Purpose |
|------|---------|
| `global-rules.toml` | Rules with `scope = "global"` synced across all projects |
| `agent-system-prompt.txt` | Last-written Glass Agent system prompt |
| `agent-mcp.json` | MCP config pointing to `glass mcp serve` |
| `agent-diag.txt` | Spawn diagnostics (PATH, args, success/failure) |
| `scripts/hooks/*.toml+.rhai` | Global hook scripts (shared across projects) |
| `scripts/tools/*.toml+.rhai` | Global MCP tool scripts |

## Run Analyzer Dashboard

`glass analyze` opens a web dashboard that visualizes all `.glass/` data files. Run it from any project directory to inspect orchestrator runs:

```bash
glass analyze                        # Analyze current project
glass analyze --dir ~/project/.glass # Analyze specific project
```

The dashboard (React + D3.js) is embedded in the Glass binary — no Node.js required at runtime. It provides 6 tabs: Overview, Timeline, Triggers, Feedback, CrossRun, RawData. Source lives in `tools/run-analyzer/`.

## Iteration Detail Log

### Purpose

The `iteration-details.md` file captures rich per-iteration data for post-run analysis. Unlike `iterations.tsv` (lean TSV fed to the agent's context window), this file is designed for human or AI review after a run completes — or visualized via `glass analyze`. Read this file to diagnose what went wrong, what the agent decided at each step, and whether the orchestrator triggered prematurely.

### What's Logged

Each iteration entry includes:

| Field | Description | When Present |
|-------|-------------|-------------|
| **Trigger** | Source (Prompt/ShellPrompt/Fast/Slow), silence duration in ms, block executing state | Every instruction iteration |
| **Action** | High-level type: `instruction`, `stuck`, `checkpoint`, `done`, `verify` | Every iteration |
| **Agent instruction** | The text the agent sent to Claude Code (truncated to 500 chars) | TypeText responses |
| **Files changed** | `git diff --name-only` since previous iteration's HEAD | When HEAD changes |
| **Verification** | KEEP or REVERT with test pass/fail counts | After metric guard runs |
| **Error** | Error description | On regressions or failures |
| **Note** | Additional context (self-correction, rendering stall warnings) | When relevant |

### Rendering Stall Detection

A known issue: the terminal renderer can stall (wgpu/winit misses a redraw), causing the silence detector to think Claude Code is idle when it's actually still working. The iteration detail log captures two signals to detect this:

1. **`silence_duration_ms`** — how long since the last PTY byte when the trigger fired. Carried from the PTY thread via the `OrchestratorSilence` event. A short silence duration (e.g., 200ms) combined with a Fast or Slow trigger suggests the PTY was recently active.
2. **`[BLOCK STILL EXECUTING]`** — whether the block manager had an active Executing block when the trigger fired. If true, the silence detector fired while a command was still running — likely a rendering stall or premature trigger.

When both conditions are true, the log entry includes: `WARNING: Trigger fired while a command block was still executing — possible rendering stall or premature trigger`

### Example Entry

```markdown
## Iteration 5 [14:23:07]

**Trigger:** Fast, silence=5200ms [BLOCK STILL EXECUTING]
**Action:** instruction
**Agent instruction:** Fix the TypeScript compilation errors in DecisionMatrix.tsx — the generic type parameter...
**Files changed:**
- `src/components/DecisionMatrix.tsx`
- `src/types.ts`
**Note:** WARNING: Trigger fired while a command block was still executing — possible rendering stall or premature trigger
```

### Implementation

- `append_iteration_detail()` in `orchestrator.rs` — appends markdown entries
- `append_iteration_detail_run_separator()` — marks new run start with timestamp
- `prune_iteration_details()` — keeps last 2000 lines, runs at end of each run
- `git_files_changed_since()` / `git_head_short()` — helper functions for diff tracking
- `SmartTrigger::silence_duration()` — exposes time since last PTY byte
- `OrchestratorSilence` event carries `silence_duration_ms` from PTY thread
- `OrchestratorState` captures `last_trigger_silence_duration`, `last_trigger_block_executing`, `last_trigger_source` at trigger time, consumed when the agent response arrives

## Feedback Lifecycle

### on_run_start (called when orchestrator activates)
1. Compute file paths under `<project>/.glass/` and `~/.glass/`
2. Load merged rule engine from project `rules.toml` + global `global-rules.toml`
3. Reset all `trigger_count` to 0 (per-run firing tracking)
4. Snapshot current config values to `tuning-history.toml`
5. Load attribution data from `rule-attribution.toml`
6. Check ablation conditions: if `ablation_enabled` and no provisional rules exist, select an ablation target (highest passenger score first) via `ablation::select_target()`
7. Return `FeedbackState` handle (includes `ablation_target`, `attribution_scores`)

### check_rules (called every iteration during OrchestratorSilence)
- `RuleEngine` evaluates all active rules against live `RunState`
- If an ablation target is set, that rule is skipped (exists but doesn't fire this run)
- Returns `Vec<RuleAction>` — actions enforced by the orchestrator:

| Action | What It Does |
|--------|-------------|
| `ForceCommit` | `git commit -am` to checkpoint (if no regression) |
| `IsolateCommit { file }` | `git add <file> && git commit` for hot files |
| `SplitInstructions` | Break numbered instructions, send one at a time |
| `RevertOutOfScope { files }` | `git checkout --` files not in PRD deliverables |
| `BlockUntilResolved { message }` | Halt progress until dependency resolved |
| `ExtendSilence { extra_secs }` | Increase silence threshold |
| `RunVerifyTwice` | Run verification twice before reverting |
| `EarlyStuck { threshold }` | Lower stuck detection threshold |
| `TextInjection(text)` | Append text to agent context |

### on_run_end (called when orchestrator deactivates)
1. **Analyze** — run all 11 detectors on `RunData` → produce `Finding`s (Tier 1 + 2)
2. **Compute metrics** — iterations, duration, revert_rate, stuck_rate, waste_rate, checkpoint_rate
3. **Load baseline** — previous run's metrics for regression comparison
3b. **Record rule firings** — collect each rule's `trigger_count` into `RunMetrics.rule_firings`
3c. **Update attribution** — compute metric deltas vs baseline, call `attribution::update()` to correlate rule firings with improvements/regressions, update passenger scores
3d. **Evaluate ablation** — if ablation target was set, compare current metrics against 3-run rolling average. If regressed → rule is "needed" (stays Confirmed). If same/improved → rule is a "passenger" (demoted to Stale)
3e. **Prune attribution** — remove scores for rules that were archived by staleness
4. **Regression check** — compare current metrics vs previous run's baseline
4b. **Promote or reject** provisional rules:
   - Improved/Neutral → promote to Confirmed
   - Regressed → reject all provisionals, archive them
5. **Apply new findings** — create new Provisional rules from detector findings
6. **Staleness** — increment stale_runs for rules that didn't fire; archive after threshold
7. **Drift** — detect worsening trends over last 3 runs
8. **Pending ConfigTuning evaluation** — load `tuning-history.toml` and check for a pending config change from the previous run:
   - Regressed → revert config value to old, set 5-run cooldown on that field, suppress new ConfigTuning this run
   - Improved/Neutral → confirm change, clear pending
   - Decrement all cooldowns, remove expired
8b. **Config tuning** — extract Tier 1 findings (max 1 per run, skip fields in cooldown) → write to config.toml and record as pending in `tuning-history.toml` for next-run evaluation
9. **Build LLM prompt** — if `feedback_llm = true`, build analysis prompt from run data + existing findings (returned in `FeedbackResult.llm_prompt`)
10. **Build script prompt** — if `script_generation = true` and lower tiers have been tried (rules exist) but waste/stuck rates exceed 33%, build Tier 4 prompt (returned in `FeedbackResult.script_prompt`)
11. **Persist** — save rules.toml, run-metrics.toml (with rule_firings), archived-rules.toml, rule-attribution.toml
12. **Sync global** — copy global-scoped rules to `~/.glass/global-rules.toml`; remove rejected/stale ones
13. **Script lifecycle** — `ScriptBridge::on_feedback_run_end(regressed)` promotes/rejects/ages scripts based on regression result (see Script Lifecycle below)

### Feedback LLM (Tier 3 — async, after on_run_end)

When `feedback_llm = true` in config:

1. `on_run_end` returns `llm_prompt = Some(...)` containing run metrics, last 50 iteration log lines, PRD summary (500 words), git diff, and existing rule-based findings
2. `run_feedback_on_end` captures `project_root` and `max_prompt_hints` at spawn time (to handle project switches), then spawns an ephemeral claude subprocess (60s timeout) with `EphemeralPurpose::FeedbackAnalysis`
3. The LLM responds with up to 5 structured blocks: `FINDING: / SCOPE: / SEVERITY:`
4. `EphemeralAgentComplete` handler calls `apply_llm_findings()` which:
   - Parses the response via `llm::parse_llm_response()`
   - Deduplicates against existing `prompt_hint` rules via `llm::dedup_findings()`
   - Writes new Provisional PromptHint rules to `rules.toml`
5. These Tier 3 rules are injected into the orchestrator agent's system prompt (up to 5 most recent) via `prompt_hints()` at every spawn/restart. Injection increments `trigger_count` so hints participate in the staleness lifecycle.
6. If the next run improves, they get promoted. If it regresses, they get rejected.

**Fire-and-forget:** If the LLM call fails or times out, Tier 1+2 findings are already persisted. Tier 3 is additive.

**Race condition handling:** If the user re-enables the orchestrator (potentially in a different project) before the ephemeral agent completes, the response handler uses the `project_root` captured at spawn time — NOT `self.orchestrator.project_root` which may have changed. The LLM findings go to the correct project's `rules.toml`. The in-memory RuleEngine for the new run won't see these findings; they take effect on the next `on_run_start()`.

### Script Generation (Tier 4 — async, after on_run_end)

When `script_generation = true` in config (default) and lower tiers have been tried (rules exist in any state) but waste/stuck rates exceed 33%:

1. `on_run_end` returns `script_prompt = Some(...)` containing run metrics, all 20 hook points, the full GlassApi reference, and instructions to write a Rhai script
2. `run_feedback_on_end` captures `project_root` at spawn time, then spawns an ephemeral agent (60s timeout) with `EphemeralPurpose::ScriptGeneration`
3. The LLM responds with one of: structured blocks (`SCRIPT_NAME:`, `SCRIPT_HOOKS:`, fenced `\`\`\`rhai` source), or a single `TOML_SUFFICIENT: ...` line if it judges a TOML rule is enough
4. `EphemeralAgentComplete` handler calls `glass_feedback::parse_script_response()` which returns one of `Script { name, hooks, source }`, `TomlSufficient`, or `Unparseable`
5. On `Script`, the handler writes a `.toml` manifest + `.rhai` source to `<project>/.glass/scripts/feedback/` with `status = "provisional"`
6. `script_bridge.reload()` picks up the new script for the next run
7. The script's lifecycle follows the same promotion/rejection path as rules (see Script Lifecycle below)

**Deduplication:** Before writing, the handler checks if a script with the same name already exists. If the existing script is not archived, the new one is skipped. Archived scripts are safe to overwrite.

**Parse failure suppression:** If `parse_script_response` returns `Unparseable` 3 times in a row, Tier 4 generation is suppressed until a successful parse resets the counter. `TomlSufficient` is a valid non-script outcome and resets the counter rather than incrementing it.

**Fire-and-forget:** Same pattern as Tier 3. If the ephemeral agent fails or times out, Tier 1-3 findings are already persisted.

## Scripting Engine (`glass_scripting` crate)

### Overview

The scripting layer lets Glass improve itself at runtime through Rhai scripts. Scripts hook into 20 event points across every Glass component and interact through a curated action API.

### Key Files

| File | Purpose |
|------|---------|
| `crates/glass_scripting/src/engine.rs` | Rhai engine setup, GlassApi custom type, sandbox config |
| `crates/glass_scripting/src/hooks.rs` | HookRegistry: maps HookPoint → sorted scripts |
| `crates/glass_scripting/src/loader.rs` | Load .toml manifest + .rhai source pairs from disk |
| `crates/glass_scripting/src/lifecycle.rs` | Promote, reject, record_failure, record_trigger, increment_stale |
| `crates/glass_scripting/src/mcp.rs` | ScriptToolRegistry for dynamic MCP tools |
| `crates/glass_scripting/src/profile.rs` | Export/import shareable profiles |
| `src/script_bridge.rs` | Bridge: owns ScriptSystem, routes events, executes actions, tracks lifecycle |

### Hook Points

Scripts subscribe to events via their `.toml` manifest's `hooks` array:

| Hook | Fires When | Event Data |
|------|-----------|------------|
| `CommandStart` | Command begins executing | command |
| `CommandComplete` | Command finishes | command, exit_code, duration_ms |
| `BlockStateChange` | Block state transition | — |
| `SnapshotBefore` | About to snapshot (can veto) | command |
| `SnapshotAfter` | Snapshot taken | — |
| `HistoryQuery` | Search executed | — |
| `HistoryInsert` | Command record stored | — |
| `PipelineComplete` | All pipe stages finished | — |
| `ConfigReload` | config.toml changed | — |
| `OrchestratorRunStart` | Ctrl+Shift+O on | — |
| `OrchestratorRunEnd` | Deactivation | iterations |
| `OrchestratorIteration` | Each silence→response cycle | iteration |
| `OrchestratorCheckpoint` | Checkpoint fired | — |
| `OrchestratorStuck` | Stuck detected | — |
| `McpRequest` | MCP tool call received | — |
| `McpResponse` | MCP tool result returned | — |
| `TabCreate` | New tab opened | tab_index |
| `TabClose` | Tab closed | tab_index |
| `SessionStart` | Glass launched | — |
| `SessionEnd` | Glass shutting down | — |

### GlassApi (the `glass` object in scripts)

Read-only methods (from per-hook snapshot, no DB queries):
- `glass.cwd()`, `glass.git_branch()`, `glass.git_dirty_files()`, `glass.config(key)`, `glass.active_rules()`

Action methods (queued, executed after script completes):
- `glass.commit(msg)`, `glass.log(level, msg)`, `glass.notify(msg)`, `glass.set_config(key, value)`, `glass.inject_prompt_hint(text)`, `glass.force_snapshot(paths)`, `glass.trigger_checkpoint(reason)`, `glass.extend_silence(secs)`

### Execution Model

1. Scripts sorted by priority: confirmed > user > provisional (reversed for McpRequest: user > confirmed > provisional)
2. Each script runs in its own Rhai Scope — no shared state
3. Actions collected into `Vec<Action>`, executed by bridge after all scripts complete
4. Failed scripts logged and skipped — other scripts still run
5. **SnapshotBefore:** AND aggregation — any confirmed/user script returning false vetoes
6. **McpRequest:** First-responder-wins — first script with non-empty actions stops the loop

### Sandbox Limits

```rust
engine.set_max_operations(100_000);     // primary computation bound
engine.set_max_string_size(1_048_576);  // 1MB per string
engine.set_max_array_size(10_000);
engine.set_max_map_size(10_000);
```

Configurable via `[scripting]` section with hard ceilings (compiled constants):
- `max_operations`: default 100,000, ceiling 1,000,000
- `max_timeout_ms`: default 2,000, ceiling 10,000
- `max_scripts_per_hook`: default 10, ceiling 25
- `max_total_scripts`: default 100, ceiling 500
- `max_mcp_tools`: default 20, ceiling 50

### Script Lifecycle

Mirrors the rule lifecycle. The ScriptBridge tracks per-run execution and calls lifecycle functions on run end:

```
Tier 4 generates script
    → Provisional (runs for one cycle)
        ↓ next run improved/neutral + script fired
      Confirmed (permanent, runs every session)
        ↓ no triggers for 5 runs
      Stale → (re-triggered) → Confirmed
        ↓ no triggers for 10 runs
      Archived (status in manifest, file stays in place)

      Provisional → (regression detected) → Archived
      Any status → (3 consecutive errors) → Archived
```

**Per-run tracking:** `ScriptBridge` maintains `scripts_triggered: HashSet` and `scripts_errored: HashMap` during each orchestrator run. Reset on run start.

**on_feedback_run_end(regressed):**
1. For each feedback-origin script (skip user-origin):
   - Errored ≥3 times this run → `lifecycle::record_failure` (auto-archive)
   - Provisional + regressed → `lifecycle::reject_script`
   - Provisional + not regressed + triggered → `lifecycle::promote_script`
   - Confirmed/Stale + triggered → `lifecycle::record_trigger` (resets failure count, resets stale)
   - Confirmed/Stale + not triggered → `lifecycle::increment_stale(path, 5, 10)`
2. Reset tracking counters
3. Reload scripts from disk

### Dynamic MCP Tools

Scripts with `type = "mcp_tool"` in their manifest register as MCP tools:
- `glass_script_tool` — static MCP tool that forwards to the Glass binary via IPC, which runs the named script
- `glass_list_script_tools` — returns all confirmed script tool definitions (name, description, params_schema)
- Provisional tools are registered but not advertised

### Profile Export/Import

```bash
glass profile export --name rust-backend    # bundles confirmed scripts + rules
glass profile import --path rust-backend.glassprofile  # imports as provisional
```

Imported profiles enter as provisional. The local feedback loop validates and promotes/rejects them.

### Safety Guards

- **ConfigReload loop guard:** `config_reload_guard` flag prevents ConfigReload → SetConfig → ConfigReload infinite recursion. Set on SetConfig, cleared on any non-ConfigReload hook.
- **Tier 4 deduplication:** New scripts are skipped if a non-archived script with the same name exists.
- **Parse failure suppression:** After 3 consecutive `parse_script_response` failures, Tier 4 ephemeral agent spawn is suppressed.
- **Scripts cannot:** access filesystem directly, spawn processes, write to PTY, modify renderer, delete history/snapshots. All side effects go through the curated Action API.

## Rule Status Lifecycle

```
Finding detected
    → Proposed → Provisional (applied with conservative cap)
                    ↓ next run improved/neutral
                 Confirmed (active, enforced every iteration)
                    ↓ no triggers for N runs
                 Stale → (re-triggered) → Confirmed
                    ↓ no triggers for M more runs
                 Archived (moved to archived-rules.toml)

                 Provisional → (regression detected) → Rejected → Archived
```

## Attribution & Ablation Testing

### Problem
Single-run before/after comparison can't establish causation. Rules that are present during a good run get promoted even if they didn't contribute ("passengers"). Over many runs, the confirmed set bloats with passengers that add overhead without benefit.

### Attribution Engine (`crates/glass_feedback/src/attribution.rs`)
Runs every run (cheap). Tracks which rules fired per run and correlates with metric deltas:

1. For each active rule, bucket the run into "fired" (`trigger_count > 0`) or "didn't fire"
2. Update rolling averages for metric deltas in each bucket
3. For rules with 5+ data points, compute `passenger_score`:
   - `benefit = avg_delta_when_not_fired - avg_delta_when_fired` (positive when rule helps)
   - `passenger_score = 1.0 - (benefit * 5.0).clamp(0.0, 1.0)`
   - 0.0 = clearly helpful, 1.0 = no detectable benefit

Scores persist to `.glass/rule-attribution.toml`. Pruned when rules are archived.

### Ablation Engine (`crates/glass_feedback/src/ablation.rs`)
Activates only when the system has converged (no provisional rules/scripts). Disables one confirmed rule per run and measures impact.

**Trigger conditions** (all must be true):
1. `ablation_enabled = true` in config
2. No provisional rules exist
3. At least one confirmed rule hasn't been tested this sweep (or `ablation_sweep_interval` runs since last sweep)

**Target selection:** Confirmed rules sorted by `passenger_score` descending (most suspicious first). Pinned rules are excluded.

**Evaluation:** Compare current metrics against 3-run rolling average. Same regression thresholds as the main guard (revert > 0.10, stuck > 0.05, waste > 0.10).

**Results:**
- **Needed** → rule stays Confirmed, `last_ablation_run` updated
- **Passenger** → rule demoted to Stale (5 more runs to resurrect before archival)
- **Concurrent regression** (unrelated cause) → conservatively marked "needed" (re-tested next sweep)

**Sweep lifecycle:**
1. System converges → ablation begins
2. One rule tested per run, ordered by passenger_score
3. After all confirmed rules tested → sweep idle
4. Re-sweep trigger: new rule confirmed, or `ablation_sweep_interval` runs elapsed (default 20)

### Design Principle
Attribution informs, ablation confirms. Attribution is too noisy to act on alone, but useful for prioritizing ablation targets. Only ablation can demote rules.

## Analyzer Detectors

| Detector | Fires When | Data Needed | Finding |
|----------|-----------|-------------|---------|
| `detect_silence_waste` | avg idle between iterations > 2× config timeout | `avg_idle_between_iterations_secs` | Reduce silence_timeout_secs |
| `detect_stuck_sensitivity` | stuck_count > 20% of iterations, low waste | `stuck_count` | Increase max_retries_before_stuck |
| `detect_stuck_leniency` | fingerprint sequence shows repeated states without stuck | `fingerprint_sequence` | Decrease max_retries_before_stuck |
| `detect_checkpoint_overhead` | checkpoint_count > 25% of iterations | `checkpoint_count` | Reduce checkpoint frequency |
| `detect_checkpoint_frequency` | iterations_since_checkpoint > 20 consistently | `checkpoint_count` | More frequent checkpoints |
| `detect_instruction_overload` | Agent responses have 4+ numbered items | `agent_responses` | Enable smaller_instructions rule |
| `detect_flaky_verification` | verify sequence alternates pass/fail | `verify_pass_fail_sequence` | Enable run_verify_twice rule |
| `detect_scope_creep` | >3 files changed outside PRD deliverables | `prd_content`, `git_diff_stat` | Enable restrict_scope rule |
| `detect_uncommitted_drift` | >5 iterations without a commit | `commit_count`, `iterations` | Enable force_commit rule |
| `detect_hot_files` | Same file reverted 3+ times | `reverted_files` | Enable isolate_commit rule |
| `detect_ordering_failure` | Stuck events followed by reverts in TSV | `iterations_tsv` | Enable dependency blocking |

## Config Reference

```toml
[agent]
mode = "Assist"                    # Agent mode (Off/Watch/Assist/Autonomous)

[agent.orchestrator]
enabled = true                     # Whether orchestrator config section exists
silence_timeout_secs = 30          # Slow fallback silence threshold
fast_trigger_secs = 5              # Fast trigger after output stops
prd_path = "PRD.md"                # Relative path to project plan
checkpoint_path = ".glass/checkpoint.md"
max_retries_before_stuck = 3       # N identical responses = stuck
orchestrator_mode = "build"        # "build" | "general"
verify_mode = "floor"              # "floor" | "files" | "off"
verify_command = ""                # Override auto-detected verify command
verify_files = []                  # Files to check (general mode)
completion_artifact = ".glass/done"
max_iterations = 120               # Bounded run limit (0 = unlimited)
checkpoint_interval = 15           # Iterations between auto context refresh (5-100)
agent_prompt_pattern = ""          # Regex for instant prompt detection
feedback_llm = false               # Enable LLM qualitative analysis (Tier 3 prompt hints)
max_prompt_hints = 10              # Max Tier 3 prompt hint rules per project
ablation_enabled = true            # Enable automatic ablation testing of confirmed rules
ablation_sweep_interval = 20       # Runs between re-sweeps after full ablation coverage

[scripting]
enabled = true                     # Master switch for scripting engine
max_operations = 100000            # Per script operation limit (hard ceiling: 1000000)
max_timeout_ms = 2000              # Wall-clock safety net (hard ceiling: 10000)
max_scripts_per_hook = 10          # Per hook point (hard ceiling: 25)
max_total_scripts = 100            # Across all hooks (hard ceiling: 500)
max_mcp_tools = 20                 # Dynamic MCP tools (hard ceiling: 50)
script_generation = true           # Allow feedback loop to generate Tier 4 scripts
```

## Constants

| Constant | Value | Location | Purpose |
|----------|-------|----------|---------|
| `AUTO_CHECKPOINT_INTERVAL` | 15 | orchestrator.rs:378 | Default iterations before auto-checkpoint (overridden by `checkpoint_interval` config) |
| `CRASH_RECOVERY_GRACE_SECS` | 10 | orchestrator.rs:381 | Ignore PromptStart after typing |
| `DEPENDENCY_BLOCK_MAX_ITERATIONS` | 3 | orchestrator.rs:384 | Auto-clear dependency block |
| `SYNTHESIS_TIMEOUT_SECS` | 120 | orchestrator.rs:387 | Fallback if ephemeral agent hangs |
| `CONTEXT_LINES_ON_ERROR` | 30 | orchestrator.rs:1002 | Terminal lines when command failed + SOI |
| `CONTEXT_LINES_ON_SUCCESS` | 20 | orchestrator.rs:1003 | Terminal lines when command succeeded + SOI |
| `CONTEXT_LINES_FALLBACK` | 80 | orchestrator.rs:1004 | Terminal lines when no SOI data |

## Things to Know

- **`project_root` is captured at Ctrl+Shift+O time.** The shell's OSC 7 CWD stops updating once Claude Code starts, so all file operations use the stored `project_root`, not live CWD.
- **All git commands use `git_cmd()`** which adds `CREATE_NO_WINDOW` on Windows to prevent console flashing.
- **The Glass Agent cannot write code.** It has full MCP tool access for observability (history, context, diffs, queries) but must instruct Claude Code (running in the terminal) to do implementation work.
- **Deferred TypeText is a Vec, not Option.** Multiple responses can queue up while a block is executing. They flush one at a time on each silence trigger.
- **Metric guard reverts use `git reset --hard`.** The `last_good_commit` is captured at the start of each iteration before verification runs.
- **Iterations.tsv format:** `iteration\tcommit\tfeature\t(metric)\tstatus\tdescription` — note the empty metric column (index 3), status is at index 4.
- **Post-mortem timestamp** uses a leap-year-aware date calculation (not chrono).
- **Global rules sync is bidirectional:** confirmed/provisional global rules are upserted to `~/.glass/global-rules.toml`; rejected/stale ones are removed.
- **`trigger_count` is reset to 0 at the start of each run** in `on_run_start`. It tracks per-run firing for accurate staleness detection.
- **Feedback LLM is fire-and-forget.** The ephemeral agent runs in the background after deactivation. If the user re-enables orchestrator before it completes, the response handler uses the project root captured at spawn time, not the current one. LLM findings take effect on the next `on_run_start`, not the current run.
- **Four finding tiers:** Tier 1 = config tuning (adjusts config.toml), Tier 2 = behavioral rules (force_commit, split_instructions, etc.), Tier 3 = LLM prompt hints (qualitative advice injected into agent context), Tier 4 = LLM-generated Rhai scripts (new logic loaded at runtime). Tiers 1+2 are synchronous in `on_run_end`. Tiers 3+4 are async via ephemeral agents.
- **Script lifecycle mirrors rule lifecycle.** Provisional → Confirmed (on improvement) or → Archived (on regression/errors/staleness). The `ScriptBridge` tracks per-run triggers/errors and calls `glass_scripting::lifecycle` functions in `on_feedback_run_end`.
- **Scripts run on the main thread.** The Rhai `max_operations` limit is the primary bound. A `max_timeout_ms` wall-clock safety net is configured but execution is synchronous — heavy scripts could briefly block rendering.
- **Project scripts override global scripts** with the same name. `load_all_scripts` loads project-local first, then global, skipping name duplicates.
- **ConfigReload loop guard** prevents scripts from causing infinite config reload cycles. The `config_reload_guard` flag is set when `SetConfig` executes and cleared on the next non-ConfigReload hook.
- **Attribution runs every run, ablation only when converged.** Attribution is cheap (just logging + math). Ablation only activates when no provisional rules exist, preventing confounded results.
- **Ablation uses 3-run rolling average** for evaluation, not single-run comparison. This reduces noise from anomalous runs.
- **Ablation passengers are demoted to Stale, not archived immediately.** This gives 5 runs for the rule to resurrect if project conditions change (e.g., different phase of development).
- **Run IDs are lexicographically comparable** (`run-{unix_timestamp}`) because Unix timestamps have consistent digit counts. Ablation sweep tracking relies on this for `last_ablation_run` comparisons.

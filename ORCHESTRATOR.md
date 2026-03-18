# Glass Orchestrator — Complete Architecture Reference

Read this file to understand the orchestrator mode before making any changes. This is the authoritative reference — do not ask the user to re-explain what's documented here.

## What It Does

The orchestrator drives Claude Code sessions autonomously. It spawns a separate "Glass Agent" (a claude subprocess) that reviews terminal output, makes product decisions, and types instructions into Claude Code running in the terminal. The flow is: silence detected → capture terminal context → send to Glass Agent → agent responds with instruction → type into PTY → repeat.

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
| `crates/glass_core/src/agent_runtime.rs` | Agent command args, system prompt building, activity stream |

## The Main Loop

```
User presses Ctrl+Shift+O
    → Orchestrator activates
    → Glass Agent subprocess spawns with system prompt + handoff
    → Kickoff phase begins (suppress loop while user chats)

User stops typing for 30s → kickoff completes

SmartTrigger fires (silence detected)
    → OrchestratorSilence event
    → Guard checks: active? agent alive? response_pending? kickoff?
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
        TypeText → type into PTY (or defer if kickoff/executing)
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
6. Fresh kickoff phase begins

## Orchestrator Modes

Set via `orchestrator_mode` in config. Auto-detected at activation.

| Mode | When | Agent Tools | Agent Role |
|------|------|-------------|------------|
| **build** | Cargo.toml, package.json, etc. exist | glass_query, glass_context (observe only) | Guide Claude Code through plan→implement→commit→verify |
| **general** | PRD has deliverables but no code project | glass_query, glass_context | Orchestrate research/planning/design, track by deliverable files |
| **audit** | Manual selection | All MCP tools (tab control, history, etc.) | Test features interactively via MCP, delegate code fixes |

## Kickoff Flow

The kickoff phase prevents the orchestrator from interrupting while the user chats with Claude Code.

1. **Ctrl+Shift+O pressed** → `kickoff_complete = false`, `last_user_keypress = None`
2. **Glass prints visible message** → `[GLASS] Orchestrator active. No PRD found — describe what you want to build...` (or PRD continue prompt)
3. **Glass Agent spawns** → sends initial response, but TypeText is **deferred** during kickoff (not typed into PTY)
4. **User types** → `mark_user_keypress()` called on each keypress, resets idle timer
5. **Silence fires** → kickoff guard suppresses:
   - `last_user_keypress.is_none()` → suppress (waiting for first input)
   - `user_recently_active(30s)` → suppress (user still typing)
6. **User goes idle 30s** → `kickoff_complete = true`, deferred text flushes, autonomous loop begins

**Critical:** During kickoff, ALL TypeText responses are deferred to `deferred_type_text`. The deferred flush only runs AFTER the kickoff guard passes. This prevents the Glass Agent from interrupting the user mid-conversation.

## Silence Detection (SmartTrigger)

Four trigger modes in priority order:

1. **Prompt regex** — instant fire when terminal output matches `agent_prompt_pattern` config
2. **Shell prompt (OSC 133;A)** — instant fire when shell returns to prompt
3. **Fast trigger** — fires `fast_trigger_secs` (default 5) after output stops flowing
4. **Slow fallback** — fires every `silence_timeout_secs` (default 30) periodically

The SmartTrigger lives in the PTY reader thread and sends `AppEvent::OrchestratorSilence` to the main thread.

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

When a checkpoint fires (agent-requested, auto after 15 iterations, or bounded limit):

1. `trigger_checkpoint_synthesis()` gathers git state, iterations log, metric summary
2. Builds fallback checkpoint content (pure Rust, no AI)
3. Spawns ephemeral claude subprocess for AI-synthesized checkpoint (120s timeout)
4. On completion (or timeout/failure): writes `.glass/checkpoint.md`
5. Kills current Glass Agent, spawns fresh agent with handoff: "Read .glass/checkpoint.md and continue"
6. Resets stuck detection, iterations_since_checkpoint counter

**Checkpoint.md contains:** completed work summary, current errors, abandoned approaches, key decisions, git state, next steps.

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
  --system-prompt-file ~/.glass/agent-system-prompt.txt
  --mcp-config ~/.glass/agent-mcp.json
  --allowedTools <mode-specific tools>
  --dangerously-skip-permissions
  --disable-slash-commands
```

- **stdin:** JSON messages (context sends from orchestrator)
- **stdout:** stream-json (parsed by reader thread → AppEvents)
- **stderr:** null (prevents deadlock from stderr buffer fill)
- **Windows:** CREATE_NO_WINDOW flag
- **Crash recovery:** 3 restart attempts with exponential backoff (5s → 15s → 45s)
- **Generation tracking:** Each respawn increments `agent_generation` to filter stale AgentCrashed events

## Usage Tracking

Background thread polls Anthropic OAuth usage API every 60 seconds.

| Threshold | Event | Action |
|-----------|-------|--------|
| >= 95% | UsageHardStop | Write emergency checkpoint, deactivate |
| >= 80% | UsagePause | Deactivate, skip ephemeral agents |
| < 20% | UsageResume | Log only — user must re-enable manually |

## Deferred TypeText

TypeText responses are buffered (not typed immediately) in two cases:
1. **During kickoff** — user is still chatting, agent responses queue up
2. **Block executing** — Claude Code is actively running a command

The deferred queue is flushed one item at a time on each silence trigger, AFTER the kickoff guard passes. Each flush types one deferred message and returns (letting the terminal process it before the next).

## Course Correction (Nudge)

While the orchestrator is running, the user can write `.glass/nudge.md` in the project root. On the next silence trigger, the orchestrator reads it, includes it as `[USER_NUDGE]` in the context sent to the Glass Agent, then deletes the file.

---

# Self-Improvement Feedback Loop

## Overview

The feedback loop analyzes each orchestrator run and produces findings that tune future runs. It operates across three tiers:

1. **Tier 1: Config Tuning** — adjusts `config.toml` values (silence timeout, max retries, etc.)
2. **Tier 2: Behavioral Rules** — adds rules to `rules.toml` (force_commit, split_instructions, etc.)
3. **Tier 3: Prompt Hints** — injects text into the agent's context

## Files Created

### Per-project (`<project_root>/.glass/`)

| File | Purpose | Created By |
|------|---------|------------|
| `rules.toml` | Active behavioral rules (provisional/confirmed) | `on_run_end()` |
| `run-metrics.toml` | Historical run metrics (one entry per run) | `on_run_end()` |
| `tuning-history.toml` | Config snapshots at each run start | `on_run_start()` |
| `archived-rules.toml` | Rules that were rejected or went stale | `on_run_end()` |
| `iterations.tsv` | Per-iteration log (TSV: iteration, commit, feature, metric, status, description) | `append_iteration_log()` during run |
| `checkpoint.md` | Last checkpoint for agent context handoff | Checkpoint synthesis |
| `postmortem-YYYYMMDD-HHMMSS.md` | Run summary report | `generate_postmortem()` on Done/deactivate |
| `nudge.md` | User course correction (read and deleted per iteration) | User-created |
| `handoff.md` | User notes for next orchestrator activation (read and deleted) | User-created |
| `done` | Completion artifact signal (configurable path) | Agent creates, orchestrator deletes |

### Global (`~/.glass/`)

| File | Purpose |
|------|---------|
| `global-rules.toml` | Rules with `scope = "global"` synced across all projects |
| `agent-system-prompt.txt` | Last-written Glass Agent system prompt |
| `agent-mcp.json` | MCP config pointing to `glass mcp serve` |
| `agent-diag.txt` | Spawn diagnostics (PATH, args, success/failure) |

## Feedback Lifecycle

### on_run_start (called when orchestrator activates)
1. Compute file paths under `<project>/.glass/` and `~/.glass/`
2. Load merged rule engine from project `rules.toml` + global `global-rules.toml`
3. Reset all `trigger_count` to 0 (per-run firing tracking)
4. Snapshot current config values to `tuning-history.toml`
5. Return `FeedbackState` handle

### check_rules (called every iteration during OrchestratorSilence)
- `RuleEngine` evaluates all active rules against live `RunState`
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
3. **Regression check** — compare current metrics vs previous run's baseline
4. **Promote or reject** provisional rules:
   - Improved/Neutral → promote to Confirmed
   - Regressed → reject all provisionals, archive them
5. **Apply new findings** — create new Provisional rules from detector findings
6. **Staleness** — increment stale_runs for rules that didn't fire; archive after threshold
7. **Drift** — detect worsening trends over last 3 runs
8. **Config tuning** — extract Tier 1 findings → write to config.toml (max 1 per run)
9. **Build LLM prompt** — if `feedback_llm = true`, build analysis prompt from run data + existing findings (returned in `FeedbackResult.llm_prompt`)
10. **Persist** — save rules.toml, run-metrics.toml, archived-rules.toml
11. **Sync global** — copy global-scoped rules to `~/.glass/global-rules.toml`; remove rejected/stale ones

### Feedback LLM (Tier 3 — async, after on_run_end)

When `feedback_llm = true` in config:

1. `on_run_end` returns `llm_prompt = Some(...)` containing run metrics, last 50 iteration log lines, PRD summary (500 words), git diff, and existing rule-based findings
2. `run_feedback_on_end` captures `project_root` and `max_prompt_hints` at spawn time (to handle project switches), then spawns an ephemeral claude subprocess (60s timeout) with `EphemeralPurpose::FeedbackAnalysis`
3. The LLM responds with up to 5 structured blocks: `FINDING: / SCOPE: / SEVERITY:`
4. `EphemeralAgentComplete` handler calls `apply_llm_findings()` which:
   - Parses the response via `llm::parse_llm_response()`
   - Deduplicates against existing `prompt_hint` rules via `llm::dedup_findings()`
   - Writes new Provisional PromptHint rules to `rules.toml`
5. These Tier 3 rules get injected into the Glass Agent's context on future runs via `prompt_hints()`
6. If the next run improves, they get promoted. If it regresses, they get rejected.

**Fire-and-forget:** If the LLM call fails or times out, Tier 1+2 findings are already persisted. Tier 3 is additive.

**Race condition handling:** If the user re-enables the orchestrator (potentially in a different project) before the ephemeral agent completes, the response handler uses the `project_root` captured at spawn time — NOT `self.orchestrator.project_root` which may have changed. The LLM findings go to the correct project's `rules.toml`. The in-memory RuleEngine for the new run won't see these findings; they take effect on the next `on_run_start()`.

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
orchestrator_mode = "build"        # "build" | "general" | "audit"
verify_mode = "floor"              # "floor" | "files" | "off"
verify_command = ""                # Override auto-detected verify command
verify_files = []                  # Files to check (general mode)
completion_artifact = ".glass/done"
max_iterations = 120               # Bounded run limit (0 = unlimited)
agent_prompt_pattern = ""          # Regex for instant prompt detection
feedback_llm = false               # Enable LLM qualitative analysis (Tier 3 prompt hints)
max_prompt_hints = 10              # Max Tier 3 prompt hint rules per project
```

## Constants

| Constant | Value | Location | Purpose |
|----------|-------|----------|---------|
| `AUTO_CHECKPOINT_INTERVAL` | 15 | orchestrator.rs:377 | Iterations before auto-checkpoint |
| `CRASH_RECOVERY_GRACE_SECS` | 10 | orchestrator.rs:381 | Ignore PromptStart after typing |
| `DEPENDENCY_BLOCK_MAX_ITERATIONS` | 3 | orchestrator.rs:384 | Auto-clear dependency block |
| `SYNTHESIS_TIMEOUT_SECS` | 120 | orchestrator.rs:387 | Fallback if ephemeral agent hangs |
| `CONTEXT_LINES_ON_ERROR` | 30 | orchestrator.rs:1002 | Terminal lines when command failed + SOI |
| `CONTEXT_LINES_ON_SUCCESS` | 20 | orchestrator.rs:1003 | Terminal lines when command succeeded + SOI |
| `CONTEXT_LINES_FALLBACK` | 80 | orchestrator.rs:1004 | Terminal lines when no SOI data |

## Things to Know

- **`project_root` is captured at Ctrl+Shift+O time.** The shell's OSC 7 CWD stops updating once Claude Code starts, so all file operations use the stored `project_root`, not live CWD.
- **All git commands use `git_cmd()`** which adds `CREATE_NO_WINDOW` on Windows to prevent console flashing.
- **The Glass Agent cannot write code.** In build/general mode it only has `glass_query` and `glass_context` tools. It must instruct Claude Code (running in the terminal) to do implementation work.
- **Deferred TypeText is a Vec, not Option.** Multiple responses can queue up during kickoff or while a block is executing. They flush one at a time on each silence trigger.
- **Metric guard reverts use `git reset --hard`.** The `last_good_commit` is captured at the start of each iteration before verification runs.
- **Iterations.tsv format:** `iteration\tcommit\tfeature\t(metric)\tstatus\tdescription` — note the empty metric column (index 3), status is at index 4.
- **Post-mortem timestamp** uses a leap-year-aware date calculation (not chrono).
- **Global rules sync is bidirectional:** confirmed/provisional global rules are upserted to `~/.glass/global-rules.toml`; rejected/stale ones are removed.
- **`trigger_count` is reset to 0 at the start of each run** in `on_run_start`. It tracks per-run firing for accurate staleness detection.
- **Feedback LLM is fire-and-forget.** The ephemeral agent runs in the background after deactivation. If the user re-enables orchestrator before it completes, the response handler uses the project root captured at spawn time, not the current one. LLM findings take effect on the next `on_run_start`, not the current run.
- **Three finding tiers:** Tier 1 = config tuning (adjusts config.toml), Tier 2 = behavioral rules (force_commit, split_instructions, etc.), Tier 3 = LLM prompt hints (qualitative advice injected into agent context). Tiers 1+2 are synchronous in `on_run_end`. Tier 3 is async via ephemeral agent.

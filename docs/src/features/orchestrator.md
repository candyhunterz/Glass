# Orchestrator Mode

Orchestrator Mode drives autonomous project development by pairing two AI agents: Claude Code (the implementer, running in the PTY) and the Glass Agent (the reviewer/guide, running as a background subprocess). Glass manages the feedback loop between them, enabling overnight project builds from a PRD or unattended completion of in-progress work.

---

## Overview

Orchestrator Mode has two phases: **kickoff** (interactive) and **autonomous loop**.

When you press Ctrl+Shift+O, Glass enters the kickoff phase. You interact directly with the AI agent running in the terminal — answer questions, describe your task, provide context. Glass tracks your keyboard activity and suppresses the autonomous loop while you're engaged. Once both you and the terminal have been idle for the silence threshold (default 30 seconds), the kickoff phase ends and the autonomous loop begins.

During the autonomous loop, Glass monitors the PTY for silence. When the agent finishes working, Glass captures terminal context and sends it to the Glass Agent (a background reviewer process) for review. The Glass Agent decides the next step and Glass types its instructions back into the terminal. This cycle repeats until the project is complete.

The Glass Agent's system prompt includes:
- The project plan (from PRD.md, truncated to 4000 words with a notice if truncated)
- Current progress (from .glass/checkpoint.md)
- Iteration history (last 50 entries from .glass/iterations.tsv)
- The iteration protocol: PLAN, IMPLEMENT, COMMIT, VERIFY, DECIDE

---

## Kickoff Phase

The kickoff phase prevents the orchestrator from taking over while you're still setting up the task. This is model-agnostic — it works with any AI agent running in the terminal, not just Claude Code.

**How it works:**

1. Press **Ctrl+Shift+O** — orchestrator activates, Glass Agent spawns in the background
2. If a PRD exists, Glass displays a prompt asking whether to continue or start fresh
3. You interact directly with the AI agent in the terminal — answer questions, type your task description, clarify requirements
4. Glass tracks your keyboard activity. As long as you've typed recently (within the silence threshold), the autonomous loop is suppressed
5. Once you stop typing and the terminal goes silent for the full threshold duration, kickoff ends
6. The Glass Agent takes over and begins the autonomous feedback loop

**Key behaviors during kickoff:**

- You can have as many back-and-forth exchanges as needed — there is no limit
- The orchestrator waits for you to finish naturally, not for a specific key or number of inputs
- If you haven't typed at all since activation, the orchestrator will not start (it waits for at least one interaction)

---

## Workflows

### Example: Planning a Japan Trip

A concrete example of the kickoff-to-autonomous flow:

1. Open Glass, start your AI agent
2. Press **Ctrl+Shift+O** — orchestrator activates
3. Glass shows: "Found existing PRD. Continue? (y/n)"
4. You type **N**, Enter
5. Agent asks: "Starting fresh. What would you like to focus on?"
6. Silence fires — **suppressed** (you haven't typed yet)
7. You type: "Plan a 2-week Japan trip for October. Tokyo, Kyoto, Osaka. Budget $5k. Street food and temples."
8. Agent asks: "Any dietary restrictions? Day-by-day or just highlights?"
9. Silence fires — **suppressed** (you typed recently)
10. You type: "No restrictions. Day-by-day with restaurant recs. Hotels near train stations."
11. Agent starts working on the PRD, you sit back
12. Silence fires — you typed before but have been idle for the full threshold — **kickoff complete**
13. Glass Agent takes over — reviews terminal context, begins driving the agent autonomously: review, instruct, verify, next item

### Fresh Project from PRD

1. Write `PRD.md` in your project root describing what to build
2. Open Glass in the project directory
3. Start your AI agent (e.g., `claude --dangerously-skip-permissions`)
4. Press **Ctrl+Shift+O** to enable orchestration
5. The agent may ask clarifying questions — answer them at your own pace
6. Once you stop typing and the terminal goes quiet, the Glass Agent takes over

### Mid-Work Handoff

Already working on something and want to hand it off overnight:

1. Write `.glass/handoff.md` with your instructions (e.g., "finish the auth module, then add tests")
2. Press **Ctrl+Shift+O**

The orchestrator captures:
- Your terminal context (last 100 lines)
- Recent git history (last 10 commits)
- Your handoff note

The Glass Agent picks up where you left off.

### Course Correction

While the orchestrator is running, write `.glass/nudge.md` with new instructions. The orchestrator picks it up on the next silence cycle and injects it as a `[USER_NUDGE]` in the context sent to the agent. The file is deleted after reading.

---

## Autonomous Loop

Once kickoff is complete, the autonomous feedback loop runs:

```
┌─────────────────────────────────────────────────────────┐
│                    Glass Orchestrator                    │
│                                                         │
│  1. PTY goes silent (30s default)                       │
│  2. Capture last 100 lines of terminal output           │
│  3. Send [TERMINAL_CONTEXT] to Glass Agent              │
│  4. Agent responds:                                     │
│     • Text → type into PTY as instructions              │
│     • GLASS_WAIT → check again after next silence       │
│     • GLASS_CHECKPOINT → refresh context cycle          │
│     • GLASS_DONE → stop orchestration                   │
│     • GLASS_VERIFY → report verification commands       │
│  5. Repeat from step 1                                  │
└─────────────────────────────────────────────────────────┘
```

---

## Checkpoint Cycle

Long-running orchestration sessions need periodic context refresh to prevent the Glass Agent from hitting its context limit.

**Automatic triggers:**
- The agent emits `GLASS_CHECKPOINT: {"completed": "...", "next": "..."}` after completing a feature
- Glass auto-triggers a checkpoint every 15 iterations if the agent hasn't checkpointed

**Refresh process:**
1. Glass tells Claude Code to commit pending changes and write a status update to `.glass/checkpoint.md`
2. Glass polls the checkpoint file's modification time
3. Once updated (or after 180 seconds), the Glass Agent subprocess is killed
4. A new Glass Agent is spawned with a fresh system prompt containing the updated checkpoint
5. The new agent receives a `[ORCHESTRATOR_CHECKPOINT_REFRESH]` handoff message and continues

---

## Metric Guard

The metric guard prevents the agent from introducing regressions. After each orchestrator iteration, Glass runs verification commands and compares results against a baseline captured when orchestration started.

### Auto-Detection

Glass auto-detects verification commands based on project marker files:

| Marker File | Verify Command |
|---|---|
| `Cargo.toml` | `cargo test` |
| `package.json` with `"test"` script | `npm test` |
| `pyproject.toml` or `setup.py` | `pytest` |
| `go.mod` | `go test ./...` |
| `tsconfig.json` | `npx tsc --noEmit` |
| `Makefile` with `test` target | `make test` |

Users can override auto-detection with `verify_command` in config. Set `verify_mode = "disabled"` to turn off the metric guard entirely.

### Regression Detection

The metric guard tracks a "floor" for each verification command:
- **Pass count dropped** — regression
- **Fail count increased** — regression
- **Exit code went from 0 to non-zero** — regression (build broke)
- **Tests added (pass count increased, fail count unchanged)** — floor rises

### Auto-Revert

When regression is detected, Glass:
1. Reverts all changes via `git reset --hard` to the last known good commit
2. Sends a `[METRIC_GUARD]` message to the Glass Agent with error details
3. The agent instructs Claude Code to try a different approach

### Agent Discovery

Agents can report additional verification commands via `GLASS_VERIFY`:
```
GLASS_VERIFY: {"commands": [{"name": "integration", "cmd": "./scripts/integration-test.sh"}]}
```
Agent-discovered commands are appended to auto-detected ones. The agent cannot remove or replace auto-detected commands.

---

## Artifact-Based Completion

An optional file path that, when created or modified, triggers the orchestrator immediately. More deterministic than silence detection.

- Default path: `.glass/done` (configurable via `completion_artifact`)
- When the file is created, Glass fires an `OrchestratorSilence` event instantly
- The file is deleted after processing (one-shot signal)
- The Glass Agent's system prompt instructs agents to write this file when done

Set `completion_artifact = ""` in config to disable.

---

## Bounded Iteration Mode

Optionally limit orchestration to N iterations, then gracefully checkpoint and stop.

- Configure via `max_iterations` in `[agent.orchestrator]` (omit or set to 0 for unlimited)
- When the limit is reached, Glass triggers a checkpoint cycle, prints a summary, and deactivates
- The summary includes iteration count and metric guard stats (kept/reverted counts, test baseline vs. current)
- The iteration counter is NOT reset on re-enable — to run another batch, increase `max_iterations`

---

## Safety Features

### Stuck Detection

If the agent sends 3 identical responses in a row (configurable via `max_retries_before_stuck`), the orchestrator tells Claude Code to stash its changes and try a fundamentally different approach. The stuck detection buffer is reset after each checkpoint or different response.

### Crash Recovery

If Claude Code exits unexpectedly (detected via shell prompt-start events), the orchestrator restarts it with a prompt to read `.glass/checkpoint.md` and continue. A 10-second grace period after the orchestrator types into the PTY prevents false crash detections.

### Usage Tracking

Glass polls the Anthropic OAuth usage API every 60 seconds:

| Utilization | Action |
|---|---|
| >= 95% (5-hour window) | **Hard stop**: write emergency checkpoint, pause orchestrator |
| >= 80% (5-hour window) | **Pause**: disable orchestrator, user must re-enable manually |
| < 20% (5-hour window) | **Resume signal**: usage event sent (user still re-enables manually) |

### Kickoff Guard

When orchestration is first enabled, the autonomous loop is suppressed until the user has finished interacting with the AI agent. Glass tracks the timestamp of the user's last keypress. During the kickoff phase:

- If the user hasn't typed at all yet, the silence trigger is suppressed
- If the user typed recently (within the silence threshold), the silence trigger is suppressed
- Once the user has typed and then gone idle for the full threshold, kickoff ends and the autonomous loop begins

This is model-agnostic — it tracks user keyboard activity, not any specific agent's prompt format.

### Backpressure

Context sends are gated by a `response_pending` flag. While waiting for the Glass Agent to respond, additional silence events do not trigger new context sends. This flag is also set during agent handoff (spawn + initial message) to prevent premature silence triggers before the agent has responded.

---

## Files

| File | Purpose | Lifecycle |
|---|---|---|
| `PRD.md` | Project plan | User-created, read on agent spawn |
| `.glass/checkpoint.md` | Progress checkpoint | Written by Claude Code, read on agent spawn |
| `.glass/handoff.md` | Handoff instructions | User-created, read on enable, deleted after agent starts |
| `.glass/nudge.md` | Course correction | User-created, read on next silence, deleted after |
| `.glass/iterations.tsv` | Iteration log | Appended each iteration, included in system prompt (last 50) |
| `.glass/done` | Completion signal | Written by agent, triggers orchestrator, deleted after processing |

---

## Feedback Loop

The orchestrator learns from each run. After orchestration stops, a rule-based analyzer examines the run's metrics and produces findings across three tiers:

**Tier 1 — Config Tuning:** Findings that map directly to config values. If silence timeout was too short (agent got interrupted), Glass increases it. If stuck detection was too sensitive, Glass raises the threshold. Applied automatically, protected by a regression guard.

**Tier 2 — Behavioral Rules:** Runtime rules injected as text instructions in the agent context. Examples: "Commit src/main.rs in isolation" (hot file detected), "Commit current changes before continuing" (uncommitted drift), "Give ONE instruction per response" (instruction overload). These are model-agnostic — any AI agent follows them.

**Tier 3 — Prompt Hints (opt-in):** When `feedback_llm = true`, an LLM analyzes the run qualitatively and produces hints like "This project's tests are flaky on first run." Capped at 10 per project. Requires an extra API call.

### Rule Lifecycle

Every rule goes through a guarded lifecycle:

```
proposed → provisional → confirmed → stale → archived
              ↓               ↓         ↓
           rejected      provisional  confirmed
                          (env drift)  (re-triggered)
```

- **Provisional → Confirmed:** Next run's metrics didn't regress
- **Provisional → Rejected:** Next run's metrics regressed — rule and config rolled back
- **Confirmed → Stale:** Rule hasn't triggered in 10 runs
- **Stale → Archived:** Stale for 5 more runs, moved to archived file

### Regression Guard

Before each run, Glass snapshots the current config and provisional rules. After the run, it compares metrics (revert rate, stuck rate, waste rate). If any metric regressed, all provisional changes are rolled back and marked rejected.

Safety constraints:
- Max 3 provisional rules per run
- Max 1 config value change per run
- Rejected changes get a 5-run cooldown before re-proposal
- User can pin rules with `status = "pinned"` to prevent auto-revert

### Default Rules

Glass ships with 6 default rules that enter each project as provisional:

| Rule | Action |
|---|---|
| Uncommitted drift (5+ iterations) | Force commit |
| Hot file (3+ reverts) | Isolate commits |
| Instruction overload (4+ per response) | One instruction per response |
| Flaky verification | Run verify twice |
| High revert rate (>30%) | Smaller instructions |
| High waste rate (>15%) | Verify progress |

### Files

| File | Purpose |
|---|---|
| `.glass/rules.toml` | Project rules with lifecycle state |
| `.glass/run-metrics.toml` | Last 20 run metrics |
| `.glass/tuning-history.toml` | Config snapshots for rollback |
| `.glass/archived-rules.toml` | Pruned stale/rejected rules |
| `~/.glass/global-rules.toml` | Cross-project rules |
| `~/.glass/default-rules.toml` | Shipped defaults |

---

## Configuration

```toml
[agent.orchestrator]
enabled = false                # Enable orchestrator (toggled at runtime with Ctrl+Shift+O)
silence_timeout_secs = 30      # Seconds of PTY silence before sending context to agent
prd_path = "PRD.md"            # Path to project requirements document
checkpoint_path = ".glass/checkpoint.md"  # Path to checkpoint file
max_retries_before_stuck = 3   # Identical responses before stuck detection triggers
verify_mode = "floor"          # "floor" (auto-detect + guard) or "disabled"
# verify_command = "cargo test" # Optional override (skips auto-detect)
completion_artifact = ".glass/done"  # File path that triggers orchestrator when created
# max_iterations = 25          # Optional iteration limit (omit or 0 for unlimited)
# Feedback loop
feedback_llm = false           # Enable LLM qualitative analysis after each run (opt-in)
# max_prompt_hints = 10        # Max Tier 3 prompt hints per project
```

The orchestrator requires Agent Mode to be configured (the `[agent]` section). The Glass Agent subprocess uses the same Claude CLI as Agent Mode.

---

## Status Bar

When orchestrating, the status bar shows:
- `[orchestrating | iter #N]` — current iteration number
- Usage display: `5h: 42% | 7d: 15%` — OAuth API utilization (color-coded: green < 70%, yellow 70-85%, red 85%+)
- `PAUSED` — shown when usage limits triggered a pause

---

## Requirements

- Claude CLI must be installed and available on `PATH`
- Agent Mode must be configured (`[agent]` section in `~/.glass/config.toml`)
- A `PRD.md` file in the project root (recommended but not required — a warning is logged if missing)

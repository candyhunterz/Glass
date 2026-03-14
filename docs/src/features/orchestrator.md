# Orchestrator Mode

Orchestrator Mode drives autonomous project development by pairing two AI agents: Claude Code (the implementer, running in the PTY) and the Glass Agent (the reviewer/guide, running as a background subprocess). Glass manages the feedback loop between them, enabling overnight project builds from a PRD or unattended completion of in-progress work.

---

## Overview

When orchestration is enabled (Ctrl+Shift+O), Glass monitors the PTY for silence. When Claude Code finishes working, Glass captures terminal context and sends it to the Glass Agent for review. The agent decides the next step and Glass types its instructions back into the terminal. This cycle repeats until the project is complete.

The Glass Agent's system prompt includes:
- The project plan (from PRD.md, truncated to 4000 words with a notice if truncated)
- Current progress (from .glass/checkpoint.md)
- Iteration history (last 50 entries from .glass/iterations.tsv)
- The iteration protocol: PLAN, IMPLEMENT, COMMIT, VERIFY, DECIDE

---

## Workflows

### Fresh Project from PRD

1. Write `PRD.md` in your project root describing what to build
2. Open Glass in the project directory
3. Start Claude Code: `claude --dangerously-skip-permissions`
4. Press **Ctrl+Shift+O** to enable orchestration

The orchestrator reads the PRD, builds a system prompt for the Glass Agent, captures your terminal context, and starts the feedback loop.

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

## Feedback Loop

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

### User Override

Any keyboard input while orchestrating automatically disables the orchestrator. Press Ctrl+Shift+O to re-enable.

### Backpressure

Context sends are gated by a `response_pending` flag. While waiting for the Glass Agent to respond, additional silence events do not trigger new context sends.

---

## Files

| File | Purpose | Lifecycle |
|---|---|---|
| `PRD.md` | Project plan | User-created, read on agent spawn |
| `.glass/checkpoint.md` | Progress checkpoint | Written by Claude Code, read on agent spawn |
| `.glass/handoff.md` | Handoff instructions | User-created, read on enable, deleted after agent starts |
| `.glass/nudge.md` | Course correction | User-created, read on next silence, deleted after |
| `.glass/iterations.tsv` | Iteration log | Appended each iteration, included in system prompt (last 50) |

---

## Configuration

```toml
[agent.orchestrator]
enabled = false                # Enable orchestrator (toggled at runtime with Ctrl+Shift+O)
silence_timeout_secs = 30      # Seconds of PTY silence before sending context to agent
prd_path = "PRD.md"            # Path to project requirements document
checkpoint_path = ".glass/checkpoint.md"  # Path to checkpoint file
max_retries_before_stuck = 3   # Identical responses before stuck detection triggers
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

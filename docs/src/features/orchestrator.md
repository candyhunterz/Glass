# Orchestrator Mode

Orchestrator Mode drives autonomous project development by pairing two AI agents: Claude Code (the implementer, running in the PTY) and the Glass Agent (the reviewer/guide, running as a background subprocess). Glass manages the feedback loop between them, enabling overnight project builds or unattended completion of in-progress work.

---

## Overview

When you press **Ctrl+Shift+O**, Glass gathers project context from multiple sources, validates that context exists, then spawns the Glass Agent and begins the autonomous loop immediately. There is no interactive kickoff phase — the agent takes over as soon as context is validated.

The Glass Agent's system prompt includes core rules (iteration protocol, response format, critical rules). All project-specific context (files, terminal output, git status, user instructions) is delivered via the initial message.

---

## Activation Flow

```
Ctrl+Shift+O pressed
    |
    +-- Capture project root from terminal CWD
    +-- Capture terminal context (last 200 lines)
    +-- Read .glass/agent-instructions.md (if exists)
    |     +-- Parse optional YAML frontmatter for context_files list
    |     +-- Extract free-form body as agent instructions
    +-- Read explicitly listed context_files from frontmatter
    +-- Auto-scan: find *.md files in project root modified in last 30 minutes
    +-- Read configured prd_path (if set and file exists)
    +-- Deduplicate all discovered files
    |
    +-- If ZERO context files discovered:
    |     +-- Show centered toast: "No project context found -- create a plan first"
    |     +-- Don't activate (orchestrator stays off)
    |     +-- Return
    |
    +-- Auto-detect orchestrator mode and verify mode from project structure
    +-- Build agent system prompt (core rules only)
    +-- Build initial message with all gathered context
    +-- Spawn Glass Agent with initial message
    +-- Set orchestrator active
    +-- Agent begins reviewing terminal + directing Claude immediately
```

### Context Sources (Priority Order)

1. **`context_files` from frontmatter** — explicitly listed in `.glass/agent-instructions.md` (highest priority)
2. **`prd_path` from config** — e.g., `PRD.md` (only if the file actually exists)
3. **Auto-scanned `.md` files** — any markdown in project root modified in last 30 minutes (newest first)
4. **Terminal context** — last 200 lines of terminal output
5. **Git status** — `git log --oneline -10` and `git diff --stat` (omitted if not a git repo)

Combined file content is capped at ~8,000 words. Files beyond the budget are truncated with a note to read the full file.

---

## `.glass/agent-instructions.md`

A project-level file for steering the Glass Agent. Optional YAML frontmatter lists context files; the body contains free-form instructions.

```markdown
---
context_files:
  - PRD-trip-planner.md
  - docs/superpowers/plans/2026-03-19-multi-trip-design.md
---

Focus on the multi-trip homepage layout first.
Use vanilla HTML/CSS/JS -- no frameworks.
When Claude asks design questions, favor simplicity over features.
Keep each page under 500 lines.
Commit after each completed feature.
```

**Parsing rules:**
- Frontmatter is optional — if the file starts with `---`, content between the two `---` delimiters is parsed
- Only one recognized frontmatter field: `context_files` (list of relative paths from project root)
- Everything after frontmatter is the instruction body, appended to the agent's initial message
- If this file doesn't exist, the `agent_instructions` field in `[agent.orchestrator]` config is used as a fallback

---

## Workflows

### Brainstorm Then Orchestrate

The typical workflow: brainstorm with Claude, create a plan, then let the orchestrator execute it.

1. Open Glass, start Claude Code (`claude --dangerously-skip-permissions`)
2. Chat with Claude — describe your project, brainstorm, let it create a planning doc
3. Claude creates `PRD-my-project.md` (or any `.md` name)
4. Optionally create `.glass/agent-instructions.md` to steer the agent
5. Press **Ctrl+Shift+O** — orchestrator discovers the `.md` file, activates, agent takes over

### Fresh Project from PRD

1. Write `PRD.md` (or any name) in your project root describing what to build
2. Open Glass in the project directory
3. Start Claude Code
4. Press **Ctrl+Shift+O** — orchestrator reads the PRD and begins autonomous development

### Mid-Work Handoff

Already working on something and want to hand it off:

1. Create `.glass/agent-instructions.md` with context:
   ```markdown
   ---
   context_files:
     - src/auth.rs
     - CHANGELOG.md
   ---
   Finish the auth module, then add integration tests.
   The OAuth flow is half-done -- see src/auth.rs for current state.
   ```
2. Press **Ctrl+Shift+O** — agent reads your instructions and the listed files, picks up where you left off

### Course Correction

While the orchestrator is running, write `.glass/nudge.md` with new instructions. The orchestrator picks it up on the next silence cycle and injects it as a `[USER_NUDGE]` in the context sent to the agent. The file is deleted after reading.

---

## Autonomous Loop

Once activated, the autonomous feedback loop runs:

```
+-----------------------------------------------------------+
|                    Glass Orchestrator                       |
|                                                           |
|  1. PTY goes silent (30s default)                         |
|  2. Capture terminal context (20-80 lines based on SOI)   |
|  3. Send [TERMINAL_CONTEXT] to Glass Agent                |
|  4. Agent responds:                                       |
|     - Text -> type into PTY as instructions               |
|     - GLASS_WAIT -> check again after next silence        |
|     - GLASS_CHECKPOINT -> refresh context cycle           |
|     - GLASS_DONE -> stop orchestration                    |
|     - GLASS_VERIFY -> report verification commands        |
|  5. Repeat from step 1                                    |
+-----------------------------------------------------------+
```

**Key behaviors:**
- The agent acts AS the user — it answers Claude Code's questions decisively based on project context
- `GLASS_WAIT` is used when Claude Code is mid-turn (processing, using tools, streaming output)
- Instructions are kept short and actionable (1-3 sentences)
- The agent never echoes terminal content — responses are typed as-is into the PTY

---

## Checkpoint Cycle

Long-running sessions need periodic context refresh to prevent the Glass Agent from hitting its context limit.

**Automatic triggers:**
- The agent emits `GLASS_CHECKPOINT: {"completed": "...", "next": "..."}` after completing a feature
- Glass auto-triggers a checkpoint every 15 iterations if the agent hasn't checkpointed

**Refresh process:**
1. Checkpoint content is written to `.glass/checkpoint.md`
2. The Glass Agent subprocess is killed
3. A new Glass Agent is spawned with: `"Resume from checkpoint. Read .glass/checkpoint.md and continue."`
4. The new agent reads the checkpoint file and continues

Note: only the first spawn gets the full context bundle. Checkpoint respawns send a minimal resume message to save tokens.

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

### Response Pending Timeout

If the Glass Agent doesn't respond within 120 seconds, `response_pending` is auto-cleared with a warning log. This prevents permanent deadlock when the agent dies silently or hangs.

### Usage Tracking

Glass polls the Anthropic OAuth usage API every 60 seconds:

| Utilization | Action |
|---|---|
| >= 95% (5-hour window) | **Hard stop**: write emergency checkpoint, pause orchestrator |
| >= 80% (5-hour window) | **Pause**: disable orchestrator, user must re-enable manually |
| < 20% (5-hour window) | **Resume signal**: usage event sent (user still re-enables manually) |

### Backpressure

Context sends are gated by a `response_pending` flag. While waiting for the Glass Agent to respond, additional silence events do not trigger new context sends. This flag is also set during agent handoff (spawn + initial message) to prevent premature silence triggers before the agent has responded.

---

## Files

| File | Purpose | Lifecycle |
|---|---|---|
| `*.md` (project root) | Project context | Auto-discovered if modified recently, read on activation |
| `.glass/agent-instructions.md` | Agent steering + context file list | User-created, read on activation |
| `.glass/checkpoint.md` | Progress checkpoint | Written by Claude Code, read on agent respawn |
| `.glass/nudge.md` | Course correction | User-created, read on next silence, deleted after |
| `.glass/iterations.tsv` | Iteration log | Appended each iteration |
| `.glass/done` | Completion signal | Written by agent, triggers orchestrator, deleted after processing |

---

## Feedback Loop

The orchestrator learns from each run. After orchestration stops, a rule-based analyzer examines the run's metrics and produces findings across three tiers:

**Tier 1 — Config Tuning:** Findings that map directly to config values. If silence timeout was too short (agent got interrupted), Glass increases it. If stuck detection was too sensitive, Glass raises the threshold. Applied automatically, protected by a regression guard.

**Tier 2 — Rust-Level Enforcement:** Rules that the orchestrator executes directly in code — no LLM compliance needed. Glass runs git commands, splits agent responses, reverts out-of-scope files, and blocks forward progress autonomously. 8 of 9 actions are enforced in Rust:

| Action | Enforcement |
|---|---|
| Force commit | `git commit -am` when 5+ iterations without a commit |
| Isolate commit | `git add <file> && git commit` for hot files (3+ reverts) |
| Split instructions | Parse agent response, buffer numbered items, send one at a time |
| Scope guard | `git checkout --` for files outside PRD deliverables |
| Dependency block | Block context send, type resolution message into PTY |
| Extend silence | Dynamically increase silence threshold |
| Verify twice | Run verification twice before declaring regression |
| Early stuck | Lower stuck detection threshold |

Only `verify_progress` remains as a text suggestion — the orchestrator cannot determine what "progress" means for an arbitrary task.

**Tier 3 — Prompt Hints (opt-in):** When `feedback_llm = true`, an LLM analyzes the run qualitatively and produces hints like "This project's tests are flaky on first run." Capped at 10 per project. Requires an extra API call.

### Rule Lifecycle

Every rule goes through a guarded lifecycle:

```
proposed -> provisional -> confirmed -> stale -> archived
               |               |         |
            rejected      provisional  confirmed
                          (env drift)  (re-triggered)
```

- **Provisional -> Confirmed:** Next run's metrics didn't regress
- **Provisional -> Rejected:** Next run's metrics regressed — rule and config rolled back
- **Confirmed -> Stale:** Rule hasn't triggered in 10 runs
- **Stale -> Archived:** Stale for 5 more runs, moved to archived file

### Regression Guard

Before each run, Glass snapshots the current config and provisional rules. After the run, it compares metrics (revert rate, stuck rate, waste rate). If any metric regressed, all provisional changes are rolled back and marked rejected.

Safety constraints:
- Max 3 provisional rules per run
- Max 1 config value change per run
- Rejected changes get a 5-run cooldown before re-proposal
- User can pin rules with `status = "pinned"` to prevent auto-revert

### Default Rules

Glass ships with 6 default rules that enter each project as provisional:

| Rule | Enforcement | LLM needed? |
|---|---|---|
| Uncommitted drift (5+ iterations) | Auto `git commit -am` | No |
| Hot file (3+ reverts) | Auto `git add <file> && git commit` | No |
| Instruction overload (4+ per response) | Split response, buffer, send one at a time | No |
| Flaky verification | Run tests twice before reverting | No |
| High revert rate (>30%) | Split instructions (same as overload) | No |
| High waste rate (>15%) | Text suggestion to verify progress | Yes |

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
prd_path = "PRD.md"            # Optional hint for context discovery (auto-detected if missing)
checkpoint_path = ".glass/checkpoint.md"  # Path to checkpoint file
max_retries_before_stuck = 3   # Identical responses before stuck detection triggers
verify_mode = "floor"          # "floor" (auto-detect + guard) or "disabled"
# verify_command = "cargo test" # Optional override (skips auto-detect)
completion_artifact = ".glass/done"  # File path that triggers orchestrator when created
# max_iterations = 25          # Optional iteration limit (omit or 0 for unlimited)
# agent_instructions = "..."   # Fallback instructions when .glass/agent-instructions.md missing
# Feedback loop
feedback_llm = false           # Enable LLM qualitative analysis after each run (opt-in)
# max_prompt_hints = 10        # Max Tier 3 prompt hints per project

# Multi-provider backend
# provider = "claude-code"         # "claude-code", "anthropic-api", "openai-api", "codex-cli", "ollama", "custom"
# model = ""                       # Provider default. Examples: "gpt-4o", "claude-opus-4-6"
# implementer = "claude-code"      # "claude-code", "codex", "aider", "gemini", "custom"
# implementer_name = "Claude Code" # Display name in system prompt
# persona = ""                     # Inline persona or path to .md file
```

The orchestrator requires Agent Mode to be configured (the `[agent]` section). By default, the Glass Agent uses the Claude CLI. Set `provider` in `[agent]` to use other models (OpenAI, Anthropic API, Ollama, or any OpenAI-compatible endpoint). The `implementer` field controls which CLI runs in the terminal.

---

## Using ChatGPT OAuth (Codex)

Glass can route both the implementer and the reviewer through the local
[Codex CLI](https://github.com/openai/codex), reusing your ChatGPT Plus / Pro / Team
plan with no API key required.

1. Install Codex (`npm i -g @openai/codex` or your platform's equivalent).
2. Run `codex login` once — this opens a browser and stores tokens in `~/.codex/auth.json` (on Windows: `%USERPROFILE%\.codex\auth.json`).
3. In `~/.glass/config.toml`:

   ```toml
   [agent]
   provider = "codex-cli"     # reviewer uses Codex via OAuth
   model = "gpt-5-codex"

   [agent.orchestrator]
   enabled = true
   implementer = "codex"      # implementer also uses Codex
   ```

Glass does not perform OAuth itself — Codex owns login, token refresh, and endpoint
selection. If the Codex token file is missing, Glass surfaces a clear error in the
config error banner pointing you at `codex login`, and a one-time onboarding toast
on first hit, then refuses to start the orchestrator until you log in.

Note: ChatGPT plans don't expose a public usage API, so the 5-hour / 7-day usage
indicator and auto-pause thresholds in the status bar do **not** apply to
`codex-cli` or `openai-api` providers — they only work for Claude OAuth.

**Caveat**: When the implementer is Codex, Glass cannot send `/clear` between
checkpoints (Codex has no equivalent command), so context-clear is a no-op for
that implementer. Checkpoint cycles still work; only the freshness reset is
skipped.

---

## Status Bar

When orchestrating, the status bar shows:
- `[orchestrating | iter #N]` — current iteration number
- `[orchestrating | iter #N | waiting for agent]` — waiting for Glass Agent response
- Usage display: `5h: 42% | 7d: 15%` — OAuth API utilization (color-coded: green < 70%, yellow 70-85%, red 85%+)
- `PAUSED` — shown when usage limits triggered a pause

---

## Requirements

- Claude CLI must be installed and available on `PATH`
- Agent Mode must be configured (`[agent]` section in `~/.glass/config.toml`)
- At least one `.md` file in the project (PRD, plan doc, agent-instructions.md, or any recently modified markdown)

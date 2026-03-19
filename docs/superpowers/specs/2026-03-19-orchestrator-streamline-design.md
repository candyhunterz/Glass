# Orchestrator Streamline Design

**Date:** 2026-03-19
**Status:** Approved

## Problem

The orchestrator's kickoff phase causes timing bugs, garbage agent responses, PTY injection issues, and a complex state machine. The rigid PRD filename requirement (`prd_path`) breaks when Claude names planning docs differently. The agent lacks a user-controlled customization hook.

## Solution

Remove the kickoff phase entirely. Require project context before activation. Auto-discover context files from multiple sources. Let users steer the agent via `.glass/agent-instructions.md`.

## Activation Flow

```
Ctrl+Shift+O pressed
    │
    ├─ Capture project_root from terminal CWD
    ├─ Capture terminal context (last 200 lines)
    ├─ Read .glass/agent-instructions.md (if exists)
    │   ├─ Parse optional YAML frontmatter for context_files list
    │   └─ Extract free-form body as agent instructions
    ├─ Read explicitly listed context_files from frontmatter
    ├─ Auto-scan: find *.md files in project root modified in last 30 minutes
    ├─ Read configured prd_path (if set and exists)
    ├─ Deduplicate all discovered files
    │
    ├─ If ZERO context files discovered (all sources returned no files):
    │   ├─ Show centered toast: "No project context found — create a plan first"
    │   ├─ Don't activate orchestrator (active stays false)
    │   └─ Return
    │
    ├─ Auto-detect orchestrator_mode and verify_mode from project structure
    │   (keep existing auto_detect_orchestrator_config logic, but don't write to config.toml)
    ├─ Build agent system prompt with core rules (hardcoded, written to agent-system-prompt.txt)
    ├─ Build initial message with all gathered context (dynamic)
    ├─ Spawn agent — initial message sent as first stdin JSON message after spawn
    ├─ Set orchestrator.active = true
    └─ Agent immediately starts reviewing terminal + directing Claude
```

### ZERO Context Gate

Activation is blocked when **no context files** are discovered from any of the four sources (agent-instructions.md context_files, prd_path, auto-scanned recent .md files). Terminal context alone is not sufficient — the agent needs at least one file to provide project direction. This prevents the "agent types garbage" scenario.

### Settings Overlay Toggle

The settings overlay toggle path (Ctrl+Shift+, → orchestrator toggle) follows the same activation flow as Ctrl+Shift+O. Both paths share the same context assembly and validation logic.

## Context Assembly

### Initial Message Delivery

After spawning the agent via `try_spawn_agent`, the initial message is sent as the first stdin JSON message (same mechanism as the current `initial_message` parameter). The `try_spawn_agent` function already supports this — `initial_message: Option<String>` is written to stdin before the activity writer thread starts.

The agent's initial message on first spawn is a structured context bundle:

```
[ORCHESTRATOR_START]

## Agent Instructions
<contents of .glass/agent-instructions.md body, if exists>

## Project Context Files
### path/to/PRD-trip-planner.md
<full file contents>
### docs/superpowers/plans/2026-03-19-...md
<full file contents>
...

## Terminal Context (last 200 lines)
<raw terminal text>

## Git Status
Recent commits: <git log --oneline -10>
Uncommitted: <git diff --stat>
```

Git status section is omitted entirely if the project root does not contain a `.git` directory.

### Context Size Budget

Combined context files capped at ~8,000 words (~10K tokens). Files are included in priority order until the aggregate word count reaches 8,000. The last file to push over the limit is truncated. Files below the cutoff are omitted with `[TRUNCATED — read full file at <path>]`.

Priority order:
1. Explicitly listed `context_files` from frontmatter (highest)
2. Configured `prd_path` file
3. Auto-discovered recent `.md` files (sorted by modification time, newest first)

Word counting uses the same `split_whitespace().count()` approach as the current PRD truncation.

### Checkpoint / Crash Respawn

- **First spawn:** Full context assembly as above
- **Checkpoint respawn:** `"Resume from checkpoint. Read .glass/checkpoint.md and continue."`
- **Crash restart:** Same as checkpoint respawn

On checkpoint/crash respawn, the system prompt file is rewritten with the same core rules (hardcoded section unchanged). Only the initial message changes (minimal checkpoint resume vs. full context bundle).

The agent can read any files it needs via Claude's built-in Read tool. No need to re-send full file contents on respawn.

## System Prompt Structure (Hybrid)

Core prompt stays hardcoded in Rust. Context is assembled dynamically into the initial message.

### Hardcoded in Rust (system prompt file):
- **Core identity:** "You are the Glass Agent... You ARE the user — answer Claude's questions decisively."
- **Mode instructions:** BUILD / GENERAL / AUDIT protocols (unchanged)
- **Critical rules:** GLASS_WAIT conditions, never echo terminal text, keep instructions short
- **Response format:** TypeText / GLASS_WAIT / GLASS_CHECKPOINT / GLASS_DONE / GLASS_VERIFY
- **Verification / metric guard / context refresh:** unchanged

### Dynamic in initial message:
- Agent instructions (from `.glass/agent-instructions.md` body)
- Project context files (auto-discovered + explicit)
- Terminal context
- Git status

## `.glass/agent-instructions.md` Format

```markdown
---
context_files:
  - PRD-trip-planner.md
  - docs/superpowers/plans/2026-03-19-multi-trip-design.md
---

Focus on the multi-trip homepage layout first.
Use vanilla HTML/CSS/JS — no frameworks.
When Claude asks design questions, favor simplicity over features.
Keep each page under 500 lines.
Commit after each completed feature.
```

### Parsing Rules
- Frontmatter is optional — if the file starts with `---`, parse YAML between the two `---` delimiters
- Only one recognized frontmatter field: `context_files` (list of relative paths from project root)
- Everything after frontmatter (or entire file if no frontmatter) is the instruction body
- Instruction body appended verbatim to agent's initial message under `## Agent Instructions`
- Paths in `context_files` resolved relative to project root, not `.glass/` directory
- Hand-parse the simple `context_files` list (line-by-line `- ` prefix stripping) to avoid adding a YAML crate dependency

### Config Fallback
New optional field `agent_instructions` in `[agent.orchestrator]` config section. If `.glass/agent-instructions.md` doesn't exist, this string is used as the instruction body (no frontmatter support in the TOML field). Project-level file always takes precedence.

### Relationship to `handoff.md`
The existing `.glass/handoff.md` (one-shot user notes, read and deleted on activation) is superseded by `agent-instructions.md`. The `handoff.md` code path is removed. Users who want one-shot notes can edit `agent-instructions.md` and remove lines after the run.

## Centered Toast Notification

New rendering element for important messages.

```
┌─────────────────────────────────────────────────┐
│                                                 │
│            Normal terminal content               │
│                                                 │
│     ┌───────────────────────────────────┐       │
│     │ ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│       │
│     │ ░ No project context found —     ░│       │
│     │ ░ create a plan first            ░│       │
│     │ ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│       │
│     └───────────────────────────────────┘       │
│                                                 │
│            Normal terminal content               │
│                                                 │
└─────────────────────────────────────────────────┘
```

- Semi-transparent dark backdrop (`[0.1, 0.1, 0.1, 0.85]`) sized to text + padding
- White text centered on the backdrop
- Passed as `Option<&str>` parameter to `draw_frame` (consistent with existing toast/overlay data flow)
- Stored as `Option<(String, Instant)>` on `Processor` — `centered_toast`
- Auto-dismisses after 5 seconds (checked before render)
- Reusable for future cases (e.g., "Orchestrator stopped", "Budget exceeded")

## Code Changes

### Removed
- `OrchestratorState` fields: `kickoff_complete`, `last_user_keypress`
- `OrchestratorState` methods: `mark_user_keypress()`, `user_recently_active()`
- Kickoff guard block in silence handler (~20 lines)
- Keypress tracking in keyboard handler (`if !self.orchestrator.kickoff_complete { ... }`)
- PRD-exists check and both status message branches in Ctrl+Shift+O handler
- Settings overlay toggle kickoff reset (`kickoff_complete = false`, `last_user_keypress = None`)
- `KICKOFF MODE` text blocks from system prompt builder (both `prd_missing` and `!prd_missing` variants)
- `kickoff_instructions` variable in system prompt builder
- Config writes at activation (`update_config_field` calls for orchestrator_mode, verify_mode, verify_files)
- Deferred TypeText during kickoff check (`if !self.orchestrator.kickoff_complete { defer }`)
- `.glass/handoff.md` read-and-delete code path
- `RunData.kickoff_duration_secs` and `RunMetrics.kickoff_duration_secs` (set to 0, keep fields for backward compat)

### Added
- Context assembly function: `gather_orchestrator_context(project_root) -> OrchestratorContext` (~60 lines)
- `agent-instructions.md` parser: `parse_agent_instructions(path) -> (Vec<String>, String)` (~30 lines)
- Recent `.md` file scanner (~15 lines)
- Centered toast renderer (~40 lines in frame.rs)
- `centered_toast: Option<(String, Instant)>` field on Processor
- `agent_instructions: Option<String>` field in OrchestratorConfig

### Modified
- `auto_detect_orchestrator_config`: keep the detection logic, remove the config.toml write-back. Detected values used in-memory only.
- `generate_postmortem`: accept list of context file paths instead of single `prd_path`
- `deferred_type_text`: remains for the block-executing case (only the kickoff deferral path is removed)
- `try_spawn_agent`: system prompt no longer includes inline PRD/checkpoint/iteration content (moved to initial message)

### Unchanged
- `prd_path` config field (optional hint for context discovery)
- `orchestrator_mode`, `verify_mode` config fields
- Silence detection, response handling, checkpoint cycle, stuck detection
- Metric guard, verification commands
- Agent crash restart logic (uses checkpoint respawn path)
- `response_pending` timeout (120s, added earlier this session)

### ORCHESTRATOR.md
Updated with the new activation flow diagram.

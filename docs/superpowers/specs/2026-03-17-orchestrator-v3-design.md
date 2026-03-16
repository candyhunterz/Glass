# Orchestrator V3: General Mode, Kickoff Flow, and Settings UX

**Date:** 2026-03-17
**Status:** Approved
**Context:** The orchestrator works well for code audit/fix loops with test suites, but is unusable for non-code tasks (research, planning, web development without tests). The PRD must be manually written, settings are confusing and partially broken, and the auto-pause on any keystroke is too aggressive.

## Problem

Three gaps preventing the orchestrator from being a general-purpose autonomous agent:

1. **Cold start** — Users must manually write a PRD file before starting orchestration. Most users use Claude Code to generate PRDs anyway. There's no guided kickoff.
2. **Code-only verification** — The metric guard only understands test counts (`cargo test`, `npm test`). For research, planning, or design tasks, there's no quality signal. The orchestrator runs blind.
3. **Settings friction** — Too many config knobs, most settings aren't editable in the overlay, changing verify mode hangs Glass, and auto-pause on any keystroke kills long-running sessions accidentally.

## Design

### 1. Kickoff Flow

When the user presses Ctrl+Shift+O, the orchestrator checks whether the configured `prd_path` file exists.

**PRD does not exist → Kickoff mode:**

1. Glass captures the current terminal content (the user's intent is usually visible — they've been describing what they want to Claude Code).
2. The Glass Agent's first instruction to Claude Code: "The user wants to start a new project. Based on the terminal context, generate a detailed PRD. Name the file descriptively (e.g., PRD-japan-vacation.md). Include a ## Deliverables section listing output files, ## Requirements, and ## Research Areas if applicable. Write it to disk, then begin executing it."
3. Claude Code generates the PRD and writes it.
4. The Glass Agent detects the new PRD file (via terminal output showing the write, or via git status), and sends a config update instruction. Claude Code runs Glass's config update to set `prd_path` to the new filename.
5. Auto-detection runs (see Section 2) to set mode and verification strategy.
6. Normal orchestration loop begins.

**PRD exists → Prompt user:**

1. Glass types into the terminal: `[GLASS] Found existing PRD at {prd_path}. Continue with it? (y=continue, n=start fresh)`
2. User types `y` or presses Enter → orchestrator starts with existing PRD (current behavior).
3. User types `n` → orchestrator enters kickoff mode. The old PRD is preserved. A new PRD with a descriptive name is generated and `config.toml` `prd_path` is updated to point to it.

**Auto-pause removed:**

The current behavior where any PTY input auto-pauses orchestration is removed. Only Ctrl+Shift+O toggles orchestration on/off. This means:
- Users can type nudges into the terminal while the orchestrator runs.
- Accidental keystrokes don't kill overnight runs.
- The kickoff flow works naturally (user input is expected during kickoff).

### 2. General Mode

A new `orchestrator_mode = "general"` for non-code tasks.

**System prompt:**

The current build/audit prompts are code-specific ("look at terminal output, run tests, find bugs"). General mode uses a task-agnostic prompt:

```
You are the Glass Agent orchestrating a task. Your role is to guide Claude Code
through the PRD by observing terminal output and providing the next instruction.

Rules:
- Read the PRD and work through deliverables in order.
- Use whatever tools are needed: web search, file creation, shell commands, code.
- Track progress by deliverable completion.
- If Claude Code stalls or goes off-track, redirect it to the next PRD item.
- When all deliverables in the PRD are complete, respond with GLASS_DONE.
```

Build and audit modes keep their existing prompts unchanged.

**File-based verification (`verify_mode = "files"`):**

A new verification strategy that checks deliverable files instead of test counts.

Config:
```toml
verify_mode = "files"
verify_files = ["vacation-plan.md", "site/index.html", "research/flights.md"]
```

The verify step (runs on each iteration, same as test verification):
1. For each file in `verify_files`: check existence and byte size.
2. Compare to the last check:
   - Any file that previously existed is now missing → regression, revert.
   - Any file shrank by more than 50% → regression, revert.
   - All files exist and none regressed → keep.
3. First run establishes the baseline (same as test count baseline).
4. Files growing or new files appearing is always a keep.

When `verify_files` is empty or `verify_mode = "off"`, no verification runs.

**Auto-detection:**

Runs after PRD generation (kickoff) or on orchestrator activation (existing PRD). Sets `orchestrator_mode` and `verify_mode` automatically.

Detection logic:
```
1. Check for code project markers in CWD:
   - Cargo.toml → mode=build, verify=floor, cmd="cargo test --workspace"
   - package.json with "test" script → mode=build, verify=floor, cmd="npm test"
   - pyproject.toml or setup.py → mode=build, verify=floor, cmd="pytest"
   - go.mod → mode=build, verify=floor, cmd="go test ./..."
   - Makefile with "test" target → mode=build, verify=floor, cmd="make test"

2. If no code markers, parse PRD for deliverables:
   - Look for "## Deliverables" section
   - Extract file paths from list items (e.g., "- vacation-plan.md (itinerary)" → "vacation-plan.md")
   - If deliverables found → mode=general, verify=files, verify_files=[extracted paths]

3. If neither → mode=general, verify=off
```

The auto-detected values are written to `config.toml` so they persist across restarts and are visible in the settings overlay.

### 3. Settings UX

**Editable fields (3):**

| Field | Control | Range | Default |
|-------|---------|-------|---------|
| Enabled | Toggle (Enter) | on/off | off |
| Max iterations | Increment (arrows) | 0=unlimited, step 10 | 0 |
| Silence timeout | Increment (arrows) | 10-300s, step 10 | 60 |

**Display-only fields (4):**

| Field | Source | Example |
|-------|--------|---------|
| PRD path | config | `PRD-japan-vacation.md` |
| Mode | auto-detected | `general` / `build` / `audit` |
| Verify mode | auto-detected | `floor` / `files` / `off` |
| Status | runtime | `active (iter #12)` / `idle` |

**Removed from settings:**
- Fast trigger secs (technical, keep in config.toml for power users)
- Prompt pattern (technical, keep in config.toml for power users)
- Completion artifact (internal implementation detail)
- Orchestrator mode toggle (auto-detected)
- Verify mode toggle (auto-detected, and was causing hangs)

**Bug fix — config reload hang:**

Root cause: toggling verify mode triggers a config reload which restarts the agent runtime. If a verify thread is in-flight with `response_pending = true`, the restart races with the pending verify. The `response_pending` flag is never cleared, so the orchestrator blocks forever.

Fix: In the `ConfigReloaded` handler, when restarting the agent runtime, clear `response_pending = false` before dropping the old runtime.

## Files Changed

| File | Change |
|------|--------|
| `crates/glass_core/src/config.rs` | Add `verify_files: Vec<String>` to OrchestratorConfig. Add "general" to orchestrator_mode validation. |
| `crates/glass_core/src/agent_runtime.rs` | Add general mode system prompt template. Include kickoff instructions when PRD is missing. |
| `src/orchestrator.rs` | Add auto-detection function (project markers + PRD parsing). Add file-based verification logic. Add kickoff state tracking. |
| `src/main.rs` | Rewrite Ctrl+Shift+O handler for kickoff flow. Remove auto-pause on user input. Add file verification to verify thread. Clear `response_pending` on config reload. Wire auto-detection into activation path. |
| `crates/glass_renderer/src/settings_overlay.rs` | Update `fields_for_section` for orchestrator: 3 editable + 4 display-only. Remove verify_mode and orchestrator_mode toggles. |

## Not In Scope

- **Visual PRD editor overlay** — Users edit PRDs in their editor or via Claude Code, not in a Glass overlay.
- **Multi-agent orchestration** — One Glass Agent driving one Claude Code instance. Multi-agent is a separate feature.
- **Web browsing in the Glass Agent** — The Glass Agent observes the terminal. Claude Code does the web searching.
- **PRD templates per task type** — The Glass Agent generates PRDs from scratch. Templates could be added later.
- **Undo for config changes** — Config updates from auto-detection are persistent. The user can manually revert in config.toml.

# Documentation Implementation Plan (Branch 7 of 8)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fill all documentation gaps identified by the prelaunch audit — CHANGELOG, CONTRIBUTING, config examples, Rhai script examples, CLAUDE.md accuracy, mdBook scripting page, badge/screenshot placeholders, and reconcile stale doc-to-code mismatches.

**Architecture:** Work leaf-first: create standalone files with no code deps (CHANGELOG, CONTRIBUTING, templates, config example), then update existing docs (CLAUDE.md, lib.rs comments, mdBook), then visual polish (badges, screenshot placeholder). This order avoids merge conflicts if earlier branches change code.

**Tech Stack:** Markdown, TOML, Rhai, mdBook (existing)

**Branch:** `audit/documentation` off `master`

**Spec:** `docs/superpowers/specs/2026-03-18-prelaunch-audit-fixes-design.md` Branch 7

---

### Task 1: Branch setup + CHANGELOG.md (DOC-2)

**Files:**
- Create: `CHANGELOG.md`

- [ ] **Step 1: Create branch**

```bash
git checkout -b audit/documentation master
```

- [ ] **Step 2: Create CHANGELOG.md**

Create `CHANGELOG.md` at repo root following [Keep a Changelog](https://keepachangelog.com/) format. Backfill from git history across milestones. Structure:

```markdown
# Changelog

All notable changes to Glass are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [3.0] - 2026-03-18
### Added
- Ablation testing for confirmed feedback rules
- Attribution tracking for orchestrator feedback rules
- Ablation sweep interval configuration field

## [2.5] - 2026-03-17
### Added
- LLM-based qualitative feedback analysis after orchestrator runs
- Feedback loop data model with Tier 1/2/3 rule storage
- Prompt hint injection into orchestrator checkpoint synthesis

## [2.0] - 2026-03-16
### Added
- Orchestrator Mode: silence-triggered autonomous build/audit loop
- Metric guard with floor verification (test counts, clippy, build)
- Checkpoint synthesis and stuck detection
- Settings overlay with live config editing
- Activity stream with severity-colored event feed
- Orchestrator overlay showing iteration log, PRD progress, metric history

## [1.5] - 2026-03-14
### Added
- Self-improvement scripting layer (Rhai engine, hook system, sandbox, profiles)
- Script lifecycle management (provisional → confirmed → stale → archived)
- MCP tool exposure from Rhai scripts
- Script generation from feedback loop

## [1.0] - 2026-03-08
### Added
- GPU-accelerated terminal emulator with wgpu rendering
- Command blocks with exit codes, durations, CWD badges
- Command-level undo via pre-exec filesystem snapshots
- Visual pipeline debugging with per-stage capture
- Full-text history search (SQLite FTS5)
- Tabs and split panes (binary split tree)
- 33 MCP tools for AI agent integration
- Structured Output Intelligence (SOI) with 19 format-specific parsers
- Multi-agent coordination (advisory locks, messaging, registry)
- Shell integration for bash, zsh, fish, PowerShell (OSC 133)
- Cross-platform: Windows (ConPTY), macOS (forkpty), Linux (forkpty)
```

Flesh out each section by running `git log --oneline` between milestone tags/dates and grouping changes into Added/Changed/Fixed/Removed subsections. The above is a skeleton — expand with specific features from commit messages.

- [ ] **Step 3: Build and test (docs only — no cargo build needed)**

Verify the markdown renders correctly by reading it back.

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs(DOC-2): add CHANGELOG.md with backfilled milestone history

Keep a Changelog format. Covers v1.0 through v3.0 milestones."
```

---

### Task 2: CONTRIBUTING.md + GitHub templates (DOC-3)

**Files:**
- Create: `CONTRIBUTING.md`
- Create: `.github/ISSUE_TEMPLATE/bug_report.md`
- Create: `.github/ISSUE_TEMPLATE/feature_request.md`
- Create: `.github/PULL_REQUEST_TEMPLATE.md`

- [ ] **Step 1: Create CONTRIBUTING.md**

Create `CONTRIBUTING.md` at repo root covering:

1. **Prerequisites** — Rust toolchain (stable), platform deps (list Linux `apt install` packages: `libxkbcommon-dev libxtst-dev libfontconfig-dev`, Fedora `dnf` equivalents, Arch `pacman` equivalents)
2. **Building** — `cargo build`, `cargo build --release`, `cargo build --features perf`
3. **Testing** — `cargo test --workspace` (~420 tests), note ConPTY tests are `#[cfg(target_os = "windows")]` only
4. **Linting** — `cargo fmt --all -- --check`, `cargo clippy --workspace -- -D warnings` (all warnings are errors)
5. **Code style** — tests in same file (`#[cfg(test)] mod tests`), `alacritty_terminal` pinned to exact `=0.25.1`
6. **PR process** — branch off `master`, CI must pass (fmt, clippy, build+test on all 3 platforms), PR target is `main`
7. **Commit messages** — conventional commits style (`feat:`, `fix:`, `docs:`, `chore:`)
8. **Architecture overview** — point to `CLAUDE.md` for crate map, `ORCHESTRATOR.md` for orchestrator internals

- [ ] **Step 2: Create bug report template**

Create `.github/ISSUE_TEMPLATE/bug_report.md`:

```markdown
---
name: Bug Report
about: Report a bug in Glass
title: "[BUG] "
labels: bug
assignees: ''
---

## Description
<!-- A clear description of the bug -->

## Steps to Reproduce
1.
2.
3.

## Expected Behavior
<!-- What should have happened -->

## Actual Behavior
<!-- What actually happened -->

## Environment
- OS: [e.g., Windows 11, macOS 15, Ubuntu 24.04]
- Glass version: [e.g., v1.0]
- Shell: [e.g., bash, zsh, fish, PowerShell]
- GPU: [e.g., NVIDIA RTX 4090, Apple M3, Intel UHD]

## Logs
<!-- Paste relevant output from Glass's stderr or `glass check` -->

## Screenshots
<!-- If applicable -->
```

- [ ] **Step 3: Create feature request template**

Create `.github/ISSUE_TEMPLATE/feature_request.md`:

```markdown
---
name: Feature Request
about: Suggest an idea for Glass
title: "[FEATURE] "
labels: enhancement
assignees: ''
---

## Problem
<!-- What problem does this solve? -->

## Proposed Solution
<!-- How should it work? -->

## Alternatives Considered
<!-- Any alternatives you've thought about? -->

## Additional Context
<!-- Screenshots, mockups, or references -->
```

- [ ] **Step 4: Create PR template**

Create `.github/PULL_REQUEST_TEMPLATE.md`:

```markdown
## Summary
<!-- What does this PR do? -->

## Changes
-

## Test Plan
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] `cargo fmt --all -- --check` clean

## Related Issues
<!-- Closes #... -->
```

- [ ] **Step 5: Commit**

```bash
git add CONTRIBUTING.md .github/ISSUE_TEMPLATE/bug_report.md .github/ISSUE_TEMPLATE/feature_request.md .github/PULL_REQUEST_TEMPLATE.md
git commit -m "docs(DOC-3): add CONTRIBUTING.md, issue templates, and PR template

Build/test/lint instructions, code style guide, PR process.
GitHub issue templates for bugs and feature requests."
```

---

### Task 3: Update CLAUDE.md (DOC-4, DOC-14 partial)

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Fix crate count and add missing crates**

In `CLAUDE.md`, update the Architecture section. Change "9 crates" to "14 crates". Add the 5 missing crate entries to the crate list:

```
crates/glass_errors/     - Centralized error types and structured error handling
crates/glass_soi/        - Structured Output Intelligence: 19 format-specific parsers for command output
crates/glass_agent/      - Agent runtime: event-driven AI agent with MCP tool access
crates/glass_feedback/   - Feedback loop: LLM-based qualitative analysis, rule extraction, prompt hints
crates/glass_scripting/  - Self-improvement scripting: Rhai engine, hook system, sandbox, profiles
```

Place them in logical order within the existing list (e.g., `glass_errors` near the top since it's foundational, `glass_soi` after `glass_pipes`, `glass_agent` after `glass_mcp`, `glass_feedback` after `glass_agent`, `glass_scripting` last).

- [ ] **Step 2: Update Configuration section**

The current config section says: `Sections: font, shell, history, snapshot, pipes.`

Update to reflect the actual `GlassConfig` struct which has these sections: `font_family`, `font_size`, `shell`, `history`, `snapshot`, `pipes`, `soi`, `agent` (with `agent.orchestrator` sub-section, `agent.permissions`, `agent.quiet_rules`), `scripting`.

Replace with:
```
`~/.glass/config.toml` — hot-reloaded via notify watcher. Sections: font, shell, history, snapshot, pipes, soi, agent (with orchestrator, permissions, quiet_rules sub-sections), scripting. See `config.example.toml` for all fields with defaults.
```

- [ ] **Step 3: Update Key Files section**

Add entries for key files in the new crates:
```
- `crates/glass_soi/src/lib.rs` - SOI parser registry and format detection
- `crates/glass_agent/src/lib.rs` - Agent runtime event loop and MCP tool dispatch
- `crates/glass_feedback/src/lib.rs` - Feedback loop lifecycle, rule extraction, prompt hints
- `crates/glass_scripting/src/engine.rs` - Rhai script execution engine with sandbox
- `crates/glass_scripting/src/hooks.rs` - Hook registry mapping scripts to lifecycle events
```

- [ ] **Step 4: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

(CLAUDE.md is not compiled, but ensure no build regressions from any accidental changes.)

- [ ] **Step 5: Commit**

```bash
git add CLAUDE.md
git commit -m "docs(DOC-4): update CLAUDE.md — 9→14 crates, add missing entries

Add glass_errors, glass_soi, glass_agent, glass_feedback, glass_scripting.
Update config section to reflect all actual TOML sections.
Add key files for new crates."
```

---

### Task 4: config.example.toml (DOC-5)

**Files:**
- Create: `config.example.toml`

- [ ] **Step 1: Create config.example.toml at repo root**

Generate from the actual `GlassConfig` struct in `crates/glass_core/src/config.rs`. Every field should be present, commented out, with its default value and a brief description. Structure:

```toml
# Glass Configuration
# Copy to ~/.glass/config.toml and uncomment fields to customize.
# All values shown are defaults. Hot-reloaded on save.

# Font family. Platform defaults: "Consolas" (Windows), "Menlo" (macOS), "Monospace" (Linux)
# font_family = "Consolas"

# Font size in points.
# font_size = 14.0

# Override default shell. Uses $SHELL / system default when unset.
# shell = "/bin/zsh"

# --- History -----------------------------------------------------------
# [history]
# max_output_capture_kb = 50

# --- Snapshots ---------------------------------------------------------
# [snapshot]
# enabled = true
# max_count = 1000
# max_size_mb = 500
# retention_days = 30

# --- Pipes -------------------------------------------------------------
# [pipes]
# enabled = true
# max_capture_mb = 10
# auto_expand = true

# --- Structured Output Intelligence -----------------------------------
# [soi]
# enabled = true
# shell_summary = false
# format = "oneline"
# min_lines = 0

# --- Agent -------------------------------------------------------------
# [agent]
# mode = "off"              # "off", "watch", "act"
# max_budget_usd = 1.0
# cooldown_secs = 30
# allowed_tools = "glass_query,glass_query_trend,glass_query_drill,glass_context,Bash,Read"

# [agent.permissions]
# edit_files = "approve"    # "approve", "auto", "never"
# run_commands = "approve"
# git_operations = "approve"

# [agent.quiet_rules]
# ignore_exit_zero = false
# ignore_patterns = []

# [agent.orchestrator]
# enabled = false
# silence_timeout_secs = 60
# prd_path = "PRD.md"
# checkpoint_path = ".glass/checkpoint.md"
# max_retries_before_stuck = 3
# fast_trigger_secs = 5
# agent_prompt_pattern = ""
# verify_mode = "floor"     # "floor" or "disabled"
# verify_command = ""
# completion_artifact = ".glass/done"
# max_iterations = 0        # 0 = unlimited
# orchestrator_mode = "build"  # "build" or "audit"
# verify_files = []
# feedback_llm = false
# max_prompt_hints = 10
# ablation_enabled = true
# ablation_sweep_interval = 20

# --- Scripting ---------------------------------------------------------
# [scripting]
# enabled = true
# max_operations = 10000
# max_timeout_ms = 5000
# max_scripts_per_hook = 10
# max_total_scripts = 100
# max_mcp_tools = 50
# script_generation = true
```

Cross-reference every field against the actual struct definitions in `crates/glass_core/src/config.rs` (lines 88-407) to ensure nothing is missed.

- [ ] **Step 2: Commit**

```bash
git add config.example.toml
git commit -m "docs(DOC-5): add config.example.toml with all sections and defaults

Every field from GlassConfig commented out with default values.
Covers font, shell, history, snapshot, pipes, soi, agent
(permissions, quiet_rules, orchestrator), and scripting."
```

---

### Task 5: Example Rhai scripts (DOC-6)

**Files:**
- Create: `examples/scripts/auto_git_status.rhai`
- Create: `examples/scripts/auto_git_status.toml`
- Create: `examples/scripts/notify_long_command.rhai`
- Create: `examples/scripts/notify_long_command.toml`
- Create: `examples/scripts/block_rm_rf.rhai`
- Create: `examples/scripts/block_rm_rf.toml`
- Create: `examples/scripts/README.md`

- [ ] **Step 1: Create examples/scripts/ directory**

```bash
mkdir -p examples/scripts
```

- [ ] **Step 2: Create auto_git_status example**

A script that runs `git status` after every command that modifies tracked files. This demonstrates the `command_complete` hook and the `run_command` action.

`examples/scripts/auto_git_status.toml`:
```toml
name = "auto_git_status"
hooks = ["command_complete"]
status = "confirmed"
origin = "user"
version = 1
api_version = "1"
```

`examples/scripts/auto_git_status.rhai`:
```rhai
// Auto Git Status — run `git status --short` after commands that touch tracked files.
// Hook: command_complete
//
// Install: copy both files to ~/.glass/scripts/ or <project>/.glass/scripts/

let cmd = ctx.command;

// Only trigger for commands likely to modify files
let triggers = ["git", "rm", "mv", "cp", "touch", "sed", "mkdir", "cargo"];
let dominated = false;
for t in triggers {
    if cmd.contains(t) {
        dominated = true;
    }
}

if dominated {
    actions.run_command("git status --short 2>/dev/null");
}
```

- [ ] **Step 3: Create notify_long_command example**

A script that logs a notification when a command takes longer than 10 seconds. Demonstrates `command_complete` hook with duration inspection.

`examples/scripts/notify_long_command.toml`:
```toml
name = "notify_long_command"
hooks = ["command_complete"]
status = "confirmed"
origin = "user"
version = 1
api_version = "1"
```

`examples/scripts/notify_long_command.rhai`:
```rhai
// Notify Long Command — log when a command exceeds a duration threshold.
// Hook: command_complete
//
// Install: copy both files to ~/.glass/scripts/ or <project>/.glass/scripts/

let duration_secs = ctx.duration_ms / 1000;
let threshold = 10;

if duration_secs >= threshold {
    actions.log("info", `Command finished in ${duration_secs}s: ${ctx.command}`);
}
```

- [ ] **Step 4: Create block_rm_rf example**

A safety script that prevents `rm -rf /` and similar dangerous patterns. Demonstrates `command_start` hook with the ability to warn.

`examples/scripts/block_rm_rf.toml`:
```toml
name = "block_rm_rf"
hooks = ["command_start"]
status = "confirmed"
origin = "user"
version = 1
api_version = "1"
```

`examples/scripts/block_rm_rf.rhai`:
```rhai
// Block rm -rf — warn when a dangerous rm pattern is detected.
// Hook: command_start
//
// Install: copy both files to ~/.glass/scripts/ or <project>/.glass/scripts/

let cmd = ctx.command;

let dangerous = [
    "rm -rf /",
    "rm -rf /*",
    "rm -rf ~",
    "rm -rf $HOME",
];

for pattern in dangerous {
    if cmd.contains(pattern) {
        actions.log("warn", `Dangerous command blocked: ${cmd}`);
    }
}
```

- [ ] **Step 5: Create examples/scripts/README.md**

Brief README explaining:
- What Rhai scripts are and where they live (`~/.glass/scripts/` or `<project>/.glass/scripts/`)
- Each script needs a `.rhai` file and a `.toml` manifest
- Available hooks (reference the `HookPoint` enum: `command_start`, `command_complete`, `block_state_change`, `snapshot_before`, `snapshot_after`, `history_query`, `history_insert`, `pipeline_complete`, `config_reload`, `orchestrator_run_start`, `orchestrator_run_end`, `orchestrator_iteration`, `orchestrator_checkpoint`, `orchestrator_stuck`, `mcp_request`, `mcp_response`, `tab_create`, `tab_close`, `session_start`, `session_end`)
- Available actions (reference `actions.rs`: `run_command`, `log`, `set_config`, `write_file`)
- Link to the full scripting docs page in mdBook

- [ ] **Step 6: Commit**

```bash
git add examples/scripts/
git commit -m "docs(DOC-6): add example Rhai scripts with manifests

Three examples: auto git status on file-modifying commands,
long-command notification, dangerous rm pattern warning.
Includes README with hook list and install instructions."
```

---

### Task 6: Scripting mdBook page (DOC-7)

**Files:**
- Create: `docs/src/features/scripting.md`
- Modify: `docs/src/SUMMARY.md`

- [ ] **Step 1: Create docs/src/features/scripting.md**

Write a feature page covering:

1. **Overview** — Glass embeds a Rhai scripting engine that lets users automate reactions to terminal events. Scripts are lightweight `.rhai` files paired with `.toml` manifests.
2. **Installation** — Place script pairs in `~/.glass/scripts/` (global) or `<project>/.glass/scripts/` (project-scoped). Glass loads them on startup and on config reload.
3. **Manifest format** — Document the `.toml` fields: `name`, `hooks` (list of hook points), `status` (provisional/confirmed/rejected/stale/archived), `origin` (user/feedback), `version`, `api_version`.
4. **Hook points** — Table listing all 19 `HookPoint` variants with a one-line description of when each fires.
5. **Context object** — What `ctx` fields are available (varies by hook): `command`, `exit_code`, `duration_ms`, `cwd`, `output` (truncated), `block_id`.
6. **Actions** — What `actions` methods are available: `run_command(cmd)`, `log(level, msg)`, `set_config(section, key, value)`, `write_file(path, content)`.
7. **Sandbox** — Scripts run with resource limits: `max_operations`, `max_timeout_ms` (configurable in `[scripting]`). No filesystem access except through actions.
8. **Script lifecycle** — Explain the status flow: `provisional` (new/untested) → `confirmed` (validated by feedback loop or user) → `stale` (not triggered recently) → `archived`. User scripts start as `confirmed`.
9. **AI-generated scripts** — The feedback loop can generate scripts from orchestrator patterns. These start as `provisional` and are promoted to `confirmed` after validation. Controlled by `[scripting] script_generation = true`.
10. **MCP tools from scripts** — Scripts can expose custom MCP tools via the `mcp` module. Brief mention with pointer to the MCP reference page.
11. **Examples** — Link to `examples/scripts/` in the repo.
12. **Configuration** — Reference the `[scripting]` section of `config.example.toml`.

- [ ] **Step 2: Add scripting page to SUMMARY.md**

In `docs/src/SUMMARY.md`, add the scripting page entry after the Settings Overlay line (line 29):

```markdown
- [Scripting](./features/scripting.md)
```

This places it as the 12th feature in the Features section.

- [ ] **Step 3: Commit**

```bash
git add docs/src/features/scripting.md docs/src/SUMMARY.md
git commit -m "docs(DOC-7): add scripting feature page to mdBook

Covers hooks, manifests, context, actions, sandbox, lifecycle,
AI-generated scripts, MCP tool exposure, and configuration.
Added to SUMMARY.md feature list."
```

---

### Task 7: Reconcile config and doc-to-code mismatches (DOC-9, DOC-14)

**Files:**
- Modify: `crates/glass_mcp/src/lib.rs` (DOC-14 — fix tool count)
- Audit: `docs/src/configuration.md` vs `crates/glass_core/src/config.rs` (DOC-9)

- [ ] **Step 1: Fix glass_mcp lib.rs doc comment (DOC-14)**

In `crates/glass_mcp/src/lib.rs`, the module doc comment (lines 1-9) says "Provides four tools" but there are 33 MCP tools. Update:

```rust
//! glass_mcp — MCP server exposing Glass terminal capabilities to AI assistants.
//!
//! Provides 33 tools via the Model Context Protocol spanning history queries,
//! context summaries, undo/diff, tab/pane management, pipe inspection,
//! agent coordination, and scripting. All logging goes to stderr; stdout
//! carries only JSON-RPC messages.
```

Remove the 4-tool bullet list (lines 3-8) and replace with a high-level category summary. The individual tool documentation lives in the tool implementations in `tools.rs`.

- [ ] **Step 2: Audit docs/src/configuration.md against config.rs (DOC-9)**

Read `docs/src/configuration.md` and cross-reference every documented key against the actual `GlassConfig`, `HistorySection`, `SnapshotSection`, `SoiSection`, `PipesSection`, `AgentSection`, `OrchestratorSection`, and `ScriptingSection` structs. Fix:

- Any documented keys that don't exist in the struct (remove or note as planned)
- Any struct fields not documented (add)
- Default value mismatches (correct to match the `default_*` functions in config.rs)
- The `soi.min_lines` field exists in the struct (default 0) — ensure it's documented
- Font config uses top-level `font_family` and `font_size`, not a `[font]` section — verify docs match

This is an audit step. Fix whatever mismatches are found. The exact edits depend on the current state of `docs/src/configuration.md`.

- [ ] **Step 3: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add crates/glass_mcp/src/lib.rs docs/src/configuration.md
git commit -m "docs(DOC-9/DOC-14): reconcile doc-to-code mismatches

Fix glass_mcp lib.rs: 4 tools → 33 tools in module doc comment.
Audit configuration.md against actual GlassConfig struct fields
and fix any key/default mismatches."
```

---

### Task 8: CI/license badges and small fixes (DOC-10, DOC-15, DOC-16)

**Files:**
- Modify: `README.md` (badges)

- [ ] **Step 1: Add badges to README.md (DOC-10)**

Insert badges after the `# Glass` title (line 1) and before the description paragraph (line 3). Add:

```markdown
[![CI](https://github.com/candyhunterz/Glass/actions/workflows/ci.yml/badge.svg)](https://github.com/candyhunterz/Glass/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
```

Place on a single line separated by spaces, with a blank line before and after.

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs(DOC-10): add CI and license badges to README"
```

---

### Task 9: README screenshot placeholder (DOC-1)

**Files:**
- Modify: `README.md`

**Note:** This task prepares the markup and placeholder. The actual screenshot or GIF must be provided by the user — it cannot be generated programmatically.

- [ ] **Step 1: Add screenshot placeholder to README.md**

Insert after the badges (added in Task 8) and before the description paragraph. Add:

```markdown
<!-- TODO: Replace with actual screenshot or GIF demo showing command blocks, pipe viz, exit badges, and splits -->
<p align="center">
  <img src="docs/assets/hero-screenshot.png" alt="Glass terminal showing command blocks, pipe visualization, and split panes" width="800">
</p>
```

Create the `docs/assets/` directory. Do NOT create a placeholder image file — just the directory and the markdown reference.

```bash
mkdir -p docs/assets
```

- [ ] **Step 2: Add a note in CONTRIBUTING.md**

Append a note to the CONTRIBUTING.md (created in Task 2) under a "Screenshots" subsection:

```markdown
### Screenshots

The README references `docs/assets/hero-screenshot.png`. To update:
1. Open Glass with a session showing command blocks, pipe visualization, exit badges, and split panes
2. Capture at 800px width minimum
3. Save as PNG to `docs/assets/hero-screenshot.png`
```

- [ ] **Step 3: Commit**

```bash
git add README.md docs/assets/ CONTRIBUTING.md
git commit -m "docs(DOC-1): add screenshot placeholder markup in README

Prepares <img> tag pointing to docs/assets/hero-screenshot.png.
Actual image must be provided separately. Adds docs/assets/ directory."
```

---

### Task 10: Final verification and cleanup

- [ ] **Step 1: Run clippy**

```bash
cargo clippy --workspace -- -D warnings 2>&1
```

Fix any warnings (should only be from the `glass_mcp/src/lib.rs` doc comment change in Task 7).

- [ ] **Step 2: Run fmt**

```bash
cargo fmt --all -- --check 2>&1
```

Fix any formatting issues.

- [ ] **Step 3: Run full test suite**

```bash
cargo test --workspace 2>&1
```

- [ ] **Step 4: Verify all new files exist**

```bash
ls CHANGELOG.md CONTRIBUTING.md config.example.toml
ls .github/ISSUE_TEMPLATE/bug_report.md .github/ISSUE_TEMPLATE/feature_request.md .github/PULL_REQUEST_TEMPLATE.md
ls examples/scripts/auto_git_status.rhai examples/scripts/notify_long_command.rhai examples/scripts/block_rm_rf.rhai
ls docs/src/features/scripting.md
ls docs/assets/
```

- [ ] **Step 5: Commit any cleanup**

```bash
git add -A
git commit -m "chore: clippy and fmt cleanup for documentation branch"
```

- [ ] **Step 6: Summary — verify all items addressed**

Check off against the spec:
- [x] DOC-1: README screenshot placeholder (Task 9)
- [x] DOC-2: CHANGELOG.md (Task 1)
- [x] DOC-3: CONTRIBUTING.md + GitHub templates (Task 2)
- [x] DOC-4: Update CLAUDE.md — 9→14 crates (Task 3)
- [x] DOC-5: config.example.toml (Task 4)
- [x] DOC-6: Example Rhai scripts (Task 5)
- [x] DOC-7: Scripting mdBook page (Task 6)
- [x] DOC-9: Reconcile config mismatches (Task 7)
- [x] DOC-10: CI/license badges (Task 8)
- [x] DOC-14: Fix glass_mcp tool count (Task 7)

### Items not in scope for this branch

- **DOC-8 (MCP tool parameter docs):** Deferred — schemas are complex and best done as a dedicated pass with tool-by-tool verification.
- **DOC-11 (macOS keybinding column):** Deferred — requires macOS testing to verify bindings.
- **DOC-12 (Linux build deps in mdBook):** Partially covered by CONTRIBUTING.md (Task 2) which lists platform deps.
- **DOC-13 (module docs for glass_scripting/glass_core lib.rs):** Low priority — existing doc comments are adequate for internal crates.
- **DOC-15 (PRD template):** Deferred — PRD format is still evolving with orchestrator improvements.
- **DOC-16 (cargo doc in CI):** Deferred — CI changes belong in the setup-packaging branch.
- **DOC-17 (manual shell integration fallback):** Deferred — depends on setup-packaging branch embedding scripts first.

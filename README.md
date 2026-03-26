# Glass

[![CI](https://github.com/candyhunterz/Glass/actions/workflows/ci.yml/badge.svg)](https://github.com/candyhunterz/Glass/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

<!-- TODO: hero demo -->
<p align="center">
  <img src="docs/assets/hero-demo.gif" alt="Glass building a project autonomously from a PRD" width="800">
  <br>
  <em>27 iterations, 30 minutes, 20 commits — zero human intervention</em>
</p>

Write a PRD. Press Ctrl+Shift+O. Walk away. Come back to a built project.

Glass is an autonomous software development orchestrator. It pairs a reviewer agent with an implementer (Claude Code, Codex, Aider, or any CLI) and drives them through your project plan — planning, writing tests, implementing, verifying, committing, and iterating — all without human intervention. It runs for hours, resets context when it fills up, reverts regressions automatically, and pauses itself when it hits API usage limits.

It looks like a terminal because it is one. Glass is a full GPU-accelerated terminal emulator, but every feature exists to make the orchestrator more capable: command-level undo feeds the safety net, structured output parsing feeds the reviewer's context, MCP tools give the reviewer direct observability, and history search gives it memory across sessions.

- [How It Works](#how-it-works)
- [Quick Start](#quick-start)
- [Installation](#installation)
- [What the Orchestrator Does](#what-the-orchestrator-does)
- [Safety](#safety)
- [Writing a Good PRD](#writing-a-good-prd)
- [Provider Configuration](#provider-configuration)
- [Terminal Features](#terminal-features)
- [MCP Tools](#mcp-tools)
- [AI Integration](#ai-integration)
- [Architecture](#architecture)
- [Configuration](#configuration)
- [Keyboard Shortcuts](#keyboard-shortcuts)
- [FAQ](#faq)
- [Contributing](#contributing)
- [License](#license)

---

## How It Works

```
You write a PRD
       |
       v
 Glass Agent (reviewer)  <----  silence detected (implementer stopped working)
       |                              ^
       | TypeText instruction         |
       v                              |
 Claude Code (implementer)  --->  terminal output
       |
       v
 Tests pass? ── no ──> auto-revert, try again
       |
      yes
       |
       v
 Commit. Next feature.
       |
       v
 Context full? ── yes ──> checkpoint, respawn both agents, continue
       |
      no
       |
       v
 All done? ── yes ──> GLASS_DONE
```

Two agents, one terminal. The Glass Agent reviews terminal output, makes product decisions, and types instructions. The implementer writes code and runs tests. Glass manages the loop between them — silence detection triggers the reviewer, the metric guard catches regressions, and the checkpoint cycle keeps context fresh across 80+ iterations.

<!-- TODO: demo video -->
<details>
<summary><strong>Demo: Watch Glass build a full-stack app from a PRD</strong></summary>
<p align="center">
  <img src="docs/assets/demo-full-build.gif" alt="Glass orchestrator building a full-stack app" width="800">
</p>
</details>

---

## Quick Start

1. Install Glass ([download](#installation) or build from source)
2. Open Glass in your project directory
3. Write a `PRD.md`:

```markdown
---
title: My App
mode: build
verify: npm test
---

# My App

## Deliverables

### 1. User authentication
- Login/signup with email and password
- JWT tokens, refresh flow
- Protected route middleware

### 2. Dashboard
- Display user data from API
- Responsive layout
- Loading states and error handling
```

4. Start your implementer: `claude --dangerously-skip-permissions`
5. Press **Ctrl+Shift+O**
6. Walk away

<!-- TODO: demo video -->
<details>
<summary><strong>Demo: Zero to running orchestrator in 60 seconds</strong></summary>
<p align="center">
  <img src="docs/assets/demo-quickstart.gif" alt="Starting the orchestrator from scratch in 60 seconds" width="800">
</p>
</details>

The orchestrator auto-detects your project type, finds the test command, establishes a test baseline, and starts driving the implementer through the PRD. Open the dashboard (Ctrl+Shift+G, scroll to Orchestrator tab) to watch progress:

```
ORCHESTRATOR    iter #12/80 | 14m 32s | 1 respawns
Mode: build | Verify: floor (42 passed) | Guard: 11 kept, 1 reverted
Status: active
[========            ] 1/2 Dashboard
```

---

## Installation

### Pre-built binaries

| Platform | Download | Format |
|----------|----------|--------|
| Windows | [Glass-1.1.0-x86_64.msi](https://github.com/candyhunterz/Glass/releases/latest) | MSI installer |
| macOS (Apple Silicon) | [Glass-1.1.0-aarch64.dmg](https://github.com/candyhunterz/Glass/releases/latest) | DMG disk image |
| macOS (Intel) | [Glass-1.1.0-x86_64.dmg](https://github.com/candyhunterz/Glass/releases/latest) | DMG disk image |
| Linux (Debian/Ubuntu) | [glass_1.1.0_amd64.deb](https://github.com/candyhunterz/Glass/releases/latest) | deb package |

Or download from [github.com/candyhunterz/Glass/releases](https://github.com/candyhunterz/Glass/releases).

> **macOS Gatekeeper:** If macOS blocks Glass, run: `xattr -cr /Applications/Glass.app`

### Homebrew (macOS / Linux)

```bash
brew tap candyhunterz/glass
brew install glass
```

### Build from source

```bash
# Linux: install system deps first
# Debian/Ubuntu: sudo apt install libxkbcommon-dev libwayland-dev libx11-dev libxi-dev libxtst-dev
# Fedora: sudo dnf install libxkbcommon-devel wayland-devel libX11-devel libXi-devel libXtst-devel

git clone https://github.com/candyhunterz/Glass.git
cd Glass
cargo build --release
# Binary: target/release/glass (target\release\glass.exe on Windows)
```

### Cargo install

```bash
cargo install --git https://github.com/candyhunterz/Glass.git glass
```

---

## What the Orchestrator Does

### TDD cycle

The orchestrator follows a test-driven protocol for each feature:

1. **Plan** — tell the implementer what to build and define acceptance criteria
2. **Test first** — write tests before implementation
3. **Implement** — let the implementer work
4. **Verify** — run tests, demand actual output (not self-reported "tests pass")
5. **Commit** — only after tests pass
6. **Next** — move to the next feature, or fix if tests failed

### Checkpoint cycle

Claude Code (or any LLM implementer) runs out of context after ~15-20 iterations. The orchestrator handles this:

- Every N iterations (default 20), it writes a checkpoint summary
- Kills and respawns both agents with fresh context
- The new agents pick up from the checkpoint and continue
- Iteration count, test baseline, and project state all survive

This is why Glass can run 80+ iterations over hours — something no single-session agent can do.

### Metric guard

The orchestrator auto-detects your project's test/build commands (cargo test, npm test, pytest, go test, make test). After each iteration:

- Runs the test suite in the background
- If test count drops or build breaks → **auto-revert via git** to the last good commit
- Test floor only goes up, never down
- The implementer is told to try a different approach

### Stuck detection

If the reviewer gives 3 identical responses in a row, the orchestrator declares it stuck and forces a different approach — stash changes, try again from a different angle.

### Usage limits

Glass polls the Anthropic OAuth usage API every 60 seconds:

- **80%** — auto-pause, write checkpoint, kill agents
- **95%** — hard stop with emergency checkpoint
- **< 20%** — auto-resume from checkpoint, respawn agents, continue building

You can start a run before bed and it'll pause when you hit limits, resume when they reset, and keep going.

<!-- TODO: demo video -->
<details>
<summary><strong>Demo: Metric guard catches a regression and auto-reverts</strong></summary>
<p align="center">
  <img src="docs/assets/demo-metric-guard.gif" alt="Metric guard reverting a failing change" width="800">
</p>
</details>

### Self-improving feedback loop

After each run, Glass analyzes performance and auto-tunes itself:

- **Config tuning** — adjusts silence timeout, stuck threshold, checkpoint interval based on run metrics
- **Behavioral rules** — detects patterns (uncommitted drift, high revert rate) and enforces corrections
- **LLM analysis** — optional qualitative review of what went well and what didn't
- **Script generation** — when existing rules can't explain high waste/stuck rates, generates Rhai scripts for the scripting layer

---

## Safety

| Feature | What it does |
|---------|-------------|
| Metric guard | Auto-reverts any change that breaks tests or build |
| Test floor | Test count can only go up across iterations |
| Stuck recovery | Forces new approach after 3 identical responses |
| Crash recovery | Restarts implementer with checkpoint context |
| Usage auto-pause | Pauses at 80% API usage, resumes at 20% |
| Emergency checkpoint | Saves state at 95% usage before hard stop |
| Bounded iterations | Optional cap (e.g., 80) to prevent runaway runs |
| Config validation | Rejects unknown config field names |

---

## Writing a Good PRD

The PRD is what the orchestrator follows. A well-written PRD makes the difference between a clean 20-minute run and a stuck 80-iteration mess.

**Do:**
```markdown
### 1. User authentication
- Login endpoint: POST /api/auth/login with email + password
- Returns JWT access token (15min) and refresh token (7d)
- Middleware: reject requests without valid token
- Test: login with valid creds returns 200 + tokens
- Test: login with bad password returns 401
```

**Don't:**
```markdown
### 1. Auth
- Add user authentication
```

The orchestrator needs concrete acceptance criteria. "Add auth" gives the implementer no target to test against. Specific endpoints, response formats, and test cases let the TDD cycle work.

<!-- TODO: demo video -->
<details>
<summary><strong>Demo: Same task, vague PRD vs. specific PRD — side by side</strong></summary>
<p align="center">
  <img src="docs/assets/demo-prd-comparison.gif" alt="Comparing orchestrator results with vague vs specific PRDs" width="800">
</p>
</details>

**PRD frontmatter:**
```markdown
---
title: My Project
mode: build              # "build" for code projects, "general" for research/docs
verify: cargo test       # override auto-detected test command
---
```

**Deliverable headers:** Use `### N. Name` format — the orchestrator parses these for progress tracking in the dashboard.

---

## Provider Configuration

The Glass Agent (reviewer) and implementer can use different providers:

```toml
[agent]
# Glass Agent (reviewer) provider
provider = "claude-code"        # Default: Claude Code CLI
# provider = "anthropic-api"    # Claude via API (set ANTHROPIC_API_KEY)
# provider = "openai-api"       # GPT-4o, o3 (set OPENAI_API_KEY)
# provider = "ollama"           # Local models (localhost:11434)
model = ""                      # Empty = provider default

[agent.orchestrator]
# Implementer (the CLI running in the terminal)
implementer = "claude-code"     # Default
# implementer = "aider"
# implementer = "codex"
# implementer = "custom"
# implementer_command = "my-agent --flag"
```

---

## Terminal Features

Glass is a full terminal emulator — you can use it as your daily driver. Every terminal feature feeds the orchestrator's capabilities.

| Feature | What it does | How it helps the orchestrator |
|---------|-------------|------------------------------|
| Command blocks | Exit codes, durations, CWD badges on every command | Reviewer sees structured results, not raw text |
| Undo | Pre-exec filesystem snapshots (blake3 content-addressed) | Safety net for destructive commands |
| Pipeline visualization | Multi-row UI with per-stage inspection | Reviewer can debug pipe failures via MCP |
| History search | SQLite + FTS5, search overlay, CLI query | Reviewer queries what happened across sessions |
| Structured Output Intelligence | 19 format-specific parsers (cargo, npm, pytest, jest, git, docker, kubectl, tsc, Go, terraform, etc.) | Reviewer gets parsed test counts, not raw output |
| Tabs and split panes | Binary tree layout, drag-to-reorder | Multiple workspaces for parallel tasks |
| Shell integration | Auto-injected for bash, zsh, fish, PowerShell | OSC 133 sequences drive command lifecycle tracking |
| GPU rendering | wgpu (DX12/Metal/Vulkan) with glyphon text shaping | Handles massive output without lag |
| Settings overlay | In-app config editor (Ctrl+Shift+,) | Tune orchestrator settings mid-session |
| Hot-reload config | TOML config at ~/.glass/config.toml | Change settings without restarting |

<!-- TODO: demo video -->
<details>
<summary><strong>Demo: Glass as a daily-driver terminal — undo, pipes, search, splits</strong></summary>
<p align="center">
  <img src="docs/assets/demo-terminal-features.gif" alt="Glass terminal features: undo, pipeline visualization, search, split panes" width="800">
</p>
</details>

---

## MCP Tools

Glass exposes 33 MCP tools via `glass mcp serve`. These are auto-registered with your AI CLI on first launch — see [AI Integration](#ai-integration) for details. The orchestrator's reviewer agent uses these to verify work independently of what the implementer reports.

| Category | Tools | Purpose |
|----------|-------|---------|
| Context | `glass_context`, `glass_compressed_context` | See what's happening in the terminal |
| History | `glass_history` | Query past commands and output |
| Diffs | `glass_file_diff`, `glass_command_diff` | Verify what actually changed |
| Undo | `glass_undo` | Revert to pre-command state |
| Errors | `glass_extract_errors` | Pull structured errors from output |
| Pipes | `glass_pipe_inspect` | Inspect pipeline stage data |
| Tabs | `glass_tab_create/send/output/list/close` | Run commands in separate tabs |
| SOI queries | `glass_query`, `glass_query_trend`, `glass_query_drill` | Query structured output records |
| Live awareness | `glass_has_running_command`, `glass_cancel_command` | Know if something is still running |
| Token saving | `glass_cache_check` | Avoid re-reading unchanged output |
| Coordination | `glass_agent_register/lock/unlock/send/...` | Multi-agent file locking and messaging |
| Scripting | `glass_list_script_tools`, `glass_script_tool` | Execute Rhai automation scripts |
| Health | `glass_ping` | Verify connectivity |

## AI Integration

Glass is a universal enhancer for AI coding tools. Any AI CLI launched inside Glass — Claude Code, Codex, Aider, Cursor, Gemini — automatically gets capabilities it wouldn't have in a regular terminal:

**Persistent ground truth.** Every command, its output, exit code, duration, and working directory is recorded in a queryable SQLite database with full-text search. AI agents can look up what actually happened — across sessions, across context resets, across different AI tools. The model doesn't remember; it looks things up. That's more reliable than memory.

**Structured understanding.** Glass doesn't just store raw text. 19 format-specific parsers (SOI) extract test counts, compiler errors, container states, and more into structured records. When an AI asks "what failed?", it gets parsed data, not 500 lines of scrollback.

**Safety net.** Every command gets a pre-execution filesystem snapshot. AI agents make destructive mistakes — Glass catches them. Undo is one MCP call away, regardless of which AI tool made the change.

**Zero setup.** Glass auto-registers its MCP server with installed AI tools on first launch. No manual configuration needed — open Glass, start your AI tool, and it already has access to history, context, undo, and 30 other tools.

### How it works

Glass exposes its capabilities via MCP (Model Context Protocol). AI tools connect to `glass mcp serve` and gain access to 33 tools spanning history, context, undo, diffs, pipe inspection, and more. See [MCP Tools](#mcp-tools) for the full list.

### Supported tools

Auto-registration works out of the box for:

| Tool | Config written |
|------|---------------|
| Claude Code | `~/.claude/settings.local.json` |
| Cursor | `~/.cursor/mcp.json` |
| Windsurf | `~/.codeium/windsurf/mcp_config.json` |
| Any MCP-aware tool | `.mcp.json` in project root |

To register manually: add `glass mcp serve` as a stdio MCP server in your tool's configuration.

---

## Architecture

Glass is a Rust workspace with 16 crates. See [CONTRIBUTING.md](CONTRIBUTING.md) for detailed data flow diagrams and crate responsibilities.

```
glass (binary)           Event loop, orchestrator state machine, keyboard handling
glass_terminal           PTY, VT parsing (alacritty_terminal), block manager, silence detection
glass_renderer           GPU rendering, all overlays (dashboard, settings, search, pipes)
glass_mux                Tabs, split panes (binary tree layout)
glass_history            SQLite + FTS5 command history
glass_snapshot           Filesystem snapshots, blake3 blob store, undo engine
glass_pipes              Pipeline parsing and stage capture
glass_soi                19 format-specific output parsers, compression engine
glass_mcp                MCP server (33 tools)
glass_agent              Agent runtime, activity stream
glass_agent_backend      LLM backends (Claude CLI, OpenAI, Anthropic, Ollama)
glass_coordination       Multi-agent registry, advisory locks, messaging
glass_feedback           Self-improving feedback loop, rule engine, config tuning
glass_scripting          Rhai scripting, hook system, sandboxing
glass_core               Config, events, IPC
glass_errors             Structured error types
```

1,700+ tests. Clippy clean with `-D warnings`.

---

## Configuration

Glass reads `~/.glass/config.toml`. Changes are hot-reloaded.

```toml
font_family = "JetBrains Mono"
font_size = 14.0

[agent]
mode = "Assist"

[agent.orchestrator]
enabled = false
silence_timeout_secs = 6
prd_path = "PRD.md"
checkpoint_interval = 20
max_iterations = 80
max_retries_before_stuck = 4
verify_mode = "floor"
feedback_llm = false
```

See [config.example.toml](config.example.toml) for all options with defaults.

---

## Keyboard Shortcuts

| Action | Windows / Linux | macOS |
|--------|----------------|-------|
| Toggle orchestrator | Ctrl+Shift+O | Cmd+Shift+O |
| Dashboard | Ctrl+Shift+G | Cmd+Shift+G |
| Settings | Ctrl+Shift+, | Cmd+Shift+, |
| Search history | Ctrl+Shift+F | Cmd+Shift+F |
| Undo last command | Ctrl+Shift+Z | Cmd+Shift+Z |
| Toggle pipeline view | Ctrl+Shift+P | Cmd+Shift+P |
| New tab | Ctrl+Shift+T | Cmd+Shift+T |
| Close tab | Ctrl+Shift+W | Cmd+Shift+W |
| Split horizontal | Ctrl+Shift+D | Cmd+Shift+D |
| Split vertical | Ctrl+Shift+E | Cmd+Shift+E |
| Copy | Ctrl+Shift+C | Cmd+Shift+C |
| Paste | Ctrl+Shift+V | Cmd+Shift+V |

---

## FAQ

**What AI models does Glass support?**

The orchestrator (reviewer) supports Claude Code CLI, Anthropic API, OpenAI API, Ollama (local models), and any OpenAI-compatible endpoint. The implementer (code writer) can be Claude Code, Codex, Aider, Gemini, or any CLI you specify with a custom command. See [Provider Configuration](#provider-configuration).

**Can I use different models for the reviewer and implementer?**

Yes. The reviewer and implementer are fully independent. You can pair an Opus reviewer with a local Llama implementer, or a GPT-4o reviewer with Claude Code. Configure them separately in `config.toml`:

```toml
[agent]
provider = "anthropic-api"
model = "claude-sonnet-4-6"

[agent.orchestrator]
implementer = "claude-code"
```

**Why use Glass instead of a simple loop script?**

A loop script reruns your agent until it stops. Glass does that plus: auto-reverts when tests break, checkpoints and respawns when context fills up, detects when the agent is stuck and forces a new approach, pauses at API usage limits and resumes automatically, and learns from each run to improve the next one. The difference shows up around iteration 15 when a loop script's agent is confused by its own context and Glass has already checkpointed, respawned with a fresh summary, and kept going.

**Is it safe to leave running unattended?**

That's the intended use case. The metric guard auto-reverts any change that breaks tests. The test floor only goes up. Stuck detection forces new approaches after 3 identical responses. Usage auto-pause stops at 80% API usage and resumes when it drops below 20%. Bounded iterations cap the run length. The worst case is wasted API tokens on a stuck loop, not broken code — the guard catches regressions before they're committed.

**How do I install it?**

macOS: `brew tap candyhunterz/glass && brew install glass`. Windows: download the [MSI installer](https://github.com/candyhunterz/Glass/releases/latest). Linux: download the [.deb package](https://github.com/candyhunterz/Glass/releases/latest). Or `cargo install --git https://github.com/candyhunterz/Glass.git glass` on any platform.

**Do I need to configure MCP tools manually?**

No. Glass auto-registers its MCP server with Claude Code, Cursor, and Windsurf on first launch. It also writes a `.mcp.json` in your project root when the orchestrator activates, which any MCP-aware tool can discover. See [AI Integration](#ai-integration).

**Can I use Glass as my daily terminal?**

Yes. Glass is a full GPU-accelerated terminal emulator. Every terminal feature — tabs, split panes, shell integration, GPU rendering — works independently of the AI features. The orchestrator and AI tools are optional; the terminal stands on its own.

**What shells are supported?**

Bash, Zsh, Fish, and PowerShell. Shell integration (command blocks, undo, pipe visualization) is auto-injected for all four. Glass uses your system's default shell.

**Does the orchestrator work with any programming language?**

Yes. It's language-agnostic. The test command is auto-detected (`cargo test`, `npm test`, `pytest`, `go test`, `make test`) or can be set manually. If your project has a way to verify correctness via a shell command, the orchestrator can drive it.

**How does it handle long tasks that exceed context limits?**

Checkpoint cycling. Every N iterations (default 20), Glass writes a checkpoint summary of what's been done and what's next, kills both agents, and respawns them with fresh context plus the checkpoint. This is why Glass can run 80+ iterations over hours — something no single-session agent can do.

**Is it open source?**

Yes. MIT licensed. See [CONTRIBUTING.md](CONTRIBUTING.md) for how to get started.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for build instructions, architecture overview, data flow diagrams, and where to start.

---

## License

MIT. See [LICENSE](LICENSE).

---

[github.com/candyhunterz/Glass](https://github.com/candyhunterz/Glass)

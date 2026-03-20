# Configuration Reference

Glass is configured via a single TOML file. Changes are hot-reloaded at runtime — no restart required.

## In-App Settings Overlay

Press **Ctrl+Shift+,** (Cmd+Shift+, on macOS) to open the settings overlay. This provides a visual editor for common settings across three tabs:

- **Settings** — Browse 8 config sections (Font, Agent Mode, SOI, Snapshots, Pipes, History, Orchestrator, Scripting) with a sidebar. Use arrow keys to navigate, Enter/Space to toggle booleans, and +/- to adjust numeric values. Changes are written directly to `~/.glass/config.toml` and hot-reloaded immediately.
- **Shortcuts** — A two-column keyboard shortcut cheatsheet.
- **About** — Version info, platform details, and license.

Use Tab/Shift+Tab to switch between tabs, and Escape to close.

## Config File Location

`~/.glass/config.toml` on all platforms (macOS, Linux, Windows).

Glass resolves `~` to your home directory using the system's standard home directory lookup. If the file does not exist, Glass creates a commented-out default config on first launch.

## Error Handling

When Glass detects a config parse error, an error overlay appears in the terminal window showing the file path, line number, column, and a snippet of the offending line. Glass continues running with the last valid configuration until the file is corrected and saved. Fix the error and save — the hot-reload will pick up the correction automatically.

## Complete Example

```toml
font_family = "Cascadia Code"
font_size = 14.0
shell = "/bin/bash"

[history]
max_output_capture_kb = 50

[snapshot]
enabled = true
max_count = 1000
max_size_mb = 500
retention_days = 30

[pipes]
enabled = true
max_capture_mb = 10
auto_expand = true

[soi]
enabled = true
shell_summary = false
format = "oneline"
min_lines = 0

[terminal]
scrollback = 10000

[theme]
preset = "dark"

[agent]
mode = "off"
max_budget_usd = 1.0
cooldown_secs = 30
allowed_tools = "glass_query,glass_query_trend,glass_query_drill,glass_context,Bash,Read"

[agent.permissions]
edit_files = "approve"
run_commands = "approve"
git_operations = "approve"

[agent.quiet_rules]
ignore_patterns = []
ignore_exit_zero = false

[agent.orchestrator]
enabled = false
silence_timeout_secs = 60
prd_path = "PRD.md"
checkpoint_path = ".glass/checkpoint.md"
max_retries_before_stuck = 3
fast_trigger_secs = 5
verify_mode = "floor"
completion_artifact = ".glass/done"
orchestrator_mode = "auto"
feedback_llm = false
max_prompt_hints = 10
ablation_enabled = true
ablation_sweep_interval = 20
# max_iterations = 25
# verify_command = "cargo test"
# agent_prompt_pattern = "^>"
# verify_files = []

[scripting]
enabled = true
max_operations = 10000
max_timeout_ms = 5000
max_scripts_per_hook = 10
max_total_scripts = 100
max_mcp_tools = 50
script_generation = true
```

See also [`config.example.toml`](https://github.com/candyhunterz/Glass/blob/main/config.example.toml) in the repository root for a fully commented reference.

---

## Top-Level Keys

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `font_family` | string | platform default | Font family name for the terminal grid. Must be a monospace font installed on the system. |
| `font_size` | float | `14.0` | Font size in points. |
| `shell` | string | auto-detected | Shell binary to launch. Glass auto-detects from `SHELL` (Unix) or `COMSPEC` (Windows) if not set. |

### Platform Font Defaults

| Platform | Default Font |
|----------|--------------|
| Windows | `Consolas` |
| macOS | `Menlo` |
| Linux | `Monospace` |

---

## [history]

Controls command history capture and storage. History is stored in a per-project SQLite database with FTS5 full-text search.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `max_output_capture_kb` | int | `50` | Maximum kilobytes of command output captured per command. Output beyond this limit is truncated in the history database. The terminal display is not affected. |

---

## [snapshot]

Controls filesystem snapshot behavior. Snapshots are taken before destructive commands (such as `rm`, `mv`, `sed -i`) and stored in a content-addressed blob store keyed by blake3 hash. See [Undo](./features/undo.md) for usage details.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Enable or disable filesystem snapshots entirely. When disabled, the undo feature is unavailable. |
| `max_count` | int | `1000` | Maximum number of snapshots retained. Older snapshots are pruned when this limit is reached. |
| `max_size_mb` | int | `500` | Maximum total size of the snapshot blob store in megabytes. Oldest blobs are evicted when this limit is reached. |
| `retention_days` | int | `30` | Snapshots older than this many days are automatically pruned. |

---

## [pipes]

Controls pipeline visualization and stage capture. See [Pipe Inspection](./features/pipes.md) for usage details.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Enable or disable pipe visualization. When enabled, Glass captures intermediate output between pipeline stages. |
| `max_capture_mb` | int | `10` | Maximum megabytes captured per pipeline stage. Stage output beyond this limit is truncated. |
| `auto_expand` | bool | `true` | Automatically expand the pipe visualization panel when a pipeline command completes. |

---

## [soi]

Controls Structured Output Intelligence (SOI) — Glass's ability to parse and index structured output (JSON, CSV, tables) from commands for later querying via MCP tools.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Enable or disable SOI parsing. When disabled, command output is stored as plain text only. |
| `shell_summary` | bool | `false` | When true, Glass generates a one-line shell-visible summary of parsed structured output after each command completes. |
| `format` | string | `"oneline"` | Display format for SOI labels on command blocks. |
| `min_lines` | int | `0` | Minimum number of output lines required before Glass attempts structured parsing. Commands with fewer output lines are stored as plain text only. |

---

## [terminal]

Controls terminal behavior settings.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `scrollback` | int | `10000` | Number of lines of scrollback history retained in the terminal buffer. |

---

## [theme]

Controls terminal chrome colors (tab bar, status bar, block decorations, search overlay). Ships with `dark` (default) and `light` presets. Individual color fields override preset values.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `preset` | string | `"dark"` | Color preset: `"dark"` or `"light"`. Individual fields below override preset values. |
| `terminal_bg` | [R,G,B] | `[26,26,26]` | Terminal background color. |
| `tab_bar_bg` | [R,G,B] | `[30,30,30]` | Tab bar background color. |
| `tab_active_bg` | [R,G,B] | `[50,50,50]` | Active tab background color. |
| `tab_inactive_bg` | [R,G,B] | `[35,35,35]` | Inactive tab background color. |
| `tab_accent` | [R,G,B] | `[100,149,237]` | Active tab accent underline color. |
| `status_bar_bg` | [R,G,B] | `[38,38,38]` | Status bar background color. |
| `block_separator` | [R,G,B] | `[60,60,60]` | Block separator line color. |
| `badge_success` | [R,G,B] | `[40,160,40]` | Badge color for successful commands (exit 0). |
| `badge_error` | [R,G,B] | `[200,50,50]` | Badge color for failed commands (non-zero exit). |
| `badge_running` | [R,G,B] | `[30,120,200]` | Badge color for running commands. |
| `search_backdrop` | [R,G,B,A] | `[0.05,0.05,0.05,0.85]` | Search overlay backdrop (floats 0.0-1.0). |
| `search_input_bg` | [R,G,B] | `[56,56,56]` | Search input box background color. |

---

## [agent]

Controls the Glass AI agent integration. The agent runtime watches terminal activity and can propose or execute actions within configured permission limits.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `mode` | string | `"off"` | Agent operating mode. `"off"` disables the agent. `"watch"` monitors and surfaces summaries. `"assist"` prompts with context. `"autonomous"` acts within permission limits. |
| `max_budget_usd` | float | `1.0` | Maximum cumulative API spend in USD before Glass pauses agent actions and requires user confirmation to continue. |
| `cooldown_secs` | int | `30` | Minimum seconds between consecutive agent-initiated actions. |
| `allowed_tools` | string | `"glass_query,..."` | Comma-separated list of MCP tools the agent is allowed to use. |
| `provider` | string | `"claude-code"` | LLM provider for the agent backend. Options: `"claude-code"` (Claude CLI), `"anthropic-api"` (Anthropic Messages API), `"openai-api"` (OpenAI-compatible), `"ollama"` (local Ollama), `"custom"` (any OpenAI-compatible endpoint). |
| `model` | string | (none) | Model ID override. Empty uses the provider default. Examples: `"gpt-4o"`, `"claude-opus-4-6"`, `"llama3:70b"`. |
| `api_key` | string | (none) | API key for API-based providers. Environment variables take precedence (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`). |
| `api_endpoint` | string | (none) | Custom API endpoint URL. Only needed for `"custom"` provider or self-hosted endpoints. |

### [agent.permissions]

Granular permission controls for agent-initiated operations.

| Key | Type | Default | Options | Description |
|-----|------|---------|---------|-------------|
| `edit_files` | string | `"approve"` | `"approve"`, `"auto"`, `"never"` | Controls whether the agent can edit files. `"approve"` requires user confirmation per edit. `"auto"` allows edits without confirmation. `"never"` blocks all agent file edits. |
| `run_commands` | string | `"approve"` | `"approve"`, `"auto"`, `"never"` | Controls whether the agent can run shell commands. |
| `git_operations` | string | `"approve"` | `"approve"`, `"auto"`, `"never"` | Controls whether the agent can perform git operations. |

### [agent.quiet_rules]

Controls which agent actions are silently suppressed rather than surfaced to the user via notification.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `ignore_patterns` | array of strings | `[]` | Glob patterns for commands or file paths that the agent may act on without surfacing a notification. Example: `["*.log", "tmp/**"]`. |
| `ignore_exit_zero` | bool | `false` | When true, agent-run commands that exit with code 0 are not surfaced in the action log. |

### [agent.orchestrator]

Controls the orchestrator mode that drives autonomous project development. See [Orchestrator Mode](./features/orchestrator.md) for details.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | Enable orchestrator mode. Can also be toggled at runtime with Ctrl+Shift+O. |
| `silence_timeout_secs` | int | `60` | Seconds of PTY silence before sending context to the Glass Agent. |
| `fast_trigger_secs` | int | `5` | Seconds after output stops before fast-triggering the orchestrator. |
| `prd_path` | string | `"PRD.md"` | Path to the project requirements document. |
| `checkpoint_path` | string | `".glass/checkpoint.md"` | Path to the checkpoint file used for context refresh. |
| `max_retries_before_stuck` | int | `3` | Number of identical agent responses before stuck detection triggers. |
| `agent_prompt_pattern` | string | (none) | Optional regex pattern to detect the agent's prompt for instant triggering. |
| `verify_mode` | string | `"floor"` | Verification mode. `"floor"` auto-detects and runs verification commands after each iteration, auto-reverting on regression. `"disabled"` turns off the metric guard. |
| `verify_command` | string | (none) | Optional user override for the verification command. When set, skips auto-detection and agent discovery. |
| `completion_artifact` | string | `".glass/done"` | File path (relative to project root) that triggers the orchestrator when created. Set to empty string to disable. |
| `max_iterations` | int | (none) | Maximum iterations before checkpoint-stop. Omit or set to 0 for unlimited. |
| `orchestrator_mode` | string | `"auto"` | Orchestrator mode. `"auto"` detects project type at activation. `"build"` gives agent observation-only tools. `"audit"` gives all MCP tools. `"general"` for non-code tasks. |
| `verify_files` | array of strings | `[]` | Files to check for file-based verification. Auto-populated from PRD deliverables. |
| `feedback_llm` | bool | `false` | Enable LLM qualitative analysis after each orchestrator run. Produces Tier 3 prompt hints. Requires an extra API call per run. |
| `max_prompt_hints` | int | `10` | Maximum number of Tier 3 prompt hints retained per project. |
| `ablation_enabled` | bool | `true` | Enable automatic ablation testing of confirmed feedback rules. |
| `ablation_sweep_interval` | int | `20` | Number of runs between re-sweeps after full ablation coverage. |
| `implementer` | string | `"claude-code"` | Which CLI to launch as the implementer. Options: `"claude-code"`, `"codex"`, `"aider"`, `"gemini"`, `"custom"`. |
| `implementer_command` | string | (none) | Custom launch command when `implementer = "custom"`. |
| `implementer_name` | string | `"Claude Code"` | Display name for the implementer in the orchestrator's system prompt. All prompt references update automatically. |
| `persona` | string | (none) | Custom persona for the orchestrator agent. Can be an inline string or a path to a `.md` file (e.g., `".glass/agent-persona.md"`). |

---

## [scripting]

Controls the embedded Rhai scripting engine. Scripts can hook into Glass lifecycle events (snapshot, MCP requests, orchestrator iterations) for custom automation. See [Scripting](./features/scripting.md) for full details.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Enable or disable the Rhai scripting engine. When disabled, all scripts are skipped. |
| `max_operations` | int | (none) | Maximum Rhai operations per script execution. Prevents runaway scripts from blocking the event loop. |
| `max_timeout_ms` | int | (none) | Maximum wall-clock time per script execution in milliseconds. |
| `max_scripts_per_hook` | int | (none) | Maximum number of scripts that can register for a single hook point. |
| `max_total_scripts` | int | (none) | Maximum total number of registered scripts. |
| `max_mcp_tools` | int | (none) | Maximum number of MCP tools a script may expose. |
| `script_generation` | bool | `true` | Whether AI-assisted script generation is enabled. |

Scripts are loaded from two locations:
- `~/.glass/scripts/` — Global scripts, available in all projects
- `<project>/.glass/scripts/` — Project-local scripts, scoped to the project root

Each script has a `.toml` manifest describing its hook points, status (provisional or confirmed), and metadata.

---

## Hot-Reload Behavior

Glass uses a filesystem watcher to detect changes to `~/.glass/config.toml`. When a change is detected:

1. The file is re-parsed immediately.
2. If parsing succeeds, the new configuration is applied. Most settings (font size, history limits, snapshot limits, agent settings) take effect on the next relevant event without restarting the shell.
3. If parsing fails, the error overlay is shown and the previous configuration remains active.

The `shell` key is the only setting that requires launching a new terminal session to take effect — changing it does not restart your current shell.

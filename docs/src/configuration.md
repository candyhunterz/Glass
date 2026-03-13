# Configuration Reference

Glass is configured via a single TOML file. Changes are hot-reloaded at runtime — no restart required.

## Config File Location

`~/.glass/config.toml` on all platforms (macOS, Linux, Windows).

Glass resolves `~` to your home directory using the system's standard home directory lookup. If the file does not exist, Glass starts with built-in defaults and does not create the file automatically.

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
min_lines = 5

[agent]
enabled = false
mode = "watch"
max_budget_usd = 1.0
cooldown_secs = 30

[agent.permissions]
edit_files = "approve"
run_commands = "never"
git_operations = "never"

[agent.quiet_rules]
ignore_patterns = []
ignore_exit_zero = false
```

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
| Windows | `Cascadia Code` |
| macOS | `SF Mono` |
| Linux | `DejaVu Sans Mono` |

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

Controls Structured Output Inspection (SOI) — Glass's ability to parse and index structured output (JSON, CSV, tables) from commands for later querying via MCP tools.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `true` | Enable or disable SOI parsing. When disabled, command output is stored as plain text only. |
| `shell_summary` | bool | `false` | When true, Glass generates a one-line shell-visible summary of parsed structured output after each command completes. |
| `min_lines` | int | `5` | Minimum number of output lines required before Glass attempts structured parsing. Commands with fewer output lines are stored as plain text only. |

---

## [agent]

Controls the Glass AI agent integration. When enabled, Glass can respond to agent activity in the terminal and expose budget and permission guardrails to limit autonomous behavior.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `enabled` | bool | `false` | Enable or disable agent integration features. |
| `mode` | string | `"watch"` | Agent operating mode. `"watch"` — Glass monitors agent activity and surfaces summaries. `"assist"` — Glass can prompt the agent with context. `"autonomous"` — Glass acts on agent instructions within configured permission limits. |
| `max_budget_usd` | float | `1.0` | Maximum cumulative API spend in USD before Glass pauses agent actions and requires user confirmation to continue. |
| `cooldown_secs` | int | `30` | Minimum seconds between consecutive agent-initiated actions. |

### [agent.permissions]

Granular permission controls for agent-initiated operations.

| Key | Type | Default | Options | Description |
|-----|------|---------|---------|-------------|
| `edit_files` | string | `"approve"` | `"approve"`, `"auto"`, `"never"` | Controls whether the agent can edit files. `"approve"` requires user confirmation per edit. `"auto"` allows edits without confirmation. `"never"` blocks all agent file edits. |
| `run_commands` | string | `"never"` | `"approve"`, `"auto"`, `"never"` | Controls whether the agent can run shell commands. Defaults to `"never"` for safety. |
| `git_operations` | string | `"never"` | `"approve"`, `"auto"`, `"never"` | Controls whether the agent can perform git operations. Defaults to `"never"` for safety. |

### [agent.quiet_rules]

Controls which agent actions are silently suppressed rather than surfaced to the user via notification.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `ignore_patterns` | array of strings | `[]` | Glob patterns for commands or file paths that the agent may act on without surfacing a notification. Example: `["*.log", "tmp/**"]`. |
| `ignore_exit_zero` | bool | `false` | When true, agent-run commands that exit with code 0 are not surfaced in the action log. |

---

## Hot-Reload Behavior

Glass uses a filesystem watcher to detect changes to `~/.glass/config.toml`. When a change is detected:

1. The file is re-parsed immediately.
2. If parsing succeeds, the new configuration is applied. Most settings (font size, history limits, snapshot limits, agent settings) take effect on the next relevant event without restarting the shell.
3. If parsing fails, the error overlay is shown and the previous configuration remains active.

The `shell` key is the only setting that requires launching a new terminal session to take effect — changing it does not restart your current shell.

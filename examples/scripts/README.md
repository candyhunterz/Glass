# Example Rhai Scripts

Glass embeds a [Rhai](https://rhai.rs) scripting engine that lets you automate reactions to terminal events. Each script consists of two files:

- A `.rhai` file containing the script logic
- A `.toml` manifest describing metadata and hook points

## Installation

Copy both files for a script into one of these directories:

- `~/.glass/scripts/` -- global scripts, available in all projects
- `<project>/.glass/scripts/` -- project-local scripts, scoped to the project root

Glass loads scripts on startup and on config reload.

## Scripts in this directory

| Script | Hook | Description |
|--------|------|-------------|
| `auto_git_status` | `command_complete` | Logs after commands that modify tracked files |
| `notify_long_command` | `command_complete` | Notifies when a command takes longer than 10 seconds |
| `block_rm_rf` | `command_start` | Warns when a dangerous `rm -rf` pattern is detected |

## Manifest fields

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Unique script name |
| `hooks` | array | Hook points this script listens to |
| `status` | string | Lifecycle status: `provisional`, `confirmed`, `rejected`, `stale`, `archived` |
| `origin` | string | Where the script came from: `user` or `feedback` |
| `version` | int | Script version number |
| `api_version` | string | Glass scripting API version |

## Available hook points

| Hook | When it fires |
|------|---------------|
| `command_start` | A command is about to execute |
| `command_complete` | A command has finished executing |
| `block_state_change` | A command block changes state |
| `snapshot_before` | Before a filesystem snapshot is taken |
| `snapshot_after` | After a filesystem snapshot is taken |
| `history_query` | A history search is performed |
| `history_insert` | A command is inserted into history |
| `pipeline_complete` | A pipeline finishes executing |
| `config_reload` | Configuration file is reloaded |
| `orchestrator_run_start` | An orchestrator run begins |
| `orchestrator_run_end` | An orchestrator run ends |
| `orchestrator_iteration` | An orchestrator iteration completes |
| `orchestrator_checkpoint` | An orchestrator checkpoint is triggered |
| `orchestrator_stuck` | The orchestrator detects a stuck state |
| `mcp_request` | An MCP tool request is received |
| `mcp_response` | An MCP tool response is sent |
| `tab_create` | A new tab is created |
| `tab_close` | A tab is closed |
| `session_start` | A Glass session starts |
| `session_end` | A Glass session ends |

## Script API

Scripts have access to two main objects:

- **`glass`** -- read-only accessors for Glass state (`glass.cwd()`, `glass.git_branch()`, `glass.git_dirty_files()`, `glass.config(key)`, `glass.active_rules()`) and action methods (`glass.commit(msg)`, `glass.log(level, msg)`, `glass.notify(msg)`, `glass.set_config(key, value)`)
- **`event`** -- event-specific data fields that vary by hook point (e.g., `event.command`, `event.exit_code`, `event.duration_ms`)

## Full documentation

See the [Scripting feature page](../../docs/src/features/scripting.md) in the mdBook documentation for complete details on the sandbox, lifecycle, AI-generated scripts, and MCP tool exposure.

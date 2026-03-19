# Scripting

Glass embeds a [Rhai](https://rhai.rs) scripting engine that lets you automate reactions to terminal events. Scripts are lightweight `.rhai` files paired with `.toml` manifests.

## Installation

Place script pairs in one of two locations:

- **`~/.glass/scripts/`** -- global scripts, available in all projects
- **`<project>/.glass/scripts/`** -- project-local scripts, scoped to the project root

Glass loads scripts on startup and whenever the configuration is reloaded.

## Manifest Format

Every script needs a `.toml` manifest alongside its `.rhai` file. Both files share the same base name (e.g., `my_script.rhai` + `my_script.toml`).

```toml
name = "my_script"
hooks = ["command_complete"]
status = "confirmed"
origin = "user"
version = 1
api_version = "1"
```

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Unique identifier for the script |
| `hooks` | array of strings | Hook points this script listens to (see table below) |
| `status` | string | Lifecycle status: `provisional`, `confirmed`, `rejected`, `stale`, `archived` |
| `origin` | string | Where the script came from: `user` (manually created) or `feedback` (AI-generated) |
| `version` | integer | Script version number |
| `api_version` | string | Glass scripting API version |

## Hook Points

Scripts register for one or more hook points. Glass invokes the script each time the corresponding event fires.

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

## Context Object

Scripts receive two main objects:

### `glass` -- Glass state and actions

Read-only accessors:

| Method | Returns | Description |
|--------|---------|-------------|
| `glass.cwd()` | string | Current working directory |
| `glass.git_branch()` | string | Current git branch name |
| `glass.git_dirty_files()` | array | List of modified files in the working tree |
| `glass.config(key)` | dynamic | Value of a configuration key |
| `glass.active_rules()` | array | List of active feedback rule names |

### `event` -- Event-specific data

The `event` object contains fields specific to the hook point. Common fields include:

| Field | Available in | Type | Description |
|-------|-------------|------|-------------|
| `event.command` | `command_start`, `command_complete` | string | The command text |
| `event.exit_code` | `command_complete` | integer | Command exit code |
| `event.duration_ms` | `command_complete` | integer | Command duration in milliseconds |
| `event.cwd` | most hooks | string | Working directory at event time |
| `event.block_id` | block-related hooks | string | Command block identifier |

## Actions

Scripts can trigger side effects through `glass` methods:

| Method | Description |
|--------|-------------|
| `glass.commit(message)` | Commit current changes with the given message |
| `glass.log(level, message)` | Emit a log message. Levels: `debug`, `info`, `warn`, `error` |
| `glass.notify(message)` | Show a notification to the user |
| `glass.set_config(key, value)` | Update a configuration value |

## Sandbox

Scripts execute in a sandboxed environment with configurable resource limits:

- **`max_operations`** -- Maximum Rhai operations per execution (default: 10,000). Prevents runaway loops.
- **`max_timeout_ms`** -- Maximum wall-clock time per execution in milliseconds (default: 5,000).

Scripts cannot access the filesystem directly. All side effects must go through the `glass` action methods.

Configure sandbox limits in `~/.glass/config.toml`:

```toml
[scripting]
enabled = true
max_operations = 10000
max_timeout_ms = 5000
max_scripts_per_hook = 10
max_total_scripts = 100
max_mcp_tools = 50
script_generation = true
```

## Script Lifecycle

Scripts follow a lifecycle that manages trust and freshness:

1. **Provisional** -- New or untested script. Runs but with lower priority than confirmed scripts.
2. **Confirmed** -- Validated by the feedback loop or promoted by the user. Full priority.
3. **Stale** -- Has not been triggered recently. Reduced priority.
4. **Archived** -- No longer active. Skipped during execution.
5. **Rejected** -- Explicitly rejected by the user. Skipped during execution.

User-created scripts start as `confirmed`. AI-generated scripts start as `provisional` and are promoted after validation.

## AI-Generated Scripts

The feedback loop can generate scripts from orchestrator patterns. When `[scripting] script_generation = true` (the default), Glass may create new scripts based on observed patterns during orchestrator runs.

Generated scripts:
- Start with `provisional` status and `feedback` origin
- Are promoted to `confirmed` after successful validation
- Can be rejected by the user at any time
- Follow the same sandbox limits as user scripts

## MCP Tools from Scripts

Scripts can be invoked through MCP tools, allowing AI agents to execute registered scripts:

- **`glass_list_script_tools`** -- Discover available scripts and their hook points
- **`glass_script_tool`** -- Execute a registered script by name

See the [MCP Server](../mcp-server.md) reference for details.

## Examples

Example scripts with manifests are available in the repository at [`examples/scripts/`](https://github.com/candyhunterz/Glass/tree/main/examples/scripts).

## Configuration Reference

See the `[scripting]` section in [`config.example.toml`](https://github.com/candyhunterz/Glass/blob/main/config.example.toml) for all available options.

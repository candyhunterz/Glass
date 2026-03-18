# MCP Server

Glass includes a built-in [Model Context Protocol](https://modelcontextprotocol.io/) (MCP) server that exposes 33 tools covering terminal history, undo, pipe inspection, tab orchestration, structured output querying, multi-agent coordination, scripting automation, and more.

## What is MCP?

The Model Context Protocol is an open standard for connecting AI assistants to external tools and data sources. When Glass runs its MCP server, any MCP-compatible AI client can call Glass tools to read your command history, inspect pipeline output, trigger file restores, coordinate with other agents, and control terminal tabs — all from within the assistant's context window.

---

## Setup

### With Claude Desktop

Add Glass to your Claude Desktop MCP server configuration:

```json
{
  "mcpServers": {
    "glass": {
      "command": "glass",
      "args": ["mcp"]
    }
  }
}
```

Restart Claude Desktop after saving. Claude will discover all 33 tools automatically.

### With Claude Code

Add a Glass MCP entry to your project's `CLAUDE.md` or to your Claude Code MCP config file:

```json
{
  "mcpServers": {
    "glass": {
      "command": "glass",
      "args": ["mcp"]
    }
  }
}
```

Claude Code will connect to the running Glass instance and have access to all tools in the project context.

### With Any MCP Client

Point your MCP client at the `glass mcp` command. Glass follows the MCP stdio transport protocol. The client will discover available tools automatically via the standard `tools/list` call.

---

## Tool Reference

### History and Context

| Tool | Description |
|------|-------------|
| `glass_history` | Search command history using full-text search or filters (exit code, working directory, time range). Returns commands with their output, timing, and metadata. |
| `glass_context` | Retrieve full session context: recent commands, current working directory, active shell, and environment summary. |

### Undo and Diffs

| Tool | Description |
|------|-------------|
| `glass_undo` | Restore one or more files to their pre-command state using the snapshot system. Specify a command ID to restore the files that command modified. |
| `glass_file_diff` | Inspect the diff between a file's current state and its state before a specific command ran. Useful for reviewing what a command changed before deciding to undo. |

### Pipes

| Tool | Description |
|------|-------------|
| `glass_pipe_inspect` | Retrieve the captured output of individual pipeline stages from a piped command. Returns each stage's stdin and stdout separately. |

### Tab Orchestration

| Tool | Description |
|------|-------------|
| `glass_tab_create` | Open a new terminal tab, optionally specifying a working directory and shell command to run on start. |
| `glass_tab_list` | List all open tabs with their IDs, titles, current working directories, and active command state. |
| `glass_tab_send` | Send a string of text or a command to a specific tab's PTY input. |
| `glass_tab_output` | Read recent output from a specific tab. Supports head/tail/regex filters to limit tokens. |
| `glass_tab_close` | Close a specific tab by ID. |

### Token Saving

| Tool | Description |
|------|-------------|
| `glass_cache_check` | Check whether a previously cached context snapshot is still valid (no new commands have run, no files have changed). Lets agents skip re-fetching context when nothing has changed. |
| `glass_command_diff` | Return the file diffs produced by a specific command, summarized for token efficiency. Equivalent to `glass_file_diff` but scoped to all files a command touched. |
| `glass_compressed_context` | Return a budget-aware compressed summary of session context. Accepts a token budget and a focus mode (`errors`, `files`, or `history`) to prioritize what is included. |

### Error Extraction

| Tool | Description |
|------|-------------|
| `glass_extract_errors` | Parse command output and return structured error records with file path, line number, column, message, and severity. Supports compiler output, linter output, and common error formats. |

### Live Awareness

| Tool | Description |
|------|-------------|
| `glass_has_running_command` | Check whether a command is currently executing in a given tab. Returns the command text and elapsed time if one is running. |
| `glass_cancel_command` | Send Ctrl+C to a tab to cancel the currently running command. |

### SOI Query

Structured Output Intelligence (SOI) tools query indexed structured data parsed from command output using 19 format-specific parsers.

| Tool | Description |
|------|-------------|
| `glass_query` | Query structured output for a specific command by `command_id`. Accepts a token `budget` parameter to limit response size. |
| `glass_query_trend` | Run regression detection across the last N runs of a command pattern. Returns a trend summary indicating whether metrics have improved, degraded, or stayed stable. |
| `glass_query_drill` | Expand a specific SOI record to its full detail. Used after `glass_query` to fetch a single row or object in its entirety. |

### Scripting

Rhai scripting tools allow agents to discover and execute automation scripts registered in Glass.

| Tool | Description |
|------|-------------|
| `glass_list_script_tools` | List all available Rhai scripts with their names, hook points, and status (provisional/confirmed). |
| `glass_script_tool` | Execute a registered Rhai script by name, passing optional parameters. Scripts run in a sandboxed Rhai engine with CPU and memory limits. |

### Coordination

Multi-agent coordination tools share a SQLite database at `~/.glass/agents.db`. See [Multi-Agent Coordination](./agent-coordination.md) for the full protocol.

| Tool | Description |
|------|-------------|
| `glass_agent_register` | Register an agent on session start. Returns an agent ID used in all subsequent coordination calls. |
| `glass_agent_deregister` | Deregister an agent on session end and release all held locks. |
| `glass_agent_list` | List all currently registered agents for the project, including their status, current task, and held locks. |
| `glass_agent_status` | Update the calling agent's status and current task description. |
| `glass_agent_heartbeat` | Send a liveness heartbeat. Glass uses heartbeats together with PID detection to identify stale agent registrations. |
| `glass_agent_lock` | Acquire advisory locks on one or more file paths. Atomic and all-or-nothing: if any path is held by another agent, returns a conflict identifying the holder without acquiring any locks. |
| `glass_agent_unlock` | Release advisory locks on one or more file paths held by the calling agent. |
| `glass_agent_locks` | List all active advisory locks for the project, showing which agent holds each path. |
| `glass_agent_broadcast` | Send a message to all registered agents for the project. |
| `glass_agent_send` | Send a directed message to a specific agent by ID. Use `msg_type: "request_unlock"` to ask another agent to release a file. |
| `glass_agent_messages` | Retrieve unread messages for the calling agent (both directed and broadcast). Marks retrieved messages as read. |

### Health

| Tool | Description |
|------|-------------|
| `glass_ping` | Verify that the MCP server can reach the Glass GUI process. Returns latency in milliseconds. Useful for diagnosing connection issues. |

---

## Token Efficiency Features

Glass MCP is designed to work within context window budgets:

- **Filtered output**: `glass_tab_output` and `glass_history` accept `head`, `tail`, and `regex` filter parameters so agents can retrieve only the lines they need.
- **Cache staleness checks**: Call `glass_cache_check` before re-fetching context. If nothing has changed since the last fetch, skip the round-trip entirely.
- **Budget-aware compressed context**: `glass_compressed_context` accepts an explicit token budget and a focus mode (`errors`, `files`, or `history`) and returns the most relevant content that fits, with lower-priority sections truncated or omitted.

---

## Privacy

The Glass MCP server runs locally on your machine and communicates only via stdio. No terminal data, command history, or file content is sent to any external service. Only MCP clients running on your local machine can connect.

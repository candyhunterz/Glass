# Feature Landscape

**Domain:** Structured terminal history database, search overlay, MCP server, CLI query interface
**Researched:** 2026-03-05
**Milestone:** v1.1 Structured Scrollback + MCP Server
**Confidence:** HIGH (MCP spec verified via official docs; history patterns verified via Atuin, SQLite FTS5 official docs)

---

## Table Stakes

Features users expect from a terminal history database and search system. Missing any of these makes the feature feel incomplete or broken.

### History Database

| Feature | Why Expected | Complexity | Depends On |
|---------|--------------|------------|------------|
| Command text storage | The entire point of history -- users need to recall what they ran | Low | SQLite crate |
| CWD at execution time | Atuin, Recent, and every modern history tool captures this; Glass already has OSC 7 CWD tracking | Low | Existing OSC 7 |
| Exit code per command | Users filter for failed/successful commands; Glass already has OSC 133 exit codes | Low | Existing OSC 133 |
| Duration per command | Glass already shows duration badges; storing this is expected | Low | Existing duration tracking |
| Timestamp | Every history tool records when commands ran; needed for sorting, retention, and time-based queries | Low | None |
| Output capture (truncated) | The differentiating value of Glass -- terminal output is ephemeral by default; capturing it turns scrollback into a database | Medium | PTY read pipeline, storage limits |
| FTS5 full-text indexing on command text | Users expect instant substring/word search across history; FTS5 is the standard SQLite approach | Medium | SQLite FTS5 |
| Session tracking | Users need to distinguish commands from different terminal sessions, especially when multiple Glass windows are open | Low | Session UUID generation |
| Hostname field | Future-proofing for multi-machine use; Atuin proved this is expected metadata | Low | `gethostname` |

### Search Overlay UI

| Feature | Why Expected | Complexity | Depends On |
|---------|--------------|------------|------------|
| Ctrl+Shift+F activation | Standard terminal search keybinding (Windows Terminal, WezTerm, VS Code terminal all use this) | Low | Existing input handling |
| Incremental/live results | Users expect results to update as they type, not after pressing Enter; fzf set this expectation universally | Medium | FTS5 queries, UI rendering |
| Result highlighting | Matching text must be visually highlighted in results; every search UI does this | Medium | Text attribute rendering |
| Dismiss with Escape | Universal UI pattern for closing overlays | Low | Input handling |
| Navigate results with arrow keys | Up/Down to move through matches; Enter to select/jump | Low | Input handling |
| Jump to command block on select | Selecting a result should scroll the terminal to that block; this is what makes the overlay useful vs a separate tool | Medium | Block index, scroll position management |
| Search command text by default | The primary search target; users type a command fragment and expect matches | Low | FTS5 on command column |

### CLI Query Interface

| Feature | Why Expected | Complexity | Depends On |
|---------|--------------|------------|------------|
| `glass history search <query>` | Primary CLI entry point; Atuin uses `atuin search`, `history` is the natural subcommand for Glass | Low | SQLite read access, CLI arg parsing |
| Filter by exit code (`--exit`) | Atuin established this pattern; users want to find failed commands or successes | Low | SQL WHERE clause |
| Filter by CWD (`--cwd`) | Find commands run in a specific directory; Atuin and Recent both support this | Low | SQL WHERE clause |
| Filter by time range (`--after`, `--before`) | Find commands from a specific session or timeframe | Low | SQL WHERE clause |
| Limit results (`--limit`) | Pagination for large history sets | Low | SQL LIMIT |
| Human-readable output | Timestamps, durations, exit codes formatted for terminal display by default | Low | Formatting |

### MCP Server

| Feature | Why Expected | Complexity | Depends On |
|---------|--------------|------------|------------|
| JSON-RPC 2.0 over stdio transport | MCP spec mandates JSON-RPC 2.0; stdio is the standard local transport where the host starts the server as a subprocess | Medium | JSON-RPC parsing, stdin/stdout I/O |
| `initialize` handshake | MCP spec requires capability negotiation on connection; without it, no MCP client will proceed | Low | MCP protocol state machine |
| `tools/list` response | MCP clients discover available tools via this method; must declare `tools` capability with tool name, description, inputSchema | Low | Tool definitions |
| `tools/call` dispatch | MCP clients invoke tools via this method; must return `content` array with text results and `isError` flag | Medium | Tool implementations, history DB |
| At least one search/query tool | The reason the MCP server exists -- AI assistants need to query terminal history | Medium | History database, tool input schema |
| Tool input validation | MCP spec requires servers validate inputs against JSON Schema; LLMs regularly send malformed arguments | Low | JSON Schema validation |
| Error responses (protocol + tool execution) | MCP distinguishes protocol errors (-32602 for unknown tools) from tool execution errors (`isError: true` in result); both must work | Low | Error handling |

### Retention Policies

| Feature | Why Expected | Complexity | Depends On |
|---------|--------------|------------|------------|
| Max age retention (e.g., keep 90 days) | Without limits, the database grows unbounded; users expect configurable limits | Low | SQL DELETE + scheduled cleanup |
| Max database size limit | Output capture can consume significant storage; users need a ceiling | Medium | Database size monitoring, pruning strategy |
| TOML configuration for retention settings | Glass already uses TOML config; retention settings belong there | Low | Existing config system |

---

## Differentiators

Features that set Glass apart from both traditional shell history tools (Atuin, fzf) and other terminal emulators. Not expected by users, but create significant value.

| Feature | Value Proposition | Complexity | Depends On |
|---------|-------------------|------------|------------|
| **FTS5 on command output** | No terminal emulator indexes command output for search. Atuin and fzf only index the command text itself. Searching output ("which command printed that error message?") is a capability that does not exist anywhere else. | Medium | Output capture, FTS5 content table design |
| **MCP GlassContext tool** | Expose the current terminal context (recent N commands with output, CWD, git status) to AI assistants. Gives LLMs situational awareness without user copy-pasting. No terminal does this today. | Medium | Live terminal state access, MCP tool definition |
| **MCP GlassHistory structured output** | Return history results as structured JSON with `outputSchema` defined per the MCP spec. Most MCP servers return unstructured text blobs. Structured output enables LLMs to programmatically filter and process results. | Medium | History queries, MCP `outputSchema` |
| **Search overlay with output preview** | When navigating search results, show a preview of the command's captured output below the result list. Transforms the overlay from a command finder into a knowledge retrieval system. | High | Output storage, preview rendering in GPU pipeline |
| **Directory-scoped search** (workspace filtering) | Atuin calls this "workspace" -- show history from any directory within the current git repository tree. Useful for project-specific recall. | Medium | Git root detection, CWD prefix matching |
| **Pipe CLI results to stdout as JSON** | `glass history search "error" --format json` outputs structured JSON for piping into jq or feeding scripts. Makes Glass history scriptable. | Low | JSON serialization, output formatting |
| **Per-command output size tracking** | Track byte count of output per command. Enables queries like "show commands with most output" and informs retention pruning decisions. | Low | Byte counting during PTY capture |

---

## Anti-Features

Features to explicitly NOT build in v1.1. These are tempting but wrong for this milestone.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| **Cloud sync of history** | PROJECT.md explicitly scopes this out ("history and snapshots stay local, no telemetry"). Cloud sync requires encryption, auth, server infrastructure. Atuin's sync is their core business -- not Glass's differentiator. | Keep history strictly local. Database at platform-appropriate data directory (e.g., `%APPDATA%/glass/history.db` on Windows). |
| **Shell history replacement** (rebind Ctrl+R) | Glass is a terminal emulator, not a shell plugin. Rebinding Ctrl+R conflicts with PSReadLine and bash readline. Glass should provide its own search via its own keybinding, not hijack the shell's. | Use Ctrl+Shift+F for Glass search overlay. Shell Ctrl+R continues to work normally. |
| **Import from ~/.bash_history or Atuin** | Imported entries lack output, exit codes, and CWD. Creates data quality issues and a false sense of completeness. Glass history is richer than shell history by design. | Glass history begins when Glass starts capturing. Previous commands remain in shell history files. |
| **Interactive TUI for CLI** (full-screen search) | The CLI should be a stdout query tool, not a duplicate of the search overlay. Full-screen TUI adds complexity and duplicates the overlay's job. | `glass history search` prints results to stdout. The overlay (Ctrl+Shift+F) is the interactive experience. |
| **MCP HTTP/SSE transport** | Glass is a local application. stdio is simpler, more secure, and appropriate for local AI assistant integration. HTTP transport adds auth, CORS, port management overhead. The MCP spec is still evolving remote transports (SEPs targeting June 2026). | Implement stdio transport only. The MCP spec's stdio transport is well-established and used by Claude Code, Cursor, and other local MCP clients. |
| **AI-generated command suggestions** | PROJECT.md says "Glass is not an AI itself." Building suggestion UI couples Glass to specific AI providers and competes with its MCP data-source role. | Expose data via MCP tools. Let external AI assistants (Claude, GPT, etc.) provide suggestions through their own interfaces. |
| **Regex search in overlay** | Adds mode indicators, escape character handling, and error states to the overlay UI. FTS5 word matching and substring search cover 95% of use cases. | Plain text and FTS5 word matching in the overlay. The CLI can accept more advanced query patterns if needed. |
| **Output capture for alternate screen programs** | Programs using alternate screen (vim, htop, less, top) produce output that is not meaningful to capture or search. Capturing it wastes storage and pollutes results. | Detect alternate screen mode via alacritty_terminal VTE state. Skip output capture when alternate screen is active. Only capture primary screen output. |
| **MCP resources capability** | The MCP `resources` capability (URI-addressable history entries, subscriptions) adds complexity without clear immediate value. The `tools` capability covers the core query use case. | Ship with `tools` capability only. Evaluate `resources` for a future milestone based on AI assistant integration feedback. |

---

## Feature Dependencies

```
                    SQLite Database Schema
                         |
              +----------+----------+
              |          |          |
         FTS5 Index   Metadata   Output Capture
         (command +   Storage    (PTY read hook,
          output)    (commands    truncation,
              |       table)     alt-screen skip)
              |          |          |
              +-----+----+----+-----+
                    |         |
              +-----+    +---+----+
              |          |        |
        Search Overlay  CLI    Retention
        (Ctrl+Shift+F)  Query  Policies
              |         Interface  |
              |          |    +----+
              |          |    |
              +----+-----+----+
                   |
             MCP Server
             (stdio, JSON-RPC 2.0)
                   |
            +------+------+
            |             |
      GlassHistory   GlassContext
      (query DB)     (live state)
```

Key dependency chains:

1. **SQLite database schema** must exist before anything else -- it is the foundation for all v1.1 features
2. **FTS5 index + metadata storage** depend on the schema; FTS5 virtual table must be created alongside the commands table
3. **Output capture** depends on hooking into the PTY read pipeline (existing `glass_terminal` crate) and writing to the database; must handle truncation and alternate screen detection
4. **Search overlay** depends on FTS5 for queries and the existing block index for jump-to-block navigation
5. **CLI interface** depends on the database for queries; shares query logic with the overlay (extract into a shared query module)
6. **Retention policies** depend on the database; run as periodic cleanup (on startup + interval)
7. **MCP server** depends on the query layer for GlassHistory and live terminal state for GlassContext; must run as a separate process or be spawnable as a subprocess
8. **GlassContext** additionally depends on reading the current terminal session state (recent commands, CWD from OSC 7, git info from status bar)

### Critical Architectural Note: MCP Server Process Model

The MCP server communicates via stdio (stdin/stdout). This means it **cannot** be the same process as the terminal emulator (which uses stdout for rendering). Two viable approaches:

- **Separate binary** (`glass-mcp`): A standalone executable that reads the SQLite database directly. Simpler, no IPC needed for history queries. GlassContext requires either shared state or querying the DB for recent commands.
- **Subprocess of Glass**: Glass spawns the MCP server, communicating via pipes. More complex but enables GlassContext to access live terminal state.

Recommendation: **Separate binary** for v1.1. The database is the shared state. GlassContext can return recent commands from the DB (last N commands in current session) rather than requiring live terminal memory access.

---

## MVP Recommendation

### Phase 1: Database Foundation + Output Capture

1. **SQLite database with schema** -- commands table: id, timestamp, command_text, cwd, exit_code, duration_ms, session_id, hostname, output (TEXT, truncated)
2. **FTS5 virtual table** on command_text column -- enables fast text search
3. **Command metadata logging** -- hook into existing OSC 133 lifecycle (on command completion: write to DB)
4. **Output capture** -- buffer primary screen output between OSC 133;C (command start) and OSC 133;D (command end); truncate at configurable limit (default: 16KB per command); skip alternate screen
5. **Retention policy** -- max_age (default: 90 days), max_db_size (default: 500MB); cleanup on startup

### Phase 2: Search Overlay + CLI

6. **Search overlay** (Ctrl+Shift+F) -- text input bar, live FTS5 results, arrow key navigation, Enter to jump to block, Escape to dismiss
7. **CLI query interface** -- `glass history search <query>` with `--exit`, `--cwd`, `--after`, `--before`, `--limit`, `--format` (text/json)
8. **Shared query module** -- extract query building logic used by both overlay and CLI into a reusable module

### Phase 3: MCP Server

9. **MCP server binary** (`glass-mcp`) -- stdio transport, JSON-RPC 2.0, `initialize` handshake, `tools/list`, `tools/call`
10. **GlassHistory tool** -- inputSchema: query (string), filters (exit_code, cwd, after, before, limit); returns structured command history results with outputSchema
11. **GlassContext tool** -- inputSchema: count (number, default 10); returns recent N commands from current session with CWD, exit codes, output snippets

### Defer to v1.2+

- **FTS5 on output text**: Doubles index size. Validate storage impact of output capture first. Can add as a fast-follow once storage patterns are understood.
- **Output preview in search overlay**: High rendering complexity. The overlay v1 shows command text + metadata; output preview is a v1.2 enhancement.
- **Directory/workspace filtering**: Requires git root detection logic. Low user demand until project-scoped workflows are common.
- **MCP resources capability**: Tools cover the core use case. Resources add subscription complexity without clear demand.

---

## MCP Tool Definitions (Planned)

### GlassHistory

```json
{
  "name": "GlassHistory",
  "description": "Search terminal command history. Returns commands with metadata (exit code, duration, CWD, output snippet). Use to find what commands were run, what failed, or what produced specific output.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "query": {
        "type": "string",
        "description": "Search query to match against command text"
      },
      "cwd": {
        "type": "string",
        "description": "Filter to commands run in this directory"
      },
      "exit_code": {
        "type": "integer",
        "description": "Filter to commands with this exit code (0 = success)"
      },
      "after": {
        "type": "string",
        "description": "ISO 8601 timestamp. Only return commands after this time."
      },
      "before": {
        "type": "string",
        "description": "ISO 8601 timestamp. Only return commands before this time."
      },
      "limit": {
        "type": "integer",
        "description": "Maximum results to return (default: 20)"
      }
    }
  },
  "outputSchema": {
    "type": "object",
    "properties": {
      "commands": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "command": { "type": "string" },
            "cwd": { "type": "string" },
            "exit_code": { "type": "integer" },
            "duration_ms": { "type": "integer" },
            "timestamp": { "type": "string" },
            "output_snippet": { "type": "string" }
          }
        }
      },
      "total_matches": { "type": "integer" }
    },
    "required": ["commands", "total_matches"]
  }
}
```

### GlassContext

```json
{
  "name": "GlassContext",
  "description": "Get current terminal session context: recent commands with output, current working directory, and session info. Use to understand what the user has been doing in their terminal.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "count": {
        "type": "integer",
        "description": "Number of recent commands to return (default: 10, max: 50)"
      },
      "include_output": {
        "type": "boolean",
        "description": "Include command output in results (default: true)"
      }
    },
    "additionalProperties": false
  },
  "outputSchema": {
    "type": "object",
    "properties": {
      "session_id": { "type": "string" },
      "cwd": { "type": "string" },
      "recent_commands": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "command": { "type": "string" },
            "cwd": { "type": "string" },
            "exit_code": { "type": "integer" },
            "duration_ms": { "type": "integer" },
            "timestamp": { "type": "string" },
            "output": { "type": "string" }
          }
        }
      }
    },
    "required": ["session_id", "cwd", "recent_commands"]
  }
}
```

---

## Sources

- [Atuin - Shell History](https://github.com/atuinsh/atuin) -- Rust-based shell history with SQLite, the closest prior art for structured history
- [Atuin Search Reference](https://docs.atuin.sh/cli/reference/search/) -- CLI flags and filter options that set user expectations
- [Atuin Configuration](https://docs.atuin.sh/cli/configuration/config/) -- Search modes, filter modes, workspace filtering
- [SQLite FTS5 Extension](https://sqlite.org/fts5.html) -- Official FTS5 documentation for full-text search
- [MCP Specification - Tools](https://modelcontextprotocol.io/specification/draft/server/tools) -- Tool definition schema, inputSchema, outputSchema, error handling
- [MCP Specification (2025-11-25)](https://modelcontextprotocol.io/specification/2025-11-25) -- Protocol overview, stdio transport
- [MCP Transport Future](http://blog.modelcontextprotocol.io/posts/2025-12-19-mcp-transport-future/) -- Transport evolution (stdio remains standard for local)
- [rmcp - Official Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk) -- Rust implementation with `#[tool]` macro
- [MCP Tool Schema Guide](https://www.merge.dev/blog/mcp-tool-schema) -- Practical tool definition patterns
- [WezTerm Search](https://wezterm.org/config/lua/keyassignment/Search.html) -- Terminal emulator search overlay reference
- [fzf](https://github.com/junegunn/fzf) -- Fuzzy finder UI patterns that set user expectations for search
- [Better Shell History Search (2025)](https://tratt.net/laurie/blog/2025/better_shell_history_search.html) -- Analysis of history search approaches
- [Recent (bash-history-sqlite)](https://github.com/trengrj/recent) -- SQLite history with CWD, exit code, PID metadata
- [Historian](https://github.com/jcsalterego/historian) -- Command-line utility for managing shell history in SQLite

---

*Feature research for: Glass terminal emulator -- v1.1 Structured Scrollback + MCP Server*
*Researched: 2026-03-05*

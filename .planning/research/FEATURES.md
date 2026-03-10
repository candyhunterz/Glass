# Feature Landscape: Agent MCP Features (v2.3)

**Domain:** AI agent tooling for GPU-accelerated terminal emulator -- multi-tab orchestration, structured error extraction, token-saving tools, live command awareness
**Researched:** 2026-03-09
**Confidence:** MEDIUM-HIGH (strong prior art from iTerm2 Python API, kitty remote control, rustc JSON diagnostics, Claude Code Bash tool patterns; AI-agent-specific terminal tooling is still emerging)

## Table Stakes

Features that AI agents and competing terminal tools already provide or that agents need to function effectively. Missing any of these means Glass is not competitive as an agent-capable terminal.

| Feature | Why Expected | Complexity | Dependencies |
|---------|--------------|------------|--------------|
| Multi-tab create/list/close via API | iTerm2 Python API (`async_create_tab`), kitty remote control (`kitten @ launch`), and multiple terminal MCP servers all expose tab/session management. Any agent-controllable terminal must support programmatic tab lifecycle. | Medium | MCP command channel (new async bridge between MCP server and main event loop) |
| Run command in specific tab | iTerm2's `async_send_text`, kitty's `send-text`, and every terminal MCP server provides command execution in a named/numbered session. Core agent workflow: run server in tab 1, tests in tab 2. | Low | Tab create + PTY sender access via MCP channel |
| Read output from specific tab | Every terminal MCP server returns command output. Claude Code's Bash tool returns stdout/stderr (truncated at 30K chars). Agents must read results from the tabs they manage. | Medium | Grid FairMutex lock + ANSI escape stripping + character cap |
| Filtered/truncated output retrieval | Claude Code truncates at 30K chars and recently added disk persistence for overflow (anthropics/claude-code#12054). Agents routinely hit context overflow from large build/test output. Pattern filtering (like `grep -C`) is expected. | Low | Output access from history DB or live grid |
| Live command status (running/complete) | Agents need to know if a command is still executing before reading output. Without this, agents poll blindly or read incomplete data. kitty's `ls` returns window state; iTerm2's sessions expose `is_processing`. | Low | Block manager state inspection via MCP channel |
| Basic structured error extraction | Rust provides `--error-format=json` with rich structured diagnostics (spans, severity, suggestions). GCC/Clang emit `file:line:col: severity: message`. Agents currently waste tokens parsing raw error text. At minimum, a generic `file:line:col: message` parser is expected. | Medium | New glass_errors crate with regex-based parsers |

## Differentiators

Features that set Glass apart from generic terminal MCP servers and other AI-terminal integrations. Not universally expected, but provide high value by leveraging Glass's unique data infrastructure.

| Feature | Value Proposition | Complexity | Dependencies |
|---------|-------------------|------------|--------------|
| Cached result with staleness detection (`glass_cached_result`) | No other terminal tracks whether files changed since a command ran. Agents can skip re-running `cargo test` if nothing changed -- saves wall-clock time AND tokens. Unique to Glass because it has BOTH command history AND file snapshot data to cross-reference. | Medium | History DB query + snapshot timestamp comparison for staleness heuristic |
| Changed files with diffs (`glass_changed_files`) | Glass already has pre-command snapshots via content-addressed blob store. Exposing "what did this command change" as unified diffs eliminates the agent pattern of re-reading entire files to check for changes. No other terminal MCP server has snapshot infrastructure to enable this. | Medium | Snapshot DB + `similar` crate for diff generation |
| Budget-aware context compression (`glass_context --budget`) | After context resets, agents waste tokens on bloated context restoration. A budget parameter that prioritizes failed commands > file modifications > recent commands > successful commands (just counts) is intelligent summarization. No other tool offers token-budget-aware terminal context. | Medium | Existing glass_context tool enhancement with priority-sorted truncation |
| Auto-detecting error format from command hint | Most error parsing tools require explicit language selection. Glass can infer the parser from command text ("cargo build" implies Rust, "pytest" implies Python) AND from output content ("Traceback" implies Python, "error[E" implies Rust). Multi-language auto-detection in a single tool call is uncommon. | Medium | Command text hint mapping + content-based fallback detection |
| Command cancel via MCP | Agents can send SIGINT/Ctrl+C to a running command without user intervention. Enables autonomous "run, check output, cancel if stuck" workflows. Most terminal MCP servers only support fire-and-forget execution. | Low | PTY signal byte writing via MCP channel |
| Cross-tab orchestration with command awareness | Managing a full dev environment (server + watcher + tests in separate tabs) through a single MCP session, with Glass's command-awareness layered on top: check if test tab's command finished, read only errors from build tab, check if server tab is still running. iTerm2 can manage tabs via Python API, but lacks the command lifecycle awareness that Glass's OSC 133 integration provides. | Low (additive) | All tab tools + command status working together |

## Anti-Features

Features to explicitly NOT build. These are tempting but would add complexity without proportionate value, or conflict with Glass's design philosophy.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| Built-in AI command suggestion | Glass exposes data TO agents; it is not an agent itself. Adding AI suggestions creates product confusion and competes with the very agents Glass serves. Warp does this; Glass should not. | Expose rich context via MCP tools. Let agents decide what to run. |
| Streaming output via MCP | MCP over stdio does not support server-initiated streaming well. The current MCP spec (2025-03-26) added Streamable HTTP, but Glass uses stdio transport by design (local only, no network security concerns). Implementing streaming adds complexity for marginal benefit. | Provide polling-based `glass_tab_output` with a `has_running_command` flag so agents know when to re-poll. Agents already handle async polling. |
| Automatic error correction | Parsing errors is valuable; automatically running fix commands crosses into agent territory and creates safety/trust concerns. | Return structured errors with file, line, column, message, and severity. Let the agent decide the fix. |
| Persistent named sessions across restarts | Session persistence adds significant state management complexity (orphaned PTYs, stale grid state, file handle cleanup). Tab IDs being ephemeral is fine for agent workflows which are themselves ephemeral. | Use numeric tab IDs within a session. Agents can re-create tabs on reconnect. Document this as expected behavior. |
| Remote MCP transport (HTTP/SSE) | Network transport adds security attack surface for a tool that executes arbitrary commands. PROJECT.md explicitly lists this as out of scope. | Keep MCP over stdio. Agents connect locally. This is a security feature. |
| Full shell AST parsing for error detection | Shell syntax is Turing-complete. Trying to deeply parse arbitrary shell output is unbounded complexity with diminishing returns. | Use regex-based parsers with language-specific heuristics. Accept that some outputs will fall through to the generic `file:line:col` fallback. This is honest and practical. |
| Exact token counting in budget mode | Exact token counting requires a tokenizer dependency (tiktoken or similar), adding binary size and complexity for approximate benefit. Different models tokenize differently anyway. | Use character-based approximation (1 token approximately equals 4 chars). Good enough for budget targeting. Over-counting slightly is better than under-counting. |
| Tab output diffing (delta between polls) | Tracking what changed since the last `glass_tab_output` call requires per-caller state management in the MCP server. Adds complexity for a niche use case. | Return full output (last N lines) each time. Agents can diff locally if needed. The `has_running_command` flag tells them when output is final. |

## Feature Dependencies

```
MCP Command Channel (new infrastructure)
    |
    +-- glass_tab_create
    +-- glass_tab_list
    +-- glass_tab_run
    +-- glass_tab_output ---- glass_output (tab_id mode)
    +-- glass_tab_close
    +-- glass_command_status
    +-- glass_command_cancel

History DB (existing)
    +-- glass_output (command_id mode)
    +-- glass_cached_result
    +-- glass_errors (command_id mode)

Snapshot DB (existing)
    +-- glass_changed_files

glass_errors crate (new, pure library)
    +-- glass_errors MCP tool

glass_context (existing)
    +-- glass_context budget/focus enhancement
```

**Critical path:** The MCP Command Channel is the foundation for 7 of 12 new tools. It must be built first. The remaining 5 tools (glass_output from history, glass_cached_result, glass_changed_files, glass_errors from history, glass_context budget) can be built independently since they only access existing SQLite databases.

## MVP Recommendation

### Must-have (builds on critical path):

1. **MCP Command Channel** -- Async channel bridge between MCP server and main event loop. Unblocks all live session tools. Without this, half the features are impossible. This is infrastructure, not user-facing, but it is the highest priority.
2. **glass_tab_create / glass_tab_list / glass_tab_close** -- Basic tab lifecycle. Table stakes for orchestration. Reuses existing create_session/close_session flows.
3. **glass_tab_run / glass_tab_output** -- Core agent workflow: run command, read output. The entire reason agents want tabs.
4. **glass_output (filtered)** -- Highest token-saving impact per implementation effort. Pattern filtering on build output saves 80-95% of tokens. Works from both history DB and live grid.
5. **glass_command_status** -- Agents must know if a command finished before reading output. Without this, glass_tab_output returns incomplete data silently.

### Should-have (high value, independent of critical path):

6. **glass_errors** -- Structured error extraction. High value but requires building parser infrastructure. Rust parser is most relevant to Glass's own development; generic fallback covers most other tools via `file:line:col: message`.
7. **glass_cached_result** -- Major differentiator. Saves wall-clock time, not just tokens. Requires staleness detection via snapshot timestamp cross-reference.
8. **glass_changed_files** -- Leverages existing snapshot infrastructure uniquely. Adding `similar` crate for unified diffs is straightforward.

### Nice-to-have (lower priority):

9. **glass_context budget/focus** -- Enhancement to existing tool. Useful but agents can work around it by using glass_output with pattern filters.
10. **glass_command_cancel** -- Sends SIGINT via PTY. Simple to implement, less frequently needed by agents.
11. **Additional error parsers** (Python, Node, Go, GCC beyond generic) -- Rust parser and generic fallback cover the primary use cases. Others add breadth but can iterate based on user demand.

### Defer:

- **Python/Node/Go/GCC dedicated parsers** -- Generic fallback handles the common `file:line:col: message` pattern. Dedicated parsers can be added incrementally based on actual agent usage patterns.

## Complexity Assessment

| Feature | Est. Lines | Risk | Key Challenge |
|---------|-----------|------|---------------|
| MCP Command Channel | 200-300 | **HIGH** | Crosses crate boundaries (glass_mcp to main.rs). Async channel with oneshot reply. Timeout handling for requests when main event loop is busy. Must not block the winit event loop. |
| Tab lifecycle (create/list/close) | 150-200 | Medium | Reuses existing create_session/close flows. Main risk: stable tab ID semantics if tabs are reordered. Consider using session_id (UUID) as stable reference. |
| Tab run/output | 100-150 | Medium | PTY write is trivial (bytes + newline). Grid read requires FairMutex lock + iterating grid rows + ANSI stripping. Must cap output size (100KB). |
| glass_output (filtered) | 100-150 | Low | Regex pattern compilation, line filtering, character budget. Well-understood problem. |
| glass_command_status | 50-80 | Low | Read BlockManager state enum. Return running/complete/idle. |
| glass_command_cancel | 30-50 | Low | Write ETX (0x03) to PTY sender. Cross-platform signal semantics. |
| glass_errors crate | 300-500 | Medium | Multiple regex parsers. Rust parser is most complex (multi-line error spans with `-->` arrows). But `rustc --error-format=json` exists -- consider parsing JSON output instead of human-readable text. |
| glass_cached_result | 100-150 | Medium | SQL query for matching command + staleness check. Edge cases: CWD mismatch, fuzzy command matching, partial output in history. |
| glass_changed_files | 150-200 | Low-Medium | Snapshot query + blob read + `similar` unified diff. Well-understood; `similar` crate is battle-tested. |
| glass_context budget | 80-120 | Low | Priority-sorted data with character truncation. Enhancement to existing code. |

## Competitive Landscape

### Terminal MCP servers:

| Tool | Tab Management | Error Parsing | Token Optimization | Command Status |
|------|---------------|---------------|-------------------|----------------|
| **terminal-mcp** (generic) | No | No | No | No |
| **iTerm MCP server** | Yes (via Python API bridge) | No | No | Session state only |
| **kitty remote control** | Yes (JSON protocol) | No | No | Window state via `ls` |
| **Warp** | Built-in AI, not MCP-exposed | "Ask Warp AI" for errors (proprietary) | Block model, implicit | Visual indicators |
| **Claude Code Bash tool** | No (single shell) | No (raw output) | 30K char truncation, disk overflow | No |
| **Glass v2.3 (proposed)** | Yes (MCP tools) | Yes (structured, auto-detect) | Pattern filter, cache, budget, diffs | Yes (block state) |

### Glass's unique position:

Glass is the only terminal that combines: (a) command-aware history with FTS5 search, (b) file snapshots with content-addressed dedup, (c) MCP exposure of all the above to arbitrary AI agents, and (d) multi-agent coordination. The v2.3 tools leverage this data infrastructure to provide capabilities no other terminal can offer -- particularly `glass_cached_result` (cross-referencing history timestamps with snapshot timestamps) and `glass_changed_files` (diffs from pre-command snapshots).

### AI agent pain points addressed:

| Pain Point | Current Workaround | Glass Solution | Token Savings |
|-----------|-------------------|----------------|---------------|
| Large output overflows context | Pipe to head/tail, truncation at 30K chars | `glass_output` with regex pattern filter + line limits | 80-95% for build/test output |
| Re-running commands after context reset | Run everything again, waste minutes | `glass_cached_result` with file-change staleness check | 100% when cache is valid |
| Parsing compiler errors from raw text | Regex in system prompt, fragile across languages | `glass_errors` with auto-detected per-language parsers | 60-80% (structured vs raw) |
| Managing multiple terminal sessions | Single tab, sequential commands, manual switching | `glass_tab_*` orchestration: server + tests + watcher in parallel | Indirect: faster workflows |
| Not knowing if command is still running | Sleep and hope, or check exit code post-hoc | `glass_command_status` returns live block state | Eliminates wasted polls |
| Checking what a command changed | Re-read all potentially modified files | `glass_changed_files` returns only the diffs | 70-90% for multi-file edits |

## Sources

- [iTerm2 Python API - Tab Management](https://iterm2.com/python-api/examples/launch_and_run.html) -- async_create_tab, async_send_text patterns. HIGH confidence.
- [kitty Remote Control Protocol](https://sw.kovidgoyal.net/kitty/remote-control/) -- JSON-based programmatic terminal control with tab/window management. HIGH confidence.
- [rustc JSON Output Format](https://doc.rust-lang.org/rustc/json.html) -- Structured diagnostic messages with spans, severity levels, suggestion applicability. HIGH confidence.
- [Cargo External Tools](https://doc.rust-lang.org/cargo/reference/external-tools.html) -- `--message-format=json` for machine-readable compiler output. HIGH confidence.
- [terminal-mcp on GitHub](https://github.com/elleryfamilia/terminal-mcp) -- Generic terminal MCP server as baseline competitor. MEDIUM confidence.
- [Claude Code Bash Tool](https://docs.claude.com/en/docs/agents-and-tools/tool-use/bash-tool) -- 30K char truncation, output handling patterns. HIGH confidence.
- [Claude Code Output Overflow Issue #12054](https://github.com/anthropics/claude-code/issues/12054) -- Real agent pain point: tool outputs consuming entire context window. HIGH confidence.
- [Anthropic: Writing Effective Tools for Agents](https://www.anthropic.com/engineering/writing-tools-for-agents) -- Structured error messages reduce retries; concise responses save tokens. HIGH confidence.
- [Anthropic: Effective Context Engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents) -- Budget-aware context management patterns. HIGH confidence.
- [Warp Terminal Features](https://www.warp.dev/all-features) -- Block model, AI error explanation, competitive landscape. MEDIUM confidence.
- [MCP Streaming Best Practices](https://www.byteplus.com/en/topic/541918) -- Polling vs streaming trade-offs for MCP transport. MEDIUM confidence.
- [it2 CLI for iTerm2](https://github.com/mkusaka/it2) -- CLI wrapping iTerm2 Python API for tab management. MEDIUM confidence.

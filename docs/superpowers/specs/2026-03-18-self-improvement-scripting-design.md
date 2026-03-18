# Glass Self-Improvement Scripting Layer — Design Spec

## Overview

An embedded Rhai scripting layer that lets Glass improve itself at runtime. The feedback loop (currently limited to TOML config tuning and predefined behavioral rules) gains the ability to write, validate, and promote real scripts — new logic, not just new parameters. Scripts hook into every component of Glass via the event pipeline, expose a curated action API for safe side effects, and can register dynamic MCP tools. Each user's Glass instance diverges over time based on their workflow, with export/import for community sharing.

## Motivation

The orchestrator feedback loop already demonstrates self-improvement: it detects patterns across runs, generates findings, and adjusts future behavior. But it's limited to three tiers:

- **Tier 1:** Config value tuning (silence timeout, stuck threshold)
- **Tier 2:** Toggle predefined behavioral rules (force_commit, split_instructions)
- **Tier 3:** LLM-generated prompt hints (free-form text injected into agent context)

All three are constrained to what the Rust code already implements. The scripting layer adds:

- **Tier 4:** LLM-generated Rhai scripts — genuine new logic that loads at runtime without recompilation

This extends self-improvement beyond the orchestrator to the entire Glass application: command handling, snapshots, history, pipes, MCP, and session management.

## Architecture

### Crate Structure

```
crates/glass_scripting/
  src/
    lib.rs          - Public API: ScriptEngine, ScriptHook, Action
    engine.rs       - Rhai engine setup, sandbox config, script compilation
    hooks.rs        - Hook registry (which scripts run on which events)
    actions.rs      - Action enum (all things scripts can request)
    sandbox.rs      - Execution limits (time, memory, max scripts)
    loader.rs       - Load scripts from disk, watch for changes
    lifecycle.rs    - Provisional/confirmed/rejected script status tracking
    mcp.rs          - Dynamic MCP tool registration from scripts
    profile.rs      - Export/import profile bundles

src/script_bridge.rs  - Event routing, action execution, Glass state access
```

### Dependencies

`glass_scripting` depends on:
- `glass_core` — event types, config types (only internal dependency)
- `rhai` — scripting engine
- `serde`, `toml`, `serde_json` — serialization
- `schemars` — JSON schema generation for dynamic MCP tool parameter definitions
- `tracing` — logging

No dependencies on glass_history, glass_snapshot, glass_terminal, glass_mcp, or any other crate. The bridge in the binary handles integration with those systems. MCP tool definitions use `schemars` for JSON schema generation rather than importing rmcp types.

### Dependency Graph Position

```
glass_core ──> glass_scripting (same layer as glass_feedback)
                    ^
              main binary (via script_bridge.rs)
```

### Bridge Module

`src/script_bridge.rs` in the main binary:
- Owns the `ScriptEngine` instance
- One method per hook point (e.g., `on_command_complete(&data)`)
- Executes returned `Action`s against real Glass state (history, snapshot, config, git)
- `main.rs` calls bridge methods at each event dispatch point — one line per hook

The bridge pattern mirrors how `glass_feedback` is wired today: a standalone crate with a clean API, integrated via thin calls in the event loop.

## Hook System

### Hook Points

Every component of Glass exposes hook points that scripts can subscribe to:

```rust
enum HookPoint {
    // Commands
    CommandStart,           // block transitions to Executing
    CommandComplete,        // block transitions to Complete

    // Blocks
    BlockStateChange,       // any block state transition

    // Snapshots
    SnapshotBefore,         // about to snapshot (script can filter)
    SnapshotAfter,          // snapshot taken

    // History
    HistoryQuery,           // search executed
    HistoryInsert,          // command record stored

    // Pipes
    PipelineComplete,       // all stages finished

    // Config
    ConfigReload,           // config.toml changed

    // Orchestrator
    OrchestratorRunStart,   // Ctrl+Shift+O on
    OrchestratorRunEnd,     // deactivation
    OrchestratorIteration,  // each silence->response cycle
    OrchestratorCheckpoint, // checkpoint fired
    OrchestratorStuck,      // stuck detected

    // MCP
    McpRequest,             // tool call received
    McpResponse,            // tool result returned

    // Session
    TabCreate,              // new tab opened
    TabClose,               // tab closed
    SessionStart,           // Glass launched
    SessionEnd,             // Glass shutting down
}
```

### Script Format

Each script has a companion TOML manifest (same filename, `.toml` extension) and the Rhai source file:

```toml
# ~/.glass/scripts/hooks/auto_commit_on_green.toml
name = "auto-commit-on-green"
hooks = ["CommandComplete"]
status = "confirmed"
origin = "feedback"
version = 1
api_version = 1
created = "2026-03-18"
```

```rhai
// ~/.glass/scripts/hooks/auto_commit_on_green.rhai
if event.exit_code == 0 && event.command.contains("cargo test") {
    glass.commit("tests passing");
}
```

### Execution Model

1. Scripts sorted by priority: confirmed > provisional > user-written
2. Each script runs in its own Rhai `Scope` — no shared state between scripts
3. If a script returns actions, they're collected into a `Vec<Action>`
4. Bridge executes all actions after all scripts for that hook complete
5. If a script fails (timeout, error), it's logged and skipped — other scripts still run

### Return Values

- Nothing — side-effect free observation
- `Action` or `Vec<Action>` — requests for the bridge to execute
- For `SnapshotBefore`: boolean to filter (true = proceed, false = skip). Aggregation rule: any script returning `false` vetoes the snapshot (AND logic). Only `confirmed` and `user` origin scripts can veto; `provisional` scripts can observe but not filter.
- For `McpRequest`: a response value (intercept/extend MCP tools). Priority is reversed for this hook: user-written > confirmed > provisional. First script to return a response wins; subsequent scripts are skipped. This ensures users always have final say over MCP interception.

## Action API

The curated set of operations scripts can request. The bridge executes these against real Glass state.

```rust
enum Action {
    // Git
    Commit { message: String },
    IsolateCommit { files: Vec<String>, message: String },
    RevertFiles { files: Vec<String> },

    // Config
    SetConfig { key: String, value: ConfigValue },

    // Snapshots
    ForceSnapshot { paths: Vec<String> },
    SetSnapshotPolicy { pattern: String, enabled: bool },

    // History
    TagCommand { command_id: String, tags: Vec<String> },

    // Orchestrator
    InjectPromptHint { text: String },
    TriggerCheckpoint { reason: String },
    ExtendSilence { extra_secs: u32 },
    BlockIteration { message: String, max_iterations: u32 },  // auto-clears after max_iterations (default 3, matches DEPENDENCY_BLOCK_MAX_ITERATIONS)

    // Scripts
    EnableScript { name: String },
    DisableScript { name: String },

    // MCP
    RegisterTool { name: String, description: String, schema: String, handler_script: String },
    UnregisterTool { name: String },

    // Notifications
    Log { level: LogLevel, message: String },
    Notify { message: String },
}
```

### The `glass` Object

Exposed to Rhai scripts, provides read-only context plus action submission:

```rust
// Read-only context (synchronous, from cached state)
glass.config("silence_timeout_secs")
glass.cwd()
glass.git_branch()
glass.git_dirty_files()
glass.recent_commands(n)
glass.active_rules()
glass.script_status("name")

// Action submission (queued for post-script execution)
glass.commit("message")
glass.set_config("key", value)
glass.log("info", "message")
glass.notify("message")
glass.force_snapshot(paths)
glass.inject_prompt_hint("text")
glass.register_tool(name, description, schema, handler)
// ... one method per Action variant
```

Read-only methods execute immediately against a snapshot taken once per hook invocation (not per script). All scripts on the same hook share the same snapshot. This prevents N SQLite queries for N scripts on a single `CommandComplete` event. The bridge builds the snapshot before the first script runs, then passes it to each script's `glass` object.

Action methods queue actions for the bridge to execute after all scripts for that hook complete.

### Safety Boundary — What Scripts Cannot Do

- No filesystem read/write (except through snapshot/git actions)
- No network access
- No process spawning
- No direct PTY write (scripts cannot type into the terminal)
- No renderer mutation (scripts cannot alter the UI directly)
- No deleting history or snapshots

## Script Lifecycle & Self-Improvement

### How Scripts Are Generated

Tiers 1-3 remain unchanged in `glass_feedback`. Tier 4 extends the existing feedback flow:

1. `glass_feedback::on_run_end()` runs as today — returns `FeedbackResult` with findings and `llm_prompt`
2. `FeedbackResult` gains a new field: `script_prompt: Option<String>` — built when Tier 1-3 findings alone seem insufficient (high waste/stuck rates with no matching detector)
3. The bridge calls `glass_scripting::generate_from_prompt(script_prompt, hook_points, action_api_ref)` which spawns an ephemeral agent (same pattern as feedback LLM)
4. LLM returns `SCRIPT:` block (or `FINDING:` blocks if Tiers 1-3 suffice)
5. `glass_scripting` saves the script + manifest to `~/.glass/scripts/feedback/` with `status = "provisional"`
6. Loaded on next run via `glass_scripting::load_scripts()`

The boundary: `glass_feedback` decides *whether* a script is needed and provides the prompt. `glass_scripting` handles generation, storage, and lifecycle. Neither crate depends on the other — the bridge coordinates them.

### Status Lifecycle

Mirrors the existing rule lifecycle:

```
LLM generates script
    -> Provisional (runs for one cycle)
        | next run improved/neutral
      Confirmed (permanent, runs every session)
        | no triggers for N runs
      Stale -> (re-triggered) -> Confirmed
        | no triggers for M more runs
      Archived

      Provisional -> (regression detected) -> Rejected -> Archived
```

Status is tracked in the companion `.toml` manifest, not by moving files between directories. Archived scripts stay in their original directory with `status = "archived"` in the manifest — the loader skips them. This matches how `glass_feedback` tracks rule status in `rules.toml` rather than moving files, and avoids file-watcher race conditions from renames.

### Validation

Same mechanism as rules. The feedback loop tracks metrics per run (revert_rate, stuck_rate, waste_rate, checkpoint_rate). When a provisional script is active:

1. Run completes — compare metrics to previous baseline
2. Improved or neutral — promote to confirmed
3. Regressed — reject, archive, restore previous behavior
4. Script errored 3 consecutive times — auto-rejected regardless of metrics

### User-Written Scripts

Users drop `.rhai` + `.toml` manifest pairs into `~/.glass/scripts/hooks/` or `~/.glass/scripts/tools/` with `origin = "user"` in the manifest. These skip the provisional/confirmed lifecycle — always active, user-managed.

### Script Versioning

Each manifest includes a `version` field (script revision) and an `api_version` field (Glass scripting API version). When the feedback loop modifies an existing script, it increments `version` in the manifest, saves the old `.rhai` file as `<name>.v<N>.rhai.bak` (where N is the previous version number), and writes the new `.rhai` file. Only the immediately prior version is kept — older backups are deleted. This avoids embedding script source in TOML and keeps diffing simple.

The `api_version` field enables forward compatibility: if the Glass scripting API changes in a future release, the engine can provide backward-compatible shims for older scripts or surface clear error messages ("script X requires api_version 2, current is 1").

## Dynamic MCP Tools

### Script-Defined Tools

Scripts with `type = "mcp_tool"` in their manifest are registered as MCP tools:

```toml
# ~/.glass/scripts/tools/recent_deploys.toml
name = "glass_recent_deploys"
description = "Returns recent deployment commands for the current project"
type = "mcp_tool"
status = "confirmed"
origin = "feedback"
api_version = 1

[params]
limit = { type = "number", required = false, default = 10 }
```

```rhai
// ~/.glass/scripts/tools/recent_deploys.rhai
let limit = params.limit ?? 10;
let cmds = glass.recent_commands(100);
let deploys = [];

for cmd in cmds {
    if cmd.command.contains("deploy") || cmd.command.contains("kubectl apply") {
        deploys.push(cmd);
        if deploys.len() >= limit { break; }
    }
}

deploys
```

### Registration Flow

The MCP server runs as a separate process (`glass mcp serve`) using rmcp's static `#[tool_handler]` dispatch. Dynamic tools cannot be added to the rmcp macro-generated dispatch table at runtime. Instead, dynamic tools are routed through the existing IPC channel between the MCP server and the Glass GUI binary:

1. `ScriptEngine::load_scripts()` finds manifests with `type = "mcp_tool"`
2. Parses manifest — generates JSON schema from `params` field
3. Registers in `dynamic_tools: HashMap<String, ScriptToolDef>` in the bridge
4. The MCP server has a single static tool `glass_script_tool` that accepts `{ "tool_name": "...", "params": {...} }`
5. MCP server forwards the request via IPC to the Glass binary
6. Bridge looks up `tool_name` in `dynamic_tools`, runs the Rhai script, returns result via IPC
7. MCP server returns the result as `CallToolResult`

The MCP server also exposes `glass_list_script_tools` which returns the dynamic tool registry (names, descriptions, schemas) so AI agents can discover available script-defined tools.

### Tool Lifecycle

- Provisional tools are registered but not advertised in tool listings
- Confirmed tools appear in the full MCP tool list
- Tool scripts follow the same 3-strikes rejection rule
- User-written tools (`origin: user`) are always active and listed

### Feedback-Generated Tools

The LLM prompt includes: "If you notice repeated query patterns, write an MCP tool script so agents can call it directly." Generated tools enter as provisional and follow the standard lifecycle.

## Profile Export/Import

### Export

```bash
glass profile export rust-backend
```

Produces `rust-backend.glassprofile` (tar.gz):

```
rust-backend.glassprofile/
  profile.toml              # metadata manifest
  rules.toml                # confirmed rules only
  scripts/
    hooks/                  # confirmed hook scripts
    tools/                  # confirmed MCP tool scripts
```

### Manifest

```toml
[profile]
name = "rust-backend"
glass_version = "3.1.0"
rhai_version = "1.19"       # resolved from Cargo.toml at build time via env!()
created = "2026-03-18"
runs_validated = 47
tech_stack = ["rust", "cargo", "git"]

[stats]
rules_count = 12
hook_scripts_count = 5
mcp_tools_count = 2
```

### Import

```bash
glass profile import rust-backend.glassprofile
```

1. Validate glass_version compatibility
2. Copy rules into `rules.toml` with status downgraded to `provisional`
3. Copy scripts into `~/.glass/scripts/` with status downgraded to `provisional`
4. Log summary of what was imported
5. Next orchestrator run validates everything against user's workflow

### Conflict Handling

- Rule/script with same `id` as existing one — skip, keep local version
- User prompted if imported script would shadow a user-written one

Import is a one-time seed. After import, the local feedback loop owns everything and diverges independently.

### What's Excluded From Export

- Provisional/rejected rules and scripts (unvalidated)
- Archived rules and scripts (failed experiments)
- `run-metrics.toml` (personal run history)
- `iterations.tsv` (session-specific logs)
- Full `config.toml` (only tuned deltas, not personal preferences)

## Sandboxing & Limits

### Rhai Engine Configuration

```rust
let mut engine = Engine::new();
engine.disable_symbol("eval");
engine.set_max_expr_depths(64, 32);
engine.set_max_operations(100_000);       // primary computation bound
engine.set_max_string_size(1_048_576);    // 1MB per string
engine.set_max_array_size(10_000);        // per array
engine.set_max_map_size(10_000);          // per map
```

**Memory:** Rhai does not provide a total heap memory limit. Memory is bounded indirectly through `max_operations` (limits computation that could allocate), `max_string_size`, `max_array_size`, and `max_map_size` (limit individual data structures). These combined prevent unbounded memory growth in practice.

**Timeouts:** Rhai does not support wall-clock timeouts natively. `max_operations` is the primary execution bound (counts AST operations, deterministic). As an additional safety net, the bridge runs each script via `tokio::spawn_blocking` with a `tokio::time::timeout` wrapper. If the wall-clock timeout fires before `max_operations` is hit, the bridge abandons the task handle. The background thread terminates when Rhai's operation limit is eventually reached — the thread is not killed instantly but cannot run indefinitely.

### Configurable Limits

```toml
[scripting]
enabled = true
max_operations = 100000       # per script, hard ceiling: 1000000
max_timeout_ms = 2000         # wall-clock safety net, hard ceiling: 10000
max_scripts_per_hook = 10     # hard ceiling: 25
max_total_scripts = 100       # hard ceiling: 500
max_mcp_tools = 20            # hard ceiling: 50
```

Users can adjust values up to the hard ceilings. Ceilings are compiled constants.

### Failure Handling

| Failure | Response |
|---|---|
| Exceeds max_operations | Rhai returns error, log, increment failure count |
| Exceeds wall-clock timeout | Bridge abandons task, log, increment failure count |
| Runtime error | Log error + stack trace, increment failure count |
| 3 consecutive failures | Auto-reject, archive, notify user |
| Provisional script causes run regression | Reject alongside provisional rules |

### Isolation Guarantees

- Each script gets its own Rhai `Scope` — no shared mutable state
- The `glass` object is rebuilt fresh per invocation with current read-only state
- Actions are queued, not executed during script runtime — scripts cannot observe their own side effects
- Scripts cannot import other scripts or load files from disk
- Scripts cannot call `eval` or dynamically construct code

## File Layout

```
~/.glass/
  scripts/
    hooks/                    # event hook scripts
      auto_commit_on_green.toml   # manifest (status, hooks, origin, api_version)
      auto_commit_on_green.rhai   # script source
      bump_snapshot_on_rm.toml
      bump_snapshot_on_rm.rhai
    tools/                    # MCP tool scripts
      recent_deploys.toml
      recent_deploys.rhai
    feedback/                 # auto-generated by feedback loop
      auto_generated_001.toml     # status = "provisional" | "confirmed" | "archived"
      auto_generated_001.rhai
  config.toml                # includes [scripting] section
  rules.toml                 # existing feedback rules (unchanged)
  global-rules.toml           # existing global rules (unchanged)

<project>/.glass/
  scripts/                    # project-scoped scripts (same structure)
    hooks/
    tools/
    feedback/
```

Archived scripts stay in their original directory with `status = "archived"` in the manifest. The loader skips them. No separate `archived/` directory needed.

Project-scoped scripts in `<project>/.glass/scripts/` take precedence over global scripts in `~/.glass/scripts/` when both have the same name.

## Config Reference

```toml
[scripting]
enabled = true                # master switch
max_operations = 100000       # per script operation limit (hard ceiling: 1000000)
max_timeout_ms = 2000         # wall-clock safety net (hard ceiling: 10000)
max_scripts_per_hook = 10     # per hook point (hard ceiling: 25)
max_total_scripts = 100       # across all hooks (hard ceiling: 500)
max_mcp_tools = 20            # dynamic MCP tools (hard ceiling: 50)
script_generation = true      # allow feedback loop to generate Tier 4 scripts
```

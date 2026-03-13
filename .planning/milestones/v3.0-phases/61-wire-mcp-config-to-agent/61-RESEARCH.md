# Phase 61: Wire MCP Config to Agent Subprocess - Research

**Researched:** 2026-03-13
**Domain:** Agent subprocess MCP wiring, activity stream shutdown drain
**Confidence:** HIGH

## Summary

Phase 61 fixes three targeted integration bugs identified in the v3.0 milestone audit. The core issue is that `build_agent_command_args` always emits `--mcp-config` followed by an empty string. The call site in `try_spawn_agent` (main.rs:738) passes `""` as the mcp_config_path, and the loop at lines 743-746 skips empty args -- but `--mcp-config` itself (a non-empty string) passes through as a dangling flag with no value. The Claude CLI then receives an invalid `--mcp-config` flag, preventing MCP tool discovery.

The fix requires: (1) writing a valid MCP config JSON file (`~/.glass/agent-mcp.json`) pointing to `glass mcp serve` before spawning the agent, (2) updating `build_agent_command_args` to conditionally omit `--mcp-config` when the path is empty (defensive guard), and (3) calling `flush_collapsed()` on the activity filter during agent shutdown so the last collapsed event is not silently dropped.

**Primary recommendation:** Write MCP config JSON with `std::env::current_exe()` resolved path + `["mcp", "serve"]` args, pass that path to `build_agent_command_args`, add empty-path guard in the function, and call `flush_collapsed` + send before `agent_runtime = None`.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| AGTR-03 | Three autonomy modes: Watch, Assist, Autonomous | Already implemented (agent_runtime.rs AgentMode enum). This phase ensures MCP tools are reachable from all modes. |
| SOIM-01 | glass_query tool returns structured output | Tool exists and passes tests (Phase 53). This phase wires runtime access so agent subprocess can invoke it. |
| SOIM-02 | glass_query_trend detects regressions | Tool exists and passes tests (Phase 53). Same wiring fix needed. |
| SOIM-03 | glass_query_drill expands record detail | Tool exists and passes tests (Phase 53). Same wiring fix needed. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| serde_json | workspace | Write MCP config JSON file | Already in workspace deps, used extensively |
| std::env::current_exe | stdlib | Resolve Glass binary path for MCP server command | Cross-platform, no external dep |
| dirs | workspace | Resolve ~/.glass directory | Already used in try_spawn_agent |

### Supporting
No new dependencies needed. All required functionality exists in the codebase.

## Architecture Patterns

### MCP Config JSON Format (Claude CLI)

The Claude CLI `--mcp-config` flag expects a JSON file with this structure:

```json
{
  "mcpServers": {
    "glass": {
      "command": "/absolute/path/to/glass",
      "args": ["mcp", "serve"]
    }
  }
}
```

**Source:** Claude CLI documentation and MCP JSON configuration standard. The `command` field must be an absolute path. The `args` array passes subcommand arguments. The `env` field is optional.

### Relevant Code Locations

| File | Lines | What |
|------|-------|------|
| `src/main.rs` | 674-970 | `try_spawn_agent()` -- full agent spawn flow |
| `src/main.rs` | 734-747 | Bug: empty mcp_config_path + dangling --mcp-config |
| `src/main.rs` | 301-330 | `Drop for AgentRuntime` -- shutdown/kill flow |
| `src/main.rs` | 3895-3953 | ConfigReloaded handler -- drops old runtime, respawns |
| `src/main.rs` | 4210-4265 | AgentCrashed handler -- restart with backoff |
| `crates/glass_core/src/agent_runtime.rs` | 360-387 | `build_agent_command_args` -- the function to fix |
| `crates/glass_core/src/activity_stream.rs` | 190-199 | `flush_collapsed()` -- exists but never called |
| `src/main.rs` | 4060-4083 | Activity filter process + send to channel |
| `src/main.rs` | 5049-5064 | `glass mcp serve` CLI handler -- confirms subcommand |

### Pattern 1: Write MCP Config Before Spawn

**What:** Create `~/.glass/agent-mcp.json` with the resolved Glass binary path, then pass its path to `build_agent_command_args`.

**When to use:** In `try_spawn_agent()`, after writing the system prompt and before building command args.

**Example:**
```rust
// Resolve Glass binary path for MCP server
let mcp_config_path = glass_dir.join("agent-mcp.json");
let glass_exe = std::env::current_exe()
    .ok()
    .and_then(|p| p.to_str().map(|s| s.to_string()));

if let Some(ref exe_path) = glass_exe {
    let mcp_config = serde_json::json!({
        "mcpServers": {
            "glass": {
                "command": exe_path,
                "args": ["mcp", "serve"]
            }
        }
    });
    if let Err(e) = std::fs::write(&mcp_config_path, mcp_config.to_string()) {
        tracing::warn!("AgentRuntime: failed to write MCP config: {}", e);
    }
}

let mcp_path_str = if glass_exe.is_some() && mcp_config_path.exists() {
    mcp_config_path.to_string_lossy().to_string()
} else {
    String::new()
};
```

### Pattern 2: Defensive Guard in build_agent_command_args

**What:** Skip `--mcp-config` and its value when `mcp_config_path` is empty.

**Example:**
```rust
pub fn build_agent_command_args(
    config: &AgentRuntimeConfig,
    prompt_path: &str,
    mcp_config_path: &str,
) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--input-format".to_string(),
        "stream-json".to_string(),
        "--system-prompt-file".to_string(),
        prompt_path.to_string(),
    ];
    if !mcp_config_path.is_empty() {
        args.push("--mcp-config".to_string());
        args.push(mcp_config_path.to_string());
    }
    args.push("--allowedTools".to_string());
    args.push(config.allowed_tools.clone());
    args.push("--dangerously-skip-permissions".to_string());
    args
}
```

### Pattern 3: flush_collapsed on Agent Shutdown

**What:** Before dropping the agent runtime (setting `self.agent_runtime = None`), call `flush_collapsed()` on `self.activity_filter` and send any resulting event to the channel.

**Where:** Three shutdown sites in main.rs:
1. `ConfigReloaded` handler (line 3903): `self.agent_runtime = None`
2. `AgentCrashed` handler (line 4263): `self.agent_runtime = None` (only when not restarting)
3. `CloseRequested` handler (line 1269): window close triggers Drop

**Example:**
```rust
// Before agent_runtime = None:
if let Some(event) = self.activity_filter.flush_collapsed() {
    if let Some(tx) = &self.activity_stream_tx {
        let _ = tx.try_send(event);
    }
}
self.agent_runtime = None;
```

### Anti-Patterns to Avoid
- **Passing empty string to --mcp-config:** Creates a dangling flag. Always guard with is_empty() check.
- **Relative paths in MCP config command field:** Claude CLI may not resolve them correctly. Always use absolute path from `std::env::current_exe()`.
- **Blocking on flush_collapsed:** The function is synchronous and fast. Do not await or spawn a thread.
- **Forgetting to update the existing test:** `build_args_includes_required_flags` test passes a non-empty path. Add a new test for the empty-path case.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| MCP config format | Custom JSON structure | Standard `mcpServers` format | Claude CLI expects this exact schema |
| Binary path resolution | Hardcoded paths | `std::env::current_exe()` | Works across installed and dev layouts |
| JSON serialization | Manual string formatting | `serde_json::json!` macro | Already in deps, avoids escaping bugs |

## Common Pitfalls

### Pitfall 1: Windows Path Backslashes in JSON
**What goes wrong:** `std::env::current_exe()` on Windows returns paths with backslashes. JSON requires forward slashes or escaped backslashes.
**Why it happens:** serde_json::to_string correctly escapes backslashes, but `serde_json::json!` with `.to_string()` also handles this. However, if using format! manually, backslashes would be unescaped.
**How to avoid:** Use `serde_json::json!` macro which handles escaping, or convert to forward slashes.
**Warning signs:** Agent fails to start on Windows with "command not found" even though the binary exists.

### Pitfall 2: current_exe() Returns Err
**What goes wrong:** On some platforms or under certain conditions, `current_exe()` can fail.
**Why it happens:** Platform limitations, /proc not mounted on Linux, etc.
**How to avoid:** Handle the `Err` case gracefully -- log warning and skip MCP config (agent spawns without MCP tools, same as current behavior). Never crash.

### Pitfall 3: Race Between Config Write and Subprocess Read
**What goes wrong:** Agent subprocess starts before the MCP config file is fully written.
**Why it happens:** `std::fs::write` is atomic on most platforms (write-then-rename), but theoretically possible.
**How to avoid:** Write the config file BEFORE building command args and spawning. The current flow already guarantees this ordering.

### Pitfall 4: Forgetting flush_collapsed at All Three Shutdown Sites
**What goes wrong:** Only fixing one of the three `agent_runtime = None` sites leaves the bug partially unfixed.
**Why it happens:** There are 3 distinct code paths that drop the agent runtime: ConfigReloaded, AgentCrashed (when max restarts exceeded), and window close.
**How to avoid:** Extract a helper method `fn shutdown_agent(&mut self)` or ensure all three sites call flush_collapsed.

### Pitfall 5: Empty Args Loop Still Present
**What goes wrong:** The loop at lines 743-746 that skips empty args is a latent bug magnet. Even after fixing build_agent_command_args, the loop should be simplified.
**How to avoid:** After fixing build_agent_command_args to never emit empty args, remove the empty-string filter from the loop and use `cmd.args(&args)` directly.

## Code Examples

### Current Buggy Flow (src/main.rs:734-747)
```rust
// Bug: passes "" which creates dangling --mcp-config flag
let args = glass_core::agent_runtime::build_agent_command_args(
    &config,
    &prompt_path.to_string_lossy(),
    "", // empty = no mcp config  <-- BUG
);

let mut cmd = Command::new("claude");
for arg in &args {
    if !arg.is_empty() {  // skips "" but --mcp-config already passed
        cmd.arg(arg);
    }
}
```

### Fixed Flow (what the plan should implement)
```rust
// Write MCP config JSON pointing to glass binary
let mcp_config_path = glass_dir.join("agent-mcp.json");
let mcp_path_str = match std::env::current_exe() {
    Ok(exe) => {
        let config_json = serde_json::json!({
            "mcpServers": {
                "glass": {
                    "command": exe.to_string_lossy(),
                    "args": ["mcp", "serve"]
                }
            }
        });
        match std::fs::write(&mcp_config_path, config_json.to_string()) {
            Ok(_) => mcp_config_path.to_string_lossy().to_string(),
            Err(e) => {
                tracing::warn!("AgentRuntime: failed to write MCP config: {}", e);
                String::new()
            }
        }
    }
    Err(e) => {
        tracing::warn!("AgentRuntime: failed to resolve exe path for MCP config: {}", e);
        String::new()
    }
};

let args = glass_core::agent_runtime::build_agent_command_args(
    &config,
    &prompt_path.to_string_lossy(),
    &mcp_path_str,
);

// Simplified: no need for empty-string filter since build_agent_command_args is now correct
let mut cmd = Command::new("claude");
cmd.args(&args);
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Skip MCP config (Phase 56 deferral) | Write real config JSON | Phase 61 | Agent can now call glass_query/trend/drill |
| Empty string sentinel for "no config" | Conditional flag emission | Phase 61 | No more dangling CLI flags |

## Open Questions

1. **allowed_tools list completeness**
   - What we know: Default is `"glass_query,glass_context,Bash,Read"`. Phase 53 added glass_query_trend and glass_query_drill.
   - What's unclear: Should glass_query_trend and glass_query_drill be added to the default allowed_tools list?
   - Recommendation: Add them to the default. They exist and are useful for the agent. This is a one-line change in `AgentRuntimeConfig::default()`.

2. **MCP config file overwrite on every spawn**
   - What we know: The config is written fresh on every `try_spawn_agent` call.
   - What's unclear: Is it wasteful to rewrite on every restart?
   - Recommendation: Acceptable. The file is tiny (<200 bytes) and ensures the exe path is always current. No caching needed.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in Rust test framework) |
| Config file | Cargo.toml workspace |
| Quick run command | `cargo test -p glass_core --lib agent_runtime` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| AGTR-03 | Autonomy modes gate severity filtering | unit | `cargo test -p glass_core --lib agent_runtime::tests::mode_ -x` | Yes (existing) |
| SOIM-01 | glass_query returns structured output | unit | `cargo test -p glass_mcp --lib -x` | Yes (existing) |
| SOIM-02 | glass_query_trend detects regressions | unit | `cargo test -p glass_mcp --lib -x` | Yes (existing) |
| SOIM-03 | glass_query_drill expands detail | unit | `cargo test -p glass_mcp --lib -x` | Yes (existing) |
| SC-1 | MCP config JSON written with valid path | unit | `cargo test -p glass_core --lib agent_runtime::tests::build_args_ -x` | Wave 0 |
| SC-2 | build_agent_command_args omits flag when empty | unit | `cargo test -p glass_core --lib agent_runtime::tests::build_args_omits_mcp_when_empty -x` | Wave 0 |
| SC-3 | flush_collapsed returns last event | unit | `cargo test -p glass_core --lib activity_stream -x` | Yes (existing) |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_core --lib agent_runtime && cargo clippy --workspace -- -D warnings`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `build_args_omits_mcp_config_when_empty` test -- covers SC-2 (empty path guard)
- [ ] `build_args_includes_mcp_config_when_present` test -- existing test covers this but should be renamed/clarified

## Sources

### Primary (HIGH confidence)
- `crates/glass_core/src/agent_runtime.rs` - Read directly. Contains `build_agent_command_args`, `AgentRuntimeConfig`, all tests.
- `crates/glass_core/src/activity_stream.rs` - Read directly. Contains `flush_collapsed()` implementation.
- `src/main.rs` lines 674-970 - Read directly. Contains `try_spawn_agent`, the bug at line 738, Drop impl.
- `src/main.rs` lines 3895-3953, 4210-4265 - Read directly. ConfigReloaded and AgentCrashed handlers.
- `.planning/v3.0-MILESTONE-AUDIT.md` - Read directly. Documents the integration gap and tech debt.

### Secondary (MEDIUM confidence)
- [Claude CLI --mcp-config JSON format](https://gofastmcp.com/integrations/mcp-json-configuration) - Standard MCP JSON configuration schema
- [Claude Code MCP settings](https://code.claude.com/docs/en/settings) - Claude Code configuration documentation

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - No new dependencies, all code read directly
- Architecture: HIGH - Bug is clearly identified with exact line numbers, fix pattern is straightforward
- Pitfalls: HIGH - Windows path escaping and multi-site shutdown are real concerns verified from code

**Research date:** 2026-03-13
**Valid until:** 2026-04-13 (stable domain -- MCP JSON format is standardized)

# Phase 20: Config Gate + Dead Code Cleanup - Research

**Researched:** 2026-03-06
**Domain:** Rust config gating, shell integration env vars, dead code removal
**Confidence:** HIGH

## Summary

Phase 20 closes two integration gaps identified in the v1.3 milestone audit: (1) `pipes.enabled=false` does not fully gate pipe capture behavior -- it only skips temp file reading in main.rs while shell scripts still rewrite pipelines, BlockManager still accumulates empty CapturedStage entries, and empty PipeStageRow records get persisted to the DB; (2) `classify_pipeline()`, `has_opt_out()`, and `PipelineClassification` are exported from `glass_pipes` but never imported by any consuming crate -- shell scripts implement their own opt-out logic.

The fix is straightforward: add an env var `GLASS_PIPES_DISABLED=1` to the PTY spawn when `pipes.enabled=false`, check it in both shell scripts before rewriting, skip PipelineStart/PipelineStage processing in main.rs when disabled, skip `insert_pipe_stages` when disabled, and remove the orphaned classify module and type from `glass_pipes`.

**Primary recommendation:** Gate at three layers (shell env var, main.rs event conversion, DB persistence) then delete classify.rs entirely and clean up PipelineClassification from types.rs and parser.rs.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CONF-01 | `pipes.enabled=false` fully gates all pipe capture behavior | Shell env var gate, main.rs PipelineStart/PipelineStage skip, insert_pipe_stages skip -- three-layer approach identified with exact code locations |
| PIPE-02 | Dead code removal of orphaned classify functions | classify.rs deletion, PipelineClassification removal from types.rs, Pipeline struct simplification, lib.rs export cleanup |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| glass_pipes | local crate | Pipe parsing and buffer management | Project crate being modified |
| glass_terminal | local crate | PTY spawn, BlockManager, OscScanner | Env var injection point and event handling |
| glass_core | local crate | Config types (PipesSection) | Already has pipes.enabled field |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| glass_history | local crate | DB persistence (insert_pipe_stages) | Skip call when pipes disabled |

## Architecture Patterns

### Current Config Gate Flow (BROKEN)
```
config.toml [pipes] enabled=false
  -> main.rs reads config.pipes.enabled
  -> ONLY gates temp file reading (lines 823-852)
  -> Shell scripts: NO gate (always rewrite)
  -> BlockManager: NO gate (always accumulates CapturedStage)
  -> DB persistence: NO gate (empty stages persisted)
```

### Target Config Gate Flow (FIXED)
```
config.toml [pipes] enabled=false
  -> PTY spawn: set GLASS_PIPES_DISABLED=1 env var
  -> Shell scripts: check env var, skip rewriting if set
  -> main.rs: skip PipelineStart/PipelineStage -> OscEvent conversion
  -> BlockManager: never receives pipeline events (no accumulation)
  -> DB persistence: no pipeline_stages means no insert_pipe_stages call
  -> Result: zero pipe-related side effects when disabled
```

### Dead Code Removal Scope
```
DELETE: crates/glass_pipes/src/classify.rs (entire file, ~216 lines)
MODIFY: crates/glass_pipes/src/lib.rs (remove pub mod classify, remove pub use)
MODIFY: crates/glass_pipes/src/types.rs (remove PipelineClassification struct + Default impl)
MODIFY: crates/glass_pipes/src/parser.rs (remove PipelineClassification import, use a simpler default for Pipeline.classification field)
MODIFY: crates/glass_pipes/src/types.rs (remove classification field from Pipeline struct OR replace with simpler type)
```

### Exact Code Locations for Config Gate

**1. PTY env var injection** -- `crates/glass_terminal/src/pty.rs` line 127:
```rust
// Current:
env: std::collections::HashMap::from([
    ("TERM".to_owned(), "xterm-256color".to_owned()),
    ("COLORTERM".to_owned(), "truecolor".to_owned()),
]),

// Need: spawn_pty must accept pipes_enabled param from main.rs
// and conditionally add ("GLASS_PIPES_DISABLED".to_owned(), "1".to_owned())
```

**2. Shell script gates:**

`shell-integration/glass.bash` line 212 (`__glass_accept_line`):
```bash
# Add at top of function:
[[ "$GLASS_PIPES_DISABLED" == "1" ]] && return
```

`shell-integration/glass.ps1` line 232 (Enter key handler):
```powershell
# Add before rewrite attempt:
if ($env:GLASS_PIPES_DISABLED -eq "1") { ... skip rewriting ... }
```

**3. main.rs event conversion** -- line 186-191 (shell_event_to_osc):
```rust
// Skip PipelineStart/PipelineStage conversion when pipes disabled
// Either: filter in the match arm, or skip the entire block in the event loop
// The event loop at line ~800 already has pipes_enabled check for temp file reading
// Extend that check to also skip the handle_event call for pipeline events
```

**4. DB persistence** -- main.rs line 1008-1044:
```rust
// Already gated by `!block.pipeline_stages.is_empty()` check at line 1009
// If BlockManager never receives PipelineStart/PipelineStage, stages will be empty
// So this is implicitly gated by fix #3 above -- no additional change needed
```

### Pipeline Struct After Dead Code Removal

The `Pipeline` struct has a `classification: PipelineClassification` field (types.rs line 9). This is set to `PipelineClassification::default()` in `parse_pipeline()` (parser.rs line 139) and never modified by any consuming code. After removing `PipelineClassification`, either:
- Remove the `classification` field entirely from `Pipeline`
- The `Pipeline` struct is only used in main.rs line 942 to extract stage commands -- only `stages` field is accessed

**Recommendation:** Remove `classification` field from `Pipeline` struct entirely. The only consumer (main.rs:942) uses `.stages` only.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Env var gating in shells | Complex config file reading in shell scripts | Simple env var check (`GLASS_PIPES_DISABLED`) | Shell scripts can't read TOML; env var is the standard IPC mechanism between terminal and shell |

## Common Pitfalls

### Pitfall 1: Forgetting to pass config to PTY spawn
**What goes wrong:** `spawn_pty` currently takes no config parameter; adding env var requires plumbing `pipes.enabled` from `GlassConfig` through to the PTY spawn call.
**Why it happens:** The function signature at `pty.rs:106` doesn't accept config.
**How to avoid:** Add a `pipes_enabled: bool` parameter to `spawn_pty()` and update the call site in `main.rs:253`.

### Pitfall 2: Removing PipelineClassification breaks Pipeline struct
**What goes wrong:** `Pipeline` struct has `classification: PipelineClassification` field. Removing the type without updating the struct causes compile error.
**Why it happens:** The classification field was intended for future Rust-side use but was superseded by shell-side logic.
**How to avoid:** Remove the field from Pipeline AND remove the import in parser.rs AND update parse_pipeline() to not set classification.

### Pitfall 3: Test breakage in classify.rs
**What goes wrong:** classify.rs has 11 tests. Deleting the file removes them.
**Why it happens:** Tests are for dead code being removed.
**How to avoid:** This is intentional -- dead code tests should be removed with the dead code. Verify all OTHER tests still pass after deletion.

### Pitfall 4: Shell script gate placement
**What goes wrong:** Placing the env var check in the wrong function (e.g., only in `__glass_has_pipes`) could leave emission functions still running.
**Why it happens:** The bash integration has multiple functions: `__glass_has_pipes`, `__glass_tee_rewrite`, `__glass_accept_line`, `__glass_emit_stages`.
**How to avoid:** Gate at the top of `__glass_accept_line` (bash) and the Enter key handler (PowerShell) -- these are the single entry points for all pipeline rewriting.

### Pitfall 5: Gating too aggressively in main.rs
**What goes wrong:** If you skip ALL processing of PipelineStart/PipelineStage events in the event loop but shell scripts still emit OSC sequences (e.g., config changed mid-session), unprocessed OSC data could accumulate.
**Why it happens:** Shell scripts read env var at startup; config changes after spawn won't update shell behavior.
**How to avoid:** The env var is set at PTY spawn time, so shell and terminal are in sync. If concerned, also skip the events in the shell_event_to_osc conversion (line 186) which prevents them from reaching BlockManager at all.

## Code Examples

### Env var injection in spawn_pty
```rust
// crates/glass_terminal/src/pty.rs
pub fn spawn_pty(
    event_proxy: ...,
    proxy: ...,
    window_id: ...,
    shell: Option<&str>,
    max_output_capture_kb: u32,
    pipes_enabled: bool,  // NEW parameter
) -> ... {
    let mut env = std::collections::HashMap::from([
        ("TERM".to_owned(), "xterm-256color".to_owned()),
        ("COLORTERM".to_owned(), "truecolor".to_owned()),
    ]);
    if !pipes_enabled {
        env.insert("GLASS_PIPES_DISABLED".to_owned(), "1".to_owned());
    }
    let options = TtyOptions {
        shell: Some(Shell::new(shell_program, vec![])),
        working_directory: None,
        drain_on_exit: true,
        escape_args: false,
        env,
    };
    // ...
}
```

### Bash gate
```bash
# In __glass_accept_line, first line:
__glass_accept_line() {
    [[ "$GLASS_PIPES_DISABLED" == "1" ]] && return
    local cmd="$READLINE_LINE"
    # ... rest unchanged
}
```

### PowerShell gate
```powershell
# In Enter key handler, before rewrite:
Set-PSReadLineKeyHandler -Key Enter -ScriptBlock {
    $line = $null
    $cursor = $null
    [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)

    if ($env:GLASS_PIPES_DISABLED -ne "1") {
        $rewritten = __Glass-Rewrite-Pipeline $line
        if ($rewritten) {
            [Microsoft.PowerShell.PSConsoleReadLine]::Replace(0, $line.Length, $rewritten)
        }
    }

    [Microsoft.PowerShell.PSConsoleReadLine]::AcceptLine()
    [Console]::Write("$([char]0x1b)]133;C$([char]7)")
}
```

### main.rs event skip
```rust
// In the event loop, skip pipeline events when pipes disabled:
// Option A: Filter in shell_event_to_osc conversion
// Option B: Skip handling after conversion (simpler, localized change)

// At line ~800, extend existing pipes_enabled check:
let pipes_enabled = self.config.pipes.as_ref()
    .map(|p| p.enabled)
    .unwrap_or(true);

// Skip PipelineStart/PipelineStage entirely when disabled
if !pipes_enabled {
    if matches!(shell_event,
        ShellEvent::PipelineStart { .. } | ShellEvent::PipelineStage { .. }
    ) {
        continue; // or skip handle_event for this event
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Rust-side classify_pipeline | Shell-side TTY detection + --no-glass | Phase 16 (shell scripts) | classify.rs became dead code |
| pipes.enabled gates temp file read only | pipes.enabled must gate all pipe behavior | Phase 20 (this phase) | Full config gating |

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (Rust built-in) |
| Config file | Cargo.toml (workspace) |
| Quick run command | `cargo test -p glass_pipes` |
| Full suite command | `cargo test` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CONF-01a | GLASS_PIPES_DISABLED env var set when pipes.enabled=false | unit | `cargo test -p glass_terminal -- test_spawn_pty_pipes_disabled` | Wave 0 |
| CONF-01b | Bash script skips rewrite when GLASS_PIPES_DISABLED=1 | manual-only | Manual: set env var, run piped command, verify no tee rewriting | N/A |
| CONF-01c | PowerShell script skips rewrite when GLASS_PIPES_DISABLED=1 | manual-only | Manual: set env var, run piped command, verify no Tee-Object | N/A |
| CONF-01d | main.rs skips PipelineStart/PipelineStage when pipes disabled | integration | Verified by: no pipeline_stages in block when pipes disabled | Wave 0 |
| PIPE-02a | classify_pipeline removed, code compiles | build | `cargo build` | N/A (compiler) |
| PIPE-02b | All existing tests pass after dead code removal | unit | `cargo test` | Existing |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_pipes && cargo test -p glass_terminal`
- **Per wave merge:** `cargo test`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] No new test files needed -- validation is primarily: (a) existing tests still pass after deletion, (b) `cargo build` succeeds, (c) manual shell verification
- [ ] Optional: add a unit test for spawn_pty env var injection if function signature changes make it testable

## Sources

### Primary (HIGH confidence)
- Direct code reading of all affected files in the Glass repository
- `.planning/v1.3-MILESTONE-AUDIT.md` -- integration gaps section (lines 147-153)
- `crates/glass_pipes/src/classify.rs` -- dead code to remove
- `crates/glass_pipes/src/types.rs` -- PipelineClassification struct
- `crates/glass_terminal/src/pty.rs` -- PTY spawn with env vars
- `src/main.rs` -- event loop, pipes_enabled check, insert_pipe_stages call
- `shell-integration/glass.bash` -- pipeline rewriting entry point
- `shell-integration/glass.ps1` -- PowerShell pipeline rewriting

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - all code is in the local repository, fully inspected
- Architecture: HIGH - exact code locations and line numbers identified for all changes
- Pitfalls: HIGH - based on direct code reading, not hypothetical

**Research date:** 2026-03-06
**Valid until:** 2026-04-06 (stable codebase, no external dependencies)

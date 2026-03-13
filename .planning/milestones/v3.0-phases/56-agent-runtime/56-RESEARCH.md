# Phase 56: Agent Runtime - Research

**Researched:** 2026-03-13
**Domain:** Subprocess management, Claude CLI JSON wire protocol, process lifecycle, platform-safe orphan prevention, cost enforcement, autonomy modes
**Confidence:** HIGH

## Summary

Phase 56 is the core agent runtime: Glass spawns a background `claude -p --output-format stream-json` subprocess, feeds it `ActivityEvent` JSON lines on stdin, reads `AgentProposal` JSON lines from its stdout, and routes those proposals through the winit event loop as `AppEvent::AgentProposal`. Three autonomy modes (Watch/Assist/Autonomous) filter which events trigger the agent. A cooldown timer (30 s) and a max_budget_usd cap ($1.00) protect against runaway cost.

The subprocess is managed by a new `AgentRuntime` struct that lives in `src/main.rs` inside `Processor`, following the established `CoordinationPoller` and SOI spawn-blocking patterns. There is no new crate needed: `AgentRuntime` fits cleanly in `glass_core` or directly in `src/main.rs`. The activity stream receiver already lives at `Processor.activity_stream_rx` (left there by Phase 55 with `#[allow(dead_code)]`); Phase 56 `.take()`s it.

The hardest technical problems are (1) getting the Claude CLI JSON wire protocol right — specifically `--output-format stream-json` line format and how to inject activity context on stdin without breaking the protocol; (2) platform-safe orphan prevention on Windows (Job Objects via `windows-sys`) and Unix (`prctl(PR_SET_PDEATHSIG, SIGKILL)` in child pre-exec); and (3) correctly parsing `cost_usd` from the `result` message type to track cumulative API spend.

**Primary recommendation:** Use `std::process::Command` with `Stdio::piped()` for both stdin and stdout. Spawn a dedicated reader thread (same pattern as `glass_pty_loop`) that parses JSON lines from stdout and sends `AppEvent::AgentProposal` via `EventLoopProxy`. Use `SyncSender` for the stdin writer. Apply Windows Job Object + Unix prctl for orphan prevention. Track cumulative cost in `Processor` using the `cost_usd` field on `result` messages.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| AGTR-01 | Background Claude CLI process spawns with custom system prompt and MCP tool access | `claude -p --output-format stream-json --system-prompt-file <path> --mcp-config <path> --allowedTools "Bash,Read" --dangerously-skip-permissions` spawned via `std::process::Command`. System prompt injected via temp file. |
| AGTR-02 | Agent receives activity stream via stdin (JSON lines protocol) and outputs proposals via stdout | ActivityEvent JSON written line-by-line to child stdin. Child stdout parsed as NDJSON. Glass wraps activity in a synthetic `user` message matching the `--input-format stream-json` wire format. |
| AGTR-03 | Three autonomy modes: Watch (critical issues only), Assist (suggestions), Autonomous (proposes fixes) | Watch = Error severity only; Assist = Error + Warning; Autonomous = all severities. Mode-gated in the `AgentRuntime::should_send()` method before writing to stdin. |
| AGTR-04 | Agent process lifecycle managed: start, restart on crash, graceful shutdown on app exit | Reader thread detects EOF/error on stdout and sends `AppEvent::AgentCrashed`. Processor restarts after backoff. On app exit, stdin is dropped (EOF) and child is `wait()`ed with timeout then `kill()`. |
| AGTR-05 | Platform subprocess management: Windows Job Objects, Unix prctl for cleanup on crash | Windows: `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` via `windows-sys` (already in workspace). Unix: `CommandExt::pre_exec` with `libc::prctl(PR_SET_PDEATHSIG, SIGKILL)`. |
| AGTR-06 | Cooldown timer prevents proposal spam (configurable, default 30s) | `last_proposal_time: Option<Instant>` in `AgentRuntime`. Before writing to stdin, check `elapsed() < cooldown`. Config default 30 s from `AgentRuntimeConfig`. |
| AGTR-07 | max_budget_usd enforced (default $1.00) and status bar cost display | Parse `cost_usd` from `{"type":"result",...}` lines on stdout. Accumulate in `Processor.agent_cost_usd: f64`. Gate new proposals when accumulated >= max_budget_usd. Pass `agent_cost_usd` to `build_status_text` as new optional segment. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `std::process::Command` | stdlib | Spawn `claude` subprocess with stdin/stdout pipes | Established pattern; no external dep; cross-platform |
| `std::io::{BufReader, BufWriter, Write}` | stdlib | Line-buffered stdin/stdout I/O for JSON lines protocol | Buffered I/O prevents partial-line sends |
| `std::sync::mpsc` | stdlib | Thread-safe channel from reader thread to main thread | Matches PTY pattern in `glass_terminal/pty.rs` |
| `serde_json` | 1.0 (workspace) | Parse JSON lines from stdout; serialize ActivityEvent to JSON | Already in root `Cargo.toml` |
| `windows-sys` | 0.59 (workspace) | Windows Job Object APIs for orphan prevention | Already in workspace (`Win32_System_Console` feature already included) |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `libc` | not in workspace | Unix `prctl(PR_SET_PDEATHSIG)` in child pre-exec | Only on non-Windows; needs adding to workspace or using `#[cfg(unix)]` conditional |
| `tempfile` | 3 (dev-deps) | Create temp file for system prompt | Already in dev-deps; for production use `std::fs::write` to `~/.glass/agent-prompt.txt` |
| `tracing` | workspace | Log subprocess events, stdout lines, cost tracking | Project convention |

### Windows-sys Feature Gap

The workspace `windows-sys` currently enables `Win32_System_Console`. Job Objects require `Win32_System_JobObjects`. This feature must be added to the workspace dependency.

**Required features addition in `Cargo.toml`:**
```toml
windows-sys = { version = "0.59", features = [
    "Win32_System_Console",
    "Win32_System_JobObjects",   # new for Phase 56
    "Win32_Foundation",          # HANDLE, BOOL
    "Win32_Security",            # SECURITY_ATTRIBUTES (for CreateJobObjectW)
] }
```

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `std::process::Command` | `tokio::process::Command` | tokio process requires async runtime on the spawning thread; Processor is sync/winit; std::process is the correct choice |
| Line-buffered stdin writes | Raw byte writes | Line buffering guarantees complete JSON objects per write; raw writes risk partial lines |
| Job Objects (Windows) | `Child.kill()` on Drop | Drop-based kill only fires on graceful exit; crash leaves orphan; Job Objects handle crash scenario |
| Unix prctl | SIGTERM on Drop | Same issue — Drop only fires on graceful exit |

**Installation:** No new crate deps for core functionality. Only `windows-sys` feature additions needed.

## Architecture Patterns

### Recommended Module Structure

```
src/main.rs
├── Processor.agent_runtime: Option<AgentRuntime>
├── Processor.agent_cost_usd: f64           # cumulative spend
├── Processor.agent_proposals_paused: bool   # true when budget exceeded
└── AppEvent::AgentProposal { ... }          # new variant in glass_core/event.rs

crates/glass_core/src/
├── event.rs             # Add AppEvent::AgentProposal, AppEvent::AgentCrashed
└── activity_stream.rs   # Already complete (Phase 55)

crates/glass_renderer/src/
└── status_bar.rs        # Add agent_cost_text: Option<String> to StatusLabel
```

### Pattern 1: Claude CLI Invocation (AGTR-01)

**What:** Spawn `claude` as a child process with JSON output format, custom system prompt, and MCP access.
**When to use:** When agent.mode != Off and claude CLI is available on PATH.

```rust
// Source: std::process::Command — official Rust stdlib
use std::process::{Command, Stdio};

let mut child = Command::new("claude")
    .arg("-p")
    .arg("--output-format").arg("stream-json")
    .arg("--input-format").arg("stream-json")
    .arg("--system-prompt-file").arg(&prompt_path)
    .arg("--mcp-config").arg(&mcp_config_path)
    .arg("--allowedTools").arg("glass_query,glass_context,Bash,Read")
    .arg("--dangerously-skip-permissions")  // non-interactive, no prompts
    .arg("--max-budget-usd").arg(format!("{:.2}", max_budget_usd))
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())    // suppress STDERR noise on main terminal
    .spawn()?;
```

**Critical:** `--dangerously-skip-permissions` is mandatory for non-interactive background use. Without it, the agent process will hang waiting for user permission prompts on interactive tool calls.

### Pattern 2: stdin/stdout Wire Protocol (AGTR-02)

**What:** The `--input-format stream-json` mode expects JSON lines on stdin conforming to the Agent SDK message format. The Glass agent sends wrapped `user` messages containing the activity context.

**Wire format verified from official CLI docs:**

Input (stdin) — one JSON line per message:
```json
{"type":"user","message":{"role":"user","content":"Build failed: 3 errors in src/main.rs (command_id: 42). Use glass_query to investigate."}}
```

Output (stdout) — NDJSON stream with multiple message types:
```json
{"type":"system","subtype":"init","session_id":"<uuid>","tools":["glass_query",...]}
{"type":"assistant","message":{"id":"msg_xxx","content":[{"type":"text","text":"..."}],"usage":{...}}}
{"type":"result","subtype":"success","session_id":"<uuid>","cost_usd":0.00231,"num_turns":2}
```

**Key result message fields (confirmed from official docs):**
- `type`: "result"
- `subtype`: "success" | "error_max_turns" | "error_during_execution"
- `cost_usd`: float, cost of this query call in USD
- `num_turns`: integer turn count
- `session_id`: UUID string

**AgentProposal detection strategy:** Glass scans assistant messages for a structured JSON block in the text content. The system prompt instructs the agent to output proposals as:
```json
{"glass_proposal":{"action":"...","description":"...","severity":"...","command_id":42}}
```

Glass parses each `assistant` message text for `glass_proposal` JSON objects.

### Pattern 3: Reader Thread (AGTR-02, AGTR-07)

**What:** A named background thread reads stdout lines and sends them to the winit event loop.

```rust
// Source: pattern mirrors glass_terminal/pty.rs glass_pty_loop
use std::io::{BufRead, BufReader};
use winit::event_loop::EventLoopProxy;

let stdout = child.stdout.take().expect("stdout piped");
let proxy_clone = proxy.clone();

std::thread::Builder::new()
    .name("glass-agent-reader".into())
    .spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(json_line) => {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_line) {
                        // Route by type field
                        match val.get("type").and_then(|t| t.as_str()) {
                            Some("result") => {
                                let cost = val.get("cost_usd")
                                    .and_then(|c| c.as_f64())
                                    .unwrap_or(0.0);
                                let _ = proxy_clone.send_event(AppEvent::AgentQueryResult { cost_usd: cost });
                            }
                            Some("assistant") => {
                                // Extract glass_proposal JSON from text blocks
                                if let Some(proposal) = extract_proposal(&val) {
                                    let _ = proxy_clone.send_event(AppEvent::AgentProposal(proposal));
                                }
                            }
                            _ => {} // Ignore system, user messages
                        }
                    }
                }
                Err(_) => {
                    // EOF or error — agent process exited
                    let _ = proxy_clone.send_event(AppEvent::AgentCrashed);
                    break;
                }
            }
        }
    })
    .expect("Failed to spawn agent reader thread");
```

### Pattern 4: Stdin Writer (AGTR-02, AGTR-06)

**What:** The activity stream receiver is drained by a writer thread that applies cooldown gating and mode filtering before writing JSON lines to child stdin.

```rust
use std::io::{BufWriter, Write};

let stdin = child.stdin.take().expect("stdin piped");
let mut writer = BufWriter::new(stdin);
let rx = self.activity_stream_rx.take().expect("rx from Phase 55");

std::thread::Builder::new()
    .name("glass-agent-writer".into())
    .spawn(move || {
        let mut last_proposal: Option<std::time::Instant> = None;

        for event in rx.iter() {  // blocks until sender drops or channel closes
            // Apply mode filter
            if !should_send_in_mode(mode, &event.severity) {
                continue;
            }
            // Apply cooldown
            if let Some(t) = last_proposal {
                if t.elapsed() < cooldown {
                    continue;
                }
            }
            // Write JSON line
            let msg = format_activity_as_user_message(&event);
            if writeln!(writer, "{}", msg).is_err() {
                break; // Claude process closed stdin
            }
            let _ = writer.flush();
            last_proposal = Some(std::time::Instant::now());
        }
        // rx dropped: channel closed, writer drops = EOF on child stdin
    })
    .expect("Failed to spawn agent writer thread");
```

### Pattern 5: Windows Job Object (AGTR-05)

**What:** Assign the Glass process to a Windows Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Any child processes (including claude subprocess) are automatically killed when the Job Object handle is closed (which happens on any exit including crash).

**Verified from:** Cargo's `src/cargo/util/job.rs` (stable Rust ecosystem reference implementation)

```rust
// Source: Cargo's util/job.rs — platform-proven pattern
#[cfg(target_os = "windows")]
fn setup_job_object() -> Option<windows_sys::Win32::Foundation::HANDLE> {
    use windows_sys::Win32::System::JobObjects::*;
    use windows_sys::Win32::Foundation::*;
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job == 0 {
            return None;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of_val(&info) as u32,
        );
        let me = GetCurrentProcess();
        AssignProcessToJobObject(job, me);
        Some(job)
        // Keep handle alive in a static or Processor field — closing it kills children
    }
}
```

**IMPORTANT:** Store the job handle (e.g., in `Processor.job_object: Option<RawHandle>`) for the lifetime of the process. Dropping/closing the handle triggers the kill-on-close policy.

### Pattern 6: Unix prctl for Orphan Prevention (AGTR-05)

**What:** Use `pre_exec` to call `prctl(PR_SET_PDEATHSIG, SIGKILL)` in the child process immediately after fork but before exec. This sends SIGKILL to the child if the parent dies.

```rust
// Source: std::os::unix::process::CommandExt — official Rust trait
#[cfg(unix)]
fn add_pdeathsig(cmd: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong, 0, 0, 0);
            Ok(())
        });
    }
}
```

**Warning:** There is a known interaction between `tokio` worker thread reaping and `prctl(PR_SET_PDEATHSIG)` when spawned from a non-main thread (source: "Tokio + prctl = nasty bug" blog post, Feb 2025). Glass uses tokio for MCP but the agent subprocess spawn happens on the winit main thread — this is the safe path. Document and verify spawn happens on main thread only.

### Pattern 7: Autonomy Mode Filtering (AGTR-03)

```rust
// Source: glass_core/src/activity_stream.rs (new enum to add)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMode {
    /// Agent is disabled.
    Off,
    /// Watch: propose only for critical failures (Error severity).
    Watch,
    /// Assist: propose for errors and warnings.
    Assist,
    /// Autonomous: propose for all severities including Info/Success.
    Autonomous,
}

fn should_send_in_mode(mode: AgentMode, severity: &str) -> bool {
    match mode {
        AgentMode::Off => false,
        AgentMode::Watch => severity == "Error",
        AgentMode::Assist => matches!(severity, "Error" | "Warning"),
        AgentMode::Autonomous => true,
    }
}
```

### Pattern 8: Cost Tracking and Budget Gate (AGTR-07)

**What:** Parse `cost_usd` from result messages on stdout. Accumulate per session in `Processor`. Gate new stdin writes when accumulated >= max_budget_usd.

```rust
// In Processor — new fields
agent_cost_usd: f64,           // running total this session
max_budget_usd: f64,           // from config, default 1.0
agent_proposals_paused: bool,  // true when budget exceeded

// On AppEvent::AgentQueryResult { cost_usd }:
self.agent_cost_usd += cost_usd;
if self.agent_cost_usd >= self.max_budget_usd {
    self.agent_proposals_paused = true;
    tracing::warn!(
        "Agent budget cap reached: ${:.4} >= ${:.4}",
        self.agent_cost_usd, self.max_budget_usd
    );
}
```

**Status bar integration:** Pass `agent_cost_text` as a new `Option<String>` parameter to `build_status_text()`. Show format: `"agent: $0.0023"` or `"agent: PAUSED $1.00"` when cap is reached.

```rust
// In status_bar.rs — add new field to StatusLabel:
pub agent_cost_text: Option<String>,
pub agent_cost_color: Rgb,

// Color: green when active, red when paused
let agent_cost_color = if proposals_paused {
    Rgb { r: 255, g: 80, b: 80 }   // red
} else {
    Rgb { r: 80, g: 220, b: 120 }   // green
};
```

### Pattern 9: AppEvent Additions (glass_core/event.rs)

```rust
// Add to AppEvent enum in crates/glass_core/src/event.rs:

/// A structured proposal from the background agent process.
AgentProposal(AgentProposalData),

/// The agent query completed (normal or error). Carries cost for budget tracking.
AgentQueryResult { cost_usd: f64 },

/// The agent process exited unexpectedly. Triggers restart logic.
AgentCrashed,
```

```rust
// New type in glass_core — agent proposal payload
#[derive(Debug, Clone)]
pub struct AgentProposalData {
    /// Human-readable description of the proposed action.
    pub description: String,
    /// Proposed action type: "fix_command", "explain_error", "suggest_alternative"
    pub action: String,
    /// Severity context that triggered this proposal.
    pub severity: String,
    /// command_id from the ActivityEvent that triggered this.
    pub command_id: i64,
    /// Raw text of the full agent response (for review overlay, Phase 58).
    pub raw_response: String,
}
```

### Anti-Patterns to Avoid

- **Calling `child.wait()` on the winit main thread:** `wait()` blocks until the process exits. Always call from a separate thread or use `try_wait()` for non-blocking poll.
- **Dropping `child.stdin` accidentally:** when the `BufWriter` wrapping stdin is dropped, EOF is sent. Keep the writer alive in the writer thread's closure.
- **Parsing stdout line-by-line in the main thread:** stdout reads block. Always use a dedicated reader thread.
- **Writing to stdin without flushing:** `BufWriter` buffers writes. Always `flush()` after each `writeln!()` to ensure the JSON line is sent immediately.
- **`--dangerously-skip-permissions` in interactive sessions:** Only use this flag for the background agent subprocess. Never expose this to user-spawned claude sessions.
- **Using `Stdio::inherit()` for stderr:** This would spill claude's debug output into the Glass terminal's PTY — always use `Stdio::null()`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Orphan prevention (Windows) | Custom SIGTERM handler | Windows Job Object via `windows-sys` | Job Objects work on crash, kill -9, power loss; SIGTERM handlers don't |
| Orphan prevention (Unix) | Custom SIGTERM handler | `prctl(PR_SET_PDEATHSIG, SIGKILL)` | Kernel-enforced; fires even on SIGKILL of parent |
| JSON line protocol | Custom text protocol | JSON lines (NDJSON) with `serde_json` | Already the Claude CLI wire format; free on both sides |
| Cost accumulation | Periodic API polling | Parse `cost_usd` from result messages on stdout | Direct source of truth; no extra API call; confirmed field name |
| Subprocess buffering | Manual byte accumulation | `BufReader::lines()` | Handles partial reads; guarantees complete lines; stdlib |

**Key insight:** The subprocess management pattern is well-established in Rust. The hard part is the protocol integration with Claude CLI, not the process spawning mechanics.

## Common Pitfalls

### Pitfall 1: Missing `--dangerously-skip-permissions` causes permanent hang
**What goes wrong:** Without `--dangerously-skip-permissions`, the Claude CLI process waits for interactive permission prompts when it tries to use tools like Bash. Since Glass reads stdout with `BufReader::lines()`, the reader thread hangs forever waiting for a line that never comes. No timeout, no error — just a silent hang.
**Why it happens:** Claude CLI defaults to interactive permission mode. In non-interactive (piped) mode, it still waits on stdin for permission responses.
**How to avoid:** Always include `--dangerously-skip-permissions` in the agent subprocess invocation. Verify on startup with a test spawn.
**Warning signs:** Reader thread never produces output after the initial `system/init` message.

### Pitfall 2: `cost_usd` field absent on error results
**What goes wrong:** When the agent exits with `subtype: "error_max_turns"` or `subtype: "error_during_execution"`, the `cost_usd` field is still present (confirmed: "Both success and error result messages include usage and total_cost_usd" from official docs). However, the cost may be partial if the error occurred mid-turn.
**Why it happens:** Assumption that error exits don't report cost.
**How to avoid:** Always parse `cost_usd` from all `type: "result"` messages regardless of `subtype`. Use `.unwrap_or(0.0)` for safety.
**Warning signs:** Budget cap never triggers even after error-exit sequences.

### Pitfall 3: Windows Job Object nested job conflict
**What goes wrong:** On Windows, if Glass itself is already running inside a Job Object (e.g., launched from a CI runner or Windows sandbox), `AssignProcessToJobObject` fails silently. Child processes are NOT added to the Glass Job Object and may orphan.
**Why it happens:** Pre-Windows 8, processes could only be in one job at a time.
**How to avoid:** On failure, log a warning but don't crash. The cargo pattern ignores this error silently. On Windows 8+, nested job objects are supported via `SetInformationJobObject(JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK)`.
**Warning signs:** `AssignProcessToJobObject` returns 0 (error) — check with `GetLastError()`.

### Pitfall 4: prctl + tokio interaction (Unix only)
**What goes wrong:** When `pre_exec` is called from a tokio worker thread (not the main thread), tokio's internal thread-reaping mechanism can send signals that trigger the child's death signal on the wrong parent. The child may die immediately after spawn.
**Why it happens:** `PR_SET_PDEATHSIG` is scoped to the thread in the child, and the "parent" is the thread that called `fork`. If tokio's runtime reaps that thread before the child has time to exec, the child receives the death signal.
**How to avoid:** Spawn the agent subprocess from the winit main thread only. Glass's `Processor` methods run on the main thread — this is the correct location. Never spawn the agent from a `tokio::spawn` or `spawn_blocking` closure.
**Warning signs:** Agent process exits immediately with no stdout output.

### Pitfall 5: stdin writer thread outlives child process
**What goes wrong:** If the agent process crashes (reader thread gets EOF), the writer thread continues trying to write to the closed stdin pipe. `writeln!` returns `Err(BrokenPipe)` but if not handled, the thread loops forever consuming ActivityEvent messages.
**Why it happens:** Writer thread and reader thread are independent; neither watches the other.
**How to avoid:** The writer thread's `writeln!` error branch should `break` out of the `for event in rx.iter()` loop. Add a shared `AtomicBool` `agent_alive` flag that both threads check.
**Warning signs:** Writer thread continues running after `AppEvent::AgentCrashed` is received.

### Pitfall 6: `serde_json` deserialization fails on partial stream-json lines
**What goes wrong:** The reader thread receives a partial JSON line if the subprocess writes a very large JSON object that gets split across OS read() calls.
**Why it happens:** `BufReader::lines()` reads until `\n`. Claude CLI uses `\n`-delimited JSON lines — each JSON object ends with exactly one `\n`. This is correct behavior. The issue is only theoretical if the Claude CLI ever omits the trailing newline (which it doesn't per NDJSON spec).
**How to avoid:** Use `BufReader::lines()` — it handles partial reads correctly by buffering until `\n`. Do NOT use `read_to_string` or `read_exact`.
**Warning signs:** `serde_json::from_str` returns `Error(EOF while parsing)`.

## Code Examples

Verified patterns from official sources:

### Complete Claude CLI subprocess invocation
```rust
// Source: code.claude.com/docs/en/cli-reference (official, verified 2026-03-13)
// Flags: -p (print mode), --output-format stream-json, --input-format stream-json,
//        --system-prompt-file, --mcp-config, --allowedTools, --dangerously-skip-permissions,
//        --max-budget-usd
use std::process::{Command, Stdio};

let mut cmd = Command::new("claude");
cmd.args([
    "-p",
    "--output-format", "stream-json",
    "--input-format", "stream-json",
    "--system-prompt-file", "/path/to/agent-system-prompt.txt",
    "--mcp-config", "/path/to/glass-mcp.json",
    "--allowedTools", "Bash,Read",
    "--dangerously-skip-permissions",
    "--max-budget-usd", "1.00",
])
.stdin(Stdio::piped())
.stdout(Stdio::piped())
.stderr(Stdio::null());
```

### stream-json result message parsing
```rust
// Source: github.com/anthropics/claude-code/issues/1920 + code.claude.com verified wire format
// Result message JSON:
// {"type":"result","subtype":"success","session_id":"<uuid>","cost_usd":0.00231,"num_turns":2}
fn parse_cost_from_result(line: &str) -> Option<f64> {
    let val: serde_json::Value = serde_json::from_str(line).ok()?;
    if val.get("type")?.as_str()? != "result" {
        return None;
    }
    val.get("cost_usd")?.as_f64()
}
```

### Windows Job Object setup (AGTR-05)
```rust
// Source: Cargo's util/job.rs — proven production pattern
// Requires windows-sys features: Win32_System_JobObjects, Win32_Foundation, Win32_Security
#[cfg(target_os = "windows")]
pub fn setup_windows_job_object() {
    use windows_sys::Win32::Foundation::GetCurrentProcess;
    use windows_sys::Win32::System::JobObjects::*;

    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job == 0 { return; }

        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of_val(&info) as u32,
        );
        AssignProcessToJobObject(job, GetCurrentProcess());
        // Store handle somewhere — dropping it closes it, which kills children
        // Use Box::leak or store in Processor field as HANDLE (isize on windows-sys)
    }
}
```

### Unix prctl pre_exec (AGTR-05)
```rust
// Source: std::os::unix::process::CommandExt documentation (official)
// WARNING: Only safe when called from the main thread (not tokio workers)
#[cfg(unix)]
fn configure_orphan_prevention(cmd: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        cmd.pre_exec(|| {
            // libc not in workspace — either add it, or use inline asm / syscall
            // Alternatively: use the `nix` crate, but it's also not in workspace.
            // Simplest: inline the prctl syscall number (172 on x86_64/aarch64)
            // For portability, add `libc` to workspace deps.
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL as libc::c_ulong, 0, 0, 0);
            Ok(())
        });
    }
}
```

### Activity event formatted as stream-json user message
```rust
// Source: platform.claude.com/docs/en/agent-sdk/streaming-vs-single-mode
// Input format: {"type":"user","message":{"role":"user","content":"<text>"}}
fn format_activity_as_user_message(event: &ActivityEvent) -> String {
    let text = format!(
        "[ACTIVITY] severity={} summary={} command_id={} collapsed={}",
        event.severity, event.summary, event.command_id, event.collapsed_count
    );
    serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": text
        }
    })
    .to_string()
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `claude --print` (single-shot) | `claude -p --output-format stream-json --input-format stream-json` (persistent session) | 2025-2026 (stream-json + input-format added) | Enables long-lived subprocess with multi-turn context; was previously "headless mode" |
| `--output-format json` (single result blob) | `--output-format stream-json` (NDJSON per event) | 2025 | Streaming allows real-time cost tracking without waiting for process exit |
| Manual process management | Windows Job Objects + Unix prctl | N/A (Glass first agent phase) | Crash-safe orphan prevention vs. graceful-exit-only |
| "headless mode" terminology | "Agent SDK" / programmatic usage | 2025 rename | docs.claude.com calls it "Agent SDK"; CLI still uses `-p` |

**Deprecated/outdated:**
- `--output-format text` with `-p`: gives no structured data, unsuitable for machine parsing.
- Running `claude` without `--dangerously-skip-permissions` in non-interactive mode: hangs on permission prompts.

## Open Questions

1. **`libc` crate availability for Unix prctl**
   - What we know: `libc` is not in the workspace `Cargo.toml`. `prctl` is needed on Unix.
   - What's unclear: Does Glass already use `libc` transitively (via `alacritty_terminal`)? If so, adding `libc` as a direct dep is trivial. If not, alternative: use a raw syscall wrapper.
   - Recommendation: Check `cargo tree | grep libc`. If present transitively, add `libc` as a direct workspace dep. If not, implement with inline `unsafe { libc::prctl(...) }` after adding `libc = "0.2"` to workspace deps. This is low risk — `libc` is a widely used stable crate.

2. **MCP config path for the agent subprocess**
   - What we know: Glass's MCP server is started by `glass_mcp` crate. The agent subprocess needs to connect to it via `--mcp-config`.
   - What's unclear: What socket/transport does `glass_mcp` expose? Is it stdio or TCP? Can the agent subprocess connect to it?
   - Recommendation: Phase 56 can hardcode a known MCP config path at `~/.glass/agent-mcp.json` that points to Glass's existing MCP stdio server. Investigate `glass_mcp` startup in Wave 0.

3. **Session ID persistence across restarts (AGTR-04)**
   - What we know: When the agent crashes and restarts, it starts a new session without memory of the previous session.
   - What's unclear: Should Phase 56 attempt session continuity via `--continue` or `--resume`? Phase 59 covers Agent Session Continuity (AGTS-*), suggesting Phase 56 should NOT implement this.
   - Recommendation: Phase 56 starts a fresh session on every spawn. Session continuity is Phase 59's scope. Note this explicitly so Phase 59 can find the session ID from the `system/init` message.

4. **`glass_proposal` extraction protocol**
   - What we know: The agent must output proposals in a way Glass can parse. Assistant messages are free-form text.
   - What's unclear: Should proposals be embedded as JSON blocks in text, or should Glass use a dedicated MCP tool call that the agent makes?
   - Recommendation: Inject a custom `emit_proposal` MCP tool into the Glass MCP server (or define it in the MCP config). The tool call appears as a `tool_use` content block in the assistant message stream. This is more robust than parsing embedded JSON from free-form text. However, this requires `glass_mcp` changes. For Phase 56 simplicity: use embedded JSON pattern `GLASS_PROPOSAL: {...}` in assistant text, parseable with a simple prefix search. Upgrade to MCP tool in Phase 57/58.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test (`cargo test`) |
| Config file | None (inline `#[cfg(test)]`) |
| Quick run command | `cargo test -p glass_core -- agent` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| AGTR-01 | Command::new("claude") spawns with correct flags | unit | `cargo test -p glass -- agent_runtime::tests::test_agent_command_flags` | Wave 0 |
| AGTR-02 | format_activity_as_user_message produces valid JSON | unit | `cargo test -p glass_core -- agent_runtime::tests::test_format_activity_message` | Wave 0 |
| AGTR-02 | parse_cost_from_result extracts cost_usd from result JSON | unit | `cargo test -p glass_core -- agent_runtime::tests::test_parse_cost_from_result` | Wave 0 |
| AGTR-03 | should_send_in_mode returns correct booleans per mode+severity | unit | `cargo test -p glass_core -- agent_runtime::tests::test_autonomy_mode_filter` | Wave 0 |
| AGTR-04 | AgentRuntime restart after crash re-spawns process | unit (mock) | `cargo test -p glass -- agent_runtime::tests::test_restart_on_crash` | Wave 0 |
| AGTR-05 | Windows: Job Object handle is non-null after setup | unit | `cargo test -p glass -- agent_runtime::tests::test_windows_job_object_setup` (cfg windows) | Wave 0 |
| AGTR-06 | Cooldown: second event within 30s is filtered | unit | `cargo test -p glass_core -- agent_runtime::tests::test_cooldown_filter` | Wave 0 |
| AGTR-07 | Budget gate: events blocked when cost >= max_budget_usd | unit | `cargo test -p glass_core -- agent_runtime::tests::test_budget_gate` | Wave 0 |
| AGTR-07 | status bar cost text shows "$0.0023" format | unit | `cargo test -p glass_renderer -- status_bar::tests::test_agent_cost_text_format` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_core -- agent`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_core/src/agent_runtime.rs` — new file: `AgentMode`, `AgentProposalData`, `AgentRuntimeConfig`, `should_send_in_mode`, `format_activity_as_user_message`, `parse_cost_from_result`, cooldown/budget helpers
- [ ] `crates/glass_core/src/event.rs` — add `AppEvent::AgentProposal`, `AppEvent::AgentQueryResult`, `AppEvent::AgentCrashed`
- [ ] `crates/glass_renderer/src/status_bar.rs` — add `agent_cost_text` field to `StatusLabel`
- [ ] Windows `windows-sys` features: add `Win32_System_JobObjects`, `Win32_Foundation`, `Win32_Security` to workspace `Cargo.toml`
- [ ] Investigate `glass_mcp` startup — find how to generate `~/.glass/agent-mcp.json` for subprocess

## Sources

### Primary (HIGH confidence)
- `code.claude.com/docs/en/cli-reference` — Official CLI reference. Verified flags: `-p`, `--output-format stream-json`, `--input-format stream-json`, `--system-prompt-file`, `--mcp-config`, `--allowedTools`, `--dangerously-skip-permissions`, `--max-budget-usd`. Accessed 2026-03-13.
- `platform.claude.com/docs/en/agent-sdk/cost-tracking` — Official cost tracking docs. Confirmed `cost_usd` field name on CLI result messages, `total_cost_usd` on SDK result messages. Accessed 2026-03-13.
- `platform.claude.com/docs/en/agent-sdk/streaming-vs-single-mode` — Official SDK streaming input docs. Confirmed `{"type":"user","message":{"role":"user","content":"..."}}` wire format for stdin. Accessed 2026-03-13.
- `github.com/anthropics/claude-code/issues/1920` — Community-verified stream-json message format. Confirmed `{"type":"result","subtype":"success","session_id":"...","cost_usd":2.90831585,"num_turns":62}`. Accessed 2026-03-13.
- `doc.rust-lang.org/nightly/nightly-rustc/src/cargo/util/job.rs.html` — Cargo's Windows Job Object implementation. Verified pattern for `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` + `AssignProcessToJobObject`. Accessed 2026-03-13.
- `std::os::unix::process::CommandExt` — Official Rust stdlib `pre_exec` trait for Unix-only child process setup.
- `crates/glass_core/src/activity_stream.rs` — Phase 55 completed implementation. Confirmed `ActivityEvent` struct, `create_channel()`, `ActivityFilter`. Accessed directly in codebase.
- `src/main.rs` — Confirmed `activity_stream_rx: Option<Receiver<ActivityEvent>>` stored in Processor (line ~228). Accessed directly in codebase.
- `crates/glass_renderer/src/status_bar.rs` — Confirmed `StatusLabel` struct fields; no existing `agent_cost_text` field. Accessed directly in codebase.
- `Cargo.toml` (workspace) — Confirmed `windows-sys = "0.59"` with only `Win32_System_Console` feature. `serde_json = "1.0"` in root deps. No `libc` direct dep.

### Secondary (MEDIUM confidence)
- `platform.claude.com/docs/en/agent-sdk/streaming-output` — SDK streaming output docs. Message type names (`stream_event`, `result`, `assistant`) verified. Used to corroborate stream-json output format.
- `kobzol.github.io/rust/2025/02/23/tokio-plus-prctl-equals-nasty-bug.html` — Rust community post (Feb 2025) on tokio+prctl interaction. Informs the "spawn from main thread only" constraint for Unix prctl.

### Tertiary (LOW confidence)
- General WebSearch results on Windows Job Objects — corroborate Cargo pattern; not independently verified in official Windows docs.
- `glass_proposal` embedded JSON pattern — proposed by this research; no external precedent. Marked LOW confidence; see Open Question #4.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all stdlib + workspace deps, officially documented
- Claude CLI wire protocol: HIGH — verified from official docs + GitHub issue
- Architecture: HIGH — follows established project patterns exactly
- Windows Job Objects: HIGH — Cargo's proven production pattern
- Unix prctl: MEDIUM — correct API but tokio interaction constraint needs verification in Glass's actual thread model
- `glass_proposal` detection protocol: LOW — proposed pattern, not externally validated

**Research date:** 2026-03-13
**Valid until:** 2026-04-13 (Claude CLI stream-json protocol may evolve; re-verify flag names before planning)

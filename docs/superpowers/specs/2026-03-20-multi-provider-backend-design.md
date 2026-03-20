# Multi-Provider Agent Backend — Design Spec

**Date:** 2026-03-20
**Status:** Draft
**Scope:** Extract agent backend into a trait, support multiple LLM providers for orchestrator and implementer roles

## Problem

The orchestrator is tightly coupled to Claude Code CLI. The spawn logic, I/O protocol, and JSON parsing are hardcoded in `main.rs:try_spawn_agent()` (~580 lines). Open-sourcing Glass means users will want to use other models (GPT, Gemini, local models via Ollama) for the orchestrator and/or implementer roles. Currently there is no way to do this without rewriting the agent lifecycle code.

## Goals

1. Support multiple LLM providers for the orchestrator role (Claude CLI, Anthropic API, OpenAI API, Ollama, custom OpenAI-compatible endpoints)
2. Let users pick specific models within each provider (e.g., Opus vs Sonnet, GPT-4o vs o3)
3. Allow mixing models between orchestrator and implementer (e.g., GPT orchestrator + Claude Code implementer)
4. Zero friction for the default case — users who change nothing get identical behavior to today
5. Zero risk to the existing Claude Code workflow at every phase

## Non-Goals

- Building a full code-writing agent inside Glass (the implementer is always an external CLI)
- OAuth client implementation (deferred to future work)
- Per-model prompt tuning in the UI (handled internally per backend)
- Model download/management (Ollama's job, not Glass's)

## Architecture Overview

```
                    ┌─────────────────────────┐
                    │       main.rs            │
                    │  Orchestrator state      │
                    │  machine, event routing  │
                    │                          │
                    │  Drains AgentEvent from   │
                    │  handle.event_rx, maps   │
                    │  to AppEvent via proxy   │
                    └────────┬────────────────┘
                             │
                    ┌────────▼────────────────┐
                    │  resolve_backend()       │
                    │  config → Box<dyn        │
                    │  AgentBackend>           │
                    └────────┬────────────────┘
                             │
          ┌──────────────────┼──────────────────┐
          │                  │                  │
   ┌──────▼──────┐   ┌──────▼──────┐   ┌──────▼──────┐
   │ ClaudeCli   │   │ OpenAi      │   │ Ollama      │
   │ Backend     │   │ Backend     │   │ Backend     │
   │ (Phase 1)   │   │ (Phase 2)   │   │ (Phase 3)   │
   └─────────────┘   └─────────────┘   └─────────────┘
```

## Core Types

### AgentEvent — normalized output from any backend

```rust
/// Normalized events emitted by any agent backend.
/// main.rs never sees provider-specific JSON — only these.
pub enum AgentEvent {
    /// Agent session initialized
    Init { session_id: String },
    /// Agent produced text (may contain GLASS_WAIT, GLASS_DONE, etc.)
    /// main.rs runs extract_proposal() and extract_handoff() on this text
    /// to detect proposals and handoffs — those are text-level concerns,
    /// not wire-protocol concerns, so they stay in main.rs.
    AssistantText { text: String },
    /// Agent is thinking (extended thinking / reasoning tokens)
    Thinking { text: String },
    /// Agent called a tool
    ToolCall { name: String, id: String, input: String },
    /// Tool result returned
    ToolResult { tool_use_id: String, content: String },
    /// A conversation turn completed
    TurnComplete { cost_usd: f64 },
    /// Agent process/connection died unexpectedly
    Crashed,
}
```

Every backend parses its provider-specific format (Claude CLI stream-json, OpenAI SSE chunks, Ollama JSON lines) and normalizes to `AgentEvent`. The orchestrator state machine in `main.rs` only handles `AgentEvent` — it never sees wire-protocol details.

**Proposal and handoff extraction:** `extract_proposal()` and `extract_handoff()` parse the assistant's text content (looking for `GLASS_PROPOSAL:` and `GLASS_HANDOFF:` markers). These are text-level operations that work identically regardless of provider. They stay in `main.rs` and operate on `AgentEvent::AssistantText`. This is a deliberate decision — the `AgentEvent` enum normalizes wire protocol, not application-level semantics.

### AgentBackend trait

```rust
/// Configuration passed to any backend at spawn time.
pub struct BackendSpawnConfig {
    pub system_prompt: String,
    pub initial_message: Option<String>,
    pub project_root: String,
    pub mcp_config_path: String,
    pub allowed_tools: Vec<String>,
    pub mode: AgentMode,
    pub cooldown_secs: u64,
    pub restart_count: u32,
    pub last_crash: Option<std::time::Instant>,
}

/// Handle returned by spawn() — main.rs holds this.
/// Backend-agnostic: no subprocess-specific fields.
pub struct AgentHandle {
    /// Send messages to the agent (backend's internal thread handles protocol)
    pub message_tx: std::sync::mpsc::Sender<String>,
    /// Receives normalized AgentEvents
    pub event_rx: std::sync::mpsc::Receiver<AgentEvent>,
    /// Generation counter for stale-event detection
    pub generation: u64,
}

/// Trait that every provider implements.
pub trait AgentBackend: Send + Sync {
    /// Human-readable name for logs and UI
    fn name(&self) -> &str;

    /// Spawn the agent process/connection, wire up internal threads that
    /// send AgentEvents to the returned channel.
    /// Does NOT take EventLoopProxy — backends are decoupled from winit.
    /// main.rs drains event_rx and maps AgentEvent → AppEvent itself.
    fn spawn(
        &self,
        config: &BackendSpawnConfig,
        generation: u64,
    ) -> Result<AgentHandle, BackendError>;

    /// Shut down the agent cleanly. Called on checkpoint respawn and
    /// deactivation. Each backend stores its own internal shutdown
    /// mechanism (e.g., kill channel, Arc<Mutex<Child>>) — the handle
    /// is used for cleanup coordination (dropping message_tx to signal
    /// the writer thread), not for direct process termination.
    /// CLI backend: kills child process. API backend: cancels in-flight
    /// requests and drops HTTP connection.
    fn shutdown(&self, handle: &AgentHandle);
}
```

**Key decisions:**
- **No `EventLoopProxy` in `spawn()`** — backends are fully decoupled from winit and `AppEvent`. They send `AgentEvent` through `event_rx`. `main.rs` drains the channel and maps to `AppEvent` using its own proxy. This keeps the `glass_agent_backend` crate free of winit/glass_core dependencies.
- **`message_tx` instead of `writer`** — backends receive messages through a channel, not a raw `Write` stream. Each backend's internal thread reads from `message_tx` and sends via its own protocol (stdin write for CLI, HTTP POST for APIs). The caller just sends a `String` — the backend handles formatting and transmission.
- **`shutdown()` replaces `child`** — no subprocess-specific fields on `AgentHandle`. Shutdown is backend-specific: CLI kills the child process, API backends cancel requests. Called during checkpoint respawn (`respawn_orchestrator_agent()` calls `shutdown()` before spawning a new handle) and orchestrator deactivation.
- **`format_message`/`format_activity` removed from trait** — message formatting is internal to each backend. The caller sends plain text via `message_tx`, the backend's writer thread handles protocol-specific formatting. This simplifies the interface and avoids exposing wire-format details.
- **`restart_count` and `last_crash` in `BackendSpawnConfig`** — backends may use these for exponential backoff or diagnostics logging.

### How main.rs uses AgentHandle

```rust
// Sending a message to the agent (orchestrator context, activity events, etc.)
handle.message_tx.send(content).ok();

// Draining events (in the winit event loop or a polling thread)
while let Ok(event) = handle.event_rx.try_recv() {
    match event {
        AgentEvent::AssistantText { text } => {
            // Extract proposals/handoffs from text (application-level)
            if let Some(proposal) = extract_proposal(&text) {
                proxy.send_event(AppEvent::AgentProposal(proposal));
            }
            if let Some((handoff, raw)) = extract_handoff(&text) {
                proxy.send_event(AppEvent::AgentHandoff { .. });
            }
            // Buffer for orchestrator response (emit on TurnComplete)
            buffered_response = Some(text);
        }
        AgentEvent::TurnComplete { cost_usd } => {
            proxy.send_event(AppEvent::AgentQueryResult { cost_usd });
            if let Some(response) = buffered_response.take() {
                proxy.send_event(AppEvent::OrchestratorResponse { response });
            }
        }
        AgentEvent::Thinking { text } => {
            proxy.send_event(AppEvent::OrchestratorThinking { text });
        }
        AgentEvent::ToolCall { name, input, .. } => {
            proxy.send_event(AppEvent::OrchestratorToolCall { name, params_summary: input });
        }
        AgentEvent::ToolResult { tool_use_id, content } => {
            // Look up tool name from id, emit OrchestratorToolResult
        }
        AgentEvent::Crashed => {
            proxy.send_event(AppEvent::AgentCrashed);
        }
        AgentEvent::Init { session_id } => {
            // Store session_id for handoff tracking
        }
    }
}
```

This event routing loop replaces the current reader thread's direct `AppEvent` sends. The logic is identical — it's just one hop removed from the backend.

## Config Surface

### New fields in `config.toml`

```toml
[agent]
provider = "claude-code"          # "claude-code", "anthropic-api", "openai-api", "ollama", "custom"
model = ""                        # empty = provider default (e.g. "gpt-4o", "claude-sonnet-4-6")
api_key = ""                      # optional — env var used if empty
api_endpoint = ""                 # optional — only for custom endpoints

[agent.orchestrator]
implementer = "claude-code"       # "claude-code", "codex", "aider", "gemini", "custom"
implementer_command = ""          # custom launch command (when implementer = "custom")
implementer_name = "Claude Code"  # display name in system prompt
persona = ""                      # inline persona string, or path to .md file
```

**All fields default to empty or current behavior.** A user who touches nothing gets identical behavior to today.

**Config field placement:** `provider`, `model`, `api_key`, `api_endpoint` live in `[agent]` because they control the LLM backend used for both orchestrator mode and the non-orchestrator agent modes (watch/assist/autonomous). `implementer`, `implementer_command`, `implementer_name`, `persona` live in `[agent.orchestrator]` because they only apply when the orchestrator is active.

**Security note:** `api_key` in `config.toml` is stored in plaintext. Environment variables (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, etc.) are the recommended primary auth mechanism. The `api_key` config field is a convenience fallback for users who prefer file-based config. Documentation should warn that config files may be committed to version control. The settings overlay displays auth status (e.g., "API Key" or "Env Var") but never shows the actual key value.

### Provider resolution

```rust
fn resolve_backend(config: &GlassConfig) -> Result<Box<dyn AgentBackend>, BackendError> {
    let provider = config.agent.provider.as_deref().unwrap_or("claude-code");
    let model = config.agent.model.as_deref().unwrap_or("");
    // Env var takes precedence over config file (security best practice)
    let api_key = env_key_for_provider(provider)
        .or_else(|| config.agent.api_key.clone());
    let endpoint = config.agent.api_endpoint.as_deref().unwrap_or("");

    match provider {
        "claude-code"    => Ok(Box::new(ClaudeCliBackend::new())),
        "anthropic-api"  => {
            let key = api_key.ok_or(BackendError::MissingCredentials {
                provider: "anthropic-api".into(),
                env_var: "ANTHROPIC_API_KEY".into(),
            })?;
            Ok(Box::new(AnthropicApiBackend::new(key, model, endpoint)))
        }
        "openai-api"     => {
            let key = api_key.ok_or(BackendError::MissingCredentials {
                provider: "openai-api".into(),
                env_var: "OPENAI_API_KEY".into(),
            })?;
            Ok(Box::new(OpenAiBackend::new(key, model, endpoint)))
        }
        "ollama"         => Ok(Box::new(OllamaBackend::new(model, endpoint))),
        // Custom endpoints may not require auth (e.g., local vLLM, llama.cpp).
        // Empty API key is accepted silently — auth failures surface at request time.
        "custom"         => Ok(Box::new(OpenAiBackend::new(
            api_key.unwrap_or_default(), model, endpoint,
        ))),
        _                => Ok(Box::new(ClaudeCliBackend::new())),
    }
}

fn env_key_for_provider(provider: &str) -> Option<String> {
    match provider {
        "anthropic-api" => std::env::var("ANTHROPIC_API_KEY").ok(),
        "openai-api"    => std::env::var("OPENAI_API_KEY").ok(),
        "ollama"        => None,
        "custom"        => std::env::var("GLASS_API_KEY").ok(),
        _               => None,
    }
}
```

**Error handling:** `resolve_backend()` returns `Result<>`. When credentials are missing, it returns `BackendError::MissingCredentials` with the provider name and expected env var. The caller (orchestrator activation in `main.rs`) catches this and shows a toast: `"Set OPENAI_API_KEY to use OpenAI models"`. The orchestrator does not activate.

### Auth

Auth is a single `api_key` field + env var fallback. No OAuth client, no bearer token management.

- `claude-code` provider: CLI handles its own auth (OAuth). Zero config.
- API providers: Glass checks the standard env var (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`). Users who use these APIs already have these set.
- Env var not found + no `api_key` in config: `BackendError::MissingCredentials` → toast with instructions.
- Power users: `api_key = "..."` in config as fallback. Env var takes precedence if both are set (security best practice — env vars are not committed to version control).

### Implementer launch

```rust
fn implementer_launch_command(config: &OrchestratorSection) -> String {
    match config.implementer.as_deref().unwrap_or("claude-code") {
        "claude-code" => "claude --dangerously-skip-permissions -p".to_string(),
        "codex"       => "codex --full-auto".to_string(),
        "aider"       => "aider --yes-always".to_string(),
        "gemini"      => "gemini".to_string(),
        "custom"      => config.implementer_command.clone().unwrap_or_default(),
        _             => "claude --dangerously-skip-permissions -p".to_string(),
    }
}
```

Used for crash recovery (currently hardcoded at `main.rs:6312`) and optional auto-launch when orchestrator activates.

### System prompt layering

```
┌─────────────────────────────────────────────────┐
│  Layer 1: Protocol (hardcoded, never exposed)   │  GLASS_WAIT, GLASS_DONE, response format
│  Layer 2: Mode behavior (hardcoded per mode)    │  build/audit/general iteration protocol
│  Layer 3: Persona (user-editable)               │  tone, role, domain expertise, constraints
│  Layer 4: Project instructions (already exists) │  .glass/agent-instructions.md
└─────────────────────────────────────────────────┘
```

Layers 1-2 use `{implementer_name}` instead of hardcoded "Claude Code". Layer 3 is new — loaded from `persona` config (inline string or path to `.md` file). Layer 4 is unchanged.

```rust
let persona = match config.persona {
    Some(ref p) if p.ends_with(".md") => std::fs::read_to_string(p).unwrap_or_default(),
    Some(ref p) => p.clone(),
    None => String::new(),
};
let system_prompt = format!("{protocol_and_mode}\n\n{persona}\n\n{critical_rules}");
```

## Settings Overlay

### Model discovery

**At startup (lightweight, no API calls):**
- Check which env vars exist (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`)
- Check if `claude` CLI is on PATH
- Check if Ollama is reachable at `localhost:11434`

**On settings overlay open (on demand):**
- For each available provider, fetch model list from API (cached 24h)
- Ollama: `GET /api/tags` (instant, local)
- OpenAI/Anthropic/Google: `GET /v1/models` (one request each, cached)
- Filter to chat-capable models only (exclude embeddings, TTS, image models)

**Cache details:**
- Cache files stored in `~/.glass/cache/models/{provider}.json`
- Each file contains: model list + fetch timestamp
- Cache invalidated when provider config changes or 24h expires
- Shared across Glass instances (file-based, read-only safe)

**Fallback when offline:**
- Show cached models from last fetch
- If no cache, show only Claude Code (if CLI on PATH)

### Display

```
┌─ Orchestrator ──────────────────────────────────┐
│ Enabled          ON                             │
│ Orchestrator     OpenAI / gpt-4o           ◄►   │
│ Implementer      Claude Code               ◄►   │
│ Persona          (default)                  ✎   │
│ Mode             auto                      ◄►   │
│ Auth             ✓ Env Var                      │
│ ...                                             │
└─────────────────────────────────────────────────┘
```

Left/right arrows cycle through a flat list of available `Provider / Model` combinations. Only providers with valid credentials appear. Selection auto-updates `provider` and `model` in `config.toml` via hot-reload.

Auth status is read-only display — shows "Env Var", "API Key", or "CLI Auth" depending on how credentials were resolved. Never shows the actual key value.

Display names use a best-effort friendly map with raw ID fallback for unknown models:
- Known: `claude-opus-4-6` → "Claude Opus", `gpt-4o` → "GPT-4o"
- Unknown: `claude-opus-5` → "anthropic-api / claude-opus-5"

New models appear automatically when the API returns them. Friendly names are cosmetic — added in Glass updates but never functionally required.

## Phase Breakdown

### Phase 1: Extract ClaudeCliBackend (pure refactor)

**Scope:** Zero behavior change. Move existing code behind the trait.

**New crate:** `crates/glass_agent_backend/`
- `lib.rs` — `AgentEvent`, `AgentBackend` trait, `AgentHandle`, `BackendSpawnConfig`, `BackendError`
- `claude_cli.rs` — `ClaudeCliBackend` struct

**What moves into `ClaudeCliBackend::spawn()`:**
- `build_agent_command_args()` call
- `Command::new("claude")` with all platform-specific setup (stderr null, Windows CREATE_NO_WINDOW, Linux PR_SET_PDEATHSIG, macOS orphan watchdog)
- Initial stdin message write (CLI 2.1.77+ compat)
- Prior handoff loading from `AgentSessionDb`
- Reader thread with full JSON parser (stream-json → `AgentEvent` via `event_tx` channel)
- Writer thread: reads from `message_tx` channel, formats as stream-json, writes to stdin with cooldown gating
- Coordination DB registration
- `shutdown()` implementation: kills the child process (same as current `Drop` behavior)

**What stays in `main.rs`:**
- `AgentRuntime` struct — holds `AgentHandle` + backend `Box<dyn AgentBackend>` instead of raw child/writer
- `try_spawn_agent()` — becomes thin: `resolve_backend()` → `backend.spawn()` → wrap in `AgentRuntime`
- Event routing loop: drain `handle.event_rx`, map `AgentEvent` → `AppEvent` (including proposal/handoff extraction from `AssistantText`)
- Respawn logic: `backend.shutdown()` → `backend.spawn()` with new generation
- System prompt assembly (provider-agnostic)

**What stays in `glass_core::agent_runtime`:**
- `AgentRuntimeConfig`, `AgentMode`, `CooldownTracker`, `BudgetTracker` — unchanged
- `extract_proposal()`, `extract_handoff()`, `classify_proposal()` — parse agent text, not wire protocol
- `should_send_in_mode()`, `should_quiet()` — config logic
- `build_agent_command_args()` — moves to `glass_agent_backend::claude_cli` (only used by ClaudeCliBackend)

**New config fields added:**
- In `[agent]`: `provider`, `model`, `api_key`, `api_endpoint` (all defaulting to current behavior)
- In `[agent.orchestrator]`: `implementer`, `implementer_command`, `implementer_name`, `persona`

**Crash recovery update:** Line 6312 uses `implementer_launch_command()` instead of hardcoded `claude` command.

**System prompt update:** Replace hardcoded "Claude Code" with `{implementer_name}`. Insert persona layer.

**Safety protocol:**
1. Write regression tests BEFORE moving any code — sample Claude CLI JSON → expected `AgentEvent` sequence
2. Extract as a line-for-line move — diff should show moved lines, not changed lines
3. End-to-end verification after extraction: Ctrl+Shift+O, run several iterations, verify activity overlay, checkpoint/respawn
4. The only change in behavior: `AgentEvent` as an intermediate step before `AppEvent` (one extra hop, functionally identical)

### Phase 2: OpenAiCompatibleBackend (additive)

**Scope:** New backend behind config gate. Cannot affect Claude Code path.

**New files:**
- `crates/glass_agent_backend/src/openai.rs`
- `crates/glass_agent_backend/src/model_cache.rs` — shared model list cache

**HTTP client:** Use `ureq` (already in workspace) with manual SSE line parsing in a dedicated reader thread. This matches the existing synchronous reader thread pattern — no async runtime needed. SSE parsing is straightforward: read lines, detect `data:` prefixes, parse JSON chunks. If `ureq`'s streaming proves insufficient for SSE, `eventsource-client` is a lightweight alternative that works without tokio.

**Implementation:**
- HTTP POST to `/v1/chat/completions` with `stream: true`
- SSE response parsing in a dedicated reader thread → `AgentEvent` via channel
- Writer thread: reads from `message_tx`, accumulates conversation history, sends HTTP requests
- Cost tracking from `usage` field in final SSE chunk

**Tool calling loop (the biggest work item):**
1. Include tool definitions in the request (Glass MCP tool schemas converted to OpenAI function format)
2. Parse `tool_calls` from streamed response chunks
3. Execute the tool: Glass already runs an MCP server in-process. The backend calls the MCP tool handler directly (same code path the Glass MCP server uses for external callers)
4. Append tool result as a `tool` message in conversation history
5. Send next request with updated history
6. Repeat until model produces a final text response (no tool_calls)
7. Timeout: max 10 tool-call rounds per turn to prevent infinite loops
8. Error handling: if MCP tool execution fails, send error text as tool result so the model can recover

**Scope of tool access for API backends:** API backends only get Glass MCP tools (glass_query, glass_context, etc.) — the same tools the orchestrator agent uses today. They do NOT get Bash/Read/Write/Edit, which are Claude CLI built-in tools. This is correct because the orchestrator's role is to observe and instruct, not to implement.

**Model list fetch:**
- `GET /v1/models` with 24h file cache in `~/.glass/cache/models/`
- Filter: exclude models with IDs containing `embed`, `tts`, `whisper`, `dall-e`, `moderation`
- Display name mapping for known models

**Settings overlay additions:**
- Orchestrator/Implementer fields cycle through available models
- Provider auto-detection from env vars at startup

**Covers:** OpenAI (GPT-4o, o3, etc.), Google Gemini (OpenAI-compat mode), any OpenAI-compatible local server (vLLM, llama.cpp server, LM Studio)

### Phase 3: AnthropicApiBackend + OllamaBackend (additive)

**Scope:** Two more backends behind config gate.

**New files:**
- `crates/glass_agent_backend/src/anthropic.rs`
- `crates/glass_agent_backend/src/ollama.rs`

**AnthropicApiBackend:**
- Anthropic Messages API (`/v1/messages`) with SSE streaming
- Native `tool_use` content blocks and `thinking` blocks (better thinking support than OpenAI-compat)
- Same tool calling loop as OpenAI backend but using Anthropic's tool_use format
- For users who want Claude without installing the CLI

**OllamaBackend:**
- Native Ollama API (`/api/chat`) with streaming JSON lines
- Tool support via Ollama's function calling
- Model list from `GET /api/tags` (local, instant)
- Default endpoint `http://localhost:11434`, configurable via `api_endpoint`
- No auth needed

**Settings overlay:** All providers/models in unified cycle list.

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| Phase 1 extraction breaks Claude CLI path | Low | High | Regression tests + line-for-line move |
| New backend has bugs | Medium | Zero for existing users | Config gate — default path never executes new code |
| Model list fetch slows startup | Low | Low | Fetch only on settings overlay open, not startup |
| Trait interface needs rework | Medium | Low | Only affects new backends, Claude CLI path is stable |
| Tool calling implementation has bugs | Medium | Medium | Only affects Phase 2+ backends, behind config gate |
| API key in plaintext config | Low | Medium | Env vars recommended as primary; docs warn about config file exposure |

## Files Affected

### Phase 1 (modified)
- `Cargo.toml` — add `glass_agent_backend` to workspace
- `src/main.rs` — slim down `try_spawn_agent()`, `AgentRuntime` holds `AgentHandle` + backend, event routing loop drains `event_rx`
- `crates/glass_core/src/config.rs` — `[agent]`: `provider`, `model`, `api_key`, `api_endpoint`; `[agent.orchestrator]`: `implementer`, `implementer_command`, `implementer_name`, `persona`
- `crates/glass_renderer/src/settings_overlay.rs` — orchestrator section gets Persona field
- `config.example.toml` — document new fields

### Phase 1 (new)
- `crates/glass_agent_backend/Cargo.toml`
- `crates/glass_agent_backend/src/lib.rs` — trait, types, `resolve_backend()`
- `crates/glass_agent_backend/src/claude_cli.rs` — extracted spawn/reader/writer logic

### Phase 2 (new)
- `crates/glass_agent_backend/src/openai.rs`
- `crates/glass_agent_backend/src/model_cache.rs` — shared model list cache

### Phase 2 (modified)
- `crates/glass_agent_backend/Cargo.toml` — add `ureq` (or reuse workspace dep)
- `crates/glass_agent_backend/src/lib.rs` — register OpenAI backend in `resolve_backend()`
- `crates/glass_renderer/src/settings_overlay.rs` — model picker with dynamic list

### Phase 3 (new)
- `crates/glass_agent_backend/src/anthropic.rs`
- `crates/glass_agent_backend/src/ollama.rs`

### Phase 3 (modified)
- `crates/glass_agent_backend/src/lib.rs` — register Anthropic + Ollama backends

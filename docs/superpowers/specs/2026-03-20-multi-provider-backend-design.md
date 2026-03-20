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
                    │  handle.event_rx         │
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
}

/// Handle returned by spawn() — main.rs holds this.
pub struct AgentHandle {
    /// Send orchestrator messages to the agent
    pub writer: Arc<Mutex<Box<dyn Write + Send>>>,
    /// Receives normalized AgentEvents
    pub event_rx: std::sync::mpsc::Receiver<AgentEvent>,
    /// The child process (for kill on shutdown/checkpoint)
    pub child: Option<std::process::Child>,
    /// Generation counter for stale-event detection
    pub generation: u64,
}

/// Trait that every provider implements.
pub trait AgentBackend: Send + Sync {
    /// Human-readable name for logs and UI
    fn name(&self) -> &str;

    /// Spawn the agent process/connection, wire up reader thread that
    /// sends AgentEvents to the returned channel.
    fn spawn(
        &self,
        config: &BackendSpawnConfig,
        proxy: EventLoopProxy<AppEvent>,
        generation: u64,
    ) -> Result<AgentHandle, BackendError>;

    /// Format a user message for this backend's wire protocol.
    fn format_message(&self, content: &str) -> String;

    /// Format an activity event for this backend's wire protocol.
    fn format_activity(&self, event: &ActivityEvent) -> String;
}
```

**Key decisions:**
- `AgentHandle` is a concrete struct, not a trait object. Provider-specific logic is entirely inside `spawn()`. After spawn, `main.rs` interacts with `AgentHandle` uniformly.
- `format_message` / `format_activity` are on the trait because wire formats differ (Claude CLI wraps in `{"type":"user","message":{...}}`, API backends use Messages API format).
- The reader thread is internal to each backend's `spawn()` — it parses provider-specific output and sends `AgentEvent` through the channel.

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

### Provider resolution

```rust
fn resolve_backend(config: &GlassConfig) -> Box<dyn AgentBackend> {
    let provider = config.agent.provider.as_deref().unwrap_or("claude-code");
    let model = config.agent.model.as_deref().unwrap_or("");
    let api_key = config.agent.api_key.as_deref()
        .or_else(|| env_key_for_provider(provider));
    let endpoint = config.agent.api_endpoint.as_deref().unwrap_or("");

    match provider {
        "claude-code"    => Box::new(ClaudeCliBackend::new()),
        "anthropic-api"  => Box::new(AnthropicApiBackend::new(api_key, model, endpoint)),
        "openai-api"     => Box::new(OpenAiBackend::new(api_key, model, endpoint)),
        "ollama"         => Box::new(OllamaBackend::new(model, endpoint)),
        "custom"         => Box::new(OpenAiBackend::new(api_key, model, endpoint)),
        _                => Box::new(ClaudeCliBackend::new()),
    }
}

fn env_key_for_provider(provider: &str) -> Option<String> {
    match provider {
        "anthropic-api" => std::env::var("ANTHROPIC_API_KEY").ok(),
        "openai-api"    => std::env::var("OPENAI_API_KEY").ok(),
        "ollama"        => None, // no auth needed
        "custom"        => std::env::var("GLASS_API_KEY").ok(),
        _               => None,
    }
}
```

### Auth

Auth is a single `api_key` field + env var fallback. No OAuth client, no bearer token management.

- `claude-code` provider: CLI handles its own auth (OAuth). Zero config.
- API providers: Glass checks the standard env var (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`). Users who use these APIs already have these set.
- Env var not found + no `api_key` in config: toast message telling user what to set.
- Power users: `api_key = "sk-..."` in config as override.

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
│ ...                                             │
└─────────────────────────────────────────────────┘
```

Left/right arrows cycle through a flat list of available `Provider / Model` combinations. Only providers with valid credentials appear. Selection auto-updates `provider` and `model` in `config.toml` via hot-reload.

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
- Reader thread with full JSON parser (stream-json → `AgentEvent`)
- Writer thread with cooldown gating
- Coordination DB registration

**What stays in `main.rs`:**
- `AgentRuntime` struct — holds `AgentHandle` instead of raw child/writer
- `try_spawn_agent()` — becomes thin: `resolve_backend()` → `backend.spawn()` → wrap in `AgentRuntime`
- Event routing: drain `handle.event_rx`, map `AgentEvent` → `AppEvent`
- Respawn logic in `respawn_orchestrator_agent()`
- System prompt assembly

**What stays in `glass_core::agent_runtime`:**
- `AgentRuntimeConfig`, `AgentMode`, `CooldownTracker`, `BudgetTracker` — unchanged
- `extract_proposal()`, `extract_handoff()`, `classify_proposal()` — parse agent text, not wire protocol
- `should_send_in_mode()`, `should_quiet()` — config logic

**New config fields added (all defaulting to current behavior):**
- `provider`, `model`, `api_key`, `api_endpoint`
- `implementer`, `implementer_command`, `implementer_name`, `persona`

**Crash recovery update:** Line 6312 uses `implementer_launch_command()` instead of hardcoded `claude` command.

**System prompt update:** Replace hardcoded "Claude Code" with `{implementer_name}`. Insert persona layer.

**Safety protocol:**
1. Write regression tests BEFORE moving any code — sample Claude CLI JSON → expected `AgentEvent` sequence
2. Extract as a line-for-line move — diff should show moved lines, not changed lines
3. End-to-end verification after extraction: Ctrl+Shift+O, run several iterations, verify activity overlay, checkpoint/respawn
4. The only change in behavior: `AgentEvent` as an intermediate step before `AppEvent` (one extra hop, functionally identical)

### Phase 2: OpenAiCompatibleBackend (additive)

**Scope:** New backend behind config gate. Cannot affect Claude Code path.

**New file:** `crates/glass_agent_backend/openai.rs`

**Implementation:**
- HTTP client (reqwest) for `/v1/chat/completions` with SSE streaming
- Tool calling: send tool definitions in request, parse `tool_calls` in response, execute Glass MCP tools in-process, send results back as tool messages
- Response normalization: OpenAI SSE chunks → `AgentEvent`
- Cost tracking from usage tokens in response

**Model list fetch:**
- `GET /v1/models` with 24h file cache in `~/.glass/cache/`
- Filter: exclude embedding, TTS, image, moderation models
- Display name mapping for known models

**Settings overlay additions:**
- Orchestrator/Implementer fields cycle through available models
- Provider auto-detection from env vars at startup

**Covers:** OpenAI (GPT-4o, o3, etc.), Google Gemini (OpenAI-compat mode), any OpenAI-compatible local server (vLLM, llama.cpp server, LM Studio)

**Tool calling is the biggest work item.** The Claude CLI handles MCP tool execution internally. API backends need Glass to:
1. Include tool definitions in the request
2. Parse `tool_calls` from the response
3. Execute the MCP tool (Glass already has the MCP server running)
4. Send the tool result back as the next message
5. Loop until the model produces a final text response

### Phase 3: AnthropicApiBackend + OllamaBackend (additive)

**Scope:** Two more backends behind config gate.

**New files:**
- `crates/glass_agent_backend/anthropic.rs`
- `crates/glass_agent_backend/ollama.rs`

**AnthropicApiBackend:**
- Anthropic Messages API (`/v1/messages`) with SSE streaming
- Native `tool_use` content blocks and `thinking` blocks (better thinking support than OpenAI-compat)
- For users who want Claude without installing the CLI

**OllamaBackend:**
- Native Ollama API (`/api/chat`) with streaming
- Tool support via Ollama's function calling
- Model list from `GET /api/tags` (local, instant)
- Default endpoint `http://localhost:11434`, configurable via `api_endpoint`

**Settings overlay:** All providers/models in unified cycle list.

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| Phase 1 extraction breaks Claude CLI path | Low | High | Regression tests + line-for-line move |
| New backend has bugs | Medium | Zero for existing users | Config gate — default path never executes new code |
| Model list fetch slows startup | Low | Low | Fetch only on settings overlay open, not startup |
| Trait interface needs rework | Medium | Low | Only affects new backends, Claude CLI path is stable |
| Tool calling implementation has bugs | Medium | Medium | Only affects Phase 2+ backends, behind config gate |

## Files Affected

### Phase 1 (modified)
- `Cargo.toml` — add `glass_agent_backend` to workspace
- `src/main.rs` — slim down `try_spawn_agent()`, `AgentRuntime` holds `AgentHandle`, event routing loop
- `crates/glass_core/src/config.rs` — new fields: `provider`, `model`, `api_key`, `api_endpoint`, `implementer`, `implementer_command`, `implementer_name`, `persona`
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
- `crates/glass_agent_backend/src/lib.rs` — register OpenAI backend in `resolve_backend()`
- `crates/glass_renderer/src/settings_overlay.rs` — model picker with dynamic list

### Phase 3 (new)
- `crates/glass_agent_backend/src/anthropic.rs`
- `crates/glass_agent_backend/src/ollama.rs`

### Phase 3 (modified)
- `crates/glass_agent_backend/src/lib.rs` — register Anthropic + Ollama backends

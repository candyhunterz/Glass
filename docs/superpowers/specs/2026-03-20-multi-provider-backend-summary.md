# Multi-Provider Agent Backend — Implementation Summary

**Date:** 2026-03-20
**Status:** Complete (all 3 phases shipped)
**Branch:** master

## What We Did

Extracted Glass's orchestrator agent backend from a hardcoded Claude Code CLI dependency into a pluggable trait-based architecture supporting 5 LLM providers. Users can now switch between cloud models (Claude, GPT, Gemini) and local models (Ollama, any OpenAI-compatible server) by changing one config field.

### The Problem

The orchestrator mode was tightly coupled to Claude Code CLI. The spawn logic, I/O protocol, and JSON parsing were hardcoded in `main.rs` (~660 lines). Users who wanted to use other models had no path forward without rewriting the agent lifecycle code.

### The Solution

A new `glass_agent_backend` crate with an `AgentBackend` trait that normalizes all provider-specific details behind a uniform `AgentEvent` stream. Five backend implementations, one trait interface:

```
┌─────────────────────────────────────┐
│           main.rs                    │
│  Orchestrator state machine         │
│  Drains AgentEvent, maps to AppEvent│
└──────────────┬──────────────────────┘
               │
     ┌─────────▼─────────┐
     │  resolve_backend() │
     └─────────┬─────────┘
               │
  ┌────────────┼────────────┬────────────┐
  │            │            │            │
┌─▼──┐  ┌─────▼─────┐  ┌───▼───┐  ┌────▼────┐
│CLI │  │ OpenAI API │  │Anthro │  │ Ollama  │
│    │  │ + Custom   │  │ pic   │  │         │
└────┘  └───────────┘  └───────┘  └─────────┘
```

## Three Phases

### Phase 1: Extract ClaudeCliBackend (pure refactor)

Moved ~660 lines of spawn/reader/writer logic from `main.rs` into `ClaudeCliBackend` behind the `AgentBackend` trait. Zero behavior change — every code path produces identical results.

**Key changes:**
- New `AgentRuntime` struct holds `AgentHandle` + `Box<dyn AgentBackend>` instead of raw child process/stdin writer
- `Drop for AgentRuntime` calls `backend.shutdown(token)` via `ShutdownToken` (type-erased per-spawn state)
- Event drain thread bridges `AgentEvent` → `AppEvent` (replacing direct proxy sends from the old reader thread)
- Activity stream bridge thread preserves Watch/Assist/Autonomous agent modes
- System prompt parameterized with `{implementer_name}` and optional persona layer
- Crash recovery uses configurable `implementer_launch_command()` instead of hardcoded `claude`

### Phase 2: OpenAI-Compatible Backend (additive)

New `OpenAiBackend` that works with any OpenAI-compatible API endpoint — OpenAI, Gemini (compat mode), vLLM, llama.cpp server, LM Studio.

**Key changes:**
- SSE stream parser (`parse_sse_line`) normalizes OpenAI streaming chunks to `SseChunk` variants
- Single conversation thread manages history, HTTP requests, and tool calling loop
- `SyncIpcClient` provides blocking IPC for calling Glass MCP tools from OS threads
- `resolve_backend()` factory routes provider config to the correct backend
- Model list caching with 24h file TTL and friendly display names

### Phase 3: Anthropic API + Ollama (additive)

Two more backends completing the provider lineup.

**AnthropicBackend:**
- Anthropic Messages API with SSE streaming (`/v1/messages`)
- Native `thinking` content blocks → `AgentEvent::Thinking`
- `tool_use`/`tool_result` content blocks (not OpenAI's `tool_calls` format)
- `x-api-key` + `anthropic-version` headers
- Default model: `claude-sonnet-4-6`

**OllamaBackend:**
- Native Ollama API (`/api/chat`) with JSON line streaming (simpler than SSE)
- No authentication required
- Model list from `GET /api/tags`
- Zero cost tracking (local models are free)
- Default endpoint: `http://localhost:11434`

## Files Changed

### New Files (8)

| File | Lines | Purpose |
|------|-------|---------|
| `crates/glass_agent_backend/Cargo.toml` | 25 | Crate manifest |
| `crates/glass_agent_backend/src/lib.rs` | 323 | `AgentEvent`, `AgentBackend` trait, `AgentHandle`, `ShutdownToken`, `BackendSpawnConfig`, `BackendError`, `resolve_backend()` |
| `crates/glass_agent_backend/src/claude_cli.rs` | 793 | `ClaudeCliBackend` — extracted from main.rs, stream-json parsing, process lifecycle |
| `crates/glass_agent_backend/src/openai.rs` | 679 | `OpenAiBackend` — SSE streaming, conversation management, tool calling loop |
| `crates/glass_agent_backend/src/anthropic.rs` | 739 | `AnthropicBackend` — Anthropic Messages API, thinking blocks, tool_use |
| `crates/glass_agent_backend/src/ollama.rs` | 644 | `OllamaBackend` — JSON line streaming, /api/tags model list |
| `crates/glass_agent_backend/src/ipc_tools.rs` | 196 | `SyncIpcClient` — blocking IPC for MCP tool execution from OS threads |
| `crates/glass_agent_backend/src/model_cache.rs` | 304 | Model list fetching, 24h file cache, friendly display names |

### Modified Files (8)

| File | Change |
|------|--------|
| `Cargo.toml` | Added `glass_agent_backend` dependency |
| `config.example.toml` | Documented `provider`, `model`, `api_key`, `api_endpoint`, `implementer`, `implementer_command`, `implementer_name`, `persona` fields |
| `crates/glass_core/src/config.rs` | Added 8 new config fields to `AgentSection` and `OrchestratorSection` |
| `crates/glass_core/src/agent_runtime.rs` | Deprecated `build_agent_command_args()` (moved to claude_cli.rs) |
| `crates/glass_renderer/src/settings_overlay.rs` | Added provider, model, persona, implementer display fields |
| `src/main.rs` | Replaced `AgentRuntime` struct, `Drop`, `try_spawn_agent()` (~660→~200 lines), event drain thread, activity bridge, `resolve_backend()` integration, `implementer_launch_command()`, system prompt parameterization |
| `src/orchestrator.rs` | Added `resolved_mode`, `resolved_verify_mode` fields (pre-existing bug fix) |
| `src/tests.rs` | Fixed orchestrator mode default assertion (`"build"` → `"auto"`) |

### Stats

- **+4,308 lines added, -704 lines removed** (net +3,604)
- **78 new tests** in `glass_agent_backend`
- **16 files touched** across 3 phases
- **27 commits** (including merges, docs, and fixes)

## Configuration Reference

### Provider Selection

```toml
[agent]
provider = "claude-code"      # Default — Claude Code CLI (uses CLI's own OAuth)
# provider = "anthropic-api"  # Anthropic Messages API (needs ANTHROPIC_API_KEY)
# provider = "openai-api"     # OpenAI-compatible API (needs OPENAI_API_KEY)
# provider = "ollama"         # Local Ollama (no auth, localhost:11434)
# provider = "custom"         # Any OpenAI-compatible endpoint (optional GLASS_API_KEY)

model = ""                    # Empty = provider default. Examples: "gpt-4o", "claude-opus-4-6", "llama3:70b"
api_key = ""                  # Optional — env var takes precedence (OPENAI_API_KEY, ANTHROPIC_API_KEY)
api_endpoint = ""             # Optional — for custom/self-hosted endpoints
```

### Implementer Configuration

```toml
[agent.orchestrator]
implementer = "claude-code"        # What CLI runs in the terminal: "claude-code", "codex", "aider", "gemini", "custom"
implementer_command = ""           # Custom launch command (when implementer = "custom")
implementer_name = "Claude Code"   # Display name in system prompt (auto-replaces all references)
persona = ""                       # Inline persona string, or path to .md file
```

### Auth Resolution Order

1. Environment variable (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GLASS_API_KEY`)
2. Config file `api_key` field
3. Error toast if neither found (for providers that require auth)
4. `claude-code` and `ollama` require no auth config

### Default Models Per Provider

| Provider | Default Model | Default Endpoint |
|----------|--------------|-----------------|
| `claude-code` | (CLI decides) | N/A |
| `anthropic-api` | `claude-sonnet-4-6` | `https://api.anthropic.com` |
| `openai-api` | `gpt-4o` | `https://api.openai.com` |
| `ollama` | `llama3` | `http://localhost:11434` |
| `custom` | `gpt-4o` | (must be set) |

## Updated Workflow

### Default User (no config changes)

Identical to before. `provider` defaults to `"claude-code"`, everything works as it always has.

### Using GPT as Orchestrator + Claude Code as Implementer

```bash
# Set your OpenAI API key
export OPENAI_API_KEY="sk-..."

# Add to ~/.glass/config.toml
# [agent]
# provider = "openai-api"
# model = "gpt-4o"

# Launch Glass, open a project, press Ctrl+Shift+O
# The orchestrator uses GPT-4o, types instructions to Claude Code in the terminal
```

### Using Claude API (no CLI installation needed)

```bash
export ANTHROPIC_API_KEY="sk-ant-..."

# [agent]
# provider = "anthropic-api"
# model = "claude-opus-4-6"
```

### Using Local Models via Ollama

```bash
# Start Ollama
ollama serve

# Pull a model
ollama pull llama3:70b

# [agent]
# provider = "ollama"
# model = "llama3:70b"
```

### Using a Custom OpenAI-Compatible Server

```bash
# Start your server (vLLM, llama.cpp, LM Studio, etc.)

# [agent]
# provider = "custom"
# api_endpoint = "http://localhost:8080"
# model = "my-custom-model"
```

### Mixing Orchestrator and Implementer Models

The orchestrator (reviewer/guide) and implementer (code writer) are independent:

```toml
[agent]
provider = "openai-api"          # GPT orchestrates
model = "gpt-4o"

[agent.orchestrator]
implementer = "claude-code"      # Claude Code implements
implementer_name = "Claude Code"
```

Or use a local model as orchestrator with Claude Code as implementer:

```toml
[agent]
provider = "ollama"
model = "llama3:70b"

[agent.orchestrator]
implementer = "claude-code"
```

### Custom Persona

```toml
[agent.orchestrator]
persona = "You are a senior systems architect. Be concise. Prioritize correctness over speed."
# Or load from file:
# persona = ".glass/agent-persona.md"
```

### Settings Overlay (Ctrl+Shift+S)

The Orchestrator section now shows:
- **Provider** — current provider name
- **Model** — current model
- **Implementer** — which CLI is the implementer
- **Persona** — custom persona or "(default)"

These are read-only display in the current release. Interactive cycling with dynamic model lists from provider APIs is planned for a future update.

## Architecture Details

### AgentEvent — The Normalization Layer

Every backend translates its provider-specific format into these 7 event types:

```rust
enum AgentEvent {
    Init { session_id },           // Session established
    AssistantText { text },        // Agent produced text (GLASS_WAIT, instructions, etc.)
    Thinking { text },             // Extended thinking / reasoning
    ToolCall { name, id, input },  // Agent calling an MCP tool
    ToolResult { tool_use_id, content },  // Tool result returned
    TurnComplete { cost_usd },     // Conversation turn finished
    Crashed,                       // Backend died unexpectedly
}
```

`main.rs` drains `AgentEvent` from the handle's channel and maps to `AppEvent` — it never sees provider-specific JSON.

### Tool Calling

API backends (OpenAI, Anthropic, Ollama) execute Glass MCP tools via IPC:

```
Backend conversation thread
    ↓ model requests tool call
SyncIpcClient.call_tool("glass_query", params)
    ↓ connects to Glass GUI IPC listener
Unix socket (~/.glass/glass.sock) / Named pipe (\\.\pipe\glass-terminal)
    ↓ Glass GUI executes the MCP tool
Result returned to backend
    ↓ appended to conversation history
Next API request with tool result
```

Max 10 tool-call rounds per conversation turn to prevent infinite loops.

### ShutdownToken — Per-Spawn State

Each `spawn()` creates a `ShutdownToken` wrapping backend-specific cleanup state:
- **Claude CLI:** child process handle, coordination DB agent ID and nonce
- **API backends:** `Arc<AtomicBool>` stop flag

`Drop for AgentRuntime` extracts the token and calls `backend.shutdown(token)`, ensuring cleanup on all code paths (explicit drop, respawn, deactivation).

### Model Caching

`model_cache::fetch_models()` queries provider `/v1/models` endpoints (or `/api/tags` for Ollama) and caches results to `~/.glass/cache/models/{provider}.json` with a 24-hour TTL. Falls back to stale cache on network failure. Non-chat models (embeddings, TTS, image gen, moderation) are filtered out automatically.

## Testing

78 tests in `glass_agent_backend`:

| Module | Tests | What's covered |
|--------|-------|---------------|
| `claude_cli` | 17 | Stream-json parsing, CLI arg building, backend name |
| `openai` | 13 | SSE parsing (text, tool, reasoning, done, usage), backend defaults |
| `anthropic` | 14 | Anthropic SSE parsing (text, thinking, tool_use, message events), backend defaults |
| `ollama` | 19 | JSON line parsing (text, tool calls, done), backend defaults, model fetching |
| `ipc_tools` | 2 | Client creation, connection failure handling |
| `model_cache` | 6 | Friendly names, model list filtering, edge cases |
| `lib` (resolve) | 7 | Provider routing, credential checking, fallback behavior |

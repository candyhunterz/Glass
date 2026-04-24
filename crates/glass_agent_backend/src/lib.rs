//! `glass_agent_backend` — multi-provider LLM backend abstraction for Glass Agent.
//!
//! Defines the [`AgentBackend`] trait and the shared types used when spawning
//! and communicating with agent subprocesses regardless of provider.

use std::fmt;
use std::sync::mpsc;

pub use glass_core::agent_runtime::AgentMode;

pub mod anthropic;
pub mod claude_cli;
pub mod ipc_tools;
pub mod model_cache;
pub mod ollama;
pub mod openai;

// ── Shared conversation config ────────────────────────────────────────────────

/// Configuration shared across all API-based backend conversation loops.
///
/// Groups the parameters that are identical in shape across the Ollama,
/// OpenAI, and Anthropic backends so each `conversation_loop` / `do_turn`
/// can take a single `&ConversationConfig` instead of 6-8 separate args.
pub(crate) struct ConversationConfig {
    /// API key / credential (empty for Ollama which needs no auth).
    pub api_key: String,
    /// Model identifier (e.g. `"gpt-4o"`, `"claude-sonnet-4-6"`, `"llama3"`).
    pub model: String,
    /// Base URL for the API endpoint (no trailing slash).
    pub endpoint: String,
    /// System prompt injected at session start.
    pub system_prompt: String,
    /// Optional first user message sent immediately after spawn.
    pub initial_message: Option<String>,
    /// MCP / built-in tool names the agent is permitted to call.
    pub allowed_tools: Vec<String>,
    /// Monotonically increasing session counter.
    pub generation: u64,
}

// ── Events ───────────────────────────────────────────────────────────────────

/// Normalized events emitted by any backend implementation.
///
/// All backend implementations translate their provider-specific stream format
/// into this common enum so the rest of the orchestrator can remain
/// provider-agnostic.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// The session has been established; carries the backend-assigned session id.
    Init { session_id: String },
    /// A chunk of assistant text was produced.
    AssistantText { text: String },
    /// An extended thinking block was produced.
    Thinking { text: String },
    /// The agent is calling an MCP / built-in tool.
    ToolCall {
        /// Human-readable tool name (e.g. `"Bash"`, `"Read"`).
        name: String,
        /// Opaque call id used to correlate the result.
        id: String,
        /// JSON-encoded tool input.
        input: String,
    },
    /// The tool result returned to the agent.
    ToolResult {
        /// Matches the `id` from the corresponding [`AgentEvent::ToolCall`].
        tool_use_id: String,
        /// Tool output content.
        content: String,
    },
    /// The agent turn has finished; carries the estimated cost for this turn.
    TurnComplete { cost_usd: f64 },
    /// The backend process exited unexpectedly.
    Crashed,
}

// ── Spawn configuration ───────────────────────────────────────────────────────

/// All parameters required to spawn a new backend session.
#[derive(Debug, Clone)]
pub struct BackendSpawnConfig {
    /// System prompt injected at session start.
    pub system_prompt: String,
    /// Optional first user message sent immediately after spawn.
    pub initial_message: Option<String>,
    /// Absolute path of the project the agent operates on.
    pub project_root: String,
    /// Path to the MCP configuration file passed to the backend.
    pub mcp_config_path: String,
    /// MCP / built-in tool names the agent is permitted to call.
    pub allowed_tools: Vec<String>,
    /// Operating mode (controls which events are forwarded to the agent).
    pub mode: AgentMode,
    /// Minimum seconds between forwarded events (cooldown gate).
    pub cooldown_secs: u64,
    /// How many times this session has been restarted after a crash.
    pub restart_count: u32,
    /// When the most recent crash occurred, if any.
    pub last_crash: Option<std::time::Instant>,
}

// ── Shutdown token ────────────────────────────────────────────────────────────

/// Opaque, type-erased container for per-spawn shutdown state.
///
/// Each [`AgentBackend`] implementation stores whatever cancellation handle or
/// sentinel it needs inside a `ShutdownToken`.  The orchestrator holds onto the
/// token and passes it back to [`AgentBackend::shutdown`] when it wants to stop
/// a running session.
pub struct ShutdownToken {
    inner: Box<dyn std::any::Any + Send>,
}

impl ShutdownToken {
    /// Wrap `data` in a new token.
    pub fn new<T: Send + 'static>(data: T) -> Self {
        Self {
            inner: Box::new(data),
        }
    }

    /// Borrow the inner value as `&T`, returning `None` if the stored type
    /// does not match.
    pub fn downcast<T: 'static>(&self) -> Option<&T> {
        self.inner.downcast_ref::<T>()
    }

    /// Mutably borrow the inner value as `&mut T`, returning `None` if the
    /// stored type does not match.
    pub fn downcast_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.inner.downcast_mut::<T>()
    }
}

impl fmt::Debug for ShutdownToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShutdownToken").finish_non_exhaustive()
    }
}

// ── Agent handle ─────────────────────────────────────────────────────────────

/// Live handle to a running backend session.
///
/// The orchestrator uses `message_tx` to inject messages into the agent and
/// reads `event_rx` to consume the normalized event stream.  `AgentHandle`
/// intentionally does **not** implement `Clone` because the channel endpoints
/// are single-consumer / single-producer.
pub struct AgentHandle {
    /// Send user / system messages to the running agent.
    pub message_tx: mpsc::Sender<String>,
    /// Receive normalized events produced by the agent.
    pub event_rx: mpsc::Receiver<AgentEvent>,
    /// Monotonically increasing counter; incremented on each restart so stale
    /// responses can be discarded.
    pub generation: u64,
    /// Backend-specific shutdown state returned to [`AgentBackend::shutdown`].
    pub shutdown_token: ShutdownToken,
}

impl fmt::Debug for AgentHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentHandle")
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

// ── Backend errors ────────────────────────────────────────────────────────────

/// Errors that can occur when spawning or communicating with a backend.
#[derive(Debug)]
pub enum BackendError {
    /// The required API key or credential was not set in the environment.
    MissingCredentials {
        /// Provider name (e.g. `"Claude CLI"`, `"OpenAI"`).
        provider: String,
        /// Environment variable that should contain the credential.
        env_var: String,
    },
    /// The backend executable could not be found on `PATH`.
    BinaryNotFound {
        /// The binary name that was searched for.
        binary: String,
    },
    /// The backend process could not be spawned (OS error or similar).
    SpawnFailed(String),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendError::MissingCredentials { provider, env_var } => write!(
                f,
                "{provider} backend requires credentials — set the {env_var} environment variable"
            ),
            BackendError::BinaryNotFound { binary } => {
                write!(f, "backend binary '{binary}' not found on PATH")
            }
            BackendError::SpawnFailed(msg) => write!(f, "failed to spawn backend process: {msg}"),
        }
    }
}

impl std::error::Error for BackendError {}

// ── Backend factory ──────────────────────────────────────────────────────────

/// Resolve the appropriate backend from provider config.
///
/// Checks env vars first (takes precedence), then falls back to `api_key` param.
/// Returns `Err(BackendError::MissingCredentials)` if an API provider is
/// selected but no credentials are found.
pub fn resolve_backend(
    provider: &str,
    model: &str,
    api_key: Option<&str>,
    api_endpoint: Option<&str>,
) -> Result<Box<dyn AgentBackend>, BackendError> {
    let endpoint = api_endpoint.unwrap_or("");

    match provider {
        "claude-code" | "" => Ok(Box::new(claude_cli::ClaudeCliBackend::new())),
        "anthropic-api" => {
            let key = std::env::var("ANTHROPIC_API_KEY")
                .ok()
                .or_else(|| api_key.map(|s| s.to_string()))
                .ok_or_else(|| BackendError::MissingCredentials {
                    provider: "anthropic-api".into(),
                    env_var: "ANTHROPIC_API_KEY".into(),
                })?;
            Ok(Box::new(anthropic::AnthropicBackend::new(
                &key, model, endpoint,
            )))
        }
        "openai-api" => {
            let key = std::env::var("OPENAI_API_KEY")
                .ok()
                .or_else(|| api_key.map(|s| s.to_string()))
                .ok_or_else(|| BackendError::MissingCredentials {
                    provider: "openai-api".into(),
                    env_var: "OPENAI_API_KEY".into(),
                })?;
            Ok(Box::new(openai::OpenAiBackend::new(&key, model, endpoint)))
        }
        "ollama" => {
            // Ollama does not require auth — just model + endpoint.
            Ok(Box::new(ollama::OllamaBackend::new(model, endpoint)))
        }
        "custom" => {
            // Custom endpoints may not require auth (e.g., local vLLM)
            let key = std::env::var("GLASS_API_KEY")
                .ok()
                .or_else(|| api_key.map(|s| s.to_string()))
                .unwrap_or_default();
            Ok(Box::new(openai::OpenAiBackend::new(&key, model, endpoint)))
        }
        _ => Ok(Box::new(claude_cli::ClaudeCliBackend::new())),
    }
}

// ── Backend trait ─────────────────────────────────────────────────────────────

/// Abstraction over a concrete LLM provider backend.
///
/// Implementors are responsible for:
/// - Spawning the underlying process (or connection).
/// - Translating provider-specific output into [`AgentEvent`]s.
/// - Cleanly stopping the session when [`shutdown`](AgentBackend::shutdown) is called.
pub trait AgentBackend: Send + Sync {
    /// Short human-readable name for this provider (e.g. `"claude-cli"`).
    fn name(&self) -> &str;

    /// Spawn a new agent session and return a live [`AgentHandle`].
    ///
    /// `generation` must be stored in the returned handle so the orchestrator
    /// can detect stale events after a restart.
    fn spawn(
        &self,
        config: &BackendSpawnConfig,
        generation: u64,
    ) -> Result<AgentHandle, BackendError>;

    /// Cleanly shut down the session associated with `token`.
    ///
    /// The token was originally provided inside the [`AgentHandle`] returned by
    /// [`spawn`](AgentBackend::spawn).  Implementations should use it to locate
    /// and terminate the underlying process or task.
    fn shutdown(&self, token: ShutdownToken);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_default_is_claude_cli() {
        let b = resolve_backend("", "", None, None).unwrap();
        assert_eq!(b.name(), "Claude CLI");
    }

    #[test]
    fn resolve_claude_code_explicit() {
        let b = resolve_backend("claude-code", "", None, None).unwrap();
        assert_eq!(b.name(), "Claude CLI");
    }

    #[test]
    fn resolve_unknown_falls_back_to_claude_cli() {
        let b = resolve_backend("unknown-provider", "", None, None).unwrap();
        assert_eq!(b.name(), "Claude CLI");
    }

    #[test]
    fn resolve_openai_without_key_errors() {
        // Temporarily unset the env var to ensure it's not set
        let _guard = std::env::var("OPENAI_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");
        let result = resolve_backend("openai-api", "", None, None);
        assert!(result.is_err());
        // Restore if it was set
        if let Ok(val) = _guard {
            std::env::set_var("OPENAI_API_KEY", val);
        }
    }

    #[test]
    fn resolve_openai_with_config_key() {
        let b = resolve_backend("openai-api", "gpt-4o", Some("sk-test"), None).unwrap();
        assert_eq!(b.name(), "OpenAI API");
    }

    #[test]
    fn resolve_ollama() {
        let b = resolve_backend("ollama", "llama3:70b", None, None).unwrap();
        assert_eq!(b.name(), "Ollama");
    }

    #[test]
    fn resolve_custom_allows_empty_key() {
        let b =
            resolve_backend("custom", "local-model", None, Some("http://localhost:8080")).unwrap();
        assert_eq!(b.name(), "OpenAI API");
    }
}

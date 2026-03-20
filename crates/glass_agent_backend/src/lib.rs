//! `glass_agent_backend` — multi-provider LLM backend abstraction for Glass Agent.
//!
//! Defines the [`AgentBackend`] trait and the shared types used when spawning
//! and communicating with agent subprocesses regardless of provider.

use std::fmt;
use std::sync::mpsc;

pub use glass_core::agent_runtime::AgentMode;

pub mod claude_cli;
pub mod ipc_tools;
pub mod model_cache;
pub mod openai;

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

use serde::{Deserialize, Serialize};

/// Information about a registered agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Unique agent identifier (UUID v4).
    pub id: String,
    /// Human-readable agent name (e.g. "claude-code-1").
    pub name: String,
    /// Agent type (e.g. "claude-code", "cursor", "human").
    pub agent_type: String,
    /// Canonicalized project root path.
    pub project: String,
    /// Current working directory.
    pub cwd: String,
    /// Operating system PID, if available.
    pub pid: Option<u32>,
    /// Agent status (e.g. "active", "idle", "editing").
    pub status: String,
    /// Current task description, if any.
    pub task: Option<String>,
    /// Unix timestamp when the agent registered.
    pub registered_at: i64,
    /// Unix timestamp of the last heartbeat.
    pub last_heartbeat: i64,
}

/// A file lock held by an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileLock {
    /// Canonicalized file path.
    pub path: String,
    /// ID of the agent holding the lock.
    pub agent_id: String,
    /// Name of the agent holding the lock.
    pub agent_name: String,
    /// Optional reason for the lock.
    pub reason: Option<String>,
    /// Unix timestamp when the lock was acquired.
    pub locked_at: i64,
}

/// Describes a lock conflict when acquisition fails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockConflict {
    /// Canonicalized file path that is already locked.
    pub path: String,
    /// ID of the agent holding the lock.
    pub held_by_agent_id: String,
    /// Name of the agent holding the lock.
    pub held_by_agent_name: String,
    /// Optional reason the holder gave for the lock.
    pub reason: Option<String>,
}

/// Result of a lock acquisition attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LockResult {
    /// Lock acquired successfully; contains the canonical paths acquired.
    Acquired(Vec<String>),
    /// Lock acquisition failed due to conflicts.
    Conflict(Vec<LockConflict>),
}

/// A message between agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Database row ID.
    pub id: i64,
    /// ID of the sending agent (None if sender deregistered).
    pub from_agent: Option<String>,
    /// Name of the sending agent (None if sender deregistered).
    pub from_name: Option<String>,
    /// ID of the receiving agent (None for broadcasts).
    pub to_agent: Option<String>,
    /// Message type (e.g. "file_saved", "conflict", "chat").
    pub msg_type: String,
    /// Message content (typically JSON).
    pub content: String,
    /// Unix timestamp when the message was created.
    pub created_at: i64,
}

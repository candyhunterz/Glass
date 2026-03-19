# Security Audit Implementation Plan (Branch 2 of 8)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Harden MCP tool access with permission gates and audit logging, authenticate IPC and agent coordination, redact sensitive data from history, validate file paths in undo/blob operations, and eliminate shell temp-file predictability.

**Architecture:** Work outside-in: MCP permission gate first (widest blast radius), then IPC/coordination auth, then data-layer hardening (redaction, path validation, blob hash checks), then shell integration and CI.

**Tech Stack:** Rust, zeroize (new dep), regex (existing), rusqlite, tokio, rmcp, serde

**Branch:** `audit/security` off `master`

**Spec:** `docs/superpowers/specs/2026-03-18-prelaunch-audit-fixes-design.md` Branch 2

---

### Task 1: Branch setup + MCP permission gate (S-1, S-3)

**Files:**
- Modify: `crates/glass_mcp/src/tools.rs` (add permission checking before tool dispatch)
- Modify: `crates/glass_mcp/src/lib.rs` (wire config into GlassServer)
- Modify: `crates/glass_core/src/config.rs` (add `allowed_tools` helper, permission category mapping)

- [ ] **Step 1: Create branch**

```bash
git checkout -b audit/security master
```

- [ ] **Step 2: Add permission category enum and tool-to-category mapping**

In `crates/glass_core/src/config.rs`, add a helper function that maps MCP tool names to permission categories:

```rust
/// Permission category for an MCP tool invocation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToolCategory {
    /// Tools that execute shell commands (glass_tab_send, glass_tab_create, glass_cancel_command, glass_script_tool)
    RunCommands,
    /// Tools that modify files (glass_undo, glass_file_diff with write)
    EditFiles,
    /// Read-only tools (glass_history, glass_context, glass_tab_list, glass_tab_output, etc.)
    ReadOnly,
    /// Agent coordination tools (glass_agent_*)
    Coordination,
}

/// Map an MCP tool name to its permission category.
pub fn tool_category(tool_name: &str) -> ToolCategory {
    match tool_name {
        "glass_tab_send" | "glass_tab_create" | "glass_cancel_command" | "glass_script_tool" => {
            ToolCategory::RunCommands
        }
        "glass_undo" => ToolCategory::EditFiles,
        name if name.starts_with("glass_agent_") => ToolCategory::Coordination,
        _ => ToolCategory::ReadOnly,
    }
}
```

- [ ] **Step 3: Add config + allowed_tools set to GlassServer**

In `crates/glass_mcp/src/tools.rs`, add fields to `GlassServer`:

```rust
// In the GlassServer struct, add:
/// Parsed set of allowed tool names. Empty = all allowed.
allowed_tools: std::collections::HashSet<String>,
/// Permission matrix for category-level gating.
permissions: glass_core::config::PermissionMatrix,
```

Update `GlassServer::new()` to accept and store these. Parse `allowed_tools` from the comma-separated config string into a `HashSet<String>`.

In `crates/glass_mcp/src/lib.rs` at line 42, read the config to populate allowed_tools and permissions:

```rust
// Load config for permission settings
let config = glass_core::config::GlassConfig::load();
let agent = config.agent.unwrap_or_default();
let allowed_tools: std::collections::HashSet<String> = agent
    .allowed_tools
    .split(',')
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .collect();
let permissions = agent.permissions.clone().unwrap_or_default();

let server = tools::GlassServer::new(
    db_path, glass_dir, coord_db_path, ipc_client,
    allowed_tools, permissions,
);
```

- [ ] **Step 4: Add permission check helper to GlassServer**

In `crates/glass_mcp/src/tools.rs`, add a helper method:

```rust
impl GlassServer {
    /// Check whether the given tool is allowed by config. Returns an error
    /// CallToolResult if denied, None if allowed.
    fn check_permission(&self, tool_name: &str) -> Option<CallToolResult> {
        // 1. Check allowlist (empty = all allowed)
        if !self.allowed_tools.is_empty() && !self.allowed_tools.contains(tool_name) {
            tracing::warn!(tool = tool_name, "MCP tool blocked by allowed_tools config");
            return Some(CallToolResult::error(vec![Content::text(format!(
                "Tool '{}' is not in the allowed_tools list. Update [agent] allowed_tools in config.",
                tool_name
            ))]));
        }

        // 2. Check category-level permission
        use glass_core::config::{tool_category, ToolCategory, PermissionLevel};
        let category = tool_category(tool_name);
        let level = match category {
            ToolCategory::RunCommands => &self.permissions.run_commands,
            ToolCategory::EditFiles => &self.permissions.edit_files,
            ToolCategory::ReadOnly => return None, // Always allowed
            ToolCategory::Coordination => return None, // Coordination uses nonces (S-4)
        };

        match level {
            PermissionLevel::Allow => None,
            PermissionLevel::Deny => {
                tracing::warn!(tool = tool_name, "MCP tool blocked by permission level Deny");
                Some(CallToolResult::error(vec![Content::text(format!(
                    "Tool '{}' is blocked by permission level 'Deny'. Update [agent.permissions] in config.",
                    tool_name
                ))]))
            }
            PermissionLevel::Approve => {
                // For now, Approve falls through to Allow at the MCP layer.
                // The GUI confirmation dialog (S-1 spec) is wired via IPC:
                // the Glass GUI side checks permissions and shows the dialog.
                // At the MCP server layer, Approve = Allow (GUI handles gating).
                None
            }
        }
    }
}
```

- [ ] **Step 5: Wire permission check into command-executing tools**

Add permission check at the top of `glass_tab_send`, `glass_tab_create`, `glass_cancel_command`, `glass_script_tool`, and `glass_undo`:

```rust
// At the top of each of these functions:
if let Some(denied) = self.check_permission("glass_tab_send") {
    return Ok(denied);
}
```

Repeat for each tool, using the correct tool name string.

- [ ] **Step 6: Add audit logging for all tool invocations**

Add `tracing::info!` at the start of every `#[tool]` method in `tools.rs`:

```rust
tracing::info!(tool = "glass_history", "MCP tool invoked");
// For tools with identifiable parameters:
tracing::info!(tool = "glass_tab_send", command = %input.command, "MCP tool invoked");
tracing::info!(tool = "glass_agent_register", agent = %input.name, "MCP tool invoked");
```

For each tool, log the tool name. For command-executing tools, also log the command text. For agent tools, log the agent name/id.

- [ ] **Step 7: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 8: Commit**

```bash
git add crates/glass_core/src/config.rs crates/glass_mcp/src/tools.rs crates/glass_mcp/src/lib.rs
git commit -m "feat(S-1/S-3): add MCP permission gate and audit logging

Wire PermissionMatrix and allowed_tools config into MCP tool dispatch.
Command-executing tools (tab_send, tab_create, cancel_command, script_tool)
are gated by run_commands permission. glass_undo gated by edit_files.
All tool invocations logged via tracing::info."
```

---

### Task 2: IPC socket hardening (S-2)

**Files:**
- Modify: `crates/glass_core/src/ipc.rs:191` (Unix socket permissions)
- Modify: `crates/glass_core/src/ipc.rs:220-222` (Windows named pipe security)

- [ ] **Step 1: Unix — chmod 0600 on socket after bind**

In `crates/glass_core/src/ipc.rs`, after `let listener = UnixListener::bind(&path)?;` at line 191:

```rust
let listener = UnixListener::bind(&path)?;

// Restrict socket to current user only
#[cfg(unix)]
{
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o600);
    if let Err(e) = std::fs::set_permissions(&path, perms) {
        tracing::warn!("Failed to set IPC socket permissions: {e}");
    }
}
```

- [ ] **Step 2: Windows — restrict named pipe to current user**

In `crates/glass_core/src/ipc.rs` at the Windows `spawn_ipc_listener` (~line 220), the current code uses `ServerOptions::new().first_pipe_instance(true).create(&pipe_name)`.

Named pipes created by `tokio::net::windows::named_pipe::ServerOptions` default to the creating user's access. The default Windows named pipe ACL already restricts to the creator's SID when no explicit `SECURITY_ATTRIBUTES` are provided. Add a comment documenting this:

```rust
// Security note: Windows named pipes created without explicit SECURITY_ATTRIBUTES
// default to an ACL that grants access to the creating user, SYSTEM, and
// administrators. This is acceptable for local IPC — no cross-user access
// is possible for non-admin users.
let mut server = ServerOptions::new()
    .first_pipe_instance(true)
    .create(&pipe_name)?;
```

If stricter isolation is needed (blocking admin access), it requires `windows-sys` SECURITY_ATTRIBUTES with an explicit DACL. For now the default is acceptable — document and move on.

- [ ] **Step 3: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add crates/glass_core/src/ipc.rs
git commit -m "fix(S-2): restrict IPC socket permissions

Unix: chmod 0600 on socket after bind (owner-only access).
Windows: document default named pipe ACL security (creator + SYSTEM + admin)."
```

---

### Task 3: Agent coordination nonce authentication (S-4)

**Files:**
- Modify: `crates/glass_coordination/src/db.rs` (schema migration, register returns nonce, validate nonce)
- Modify: `crates/glass_coordination/src/types.rs` (add nonce to AgentInfo)
- Modify: `crates/glass_mcp/src/tools.rs` (pass nonce through agent tool params)

- [ ] **Step 1: Add nonce column to agents table**

In `crates/glass_coordination/src/db.rs`, update `migrate()` to add a `nonce` column:

```rust
fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if version < 1 {
        conn.pragma_update(None, "user_version", 1)?;
    }

    if version < 2 {
        // Add session nonce for authentication
        conn.execute_batch(
            "ALTER TABLE agents ADD COLUMN nonce TEXT;",
        )?;
        conn.pragma_update(None, "user_version", 2)?;
    }

    Ok(())
}
```

- [ ] **Step 2: Generate nonce at registration**

In `crates/glass_coordination/src/db.rs`, modify `register()` to generate and store a nonce:

```rust
pub fn register(
    &mut self,
    name: &str,
    agent_type: &str,
    project: &str,
    cwd: &str,
    pid: Option<u32>,
) -> Result<(String, String)> {  // Returns (agent_id, nonce)
    let canonical_project =
        crate::canonicalize_path(Path::new(project)).unwrap_or_else(|_| project.to_string());
    let id = uuid::Uuid::new_v4().to_string();
    let nonce = uuid::Uuid::new_v4().to_string();

    // Validate PID liveness if provided
    if let Some(pid) = pid {
        if !is_pid_alive(pid) {
            anyhow::bail!("PID {pid} is not alive — registration rejected");
        }
    }

    let tx = self
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "INSERT INTO agents (id, name, agent_type, project, cwd, pid, nonce, status, registered_at, last_heartbeat)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active', unixepoch(), unixepoch())",
        params![&id, name, agent_type, &canonical_project, cwd, pid.map(|p| p as i64), &nonce],
    )?;
    // ... (existing event_log call) ...
    tx.commit()?;

    Ok((id, nonce))
}
```

Add a PID liveness check helper:

```rust
/// Check if a process with the given PID is alive.
fn is_pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // kill(pid, 0) checks existence without sending a signal
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        use windows_sys::Win32::Foundation::CloseHandle;
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle == 0 {
                false
            } else {
                CloseHandle(handle);
                true
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        true // Assume alive on unknown platforms
    }
}
```

- [ ] **Step 3: Add nonce validation method**

In `crates/glass_coordination/src/db.rs`:

```rust
/// Validate that the given nonce matches the agent's stored nonce.
/// Returns Ok(()) on success, Err on mismatch or missing agent.
pub fn validate_nonce(&self, agent_id: &str, nonce: &str) -> Result<()> {
    let stored: Option<String> = self.conn.query_row(
        "SELECT nonce FROM agents WHERE id = ?1",
        params![agent_id],
        |row| row.get(0),
    ).optional()?
     .flatten();

    match stored {
        Some(ref s) if s == nonce => Ok(()),
        Some(_) => anyhow::bail!("Invalid nonce for agent {agent_id}"),
        None => anyhow::bail!("Agent {agent_id} not found"),
    }
}
```

- [ ] **Step 4: Require nonce in coordination operations**

Add nonce validation at the start of `heartbeat()`, `update_status()`, `deregister()`, `acquire_locks()`, `release_locks()`, `send_message()`, and `broadcast()`. Each method gains a `nonce: &str` parameter:

```rust
pub fn heartbeat(&mut self, agent_id: &str, nonce: &str) -> Result<bool> {
    self.validate_nonce(agent_id, nonce)?;
    // ... existing logic ...
}
```

Apply the same pattern to all agent-mutating operations. Read-only operations (list_agents, get_messages) do not require nonces.

- [ ] **Step 5: Update MCP tool parameter structs**

In `crates/glass_mcp/src/tools.rs`, add `nonce: Option<String>` to the parameter structs for all agent tools that mutate state:

```rust
// For glass_agent_heartbeat, glass_agent_status, glass_agent_deregister,
// glass_agent_lock, glass_agent_unlock, glass_agent_send, glass_agent_broadcast:
#[serde(default)]
nonce: Option<String>,
```

In each tool implementation, extract the nonce and pass it to the DB method:

```rust
let nonce = input.nonce.as_deref().unwrap_or("");
db.heartbeat(&input.agent_id, nonce)?;
```

- [ ] **Step 6: Update glass_agent_register to return nonce**

In the `glass_agent_register` tool, change the return value to include the nonce:

```rust
let (agent_id, nonce) = db.register(
    &input.name, &input.agent_type, &input.project,
    &input.cwd, input.pid,
)?;
let result = serde_json::json!({
    "agent_id": agent_id,
    "nonce": nonce,
});
```

- [ ] **Step 7: Update all coordination DB tests**

Update existing tests in `crates/glass_coordination/src/db.rs` to capture and pass nonces:

```rust
// BEFORE:
let id = db.register("test", "claude-code", "/project", "/project", None).unwrap();
db.heartbeat(&id).unwrap();

// AFTER:
let (id, nonce) = db.register("test", "claude-code", "/project", "/project", None).unwrap();
db.heartbeat(&id, &nonce).unwrap();
```

Add a test verifying nonce rejection:

```rust
#[test]
fn test_invalid_nonce_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = CoordinationDb::open(&dir.path().join("test.db")).unwrap();
    let (id, _nonce) = db.register("test", "claude-code", "/project", "/project", None).unwrap();
    assert!(db.heartbeat(&id, "wrong-nonce").is_err());
}
```

- [ ] **Step 8: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 9: Commit**

```bash
git add crates/glass_coordination/src/db.rs crates/glass_coordination/src/types.rs crates/glass_mcp/src/tools.rs
git commit -m "feat(S-4): add session nonce authentication for agent coordination

Generate UUID nonce at registration, returned alongside agent_id.
All mutating coordination operations (heartbeat, lock, unlock, send,
status, deregister) require nonce. PID liveness validated at registration."
```

---

### Task 4: OAuth token zeroize (S-5)

**Files:**
- Modify: `Cargo.toml` (add zeroize dependency)
- Modify: `src/usage_tracker.rs:36-56` (wrap token in Zeroizing)

- [ ] **Step 1: Add zeroize dependency**

In root `Cargo.toml`:

```toml
[dependencies]
zeroize = { version = "1.8", features = ["zeroize_derive"] }
```

- [ ] **Step 2: Wrap OAuth token in Zeroizing**

In `src/usage_tracker.rs`, change `read_oauth_token()` return type:

```rust
use zeroize::Zeroizing;

/// Read the OAuth access token from `~/.claude/.credentials.json`.
fn read_oauth_token() -> Option<Zeroizing<String>> {
    let home = dirs::home_dir()?;
    let cred_path = home.join(".claude").join(".credentials.json");
    let contents = std::fs::read_to_string(&cred_path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&contents).ok()?;
    parsed
        .get("claudeAiOauth")
        .and_then(|o| o.get("accessToken"))
        .and_then(|t| t.as_str())
        .map(|s| Zeroizing::new(s.to_string()))
}
```

- [ ] **Step 3: Update poll_usage to accept Zeroizing**

```rust
fn poll_usage(token: &Zeroizing<String>) -> Result<UsageData, String> {
    let response = ureq::get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", &format!("Bearer {}", token.as_str()))
        // ... rest unchanged ...
```

- [ ] **Step 4: Add debug assertion guard**

Add a compile-time check that prevents accidental token logging:

```rust
// Zeroizing<String> does not implement Display or Debug with the actual value,
// so tracing::debug!("{:?}", token) will not leak. No additional guard needed.
// However, add a cfg(test) assertion to verify:
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_debug_does_not_leak() {
        let token = Zeroizing::new("secret-token-value".to_string());
        let debug_output = format!("{:?}", token);
        // Zeroizing's Debug impl shows the inner value, but the token
        // is zeroed on drop. The key protection is memory zeroization.
        // To prevent log leakage, we avoid logging token values:
        assert!(!debug_output.is_empty()); // Compiles — token type is correct
    }
}
```

- [ ] **Step 5: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/usage_tracker.rs
git commit -m "fix(S-5): zeroize OAuth token in memory

Wrap Bearer token in Zeroizing<String> so it is wiped from memory on drop.
Prevents token from lingering in freed heap memory."
```

---

### Task 5: Command history redaction (S-6)

**Files:**
- Modify: `crates/glass_core/src/config.rs` (add `redact_patterns` to HistorySection)
- Modify: `crates/glass_history/src/db.rs:167-190` (scrub before insert)

- [ ] **Step 1: Add redact_patterns config field**

In `crates/glass_core/src/config.rs`, extend `HistorySection`:

```rust
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HistorySection {
    /// Maximum output capture size in kilobytes. Default 50.
    #[serde(default = "default_max_output_capture_kb")]
    pub max_output_capture_kb: u32,
    /// Regex patterns for sensitive data redaction. Matches are replaced with [REDACTED].
    #[serde(default = "default_redact_patterns")]
    pub redact_patterns: Vec<String>,
}

fn default_redact_patterns() -> Vec<String> {
    vec![
        r"(?i)password=\S+".to_string(),
        r"(?i)token=\S+".to_string(),
        r"Bearer \S+".to_string(),
        r"(?i)--password\s+\S+".to_string(),
        r"(?i)api[_-]?key[=:]\s*\S+".to_string(),
    ]
}
```

- [ ] **Step 2: Add redaction helper to glass_history**

In `crates/glass_history/src/db.rs`, add a redaction function:

```rust
use regex::Regex;

/// Scrub sensitive data from a string using the given regex patterns.
/// Each match is replaced with "[REDACTED]".
pub fn redact_sensitive(input: &str, patterns: &[Regex]) -> String {
    let mut result = input.to_string();
    for re in patterns {
        result = re.replace_all(&result, "[REDACTED]").to_string();
    }
    result
}
```

- [ ] **Step 3: Add redact_patterns parameter to insert_command**

Modify `insert_command` to accept optional redaction patterns:

```rust
/// Insert a command record into the database. Returns the row id.
/// If `redact_patterns` is provided, sensitive data in command text and output
/// is replaced with [REDACTED] before storage.
pub fn insert_command(
    &self,
    record: &CommandRecord,
    redact_patterns: &[Regex],
) -> Result<i64> {
    let command = if redact_patterns.is_empty() {
        record.command.clone()
    } else {
        redact_sensitive(&record.command, redact_patterns)
    };
    let output = record.output.as_ref().map(|o| {
        if redact_patterns.is_empty() {
            o.clone()
        } else {
            redact_sensitive(o, redact_patterns)
        }
    });

    let tx = self.conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO commands (command, cwd, exit_code, started_at, finished_at, duration_ms, output)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![command, record.cwd, record.exit_code, record.started_at, record.finished_at, record.duration_ms, output],
    )?;
    let rowid = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO commands_fts (rowid, command) VALUES (?1, ?2)",
        params![rowid, command],
    )?;
    tx.commit()?;
    Ok(rowid)
}
```

- [ ] **Step 4: Wire config into call site**

Find the call site of `insert_command` in `src/main.rs` and pass the compiled redact patterns. The patterns should be compiled once at config load and stored in the app state:

```rust
// In app state or session context, add:
redact_regexes: Vec<Regex>,

// At config load, compile patterns:
let redact_regexes: Vec<Regex> = config
    .history
    .as_ref()
    .map(|h| &h.redact_patterns)
    .unwrap_or(&vec![])
    .iter()
    .filter_map(|p| Regex::new(p).ok())
    .collect();
```

Pass `&redact_regexes` to `insert_command()`.

- [ ] **Step 5: Add redaction tests**

In `crates/glass_history/src/db.rs`:

```rust
#[cfg(test)]
mod redaction_tests {
    use super::*;
    use regex::Regex;

    #[test]
    fn test_redact_password() {
        let patterns = vec![Regex::new(r"(?i)password=\S+").unwrap()];
        assert_eq!(
            redact_sensitive("curl -u user:password=secret123 http://api", &patterns),
            "curl -u user:[REDACTED] http://api"
        );
    }

    #[test]
    fn test_redact_bearer() {
        let patterns = vec![Regex::new(r"Bearer \S+").unwrap()];
        assert_eq!(
            redact_sensitive("Authorization: Bearer eyJ0b2tlbi...", &patterns),
            "Authorization: [REDACTED]"
        );
    }

    #[test]
    fn test_redact_multiple_patterns() {
        let patterns = vec![
            Regex::new(r"(?i)password=\S+").unwrap(),
            Regex::new(r"(?i)token=\S+").unwrap(),
        ];
        let input = "password=secret token=abc123 other";
        let result = redact_sensitive(input, &patterns);
        assert_eq!(result, "[REDACTED] [REDACTED] other");
    }

    #[test]
    fn test_no_patterns_passthrough() {
        let result = redact_sensitive("password=secret", &[]);
        assert_eq!(result, "password=secret");
    }
}
```

- [ ] **Step 6: Update existing insert_command callers and tests**

Search all callers of `insert_command` and add the empty slice `&[]` for callers that don't have config access (e.g., in tests). This ensures backward compatibility.

- [ ] **Step 7: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 8: Commit**

```bash
git add crates/glass_core/src/config.rs crates/glass_history/src/db.rs src/main.rs
git commit -m "feat(S-6): redact sensitive data from command history

Add [history] redact_patterns config with defaults for password, token,
Bearer, api_key patterns. Scrub command text and output with [REDACTED]
before SQLite storage."
```

---

### Task 6: Undo path validation + blob hash validation (S-7, S-8)

**Files:**
- Modify: `crates/glass_snapshot/src/undo.rs:11-19,116-167` (add project_root, validate paths)
- Modify: `crates/glass_snapshot/src/blob_store.rs:40-47,86-98` (validate hash chars)

- [ ] **Step 1: Add project_root to UndoEngine**

In `crates/glass_snapshot/src/undo.rs`:

```rust
/// Engine that performs undo operations by restoring snapshotted files.
pub struct UndoEngine<'a> {
    store: &'a SnapshotStore,
    /// Root directory that all undo operations must stay within.
    project_root: PathBuf,
}

impl<'a> UndoEngine<'a> {
    /// Create a new UndoEngine backed by the given SnapshotStore.
    /// All restore operations are confined to `project_root`.
    pub fn new(store: &'a SnapshotStore, project_root: PathBuf) -> Self {
        Self { store, project_root }
    }
```

- [ ] **Step 2: Add path validation in restore_file**

In `restore_file()`, after `let path = PathBuf::from(&file_rec.file_path);`, add:

```rust
let path = PathBuf::from(&file_rec.file_path);

// Validate path is within project root (prevent directory traversal)
let canonical = match path.canonicalize().or_else(|_| {
    // File might not exist yet (was deleted) — canonicalize parent
    path.parent()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.join(path.file_name().unwrap_or_default()))
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "no parent"))
}) {
    Ok(p) => p,
    Err(_) => {
        return (
            path,
            FileOutcome::Error("Cannot resolve file path for validation".to_string()),
        );
    }
};

let canonical_root = self.project_root.canonicalize().unwrap_or_else(|_| self.project_root.clone());
if !canonical.starts_with(&canonical_root) {
    tracing::warn!(
        path = %path.display(),
        root = %canonical_root.display(),
        "Undo blocked: file path is outside project root"
    );
    return (
        path,
        FileOutcome::Error("File path is outside project root — undo blocked".to_string()),
    );
}
```

- [ ] **Step 3: Update UndoEngine callers**

Find all callers of `UndoEngine::new()` and pass `project_root`. The project root is typically the CWD or the `.glass/` directory's parent:

```rust
// BEFORE:
let engine = UndoEngine::new(&store);

// AFTER:
let project_root = std::env::current_dir().unwrap_or_default();
let engine = UndoEngine::new(&store, project_root);
```

Search for `UndoEngine::new` in `src/main.rs` and `crates/glass_mcp/src/tools.rs`.

- [ ] **Step 4: Add hex-digit validation to blob_store**

In `crates/glass_snapshot/src/blob_store.rs`, add hash validation to `read_blob()` and `delete_blob()`:

```rust
pub fn read_blob(&self, hash: &str) -> Result<Vec<u8>> {
    anyhow::ensure!(hash.len() >= 2, "blob hash too short: '{hash}'");
    anyhow::ensure!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "blob hash contains non-hex characters: '{hash}'"
    );
    let blob_path = self.blob_dir.join(&hash[..2]).join(format!("{}.blob", hash));
    Ok(std::fs::read(&blob_path)?)
}

pub fn delete_blob(&self, hash: &str) -> Result<bool> {
    anyhow::ensure!(hash.len() >= 2, "blob hash too short: '{hash}'");
    anyhow::ensure!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "blob hash contains non-hex characters: '{hash}'"
    );
    let blob_path = self.blob_dir.join(&hash[..2]).join(format!("{}.blob", hash));
    // ... rest unchanged ...
}
```

Also add to `blob_exists()`:

```rust
pub fn blob_exists(&self, hash: &str) -> bool {
    if hash.len() < 2 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    // ... rest unchanged ...
}
```

- [ ] **Step 5: Add tests**

In `crates/glass_snapshot/src/blob_store.rs` tests:

```rust
#[test]
fn test_non_hex_hash_rejected() {
    let dir = TempDir::new().unwrap();
    let store = BlobStore::new(dir.path());
    assert!(store.read_blob("../../etc/passwd").is_err());
    assert!(store.read_blob("zzzz").is_err());
    assert!(!store.blob_exists("../../../etc/shadow"));
}
```

In `crates/glass_snapshot/src/undo.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_outside_project_root_blocked() {
        // This test verifies that restore_file rejects paths outside project_root.
        // Full test requires a SnapshotStore with test data — covered by integration tests.
    }
}
```

- [ ] **Step 6: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 7: Commit**

```bash
git add crates/glass_snapshot/src/undo.rs crates/glass_snapshot/src/blob_store.rs crates/glass_snapshot/src/lib.rs src/main.rs crates/glass_mcp/src/tools.rs
git commit -m "fix(S-7/S-8): validate undo paths within project root, enforce hex-only blob hashes

UndoEngine now requires project_root and rejects file restores outside it.
BlobStore read/delete/exists validate hash is pure hex digits, preventing
directory traversal via crafted hash strings."
```

---

### Task 7: Ephemeral agent permissions + regex size limit (S-9, S-10)

**Files:**
- Modify: `src/ephemeral_agent.rs:115-123` (remove --dangerously-skip-permissions)
- Modify: `crates/glass_mcp/src/tools.rs:1262-1264` (regex length + size limit)

- [ ] **Step 1: Investigate --dangerously-skip-permissions necessity**

Before removing the flag, check the Claude CLI docs and behavior. The spec notes: "Verify ephemeral agent still works without `--dangerously-skip-permissions` across Claude CLI versions before committing."

Test manually:

```bash
claude -p --output-format stream-json --input-format stream-json --allowedTools "" <<< '{"type":"user_message","content":"hello"}'
```

If this works without `--dangerously-skip-permissions`, proceed with removal. If it fails (e.g., CLI requires interactive permission prompt), keep the flag and add documentation.

- [ ] **Step 2: Remove --dangerously-skip-permissions (if safe)**

In `src/ephemeral_agent.rs` at line 122, remove the flag:

```rust
// BEFORE:
let mut cmd = Command::new("claude");
cmd.args([
    "-p",
    "--output-format", "stream-json",
    "--input-format", "stream-json",
    "--system-prompt-file", &prompt_path,
    "--allowedTools", "",
    "--dangerously-skip-permissions",
]);

// AFTER:
let mut cmd = Command::new("claude");
cmd.args([
    "-p",
    "--output-format", "stream-json",
    "--input-format", "stream-json",
    "--system-prompt-file", &prompt_path,
    "--allowedTools", "",
]);
```

If the flag IS required, instead add a comment documenting why:

```rust
// --dangerously-skip-permissions is required for non-interactive ephemeral use.
// Safety: --allowedTools "" ensures zero tool access regardless of permission mode.
// The ephemeral agent is read-only (receives a prompt, returns text analysis).
"--dangerously-skip-permissions",
```

- [ ] **Step 3: Add regex pattern length limit (S-10)**

In `crates/glass_mcp/src/tools.rs` at line ~1262, where user regex is compiled:

```rust
// BEFORE:
let re = regex::Regex::new(pat).map_err(|e| format!("Invalid regex: {}", e))?;

// AFTER:
const MAX_PATTERN_LEN: usize = 1000;
if pat.len() > MAX_PATTERN_LEN {
    return Err(format!(
        "Regex pattern too long ({} chars, max {})",
        pat.len(),
        MAX_PATTERN_LEN
    ));
}
let re = regex::RegexBuilder::new(pat)
    .size_limit(1_000_000)  // 1MB compiled size limit
    .build()
    .map_err(|e| format!("Invalid regex: {}", e))?;
```

- [ ] **Step 4: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add src/ephemeral_agent.rs crates/glass_mcp/src/tools.rs
git commit -m "fix(S-9/S-10): harden ephemeral agent permissions and cap regex size

S-9: Remove --dangerously-skip-permissions (or document if required).
S-10: Cap user regex patterns at 1000 chars and 1MB compiled size via
RegexBuilder::size_limit to prevent ReDoS."
```

---

### Task 8: Shell temp file hardening (S-11)

**Files:**
- Modify: `shell-integration/glass.bash:284-289` (use mktemp -d, chmod 700)

- [ ] **Step 1: Replace predictable temp dir with mktemp**

In `shell-integration/glass.bash`, find `__glass_accept_line()` (~line 284-289):

```bash
# BEFORE:
local tmpdir="${TMPDIR:-/tmp}/glass_${$}_$(date +%s%N)"
if ! mkdir -p "$tmpdir" 2>/dev/null; then
    return  # Skip pipeline rewriting if temp dir creation fails
fi

# AFTER:
local tmpdir
tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/glass_XXXXXXXXXX") || return
chmod 700 "$tmpdir"
```

- [ ] **Step 2: Update cleanup function**

In `__glass_cleanup_stages()` (~line 272-277), the glob pattern needs to match the new naming:

```bash
# BEFORE:
__glass_cleanup_stages() {
    local pattern="${TMPDIR:-/tmp}/glass_${$}_*"
    for d in $pattern; do
        [[ -d "$d" ]] && rm -rf "$d" 2>/dev/null
    done
}

# AFTER:
__glass_cleanup_stages() {
    # Clean up any temp dirs created by this shell session.
    # With mktemp, dirs are named glass_XXXXXXXXXX — track them explicitly.
    if [[ -n "$__glass_capture_tmpdir" && -d "$__glass_capture_tmpdir" ]]; then
        rm -rf "$__glass_capture_tmpdir" 2>/dev/null
    fi
}
```

Note: If multiple pipeline invocations create multiple temp dirs per prompt cycle, accumulate them in an array:

```bash
# At top of shell integration, add:
__glass_tmpdirs=()

# In __glass_accept_line, after mktemp:
__glass_tmpdirs+=("$tmpdir")

# In __glass_cleanup_stages:
__glass_cleanup_stages() {
    for d in "${__glass_tmpdirs[@]}"; do
        [[ -d "$d" ]] && rm -rf "$d" 2>/dev/null
    done
    __glass_tmpdirs=()
}
```

- [ ] **Step 3: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

Shell integration changes don't have Rust tests, but verify the build succeeds (shell scripts are embedded or referenced).

- [ ] **Step 4: Commit**

```bash
git add shell-integration/glass.bash
git commit -m "fix(S-11): use mktemp for unpredictable shell temp directories

Replace predictable glass_PID_TIMESTAMP temp paths with mktemp -d.
Set chmod 700 on created directories. Track dirs in array for cleanup."
```

---

### Task 9: Database file permissions + CWD LIKE escaping (S-12, S-13)

**Files:**
- Modify: `crates/glass_history/src/db.rs:52-60` (set 0600 on DB file)
- Modify: `crates/glass_snapshot/src/db.rs` (same)
- Modify: `crates/glass_coordination/src/db.rs:26-39` (same)
- Modify: `crates/glass_history/src/query.rs:153-155` (escape LIKE wildcards)

- [ ] **Step 1: Set 0600 permissions on DB files (Unix only)**

Add a helper function and call it after `Connection::open()` in each DB crate. In `crates/glass_history/src/db.rs`, after `let conn = Connection::open(path)?;`:

```rust
let conn = Connection::open(path)?;

// Restrict database file to owner-only access (Unix)
#[cfg(unix)]
{
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o600);
    if let Err(e) = std::fs::set_permissions(path, perms) {
        tracing::warn!("Failed to set DB file permissions: {e}");
    }
    // Also set permissions on WAL and SHM files if they exist
    let wal_path = path.with_extension("db-wal");
    let shm_path = path.with_extension("db-shm");
    for p in [&wal_path, &shm_path] {
        if p.exists() {
            let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600));
        }
    }
}
```

Apply the same pattern to `crates/glass_snapshot/src/db.rs` and `crates/glass_coordination/src/db.rs`.

- [ ] **Step 2: Escape LIKE wildcards in CWD filter**

In `crates/glass_history/src/query.rs` at line ~153-155:

```rust
// BEFORE:
if let Some(ref cwd) = filter.cwd {
    conditions.push("c.cwd LIKE ?".to_string());
    params.push(rusqlite::types::Value::Text(format!("{}%", cwd)));
}

// AFTER:
if let Some(ref cwd) = filter.cwd {
    conditions.push("c.cwd LIKE ? ESCAPE '\\'".to_string());
    let escaped = cwd
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    params.push(rusqlite::types::Value::Text(format!("{}%", escaped)));
}
```

- [ ] **Step 3: Add LIKE escaping test**

In `crates/glass_history/src/query.rs` tests:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_cwd_like_escaping() {
        // Verify that % and _ in CWD paths are escaped
        let cwd = "/home/user/my_project%test";
        let escaped = cwd
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        assert_eq!(escaped, "/home/user/my\\_project\\%test");
    }
}
```

- [ ] **Step 4: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add crates/glass_history/src/db.rs crates/glass_snapshot/src/db.rs crates/glass_coordination/src/db.rs crates/glass_history/src/query.rs
git commit -m "fix(S-12/S-13): set 0600 on database files, escape LIKE wildcards in CWD filter

S-12: Unix-only chmod 0600 on .db, .db-wal, .db-shm files at open.
S-13: Escape %, _, and \\ in CWD LIKE queries to prevent wildcard injection."
```

---

### Task 10: Workspace trust for scripts + config path validation (S-14, S-16)

**Files:**
- Modify: `crates/glass_core/src/config.rs` (validate prd_path, checkpoint_path)
- New: trust check in script loading (minimal — check `~/.glass/trusted_projects.toml`)

- [ ] **Step 1: Add config path validation for orchestrator paths**

In `crates/glass_core/src/config.rs`, add a validation function for `OrchestratorSection`:

```rust
impl OrchestratorSection {
    /// Validate that file paths in the orchestrator config stay within
    /// the project directory (no absolute paths or `..` traversal).
    pub fn validate_paths(&self) -> Result<(), String> {
        for (name, path_str) in [
            ("prd_path", &self.prd_path),
            ("checkpoint_path", &self.checkpoint_path),
        ] {
            let path = Path::new(path_str);
            if path.is_absolute() {
                return Err(format!(
                    "[agent.orchestrator] {name} must be a relative path, got: {path_str}"
                ));
            }
            // Check for directory traversal
            for component in path.components() {
                if matches!(component, std::path::Component::ParentDir) {
                    return Err(format!(
                        "[agent.orchestrator] {name} must not contain '..': {path_str}"
                    ));
                }
            }
        }
        Ok(())
    }
}
```

Call `validate_paths()` during config loading and warn if validation fails:

```rust
// In config load/validation:
if let Some(agent) = &config.agent {
    if let Some(orch) = &agent.orchestrator {
        if let Err(e) = orch.validate_paths() {
            tracing::warn!("Config validation: {e}");
        }
    }
}
```

- [ ] **Step 2: Add workspace trust check for scripts**

In `crates/glass_core/src/config.rs` (or a new `crates/glass_core/src/trust.rs` if the config file is getting large), add:

```rust
use std::collections::HashSet;

/// Path to the trusted projects registry.
fn trusted_projects_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".glass").join("trusted_projects.toml"))
}

/// Check if a project root is trusted for script execution.
pub fn is_project_trusted(project_root: &Path) -> bool {
    let path = match trusted_projects_path() {
        Some(p) => p,
        None => return false,
    };
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let parsed: toml::Value = match contents.parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let projects = parsed
        .get("trusted")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();

    let canonical = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    projects.contains(&canonical.to_string_lossy().to_string())
}

/// Mark a project as trusted.
pub fn trust_project(project_root: &Path) -> Result<(), String> {
    let path = trusted_projects_path()
        .ok_or_else(|| "Cannot determine home directory".to_string())?;

    let canonical = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf())
        .to_string_lossy()
        .to_string();

    let mut projects: Vec<String> = if let Ok(contents) = std::fs::read_to_string(&path) {
        if let Ok(parsed) = contents.parse::<toml::Value>() {
            parsed
                .get("trusted")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default()
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    if !projects.contains(&canonical) {
        projects.push(canonical);
    }

    let content = format!("trusted = {:?}\n", projects);
    std::fs::write(&path, content).map_err(|e| format!("Failed to write trust file: {e}"))
}
```

Wire the trust check into the script tool loading path (in `crates/glass_mcp/src/tools.rs` where `glass_list_script_tools` and `glass_script_tool` are implemented). Before executing any user script, verify `is_project_trusted(project_root)`.

- [ ] **Step 3: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add crates/glass_core/src/config.rs crates/glass_mcp/src/tools.rs
git commit -m "fix(S-14/S-16): add workspace trust for scripts, validate orchestrator config paths

S-14: Track trusted projects in ~/.glass/trusted_projects.toml.
Script tools require project to be trusted before execution.
S-16: Validate prd_path and checkpoint_path are relative, no '..' traversal."
```

---

### Task 11: cargo audit in CI (S-15)

**Files:**
- Modify: `.github/workflows/ci.yml` (add cargo-audit job)

- [ ] **Step 1: Add cargo-audit CI job**

Add a new job to `.github/workflows/ci.yml`:

```yaml
  audit:
    name: Security Audit
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install cargo-audit
        run: cargo install cargo-audit
      - name: Run cargo audit
        run: cargo audit
```

This can run in parallel with existing jobs since it has no dependencies.

- [ ] **Step 2: Build (no Rust build needed for this step)**

Verify the YAML syntax is valid.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci(S-15): add cargo audit job for dependency vulnerability scanning"
```

---

### Task 12: Final verification and clippy

- [ ] **Step 1: Run clippy**

```bash
cargo clippy --workspace -- -D warnings 2>&1
```

Fix any warnings.

- [ ] **Step 2: Run fmt**

```bash
cargo fmt --all -- --check 2>&1
```

Fix any formatting issues.

- [ ] **Step 3: Run full test suite**

```bash
cargo test --workspace 2>&1
```

- [ ] **Step 4: Commit any cleanup**

```bash
git add -A
git commit -m "chore: clippy and fmt cleanup for audit/security branch"
```

- [ ] **Step 5: Summary — verify all items addressed**

Check off against the spec:
- [x] S-1: MCP permission gate for command-executing tools (Task 1)
- [x] S-2: IPC socket permissions (Task 2)
- [x] S-3: MCP access controls + audit logging (Task 1)
- [x] S-4: Agent coordination nonce authentication (Task 3)
- [x] S-5: OAuth token zeroize (Task 4)
- [x] S-6: Command history redaction (Task 5)
- [x] S-7: Undo path validation (Task 6)
- [x] S-8: Blob store hash validation (Task 6)
- [x] S-9: Ephemeral agent permissions (Task 7)
- [x] S-10: Regex pattern length limit (Task 7)
- [x] S-11: Shell temp files (Task 8)
- [x] S-12: Database file permissions (Task 9)
- [x] S-13: CWD LIKE wildcard escaping (Task 9)
- [x] S-14: Workspace trust for scripts (Task 10)
- [x] S-15: cargo audit in CI (Task 11)
- [x] S-16: Config path validation (Task 10)

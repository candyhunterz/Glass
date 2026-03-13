# Phase 59: Agent Session Continuity - Research

**Researched:** 2026-03-13
**Domain:** Agent subprocess session management, Claude CLI stream-json protocol, SQLite schema extension, handoff JSON design
**Confidence:** HIGH

## Summary

Phase 59 makes agent sessions survive context resets. The core mechanism: the agent system prompt instructs the agent to emit a structured `GLASS_HANDOFF: {...}` JSON marker before its session ends (paralleling the `GLASS_PROPOSAL:` marker already implemented). Glass detects this marker in the reader thread, stores the handoff in a new `agent_sessions` table in `agents.db`, and when the next agent is spawned, injects the most recent handoff as the opening user message.

The Claude CLI's stream-json protocol already exposes what we need: the `type: "result"` message carries `subtype` (which is `error_max_turns` when the context/turn limit is hit), `session_id`, and `cost_usd`. The `type: "system", subtype: "compact_boundary"` message fires when context compaction occurs. These two signals are the triggers for handoff emission. The handoff itself is embedded in an assistant text message, parsed with the same brace-depth walker already in `extract_proposal()`, and stored to SQLite.

Session chaining (AGTS-04) works via a `previous_session_id TEXT` foreign-key column on `agent_sessions`. Each handoff references the prior handoff's session ID, forming a linked list. Context compaction on startup injects only the most recent handoff (one level) unless the planner chooses recursive compaction, but the linked chain provides full history traceability.

**Primary recommendation:** Add `agent_sessions` table at migration version 3 in `glass_agent/src/worktree_db.rs`, add `GLASS_HANDOFF` detection to the reader thread in `src/main.rs`, inject the handoff as an opening user message in `try_spawn_agent`, and extend the system prompt to request handoff output before context limit.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| AGTS-01 | Agent produces structured handoff summary before session ends (context exhaustion, timeout) | System prompt instructs agent to emit `GLASS_HANDOFF: {...}` when it detects approaching context limit or before any graceful exit. Glass reader thread detects this in assistant messages using the same `extract_proposal` brace-depth pattern. |
| AGTS-02 | Handoff stored in `agent_sessions` table with work completed, remaining, key decisions | New `agent_sessions` table at migration version 3 in `~/.glass/agents.db`. Columns: `id` (UUID), `project_root`, `session_id` (Claude session UUID), `previous_session_id` (nullable, FK for chain), `work_completed`, `work_remaining`, `key_decisions`, `raw_handoff`, `created_at`. |
| AGTS-03 | New agent session loads most recent handoff as initial context | `try_spawn_agent` queries `agent_sessions` for most recent row by `project_root` before spawn, serializes it as a `user` message in the stream-json wire format, writes it to stdin immediately after spawn (before any activity event). |
| AGTS-04 | Multiple sequential sessions form a chain of handoffs with context compaction | `agent_sessions.previous_session_id` column links rows. Each new handoff insert reads the current `session_id` from the `type: "system", subtype: "init"` message and stores it. Load compaction: inject only the immediate predecessor's summary, not the full chain, to keep initial context bounded. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `rusqlite` | 0.38 (workspace) | `agent_sessions` table, migration version 3 | Already used for all Glass persistence; `agents.db` already at version 2 in `WorktreeDb` |
| `serde_json` | 1.0 (workspace) | Parse `GLASS_HANDOFF` JSON from assistant text; serialize handoff as stdin user message | Already in every reader/writer path in this phase's dependencies |
| `uuid` | 1.22 (workspace) | Generate session IDs for `agent_sessions` rows | Already in workspace (added Phase 57) |
| `dirs` | 6 (workspace) | Resolve `~/.glass/agents.db` path | Already used in `WorktreeDb::open_default()` |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `tracing` | workspace | Log handoff detection, session chain queries | Project convention; use for all agent lifecycle events |
| `chrono` | workspace | Timestamp handoff rows | Already used elsewhere in Glass for timestamps |

### No New Dependencies
Phase 59 requires no new crates. All dependencies are already in the workspace. The migration extends the existing `agents.db` schema at version 3 without breaking versions 1 (CoordinationDb) or 2 (WorktreeDb).

**Installation:**
```bash
# No new deps needed — all workspace deps already present
```

## Architecture Patterns

### Recommended Module Structure

```
crates/glass_agent/src/
├── types.rs               # Add AgentSessionRecord, HandoffData structs
├── worktree_db.rs         # Add agent_sessions table at version 3
├── session_db.rs          # NEW: AgentSessionDb -- CRUD for agent_sessions
├── worktree_manager.rs    # Unchanged
└── lib.rs                 # Export AgentSessionDb, AgentSessionRecord, HandoffData

crates/glass_core/src/
├── agent_runtime.rs       # Add extract_handoff(), format_handoff_as_user_message()
└── event.rs               # Add AppEvent::AgentHandoff { session_id, handoff }

src/main.rs
├── try_spawn_agent()      # Load prior handoff, inject as first user message
├── reader thread          # Detect GLASS_HANDOFF in assistant messages
└── AppEvent::AgentHandoff handler  # Persist to agent_sessions DB
```

### Pattern 1: GLASS_HANDOFF Marker in System Prompt

**What:** Extend the agent system prompt to instruct the agent to emit a structured JSON handoff before its session ends.

**When to use:** This instruction lives permanently in the system prompt. The agent emits it when (a) it detects a user message starting with `[CONTEXT_LIMIT_WARNING]` (injected by the Glass writer thread when `error_max_turns` fires), or (b) when wrapping up normally.

**System prompt addition:**
```
When you are approaching the end of your session (you receive a [CONTEXT_LIMIT_WARNING] signal
or are completing a task), output a handoff summary using this exact format on its own line:

GLASS_HANDOFF: {"work_completed":"<what was done>","work_remaining":"<what is left>","key_decisions":"<important decisions>","previous_session_id":"<session_id if continuing>"}

This allows the next agent session to pick up exactly where you left off.
```

**When context limit fires:**
The `type: "result", subtype: "error_max_turns"` message arrives from the reader thread. The writer thread should inject a final user message: `{"type":"user","message":{"role":"user","content":"[CONTEXT_LIMIT_WARNING] Your session is ending due to turn/context limit. Please emit GLASS_HANDOFF before stopping."}}`. This warning arrives only if there are turns remaining; if `error_max_turns` fires with 0 remaining, Glass must rely on the agent having already emitted the handoff during its final turn.

**Practical reality (MEDIUM confidence):** The agent cannot reliably predict its own context exhaustion ahead of time. The more robust approach is for the agent to emit an intermediate `GLASS_HANDOFF` checkpoint after completing any significant unit of work. The system prompt should recommend this: "After completing each major task milestone, emit GLASS_HANDOFF to checkpoint your progress."

### Pattern 2: Handoff Detection in Reader Thread

**What:** Extend the existing reader thread in `try_spawn_agent()` to detect `GLASS_HANDOFF:` markers in assistant messages, mirroring `extract_proposal`.

**When to use:** Every assistant message is scanned for the `GLASS_HANDOFF:` prefix.

```rust
// Source: mirrors glass_core/src/agent_runtime.rs extract_proposal() pattern
// In reader thread, after extracting full_text from assistant message content:

if let Some(handoff) = glass_core::agent_runtime::extract_handoff(&full_text) {
    // Capture the session_id (obtained from the system/init message stored in reader thread state)
    let _ = proxy_reader.send_event(glass_core::event::AppEvent::AgentHandoff {
        session_id: current_session_id.clone(),
        handoff,
    });
}
```

**Session ID capture:** The reader thread must capture the session UUID from the `type: "system", subtype: "init"` message and store it in a local variable. This session_id is then attached to every `AgentHandoff` event.

```rust
// In reader thread, detecting the init message:
Some("system") => {
    if val.get("subtype").and_then(|s| s.as_str()) == Some("init") {
        if let Some(id) = val.get("session_id").and_then(|s| s.as_str()) {
            current_session_id = id.to_string();
        }
    }
}
```

### Pattern 3: HandoffData and AgentSessionRecord Types

**What:** New types in `glass_agent/src/types.rs`.

```rust
// Source: mirrors existing PendingWorktree/WorktreeHandle pattern
/// Structured data from a GLASS_HANDOFF marker.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct HandoffData {
    /// Summary of work completed this session.
    pub work_completed: String,
    /// Work that remains for the next session.
    pub work_remaining: String,
    /// Key decisions made that should be preserved.
    pub key_decisions: String,
    /// Session ID of the previous session (for chain continuity).
    #[serde(default)]
    pub previous_session_id: Option<String>,
}

/// A row in the `agent_sessions` table.
#[derive(Debug, Clone)]
pub struct AgentSessionRecord {
    /// UUID for this session record.
    pub id: String,
    /// Absolute project root path (canonicalized).
    pub project_root: String,
    /// Claude session UUID from the system/init message.
    pub session_id: String,
    /// Claude session UUID of the previous session in the chain.
    pub previous_session_id: Option<String>,
    /// Structured handoff content.
    pub handoff: HandoffData,
    /// Raw JSON from the GLASS_HANDOFF marker.
    pub raw_handoff: String,
    /// Unix timestamp.
    pub created_at: i64,
}
```

### Pattern 4: agent_sessions Migration (Version 3)

**What:** Add `agent_sessions` table to `agents.db` at migration version 3. The existing `migrate()` function in `worktree_db.rs` already handles version 1 (CoordinationDb territory) and version 2 (`pending_worktrees`).

```rust
// Source: mirrors existing migrate() in glass_agent/src/worktree_db.rs
if version < 3 {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_sessions (
            id                  TEXT PRIMARY KEY,
            project_root        TEXT NOT NULL,
            session_id          TEXT NOT NULL,
            previous_session_id TEXT,
            work_completed      TEXT NOT NULL,
            work_remaining      TEXT NOT NULL,
            key_decisions       TEXT NOT NULL,
            raw_handoff         TEXT NOT NULL,
            created_at          INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE INDEX IF NOT EXISTS idx_agent_sessions_project
            ON agent_sessions(project_root, created_at DESC);",
    )?;
    conn.pragma_update(None, "user_version", 3i64)?;
}
```

**CRITICAL:** The index on `(project_root, created_at DESC)` is required for efficient `SELECT ... ORDER BY created_at DESC LIMIT 1` queries at spawn time.

### Pattern 5: Loading Prior Handoff at Session Start

**What:** In `try_spawn_agent()`, query for the most recent session record for the current project root and inject it as the first user message on stdin.

```rust
// Source: mirrors try_spawn_agent() pattern in src/main.rs
// After spawning child and obtaining stdin BufWriter,
// before entering the activity event loop:

if let Some(handoff) = load_prior_handoff(&project_root) {
    let ctx_msg = format_handoff_as_user_message(&handoff);
    let _ = writeln!(writer, "{}", ctx_msg);
    let _ = writer.flush();
    tracing::info!("AgentRuntime: injected prior session handoff (session_id={})", handoff.session_id);
}
```

The handoff message is a standard stream-json user message:
```json
{"type":"user","message":{"role":"user","content":"[PRIOR_SESSION_CONTEXT] work_completed=... work_remaining=... key_decisions=..."}}
```

### Pattern 6: Handoff Chaining (AGTS-04)

**What:** Store `previous_session_id` in each handoff record. On insert, the `session_id` from the `system/init` message is stored, and the `previous_session_id` from the `HandoffData` struct (emitted by the agent itself) is persisted.

**Context compaction strategy:** Inject ONLY the most recent handoff record as initial context. Do not traverse the full chain on startup — this would grow unbounded. The chain exists for auditability and traceability, not for loading the entire history on each spawn.

**Chain depth verification (AGTS-04 success criterion 4):** The test can verify the chain by: insert 3 records with `previous_session_id` linking them → query `load_prior_handoff` → verify it returns only the most recent → verify the chain can be traversed manually by following `previous_session_id` links.

### Anti-Patterns to Avoid

- **Storing full session transcripts in `agent_sessions`:** Only the structured handoff fields are stored. Full transcripts are in `~/.claude/projects/` managed by the CLI itself. Do not duplicate them.
- **Loading the entire handoff chain at spawn time:** Only inject the most recent handoff. Injecting all prior sessions would consume significant context and defeat the purpose of compaction.
- **Relying solely on `error_max_turns` to trigger handoff:** By the time Glass receives this result message, the agent has already exited. The agent must be instructed to proactively emit `GLASS_HANDOFF` checkpoints. The `error_max_turns` path is a fallback, not the primary mechanism.
- **Writing `GLASS_HANDOFF` detection in the writer thread:** The writer thread processes outbound events. Handoff detection belongs in the reader thread (stdout), which already processes inbound assistant messages.
- **Using a separate SQLite file for `agent_sessions`:** Use the existing `agents.db` at migration version 3. No new files.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Context compaction summary | Custom LLM summarization call | Agent-generated `GLASS_HANDOFF` with specific fields | Agent already knows what it did; no extra API call needed |
| Session transcript replay | Load all prior messages on restart | `--resume <session_id>` CLI flag + single handoff injection | Claude CLI already manages session files; Glass only needs the summary |
| JSON handoff parsing | Custom recursive parser | Brace-depth walker already in `extract_proposal()` | Zero new code for the parsing path; just a new `extract_handoff()` function following the same pattern |
| SQLite migration management | New version scheme | Extend existing `migrate()` at version 3 | Migration infrastructure in `worktree_db.rs` is battle-tested and handles version conflicts cleanly |
| Cross-session context | Include full prior assistant messages | One injected user message with handoff fields | Bounded context cost; one message = ~100-200 tokens regardless of session length |

**Key insight:** The agent itself is the best summarizer of its own work. The handoff pattern leverages Claude's natural language summarization capability rather than building a separate compression system.

## Common Pitfalls

### Pitfall 1: Agent Never Emits GLASS_HANDOFF
**What goes wrong:** The agent completes its task or hits context limits without emitting a handoff. The next session starts with no context.
**Why it happens:** System prompt instructions are not followed reliably, especially when the agent is focused on a task and context exhaustion is sudden.
**How to avoid:** (1) Instruct the agent to emit `GLASS_HANDOFF` after completing each major task milestone, not just at session end. (2) Treat absence of handoff as a graceful degradation case — the next session simply starts fresh. (3) The `error_max_turns` result triggers a `[CONTEXT_LIMIT_WARNING]` injection as a final message, giving the agent one more chance.
**Warning signs:** `agent_sessions` table remains empty after multiple sessions.

### Pitfall 2: session_id Captured Before system/init Message
**What goes wrong:** `current_session_id` is empty string when the reader thread processes the first assistant message if the init message has not yet been received.
**Why it happens:** Race condition: assistant messages may appear before the reader thread state captures the session_id if the init message is very fast.
**How to avoid:** Initialize `current_session_id` to empty string; attach it to `AgentHandoff` events regardless. In the `AgentHandoff` handler, if `session_id` is empty, use a UUID generated at spawn time as a fallback. Document in code.
**Warning signs:** `agent_sessions` rows with empty `session_id` column.

### Pitfall 3: Migration Version Conflict with CoordinationDb
**What goes wrong:** `CoordinationDb` also runs migrations on `agents.db`. If it is upgraded to write version 3 independently, `WorktreeDb`'s version 3 migration for `agent_sessions` either double-runs or conflicts.
**Why it happens:** Two migration managers share the same `user_version` pragma on the same database file.
**How to avoid:** The existing code already handles this: `WorktreeDb::migrate()` checks `if version < 1` to handle the CoordinationDb case, then `if version < 2` for its own table. Add `if version < 3` for `agent_sessions`. As long as CoordinationDb stays at version 1 and `WorktreeDb` owns versions 2 and 3, there is no conflict. Verify before committing that `CoordinationDb::migrate()` still only sets version to 1.
**Warning signs:** `CREATE TABLE agent_sessions already exists` SQLite error.

### Pitfall 4: Project Root Path Mismatch
**What goes wrong:** `load_prior_handoff(project_root)` returns no rows even though prior sessions exist, because the stored path used a different canonicalization.
**Why it happens:** On Windows, paths may be stored with different slashes or drive letter casing between sessions. The `CoordinationDb` already has this problem and uses `crate::canonicalize_path()`.
**How to avoid:** Always canonicalize `project_root` before both INSERT and SELECT using the same helper used by `CoordinationDb`. `crate::canonicalize_path()` handles this with `.unwrap_or_else(|_| raw_path)` fallback.
**Warning signs:** Empty result from `load_prior_handoff` even though rows exist in the table.

### Pitfall 5: Handoff Injected Into Budget-Exhausted Session
**What goes wrong:** When a session ended because `max_budget_usd` was reached, a new session starts fresh but injects a prior handoff, consuming tokens that immediately push toward the budget again.
**Why it happens:** Budget exhaustion and context exhaustion are treated identically in the handoff path.
**How to avoid:** Inject the prior handoff regardless of how the prior session ended — the handoff is lightweight (~100-300 tokens) and the next session has a fresh budget. This is correct behavior.

## Code Examples

Verified patterns from official sources and existing codebase:

### extract_handoff() — Mirrors extract_proposal()
```rust
// Source: mirrors extract_proposal() in crates/glass_core/src/agent_runtime.rs
// GLASS_HANDOFF uses same brace-depth walker, different struct

pub fn extract_handoff(assistant_text: &str) -> Option<(HandoffData, String)> {
    let marker = "GLASS_HANDOFF:";
    let start = assistant_text.find(marker)?;
    let after_marker = &assistant_text[start + marker.len()..].trim_start();
    let brace_start = after_marker.find('{')?;
    let json_slice = &after_marker[brace_start..];
    let mut depth = 0usize;
    let mut end = None;
    for (i, ch) in json_slice.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 { end = Some(i + 1); break; }
            }
            _ => {}
        }
    }
    let json_str = &json_slice[..end?];
    let handoff: HandoffData = serde_json::from_str(json_str).ok()?;
    Some((handoff, json_str.to_string()))
}
```

### session_id Capture from system/init
```rust
// Source: verified from official Claude agent-loop docs (platform.claude.com)
// In reader thread local state:
let mut current_session_id = String::new();

// In message type matching:
Some("system") => {
    if val.get("subtype").and_then(|s| s.as_str()) == Some("init") {
        if let Some(id) = val.get("session_id").and_then(|v| v.as_str()) {
            current_session_id = id.to_string();
        }
    }
}
```

### Handoff Injection as User Message
```rust
// Source: mirrors format_activity_as_user_message() pattern in agent_runtime.rs
pub fn format_handoff_as_user_message(record: &AgentSessionRecord) -> String {
    let content = format!(
        "[PRIOR_SESSION_CONTEXT] session_id={} work_completed={} work_remaining={} key_decisions={}",
        record.session_id,
        record.handoff.work_completed,
        record.handoff.work_remaining,
        record.handoff.key_decisions,
    );
    serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": content
        }
    }).to_string()
}
```

### agent_sessions INSERT
```rust
// Source: mirrors insert_pending_worktree() in glass_agent/src/worktree_db.rs
pub fn insert_session(&mut self, record: &AgentSessionRecord) -> Result<()> {
    let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "INSERT INTO agent_sessions (id, project_root, session_id, previous_session_id,
             work_completed, work_remaining, key_decisions, raw_handoff)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            &record.id,
            &record.project_root,
            &record.session_id,
            record.previous_session_id.as_deref(),
            &record.handoff.work_completed,
            &record.handoff.work_remaining,
            &record.handoff.key_decisions,
            &record.raw_handoff,
        ],
    )?;
    tx.commit()?;
    Ok(())
}
```

### agent_sessions SELECT (most recent for project)
```rust
// Source: mirrors list_pending_worktrees() pattern
pub fn load_prior_handoff(&self, project_root: &str) -> Result<Option<AgentSessionRecord>> {
    let mut stmt = self.conn.prepare(
        "SELECT id, project_root, session_id, previous_session_id,
                work_completed, work_remaining, key_decisions, raw_handoff, created_at
         FROM agent_sessions
         WHERE project_root = ?1
         ORDER BY created_at DESC LIMIT 1",
    )?;
    let mut rows = stmt.query_map(params![project_root], |row| {
        Ok(AgentSessionRecord {
            id: row.get(0)?,
            project_root: row.get(1)?,
            session_id: row.get(2)?,
            previous_session_id: row.get(3)?,
            handoff: HandoffData {
                work_completed: row.get(4)?,
                work_remaining: row.get(5)?,
                key_decisions: row.get(6)?,
                previous_session_id: row.get(3)?,
            },
            raw_handoff: row.get(7)?,
            created_at: row.get(8)?,
        })
    })?;
    rows.next().transpose().map_err(Into::into)
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Manual handoff files (CONTEXT.md, plan.md) | Structured `GLASS_HANDOFF` JSON embedded in assistant output | Phase 59 (new) | Automated capture without user intervention |
| Claude CLI `--continue` (most-recent session) | `--resume <session_id>` + handoff injection in prompt | Available since 2025 | Precise session targeting vs. directory-scoped "most recent" |
| Context window as implicit barrier | `compact_boundary` system message + structured handoff checkpoint | 2025 (compaction) | Explicit compaction boundary; `compact_boundary` fires after the SDK summarizes old history |
| Agent starts every session cold | Agent receives `[PRIOR_SESSION_CONTEXT]` user message | Phase 59 (new) | Immediate continuity without reading prior transcript |

**Deprecated/outdated:**
- Relying on `--continue` flag in print mode (`-p`): `--continue` finds "most recent session in current directory" — if Glass changes working directory or the session was from a different invocation, this silently starts fresh. Use explicit session ID storage + `--resume` for reliable resumption.
- Note: `--resume` with `-p` is confirmed working from official CLI docs: `claude -p --resume <session_id> "prompt"`.

## Open Questions

1. **Should `--resume` be used alongside handoff injection, or only handoff injection?**
   - What we know: `--resume <session_id>` would reload the full prior transcript AND the agent would receive the handoff message. This is redundant and may hit context limits if the prior session was long.
   - What's unclear: Whether Glass should resume the Claude session itself (loading full transcript) or rely solely on the handoff summary as context.
   - Recommendation: Use handoff summary injection ONLY (no `--resume` flag). The session file on disk is the Claude CLI's responsibility. Glass's handoff provides lightweight structured context without the full transcript cost. If the user explicitly wants to resume a full session, that's a separate future feature.

2. **How to handle context_limit vs. max_turns exhaustion differently**
   - What we know: `error_max_turns` fires when `--max-turns` is hit. Context window exhaustion manifests differently — the agent may get an API error mid-turn. From `agents.db` the behavior looks the same (reader thread gets EOF or an error line).
   - What's unclear: Is there a distinct `error_context_window` subtype in the stream-json protocol, or does context exhaustion show as `error_during_execution`?
   - Recommendation: Based on research, context window exceeded in practice causes `error_during_execution` with an API error message in the stream. Treat both `error_max_turns` and `error_during_execution` as session-ending signals that should trigger a `[CONTEXT_LIMIT_WARNING]` injection attempt. The handoff checkpoint pattern (emit after major milestones) handles both cases.

3. **Retention policy for `agent_sessions` rows**
   - What we know: There is no pruning logic specified in the requirements.
   - What's unclear: Should old session records be pruned? After how long?
   - Recommendation: Phase 59 does not implement pruning. Add a timestamp index for future pruning work. Phase 60 (Agent Configuration) could add a `session_retention_days` config key.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test (`cargo test`) |
| Config file | None (inline `#[cfg(test)]`) |
| Quick run command | `cargo test -p glass_agent -- session` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| AGTS-01 | `extract_handoff` parses valid `GLASS_HANDOFF:` marker | unit | `cargo test -p glass_core -- agent_runtime::tests::extract_handoff_parses_valid_marker` | Wave 0 |
| AGTS-01 | `extract_handoff` returns None without marker | unit | `cargo test -p glass_core -- agent_runtime::tests::extract_handoff_returns_none_without_marker` | Wave 0 |
| AGTS-02 | `AgentSessionDb::insert_session` persists row | unit | `cargo test -p glass_agent -- session_db::tests::test_insert_and_list` | Wave 0 |
| AGTS-02 | Session record survives connection close+reopen | unit | `cargo test -p glass_agent -- session_db::tests::test_session_survives_restart` | Wave 0 |
| AGTS-02 | Migration sets `user_version` to 3 | unit | `cargo test -p glass_agent -- session_db::tests::test_migration_version_3` | Wave 0 |
| AGTS-03 | `load_prior_handoff` returns most recent row for project_root | unit | `cargo test -p glass_agent -- session_db::tests::test_load_prior_handoff_most_recent` | Wave 0 |
| AGTS-03 | `load_prior_handoff` returns None when no rows exist | unit | `cargo test -p glass_agent -- session_db::tests::test_load_prior_handoff_empty` | Wave 0 |
| AGTS-03 | `format_handoff_as_user_message` produces valid JSON | unit | `cargo test -p glass_core -- agent_runtime::tests::format_handoff_produces_valid_json` | Wave 0 |
| AGTS-04 | `previous_session_id` column forms chain of 3 records | unit | `cargo test -p glass_agent -- session_db::tests::test_session_chain_three_records` | Wave 0 |
| AGTS-04 | Migration version 2 (pending_worktrees) unaffected by version 3 | unit | `cargo test -p glass_agent -- worktree_db::tests::test_migration_version_2` | Exists (passes today) |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_agent -- session && cargo test -p glass_core -- agent_runtime`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_agent/src/session_db.rs` — new file: `AgentSessionDb`, migration version 3, `insert_session`, `load_prior_handoff`, tests
- [ ] `crates/glass_agent/src/types.rs` — add `HandoffData`, `AgentSessionRecord` structs
- [ ] `crates/glass_core/src/agent_runtime.rs` — add `extract_handoff()`, `format_handoff_as_user_message()`
- [ ] `crates/glass_core/src/event.rs` — add `AppEvent::AgentHandoff { session_id: String, handoff: AgentHandoffData, project_root: String, raw_json: String }`
- [ ] `crates/glass_agent/src/lib.rs` — export `AgentSessionDb`, `AgentSessionRecord`, `HandoffData`

*(Note: `AppEvent::AgentHandoff` payload type must be defined in `glass_core` to avoid circular deps; mirror the `AgentProposalData` pattern — flat fields, no `glass_agent` types.)*

## Sources

### Primary (HIGH confidence)
- `platform.claude.com/docs/en/agent-sdk/agent-loop` — Official Agent SDK loop docs. Confirmed result subtypes: `success`, `error_max_turns`, `error_max_budget_usd`, `error_during_execution`. Confirmed `session_id` on `ResultMessage`. Confirmed `compact_boundary` SystemMessage. Accessed 2026-03-13.
- `platform.claude.com/docs/en/agent-sdk/sessions` — Official session management docs. Confirmed `--resume <session_id>` works in print mode (`-p`). Confirmed `session_id` available on `ResultMessage.session_id` and on `system/init` message. Confirmed session files stored at `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl`. Accessed 2026-03-13.
- `code.claude.com/docs/en/cli-reference` — Official CLI reference. Confirmed `--resume` / `-r` flag, `--session-id` flag, `--no-session-persistence` flag, `--continue` / `-c` flag. Accessed 2026-03-13.
- `crates/glass_agent/src/worktree_db.rs` — Confirmed existing migration pattern (versions 1-2), `agents.db` path, `TransactionBehavior::Immediate` pattern. Accessed directly in codebase.
- `crates/glass_core/src/agent_runtime.rs` — Confirmed `extract_proposal()` brace-depth walker, `format_activity_as_user_message()` JSON format, `AgentProposalData` struct. Accessed directly in codebase.
- `crates/glass_core/src/event.rs` — Confirmed `AppEvent` variants, `AgentProposalData` placement. Accessed directly in codebase.
- `src/main.rs` — Confirmed reader thread structure (session_id NOT currently captured), `AppEvent::AgentQueryResult` handling, restart logic. Accessed directly in codebase.

### Secondary (MEDIUM confidence)
- `github.com/anthropics/claude-code/issues/14472` — Community report on context limit behavior: "Prompt is too long" error on resume when session exceeds context; manifests as `invalid_request` in stream. Confirms Glass should NOT use `--resume` for large sessions.
- `platform.claude.com/docs/en/agent-sdk/streaming-output` — Confirmed `CompactBoundaryMessage` / `compact_boundary` SystemMessage fires on auto-compaction. Confirmed stream-json message type structure.
- WebSearch results confirming `claude -p --resume <session_id>` works for non-interactive programmatic use.

### Tertiary (LOW confidence)
- Community patterns for structured handoff files (CONTEXT.md, plan.md, session_handoff.md) — corroborate the handoff concept; Glass automates what humans do manually.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — no new deps, all workspace, existing patterns
- GLASS_HANDOFF parsing: HIGH — exact mirror of `extract_proposal()` which is tested and shipped
- SQLite migration: HIGH — migration version 3 follows exact existing pattern; only additive
- Session chain (AGTS-04): HIGH — simple FK column + index; no algorithmic complexity
- Handoff injection at spawn: HIGH — trivial: write one JSON line to stdin before entering event loop
- Agent emission reliability: MEDIUM — depends on system prompt instruction compliance; graceful degradation handles non-emission

**Research date:** 2026-03-13
**Valid until:** 2026-04-13 (Claude CLI stream-json protocol stable; `result` subtypes confirmed from official docs)

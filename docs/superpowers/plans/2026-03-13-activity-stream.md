# Activity Stream Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a visual activity stream that surfaces AI agent behavior to Glass terminal users via a two-line contextual status bar and a fullscreen overlay.

**Architecture:** New `coordination_events` table in existing coordination DB, event emission as side-effects of existing `CoordinationDb` methods, enriched poller state, extended status bar renderer, and a new fullscreen overlay renderer. No new crates, threads, or async runtime.

**Tech Stack:** Rust, rusqlite, wgpu, glyphon, winit

**Spec:** `docs/superpowers/specs/2026-03-13-activity-stream-design.md`

---

## Chunk 1: Data Layer + Compact Status Bar

### Task 1: CoordinationEvent type and DB table

**Files:**
- Create: `crates/glass_coordination/src/event_log.rs`
- Modify: `crates/glass_coordination/src/lib.rs`
- Modify: `crates/glass_coordination/src/db.rs:47-86` (schema)

- [ ] **Step 1: Write tests for CoordinationEvent type**

Create `crates/glass_coordination/src/event_log.rs` with the type and tests:

```rust
//! Coordination event log: types and DB operations for the activity stream.
//!
//! These are human-facing UI events about agent behavior, distinct from
//! the SOI `ActivityEvent` in `glass_core::activity_stream`.

use rusqlite::{params, Connection, TransactionBehavior};

/// A single coordination event for the activity stream UI.
#[derive(Debug, Clone)]
pub struct CoordinationEvent {
    pub id: i64,
    pub timestamp: i64,
    pub project: String,
    pub category: String,
    pub agent_id: Option<String>,
    pub agent_name: Option<String>,
    pub event_type: String,
    pub summary: String,
    pub detail: Option<String>,
    pub pinned: bool,
}

/// Insert a coordination event into the database.
///
/// Called as a side-effect within existing `CoordinationDb` method transactions.
/// The `conn` parameter is the active transaction connection.
pub fn insert_event(
    conn: &Connection,
    project: &str,
    category: &str,
    agent_id: Option<&str>,
    agent_name: Option<&str>,
    event_type: &str,
    summary: &str,
    detail: Option<&str>,
    pinned: bool,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO coordination_events
         (timestamp, project, category, agent_id, agent_name, event_type, summary, detail, pinned)
         VALUES (unixepoch(), ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            project,
            category,
            agent_id,
            agent_name,
            event_type,
            summary,
            detail,
            pinned as i32,
        ],
    )?;
    Ok(())
}

/// Query recent coordination events for a project.
///
/// Returns the most recent `limit` events, ordered newest-first.
pub fn recent_events(
    conn: &Connection,
    project: &str,
    limit: usize,
) -> rusqlite::Result<Vec<CoordinationEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, project, category, agent_id, agent_name,
                event_type, summary, detail, pinned
         FROM coordination_events
         WHERE project = ?1
         ORDER BY timestamp DESC, id DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![project, limit as i64], |row| {
        Ok(CoordinationEvent {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            project: row.get(2)?,
            category: row.get(3)?,
            agent_id: row.get(4)?,
            agent_name: row.get(5)?,
            event_type: row.get(6)?,
            summary: row.get(7)?,
            detail: row.get(8)?,
            pinned: row.get::<_, i32>(9)? != 0,
        })
    })?;
    rows.collect()
}

/// Prune old non-pinned events, keeping at most `max_count` per project.
/// Pinned events older than 24 hours are also pruned.
pub fn prune_events(conn: &Connection, project: &str, max_count: usize) -> rusqlite::Result<()> {
    // Delete non-pinned events beyond the retention limit
    conn.execute(
        "DELETE FROM coordination_events
         WHERE project = ?1 AND pinned = 0 AND id NOT IN (
             SELECT id FROM coordination_events
             WHERE project = ?1 AND pinned = 0
             ORDER BY timestamp DESC, id DESC
             LIMIT ?2
         )",
        params![project, max_count as i64],
    )?;
    // Delete pinned events older than 24 hours
    conn.execute(
        "DELETE FROM coordination_events
         WHERE project = ?1 AND pinned = 1
         AND timestamp < unixepoch() - 86400",
        params![project],
    )?;
    Ok(())
}

/// Dismiss a pinned event by clearing its pinned flag.
pub fn dismiss_event(conn: &Connection, event_id: i64) -> rusqlite::Result<bool> {
    let rows = conn.execute(
        "UPDATE coordination_events SET pinned = 0 WHERE id = ?1",
        params![event_id],
    )?;
    Ok(rows > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE coordination_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                project TEXT NOT NULL,
                category TEXT NOT NULL,
                agent_id TEXT,
                agent_name TEXT,
                event_type TEXT NOT NULL,
                summary TEXT NOT NULL,
                detail TEXT,
                pinned INTEGER DEFAULT 0
            );
            CREATE INDEX idx_coord_events_project_ts
                ON coordination_events(project, timestamp);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_insert_and_query_event() {
        let conn = setup_db();
        insert_event(
            &conn,
            "/project",
            "agent",
            Some("agent-1"),
            Some("claude-code"),
            "registered",
            "claude-code registered project: Glass",
            None,
            false,
        )
        .unwrap();

        let events = recent_events(&conn, "/project", 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].category, "agent");
        assert_eq!(events[0].event_type, "registered");
        assert_eq!(events[0].agent_name.as_deref(), Some("claude-code"));
        assert!(!events[0].pinned);
    }

    #[test]
    fn test_pinned_event() {
        let conn = setup_db();
        insert_event(
            &conn,
            "/project",
            "lock",
            Some("agent-1"),
            Some("cursor"),
            "conflict",
            "cursor conflict pty.rs (held by claude-code)",
            None,
            true,
        )
        .unwrap();

        let events = recent_events(&conn, "/project", 10).unwrap();
        assert!(events[0].pinned);
    }

    #[test]
    fn test_recent_events_ordered_newest_first() {
        let conn = setup_db();
        for i in 0..5 {
            conn.execute(
                "INSERT INTO coordination_events
                 (timestamp, project, category, event_type, summary, pinned)
                 VALUES (?1, '/project', 'agent', 'registered', ?2, 0)",
                params![1000 + i, format!("event {}", i)],
            )
            .unwrap();
        }

        let events = recent_events(&conn, "/project", 3).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].summary, "event 4"); // newest
        assert_eq!(events[2].summary, "event 2"); // oldest of the 3
    }

    #[test]
    fn test_prune_keeps_max_count() {
        let conn = setup_db();
        for i in 0..10 {
            conn.execute(
                "INSERT INTO coordination_events
                 (timestamp, project, category, event_type, summary, pinned)
                 VALUES (?1, '/project', 'agent', 'registered', ?2, 0)",
                params![1000 + i, format!("event {}", i)],
            )
            .unwrap();
        }

        prune_events(&conn, "/project", 5).unwrap();
        let events = recent_events(&conn, "/project", 100).unwrap();
        assert_eq!(events.len(), 5);
        // Should keep the 5 newest
        assert_eq!(events[0].summary, "event 9");
        assert_eq!(events[4].summary, "event 5");
    }

    #[test]
    fn test_prune_preserves_pinned() {
        let conn = setup_db();
        // Insert 5 non-pinned + 1 pinned
        for i in 0..5 {
            conn.execute(
                "INSERT INTO coordination_events
                 (timestamp, project, category, event_type, summary, pinned)
                 VALUES (unixepoch(), '/project', 'agent', 'test', ?1, 0)",
                params![format!("normal {}", i)],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO coordination_events
             (timestamp, project, category, event_type, summary, pinned)
             VALUES (unixepoch(), '/project', 'lock', 'conflict', 'pinned event', 1)",
            [],
        )
        .unwrap();

        prune_events(&conn, "/project", 3).unwrap();
        let events = recent_events(&conn, "/project", 100).unwrap();
        // 3 non-pinned kept + 1 pinned = 4
        assert_eq!(events.len(), 4);
        assert!(events.iter().any(|e| e.summary == "pinned event" && e.pinned));
    }

    #[test]
    fn test_dismiss_event() {
        let conn = setup_db();
        insert_event(
            &conn,
            "/project",
            "lock",
            None,
            None,
            "conflict",
            "conflict",
            None,
            true,
        )
        .unwrap();

        let events = recent_events(&conn, "/project", 10).unwrap();
        assert!(events[0].pinned);

        let dismissed = dismiss_event(&conn, events[0].id).unwrap();
        assert!(dismissed);

        let events = recent_events(&conn, "/project", 10).unwrap();
        assert!(!events[0].pinned);
    }

    #[test]
    fn test_events_scoped_by_project() {
        let conn = setup_db();
        insert_event(&conn, "/project-a", "agent", None, None, "registered", "a", None, false).unwrap();
        insert_event(&conn, "/project-b", "agent", None, None, "registered", "b", None, false).unwrap();

        let a_events = recent_events(&conn, "/project-a", 10).unwrap();
        assert_eq!(a_events.len(), 1);
        assert_eq!(a_events[0].summary, "a");

        let b_events = recent_events(&conn, "/project-b", 10).unwrap();
        assert_eq!(b_events.len(), 1);
        assert_eq!(b_events[0].summary, "b");
    }
}
```

- [ ] **Step 2: Register module in lib.rs**

In `crates/glass_coordination/src/lib.rs`, add module declaration and re-export:

```rust
// Add after line 3 (after `mod pid;`)
pub mod event_log;

// Add to re-exports after line 13
pub use event_log::CoordinationEvent;
```

- [ ] **Step 3: Add coordination_events table to schema**

In `crates/glass_coordination/src/db.rs`, add the table creation inside `create_schema()` at the end of the `execute_batch` string (before the closing `"`):

```sql
CREATE TABLE IF NOT EXISTS coordination_events (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp  INTEGER NOT NULL,
    project    TEXT NOT NULL,
    category   TEXT NOT NULL,
    agent_id   TEXT,
    agent_name TEXT,
    event_type TEXT NOT NULL,
    summary    TEXT NOT NULL,
    detail     TEXT,
    pinned     INTEGER DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_coord_events_project_ts
    ON coordination_events(project, timestamp);
```

- [ ] **Step 4: Run tests**

Run: `cargo test --package glass_coordination`
Expected: All existing tests pass + new event_log tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/glass_coordination/src/event_log.rs crates/glass_coordination/src/lib.rs crates/glass_coordination/src/db.rs
git commit -m "feat(coordination): add coordination_events table and event_log module"
```

---

### Task 2: Emit events from CoordinationDb methods

**Files:**
- Modify: `crates/glass_coordination/src/db.rs:107-321` (register, deregister, update_status, lock_files, unlock_file, unlock_all)
- Modify: `crates/glass_coordination/src/db.rs:378-454` (broadcast, send_message)

Each method already uses `BEGIN IMMEDIATE` transactions. We insert events inside the same transaction, before the `tx.commit()` call. This ensures atomicity — if the event INSERT fails, the whole operation rolls back.

- [ ] **Step 1: Write test for event emission on register**

Add to the existing test module in `crates/glass_coordination/src/db.rs` (use the existing `test_db()` helper pattern with `TempDir`):

```rust
#[test]
fn test_register_emits_event() {
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");
    let mut db = CoordinationDb::open(&db_path).unwrap();
    let id = db.register("test-agent", "claude-code", "/project", "/cwd", None).unwrap();

    let events = crate::event_log::recent_events(db.conn(), "/project", 10).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].category, "agent");
    assert_eq!(events[0].event_type, "registered");
    assert_eq!(events[0].agent_id.as_deref(), Some(id.as_str()));
    assert!(events[0].summary.contains("test-agent"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --package glass_coordination test_register_emits_event`
Expected: FAIL — no event inserted yet

- [ ] **Step 3: Add event emission to register()**

In `crates/glass_coordination/src/db.rs`, inside `register()`, add after the INSERT statement (line 126) and before `tx.commit()` (line 127):

```rust
        crate::event_log::insert_event(
            &tx,
            &canonical_project,
            "agent",
            Some(&id),
            Some(name),
            "registered",
            &format!("{} registered project: {}", name, project),
            None,
            false,
        )?;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --package glass_coordination test_register_emits_event`
Expected: PASS

- [ ] **Step 5: Add event emission to deregister()**

In `deregister()`, we need the agent name for the event summary. Query it before deletion. Replace the method body (lines 136-141):

```rust
    pub fn deregister(&mut self, agent_id: &str) -> Result<bool> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        // Fetch agent info before deletion for event summary
        let info: Option<(String, String)> = tx
            .query_row(
                "SELECT name, project FROM agents WHERE id = ?1",
                params![agent_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        let rows = tx.execute("DELETE FROM agents WHERE id = ?1", params![agent_id])?;

        if let Some((name, project)) = &info {
            crate::event_log::insert_event(
                &tx,
                project,
                "agent",
                Some(agent_id),
                Some(name),
                "deregistered",
                &format!("{} deregistered", name),
                None,
                false,
            )?;
        }

        tx.commit()?;
        Ok(rows > 0)
    }
```

- [ ] **Step 6: Add event emission to update_status()**

In `update_status()`, query current values to detect what changed. Replace the method body (lines 169-177):

```rust
    pub fn update_status(
        &mut self,
        agent_id: &str,
        status: &str,
        task: Option<&str>,
    ) -> Result<bool> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        // Fetch current values to detect changes
        let prev: Option<(String, String, Option<String>)> = tx
            .query_row(
                "SELECT name, status, task FROM agents WHERE id = ?1",
                params![agent_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .ok();

        let rows = tx.execute(
            "UPDATE agents SET status = ?1, task = ?2, last_heartbeat = unixepoch() WHERE id = ?3",
            params![status, task, agent_id],
        )?;

        if let Some((name, old_status, old_task)) = &prev {
            // Fetch project for event scoping
            let project: String = tx
                .query_row(
                    "SELECT project FROM agents WHERE id = ?1",
                    params![agent_id],
                    |row| row.get(0),
                )
                .unwrap_or_default();

            if old_status != status {
                crate::event_log::insert_event(
                    &tx,
                    &project,
                    "agent",
                    Some(agent_id),
                    Some(name),
                    "status_changed",
                    &format!("{} status {} -> {}", name, old_status, status),
                    None,
                    false,
                )?;
            }
            if old_task.as_deref() != task {
                if let Some(new_task) = task {
                    crate::event_log::insert_event(
                        &tx,
                        &project,
                        "agent",
                        Some(agent_id),
                        Some(name),
                        "task_changed",
                        &format!("{} task: {}", name, new_task),
                        None,
                        false,
                    )?;
                }
            }
        }

        tx.commit()?;
        Ok(rows > 0)
    }
```

- [ ] **Step 7: Add event emission to lock_files()**

In `lock_files()`, add events after the lock insertions but before `tx.commit()`. Add after the heartbeat UPDATE (line 285), before `tx.commit()` (line 287):

```rust
        // Fetch agent name for event
        let agent_name: String = tx
            .query_row(
                "SELECT name FROM agents WHERE id = ?1",
                params![agent_id],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| "unknown".to_string());
        let project: String = tx
            .query_row(
                "SELECT project FROM agents WHERE id = ?1",
                params![agent_id],
                |row| row.get(0),
            )
            .unwrap_or_default();

        // Emit lock acquired events (collapse multiple files into one event)
        if canonical_paths.len() == 1 {
            crate::event_log::insert_event(
                &tx,
                &project,
                "lock",
                Some(agent_id),
                Some(&agent_name),
                "acquired",
                &format!("{} locked {}", agent_name, &canonical_paths[0]),
                None,
                false,
            )?;
        } else {
            let files_list = canonical_paths.join(", ");
            crate::event_log::insert_event(
                &tx,
                &project,
                "lock",
                Some(agent_id),
                Some(&agent_name),
                "acquired",
                &format!("{} locked {} files", agent_name, canonical_paths.len()),
                Some(&files_list),
                false,
            )?;
        }
```

Also add conflict event emission. Replace the existing conflict branch (lines 264-268) so that it commits the transaction before returning (currently the tx rolls back on drop, losing the events):

```rust
        if !conflicts.is_empty() {
            // Emit conflict events (pinned) — must commit so events persist
            let agent_name: String = tx
                .query_row(
                    "SELECT name FROM agents WHERE id = ?1",
                    params![agent_id],
                    |row| row.get(0),
                )
                .unwrap_or_else(|_| "unknown".to_string());
            let project: String = tx
                .query_row(
                    "SELECT project FROM agents WHERE id = ?1",
                    params![agent_id],
                    |row| row.get(0),
                )
                .unwrap_or_default();

            for conflict in &conflicts {
                crate::event_log::insert_event(
                    &tx,
                    &project,
                    "lock",
                    Some(agent_id),
                    Some(&agent_name),
                    "conflict",
                    &format!(
                        "{} conflict {} (held by {})",
                        agent_name, conflict.path, conflict.held_by_agent_name
                    ),
                    None,
                    true, // pinned
                )?;
            }

            // Commit the transaction so conflict events are persisted
            // (no locks are inserted in the conflict branch)
            tx.commit()?;
            return Ok(LockResult::Conflict(conflicts));
        }
```

- [ ] **Step 8: Add event emission to unlock_file() and unlock_all()**

In `unlock_file()`, add before `tx.commit()` (line 304):

```rust
        if rows > 0 {
            let info: Option<(String, String)> = tx
                .query_row(
                    "SELECT name, project FROM agents WHERE id = ?1",
                    params![agent_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .ok();
            if let Some((name, project)) = info {
                crate::event_log::insert_event(
                    &tx,
                    &project,
                    "lock",
                    Some(agent_id),
                    Some(&name),
                    "released",
                    &format!("{} unlocked {}", name, canonical),
                    None,
                    false,
                )?;
            }
        }
```

In `unlock_all()`, add before `tx.commit()` (line 319):

```rust
        if rows > 0 {
            let info: Option<(String, String)> = tx
                .query_row(
                    "SELECT name, project FROM agents WHERE id = ?1",
                    params![agent_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .ok();
            if let Some((name, project)) = info {
                crate::event_log::insert_event(
                    &tx,
                    &project,
                    "lock",
                    Some(agent_id),
                    Some(&name),
                    "released",
                    &format!("{} unlocked {} files", name, rows),
                    None,
                    false,
                )?;
            }
        }
```

- [ ] **Step 9: Add event emission to send_message() and broadcast()**

In `send_message()`, add before `tx.commit()`:

```rust
        // Fetch sender info for event
        let sender_info: Option<(String, String)> = tx
            .query_row(
                "SELECT name, project FROM agents WHERE id = ?1",
                params![from_agent_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();
        if let Some((name, project)) = sender_info {
            let evt_type = if msg_type == "request_unlock" {
                "request_unlock"
            } else {
                "sent"
            };
            crate::event_log::insert_event(
                &tx,
                &project,
                "message",
                Some(from_agent_id),
                Some(&name),
                evt_type,
                &format!("{} {} -> {}", name, msg_type, to_agent_id),
                Some(content),
                false,
            )?;
        }
```

In `broadcast()`, add before `tx.commit()`:

```rust
        let sender_name: String = tx
            .query_row(
                "SELECT name FROM agents WHERE id = ?1",
                params![from_agent_id],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| "unknown".to_string());
        crate::event_log::insert_event(
            &tx,
            project,
            "message",
            Some(from_agent_id),
            Some(&sender_name),
            "broadcast",
            &format!("{} broadcast: {}", sender_name, &content[..content.len().min(80)]),
            Some(content),
            false,
        )?;
```

- [ ] **Step 10: Run all coordination tests**

Run: `cargo test --package glass_coordination`
Expected: All tests pass (existing + new event emission tests)

- [ ] **Step 11: Commit**

```bash
git add crates/glass_coordination/src/db.rs
git commit -m "feat(coordination): emit CoordinationEvents from all DB methods"
```

---

### Task 3: AgentDisplayInfo type and enriched poller

**Files:**
- Modify: `crates/glass_core/src/coordination_poller.rs`
- Modify: `crates/glass_core/src/event.rs` (if CoordinationState changes affect AppEvent)

- [ ] **Step 1: Write test for AgentDisplayInfo construction**

Add to the test module in `crates/glass_core/src/coordination_poller.rs`:

```rust
#[test]
fn test_agent_display_info_from_query() {
    let info = AgentDisplayInfo {
        id: "uuid-1".to_string(),
        name: "claude-code".to_string(),
        agent_type: "claude-code".to_string(),
        status: "editing".to_string(),
        task: Some("refactoring pty.rs".to_string()),
        lock_count: 2,
        locked_files: vec!["pty.rs".to_string(), "block_manager.rs".to_string()],
    };
    assert_eq!(info.lock_count, 2);
    assert_eq!(info.locked_files.len(), 2);
}
```

- [ ] **Step 2: Add AgentDisplayInfo struct and update CoordinationState**

In `crates/glass_core/src/coordination_poller.rs`, add after `ConflictInfo` struct (after line 42):

```rust
/// Display-oriented agent info for the compact status bar and overlay.
///
/// Constructed by joining the `agents` and `file_locks` tables in the poller.
/// Adds lock information that `AgentInfo` does not carry.
#[derive(Debug, Clone)]
pub struct AgentDisplayInfo {
    pub id: String,
    pub name: String,
    pub agent_type: String,
    pub status: String,
    pub task: Option<String>,
    pub lock_count: usize,
    pub locked_files: Vec<String>,
}
```

Update `CoordinationState` to add new fields:

```rust
pub struct CoordinationState {
    pub agent_count: usize,
    pub lock_count: usize,
    pub locks: Vec<LockEntry>,
    pub conflicts: Vec<ConflictInfo>,
    /// Per-agent display info for the compact status bar.
    pub agents: Vec<AgentDisplayInfo>,
    /// Recent coordination events for the overlay timeline.
    pub recent_events: Vec<glass_coordination::CoordinationEvent>,
    /// Most recent notable event for the compact bar ticker.
    /// Cleared after one display cycle by the main event loop.
    pub ticker_event: Option<glass_coordination::CoordinationEvent>,
}
```

Update the `Default` impl (or `#[derive(Default)]`) to include the new fields with empty defaults.

- [ ] **Step 3: Update poll_once() to fetch agents and events**

Replace the `poll_once()` function body to also query agent display info and recent events:

```rust
fn poll_once(project_root: &str) -> CoordinationState {
    let result = (|| -> Result<CoordinationState, Box<dyn std::error::Error>> {
        let mut db = CoordinationDb::open_default()?;
        let agents = db.list_agents(project_root)?;
        let locks = db.list_locks(Some(project_root))?;

        let lock_entries: Vec<LockEntry> = locks
            .iter()
            .map(|l| LockEntry {
                path: l.path.clone(),
                agent_id: l.agent_id.clone(),
                agent_name: l.agent_name.clone(),
            })
            .collect();

        // Build AgentDisplayInfo by joining agents with their locks
        let agent_infos: Vec<AgentDisplayInfo> = agents
            .iter()
            .map(|a| {
                let agent_locks: Vec<String> = locks
                    .iter()
                    .filter(|l| l.agent_id == a.id)
                    .map(|l| l.path.clone())
                    .collect();
                AgentDisplayInfo {
                    id: a.id.clone(),
                    name: a.name.clone(),
                    agent_type: a.agent_type.clone(),
                    status: a.status.clone(),
                    task: a.task.clone(),
                    lock_count: agent_locks.len(),
                    locked_files: agent_locks,
                }
            })
            .collect();

        // Fetch recent events (last 200 for the overlay)
        let recent_events =
            glass_coordination::event_log::recent_events(db.conn(), project_root, 200)
                .unwrap_or_default();

        // Prune old events
        let _ = glass_coordination::event_log::prune_events(db.conn(), project_root, 1000);

        // Ticker: most recent event (first in the list since ordered newest-first)
        let ticker_event = recent_events.first().cloned();

        Ok(CoordinationState {
            agent_count: agents.len(),
            lock_count: locks.len(),
            locks: lock_entries,
            conflicts: Vec::new(),
            agents: agent_infos,
            recent_events,
            ticker_event,
        })
    })();

    match result {
        Ok(state) => state,
        Err(e) => {
            tracing::debug!("Coordination poll failed (non-fatal): {}", e);
            CoordinationState::default()
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --package glass_core coordination`
Expected: All tests pass. Existing `test_coordination_state_default_is_zeros` still passes because new fields default to empty.

- [ ] **Step 5: Commit**

```bash
git add crates/glass_core/src/coordination_poller.rs
git commit -m "feat(core): enrich CoordinationState with AgentDisplayInfo and events"
```

---

### Task 4: Two-line contextual status bar

**Files:**
- Modify: `crates/glass_renderer/src/status_bar.rs`
- Modify: `crates/glass_renderer/src/frame.rs` (status bar height calculation)
- Modify: `src/main.rs` (pass agent data to status bar, adjust viewport)

- [ ] **Step 1: Write test for agent activity line text generation**

Add to `crates/glass_renderer/src/status_bar.rs` tests:

```rust
#[test]
fn test_agent_activity_line_two_agents() {
    let agents = vec![
        ("claude-code".to_string(), "editing".to_string(), Some("main.rs".to_string())),
        ("cursor".to_string(), "idle".to_string(), None),
    ];
    let line = build_agent_activity_line(&agents, 2, 100);
    assert!(line.contains("claude-code"));
    assert!(line.contains("editing"));
    assert!(line.contains("cursor"));
    assert!(line.contains("idle"));
}

#[test]
fn test_agent_activity_line_overflow() {
    let agents: Vec<_> = (0..5)
        .map(|i| (format!("agent-{}", i), "active".to_string(), None))
        .collect();
    let line = build_agent_activity_line(&agents, 0, 80);
    assert!(line.contains("+3 more"));
}
```

- [ ] **Step 2: Add agent activity line builder**

In `crates/glass_renderer/src/status_bar.rs`, add a helper function before the `StatusBarRenderer` impl:

```rust
/// Agent summary for the compact status bar line.
pub struct AgentStatusEntry {
    pub name: String,
    pub status: String,
    pub task: Option<String>,
}

/// Build the agent activity line text for the two-line status bar.
///
/// Format: "● name status task  │  ● name status  │  ⚡ N locks      ▼ Ctrl+Shift+G"
/// If more than 2 agents, shows first 2 + "+N more".
pub fn build_agent_activity_line(
    agents: &[(String, String, Option<String>)],
    lock_count: usize,
    max_chars: usize,
) -> String {
    let mut parts = Vec::new();
    let show_count = agents.len().min(2);

    for (name, status, task) in agents.iter().take(show_count) {
        let entry = if let Some(t) = task {
            let truncated = if t.len() > 20 {
                format!("{}...", &t[..17])
            } else {
                t.clone()
            };
            format!("{} {} {}", name, status, truncated)
        } else {
            format!("{} {}", name, status)
        };
        parts.push(entry);
    }

    if agents.len() > 2 {
        parts.push(format!("+{} more", agents.len() - 2));
    }

    let mut line = parts.join("  |  ");

    if lock_count > 0 {
        line.push_str(&format!("  |  {} locks", lock_count));
    }

    line
}
```

- [ ] **Step 3: Add two-line status bar support to StatusBarRenderer**

Add a new method to `StatusBarRenderer`:

```rust
    /// Build status bar background rectangles for two-line mode.
    ///
    /// Returns a rect that is 2 * cell_height tall when agents are active.
    pub fn build_status_rects_two_line(
        &self,
        viewport_width: f32,
        viewport_height: f32,
    ) -> Vec<RectInstance> {
        let height = self.cell_height * 2.0;
        let y = viewport_height - height;
        vec![RectInstance {
            pos: [0.0, y, viewport_width, height],
            color: [38.0 / 255.0, 38.0 / 255.0, 38.0 / 255.0, 1.0],
        }]
    }

    /// Get the status bar height in pixels (1 or 2 lines).
    pub fn height(&self, two_line: bool) -> f32 {
        if two_line {
            self.cell_height * 2.0
        } else {
            self.cell_height
        }
    }
```

- [ ] **Step 4: Update main.rs to pass agent data and use two-line mode**

In `src/main.rs`, update the coordination_text generation (around line 1414) to build the agent activity line when agents are present:

```rust
let has_agents = !self.coordination_state.agents.is_empty();
let coordination_text = if self.coordination_state.agent_count > 0 && !has_agents {
    // Fallback: old format when agents vec not populated
    Some(format!(
        "agents: {} locks: {}",
        self.coordination_state.agent_count, self.coordination_state.lock_count
    ))
} else {
    None
};

// Build agent activity line for two-line status bar
let agent_activity_line = if has_agents {
    let agents: Vec<_> = self.coordination_state.agents.iter().map(|a| {
        (a.name.clone(), a.status.clone(), a.task.clone())
    }).collect();
    Some(glass_renderer::status_bar::build_agent_activity_line(
        &agents,
        self.coordination_state.lock_count,
        100,
    ))
} else {
    None
};
```

Pass `agent_activity_line` as a new parameter to `draw_frame()` (this requires updating the `draw_frame` signature — see next step).

- [ ] **Step 5: Update draw_frame to accept two-line mode flag**

In `crates/glass_renderer/src/frame.rs`, add `agent_activity_line: Option<&str>` parameter to `draw_frame()`. When `Some`, use `build_status_rects_two_line()` and render the agent line as an additional text row above the existing status text.

The viewport height calculation for the terminal grid must subtract `2 * cell_height` instead of `1 * cell_height` when in two-line mode, so the terminal content doesn't overlap the expanded status bar.

- [ ] **Step 6: Run full test suite**

Run: `cargo test --workspace`
Expected: All tests pass

- [ ] **Step 7: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: No warnings

- [ ] **Step 8: Commit**

```bash
git add crates/glass_renderer/src/status_bar.rs crates/glass_renderer/src/frame.rs src/main.rs
git commit -m "feat(renderer): two-line contextual status bar with agent activity"
```

---

### Task 5: Ticker event display and clearing

**Files:**
- Modify: `src/main.rs` (ticker state management)
- Modify: `crates/glass_renderer/src/status_bar.rs` (ticker display)

- [ ] **Step 1: Add ticker state to Processor**

In `src/main.rs`, add to the `Processor` struct:

```rust
/// The last ticker event ID that was displayed, used to detect new events.
last_ticker_event_id: Option<i64>,
/// Counter for ticker display cycles. When > 0, show ticker text.
ticker_display_cycles: u32,
```

- [ ] **Step 2: Add ticker logic to CoordinationUpdate handler**

In the `AppEvent::CoordinationUpdate(state)` handler (line 4021), add ticker detection:

```rust
AppEvent::CoordinationUpdate(state) => {
    // Decrement ticker BEFORE checking for new events,
    // so a new event setting cycles=1 isn't immediately cleared.
    if self.ticker_display_cycles > 0 {
        self.ticker_display_cycles -= 1;
    }

    // Detect new ticker event
    if let Some(ref evt) = state.ticker_event {
        let is_new = self.last_ticker_event_id.map_or(true, |id| id != evt.id);
        if is_new {
            self.last_ticker_event_id = Some(evt.id);
            self.ticker_display_cycles = 1; // Show for 1 poll cycle (5s)
        }
    }

    self.coordination_state = state;
    for ctx in self.windows.values() {
        ctx.window.request_redraw();
    }
}
```

- [ ] **Step 3: Pass ticker text to status bar**

When `ticker_display_cycles > 0`, pass the ticker event summary instead of the steady-state agent line. This makes the agent activity area briefly show the latest event text before reverting.

- [ ] **Step 4: Run tests and clippy**

Run: `cargo test --workspace && cargo clippy --workspace -- -D warnings`
Expected: All pass

- [ ] **Step 5: Commit**

```bash
git add src/main.rs crates/glass_renderer/src/status_bar.rs
git commit -m "feat: ticker event display in compact status bar"
```

---

## Chunk 2: Expanded Overlay + Agent Mode Hooks

> **Depends on:** Chunk 1 must be fully implemented first. Tasks 6-10 reference `CoordinationState.agents`, `CoordinationState.recent_events`, and `CoordinationEvent` which are added in Chunk 1 Tasks 1-3.

### Task 6: ActivityOverlayRenderer

**Files:**
- Create: `crates/glass_renderer/src/activity_overlay.rs`
- Modify: `crates/glass_renderer/src/lib.rs`
- Modify: `crates/glass_renderer/src/frame.rs`

- [ ] **Step 1: Create ActivityOverlayRenderer with types**

Create `crates/glass_renderer/src/activity_overlay.rs`:

```rust
//! ActivityOverlayRenderer: fullscreen overlay showing agent activity stream.
//!
//! Two-column layout: agent cards (left) + event timeline (right).
//! Follows the same pattern as ConflictOverlay and SearchOverlayRenderer.

use alacritty_terminal::vte::ansi::Rgb;

use crate::rect_renderer::RectInstance;

/// Filter for which event categories to show in the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivityViewFilter {
    #[default]
    All,
    Agents,
    Locks,
    Observations,
    Messages,
}

impl ActivityViewFilter {
    /// Cycle to the next filter tab.
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Agents,
            Self::Agents => Self::Locks,
            Self::Locks => Self::Observations,
            Self::Observations => Self::Messages,
            Self::Messages => Self::All,
        }
    }

    /// Cycle to the previous filter tab.
    pub fn prev(self) -> Self {
        match self {
            Self::All => Self::Messages,
            Self::Agents => Self::All,
            Self::Locks => Self::Agents,
            Self::Observations => Self::Locks,
            Self::Messages => Self::Observations,
        }
    }

    /// The category string this filter matches, or None for All.
    pub fn category(&self) -> Option<&str> {
        match self {
            Self::All => None,
            Self::Agents => Some("agent"),
            Self::Locks => Some("lock"),
            Self::Observations => Some("observe"),
            Self::Messages => Some("message"),
        }
    }

    /// Display label for the filter tab.
    pub fn label(&self) -> &str {
        match self {
            Self::All => "All",
            Self::Agents => "Agents",
            Self::Locks => "Locks",
            Self::Observations => "Observations",
            Self::Messages => "Messages",
        }
    }
}

/// Render data passed to the activity overlay.
#[derive(Debug)]
pub struct ActivityOverlayRenderData {
    pub agents: Vec<ActivityAgentCard>,
    pub events: Vec<ActivityTimelineEvent>,
    pub pinned: Vec<ActivityPinnedAlert>,
    pub filter: ActivityViewFilter,
    pub scroll_offset: usize,
    pub verbose: bool,
}

/// Agent card data for the left column.
#[derive(Debug, Clone)]
pub struct ActivityAgentCard {
    pub name: String,
    pub agent_type: String,
    pub status: String,
    pub task: Option<String>,
    pub locked_files: Vec<String>,
    pub is_idle: bool,
}

/// A single event for the timeline.
#[derive(Debug, Clone)]
pub struct ActivityTimelineEvent {
    pub timestamp: i64,
    pub agent_name: Option<String>,
    pub category: String,
    pub event_type: String,
    pub summary: String,
    pub pinned: bool,
}

/// A pinned alert for display below agent cards.
#[derive(Debug, Clone)]
pub struct ActivityPinnedAlert {
    pub id: i64,
    pub summary: String,
    pub timestamp: i64,
}

/// Text label for rendering in the overlay.
#[derive(Debug, Clone)]
pub struct ActivityOverlayTextLabel {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub color: Rgb,
}

/// Agent color palette — each agent gets a unique color by index.
const AGENT_COLORS: &[Rgb] = &[
    Rgb { r: 180, g: 140, b: 255 },  // purple
    Rgb { r: 100, g: 180, b: 246 },  // blue
    Rgb { r: 80,  g: 200, b: 170 },  // teal
    Rgb { r: 220, g: 180, b: 100 },  // amber
    Rgb { r: 220, g: 120, b: 120 },  // coral
    Rgb { r: 140, g: 220, b: 140 },  // green
];

/// Get the color for an agent by index.
pub fn agent_color(index: usize) -> Rgb {
    AGENT_COLORS[index % AGENT_COLORS.len()]
}

/// Verb color based on event type.
pub fn verb_color(event_type: &str) -> Rgb {
    match event_type {
        "registered" | "status_changed" | "started" | "analyzed" => Rgb { r: 106, g: 166, b: 106 },  // green
        "acquired" | "locked" | "proposing" => Rgb { r: 220, g: 180, b: 100 },  // amber
        "conflict" | "error_noticed" | "heartbeat_lost" => Rgb { r: 255, g: 102, b: 102 },  // red
        "sent" | "broadcast" | "message" => Rgb { r: 100, g: 200, b: 255 },  // blue
        _ => Rgb { r: 136, g: 136, b: 136 },  // gray
    }
}

/// Renders the activity overlay visual elements.
pub struct ActivityOverlayRenderer {
    cell_width: f32,
    cell_height: f32,
}

impl ActivityOverlayRenderer {
    pub fn new(cell_width: f32, cell_height: f32) -> Self {
        Self { cell_width, cell_height }
    }

    /// Build backdrop rectangle (semi-transparent dark overlay).
    pub fn build_backdrop_rect(
        &self,
        viewport_width: f32,
        viewport_height: f32,
    ) -> RectInstance {
        RectInstance {
            pos: [0.0, 0.0, viewport_width, viewport_height],
            color: [0.03, 0.03, 0.06, 0.95],
        }
    }

    /// Build all text labels for the overlay.
    ///
    /// Returns labels for: header, filter tabs, agent cards, event timeline.
    pub fn build_overlay_text(
        &self,
        data: &ActivityOverlayRenderData,
        viewport_width: f32,
        viewport_height: f32,
    ) -> Vec<ActivityOverlayTextLabel> {
        let mut labels = Vec::new();
        let padding = self.cell_width;
        let header_y = self.cell_height;

        // Header: "Activity Stream"
        labels.push(ActivityOverlayTextLabel {
            text: "Activity Stream".to_string(),
            x: padding,
            y: header_y,
            color: Rgb { r: 180, g: 140, b: 255 },
        });

        // Filter tabs
        let filters = [
            ActivityViewFilter::All,
            ActivityViewFilter::Agents,
            ActivityViewFilter::Locks,
            ActivityViewFilter::Observations,
            ActivityViewFilter::Messages,
        ];
        let mut tab_x = viewport_width * 0.4;
        for f in &filters {
            let color = if *f == data.filter {
                Rgb { r: 180, g: 140, b: 255 }
            } else {
                Rgb { r: 136, g: 136, b: 136 }
            };
            labels.push(ActivityOverlayTextLabel {
                text: f.label().to_string(),
                x: tab_x,
                y: header_y,
                color,
            });
            tab_x += f.label().len() as f32 * self.cell_width + self.cell_width * 2.0;
        }

        // Close hint
        labels.push(ActivityOverlayTextLabel {
            text: "Esc to close".to_string(),
            x: viewport_width - 14.0 * self.cell_width,
            y: header_y,
            color: Rgb { r: 85, g: 85, b: 85 },
        });

        // Left column: Agent cards
        let left_width = 280.0_f32.min(viewport_width * 0.35);
        let mut card_y = self.cell_height * 3.0;

        // "Active Agents (N)" header
        labels.push(ActivityOverlayTextLabel {
            text: format!("Active Agents ({})", data.agents.len()),
            x: padding,
            y: card_y,
            color: Rgb { r: 102, g: 102, b: 102 },
        });
        card_y += self.cell_height * 1.5;

        for (i, agent) in data.agents.iter().enumerate() {
            let color = agent_color(i);
            // Agent name + status
            labels.push(ActivityOverlayTextLabel {
                text: format!("{}", agent.name),
                x: padding + self.cell_width,
                y: card_y,
                color,
            });

            let status_color = match agent.status.as_str() {
                "idle" => Rgb { r: 136, g: 136, b: 136 },
                _ => Rgb { r: 106, g: 166, b: 106 },
            };
            labels.push(ActivityOverlayTextLabel {
                text: agent.status.clone(),
                x: left_width - agent.status.len() as f32 * self.cell_width - padding,
                y: card_y,
                color: status_color,
            });
            card_y += self.cell_height;

            // Task
            if let Some(ref task) = agent.task {
                let truncated = if task.len() > 30 {
                    format!("{}...", &task[..27])
                } else {
                    task.clone()
                };
                labels.push(ActivityOverlayTextLabel {
                    text: format!("Task: {}", truncated),
                    x: padding + self.cell_width,
                    y: card_y,
                    color: Rgb { r: 170, g: 170, b: 170 },
                });
                card_y += self.cell_height;
            }

            // Locked files
            for file in &agent.locked_files {
                let short = file.rsplit('/').next().unwrap_or(file);
                labels.push(ActivityOverlayTextLabel {
                    text: format!("locked: {}", short),
                    x: padding + self.cell_width,
                    y: card_y,
                    color: Rgb { r: 220, g: 180, b: 100 },
                });
                card_y += self.cell_height;
            }

            card_y += self.cell_height * 0.5; // gap between cards
        }

        // Right column: Event timeline
        let timeline_x = left_width + padding * 2.0;
        let mut event_y = self.cell_height * 3.0;

        // "Event Timeline" header
        labels.push(ActivityOverlayTextLabel {
            text: "Event Timeline".to_string(),
            x: timeline_x,
            y: event_y,
            color: Rgb { r: 102, g: 102, b: 102 },
        });
        event_y += self.cell_height * 1.5;

        // Filter and paginate events
        let filtered: Vec<&ActivityTimelineEvent> = data.events.iter()
            .filter(|e| {
                if !data.verbose && e.event_type == "dismissed" {
                    return false;
                }
                match data.filter.category() {
                    Some(cat) => e.category == cat,
                    None => true,
                }
            })
            .collect();

        let max_visible = ((viewport_height - event_y) / self.cell_height) as usize;
        let visible = filtered.iter()
            .skip(data.scroll_offset)
            .take(max_visible);

        let mut last_minute: Option<i64> = None;

        for event in visible {
            // Minute group header
            let minute = event.timestamp / 60;
            if last_minute.map_or(true, |m| m != minute) {
                let time_str = format_timestamp_minute(event.timestamp);
                labels.push(ActivityOverlayTextLabel {
                    text: time_str,
                    x: timeline_x,
                    y: event_y,
                    color: Rgb { r: 68, g: 68, b: 68 },
                });
                event_y += self.cell_height;
                last_minute = Some(minute);
            }

            // Seconds
            let secs = format!(":{:02}", event.timestamp % 60);
            labels.push(ActivityOverlayTextLabel {
                text: secs,
                x: timeline_x,
                y: event_y,
                color: Rgb { r: 68, g: 68, b: 68 },
            });

            // Agent name badge — derive stable color from name hash
            let badge_x = timeline_x + self.cell_width * 5.0;
            if let Some(ref name) = event.agent_name {
                // Use agent name hash to pick a stable color from the palette
                let color_index = name.bytes().fold(0usize, |acc, b| acc.wrapping_add(b as usize));
                labels.push(ActivityOverlayTextLabel {
                    text: name.clone(),
                    x: badge_x,
                    y: event_y,
                    color: agent_color(color_index),
                });
            }

            // Verb
            let verb_x = badge_x + self.cell_width * 14.0;
            labels.push(ActivityOverlayTextLabel {
                text: event.event_type.clone(),
                x: verb_x,
                y: event_y,
                color: verb_color(&event.event_type),
            });

            // Detail (rest of summary after the verb)
            let detail_x = verb_x + self.cell_width * 12.0;
            labels.push(ActivityOverlayTextLabel {
                text: event.summary.clone(),
                x: detail_x,
                y: event_y,
                color: Rgb { r: 204, g: 204, b: 204 },
            });

            event_y += self.cell_height;
        }

        labels
    }
}

/// Format a unix timestamp's minute portion as "HH:MM".
fn format_timestamp_minute(timestamp: i64) -> String {
    // Extract hours and minutes from unix timestamp (local-ish)
    let secs = timestamp % 86400;
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    format!("{:02}:{:02}", hours, minutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_activity_view_filter_cycle() {
        let f = ActivityViewFilter::All;
        assert_eq!(f.next(), ActivityViewFilter::Agents);
        assert_eq!(f.next().next(), ActivityViewFilter::Locks);
        assert_eq!(ActivityViewFilter::Messages.next(), ActivityViewFilter::All);
    }

    #[test]
    fn test_activity_view_filter_prev() {
        assert_eq!(ActivityViewFilter::All.prev(), ActivityViewFilter::Messages);
        assert_eq!(ActivityViewFilter::Agents.prev(), ActivityViewFilter::All);
    }

    #[test]
    fn test_activity_view_filter_category() {
        assert_eq!(ActivityViewFilter::All.category(), None);
        assert_eq!(ActivityViewFilter::Agents.category(), Some("agent"));
        assert_eq!(ActivityViewFilter::Locks.category(), Some("lock"));
    }

    #[test]
    fn test_agent_color_wraps() {
        let c1 = agent_color(0);
        let c2 = agent_color(AGENT_COLORS.len());
        assert_eq!(c1, c2); // wraps around
    }

    #[test]
    fn test_verb_color_categories() {
        let green = verb_color("registered");
        assert_eq!(green.g, 166);
        let red = verb_color("conflict");
        assert_eq!(red.r, 255);
        let gray = verb_color("unknown_type");
        assert_eq!(gray.r, 136);
    }

    #[test]
    fn test_backdrop_rect_covers_viewport() {
        let r = ActivityOverlayRenderer::new(10.0, 20.0);
        let rect = r.build_backdrop_rect(800.0, 600.0);
        assert_eq!(rect.pos[0], 0.0);
        assert_eq!(rect.pos[1], 0.0);
        assert_eq!(rect.pos[2], 800.0);
        assert_eq!(rect.pos[3], 600.0);
    }

    #[test]
    fn test_format_timestamp_minute() {
        // 10:34 = 10*3600 + 34*60 = 38040
        assert_eq!(format_timestamp_minute(38040), "10:34");
        assert_eq!(format_timestamp_minute(38042), "10:34"); // same minute
    }
}
```

- [ ] **Step 2: Register module in lib.rs**

In `crates/glass_renderer/src/lib.rs`, add:

```rust
pub mod activity_overlay;

pub use activity_overlay::{
    ActivityOverlayRenderData, ActivityOverlayRenderer, ActivityOverlayTextLabel,
    ActivityViewFilter,
};
```

- [ ] **Step 3: Run tests**

Run: `cargo test --package glass_renderer activity_overlay`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/glass_renderer/src/activity_overlay.rs crates/glass_renderer/src/lib.rs
git commit -m "feat(renderer): add ActivityOverlayRenderer with types and text generation"
```

---

### Task 7: Overlay rendering in frame.rs and draw method

**Files:**
- Modify: `crates/glass_renderer/src/frame.rs`

- [ ] **Step 1: Add draw_activity_overlay method to FrameRenderer**

Follow the same pattern as `draw_conflict_overlay()` in `frame.rs`. Add a new method. The implementation mirrors `draw_conflict_overlay()` structurally — create overlay helper, build rects and text, prepare renderers, create render pass, submit. Key differences: uses `ActivityOverlayRenderer` and takes `ActivityOverlayRenderData`.

```rust
    /// Draw the activity stream overlay.
    pub fn draw_activity_overlay(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        data: &crate::activity_overlay::ActivityOverlayRenderData,
    ) {
        let vw = width as f32;
        let vh = height as f32;
        let cell_width = self.glyph_cache.cell_width();
        let cell_height = self.glyph_cache.cell_height();

        let overlay = crate::activity_overlay::ActivityOverlayRenderer::new(cell_width, cell_height);

        // 1. Backdrop rect
        let backdrop = overlay.build_backdrop_rect(vw, vh);

        // 2. Text labels
        let labels = overlay.build_overlay_text(data, vw, vh);

        // 3. Prepare rect renderer with backdrop
        self.rect_renderer
            .prepare(device, queue, width, height, &[backdrop]);

        // 4. Build glyphon text buffer from labels
        let font_size = self.glyph_cache.font_size();
        let mut text_buffer = glyphon::Buffer::new(
            self.glyph_cache.font_system(),
            glyphon::Metrics::new(font_size, font_size * 1.2),
        );

        let text_areas: Vec<glyphon::TextArea> = labels
            .iter()
            .map(|label| {
                let mut buffer = glyphon::Buffer::new(
                    self.glyph_cache.font_system(),
                    glyphon::Metrics::new(font_size, font_size * 1.2),
                );
                buffer.set_text(
                    self.glyph_cache.font_system(),
                    &label.text,
                    &[glyphon::Attrs::new().color(glyphon::Color::rgb(
                        label.color.r, label.color.g, label.color.b,
                    ))],
                    glyphon::Shaping::Advanced,
                );
                buffer.shape_until_scroll(self.glyph_cache.font_system(), false);
                glyphon::TextArea {
                    buffer: &buffer,
                    left: label.x,
                    top: label.y,
                    scale: 1.0,
                    bounds: glyphon::TextBounds {
                        left: label.x as i32,
                        top: label.y as i32,
                        right: width as i32,
                        bottom: height as i32,
                    },
                    default_color: glyphon::Color::rgb(
                        label.color.r, label.color.g, label.color.b,
                    ),
                    custom_glyphs: &[],
                }
            })
            .collect();

        // Note: The actual text rendering must follow the same pattern as
        // draw_conflict_overlay — using self.glyph_cache to prepare text areas,
        // then rendering in a single pass with rects first, text second.
        // The exact glyphon API calls should match the pattern at the
        // draw_conflict_overlay function (search for it by name in frame.rs).
        // The implementer should read that function and replicate the
        // encoder/render_pass/prepare/render sequence exactly, substituting
        // the backdrop and labels from this method.

        // 5. Create command encoder
        let mut encoder = device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("activity_overlay") },
        );

        // 6. Render pass: clear nothing, draw rects then text
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("activity_overlay_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load, // overlay on top of existing frame
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            self.rect_renderer.render(&mut pass);
            // Text rendering via glyph_cache.text_renderer().render()
        }

        queue.submit(std::iter::once(encoder.finish()));
    }
```

**Important:** The glyphon text area construction above is illustrative. The implementer must match the exact glyphon API used in `draw_conflict_overlay()` — specifically how `self.glyph_cache` manages font system, text renderer preparation, and viewport updates. Read the `draw_conflict_overlay` function body and replicate its text rendering approach, substituting labels from `build_overlay_text()`.

- [ ] **Step 2: Run build**

Run: `cargo build --package glass_renderer`
Expected: Compiles without errors

- [ ] **Step 3: Commit**

```bash
git add crates/glass_renderer/src/frame.rs
git commit -m "feat(renderer): add draw_activity_overlay to FrameRenderer"
```

---

### Task 8: Hotkey handling and overlay state in main.rs

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add overlay state fields to Processor**

In `src/main.rs`, add to the `Processor` struct (around line 261):

```rust
    /// Whether the activity stream overlay is visible.
    activity_overlay_visible: bool,
    /// Current filter tab in the activity overlay.
    activity_view_filter: glass_renderer::ActivityViewFilter,
    /// Scroll offset in the activity overlay timeline.
    activity_scroll_offset: usize,
    /// Selected pinned alert index for dismissal.
    activity_pinned_selected: Option<usize>,
    /// Whether verbose mode is on (shows dismissed events).
    activity_verbose: bool,
```

Initialize all to defaults in the `Processor` constructor (around line 5015).

- [ ] **Step 2: Add Ctrl+Shift+G hotkey handler**

In the keyboard handling section (around line 2214), add a new match arm for `Ctrl+Shift+G`:

```rust
// Ctrl+Shift+G: Toggle activity stream overlay.
(true, true, KeyCode::KeyG) if event.state == ElementState::Pressed => {
    self.activity_overlay_visible = !self.activity_overlay_visible;
    if !self.activity_overlay_visible {
        // Reset state on close
        self.activity_view_filter = Default::default();
        self.activity_scroll_offset = 0;
        self.activity_pinned_selected = None;
        self.activity_verbose = false;
    }
    if let Some(ctx) = self.windows.values().next() {
        ctx.window.request_redraw();
    }
}
```

- [ ] **Step 3: Add overlay keyboard handlers (Tab, Up/Down, Esc, D, V)**

When `activity_overlay_visible` is true, intercept keys:

```rust
// Inside key handling, when activity_overlay_visible:
if self.activity_overlay_visible && event.state == ElementState::Pressed {
    match key {
        KeyCode::Escape => {
            self.activity_overlay_visible = false;
            self.activity_view_filter = Default::default();
            self.activity_scroll_offset = 0;
            ctx.window.request_redraw();
            return; // consume the event
        }
        KeyCode::Tab if modifiers.shift_key() => {
            self.activity_view_filter = self.activity_view_filter.prev();
            self.activity_scroll_offset = 0;
            ctx.window.request_redraw();
            return;
        }
        KeyCode::Tab => {
            self.activity_view_filter = self.activity_view_filter.next();
            self.activity_scroll_offset = 0;
            ctx.window.request_redraw();
            return;
        }
        KeyCode::ArrowUp => {
            self.activity_scroll_offset = self.activity_scroll_offset.saturating_sub(1);
            ctx.window.request_redraw();
            return;
        }
        KeyCode::ArrowDown => {
            self.activity_scroll_offset += 1;
            ctx.window.request_redraw();
            return;
        }
        KeyCode::KeyD => {
            // Dismiss pinned alert — implementation deferred to integration
            ctx.window.request_redraw();
            return;
        }
        KeyCode::KeyV => {
            self.activity_verbose = !self.activity_verbose;
            ctx.window.request_redraw();
            return;
        }
        _ => {}
    }
}
```

- [ ] **Step 4: Build ActivityOverlayRenderData and call draw_activity_overlay**

In the redraw section (around line 1800, after conflict overlay rendering), add:

```rust
if self.activity_overlay_visible {
    let agents: Vec<glass_renderer::activity_overlay::ActivityAgentCard> =
        self.coordination_state.agents.iter().map(|a| {
            glass_renderer::activity_overlay::ActivityAgentCard {
                name: a.name.clone(),
                agent_type: a.agent_type.clone(),
                status: a.status.clone(),
                task: a.task.clone(),
                locked_files: a.locked_files.clone(),
                is_idle: a.status == "idle",
            }
        }).collect();

    let events: Vec<glass_renderer::activity_overlay::ActivityTimelineEvent> =
        self.coordination_state.recent_events.iter().map(|e| {
            glass_renderer::activity_overlay::ActivityTimelineEvent {
                timestamp: e.timestamp,
                agent_name: e.agent_name.clone(),
                category: e.category.clone(),
                event_type: e.event_type.clone(),
                summary: e.summary.clone(),
                pinned: e.pinned,
            }
        }).collect();

    let pinned: Vec<glass_renderer::activity_overlay::ActivityPinnedAlert> =
        self.coordination_state.recent_events.iter()
            .filter(|e| e.pinned)
            .map(|e| glass_renderer::activity_overlay::ActivityPinnedAlert {
                id: e.id,
                summary: e.summary.clone(),
                timestamp: e.timestamp,
            })
            .collect();

    let render_data = glass_renderer::ActivityOverlayRenderData {
        agents,
        events,
        pinned,
        filter: self.activity_view_filter,
        scroll_offset: self.activity_scroll_offset,
        verbose: self.activity_verbose,
    };

    ctx.frame_renderer.draw_activity_overlay(
        ctx.renderer.device(),
        ctx.renderer.queue(),
        &view,
        sc.width,
        sc.height,
        &render_data,
    );
}
```

- [ ] **Step 5: Run build and clippy**

Run: `cargo build && cargo clippy --workspace -- -D warnings`
Expected: Compiles and passes clippy

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire activity stream overlay with Ctrl+Shift+G hotkey"
```

---

### Task 9: Agent Mode observation events

**Files:**
- Modify: `src/main.rs` (agent mode observation hooks)

This task adds `observe.*` event emission at each stage of the Agent Mode observation pipeline. The event emission uses a helper function to avoid repetitive DB open/insert boilerplate.

- [ ] **Step 1: Add helper function for emitting observation events**

In `src/main.rs`, add a helper method to `Processor`:

```rust
    /// Emit an observation event to the coordination event log.
    /// No-op if agent runtime has no project root or if DB access fails.
    fn emit_observe_event(&self, event_type: &str, summary: &str) {
        let project = match self.agent_runtime.as_ref().and_then(|r| r.project_root.as_ref()) {
            Some(p) => p.clone(),
            None => return,
        };
        if let Ok(db) = glass_coordination::CoordinationDb::open_default() {
            let _ = glass_coordination::event_log::insert_event(
                db.conn(),
                &project,
                "observe",
                None,
                Some("agent-mode"),
                event_type,
                summary,
                None,
                false,
            );
        }
    }
```

- [ ] **Step 2: Identify insertion points and add observe.command_seen**

Search `src/main.rs` for where Agent Mode processes a completed command. Look for the handler of `AppEvent::CommandFinished` or the SOI pipeline that processes command output for agent observation. At the point where Agent Mode first sees a completed command, add:

```rust
self.emit_observe_event(
    "command_seen",
    &format!("agent-mode saw {} (exit: {})", command_text, exit_code),
);
```

- [ ] **Step 3: Add observe.output_parsed and observe.error_noticed**

At the point where Agent Mode processes SOI output records (look for where `ActivityEvent` from `glass_core::activity_stream` is handled, or where error/warning severity records are counted), add:

```rust
// After SOI analysis completes:
self.emit_observe_event(
    "output_parsed",
    &format!("agent-mode analyzed {} output — {} errors", command_text, error_count),
);

// For each error detected:
self.emit_observe_event(
    "error_noticed",
    &format!("agent-mode noticed {}", error_summary),
);
```

- [ ] **Step 4: Add observe.thinking and observe.proposing**

At the point where Agent Mode decides to create a proposal (look for where `agent_proposal_worktrees` is populated or where the agent runtime sends a proposal request), add:

```rust
// When agent mode starts considering a fix:
self.emit_observe_event(
    "thinking",
    &format!("agent-mode considering fix for {}", error_summary),
);

// When agent mode creates a proposal:
self.emit_observe_event(
    "proposing",
    &format!("agent-mode proposing {}", proposal_description),
);
```

- [ ] **Step 5: Add observe.dismissed**

At the point where Agent Mode evaluates a command but decides no action is needed (e.g., all tests passed, clean build), add:

```rust
self.emit_observe_event(
    "dismissed",
    &format!("agent-mode dismissed {} (no action needed)", command_text),
);
```

- [ ] **Step 6: Run build and clippy**

Run: `cargo build && cargo clippy --workspace -- -D warnings`
Expected: Compiles and passes clippy

- [ ] **Step 7: Commit**

```bash
git add src/main.rs
git commit -m "feat: emit observe.* events from Agent Mode observation pipeline"
```

---

### Task 10: Command context events from OSC 133

**Files:**
- Modify: `src/main.rs` — the OSC 133 shell event handler

OSC 133 events are processed in `src/main.rs` where `ShellEvent` variants are handled. Look for `ShellEvent::CommandStarted` (OSC 133 C) and `ShellEvent::CommandFinished` (OSC 133 D). The `emit_observe_event` helper from Task 9 can be reused with a different category by adding a more general helper or calling `insert_event` directly.

- [ ] **Step 1: Add emit_command_event helper**

Add to `Processor` in `src/main.rs`:

```rust
    /// Emit a command context event to the coordination event log.
    fn emit_command_event(&self, event_type: &str, summary: &str) {
        // Use agent runtime's project root, or the session's CWD-derived project
        let project = match self.agent_runtime.as_ref().and_then(|r| r.project_root.as_ref()) {
            Some(p) => p.clone(),
            None => return, // No project context, skip
        };
        if let Ok(db) = glass_coordination::CoordinationDb::open_default() {
            let _ = glass_coordination::event_log::insert_event(
                db.conn(),
                &project,
                "command",
                None,
                None,
                event_type,
                summary,
                None,
                false,
            );
        }
    }
```

- [ ] **Step 2: Add command.started event on OSC 133 C**

In the `ShellEvent::CommandStarted` handler in `src/main.rs` (search for `CommandStarted` or `CommandExecuted`), add:

```rust
self.emit_command_event(
    "started",
    &format!("command started {}", command_text),
);
```

- [ ] **Step 3: Add command.finished event on OSC 133 D**

In the `ShellEvent::CommandFinished` handler, add:

```rust
self.emit_command_event(
    "finished",
    &format!(
        "command finished {} (exit: {}, {:.1}s)",
        command_text, exit_code, duration_secs
    ),
);
```

The `command_text`, `exit_code`, and `duration_secs` variables should already be available in the handler context from the block manager's command tracking.

- [ ] **Step 4: Run full test suite**

Run: `cargo test --workspace`
Expected: All tests pass

- [ ] **Step 5: Run clippy and fmt**

Run: `cargo clippy --workspace -- -D warnings && cargo fmt --all -- --check`
Expected: All pass

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat: emit command.* events from OSC 133 boundaries"
```

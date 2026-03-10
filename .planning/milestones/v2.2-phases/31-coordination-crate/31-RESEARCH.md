# Phase 31: Coordination Crate - Research

**Researched:** 2026-03-09
**Domain:** SQLite coordination database, advisory file locking, inter-process communication via shared DB
**Confidence:** HIGH

## Summary

Phase 31 creates a new `glass_coordination` crate that is a pure synchronous library with zero `glass_*` dependencies. It provides agent registration, advisory file locking, inter-agent messaging, and stale agent pruning -- all backed by a shared SQLite database at `~/.glass/agents.db`. The crate follows established project conventions closely: rusqlite 0.38 with WAL mode, `PRAGMA user_version` migrations, `anyhow::Result` error handling, and in-file `#[cfg(test)]` tests using `tempfile`.

The primary technical challenges are: (1) atomic lock acquisition using `BEGIN IMMEDIATE` transactions to prevent TOCTOU races, (2) cross-platform path canonicalization with `dunce` and NTFS case-insensitive normalization, and (3) PID liveness checking without heavy dependencies. All three have clean solutions using existing workspace dependencies plus two small new ones (`uuid`, `dunce`).

**Primary recommendation:** Model the crate directly on `glass_history` and `glass_snapshot` patterns. Use `Connection` ownership (not `&self` shared reference) so `transaction_with_behavior(Immediate)` works naturally. Keep PID liveness checking inline with `#[cfg]`-gated platform code using `libc` on Unix and `windows-sys` on Windows.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| COORD-01 | Agent registers with name/type/project/CWD/PID, receives UUID | UUID v4 generation via `uuid` crate; `agents` table schema from design doc; `CoordinationDb::register()` |
| COORD-02 | Agent deregisters, releasing all locks, preserving messages | `ON DELETE CASCADE` on `file_locks`, `ON DELETE SET NULL` on `messages.from_agent`; `CoordinationDb::deregister()` |
| COORD-03 | Agent heartbeat with 60s interval, 10min timeout | `heartbeat()` updates `last_heartbeat`; stale threshold = 600 seconds |
| COORD-04 | Stale agents auto-pruned via heartbeat timeout or dead PID | `prune_stale()` checks heartbeat age AND PID liveness; platform-specific PID check via libc/windows-sys |
| COORD-05 | Atomic file lock acquisition (all-or-nothing, conflict detection) | `BEGIN IMMEDIATE` transaction; query conflicts first, insert all or return conflicts |
| COORD-06 | Path canonicalization with dunce on Windows, lowercase on NTFS | `dunce::canonicalize()` + conditional `.to_lowercase()` on Windows for NTFS case-insensitivity |
| COORD-07 | Unlock specific files or release all locks | `unlock_file()` and `unlock_all()` methods; simple DELETE statements |
| COORD-08 | Broadcast typed message to all project agents | `broadcast()` inserts message with `to_agent = NULL`; `read_messages` query handles both broadcast and directed |
| COORD-09 | Directed message to specific agent | `send_message()` with explicit `to_agent` field |
| COORD-10 | Read unread messages (marks as read, preserves from deregistered senders) | `read_messages()` uses UPDATE+SELECT in transaction; `from_agent` is SET NULL on sender deregister |
| COORD-11 | Agents scoped by project root | `project` column in `agents` table; `list_agents`, `list_locks`, `broadcast` all filter by project |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| rusqlite | 0.38.0 (workspace) | SQLite database access | Already in workspace, used by glass_history and glass_snapshot |
| uuid | 1.22.0 | Agent ID generation (v4 random) | De facto standard for UUID in Rust, 690M+ downloads |
| dunce | 1.0.5 | Windows path canonicalization without UNC prefix | 93M+ downloads, no-op on non-Windows, solves \\?\ prefix issue |
| anyhow | 1.0.102 (workspace) | Error handling | Already in workspace, project convention |
| dirs | 6 (workspace) | Home directory resolution (~/.glass/) | Already in workspace |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| serde | 1.0.228 (workspace) | Derive Serialize/Deserialize on data types | Return types need serialization for MCP layer (Phase 32) |
| tracing | 0.1.44 (workspace) | Debug/info logging | Logging prune events, lock conflicts |

### Platform Dependencies (no new crates needed)
| Platform | Mechanism | Purpose |
|----------|-----------|---------|
| Unix | libc (implicit via std) | `kill(pid, 0)` for PID liveness check |
| Windows | windows-sys 0.59 (workspace) | `OpenProcess` for PID liveness check |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| uuid | nanoid | UUID is standard, nanoid is shorter but non-standard |
| dunce | std::fs::canonicalize | std version produces UNC paths on Windows (\\?\C:\...) which break string comparison |
| process_alive crate | Manual #[cfg] platform code | process_alive is cleaner but adds a dependency; manual code is ~20 lines total and avoids dep |
| sysinfo crate | Manual #[cfg] platform code | sysinfo is 3MB+ and parses all processes; we only need single PID check |

**Installation (new deps only):**
```bash
# In crates/glass_coordination/Cargo.toml
uuid = { version = "1", features = ["v4"] }
dunce = "1.0"
```

## Architecture Patterns

### Recommended Crate Structure
```
crates/glass_coordination/
  Cargo.toml
  src/
    lib.rs          # Public API re-exports, CoordinationDb struct, resolve_db_path()
    db.rs           # Schema creation, migrations, all SQL operations
    types.rs        # AgentInfo, FileLock, Message, LockResult structs
    pid.rs          # Platform-specific PID liveness checking (#[cfg] gated)
```

### Pattern 1: Open-Per-Call Connection Ownership
**What:** Each `CoordinationDb` instance owns a `Connection` (not `Arc<Connection>`). Callers open a new `CoordinationDb` for each logical operation, then drop it.
**When to use:** Always -- this is the design decision from STATE.md ("CoordinationDb is synchronous library, thread safety via open-per-call").
**Why:** SQLite WAL mode handles concurrent readers/writers. Opening per-call avoids thread-safety complexity. Each MCP tool call will open, operate, close.
**Example:**
```rust
// Source: Project design decision (STATE.md)
pub struct CoordinationDb {
    conn: Connection,
}

impl CoordinationDb {
    pub fn open() -> Result<Self> {
        let path = resolve_db_path();
        // ... open connection, set pragmas, ensure schema
        Ok(Self { conn })
    }
}
```

### Pattern 2: BEGIN IMMEDIATE for Write Transactions
**What:** All write operations use `BEGIN IMMEDIATE` to acquire a write lock immediately, preventing SQLITE_BUSY errors from lock escalation.
**When to use:** Every method that writes to the database (`register`, `deregister`, `lock_files`, `broadcast`, etc.).
**Why:** Design decision from STATE.md ("BEGIN IMMEDIATE for all write transactions prevents SQLITE_BUSY"). With WAL mode and multiple processes, DEFERRED transactions can fail when upgrading from read to write lock.
**Example:**
```rust
// Source: rusqlite 0.38 docs + project decision
use rusqlite::TransactionBehavior;

pub fn register(&mut self, name: &str, agent_type: &str, project: &str, cwd: &str, pid: Option<u32>) -> Result<String> {
    let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let id = uuid::Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO agents (id, name, agent_type, project, cwd, pid, registered_at, last_heartbeat)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, unixepoch(), unixepoch())",
        params![id, name, agent_type, project, cwd, pid.map(|p| p as i64)],
    )?;
    tx.commit()?;
    Ok(id)
}
```

**Important:** Because `transaction_with_behavior` requires `&mut self`, `CoordinationDb` methods that write must take `&mut self`. This is fine with the open-per-call pattern since each caller owns its instance.

### Pattern 3: Atomic Lock Acquisition (All-or-Nothing)
**What:** `lock_files` checks all requested paths for conflicts within a single IMMEDIATE transaction. If any conflict exists, none are locked.
**When to use:** COORD-05 requirement.
**Example:**
```rust
pub fn lock_files(&mut self, agent_id: &str, paths: &[PathBuf], reason: Option<&str>) -> Result<LockResult> {
    let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

    let canonical_paths: Vec<String> = paths.iter()
        .map(|p| canonicalize_path(p))
        .collect::<Result<Vec<_>>>()?;

    // Check for conflicts
    let mut conflicts = Vec::new();
    for canon_path in &canonical_paths {
        let mut stmt = tx.prepare_cached(
            "SELECT fl.path, a.name, fl.reason
             FROM file_locks fl JOIN agents a ON fl.agent_id = a.id
             WHERE fl.path = ?1 AND fl.agent_id != ?2"
        )?;
        // ... collect conflicts
    }

    if !conflicts.is_empty() {
        return Ok(LockResult::Conflict(conflicts));
    }

    // All clear -- insert locks
    for canon_path in &canonical_paths {
        tx.execute(
            "INSERT OR REPLACE INTO file_locks (path, agent_id, reason, locked_at)
             VALUES (?1, ?2, ?3, unixepoch())",
            params![canon_path, agent_id, reason],
        )?;
    }
    tx.commit()?;
    Ok(LockResult::Acquired(canonical_paths))
}
```

### Pattern 4: Path Canonicalization
**What:** All file paths are canonicalized before storage using `dunce::canonicalize()`, with NTFS case normalization on Windows.
**When to use:** In `lock_files`, `unlock_file`, and any path comparison.
**Example:**
```rust
fn canonicalize_path(path: &Path) -> Result<String> {
    let canonical = dunce::canonicalize(path)
        .with_context(|| format!("Cannot canonicalize path: {}", path.display()))?;
    let path_str = canonical.to_string_lossy().to_string();

    // NTFS is case-insensitive; normalize to lowercase on Windows
    #[cfg(target_os = "windows")]
    let path_str = path_str.to_lowercase();

    Ok(path_str)
}
```

### Pattern 5: SQLite Schema with Pragma-Based Migration
**What:** Schema created with `CREATE TABLE IF NOT EXISTS`, versioned with `PRAGMA user_version`.
**When to use:** Standard pattern used by glass_history and glass_snapshot.
**Example:**
```rust
fn create_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agents (
            id             TEXT PRIMARY KEY,
            name           TEXT NOT NULL,
            agent_type     TEXT NOT NULL,
            project        TEXT NOT NULL,
            cwd            TEXT NOT NULL,
            pid            INTEGER,
            status         TEXT NOT NULL DEFAULT 'active',
            task           TEXT,
            registered_at  INTEGER NOT NULL,
            last_heartbeat INTEGER NOT NULL
        );
        -- ... other tables and indices
    ")?;
    Ok(())
}

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < 1 {
        conn.pragma_update(None, "user_version", 1)?;
    }
    Ok(())
}
```

### Anti-Patterns to Avoid
- **Shared Connection across threads:** Do NOT use `Arc<Mutex<Connection>>`. The design explicitly uses open-per-call for thread safety.
- **DEFERRED transactions for writes:** Always use IMMEDIATE. DEFERRED transactions from multiple processes can deadlock when both try to upgrade to write locks.
- **Checking locks before acquiring:** Do NOT implement check-then-lock (TOCTOU race). Use the atomic pattern where conflict detection and acquisition happen in the same transaction.
- **String path comparison without canonicalization:** Paths like `src/main.rs`, `./src/main.rs`, and `C:\Users\...\src\main.rs` must all resolve to the same canonical form.
- **Using `unchecked_transaction()` for IMMEDIATE:** rusqlite 0.38 does NOT have `unchecked_transaction_with_behavior()`. Use `transaction_with_behavior(Immediate)` with `&mut self`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| UUID generation | Custom random ID string | `uuid::Uuid::new_v4()` | Collision resistance, RFC 4122 compliance, 128-bit entropy |
| Path canonicalization on Windows | `std::fs::canonicalize()` | `dunce::canonicalize()` | std produces UNC paths (\\?\C:\) that break string comparison and confuse tools |
| SQLite concurrent access | Custom file locking / IPC | SQLite WAL mode + `busy_timeout` | Battle-tested concurrent reader/writer support built into SQLite |
| Schema versioning | Custom version table | `PRAGMA user_version` | Built into SQLite, project convention from glass_history |
| Cross-platform home directory | `std::env::var("HOME")` | `dirs::home_dir()` | Handles Windows `USERPROFILE`, XDG on Linux, macOS correctly |

**Key insight:** This crate is essentially a thin Rust API over a well-designed SQLite schema. The complexity is in getting transactions, path normalization, and cascading deletes right -- all of which SQLite handles natively.

## Common Pitfalls

### Pitfall 1: SQLITE_BUSY with DEFERRED Transactions
**What goes wrong:** Two processes both start DEFERRED transactions, read, then try to write. SQLite cannot upgrade both to write locks -- one gets SQLITE_BUSY.
**Why it happens:** DEFERRED is the default. Multiple MCP processes hit the same DB simultaneously.
**How to avoid:** Use `BEGIN IMMEDIATE` for all write transactions. Combined with `PRAGMA busy_timeout = 5000`, the second writer will wait up to 5 seconds for the first to finish.
**Warning signs:** Intermittent "database is locked" errors under concurrent agent usage.

### Pitfall 2: UNC Paths on Windows Breaking Lock Comparison
**What goes wrong:** `std::fs::canonicalize()` returns `\\?\C:\Users\...` on Windows. Two agents canonicalizing the same file might get different UNC vs non-UNC representations.
**Why it happens:** Windows UNC extended-length path prefix is inconsistently applied.
**How to avoid:** Use `dunce::canonicalize()` which strips the UNC prefix when safe. Always use the canonical form for storage and comparison.
**Warning signs:** Same file locked under two different path representations without conflict detection.

### Pitfall 3: NTFS Case-Insensitive Path Conflicts
**What goes wrong:** One agent locks `src/Main.rs` and another locks `src/main.rs`. These are the same file on NTFS but have different string representations.
**Why it happens:** NTFS is case-insensitive but case-preserving. `canonicalize()` preserves the case of the path as stored on disk.
**How to avoid:** On Windows, lowercase all path strings after canonicalization before storing in the DB.
**Warning signs:** Lock conflicts not detected for paths that differ only in case.

### Pitfall 4: PID Reuse False Positives
**What goes wrong:** An agent's original process dies, its PID is reused by an unrelated process. The stale check sees the PID is "alive" and doesn't prune.
**Why it happens:** Operating systems recycle PIDs. Windows PIDs cycle relatively quickly.
**How to avoid:** PID check is a supplement to heartbeat timeout, not a replacement. If heartbeat has timed out (10 minutes), prune regardless of PID status. PID check is only for early detection (process died but heartbeat hasn't timed out yet).
**Warning signs:** Stale agent entries persisting beyond heartbeat timeout.

### Pitfall 5: Foreign Key CASCADE Not Enabled
**What goes wrong:** Deleting an agent doesn't cascade to its file_locks or messages.
**Why it happens:** SQLite has foreign keys OFF by default. Must set `PRAGMA foreign_keys = ON` on every connection.
**How to avoid:** Set the pragma in the `open()` method, just like glass_history and glass_snapshot do.
**Warning signs:** Orphaned file_lock rows after agent deletion.

### Pitfall 6: Messages from Deregistered Agents Lost
**What goes wrong:** When an agent deregisters, its sent messages are deleted instead of preserved.
**Why it happens:** Using `ON DELETE CASCADE` on `messages.from_agent` instead of `ON DELETE SET NULL`.
**How to avoid:** Schema must use `ON DELETE SET NULL` for `messages.from_agent` (COORD-10). This requires `from_agent` to be nullable.
**Warning signs:** Recipients missing messages from agents that deregistered before they read.

## Code Examples

### Database Open with Standard Pragmas
```rust
// Source: glass_history/db.rs pattern + project decisions
pub fn open() -> Result<Self> {
    let path = resolve_db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(&path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;
         PRAGMA foreign_keys = ON;",
    )?;
    Self::create_schema(&conn)?;
    Self::migrate(&conn)?;
    Ok(Self { conn })
}

fn resolve_db_path() -> PathBuf {
    let home = dirs::home_dir().expect("Could not determine home directory");
    let glass_dir = home.join(".glass");
    std::fs::create_dir_all(&glass_dir).ok();
    glass_dir.join("agents.db")
}
```

### PID Liveness Check (Platform-Specific)
```rust
// Source: Standard OS APIs, #[cfg] pattern from project conventions

/// Check if a process with the given PID is still alive.
pub fn is_pid_alive(pid: u32) -> bool {
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
}
```

### Stale Agent Pruning
```rust
// Source: Design doc + COORD-04 requirement
pub fn prune_stale(&mut self, timeout_secs: i64) -> Result<Vec<String>> {
    let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;
    let cutoff = now - timeout_secs;

    // Find agents past heartbeat timeout
    let mut stmt = tx.prepare(
        "SELECT id, pid FROM agents WHERE last_heartbeat < ?1"
    )?;
    let stale_by_timeout: Vec<(String, Option<i64>)> = stmt
        .query_map(params![cutoff], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);

    // Also find agents with dead PIDs (even if heartbeat is recent)
    let mut stmt = tx.prepare("SELECT id, pid FROM agents WHERE pid IS NOT NULL")?;
    let all_with_pid: Vec<(String, i64)> = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut pruned_ids = Vec::new();

    // Prune heartbeat-stale agents
    for (id, _pid) in &stale_by_timeout {
        pruned_ids.push(id.clone());
    }

    // Prune PID-dead agents (only if not already in stale list)
    for (id, pid) in &all_with_pid {
        if !pruned_ids.contains(id) && !is_pid_alive(*pid as u32) {
            pruned_ids.push(id.clone());
        }
    }

    // Delete all stale agents (CASCADE removes locks)
    for id in &pruned_ids {
        tx.execute("DELETE FROM agents WHERE id = ?1", params![id])?;
    }
    tx.commit()?;
    Ok(pruned_ids)
}
```

### Read Messages (Mark as Read)
```rust
// Source: Design doc + COORD-10 requirement
pub fn read_messages(&mut self, agent_id: &str) -> Result<Vec<Message>> {
    let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

    // Select unread messages (broadcast to_agent IS NULL or directed to this agent)
    let mut stmt = tx.prepare(
        "SELECT m.id, m.from_agent, a.name, m.to_agent, m.msg_type, m.content, m.created_at
         FROM messages m
         LEFT JOIN agents a ON m.from_agent = a.id
         WHERE m.read = 0 AND (m.to_agent = ?1 OR m.to_agent IS NULL)
         ORDER BY m.created_at ASC"
    )?;
    let messages: Vec<Message> = stmt
        .query_map(params![agent_id], |row| {
            Ok(Message {
                id: row.get(0)?,
                from_agent: row.get(1)?,
                from_name: row.get(2)?,
                to_agent: row.get(3)?,
                msg_type: row.get(4)?,
                content: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);

    // Mark as read
    let ids: Vec<i64> = messages.iter().map(|m| m.id).collect();
    for id in &ids {
        tx.execute("UPDATE messages SET read = 1 WHERE id = ?1", params![id])?;
    }
    tx.commit()?;
    Ok(messages)
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `std::fs::canonicalize` | `dunce::canonicalize` | dunce 1.0 (2019) | Avoids UNC prefix on Windows paths |
| Custom ID generation | `uuid` crate v4 | uuid 1.0 (2022) | Standard RFC 4122 UUID generation |
| `unchecked_transaction()` + manual BEGIN IMMEDIATE | `transaction_with_behavior(Immediate)` | rusqlite design | Proper API for IMMEDIATE transactions |

**Deprecated/outdated:**
- None relevant. rusqlite 0.38, uuid 1.x, and dunce 1.x are all current stable releases.

## Open Questions

1. **Broadcast message read tracking per-recipient**
   - What we know: The design uses a single `read` flag on messages. A broadcast message (`to_agent = NULL`) marked as read by one agent becomes invisible to all agents.
   - What's unclear: Should each agent have independent read tracking for broadcasts?
   - Recommendation: For V1, use the simple approach but change the query: select broadcasts where `created_at > agent.last_message_read_at` or use a separate `message_reads` join table. The simpler approach from the design doc (single `read` flag) works if `read_messages` only marks directed messages as read, and broadcasts are excluded from marking. **This needs resolution during planning.** A pragmatic fix: broadcast messages get `read` set per-recipient via inserting one message row per recipient at broadcast time, not a single shared row.

2. **Project path canonicalization for scoping**
   - What we know: Agents register with a `project` root path. Lock visibility is scoped by project.
   - What's unclear: Should the `project` path also be canonicalized with `dunce`?
   - Recommendation: Yes -- canonicalize `project` at registration time using the same `canonicalize_path()` function. This ensures agents on the same repo match even if they specify different path representations.

3. **Windows features for OpenProcess**
   - What we know: windows-sys 0.59 is in workspace with feature `Win32_System_Console`.
   - What's unclear: Whether `Win32_System_Threading` and `Win32_Foundation` features are needed (they likely are for `OpenProcess` and `CloseHandle`).
   - Recommendation: Add the needed features to the workspace dependency or use a crate-local windows-sys dependency with just the threading features.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test framework (cargo test) |
| Config file | None needed -- standard `#[cfg(test)] mod tests` pattern |
| Quick run command | `cargo test -p glass_coordination` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| COORD-01 | Agent register returns UUID | unit | `cargo test -p glass_coordination -- test_register` | Wave 0 |
| COORD-02 | Deregister cascades locks, preserves messages | unit | `cargo test -p glass_coordination -- test_deregister` | Wave 0 |
| COORD-03 | Heartbeat updates timestamp | unit | `cargo test -p glass_coordination -- test_heartbeat` | Wave 0 |
| COORD-04 | Stale agents pruned by timeout and dead PID | unit | `cargo test -p glass_coordination -- test_prune_stale` | Wave 0 |
| COORD-05 | Atomic lock all-or-nothing with conflict detection | unit | `cargo test -p glass_coordination -- test_lock_files` | Wave 0 |
| COORD-06 | Path canonicalization (dunce + lowercase) | unit | `cargo test -p glass_coordination -- test_canonicalize` | Wave 0 |
| COORD-07 | Unlock specific files or all | unit | `cargo test -p glass_coordination -- test_unlock` | Wave 0 |
| COORD-08 | Broadcast typed message to project agents | unit | `cargo test -p glass_coordination -- test_broadcast` | Wave 0 |
| COORD-09 | Directed message to specific agent | unit | `cargo test -p glass_coordination -- test_send_message` | Wave 0 |
| COORD-10 | Read unread messages, mark as read, preserve from deregistered | unit | `cargo test -p glass_coordination -- test_read_messages` | Wave 0 |
| COORD-11 | Project scoping isolation | unit | `cargo test -p glass_coordination -- test_project_scoping` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_coordination`
- **Per wave merge:** `cargo test --workspace && cargo clippy --workspace -- -D warnings`
- **Phase gate:** Full suite green before verification

### Wave 0 Gaps
- [ ] `crates/glass_coordination/Cargo.toml` -- crate manifest
- [ ] `crates/glass_coordination/src/lib.rs` -- public API and re-exports
- [ ] `crates/glass_coordination/src/db.rs` -- schema + all SQL operations + tests
- [ ] `crates/glass_coordination/src/types.rs` -- data structures
- [ ] `crates/glass_coordination/src/pid.rs` -- platform PID liveness + tests
- [ ] Workspace Cargo.toml members includes `glass_coordination`

## Sources

### Primary (HIGH confidence)
- glass_history/src/db.rs -- SQLite patterns, WAL pragmas, migration approach, test patterns
- glass_snapshot/src/db.rs -- Same SQLite patterns, confirms conventions
- AGENT_COORDINATION_DESIGN.md -- Full schema, API design, architectural decisions
- REQUIREMENTS.md -- Authoritative requirement definitions (COORD-01 through COORD-11)
- STATE.md -- Locked design decisions (global agents.db, open-per-call, BEGIN IMMEDIATE, dunce canonicalization)

### Secondary (MEDIUM confidence)
- [rusqlite 0.38 docs](https://docs.rs/rusqlite/0.38.0/) -- Transaction API confirmation, no `unchecked_transaction_with_behavior`
- [dunce crate](https://docs.rs/dunce) -- v1.0.5, cross-platform path canonicalization
- [uuid crate](https://docs.rs/uuid) -- v1.22.0, `new_v4()` for random UUID generation
- [process_alive crate](https://lib.rs/crates/process_alive) -- v0.2.0, alternative PID checking (not recommended due to extra dep)

### Tertiary (LOW confidence)
- Windows `OpenProcess` API behavior for PID checking -- based on general Windows API knowledge, verify with testing

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- uses existing workspace deps (rusqlite, anyhow, dirs) plus two small well-known crates (uuid, dunce)
- Architecture: HIGH -- follows exact patterns from glass_history and glass_snapshot, design doc is detailed
- Pitfalls: HIGH -- identified from concrete project experience (existing crates) and SQLite documentation
- PID liveness: MEDIUM -- platform-specific code needs testing on each platform; Unix kill(0) is well-documented, Windows OpenProcess needs feature flags verified

**Research date:** 2026-03-09
**Valid until:** 2026-04-09 (stable domain, dependencies are mature)

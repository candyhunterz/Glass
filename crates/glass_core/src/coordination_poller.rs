//! Background coordination poller: queries the coordination DB on a named
//! thread every 5 seconds, sends `AppEvent::CoordinationUpdate` to the
//! winit event loop.
//!
//! Follows the same spawn-thread pattern as `updater::spawn_update_checker`.

use std::thread;
use std::time::Duration;

use winit::event_loop::EventLoopProxy;

use crate::event::AppEvent;
use glass_coordination::CoordinationDb;

/// Snapshot of multi-agent coordination state, sent to the UI thread.
#[derive(Debug, Clone, Default)]
pub struct CoordinationState {
    /// Number of agents registered for the current project.
    pub agent_count: usize,
    /// Number of file locks held in the current project.
    pub lock_count: usize,
    /// Details of each held lock.
    pub locks: Vec<LockEntry>,
    /// Detected conflicts (reserved for Plan 02 overlay).
    pub conflicts: Vec<ConflictInfo>,
    /// Per-agent display info for the compact status bar.
    pub agents: Vec<AgentDisplayInfo>,
    /// Recent coordination events for the overlay timeline.
    pub recent_events: Vec<glass_coordination::CoordinationEvent>,
    /// Most recent notable event for the compact bar ticker.
    /// Cleared after one display cycle by the main event loop.
    pub ticker_event: Option<glass_coordination::CoordinationEvent>,
}

/// A single file lock entry for display purposes.
#[derive(Debug, Clone)]
pub struct LockEntry {
    pub path: String,
    pub agent_id: String,
    pub agent_name: String,
}

/// Conflict information when multiple agents contend for the same file.
#[derive(Debug, Clone)]
pub struct ConflictInfo {
    pub path: String,
    /// Vec of (agent_id, agent_name) pairs involved.
    pub agents: Vec<(String, String)>,
}

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

/// Spawn a background thread that polls the coordination DB every 5 seconds.
///
/// Sends `AppEvent::CoordinationUpdate(state)` to the event loop proxy.
/// Terminates when the proxy's event loop is dropped (send_event returns Err).
pub fn spawn_coordination_poller(project_root: String, proxy: EventLoopProxy<AppEvent>) {
    thread::Builder::new()
        .name("Glass coordination poller".into())
        .spawn(move || {
            loop {
                // Sleep BEFORE polling so startup isn't delayed by DB I/O.
                thread::sleep(Duration::from_secs(5));

                let state = poll_once(&project_root);

                if proxy
                    .send_event(AppEvent::CoordinationUpdate(state))
                    .is_err()
                {
                    // Event loop closed, exit thread.
                    break;
                }
            }
        })
        .expect("Failed to spawn coordination poller thread");
}

/// Poll the coordination DB once and return the current state.
///
/// Opens a fresh `CoordinationDb` each cycle (open-per-call pattern).
/// Returns `CoordinationState::default()` on any error (including missing DB).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordination_state_default_is_zeros() {
        let state = CoordinationState::default();
        assert_eq!(state.agent_count, 0);
        assert_eq!(state.lock_count, 0);
        assert!(state.locks.is_empty());
        assert!(state.conflicts.is_empty());
        assert!(state.agents.is_empty());
        assert!(state.recent_events.is_empty());
        assert!(state.ticker_event.is_none());
    }

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

    #[test]
    fn test_poll_once_no_db_returns_default() {
        // Use a nonsense project root that won't match any registered agents.
        // The DB may or may not exist at the default path, but either way
        // we should get a valid (possibly zero-count) state, not a panic.
        let state = poll_once("/nonexistent/project/root/abc123xyz");
        // Should return default-like state (0 agents, 0 locks for this project)
        assert_eq!(state.agent_count, 0);
        assert_eq!(state.lock_count, 0);
        assert!(state.locks.is_empty());
        assert!(state.conflicts.is_empty());
    }
}

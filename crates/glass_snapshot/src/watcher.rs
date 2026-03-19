//! Filesystem watcher for monitoring directory changes during command execution.
//!
//! Wraps the `notify` crate to provide a simple interface:
//! create a `FsWatcher`, let it collect events, then `drain_events()`
//! to get a deduplicated, filtered list of file changes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use anyhow::Result;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::ignore_rules::IgnoreRules;
use crate::types::WatcherEvent;

/// Watches a directory tree for filesystem changes.
pub struct FsWatcher {
    /// Keep the watcher alive -- dropping it stops monitoring.
    _watcher: RecommendedWatcher,
    /// Channel receiving raw notify events.
    rx: mpsc::Receiver<Result<Event, notify::Error>>,
    /// Ignore rules for filtering paths.
    ignore: IgnoreRules,
}

impl FsWatcher {
    /// Start watching `cwd` recursively, filtering events through `ignore` rules.
    pub fn new(cwd: &Path, ignore: IgnoreRules) -> Result<Self> {
        // Canonicalize to resolve symlinks (e.g. macOS /var -> /private/var)
        // so event paths match the ignore rules root.
        let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
        let (tx, rx) = mpsc::channel();

        let mut watcher = notify::recommended_watcher(move |res| {
            if tx.send(res).is_err() {
                // Channel disconnected — receiver dropped (FsWatcher was dropped)
            }
        })?;

        watcher.watch(&cwd, RecursiveMode::Recursive)?;

        Ok(Self {
            _watcher: watcher,
            rx,
            ignore,
        })
    }

    /// Drain all pending events, filtering ignored paths and deduplicating.
    ///
    /// Returns one event per path (the last event wins for deduplication).
    pub fn drain_events(&self) -> Vec<WatcherEvent> {
        let mut seen: HashMap<PathBuf, WatcherEvent> = HashMap::new();

        while let Ok(result) = self.rx.try_recv() {
            let event = match result {
                Ok(e) => e,
                Err(_) => continue,
            };

            for path in &event.paths {
                // Skip ignored paths
                if self.ignore.is_ignored(path) {
                    continue;
                }

                if let Some(watcher_event) = WatcherEvent::from_notify(&event, path) {
                    // Keep last event per path (deduplication)
                    seen.insert(watcher_event.path.clone(), watcher_event);
                }
            }
        }

        seen.into_values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::WatcherEventKind;
    use std::time::Duration;
    use tempfile::TempDir;

    /// Small delay to let the OS deliver FS events to notify.
    fn wait_for_events() {
        std::thread::sleep(Duration::from_millis(200));
    }

    // -- WatcherEvent::from_notify unit tests --

    /// Helper to create a notify event with paths.
    fn make_event(kind: notify::EventKind, paths: Vec<PathBuf>) -> Event {
        let mut event = Event::new(kind);
        event.paths = paths;
        event
    }

    #[test]
    fn test_from_notify_create() {
        let path = PathBuf::from("/tmp/a.txt");
        let event = make_event(
            notify::EventKind::Create(notify::event::CreateKind::File),
            vec![path.clone()],
        );
        let we = WatcherEvent::from_notify(&event, &path).unwrap();
        assert_eq!(we.kind, WatcherEventKind::Create);
    }

    #[test]
    fn test_from_notify_modify() {
        let path = PathBuf::from("/tmp/a.txt");
        let event = make_event(
            notify::EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Any,
            )),
            vec![path.clone()],
        );
        let we = WatcherEvent::from_notify(&event, &path).unwrap();
        assert_eq!(we.kind, WatcherEventKind::Modify);
    }

    #[test]
    fn test_from_notify_delete() {
        let path = PathBuf::from("/tmp/a.txt");
        let event = make_event(
            notify::EventKind::Remove(notify::event::RemoveKind::File),
            vec![path.clone()],
        );
        let we = WatcherEvent::from_notify(&event, &path).unwrap();
        assert_eq!(we.kind, WatcherEventKind::Delete);
    }

    #[test]
    fn test_from_notify_access_returns_none() {
        let path = PathBuf::from("/tmp/a.txt");
        let event = make_event(
            notify::EventKind::Access(notify::event::AccessKind::Read),
            vec![path.clone()],
        );
        assert!(WatcherEvent::from_notify(&event, &path).is_none());
    }

    // -- FsWatcher integration tests --

    #[test]
    fn test_watcher_new_watches_directory() {
        let dir = TempDir::new().unwrap();
        let rules = IgnoreRules::load(dir.path());
        let watcher = FsWatcher::new(dir.path(), rules);
        assert!(watcher.is_ok());
    }

    #[test]
    fn test_watcher_detects_create() {
        let dir = TempDir::new().unwrap();
        let rules = IgnoreRules::load(dir.path());
        let watcher = FsWatcher::new(dir.path(), rules).unwrap();

        // Create a file
        std::fs::write(dir.path().join("new_file.txt"), "hello").unwrap();
        wait_for_events();

        let events = watcher.drain_events();
        assert!(
            !events.is_empty(),
            "Expected at least one event for file creation"
        );

        let paths: Vec<_> = events.iter().map(|e| e.path.clone()).collect();
        assert!(
            paths.iter().any(|p| p.ends_with("new_file.txt")),
            "Expected event for new_file.txt, got: {:?}",
            paths
        );
    }

    #[test]
    fn test_watcher_detects_modify() {
        let dir = TempDir::new().unwrap();
        // Create file before starting watcher
        let file_path = dir.path().join("existing.txt");
        std::fs::write(&file_path, "initial").unwrap();

        let rules = IgnoreRules::load(dir.path());
        let watcher = FsWatcher::new(dir.path(), rules).unwrap();
        wait_for_events();
        // Drain any initial events
        let _ = watcher.drain_events();

        // Modify the file
        std::fs::write(&file_path, "modified content").unwrap();
        wait_for_events();

        let events = watcher.drain_events();
        assert!(
            !events.is_empty(),
            "Expected at least one event for file modification"
        );

        let paths: Vec<_> = events.iter().map(|e| e.path.clone()).collect();
        assert!(
            paths.iter().any(|p| p.ends_with("existing.txt")),
            "Expected event for existing.txt, got: {:?}",
            paths
        );
    }

    #[test]
    fn test_watcher_detects_delete() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("to_delete.txt");
        std::fs::write(&file_path, "delete me").unwrap();

        let rules = IgnoreRules::load(dir.path());
        let watcher = FsWatcher::new(dir.path(), rules).unwrap();
        wait_for_events();
        let _ = watcher.drain_events();

        // Delete the file
        std::fs::remove_file(&file_path).unwrap();
        wait_for_events();

        let events = watcher.drain_events();
        assert!(
            !events.is_empty(),
            "Expected at least one event for file deletion"
        );

        let paths: Vec<_> = events.iter().map(|e| e.path.clone()).collect();
        assert!(
            paths.iter().any(|p| p.ends_with("to_delete.txt")),
            "Expected event for to_delete.txt, got: {:?}",
            paths
        );
    }

    #[test]
    fn test_watcher_filters_ignored_paths() {
        let dir = TempDir::new().unwrap();
        let nm_dir = dir.path().join("node_modules");
        std::fs::create_dir_all(&nm_dir).unwrap();

        let rules = IgnoreRules::load(dir.path());
        let watcher = FsWatcher::new(dir.path(), rules).unwrap();
        wait_for_events();
        let _ = watcher.drain_events();

        // Create file inside node_modules (should be ignored)
        std::fs::write(nm_dir.join("package.json"), "{}").unwrap();
        // Also create a normal file (should be captured)
        std::fs::write(dir.path().join("visible.txt"), "hi").unwrap();
        wait_for_events();

        let events = watcher.drain_events();
        let paths: Vec<_> = events.iter().map(|e| e.path.clone()).collect();

        // No events for node_modules paths
        assert!(
            !paths
                .iter()
                .any(|p| p.to_string_lossy().contains("node_modules")),
            "node_modules events should be filtered, got: {:?}",
            paths
        );
    }

    #[test]
    fn test_watcher_deduplicates_events() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("dedup.txt");
        std::fs::write(&file_path, "v1").unwrap();

        let rules = IgnoreRules::load(dir.path());
        let watcher = FsWatcher::new(dir.path(), rules).unwrap();
        wait_for_events();
        let _ = watcher.drain_events();

        // Multiple writes to the same file
        std::fs::write(&file_path, "v2").unwrap();
        std::fs::write(&file_path, "v3").unwrap();
        std::fs::write(&file_path, "v4").unwrap();
        wait_for_events();

        let events = watcher.drain_events();
        // Count events for our specific file
        let dedup_events: Vec<_> = events
            .iter()
            .filter(|e| e.path.ends_with("dedup.txt"))
            .collect();

        assert!(
            dedup_events.len() <= 1,
            "Expected at most 1 deduplicated event for dedup.txt, got: {}",
            dedup_events.len()
        );
    }
}

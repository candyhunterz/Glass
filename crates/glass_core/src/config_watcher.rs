//! Config file watcher: monitors ~/.glass/config.toml for changes and sends
//! `AppEvent::ConfigReloaded` events via the winit event loop proxy.
//!
//! Watches the PARENT DIRECTORY (not the file itself) to survive atomic saves
//! (vim/VSCode write-tmp-then-rename pattern).

use std::path::PathBuf;

use notify::{recommended_watcher, Event, RecursiveMode, Watcher};
use winit::event_loop::EventLoopProxy;

use crate::config::GlassConfig;
use crate::event::AppEvent;

/// Spawn a background thread that watches for config file changes.
///
/// Watches the parent directory of `config_path` with non-recursive mode
/// to handle atomic saves. Filters events to only `config.toml` changes.
/// On change: reads file, validates with `load_validated()`, and sends
/// `AppEvent::ConfigReloaded` via the proxy.
pub fn spawn_config_watcher(config_path: PathBuf, proxy: EventLoopProxy<AppEvent>) {
    let watch_dir = config_path
        .parent()
        .expect("config_path must have a parent directory")
        .to_path_buf();

    std::thread::Builder::new()
        .name("Glass config watcher".into())
        .spawn(move || {
            let proxy_clone = proxy.clone();
            let config_path_clone = config_path.clone();

            let mut watcher = match recommended_watcher(
                move |res: Result<Event, notify::Error>| {
                    let event = match res {
                        Ok(ev) => ev,
                        Err(e) => {
                            tracing::warn!("Config watcher error: {}", e);
                            return;
                        }
                    };

                    // Filter: only react to events involving config.toml
                    let is_config_event = event.paths.iter().any(|p| {
                        p.file_name()
                            .map(|n| n == "config.toml")
                            .unwrap_or(false)
                    });
                    if !is_config_event {
                        return;
                    }

                    // Read the file; silently skip on read error (mid-write)
                    let contents = match std::fs::read_to_string(&config_path_clone) {
                        Ok(c) => c,
                        Err(_) => return, // File may be mid-write; wait for next event
                    };

                    // Validate and send event
                    match GlassConfig::load_validated(&contents) {
                        Ok(config) => {
                            let _ = proxy_clone.send_event(AppEvent::ConfigReloaded {
                                config: Box::new(config),
                                error: None,
                            });
                        }
                        Err(err) => {
                            let _ = proxy_clone.send_event(AppEvent::ConfigReloaded {
                                config: Box::new(GlassConfig::default()),
                                error: Some(err),
                            });
                        }
                    }
                },
            ) {
                Ok(w) => w,
                Err(e) => {
                    tracing::error!("Failed to create config watcher: {}", e);
                    return;
                }
            };

            // Watch the parent directory (not the file) for atomic save support
            if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
                tracing::error!("Failed to watch config directory {}: {}", watch_dir.display(), e);
                return;
            }

            tracing::info!("Config watcher started for {}", config_path.display());

            // Keep the watcher alive by blocking this thread indefinitely.
            // The watcher is dropped when the process exits.
            loop {
                std::thread::park();
            }
        })
        .expect("Failed to spawn config watcher thread");
}

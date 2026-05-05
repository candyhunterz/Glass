//! Codex CLI backend — `AgentBackend` impl that spawns `codex exec --json`
//! and translates its JSON event stream into [`AgentEvent`]s.
//!
//! Auth is handled by Codex itself via `codex login`; Glass only checks
//! token-file existence for a friendly pre-flight error.

use crate::{AgentBackend, AgentHandle, BackendError, BackendSpawnConfig, ShutdownToken};

pub mod auth;
pub mod parse;

/// Codex CLI backend. Construct cheaply; binary and login checks run at `spawn` time.
pub struct CodexCliBackend;

impl CodexCliBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CodexCliBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentBackend for CodexCliBackend {
    fn name(&self) -> &str {
        "Codex CLI"
    }

    fn spawn(
        &self,
        _config: &BackendSpawnConfig,
        _generation: u64,
    ) -> Result<AgentHandle, BackendError> {
        // Pre-flight: check that the user has run `codex login`.
        if !auth::is_logged_in() {
            return Err(BackendError::LoginRequired {
                provider: "codex-cli".into(),
                command_hint: "codex login".into(),
            });
        }
        // Process spawn + reader/writer threads land in Plan-Task 6 (deferred).
        Err(BackendError::SpawnFailed(
            "CodexCliBackend::spawn not yet implemented".into(),
        ))
    }

    fn shutdown(&self, _token: ShutdownToken) {
        // Implemented in Plan-Task 7 (deferred).
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentMode;

    fn dummy_config() -> BackendSpawnConfig {
        BackendSpawnConfig {
            system_prompt: String::new(),
            initial_message: None,
            project_root: ".".into(),
            mcp_config_path: String::new(),
            allowed_tools: vec![],
            mode: AgentMode::Off,
            cooldown_secs: 0,
            restart_count: 0,
            last_crash: None,
        }
    }

    #[test]
    fn spawn_returns_login_required_when_no_token() {
        let tmp = std::env::temp_dir().join("glass-codex-spawn-no-token");
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::set_var("CODEX_HOME", &tmp);
        let backend = CodexCliBackend::new();
        let result = backend.spawn(&dummy_config(), 0);
        std::env::remove_var("CODEX_HOME");

        match result {
            Err(BackendError::LoginRequired {
                provider,
                command_hint,
            }) => {
                assert_eq!(provider, "codex-cli");
                assert_eq!(command_hint, "codex login");
            }
            other => panic!("expected LoginRequired, got {other:?}"),
        }
    }
}

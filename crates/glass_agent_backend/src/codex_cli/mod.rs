//! Codex CLI backend — `AgentBackend` impl that spawns `codex exec --json`
//! and translates its JSON event stream into [`AgentEvent`]s.
//!
//! Auth is handled by Codex itself via `codex login`; Glass only checks
//! token-file existence for a friendly pre-flight error.

pub mod auth;

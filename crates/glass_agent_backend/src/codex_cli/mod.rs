//! Codex CLI backend — `AgentBackend` impl that spawns `codex exec --json`
//! and translates its JSON event stream into [`AgentEvent`]s.
//!
//! Auth is handled by Codex itself via `codex login`; Glass only checks
//! token-file existence for a friendly pre-flight error.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::{
    AgentBackend, AgentEvent, AgentHandle, BackendError, BackendSpawnConfig, ShutdownToken,
};

pub mod auth;
pub mod parse;

struct CodexShutdownState {
    child: Arc<Mutex<Option<std::process::Child>>>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    stop: Arc<AtomicBool>,
}

/// Codex CLI backend. Construct cheaply; binary and login checks run at `spawn` time.
pub struct CodexCliBackend {
    model: String,
}

impl CodexCliBackend {
    pub fn new() -> Self {
        Self::with_model("")
    }

    pub fn with_model(model: &str) -> Self {
        Self {
            model: model.to_string(),
        }
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
        config: &BackendSpawnConfig,
        generation: u64,
    ) -> Result<AgentHandle, BackendError> {
        // Pre-flight: check that the user has run `codex login`.
        if !auth::is_logged_in() {
            return Err(BackendError::LoginRequired {
                provider: "codex-cli".into(),
                command_hint: "codex login".into(),
            });
        }

        let initial_prompt = format_initial_prompt(&config.system_prompt, &config.initial_message);
        let mcp_command = std::env::current_exe().ok();

        if !config.mcp_config_path.is_empty() {
            tracing::warn!(
                "CodexCliBackend: ignoring mcp_config_path; codex-cli 0.128.0 has no --mcp-config flag"
            );
        }

        let (event_tx, event_rx) = mpsc::channel::<AgentEvent>();
        let (message_tx, message_rx) = mpsc::channel::<String>();
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(50)));
        let child = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));

        let model = self.model.clone();
        let project_root = config.project_root.clone();
        let mcp_command = mcp_command.clone();
        let event_tx_worker = event_tx;
        let stderr_tail_worker = Arc::clone(&stderr_tail);
        let child_worker = Arc::clone(&child);
        let stop_worker = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("glass-codex-worker".into())
            .spawn(move || {
                let _ = run_codex_turn(CodexTurn {
                    model: &model,
                    project_root: &project_root,
                    prompt: &initial_prompt,
                    mcp_command: mcp_command.as_deref(),
                    event_tx: &event_tx_worker,
                    stderr_tail: &stderr_tail_worker,
                    child_slot: &child_worker,
                    stop: &stop_worker,
                });

                for content in message_rx.iter() {
                    if stop_worker.load(Ordering::Relaxed) {
                        break;
                    }
                    let content = extract_user_content(&content);
                    if !run_codex_turn(CodexTurn {
                        model: &model,
                        project_root: &project_root,
                        prompt: &content,
                        mcp_command: mcp_command.as_deref(),
                        event_tx: &event_tx_worker,
                        stderr_tail: &stderr_tail_worker,
                        child_slot: &child_worker,
                        stop: &stop_worker,
                    }) {
                        break;
                    }
                }
            })
            .map_err(|e| BackendError::SpawnFailed(format!("failed to spawn codex worker: {e}")))?;

        tracing::info!(
            "CodexCliBackend: codex subprocess spawned (model={}, generation={}, restart_count={})",
            if self.model.is_empty() {
                "<default>"
            } else {
                &self.model
            },
            generation,
            config.restart_count
        );

        Ok(AgentHandle {
            message_tx,
            event_rx,
            generation,
            shutdown_token: ShutdownToken::new(CodexShutdownState {
                child,
                stderr_tail,
                stop,
            }),
        })
    }

    fn shutdown(&self, mut token: ShutdownToken) {
        let Some(state) = token.downcast_mut::<CodexShutdownState>() else {
            tracing::warn!("CodexCliBackend::shutdown: token type mismatch");
            return;
        };

        state.stop.store(true, Ordering::Relaxed);

        if let Some(ref mut child) = *state.child.lock() {
            match child.try_wait() {
                Ok(Some(_status)) => {}
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
        *state.child.lock() = None;

        let tail = stderr_tail_to_string(&state.stderr_tail);
        if !tail.is_empty() {
            tracing::debug!("CodexCliBackend::shutdown stderr tail:\n{tail}");
        }
    }
}

struct CodexTurn<'a> {
    model: &'a str,
    project_root: &'a str,
    prompt: &'a str,
    mcp_command: Option<&'a std::path::Path>,
    event_tx: &'a mpsc::Sender<AgentEvent>,
    stderr_tail: &'a Arc<Mutex<VecDeque<String>>>,
    child_slot: &'a Arc<Mutex<Option<std::process::Child>>>,
    stop: &'a Arc<AtomicBool>,
}

fn run_codex_turn(turn: CodexTurn<'_>) -> bool {
    let args = build_codex_args(turn.model, turn.project_root, turn.prompt, turn.mcp_command);
    let mut cmd = codex_command(&args);
    cmd.current_dir(turn.project_root);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_child_options(&mut cmd);

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            tracing::warn!("CodexCliBackend: failed to spawn codex: {e}");
            let _ = turn.event_tx.send(AgentEvent::Crashed);
            return false;
        }
    };

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => return false,
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => return false,
    };

    *turn.child_slot.lock() = Some(child);

    let stderr_tail = Arc::clone(turn.stderr_tail);
    let _ = std::thread::Builder::new()
        .name("glass-codex-stderr".into())
        .spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                push_stderr_tail(&stderr_tail, line);
            }
        });

    let reader = BufReader::new(stdout);
    for line in reader.lines().map_while(Result::ok) {
        if turn.stop.load(Ordering::Relaxed) {
            break;
        }
        if let Some(event) = parse::parse_codex_event(&line, turn.model) {
            if turn.event_tx.send(event).is_err() {
                return false;
            }
        }
    }

    let status = {
        let mut child = turn.child_slot.lock();
        child.as_mut().and_then(|child| child.wait().ok())
    };
    *turn.child_slot.lock() = None;

    if turn.stop.load(Ordering::Relaxed) {
        return false;
    }

    if !status.map(|status| status.success()).unwrap_or(false) {
        let tail = stderr_tail_to_string(turn.stderr_tail);
        if !tail.is_empty() {
            tracing::warn!("CodexCliBackend: codex exited unsuccessfully; stderr tail:\n{tail}");
        }
        let _ = turn.event_tx.send(AgentEvent::Crashed);
        return false;
    }

    true
}

fn apply_child_options(cmd: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
    }

    #[cfg(target_os = "macos")]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                std::thread::Builder::new()
                    .name("glass-codex-orphan-watchdog".into())
                    .spawn(|| loop {
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        if libc::getppid() == 1 {
                            std::process::exit(1);
                        }
                    })
                    .ok();
                Ok(())
            });
        }
    }
}

fn build_codex_args(
    model: &str,
    project_root: &str,
    initial_prompt: &str,
    mcp_command: Option<&std::path::Path>,
) -> Vec<String> {
    let mut args = vec![
        "exec".to_string(),
        "--json".to_string(),
        "--cd".to_string(),
        project_root.to_string(),
        "--ephemeral".to_string(),
        "--sandbox".to_string(),
        "read-only".to_string(),
    ];

    if !model.trim().is_empty() {
        args.push("--model".to_string());
        args.push(model.to_string());
    }

    if let Some(command) = mcp_command {
        let command = command.to_string_lossy();
        let command_value = serde_json::to_string(&command.as_ref()).unwrap_or_default();
        args.push("-c".to_string());
        args.push(format!("mcp_servers.glass.command={command_value}"));
        args.push("-c".to_string());
        args.push(r#"mcp_servers.glass.args=["mcp","serve"]"#.to_string());
    }

    args.push(initial_prompt.to_string());
    args
}

fn codex_command(args: &[String]) -> Command {
    if cfg!(target_os = "windows") {
        let mut cmd = Command::new("cmd.exe");
        cmd.arg("/C").arg("codex").args(args);
        cmd
    } else {
        let mut cmd = Command::new("codex");
        cmd.args(args);
        cmd
    }
}

fn format_initial_prompt(system_prompt: &str, initial_message: &Option<String>) -> String {
    let user_message = initial_message.as_deref().unwrap_or("GLASS_WAIT");
    sanitize_prompt_arg(&format!(
        "SYSTEM INSTRUCTIONS:\n{system_prompt}\n\nUSER MESSAGE:\n{user_message}"
    ))
}

fn sanitize_prompt_arg(prompt: &str) -> String {
    prompt.replace("\r\n", "\n").replace('\n', "\\n")
}

fn extract_user_content(raw: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) {
        if let Some(content) = value.pointer("/message/content").and_then(|c| c.as_str()) {
            return content.to_string();
        }
    }
    raw.to_string()
}

fn push_stderr_tail(tail: &Arc<Mutex<VecDeque<String>>>, line: String) {
    let mut tail = tail.lock();
    if tail.len() == 50 {
        tail.pop_front();
    }
    tail.push_back(line);
}

fn stderr_tail_to_string(tail: &Arc<Mutex<VecDeque<String>>>) -> String {
    tail.lock().iter().cloned().collect::<Vec<_>>().join("\n")
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

    #[test]
    fn build_args_uses_json_exec_and_default_model_when_empty() {
        let args = build_codex_args("", "C:\\repo", "hello", None);
        assert_eq!(args[0], "exec");
        assert!(args.contains(&"--json".to_string()));
        assert!(args.contains(&"--cd".to_string()));
        assert!(args.contains(&"C:\\repo".to_string()));
        assert!(args.contains(&"--ephemeral".to_string()));
        assert!(!args.contains(&"--model".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("hello"));
    }

    #[test]
    fn build_args_adds_model_and_mcp_config_overrides() {
        let args = build_codex_args(
            "gpt-4o",
            "/repo",
            "hello",
            Some(std::path::Path::new("/bin/glass")),
        );
        assert!(args.windows(2).any(|w| w == ["--model", "gpt-4o"]));
        assert!(args
            .iter()
            .any(|arg| arg == r#"mcp_servers.glass.command="/bin/glass""#));
        assert!(args
            .iter()
            .any(|arg| arg == r#"mcp_servers.glass.args=["mcp","serve"]"#));
    }

    #[test]
    fn extracts_stream_json_user_content() {
        let raw = r#"{"type":"user","message":{"role":"user","content":"hello"}}"#;
        assert_eq!(extract_user_content(raw), "hello");
        assert_eq!(extract_user_content("plain"), "plain");
    }

    #[test]
    fn initial_prompt_escapes_newlines_for_shell_wrappers() {
        let prompt = format_initial_prompt("line1\nline2", &Some("hello\nthere".to_string()));
        assert!(prompt.contains(r"line1\nline2"));
        assert!(prompt.contains(r"hello\nthere"));
        assert!(!prompt.contains('\n'));
    }

    #[test]
    fn uses_cmd_wrapper_on_windows() {
        let args = vec!["exec".to_string(), "--json".to_string()];
        let command = codex_command(&args);
        if cfg!(target_os = "windows") {
            assert_eq!(command.get_program(), "cmd.exe");
        } else {
            assert_eq!(command.get_program(), "codex");
        }
    }
}

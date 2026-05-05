use std::time::{Duration, Instant};

use glass_agent_backend::{
    AgentBackend, AgentEvent, AgentMode, BackendSpawnConfig, CodexCliBackend,
};

fn project_root() -> String {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap()
        .to_string_lossy()
        .to_string()
}

#[test]
#[ignore]
fn codex_cli_e2e_emits_basic_events() {
    let backend = CodexCliBackend::new();
    let config = BackendSpawnConfig {
        system_prompt: "You are a concise test agent.".to_string(),
        initial_message: Some("Say the single word HELLO and nothing else.".to_string()),
        project_root: project_root(),
        mcp_config_path: String::new(),
        allowed_tools: vec![],
        mode: AgentMode::Off,
        cooldown_secs: 0,
        restart_count: 0,
        last_crash: None,
    };

    let handle = backend.spawn(&config, 0).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut saw_init = false;
    let mut saw_text = false;
    let mut saw_turn_complete = false;

    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let timeout = remaining.min(Duration::from_millis(500));
        let Ok(event) = handle.event_rx.recv_timeout(timeout) else {
            continue;
        };

        match event {
            AgentEvent::Init { .. } => saw_init = true,
            AgentEvent::AssistantText { text } => {
                if text.contains("HELLO") {
                    saw_text = true;
                }
            }
            AgentEvent::TurnComplete { .. } => {
                saw_turn_complete = true;
                if saw_init && saw_text {
                    break;
                }
            }
            AgentEvent::Crashed => break,
            _ => {}
        }
    }

    backend.shutdown(handle.shutdown_token);

    assert!(saw_init, "expected Init event");
    assert!(saw_text, "expected AssistantText containing HELLO");
    assert!(saw_turn_complete, "expected TurnComplete event");
}

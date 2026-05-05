//! Parser for Codex CLI `codex exec --json` JSONL events.

use crate::AgentEvent;

pub fn parse_codex_event(line: &str, model: &str) -> Option<AgentEvent> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let event_type = value.get("type").and_then(|t| t.as_str())?;

    match event_type {
        "thread.started" => {
            let session_id = value.get("thread_id").and_then(|id| id.as_str())?;
            Some(AgentEvent::Init {
                session_id: session_id.to_owned(),
            })
        }
        "item.started" => parse_started_item(&value),
        "item.completed" => parse_completed_item(&value),
        "turn.completed" => {
            let input_tokens = value
                .pointer("/usage/input_tokens")
                .and_then(|tokens| tokens.as_u64())
                .unwrap_or(0);
            let output_tokens = value
                .pointer("/usage/output_tokens")
                .and_then(|tokens| tokens.as_u64())
                .unwrap_or(0);
            Some(AgentEvent::TurnComplete {
                cost_usd: cost_for_turn(model, input_tokens, output_tokens),
            })
        }
        _ => None,
    }
}

fn parse_started_item(value: &serde_json::Value) -> Option<AgentEvent> {
    let item = value.get("item")?;
    let item_type = item.get("type").and_then(|t| t.as_str())?;

    match item_type {
        "command_execution" => {
            let id = item.get("id").and_then(|id| id.as_str())?;
            let command = item.get("command").and_then(|command| command.as_str())?;
            let input = serde_json::json!({ "command": command }).to_string();
            Some(AgentEvent::ToolCall {
                name: item_type.to_owned(),
                id: id.to_owned(),
                input,
            })
        }
        _ => None,
    }
}

fn parse_completed_item(value: &serde_json::Value) -> Option<AgentEvent> {
    let item = value.get("item")?;
    let item_type = item.get("type").and_then(|t| t.as_str())?;

    match item_type {
        "agent_message" => {
            let text = item.get("text").and_then(|text| text.as_str())?;
            Some(AgentEvent::AssistantText {
                text: text.to_owned(),
            })
        }
        "reasoning" => {
            let text = item
                .get("text")
                .or_else(|| item.get("summary"))
                .and_then(|text| text.as_str())?;
            Some(AgentEvent::Thinking {
                text: text.to_owned(),
            })
        }
        "command_execution" => {
            let id = item.get("id").and_then(|id| id.as_str())?;
            let content = item
                .get("aggregated_output")
                .and_then(|output| output.as_str())
                .unwrap_or("");
            Some(AgentEvent::ToolResult {
                tool_use_id: id.to_owned(),
                content: content.to_owned(),
            })
        }
        _ => None,
    }
}

pub fn cost_for_turn(model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
    let Some((input_per_million, output_per_million)) = price_for_model(model) else {
        return 0.0;
    };

    (input_tokens as f64 * input_per_million / 1_000_000.0)
        + (output_tokens as f64 * output_per_million / 1_000_000.0)
}

fn price_for_model(model: &str) -> Option<(f64, f64)> {
    let model = model.trim();
    if model.starts_with("gpt-5-codex") {
        Some((1.25, 10.0))
    } else if model.starts_with("gpt-4o-mini") {
        Some((0.15, 0.60))
    } else if model.starts_with("gpt-4o") {
        Some((2.50, 10.0))
    } else if model.starts_with("o1-mini") {
        Some((1.10, 4.40))
    } else if model.starts_with("o1") {
        Some((15.0, 60.0))
    } else if model.starts_with("o3-mini") {
        Some((1.10, 4.40))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("codex")
            .join(name);
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn parses_init_fixture() {
        match parse_codex_event(&fixture("init.jsonl"), "gpt-5.5") {
            Some(AgentEvent::Init { session_id }) => {
                assert_eq!(session_id, "019df94b-9e24-7571-9453-6a4c64c4dc22");
            }
            other => panic!("expected Init, got {other:?}"),
        }
    }

    #[test]
    fn parses_text_fixture() {
        match parse_codex_event(&fixture("text_delta.jsonl"), "gpt-5.5") {
            Some(AgentEvent::AssistantText { text }) => assert_eq!(text, "Hello."),
            other => panic!("expected AssistantText, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_call_fixture() {
        match parse_codex_event(&fixture("tool_call.jsonl"), "gpt-5.5") {
            Some(AgentEvent::ToolCall { name, id, input }) => {
                assert_eq!(name, "command_execution");
                assert_eq!(id, "item_0");
                assert!(input.contains("Get-ChildItem -Name Cargo.toml"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_result_fixture() {
        match parse_codex_event(&fixture("tool_result.jsonl"), "gpt-5.5") {
            Some(AgentEvent::ToolResult {
                tool_use_id,
                content,
            }) => {
                assert_eq!(tool_use_id, "item_0");
                assert_eq!(content, "Cargo.toml\r\n");
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn parses_turn_end_fixture() {
        match parse_codex_event(&fixture("turn_end.jsonl"), "gpt-4o") {
            Some(AgentEvent::TurnComplete { cost_usd }) => {
                assert!(cost_usd > 0.0, "cost was {cost_usd}");
            }
            other => panic!("expected TurnComplete, got {other:?}"),
        }
    }

    #[test]
    fn unknown_type_returns_none() {
        assert!(parse_codex_event(r#"{"type":"unknown"}"#, "gpt-5.5").is_none());
    }

    #[test]
    fn malformed_json_returns_none() {
        assert!(parse_codex_event("not json", "gpt-5.5").is_none());
    }

    #[test]
    fn cost_for_known_model() {
        let cost = cost_for_turn("gpt-4o", 1_000_000, 1_000_000);
        assert!((cost - 12.5).abs() < f64::EPSILON);
    }

    #[test]
    fn cost_for_unknown_model_is_zero() {
        assert_eq!(cost_for_turn("not-a-model", 1_000_000, 1_000_000), 0.0);
    }
}

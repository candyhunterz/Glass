//! Ephemeral agent: short-lived claude CLI sessions for checkpoint
//! synthesis and quality verification.
//!
//! Spawns a `claude` process on a background thread with a purpose-built
//! system prompt, sends a single user message, reads the response, and
//! signals completion via `AppEvent::EphemeralAgentComplete`.

use std::io::{BufRead, BufReader, Write as IoWrite};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use glass_core::event::{
    AppEvent, EphemeralAgentError, EphemeralAgentResult, EphemeralPurpose,
};
use winit::event_loop::EventLoopProxy;

/// Request for an ephemeral agent session.
pub struct EphemeralAgentRequest {
    pub system_prompt: String,
    pub user_message: String,
    pub timeout: Duration,
    pub purpose: EphemeralPurpose,
}

/// Parse the assistant text from a stream-json assistant message.
fn parse_assistant_text(line: &str) -> Option<String> {
    let val: serde_json::Value = serde_json::from_str(line).ok()?;
    if val.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let content = val.get("message")?.get("content")?.as_array()?;
    let mut text = String::new();
    for block in content {
        if block.get("type").and_then(|v| v.as_str()) == Some("text") {
            if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                text.push_str(t);
            }
        }
    }
    if text.is_empty() { None } else { Some(text) }
}

/// Parse cost and duration from a stream-json result message.
fn parse_result_message(line: &str) -> Option<(Option<f64>, Option<u64>)> {
    let val: serde_json::Value = serde_json::from_str(line).ok()?;
    if val.get("type")?.as_str()? != "result" {
        return None;
    }
    let cost = val.get("cost_usd").and_then(|v| v.as_f64());
    let duration = val.get("duration_ms").and_then(|v| v.as_u64());
    Some((cost, duration))
}

/// Strip markdown code fences from a response.
pub fn strip_markdown_fences(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_string();
        }
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_string();
        }
    }
    trimmed.to_string()
}

/// Spawn a short-lived claude session on a background thread.
/// Returns immediately. Completion is signaled via AppEvent::EphemeralAgentComplete.
pub fn spawn_ephemeral_agent(
    request: EphemeralAgentRequest,
    proxy: EventLoopProxy<AppEvent>,
) -> Result<(), EphemeralAgentError> {
    std::thread::Builder::new()
        .name("glass-ephemeral".into())
        .spawn(move || {
            let result = run_ephemeral_blocking(&request);
            match &result {
                Ok(resp) => {
                    tracing::info!(
                        "Ephemeral agent ({:?}): completed in {:?}ms, cost={:?}",
                        request.purpose, resp.duration_ms, resp.cost_usd,
                    );
                }
                Err(e) => {
                    tracing::warn!("Ephemeral agent ({:?}): failed: {:?}", request.purpose, e);
                }
            }
            let _ = proxy.send_event(AppEvent::EphemeralAgentComplete {
                result,
                purpose: request.purpose,
            });
        })
        .map_err(|e| EphemeralAgentError::SpawnFailed(e.to_string()))?;
    Ok(())
}

/// Blocking implementation of an ephemeral agent session.
fn run_ephemeral_blocking(
    request: &EphemeralAgentRequest,
) -> Result<EphemeralAgentResult, EphemeralAgentError> {
    let started_at = Instant::now();

    // Write system prompt to temp file (auto-deleted on drop)
    let mut prompt_file = tempfile::NamedTempFile::new()
        .map_err(|e| EphemeralAgentError::SpawnFailed(format!("tempfile: {e}")))?;
    prompt_file
        .write_all(request.system_prompt.as_bytes())
        .map_err(|e| EphemeralAgentError::SpawnFailed(format!("write prompt: {e}")))?;
    let prompt_path = prompt_file.path().to_string_lossy().to_string();

    // Build command
    let mut cmd = Command::new("claude");
    cmd.args([
        "-p",
        "--output-format", "stream-json",
        "--input-format", "stream-json",
        "--system-prompt-file", &prompt_path,
        "--allowedTools", "",
        "--dangerously-skip-permissions",
    ]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());

    // Windows: suppress console window
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    // Linux: kill child if Glass exits
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

    let mut child = cmd
        .spawn()
        .map_err(|e| EphemeralAgentError::SpawnFailed(format!("spawn claude: {e}")))?;

    // Send user message via stdin
    let user_msg = serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": request.user_message
        }
    });
    if let Some(mut stdin) = child.stdin.take() {
        let msg_str = serde_json::to_string(&user_msg).unwrap_or_default();
        let _ = writeln!(stdin, "{msg_str}");
        // Drop stdin to signal EOF
    }

    // Read stdout until result message or timeout
    let mut response_text = String::new();
    let mut cost_usd = None;
    let mut duration_ms = None;

    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line_result in reader.lines() {
            if started_at.elapsed() > request.timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err(EphemeralAgentError::Timeout);
            }
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };
            if line.trim().is_empty() { continue; }
            if let Some(text) = parse_assistant_text(&line) {
                response_text = text;
            }
            if let Some((c, d)) = parse_result_message(&line) {
                cost_usd = c;
                duration_ms = d;
                break;
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    if response_text.is_empty() {
        return Err(EphemeralAgentError::ParseError(
            "no assistant text in response".to_string(),
        ));
    }

    response_text = strip_markdown_fences(&response_text);

    Ok(EphemeralAgentResult {
        text: response_text,
        cost_usd,
        duration_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_assistant_text_basic() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hello world"}]}}"#;
        assert_eq!(parse_assistant_text(line), Some("hello world".to_string()));
    }

    #[test]
    fn parse_assistant_text_multi_block() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"part1"},{"type":"text","text":" part2"}]}}"#;
        assert_eq!(parse_assistant_text(line), Some("part1 part2".to_string()));
    }

    #[test]
    fn parse_assistant_text_non_assistant() {
        let line = r#"{"type":"result","cost_usd":0.01}"#;
        assert_eq!(parse_assistant_text(line), None);
    }

    #[test]
    fn parse_assistant_text_malformed() {
        assert_eq!(parse_assistant_text("not json"), None);
        assert_eq!(parse_assistant_text(""), None);
    }

    #[test]
    fn parse_result_with_cost() {
        let line = r#"{"type":"result","cost_usd":0.00234,"duration_ms":1200}"#;
        let (cost, dur) = parse_result_message(line).unwrap();
        assert!((cost.unwrap() - 0.00234).abs() < 1e-6);
        assert_eq!(dur, Some(1200));
    }

    #[test]
    fn parse_result_missing_fields() {
        let line = r#"{"type":"result"}"#;
        let (cost, dur) = parse_result_message(line).unwrap();
        assert!(cost.is_none());
        assert!(dur.is_none());
    }

    #[test]
    fn parse_result_non_result() {
        let line = r#"{"type":"assistant","message":{}}"#;
        assert!(parse_result_message(line).is_none());
    }

    #[test]
    fn strip_fences_json() {
        let text = "```json\n{\"score\": 7}\n```";
        assert_eq!(strip_markdown_fences(text), "{\"score\": 7}");
    }

    #[test]
    fn strip_fences_plain() {
        let text = "```\nhello\n```";
        assert_eq!(strip_markdown_fences(text), "hello");
    }

    #[test]
    fn strip_fences_none() {
        let text = "{\"score\": 7}";
        assert_eq!(strip_markdown_fences(text), "{\"score\": 7}");
    }

    #[test]
    fn strip_fences_with_whitespace() {
        let text = "  ```json\n  {\"score\": 7}  \n```  ";
        assert_eq!(strip_markdown_fences(text), "{\"score\": 7}");
    }
}

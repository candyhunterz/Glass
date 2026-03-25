//! LLM provider detection for the onboarding welcome overlay.
//!
//! Scans for available providers: Claude CLI (PATH), OpenAI (env var),
//! Anthropic (env var), Ollama (env var or TCP probe). This module
//! performs I/O and should be called from main.rs, not the coordinator.

use crate::onboarding::ProviderStatus;

/// Detect all available LLM providers. Returns status for each known provider.
///
/// - Claude CLI: scans PATH for `claude` / `claude.exe` binary
/// - OpenAI: checks `OPENAI_API_KEY` environment variable
/// - Anthropic: checks `ANTHROPIC_API_KEY` environment variable
/// - Ollama: checks `OLLAMA_HOST` env var, then probes `localhost:11434` with 200ms timeout
pub async fn detect_providers() -> Vec<ProviderStatus> {
    let mut providers = Vec::with_capacity(4);

    providers.push(detect_claude_cli());
    providers.push(detect_env_provider("OpenAI API", "OPENAI_API_KEY"));
    providers.push(detect_env_provider("Anthropic API", "ANTHROPIC_API_KEY"));
    providers.push(detect_ollama().await);

    providers
}

/// Check if `claude` binary is on PATH by scanning PATH directories.
fn detect_claude_cli() -> ProviderStatus {
    let binary_name = if cfg!(target_os = "windows") {
        "claude.exe"
    } else {
        "claude"
    };

    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(binary_name);
            if candidate.exists() {
                return ProviderStatus {
                    name: "Claude CLI",
                    available: true,
                    detail: "found".to_string(),
                };
            }
        }
    }

    ProviderStatus {
        name: "Claude CLI",
        available: false,
        detail: "not on PATH".to_string(),
    }
}

/// Check for an API key in the environment.
fn detect_env_provider(name: &'static str, env_var: &str) -> ProviderStatus {
    let value = std::env::var(env_var).ok();
    check_env_value(name, value)
}

/// Pure helper: check an optional env value. Testable without mutating the environment.
fn check_env_value(name: &'static str, value: Option<String>) -> ProviderStatus {
    match value {
        Some(val) if !val.is_empty() => ProviderStatus {
            name,
            available: true,
            detail: "key set".to_string(),
        },
        _ => ProviderStatus {
            name,
            available: false,
            detail: "no key".to_string(),
        },
    }
}

/// Check for Ollama: OLLAMA_HOST env var, or TCP probe of localhost:11434.
async fn detect_ollama() -> ProviderStatus {
    // Check env var first
    if let Ok(host) = std::env::var("OLLAMA_HOST") {
        if !host.is_empty() {
            return ProviderStatus {
                name: "Ollama",
                available: true,
                detail: format!("host: {host}"),
            };
        }
    }

    // Probe default port with 200ms timeout
    let probe = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        tokio::net::TcpStream::connect("127.0.0.1:11434"),
    )
    .await;

    match probe {
        Ok(Ok(_)) => ProviderStatus {
            name: "Ollama",
            available: true,
            detail: "running".to_string(),
        },
        _ => ProviderStatus {
            name: "Ollama",
            available: false,
            detail: "not running".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_provider_detects_set_key() {
        let status = check_env_value("Test Provider", Some("sk-test-123".to_string()));
        assert!(status.available);
        assert_eq!(status.detail, "key set");
    }

    #[test]
    fn env_provider_detects_missing_key() {
        let status = check_env_value("Test Provider", None);
        assert!(!status.available);
        assert_eq!(status.detail, "no key");
    }

    #[test]
    fn env_provider_rejects_empty_key() {
        let status = check_env_value("Test Provider", Some(String::new()));
        assert!(!status.available);
    }
}

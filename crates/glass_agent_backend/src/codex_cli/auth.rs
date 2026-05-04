//! Codex token-store discovery and presence check.
//!
//! Glass never reads the token contents — only checks whether the file exists
//! so we can surface a friendly `LoginRequired` error before spawning `codex`.

use std::path::PathBuf;

/// Default path to Codex's OAuth token store on the current platform.
///
/// Returns `None` if the home directory cannot be determined.
/// Override with the `CODEX_HOME` environment variable (matches Codex CLI's own behavior).
pub fn codex_token_path() -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("CODEX_HOME") {
        return Some(PathBuf::from(custom).join("auth.json"));
    }
    let home = dirs::home_dir()?;
    Some(home.join(".codex").join("auth.json"))
}

/// Returns `true` if the Codex token file exists. Does NOT validate or parse contents.
pub fn is_logged_in() -> bool {
    codex_token_path().map(|p| p.exists()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_token_path_respects_env_override() {
        let tmp = std::env::temp_dir().join("glass-codex-test");
        std::env::set_var("CODEX_HOME", &tmp);
        let path = codex_token_path().expect("path");
        assert_eq!(path, tmp.join("auth.json"));
        std::env::remove_var("CODEX_HOME");
    }

    #[test]
    fn codex_token_path_default_is_under_home() {
        std::env::remove_var("CODEX_HOME");
        let path = codex_token_path().expect("home should exist on test machines");
        let s = path.to_string_lossy();
        assert!(s.contains(".codex"), "path was: {s}");
        assert!(s.ends_with("auth.json"), "path was: {s}");
    }

    #[test]
    fn is_logged_in_returns_false_for_nonexistent_dir() {
        let tmp = std::env::temp_dir().join("glass-codex-no-such-dir");
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::set_var("CODEX_HOME", &tmp);
        assert!(!is_logged_in());
        std::env::remove_var("CODEX_HOME");
    }

    #[test]
    fn is_logged_in_returns_true_when_file_exists() {
        let tmp = std::env::temp_dir().join("glass-codex-fake-login");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("auth.json"), "fake").unwrap();
        std::env::set_var("CODEX_HOME", &tmp);
        assert!(is_logged_in());
        std::env::remove_var("CODEX_HOME");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

use serde::Deserialize;

/// Configuration for history database behavior.
#[derive(Clone, Debug, Deserialize)]
pub struct HistoryConfig {
    /// Maximum age of records in days. Records older than this are pruned.
    pub max_age_days: u32,
    /// Maximum database size in bytes. Oldest records are pruned when exceeded.
    pub max_size_bytes: u64,
    /// Maximum output capture size in kilobytes. Output exceeding this is truncated.
    #[serde(default = "default_max_output_capture_kb")]
    pub max_output_capture_kb: u32,
}

fn default_max_output_capture_kb() -> u32 {
    50
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            max_age_days: 30,
            max_size_bytes: 1_073_741_824, // 1 GB
            max_output_capture_kb: default_max_output_capture_kb(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = HistoryConfig::default();
        assert_eq!(config.max_age_days, 30);
        assert_eq!(config.max_size_bytes, 1_073_741_824);
    }

    #[test]
    fn test_max_output_capture_kb_default() {
        let config = HistoryConfig::default();
        assert_eq!(config.max_output_capture_kb, 50);
    }

    #[test]
    fn test_max_output_capture_kb_from_toml() {
        let toml_str = r#"
            max_age_days = 30
            max_size_bytes = 1073741824
            max_output_capture_kb = 100
        "#;
        let config: HistoryConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.max_output_capture_kb, 100);
    }

    #[test]
    fn test_max_output_capture_kb_missing_uses_default() {
        let toml_str = r#"
            max_age_days = 30
            max_size_bytes = 1073741824
        "#;
        let config: HistoryConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.max_output_capture_kb, 50);
    }
}

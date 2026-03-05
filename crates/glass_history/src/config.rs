use serde::Deserialize;

/// Configuration for history database behavior.
#[derive(Clone, Debug, Deserialize)]
pub struct HistoryConfig {
    /// Maximum age of records in days. Records older than this are pruned.
    pub max_age_days: u32,
    /// Maximum database size in bytes. Oldest records are pruned when exceeded.
    pub max_size_bytes: u64,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            max_age_days: 30,
            max_size_bytes: 1_073_741_824, // 1 GB
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
}

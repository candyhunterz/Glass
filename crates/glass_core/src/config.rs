use serde::Deserialize;

/// Glass terminal configuration, loaded from `~/.glass/config.toml`.
///
/// All fields have sensible defaults. Missing fields in the TOML file
/// are filled from the `Default` implementation. A missing or malformed
/// config file silently falls back to all defaults (no crash, no error dialog).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GlassConfig {
    pub font_family: String,
    pub font_size: f32,
    pub shell: Option<String>,
    /// History configuration section. Optional in the TOML file;
    /// uses defaults (max_output_capture_kb=50) when absent.
    pub history: Option<HistorySection>,
    /// Snapshot configuration section. Optional in the TOML file;
    /// uses defaults when present without explicit field values.
    pub snapshot: Option<SnapshotSection>,
}

/// History-related configuration in the `[history]` TOML section.
#[derive(Debug, Clone, Deserialize)]
pub struct HistorySection {
    /// Maximum output capture size in kilobytes. Default 50.
    #[serde(default = "default_max_output_capture_kb")]
    pub max_output_capture_kb: u32,
}

fn default_max_output_capture_kb() -> u32 {
    50
}

/// Snapshot-related configuration in the `[snapshot]` TOML section.
#[derive(Debug, Clone, Deserialize)]
pub struct SnapshotSection {
    /// Whether snapshot capture is enabled. Default true.
    #[serde(default = "default_snapshot_enabled")]
    pub enabled: bool,
    /// Maximum number of snapshots to retain. Default 1000.
    #[serde(default = "default_max_count")]
    pub max_count: u32,
    /// Maximum total blob storage size in megabytes. Default 500.
    #[serde(default = "default_max_size_mb")]
    pub max_size_mb: u32,
    /// Number of days to retain snapshots. Default 30.
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
}

fn default_snapshot_enabled() -> bool {
    true
}
fn default_max_count() -> u32 {
    1000
}
fn default_max_size_mb() -> u32 {
    500
}
fn default_retention_days() -> u32 {
    30
}

impl Default for GlassConfig {
    fn default() -> Self {
        Self {
            font_family: "Consolas".into(),
            font_size: 14.0,
            shell: None,
            history: None,
            snapshot: None,
        }
    }
}

impl GlassConfig {
    /// Load configuration from `~/.glass/config.toml`.
    ///
    /// Returns `Self::default()` if the file is missing, unreadable, or malformed.
    /// Does NOT create the config file if it does not exist.
    pub fn load() -> Self {
        let Some(home) = dirs::home_dir() else {
            tracing::debug!("Could not determine home directory; using default config");
            return Self::default();
        };

        let config_path = home.join(".glass").join("config.toml");

        match std::fs::read_to_string(&config_path) {
            Ok(contents) => {
                let config = Self::load_from_str(&contents);
                tracing::info!("Loaded config from {}", config_path.display());
                config
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                tracing::debug!("No config file at {}; using defaults", config_path.display());
                Self::default()
            }
            Err(err) => {
                tracing::warn!("Failed to read {}: {}; using defaults", config_path.display(), err);
                Self::default()
            }
        }
    }

    /// Parse a TOML string into a `GlassConfig`, falling back to defaults on error.
    ///
    /// Used by `load()` and useful for testing.
    pub fn load_from_str(s: &str) -> Self {
        match toml::from_str(s) {
            Ok(config) => config,
            Err(err) => {
                tracing::warn!("Failed to parse config TOML: {err}; using defaults");
                Self::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_full_config() {
        let toml = "font_family = \"Cascadia Code\"\nfont_size = 16.0\nshell = \"bash\"";
        let config = GlassConfig::load_from_str(toml);
        assert_eq!(config.font_family, "Cascadia Code");
        assert_eq!(config.font_size, 16.0);
        assert_eq!(config.shell, Some("bash".to_owned()));
    }

    #[test]
    fn load_partial_config() {
        let toml = "font_size = 18.0";
        let config = GlassConfig::load_from_str(toml);
        assert_eq!(config.font_size, 18.0);
        assert_eq!(config.font_family, "Consolas"); // default
        assert_eq!(config.shell, None); // default
    }

    #[test]
    fn load_empty_config() {
        let config = GlassConfig::load_from_str("");
        assert_eq!(config.font_family, "Consolas");
        assert_eq!(config.font_size, 14.0);
        assert_eq!(config.shell, None);
    }

    #[test]
    fn load_malformed_toml_returns_defaults() {
        let config = GlassConfig::load_from_str("invalid {{{{");
        assert_eq!(config.font_family, "Consolas");
        assert_eq!(config.font_size, 14.0);
        assert_eq!(config.shell, None);
    }

    #[test]
    fn empty_toml_has_no_snapshot_section() {
        let config = GlassConfig::load_from_str("");
        assert!(config.snapshot.is_none());
    }

    #[test]
    fn snapshot_section_with_no_fields_uses_defaults() {
        let toml = "[snapshot]";
        let config = GlassConfig::load_from_str(toml);
        let snap = config.snapshot.expect("snapshot section should be Some");
        assert!(snap.enabled);
        assert_eq!(snap.max_count, 1000);
        assert_eq!(snap.max_size_mb, 500);
        assert_eq!(snap.retention_days, 30);
    }

    #[test]
    fn snapshot_section_partial_fields() {
        let toml = "[snapshot]\nenabled = false\nmax_count = 50";
        let config = GlassConfig::load_from_str(toml);
        let snap = config.snapshot.expect("snapshot section should be Some");
        assert!(!snap.enabled);
        assert_eq!(snap.max_count, 50);
        assert_eq!(snap.max_size_mb, 500); // default
        assert_eq!(snap.retention_days, 30); // default
    }

    #[test]
    fn snapshot_section_all_fields() {
        let toml = "[snapshot]\nenabled = true\nmax_count = 2000\nmax_size_mb = 1024\nretention_days = 90";
        let config = GlassConfig::load_from_str(toml);
        let snap = config.snapshot.expect("snapshot section should be Some");
        assert!(snap.enabled);
        assert_eq!(snap.max_count, 2000);
        assert_eq!(snap.max_size_mb, 1024);
        assert_eq!(snap.retention_days, 90);
    }

    #[test]
    fn load_missing_file_returns_defaults() {
        // GlassConfig::load() should return defaults when no config file exists
        // We can't guarantee ~/.glass/config.toml doesn't exist, but load() should never panic
        let config = GlassConfig::load();
        // At minimum, it should return a valid config (either loaded or default)
        assert!(!config.font_family.is_empty());
        assert!(config.font_size > 0.0);
    }
}

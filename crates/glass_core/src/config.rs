#[derive(Debug, Clone)]
pub struct GlassConfig {
    pub font_family: String,
    pub font_size: f32,
    pub shell: Option<String>,
}

impl Default for GlassConfig {
    fn default() -> Self {
        Self {
            font_family: "Consolas".into(),
            font_size: 14.0,
            shell: None,
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
    fn load_missing_file_returns_defaults() {
        // GlassConfig::load() should return defaults when no config file exists
        // We can't guarantee ~/.glass/config.toml doesn't exist, but load() should never panic
        let config = GlassConfig::load();
        // At minimum, it should return a valid config (either loaded or default)
        assert!(!config.font_family.is_empty());
        assert!(config.font_size > 0.0);
    }
}

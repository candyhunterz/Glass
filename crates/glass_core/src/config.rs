use serde::Deserialize;
use std::fmt;

/// Permission level for an agent action category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionLevel {
    /// Agent must request user approval before acting.
    #[default]
    Approve,
    /// Agent may act automatically without user approval.
    Auto,
    /// Agent is never allowed to perform this category of action.
    Never,
}

/// Category of action a proposal is requesting permission for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionKind {
    /// Proposal edits one or more files.
    EditFiles,
    /// Proposal runs shell commands (non-git).
    RunCommands,
    /// Proposal performs a git operation.
    GitOperations,
}

/// Per-category permission levels for the agent runtime.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PermissionMatrix {
    /// Permission level for file-editing proposals.
    #[serde(default)]
    pub edit_files: PermissionLevel,
    /// Permission level for shell-command proposals.
    #[serde(default)]
    pub run_commands: PermissionLevel,
    /// Permission level for git-operation proposals.
    #[serde(default)]
    pub git_operations: PermissionLevel,
}

impl Default for PermissionMatrix {
    fn default() -> Self {
        Self {
            edit_files: PermissionLevel::Approve,
            run_commands: PermissionLevel::Approve,
            git_operations: PermissionLevel::Approve,
        }
    }
}

/// Rules that suppress agent notifications for low-signal events.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct QuietRules {
    /// When true, suppress notifications for events with severity "Success".
    #[serde(default)]
    pub ignore_exit_zero: bool,
    /// Suppress notifications whose summary contains any of these substrings.
    #[serde(default)]
    pub ignore_patterns: Vec<String>,
}

/// Structured error from config validation, including location info when available.
#[derive(Debug, Clone)]
pub struct ConfigError {
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub snippet: Option<String>,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (Some(line), Some(col)) => {
                write!(
                    f,
                    "Config error (line {}, col {}): {}",
                    line, col, self.message
                )
            }
            _ => write!(f, "Config error: {}", self.message),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Agent runtime configuration in the `[agent]` TOML section.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AgentSection {
    /// Agent operation mode. Default: Off.
    #[serde(default)]
    pub mode: crate::agent_runtime::AgentMode,
    /// Maximum cost budget in USD. Default 1.0.
    #[serde(default = "default_agent_max_budget_usd")]
    pub max_budget_usd: f64,
    /// Cooldown window in seconds between forwarded events. Default 30.
    #[serde(default = "default_agent_cooldown_secs")]
    pub cooldown_secs: u64,
    /// Comma-separated list of allowed MCP tools.
    #[serde(default = "default_agent_allowed_tools")]
    pub allowed_tools: String,
    /// Per-category permission levels. None when section is absent.
    #[serde(default)]
    pub permissions: Option<PermissionMatrix>,
    /// Rules for suppressing low-signal notifications. None when section is absent.
    #[serde(default)]
    pub quiet_rules: Option<QuietRules>,
}

fn default_agent_max_budget_usd() -> f64 {
    1.0
}
fn default_agent_cooldown_secs() -> u64 {
    30
}
fn default_agent_allowed_tools() -> String {
    "glass_query,glass_context,Bash,Read".to_string()
}

impl Default for AgentSection {
    fn default() -> Self {
        Self {
            mode: crate::agent_runtime::AgentMode::Off,
            max_budget_usd: default_agent_max_budget_usd(),
            cooldown_secs: default_agent_cooldown_secs(),
            allowed_tools: default_agent_allowed_tools(),
            permissions: None,
            quiet_rules: None,
        }
    }
}

/// Glass terminal configuration, loaded from `~/.glass/config.toml`.
///
/// All fields have sensible defaults. Missing fields in the TOML file
/// are filled from the `Default` implementation. A missing or malformed
/// config file silently falls back to all defaults (no crash, no error dialog).
#[derive(Debug, Clone, PartialEq, Deserialize)]
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
    /// Pipe visualization configuration section. Optional in the TOML file;
    /// uses defaults when present without explicit field values.
    pub pipes: Option<PipesSection>,
    /// SOI configuration section. Optional in the TOML file;
    /// uses defaults when present without explicit field values.
    pub soi: Option<SoiSection>,
    /// Agent runtime configuration section. Optional; defaults to Off mode.
    pub agent: Option<AgentSection>,
}

/// History-related configuration in the `[history]` TOML section.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HistorySection {
    /// Maximum output capture size in kilobytes. Default 50.
    #[serde(default = "default_max_output_capture_kb")]
    pub max_output_capture_kb: u32,
}

fn default_max_output_capture_kb() -> u32 {
    50
}

/// Snapshot-related configuration in the `[snapshot]` TOML section.
#[derive(Debug, Clone, PartialEq, Deserialize)]
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

/// SOI (Structured Output Intelligence) configuration in the `[soi]` TOML section.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SoiSection {
    /// Whether SOI parsing and display is enabled. Default true.
    #[serde(default = "default_soi_enabled")]
    pub enabled: bool,
    /// Whether to use the shell's summary command for SOI. Default false.
    #[serde(default = "default_soi_shell_summary")]
    pub shell_summary: bool,
    /// Display format for SOI labels. Default "oneline".
    #[serde(default = "default_soi_format")]
    pub format: String,
    /// Minimum number of output lines before SOI label is shown. Default 0.
    #[serde(default)]
    pub min_lines: u32,
}

fn default_soi_enabled() -> bool {
    true
}
fn default_soi_shell_summary() -> bool {
    false
}
fn default_soi_format() -> String {
    "oneline".to_string()
}

/// Pipe visualization configuration in the `[pipes]` TOML section.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PipesSection {
    /// Whether pipe stage capture is enabled. Default true.
    #[serde(default = "default_pipes_enabled")]
    pub enabled: bool,
    /// Maximum capture size per stage in megabytes. Default 10.
    #[serde(default = "default_max_capture_mb")]
    pub max_capture_mb: u32,
    /// Whether to auto-expand pipeline blocks on failure or many stages. Default true.
    #[serde(default = "default_auto_expand")]
    pub auto_expand: bool,
}

fn default_pipes_enabled() -> bool {
    true
}
fn default_max_capture_mb() -> u32 {
    10
}
fn default_auto_expand() -> bool {
    true
}

fn default_font_family() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "Consolas"
    }
    #[cfg(target_os = "macos")]
    {
        "Menlo"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        "Monospace"
    }
}

impl Default for GlassConfig {
    fn default() -> Self {
        Self {
            font_family: default_font_family().into(),
            font_size: 14.0,
            shell: None,
            history: None,
            snapshot: None,
            pipes: None,
            soi: None,
            agent: None,
        }
    }
}

impl GlassConfig {
    /// Returns the path to `~/.glass/config.toml`, or None if home dir is unavailable.
    pub fn config_path() -> Option<std::path::PathBuf> {
        dirs::home_dir().map(|h| h.join(".glass").join("config.toml"))
    }

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
                tracing::debug!(
                    "No config file at {}; using defaults",
                    config_path.display()
                );
                Self::default()
            }
            Err(err) => {
                tracing::warn!(
                    "Failed to read {}: {}; using defaults",
                    config_path.display(),
                    err
                );
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

    /// Parse a TOML string into a `GlassConfig`, returning a structured error on failure.
    ///
    /// Unlike `load_from_str()`, this returns `Err(ConfigError)` with line/column info
    /// so callers can display actionable error messages to the user.
    pub fn load_validated(s: &str) -> Result<Self, ConfigError> {
        toml::from_str(s).map_err(|e| {
            let message = e.message().to_string();
            let (line, column, snippet) = if let Some(span) = e.span() {
                let prefix = &s[..span.start];
                let line = prefix.chars().filter(|&c| c == '\n').count() + 1;
                let last_newline = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
                let column = span.start - last_newline + 1;
                let snippet = s.lines().nth(line - 1).map(|l| l.to_string());
                (Some(line), Some(column), snippet)
            } else {
                (None, None, None)
            };
            ConfigError {
                message,
                line,
                column,
                snippet,
            }
        })
    }

    /// Returns true if font-related settings differ between two configs.
    ///
    /// Used by the config watcher to decide whether a font rebuild is needed.
    pub fn font_changed(&self, other: &GlassConfig) -> bool {
        self.font_family != other.font_family || self.font_size != other.font_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === ConfigError + load_validated tests ===

    #[test]
    fn load_validated_malformed_toml_returns_error() {
        let result = GlassConfig::load_validated("invalid {{{{");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn load_validated_type_mismatch_returns_error_with_line() {
        let result = GlassConfig::load_validated("font_size = \"not_a_number\"");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.line, Some(1));
        // Message should contain some field context
        assert!(!err.message.is_empty());
    }

    #[test]
    fn load_validated_valid_toml_returns_ok() {
        let result = GlassConfig::load_validated("font_family = \"Cascadia\"\nfont_size = 16.0");
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.font_family, "Cascadia");
        assert_eq!(config.font_size, 16.0);
    }

    #[test]
    fn load_validated_empty_returns_default() {
        let result = GlassConfig::load_validated("");
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.font_family, default_font_family());
        assert_eq!(config.font_size, 14.0);
    }

    #[test]
    fn config_error_display_with_line_col() {
        let err = ConfigError {
            message: "expected string".to_string(),
            line: Some(3),
            column: Some(5),
            snippet: None,
        };
        let display = format!("{}", err);
        assert_eq!(display, "Config error (line 3, col 5): expected string");
    }

    #[test]
    fn config_error_display_without_line_col() {
        let err = ConfigError {
            message: "something went wrong".to_string(),
            line: None,
            column: None,
            snippet: None,
        };
        let display = format!("{}", err);
        assert_eq!(display, "Config error: something went wrong");
    }

    #[test]
    fn font_changed_same_font_different_shell() {
        let a = GlassConfig {
            shell: Some("bash".to_string()),
            ..GlassConfig::default()
        };
        let b = GlassConfig {
            shell: Some("zsh".to_string()),
            ..GlassConfig::default()
        };
        assert!(!a.font_changed(&b));
    }

    #[test]
    fn font_changed_different_font_size() {
        let a = GlassConfig::default();
        let b = GlassConfig {
            font_size: 18.0,
            ..GlassConfig::default()
        };
        assert!(a.font_changed(&b));
    }

    #[test]
    fn font_changed_different_font_family() {
        let a = GlassConfig::default();
        let b = GlassConfig {
            font_family: "JetBrains Mono".to_string(),
            ..GlassConfig::default()
        };
        assert!(a.font_changed(&b));
    }

    #[test]
    fn glass_config_partial_eq() {
        let a = GlassConfig::default();
        let b = GlassConfig::default();
        assert_eq!(a, b);

        let c = GlassConfig {
            font_size: 20.0,
            ..GlassConfig::default()
        };
        assert_ne!(a, c);
    }

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
        assert_eq!(config.font_family, default_font_family()); // default
        assert_eq!(config.shell, None); // default
    }

    #[test]
    fn load_empty_config() {
        let config = GlassConfig::load_from_str("");
        assert_eq!(config.font_family, default_font_family());
        assert_eq!(config.font_size, 14.0);
        assert_eq!(config.shell, None);
    }

    #[test]
    fn load_malformed_toml_returns_defaults() {
        let config = GlassConfig::load_from_str("invalid {{{{");
        assert_eq!(config.font_family, default_font_family());
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
        let toml =
            "[snapshot]\nenabled = true\nmax_count = 2000\nmax_size_mb = 1024\nretention_days = 90";
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

    #[test]
    fn test_empty_toml_has_no_pipes_section() {
        let config = GlassConfig::load_from_str("");
        assert!(config.pipes.is_none());
    }

    #[test]
    fn test_pipes_section_with_no_fields_uses_defaults() {
        let toml = "[pipes]";
        let config = GlassConfig::load_from_str(toml);
        let pipes = config.pipes.expect("pipes section should be Some");
        assert!(pipes.enabled);
        assert_eq!(pipes.max_capture_mb, 10);
        assert!(pipes.auto_expand);
    }

    #[test]
    fn test_pipes_section_partial_fields() {
        let toml = "[pipes]\nenabled = false";
        let config = GlassConfig::load_from_str(toml);
        let pipes = config.pipes.expect("pipes section should be Some");
        assert!(!pipes.enabled);
        assert_eq!(pipes.max_capture_mb, 10); // default
        assert!(pipes.auto_expand); // default
    }

    #[test]
    fn test_pipes_section_all_fields() {
        let toml = "[pipes]\nenabled = false\nmax_capture_mb = 5\nauto_expand = false";
        let config = GlassConfig::load_from_str(toml);
        let pipes = config.pipes.expect("pipes section should be Some");
        assert!(!pipes.enabled);
        assert_eq!(pipes.max_capture_mb, 5);
        assert!(!pipes.auto_expand);
    }

    #[test]
    fn test_soi_section_defaults() {
        let toml = "[soi]";
        let config = GlassConfig::load_from_str(toml);
        let soi = config.soi.expect("soi section should be Some");
        assert!(soi.enabled);
        assert!(!soi.shell_summary);
        assert_eq!(soi.format, "oneline");
        assert_eq!(soi.min_lines, 0);
    }

    #[test]
    fn test_soi_section_roundtrip() {
        let toml = "[soi]\nenabled = false\nshell_summary = true";
        let config = GlassConfig::load_from_str(toml);
        let soi = config.soi.expect("soi section should be Some");
        assert!(!soi.enabled);
        assert!(soi.shell_summary);
        assert_eq!(soi.format, "oneline"); // default
        assert_eq!(soi.min_lines, 0); // default
    }

    #[test]
    fn test_soi_section_absent_uses_defaults() {
        let config = GlassConfig::load_from_str("");
        assert!(config.soi.is_none());
    }

    // === PermissionMatrix / QuietRules / AgentSection extension tests ===

    #[test]
    fn permission_matrix_full_toml() {
        let toml = "[agent.permissions]\nedit_files = \"never\"\nrun_commands = \"auto\"\ngit_operations = \"approve\"";
        let config = GlassConfig::load_from_str(toml);
        let agent = config.agent.expect("agent section should be Some");
        let perms = agent.permissions.expect("permissions should be Some");
        assert_eq!(perms.edit_files, PermissionLevel::Never);
        assert_eq!(perms.run_commands, PermissionLevel::Auto);
        assert_eq!(perms.git_operations, PermissionLevel::Approve);
    }

    #[test]
    fn quiet_rules_full_toml() {
        let toml = "[agent.quiet_rules]\nignore_exit_zero = true\nignore_patterns = [\"cargo check\", \"git status\"]";
        let config = GlassConfig::load_from_str(toml);
        let agent = config.agent.expect("agent section should be Some");
        let qr = agent.quiet_rules.expect("quiet_rules should be Some");
        assert!(qr.ignore_exit_zero);
        assert_eq!(qr.ignore_patterns, vec!["cargo check", "git status"]);
    }

    #[test]
    fn agent_section_no_sub_tables_backward_compat() {
        // [agent] section with only existing fields (no permissions/quiet_rules sub-tables)
        // must still parse successfully with permissions=None and quiet_rules=None.
        let toml = "[agent]\nmax_budget_usd = 2.0";
        let config = GlassConfig::load_from_str(toml);
        let agent = config.agent.expect("agent section should be Some");
        assert!(agent.permissions.is_none());
        assert!(agent.quiet_rules.is_none());
        assert_eq!(agent.max_budget_usd, 2.0);
    }

    #[test]
    fn permission_matrix_partial_fields_uses_approve_default() {
        let toml = "[agent.permissions]\nedit_files = \"never\"";
        let config = GlassConfig::load_from_str(toml);
        let agent = config.agent.expect("agent section should be Some");
        let perms = agent.permissions.expect("permissions should be Some");
        assert_eq!(perms.edit_files, PermissionLevel::Never);
        // Omitted fields should default to Approve
        assert_eq!(perms.run_commands, PermissionLevel::Approve);
        assert_eq!(perms.git_operations, PermissionLevel::Approve);
    }

    #[test]
    fn default_agent_section_has_none_permissions_and_quiet_rules() {
        let agent = AgentSection::default();
        assert!(agent.permissions.is_none());
        assert!(agent.quiet_rules.is_none());
    }

    #[test]
    fn permission_level_serde_snake_case() {
        // approve, auto, never must all round-trip through serde
        let toml = "[agent.permissions]\nedit_files = \"approve\"\nrun_commands = \"auto\"\ngit_operations = \"never\"";
        let config = GlassConfig::load_from_str(toml);
        let agent = config.agent.expect("agent section should be Some");
        let perms = agent.permissions.expect("permissions should be Some");
        assert_eq!(perms.edit_files, PermissionLevel::Approve);
        assert_eq!(perms.run_commands, PermissionLevel::Auto);
        assert_eq!(perms.git_operations, PermissionLevel::Never);
    }
}

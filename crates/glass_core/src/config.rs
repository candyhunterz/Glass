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
    /// Orchestrator sub-section. Optional; None when absent.
    #[serde(default)]
    pub orchestrator: Option<OrchestratorSection>,
}

/// Orchestrator configuration in the `[agent.orchestrator]` TOML section.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OrchestratorSection {
    /// Whether the orchestrator loop is active. Default false.
    #[serde(default)]
    pub enabled: bool,
    /// Seconds of PTY silence before triggering the orchestrator. Default 60.
    #[serde(default = "default_orch_silence_timeout")]
    pub silence_timeout_secs: u64,
    /// Path to the project plan file (relative to CWD). Default "PRD.md".
    #[serde(default = "default_orch_prd_path")]
    pub prd_path: String,
    /// Path to the checkpoint file (relative to CWD). Default ".glass/checkpoint.md".
    #[serde(default = "default_orch_checkpoint_path")]
    pub checkpoint_path: String,
    /// Max identical responses before stuck detection triggers. Default 3.
    #[serde(default = "default_orch_max_retries")]
    pub max_retries_before_stuck: u32,
    /// Seconds after output stops before fast-triggering the orchestrator. Default 5.
    #[serde(default = "default_orch_fast_trigger")]
    pub fast_trigger_secs: u64,
    /// Optional regex pattern to detect the agent's prompt for instant triggering.
    #[serde(default)]
    pub agent_prompt_pattern: Option<String>,
    /// Verification mode: "floor" (default) or "disabled".
    #[serde(default = "default_orch_verify_mode")]
    pub verify_mode: String,
    /// Optional user-override verification command. Overrides auto-detect + agent discovery.
    #[serde(default)]
    pub verify_command: Option<String>,
    /// File path (relative to CWD) that triggers orchestrator when created. Default ".glass/done".
    #[serde(default = "default_orch_completion_artifact")]
    pub completion_artifact: String,
    /// Maximum iterations before checkpoint-stop. None = unlimited.
    #[serde(default)]
    pub max_iterations: Option<u32>,
    /// Orchestrator mode: "build" (default) or "audit".
    /// Build mode: agent has observation-only tools, delegates implementation to Claude Code.
    /// Audit mode: agent gets all MCP tools to test features interactively, delegates code fixes to Claude Code.
    #[serde(default = "default_orch_mode")]
    pub orchestrator_mode: String,
    /// Files to check for file-based verification. Auto-populated from PRD deliverables.
    #[serde(default)]
    pub verify_files: Vec<String>,
    /// Enable LLM-based qualitative analysis after each orchestrator run.
    #[serde(default)]
    pub feedback_llm: bool,
    /// Maximum number of prompt hints (Tier 3) stored per project.
    #[serde(default = "default_max_prompt_hints")]
    pub max_prompt_hints: usize,
}

fn default_orch_silence_timeout() -> u64 {
    60
}
fn default_orch_prd_path() -> String {
    "PRD.md".to_string()
}
fn default_orch_checkpoint_path() -> String {
    ".glass/checkpoint.md".to_string()
}
fn default_orch_max_retries() -> u32 {
    3
}
fn default_orch_fast_trigger() -> u64 {
    5
}
fn default_orch_verify_mode() -> String {
    "floor".to_string()
}
fn default_orch_completion_artifact() -> String {
    ".glass/done".to_string()
}
fn default_orch_mode() -> String {
    "build".to_string()
}
fn default_max_prompt_hints() -> usize {
    10
}

fn default_agent_max_budget_usd() -> f64 {
    1.0
}
fn default_agent_cooldown_secs() -> u64 {
    30
}
fn default_agent_allowed_tools() -> String {
    "glass_query,glass_query_trend,glass_query_drill,glass_context,Bash,Read".to_string()
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
            orchestrator: None,
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

/// Update a single field in a TOML config file.
///
/// If `section` is None, updates a top-level key. If `section` is Some,
/// updates a key within that `[section]`. Creates the section if it doesn't
/// exist. The hot-reload watcher will detect the file change.
pub fn update_config_field(
    path: &std::path::Path,
    section: Option<&str>,
    key: &str,
    value: &str,
) -> Result<(), ConfigError> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    // Use toml::Table deserialization which correctly handles full TOML documents
    // with multiple table headers (e.g., [agent] + [agent.orchestrator]).
    // The previous toml::Value::parse() silently failed on multi-section files
    // and fell through to an empty table, wiping all existing config.
    let mut table: toml::map::Map<String, toml::Value> =
        toml::from_str::<toml::map::Map<String, toml::Value>>(&content).unwrap_or_default();

    // Parse the value string into a TOML value
    let parsed_value: toml::Value = value
        .parse()
        .unwrap_or(toml::Value::String(value.to_string()));

    if let Some(section_name) = section {
        // Traverse dotted section names (e.g., "agent.orchestrator" -> agent -> orchestrator)
        let parts: Vec<&str> = section_name.split('.').collect();
        let mut current = &mut table;
        for part in &parts {
            let entry = current
                .entry(part.to_string())
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
            current = entry.as_table_mut().ok_or_else(|| ConfigError {
                message: format!("Config section '{part}' is not a table"),
                line: None,
                column: None,
                snippet: None,
            })?;
        }
        current.insert(key.to_string(), parsed_value);
    } else {
        table.insert(key.to_string(), parsed_value);
    }

    let output = toml::to_string_pretty(&table).map_err(|e| ConfigError {
        message: format!("Failed to serialize config: {}", e),
        line: None,
        column: None,
        snippet: None,
    })?;

    std::fs::write(path, output).map_err(|e| ConfigError {
        message: format!("Failed to write config: {}", e),
        line: None,
        column: None,
        snippet: None,
    })?;

    Ok(())
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

    // === update_config_field tests ===

    #[test]
    fn test_update_config_field_creates_section() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "font_size = 14.0\n").unwrap();

        update_config_field(&path, None, "font_size", "16.0").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("16.0"));
    }

    #[test]
    fn test_update_config_field_nested_section() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[soi]\nenabled = true\n").unwrap();

        update_config_field(&path, Some("soi"), "enabled", "false").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("enabled = false"));
    }

    #[test]
    fn test_orchestrator_section_defaults() {
        let toml = "[agent]\nmode = \"Autonomous\"\n[agent.orchestrator]\nenabled = true";
        let config = GlassConfig::load_from_str(toml);
        let orch = config
            .agent
            .expect("agent section")
            .orchestrator
            .expect("orchestrator section");
        assert!(orch.enabled);
        assert_eq!(orch.silence_timeout_secs, 60);
        assert_eq!(orch.prd_path, "PRD.md");
        assert_eq!(orch.checkpoint_path, ".glass/checkpoint.md");
        assert_eq!(orch.max_retries_before_stuck, 3);
    }

    #[test]
    fn test_orchestrator_section_absent_is_none() {
        let toml = "[agent]\nmode = \"Autonomous\"";
        let config = GlassConfig::load_from_str(toml);
        assert!(config.agent.unwrap().orchestrator.is_none());
    }

    #[test]
    fn test_orchestrator_section_custom_values() {
        let toml = "[agent.orchestrator]\nenabled = true\nsilence_timeout_secs = 15\nprd_path = \"docs/plan.md\"";
        let config = GlassConfig::load_from_str(toml);
        let orch = config.agent.unwrap().orchestrator.unwrap();
        assert_eq!(orch.silence_timeout_secs, 15);
        assert_eq!(orch.prd_path, "docs/plan.md");
    }

    #[test]
    fn test_orchestrator_section_new_fields_defaults() {
        let toml = "[agent.orchestrator]\nenabled = true";
        let config = GlassConfig::load_from_str(toml);
        let orch = config.agent.unwrap().orchestrator.unwrap();
        assert_eq!(orch.fast_trigger_secs, 5);
        assert!(orch.agent_prompt_pattern.is_none());
    }

    #[test]
    fn test_orchestrator_section_new_fields_custom() {
        let toml = r#"[agent.orchestrator]
enabled = true
fast_trigger_secs = 3
agent_prompt_pattern = "^❯""#;
        let config = GlassConfig::load_from_str(toml);
        let orch = config.agent.unwrap().orchestrator.unwrap();
        assert_eq!(orch.fast_trigger_secs, 3);
        assert_eq!(orch.agent_prompt_pattern.as_deref(), Some("^❯"));
    }

    #[test]
    fn test_orchestrator_v2_fields_defaults() {
        let toml = "[agent.orchestrator]\nenabled = true";
        let config = GlassConfig::load_from_str(toml);
        let orch = config.agent.unwrap().orchestrator.unwrap();
        assert_eq!(orch.verify_mode, "floor");
        assert!(orch.verify_command.is_none());
        assert_eq!(orch.completion_artifact, ".glass/done");
        assert!(orch.max_iterations.is_none());
    }

    #[test]
    fn test_orchestrator_v2_fields_custom() {
        let toml = r#"[agent.orchestrator]
enabled = true
verify_mode = "disabled"
verify_command = "cargo test"
completion_artifact = ".build/complete"
max_iterations = 25"#;
        let config = GlassConfig::load_from_str(toml);
        let orch = config.agent.unwrap().orchestrator.unwrap();
        assert_eq!(orch.verify_mode, "disabled");
        assert_eq!(orch.verify_command.as_deref(), Some("cargo test"));
        assert_eq!(orch.completion_artifact, ".build/complete");
        assert_eq!(orch.max_iterations, Some(25));
    }

    #[test]
    fn test_orchestrator_verify_files_default_empty() {
        let toml = "[agent.orchestrator]\nenabled = true";
        let config = GlassConfig::load_from_str(toml);
        let orch = config.agent.unwrap().orchestrator.unwrap();
        assert!(orch.verify_files.is_empty());
    }

    #[test]
    fn test_orchestrator_verify_files_custom() {
        let toml = r#"[agent.orchestrator]
enabled = true
verify_files = ["plan.md", "site/index.html"]"#;
        let config = GlassConfig::load_from_str(toml);
        let orch = config.agent.unwrap().orchestrator.unwrap();
        assert_eq!(orch.verify_files, vec!["plan.md", "site/index.html"]);
    }

    #[test]
    fn test_update_config_field_dotted_section() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[agent.orchestrator]\nenabled = true\n").unwrap();

        update_config_field(
            &path,
            Some("agent.orchestrator"),
            "silence_timeout_secs",
            "15",
        )
        .unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let config = GlassConfig::load_from_str(&content);
        let orch = config.agent.unwrap().orchestrator.unwrap();
        assert_eq!(orch.silence_timeout_secs, 15);
    }

    #[test]
    fn test_update_parent_preserves_child_section() {
        // Bug: writing to [agent].mode was wiping [agent.orchestrator]
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[agent]\nmode = \"Assist\"\n\n[agent.orchestrator]\nsilence_timeout_secs = 30\nprd_path = \"PRD.md\"\nverify_mode = \"floor\"\n",
        )
        .unwrap();

        // Update parent section field
        update_config_field(&path, Some("agent"), "mode", "\"Off\"").unwrap();

        // Child section must survive
        let content = std::fs::read_to_string(&path).unwrap();
        let config = GlassConfig::load_from_str(&content);
        let agent = config.agent.expect("agent section must exist");
        let orch = agent
            .orchestrator
            .expect("orchestrator must survive parent update");
        assert_eq!(orch.silence_timeout_secs, 30);
        assert_eq!(orch.prd_path, "PRD.md");
        assert_eq!(orch.verify_mode, "floor");
    }

    // === Audit: update_config_field covers all settings handler section paths ===

    #[test]
    fn test_update_config_field_empty_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        // File doesn't exist yet
        update_config_field(&path, None, "font_size", "18.0").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let config = GlassConfig::load_from_str(&content);
        assert_eq!(config.font_size, 18.0);
    }

    #[test]
    fn test_update_config_field_agent_section() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        update_config_field(&path, Some("agent"), "cooldown_secs", "15").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let config = GlassConfig::load_from_str(&content);
        assert_eq!(config.agent.unwrap().cooldown_secs, 15);
    }

    #[test]
    fn test_update_config_field_agent_permissions() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        update_config_field(&path, Some("agent.permissions"), "edit_files", "\"never\"").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let config = GlassConfig::load_from_str(&content);
        let perms = config.agent.unwrap().permissions.unwrap();
        assert_eq!(perms.edit_files, PermissionLevel::Never);
    }

    #[test]
    fn test_update_config_field_agent_quiet_rules() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        update_config_field(&path, Some("agent.quiet_rules"), "ignore_exit_zero", "true").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let config = GlassConfig::load_from_str(&content);
        let qr = config.agent.unwrap().quiet_rules.unwrap();
        assert!(qr.ignore_exit_zero);
    }

    #[test]
    fn test_update_config_field_history_section() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        update_config_field(&path, Some("history"), "max_output_capture_kb", "100").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let config = GlassConfig::load_from_str(&content);
        assert_eq!(config.history.unwrap().max_output_capture_kb, 100);
    }

    #[test]
    fn test_update_config_field_snapshot_section() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        update_config_field(&path, Some("snapshot"), "retention_days", "7").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let config = GlassConfig::load_from_str(&content);
        assert_eq!(config.snapshot.unwrap().retention_days, 7);
    }

    #[test]
    fn test_update_config_field_pipes_section() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        update_config_field(&path, Some("pipes"), "max_capture_mb", "5").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let config = GlassConfig::load_from_str(&content);
        assert_eq!(config.pipes.unwrap().max_capture_mb, 5);
    }

    // === Audit: config round-trip (write → read back → values match) ===

    #[test]
    fn test_roundtrip_all_sections() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();

        // Write fields across multiple sections
        update_config_field(&path, None, "font_size", "20.0").unwrap();
        update_config_field(&path, Some("soi"), "enabled", "false").unwrap();
        update_config_field(&path, Some("soi"), "min_lines", "5").unwrap();
        update_config_field(&path, Some("snapshot"), "max_count", "500").unwrap();
        update_config_field(&path, Some("pipes"), "auto_expand", "false").unwrap();
        update_config_field(&path, Some("history"), "max_output_capture_kb", "200").unwrap();
        update_config_field(&path, Some("agent"), "cooldown_secs", "10").unwrap();
        update_config_field(
            &path,
            Some("agent.orchestrator"),
            "silence_timeout_secs",
            "15",
        )
        .unwrap();

        // Read back and verify all values
        let content = std::fs::read_to_string(&path).unwrap();
        let config = GlassConfig::load_from_str(&content);
        assert_eq!(config.font_size, 20.0);
        let soi = config.soi.unwrap();
        assert!(!soi.enabled);
        assert_eq!(soi.min_lines, 5);
        assert_eq!(config.snapshot.unwrap().max_count, 500);
        assert!(!config.pipes.unwrap().auto_expand);
        assert_eq!(config.history.unwrap().max_output_capture_kb, 200);
        assert_eq!(config.agent.as_ref().unwrap().cooldown_secs, 10);
        assert_eq!(
            config
                .agent
                .unwrap()
                .orchestrator
                .unwrap()
                .silence_timeout_secs,
            15
        );
    }

    // === Audit: serde defaults consistency ===

    #[test]
    fn test_agent_section_serde_defaults_match_default_impl() {
        // Parsing "[agent]" with no fields should produce the same as AgentSection::default()
        let toml = "[agent]";
        let config = GlassConfig::load_from_str(toml);
        let from_serde = config.agent.unwrap();
        let from_default = AgentSection::default();
        assert_eq!(from_serde, from_default);
    }

    #[test]
    fn test_history_section_defaults() {
        let toml = "[history]";
        let config = GlassConfig::load_from_str(toml);
        let history = config.history.unwrap();
        assert_eq!(history.max_output_capture_kb, 50);
    }

    #[test]
    fn test_history_section_custom() {
        let toml = "[history]\nmax_output_capture_kb = 200";
        let config = GlassConfig::load_from_str(toml);
        let history = config.history.unwrap();
        assert_eq!(history.max_output_capture_kb, 200);
    }
}

#[cfg(test)]
mod agent_spawn_test {
    use super::*;

    #[test]
    fn test_user_config_parses_for_orchestrator() {
        let toml = r#"[agent]
mode = "Assist"

[agent.orchestrator]
silence_timeout_secs = 30
prd_path = "PRD-full-audit.md"
verify_mode = "floor"
max_iterations = 100
completion_artifact = ".glass/done"
orchestrator_mode = "audit"
"#;
        let config = GlassConfig::load_from_str(toml);
        let agent = config.agent.as_ref().expect("agent section must exist");

        // Simulate respawn_orchestrator_agent: clone and set enabled=true
        let mut agent_clone = agent.clone();
        if let Some(ref mut orch) = agent_clone.orchestrator {
            orch.enabled = true;
        }

        let orch = agent_clone
            .orchestrator
            .as_ref()
            .expect("orchestrator must exist");
        assert!(orch.enabled, "enabled must be true after override");
        assert_eq!(orch.orchestrator_mode, "audit");
        assert_eq!(orch.prd_path, "PRD-full-audit.md");

        // Build AgentRuntimeConfig
        let runtime_config = crate::agent_runtime::AgentRuntimeConfig {
            mode: agent_clone.mode,
            max_budget_usd: agent_clone.max_budget_usd,
            cooldown_secs: agent_clone.cooldown_secs,
            allowed_tools: agent_clone.allowed_tools.clone(),
            orchestrator: agent_clone.orchestrator.clone(),
        };

        // Build args
        let args = crate::agent_runtime::build_agent_command_args(
            &runtime_config,
            "/tmp/prompt.txt",
            "/tmp/mcp.json",
        );

        let args_str = args.join(" ");
        eprintln!("ARGS: {}", args_str);

        // Check tools include MCP tools (audit mode)
        assert!(
            args_str.contains("glass_history"),
            "audit mode must include glass_history"
        );
        assert!(
            args_str.contains("glass_tab_create"),
            "audit mode must include glass_tab_create"
        );
        assert!(args_str.contains("Read"), "audit mode must include Read");
        assert!(
            !args_str.contains("Bash"),
            "audit mode must NOT include Bash"
        );
        assert!(
            args_str.contains("--disable-slash-commands"),
            "must disable slash commands"
        );
    }
}

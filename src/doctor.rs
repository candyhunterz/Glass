use anyhow::Result;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
    Skip,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub category: String,
    pub name: String,
    pub status: CheckStatus,
    pub message: String,
    pub fix: Option<String>,
}

impl CheckResult {
    fn with_fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub pass: usize,
    pub warn: usize,
    pub fail: usize,
    pub skip: usize,
    pub info: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub version: String,
    pub checks: Vec<CheckResult>,
    pub summary: Summary,
}

impl DoctorReport {
    pub fn from_checks(checks: Vec<CheckResult>) -> Self {
        let summary = Summary {
            pass: checks
                .iter()
                .filter(|c| c.status == CheckStatus::Pass)
                .count(),
            warn: checks
                .iter()
                .filter(|c| c.status == CheckStatus::Warn)
                .count(),
            fail: checks
                .iter()
                .filter(|c| c.status == CheckStatus::Fail)
                .count(),
            skip: checks
                .iter()
                .filter(|c| c.status == CheckStatus::Skip)
                .count(),
            info: checks
                .iter()
                .filter(|c| c.status == CheckStatus::Info)
                .count(),
        };
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            checks,
            summary,
        }
    }

    pub fn has_failures(&self) -> bool {
        self.summary.fail > 0
    }

    pub fn format_terminal(&self) -> String {
        let mut out = format!("Glass Doctor v{}\n\n", self.version);
        let mut current_cat = String::new();
        for check in &self.checks {
            if check.category != current_cat {
                if !current_cat.is_empty() {
                    out.push('\n');
                }
                out.push_str(category_display_name(&check.category));
                out.push('\n');
                current_cat.clone_from(&check.category);
            }
            let icon = match check.status {
                CheckStatus::Pass => "\u{2713}",
                CheckStatus::Warn => "\u{26A0}",
                CheckStatus::Fail => "\u{2717}",
                CheckStatus::Skip => "\u{25CB}",
                CheckStatus::Info => "\u{00B7}",
            };
            out.push_str(&format!(
                "  {} {:<17} {}\n",
                icon, check.name, check.message
            ));
            if let Some(ref fix) = check.fix {
                if matches!(check.status, CheckStatus::Warn | CheckStatus::Fail) {
                    out.push_str(&format!("    Fix: {}\n", fix));
                }
            }
        }
        out.push('\n');
        out.push_str(&format!(
            "{} passed, {} warnings, {} failed, {} skipped, {} info\n",
            self.summary.pass,
            self.summary.warn,
            self.summary.fail,
            self.summary.skip,
            self.summary.info,
        ));
        out
    }

    pub fn format_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Category {
    System,
    Gpu,
    Shell,
    Config,
    Data,
    Agent,
    Git,
}

pub struct DoctorOptions {
    pub json: bool,
    pub fix: bool,
    pub category: Option<Category>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn cr(cat: &str, name: &str, status: CheckStatus, msg: impl Into<String>) -> CheckResult {
    CheckResult {
        category: cat.to_string(),
        name: name.to_string(),
        status,
        message: msg.into(),
        fix: None,
    }
}

fn category_display_name(tag: &str) -> &str {
    match tag {
        "system" => "System",
        "gpu" => "GPU",
        "shell" => "Shell Integration",
        "config" => "Configuration",
        "data" => "Data Stores",
        "agent" => "Agent & Orchestrator",
        "git" => "Git",
        _ => tag,
    }
}

fn should_run(filter: &Option<Category>, target: Category) -> bool {
    filter.is_none_or(|f| f == target)
}

fn find_local_glass_dir() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok();
    while let Some(d) = dir {
        let glass = d.join(".glass");
        if glass.is_dir() {
            return Some(glass);
        }
        dir = d.parent().map(|p| p.to_path_buf());
    }
    None
}

fn global_glass_dir() -> Option<PathBuf> {
    dirs::home_dir()
        .map(|h| h.join(".glass"))
        .filter(|p| p.is_dir())
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(meta) = std::fs::symlink_metadata(entry.path()) {
                if meta.is_dir() {
                    total += dir_size(&entry.path());
                } else if meta.is_file() {
                    total += meta.len();
                }
            }
        }
    }
    total
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{} KB", bytes / 1024)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{} MB", bytes / (1024 * 1024))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn run_command(cmd: &str, args: &[&str]) -> Option<String> {
    let mut command = std::process::Command::new(cmd);
    command.args(args);
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    match command.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !stdout.is_empty() {
                Some(stdout)
            } else if !stderr.is_empty() {
                Some(stderr)
            } else if output.status.success() {
                Some(String::new())
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

fn detect_shell() -> Option<String> {
    std::env::var("SHELL")
        .or_else(|_| std::env::var("COMSPEC"))
        .ok()
}

fn shell_name(path: &str) -> &str {
    let lower = path.to_lowercase();
    if lower.contains("bash") {
        "Bash"
    } else if lower.contains("zsh") {
        "Zsh"
    } else if lower.contains("fish") {
        "Fish"
    } else if lower.contains("pwsh") || lower.contains("powershell") {
        "PowerShell"
    } else {
        "Unknown"
    }
}

fn shell_script(name: &str) -> Option<&str> {
    match name {
        "Bash" => Some("glass.bash"),
        "Zsh" => Some("glass.zsh"),
        "Fish" => Some("glass.fish"),
        "PowerShell" => Some("glass.ps1"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// System Checks
// ---------------------------------------------------------------------------

fn check_version() -> CheckResult {
    cr(
        "system",
        "version",
        CheckStatus::Pass,
        format!("Glass v{}", env!("CARGO_PKG_VERSION")),
    )
}

fn check_platform() -> CheckResult {
    cr(
        "system",
        "platform",
        CheckStatus::Pass,
        format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
    )
}

fn check_shell_detect(shell: &Option<String>) -> CheckResult {
    match shell {
        Some(s) => {
            let name = shell_name(s);
            cr(
                "system",
                "shell",
                CheckStatus::Pass,
                format!("{} ({})", name, s),
            )
        }
        None => cr("system", "shell", CheckStatus::Warn, "No shell detected")
            .with_fix("Set $SHELL (Unix) or $COMSPEC (Windows) environment variable"),
    }
}

// ---------------------------------------------------------------------------
// GPU Checks
// ---------------------------------------------------------------------------

fn check_gpu() -> Vec<CheckResult> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        #[cfg(target_os = "windows")]
        backends: wgpu::Backends::DX12 | wgpu::Backends::VULKAN,
        #[cfg(not(target_os = "windows"))]
        backends: wgpu::Backends::all(),
        ..Default::default()
    });

    let adapters: Vec<wgpu::Adapter> =
        pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));

    if adapters.is_empty() {
        return vec![
            cr(
                "gpu",
                "gpu_adapter",
                CheckStatus::Fail,
                "No GPU adapters found",
            )
            .with_fix(
                "Install GPU drivers. Glass requires DX12 (Windows), Metal (macOS), or Vulkan (Linux).",
            ),
            cr("gpu", "gpu_backend", CheckStatus::Skip, "No adapters to check"),
        ];
    }

    let primary = adapters[0].get_info();
    let adapter_result = cr(
        "gpu",
        "gpu_adapter",
        CheckStatus::Pass,
        format!(
            "{} ({:?}, {:?})",
            primary.name, primary.backend, primary.device_type
        ),
    );

    let expected_name = if cfg!(target_os = "windows") {
        "DX12"
    } else if cfg!(target_os = "macos") {
        "Metal"
    } else {
        "Vulkan"
    };

    let expected_backend = if cfg!(target_os = "windows") {
        wgpu::Backend::Dx12
    } else if cfg!(target_os = "macos") {
        wgpu::Backend::Metal
    } else {
        wgpu::Backend::Vulkan
    };

    let all_cpu = adapters
        .iter()
        .all(|a| a.get_info().device_type == wgpu::DeviceType::Cpu);
    let has_expected = adapters
        .iter()
        .any(|a| a.get_info().backend == expected_backend);

    let backend_result = if all_cpu {
        cr(
            "gpu",
            "gpu_backend",
            CheckStatus::Warn,
            "Only software/CPU adapters found — performance will be poor",
        )
        .with_fix("Install proper GPU drivers")
    } else if has_expected {
        cr(
            "gpu",
            "gpu_backend",
            CheckStatus::Pass,
            format!(
                "{} available (preferred for {})",
                expected_name,
                std::env::consts::OS
            ),
        )
    } else {
        cr(
            "gpu",
            "gpu_backend",
            CheckStatus::Warn,
            format!("{} not found among available adapters", expected_name),
        )
    };

    vec![adapter_result, backend_result]
}

// ---------------------------------------------------------------------------
// Shell Integration Checks
// ---------------------------------------------------------------------------

fn check_osc_support(shell: &Option<String>) -> CheckResult {
    let name = shell.as_deref().map(shell_name).unwrap_or("Unknown");
    match shell_script(name) {
        Some(script) => cr(
            "shell",
            "osc_support",
            CheckStatus::Pass,
            format!("{} \u{2014} full shell integration ({})", name, script),
        ),
        None => cr(
            "shell",
            "osc_support",
            CheckStatus::Warn,
            format!(
                "{} \u{2014} shell integration not available. Command detection, undo, and pipe visualization won't function.",
                name
            ),
        )
        .with_fix("Use bash, zsh, fish, or PowerShell for full Glass features"),
    }
}

// ---------------------------------------------------------------------------
// Config Checks
// ---------------------------------------------------------------------------

struct ConfigState {
    path: Option<PathBuf>,
    exists: bool,
    raw_text: Option<String>,
    parsed: Option<glass_core::config::GlassConfig>,
    parse_error: Option<String>,
}

fn gather_config() -> ConfigState {
    let path = glass_core::config::GlassConfig::config_path();
    let exists = path.as_ref().is_some_and(|p| p.exists());
    if !exists {
        return ConfigState {
            path,
            exists,
            raw_text: None,
            parsed: None,
            parse_error: None,
        };
    }
    let raw_text = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok());
    let (parsed, parse_error) = match &raw_text {
        Some(text) => match glass_core::config::GlassConfig::load_validated(text) {
            Ok(config) => (Some(config), None),
            Err(e) => (None, Some(e.to_string())),
        },
        None => (None, Some("Could not read config file".to_string())),
    };
    ConfigState {
        path,
        exists,
        raw_text,
        parsed,
        parse_error,
    }
}

fn check_config_exists_at(path: &Option<PathBuf>) -> CheckResult {
    match path {
        Some(p) if p.exists() => cr(
            "config",
            "config_exists",
            CheckStatus::Pass,
            format!("{} found", p.display()),
        ),
        Some(p) => cr(
            "config",
            "config_exists",
            CheckStatus::Warn,
            format!("{} not found \u{2014} using defaults", p.display()),
        )
        .with_fix("Run `glass doctor --fix` to create default config"),
        None => cr(
            "config",
            "config_exists",
            CheckStatus::Warn,
            "Cannot determine config path (no home directory)",
        )
        .with_fix("Set HOME environment variable"),
    }
}

fn check_config_parse(state: &ConfigState) -> CheckResult {
    if !state.exists {
        return cr(
            "config",
            "config_parse",
            CheckStatus::Skip,
            "No config file to parse",
        );
    }
    if state.raw_text.is_none() {
        return cr(
            "config",
            "config_parse",
            CheckStatus::Fail,
            "Could not read config file",
        );
    }
    if state.parsed.is_some() {
        let raw = state.raw_text.as_ref().unwrap();
        let key_count = raw
            .lines()
            .filter(|l| {
                let t = l.trim();
                !t.is_empty() && !t.starts_with('#') && !t.starts_with('[') && t.contains('=')
            })
            .count();
        cr(
            "config",
            "config_parse",
            CheckStatus::Pass,
            format!("Valid TOML, {} keys set", key_count),
        )
    } else {
        let err_msg = state.parse_error.as_deref().unwrap_or("unknown error");
        cr(
            "config",
            "config_parse",
            CheckStatus::Fail,
            format!("Parse error: {}", err_msg),
        )
        .with_fix("Fix the TOML syntax in your config file")
    }
}

fn check_config_font(state: &ConfigState) -> CheckResult {
    if !state.exists {
        return cr("config", "config_font", CheckStatus::Skip, "No config file");
    }
    let explicitly_set = state.raw_text.as_ref().is_some_and(|text| {
        text.lines().any(|l| {
            let t = l.trim();
            !t.starts_with('#') && t.starts_with("font_family")
        })
    });
    if !explicitly_set {
        return cr(
            "config",
            "config_font",
            CheckStatus::Skip,
            "Using default font",
        );
    }
    let font = state
        .parsed
        .as_ref()
        .map(|c| c.font_family.as_str())
        .unwrap_or("unknown");
    cr(
        "config",
        "config_font",
        CheckStatus::Info,
        format!(
            "Font: \"{}\" (runtime-resolved, cannot validate here)",
            font
        ),
    )
}

fn check_config_shell(state: &ConfigState) -> CheckResult {
    if !state.exists {
        return cr(
            "config",
            "config_shell",
            CheckStatus::Skip,
            "No config file",
        );
    }
    let config = match &state.parsed {
        Some(c) => c,
        None => {
            return cr(
                "config",
                "config_shell",
                CheckStatus::Skip,
                "Config could not be parsed",
            )
        }
    };
    match &config.shell {
        None => cr(
            "config",
            "config_shell",
            CheckStatus::Skip,
            "Shell not set in config (using auto-detection)",
        ),
        Some(shell_path) => {
            if Path::new(shell_path).exists() {
                cr(
                    "config",
                    "config_shell",
                    CheckStatus::Pass,
                    format!("{} exists", shell_path),
                )
            } else {
                cr(
                    "config",
                    "config_shell",
                    CheckStatus::Fail,
                    format!("{} does not exist", shell_path),
                )
                .with_fix(
                    "Check the path or remove the `shell` line from config to use auto-detection",
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Data Checks
// ---------------------------------------------------------------------------

fn check_history_db(local: &Option<PathBuf>, global: &Option<PathBuf>) -> CheckResult {
    let (db_path, label) = {
        let local_db = local
            .as_ref()
            .map(|d| d.join("history.db"))
            .filter(|p| p.exists());
        let global_db = global
            .as_ref()
            .map(|d| d.join("history.db"))
            .filter(|p| p.exists());
        if let Some(p) = local_db {
            (p, "local project")
        } else if let Some(p) = global_db {
            (p, "global")
        } else {
            return cr(
                "data",
                "history_db",
                CheckStatus::Skip,
                "No history database found",
            );
        }
    };

    match rusqlite::Connection::open(&db_path) {
        Ok(conn) => {
            match conn.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0)) {
                Ok(ref result) if result == "ok" => {
                    match conn.query_row("SELECT count(*) FROM commands", [], |row| {
                        row.get::<_, i64>(0)
                    }) {
                        Ok(count) => cr(
                            "data",
                            "history_db",
                            CheckStatus::Pass,
                            format!(
                                ".glass/history.db \u{2014} OK, {} commands ({})",
                                count, label
                            ),
                        ),
                        Err(e) => cr(
                            "data",
                            "history_db",
                            CheckStatus::Fail,
                            format!("Could not query commands: {}", e),
                        ),
                    }
                }
                Ok(result) => cr(
                    "data",
                    "history_db",
                    CheckStatus::Fail,
                    format!("Database corruption detected: {}", result),
                )
                .with_fix("Delete the database and let Glass recreate it"),
                Err(e) => cr(
                    "data",
                    "history_db",
                    CheckStatus::Fail,
                    format!("Integrity check failed: {}", e),
                ),
            }
        }
        Err(e) => cr(
            "data",
            "history_db",
            CheckStatus::Fail,
            format!("Could not open database: {}", e),
        ),
    }
}

fn check_history_fts(local: &Option<PathBuf>, global: &Option<PathBuf>) -> CheckResult {
    let db_path = local
        .as_ref()
        .map(|d| d.join("history.db"))
        .filter(|p| p.exists())
        .or_else(|| {
            global
                .as_ref()
                .map(|d| d.join("history.db"))
                .filter(|p| p.exists())
        });

    let db_path = match db_path {
        Some(p) => p,
        None => {
            return cr(
                "data",
                "history_fts",
                CheckStatus::Skip,
                "No history database",
            )
        }
    };

    match rusqlite::Connection::open(&db_path) {
        Ok(conn) => {
            match conn.query_row("SELECT count(*) FROM commands_fts", [], |row| {
                row.get::<_, i64>(0)
            }) {
                Ok(count) => cr(
                    "data",
                    "history_fts",
                    CheckStatus::Pass,
                    format!("FTS5 index operational ({} entries)", count),
                ),
                Err(e) => cr(
                    "data",
                    "history_fts",
                    CheckStatus::Fail,
                    format!("FTS5 virtual table error: {}", e),
                ),
            }
        }
        Err(e) => cr(
            "data",
            "history_fts",
            CheckStatus::Fail,
            format!("Could not open database: {}", e),
        ),
    }
}

fn check_snapshot_db(local: &Path) -> CheckResult {
    let db_path = local.join("snapshots.db");
    if !db_path.exists() {
        return cr(
            "data",
            "snapshot_db",
            CheckStatus::Skip,
            "No snapshot database",
        );
    }
    match rusqlite::Connection::open(&db_path) {
        Ok(conn) => {
            match conn.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0)) {
                Ok(ref result) if result == "ok" => cr(
                    "data",
                    "snapshot_db",
                    CheckStatus::Pass,
                    ".glass/snapshots.db \u{2014} OK",
                ),
                Ok(result) => cr(
                    "data",
                    "snapshot_db",
                    CheckStatus::Fail,
                    format!("Database corruption: {}", result),
                )
                .with_fix("Delete the snapshot database and let Glass recreate it"),
                Err(e) => cr(
                    "data",
                    "snapshot_db",
                    CheckStatus::Fail,
                    format!("Integrity check failed: {}", e),
                ),
            }
        }
        Err(e) => cr(
            "data",
            "snapshot_db",
            CheckStatus::Fail,
            format!("Could not open database: {}", e),
        ),
    }
}

fn check_snapshot_blobs(local: &Path) -> CheckResult {
    let blobs_dir = local.join("blobs");
    if !blobs_dir.is_dir() {
        return cr(
            "data",
            "snapshot_blobs",
            CheckStatus::Skip,
            "No blob store directory",
        );
    }
    match std::fs::read_dir(&blobs_dir) {
        Ok(entries) => {
            let count = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.metadata().is_ok_and(|m| m.is_file()))
                .count();
            cr(
                "data",
                "snapshot_blobs",
                CheckStatus::Pass,
                format!(".glass/blobs/ \u{2014} {} blobs", count),
            )
        }
        Err(e) => cr(
            "data",
            "snapshot_blobs",
            CheckStatus::Fail,
            format!("Could not read blobs directory: {}", e),
        ),
    }
}

fn check_data_size(local: Option<&Path>, global: Option<&Path>) -> CheckResult {
    let local_size = local.map(dir_size).unwrap_or(0);
    let global_size = global.map(dir_size).unwrap_or(0);

    let warn_threshold = 500 * 1024 * 1024; // 500 MB
    let status = if local_size > warn_threshold || global_size > warn_threshold {
        CheckStatus::Warn
    } else {
        CheckStatus::Pass
    };

    let msg = match (local, global) {
        (Some(_), Some(_)) => format!(
            "Local: {}, Global: {}",
            format_size(local_size),
            format_size(global_size)
        ),
        (Some(_), None) => format!("Local: {}", format_size(local_size)),
        (None, Some(_)) => format!("Global: {}", format_size(global_size)),
        (None, None) => "No data directories".to_string(),
    };

    let mut result = cr("data", "data_size", status, msg);
    if local_size > warn_threshold || global_size > warn_threshold {
        result = result.with_fix("Consider running `glass history prune` to reduce database size");
    }
    result
}

// ---------------------------------------------------------------------------
// Agent Checks
// ---------------------------------------------------------------------------

fn check_agent_config(config: &Option<glass_core::config::GlassConfig>) -> CheckResult {
    match config {
        Some(cfg) => match &cfg.agent {
            Some(agent) => {
                let mode = format!("{:?}", agent.mode).to_lowercase();
                cr(
                    "agent",
                    "agent_config",
                    CheckStatus::Info,
                    format!("Mode: {}", mode),
                )
            }
            None => cr(
                "agent",
                "agent_config",
                CheckStatus::Info,
                "Not configured \u{2014} using defaults",
            ),
        },
        None => cr(
            "agent",
            "agent_config",
            CheckStatus::Info,
            "Not configured \u{2014} using defaults",
        ),
    }
}

fn check_agent_provider(config: &Option<glass_core::config::GlassConfig>) -> CheckResult {
    let provider = config
        .as_ref()
        .and_then(|c| c.agent.as_ref())
        .map(|a| a.provider.as_str())
        .unwrap_or("claude-code");

    let provider = if provider.is_empty() {
        "claude-code"
    } else {
        provider
    };

    match provider {
        "claude-code" => match run_command("claude", &["--version"]) {
            Some(version) => {
                let v = version.lines().next().unwrap_or(&version);
                cr(
                    "agent",
                    "agent_provider",
                    CheckStatus::Pass,
                    format!("claude CLI {} (on PATH)", v),
                )
            }
            None => cr(
                "agent",
                "agent_provider",
                CheckStatus::Fail,
                "claude CLI not found on PATH",
            )
            .with_fix("Install Claude Code CLI: https://docs.anthropic.com/en/docs/claude-code"),
        },
        "anthropic-api" => {
            if std::env::var("ANTHROPIC_API_KEY").is_ok() {
                cr(
                    "agent",
                    "agent_provider",
                    CheckStatus::Pass,
                    "ANTHROPIC_API_KEY is set",
                )
            } else {
                cr(
                    "agent",
                    "agent_provider",
                    CheckStatus::Fail,
                    "ANTHROPIC_API_KEY not set",
                )
                .with_fix("Set the ANTHROPIC_API_KEY environment variable")
            }
        }
        "openai-api" => {
            if std::env::var("OPENAI_API_KEY").is_ok() {
                cr(
                    "agent",
                    "agent_provider",
                    CheckStatus::Pass,
                    "OPENAI_API_KEY is set",
                )
            } else {
                cr(
                    "agent",
                    "agent_provider",
                    CheckStatus::Fail,
                    "OPENAI_API_KEY not set",
                )
                .with_fix("Set the OPENAI_API_KEY environment variable")
            }
        }
        "ollama" => match run_command("ollama", &["--version"]) {
            Some(version) => {
                let v = version.lines().next().unwrap_or(&version);
                cr(
                    "agent",
                    "agent_provider",
                    CheckStatus::Pass,
                    format!("ollama {} (on PATH)", v),
                )
            }
            None => cr(
                "agent",
                "agent_provider",
                CheckStatus::Fail,
                "ollama not found on PATH",
            )
            .with_fix("Install Ollama: https://ollama.ai"),
        },
        other => cr(
            "agent",
            "agent_provider",
            CheckStatus::Warn,
            format!("Unknown provider: {}", other),
        ),
    }
}

fn check_mcp_registration() -> CheckResult {
    let results = crate::mcp_register::auto_register();
    let registered: Vec<&str> = results
        .iter()
        .filter(|r| {
            matches!(
                r.action,
                crate::mcp_register::RegistrationAction::Registered
                    | crate::mcp_register::RegistrationAction::AlreadyExists
            )
        })
        .map(|r| r.tool)
        .collect();

    if registered.is_empty() {
        cr(
            "agent",
            "mcp_registration",
            CheckStatus::Warn,
            "Not registered with any AI tools",
        )
        .with_fix(
            "Glass MCP tools won't be available to AI assistants. Install Claude Code, Cursor, or Windsurf.",
        )
    } else {
        cr(
            "agent",
            "mcp_registration",
            CheckStatus::Pass,
            format!("Registered in: {}", registered.join(", ")),
        )
    }
}

fn check_coordination_db() -> CheckResult {
    let db_path = dirs::home_dir().map(|h| h.join(".glass").join("agents.db"));

    match db_path {
        Some(p) if p.exists() => match rusqlite::Connection::open(&p) {
            Ok(conn) => match conn.query_row("SELECT 1", [], |row| row.get::<_, i32>(0)) {
                Ok(_) => cr(
                    "agent",
                    "coordination_db",
                    CheckStatus::Pass,
                    "agents.db \u{2014} accessible",
                ),
                Err(e) => cr(
                    "agent",
                    "coordination_db",
                    CheckStatus::Fail,
                    format!("agents.db query failed: {}", e),
                ),
            },
            Err(e) => cr(
                "agent",
                "coordination_db",
                CheckStatus::Fail,
                format!("Could not open agents.db: {}", e),
            ),
        },
        _ => cr(
            "agent",
            "coordination_db",
            CheckStatus::Skip,
            "Not created yet \u{2014} created on first multi-agent use",
        ),
    }
}

// ---------------------------------------------------------------------------
// Git Checks
// ---------------------------------------------------------------------------

fn check_git_available() -> CheckResult {
    match run_command("git", &["--version"]) {
        Some(version) => {
            let v = version.trim_start_matches("git version ").trim();
            cr(
                "git",
                "git_available",
                CheckStatus::Pass,
                format!("git {}", v),
            )
        }
        None => cr(
            "git",
            "git_available",
            CheckStatus::Fail,
            "git not found on PATH",
        )
        .with_fix("Install git: https://git-scm.com/downloads"),
    }
}

fn check_git_repo() -> CheckResult {
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(_) => {
            return cr(
                "git",
                "git_repo",
                CheckStatus::Skip,
                "Cannot determine current directory",
            )
        }
    };

    match glass_terminal::status::query_git_status(&cwd.to_string_lossy()) {
        Some(info) => {
            let dirty_msg = if info.dirty_count > 0 {
                format!(" ({} dirty files)", info.dirty_count)
            } else {
                String::new()
            };
            cr(
                "git",
                "git_repo",
                CheckStatus::Pass,
                format!("On branch {}{}", info.branch, dirty_msg),
            )
        }
        None => cr("git", "git_repo", CheckStatus::Warn, "Not a git repository")
            .with_fix("Run `git init` to enable full Glass features"),
    }
}

// ---------------------------------------------------------------------------
// Auto-fix
// ---------------------------------------------------------------------------

fn apply_fixes(checks: &mut [CheckResult]) {
    for check in checks.iter_mut() {
        if check.name == "config_exists" && check.status == CheckStatus::Warn {
            glass_core::config::GlassConfig::ensure_default_config();
            let new = check_config_exists_at(&glass_core::config::GlassConfig::config_path());
            if new.status == CheckStatus::Pass {
                check.status = CheckStatus::Pass;
                check.message = format!("{} (created by --fix)", new.message);
                check.fix = None;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Entry Point
// ---------------------------------------------------------------------------

pub fn run_doctor(options: DoctorOptions) -> Result<()> {
    let mut checks = Vec::new();

    let detected_shell = detect_shell();
    let config_state = gather_config();
    let local_glass = find_local_glass_dir();
    let global_glass = global_glass_dir();

    // 1. System
    if should_run(&options.category, Category::System) {
        checks.push(check_version());
        checks.push(check_platform());
        checks.push(check_shell_detect(&detected_shell));
    }

    // 2. GPU
    if should_run(&options.category, Category::Gpu) {
        checks.extend(check_gpu());
    }

    // 3. Shell Integration
    if should_run(&options.category, Category::Shell) {
        checks.push(check_osc_support(&detected_shell));
    }

    // 4. Config
    if should_run(&options.category, Category::Config) {
        checks.push(check_config_exists_at(&config_state.path));
        checks.push(check_config_parse(&config_state));
        checks.push(check_config_font(&config_state));
        checks.push(check_config_shell(&config_state));
    }

    // 5. Data
    if should_run(&options.category, Category::Data) {
        if local_glass.is_none() && global_glass.is_none() {
            checks.push(cr(
                "data",
                "data",
                CheckStatus::Skip,
                "No .glass/ directory found \u{2014} run Glass in a project first",
            ));
        } else {
            checks.push(check_history_db(&local_glass, &global_glass));
            checks.push(check_history_fts(&local_glass, &global_glass));

            if let Some(ref local) = local_glass {
                checks.push(check_snapshot_db(local));
                checks.push(check_snapshot_blobs(local));
            } else {
                checks.push(cr(
                    "data",
                    "snapshot_db",
                    CheckStatus::Skip,
                    "No local .glass/ directory",
                ));
                checks.push(cr(
                    "data",
                    "snapshot_blobs",
                    CheckStatus::Skip,
                    "No local .glass/ directory",
                ));
            }

            checks.push(check_data_size(
                local_glass.as_deref(),
                global_glass.as_deref(),
            ));
        }
    }

    // 6. Agent
    if should_run(&options.category, Category::Agent) {
        checks.push(check_agent_config(&config_state.parsed));
        checks.push(check_agent_provider(&config_state.parsed));
        checks.push(check_mcp_registration());
        checks.push(check_coordination_db());
    }

    // 7. Git
    if should_run(&options.category, Category::Git) {
        checks.push(check_git_available());
        checks.push(check_git_repo());
    }

    // Auto-fix pass
    if options.fix {
        apply_fixes(&mut checks);
    }

    let report = DoctorReport::from_checks(checks);

    if options.json {
        println!("{}", report.format_json());
    } else {
        print!("{}", report.format_terminal());
    }

    if report.has_failures() {
        std::process::exit(1);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_check(status: CheckStatus, cat: &str, name: &str) -> CheckResult {
        cr(cat, name, status, "test message")
    }

    #[test]
    fn test_terminal_formatting() {
        let report = DoctorReport {
            version: "1.1.0".to_string(),
            checks: vec![
                cr("system", "version", CheckStatus::Pass, "Glass v1.1.0"),
                cr("system", "platform", CheckStatus::Pass, "windows x86_64"),
                cr("gpu", "gpu_adapter", CheckStatus::Fail, "No GPU found")
                    .with_fix("Install drivers"),
            ],
            summary: Summary {
                pass: 2,
                warn: 0,
                fail: 1,
                skip: 0,
                info: 0,
            },
        };
        let output = report.format_terminal();
        assert!(output.contains("Glass Doctor v1.1.0"));
        assert!(output.contains("System"));
        assert!(output.contains("version"));
        assert!(output.contains("Glass v1.1.0"));
        assert!(output.contains("GPU"));
        assert!(output.contains("gpu_adapter"));
        assert!(output.contains("Fix: Install drivers"));
        assert!(output.contains("2 passed, 0 warnings, 1 failed, 0 skipped, 0 info"));
    }

    #[test]
    fn test_json_round_trip() {
        let report = DoctorReport::from_checks(vec![
            cr("system", "version", CheckStatus::Pass, "Glass v1.1.0"),
            cr("gpu", "gpu_adapter", CheckStatus::Fail, "No GPU").with_fix("Install drivers"),
        ]);
        let json = serde_json::to_string(&report).unwrap();
        let deserialized: DoctorReport = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.version, report.version);
        assert_eq!(deserialized.checks.len(), 2);
        assert_eq!(deserialized.checks[0].status, CheckStatus::Pass);
        assert_eq!(deserialized.checks[1].status, CheckStatus::Fail);
        assert_eq!(
            deserialized.checks[1].fix.as_deref(),
            Some("Install drivers")
        );
        assert_eq!(deserialized.summary.pass, 1);
        assert_eq!(deserialized.summary.fail, 1);
    }

    #[test]
    fn test_config_parse_valid() {
        let valid_toml = "";
        let parsed = glass_core::config::GlassConfig::load_validated(valid_toml).ok();
        // GlassConfig should parse an empty string with defaults
        assert!(parsed.is_some(), "GlassConfig should accept empty config");
        let state = ConfigState {
            path: Some(PathBuf::from("/fake/path")),
            exists: true,
            raw_text: Some(valid_toml.to_string()),
            parsed,
            parse_error: None,
        };
        let result = check_config_parse(&state);
        assert_eq!(result.status, CheckStatus::Pass);
        assert!(result.message.contains("Valid TOML"));
    }

    #[test]
    fn test_config_parse_invalid() {
        let state = ConfigState {
            path: Some(PathBuf::from("/fake/path")),
            exists: true,
            raw_text: Some("invalid = [[[".to_string()),
            parsed: None,
            parse_error: Some("expected a value".to_string()),
        };
        let result = check_config_parse(&state);
        assert_eq!(result.status, CheckStatus::Fail);
        assert!(result.message.contains("Parse error"));
    }

    #[test]
    fn test_config_parse_missing() {
        let state = ConfigState {
            path: None,
            exists: false,
            raw_text: None,
            parsed: None,
            parse_error: None,
        };
        let result = check_config_parse(&state);
        assert_eq!(result.status, CheckStatus::Skip);
    }

    #[test]
    fn test_category_filter() {
        assert!(should_run(&None, Category::System));
        assert!(should_run(&None, Category::Gpu));
        assert!(should_run(&Some(Category::System), Category::System));
        assert!(!should_run(&Some(Category::Gpu), Category::System));
        assert!(!should_run(&Some(Category::System), Category::Git));
    }

    #[test]
    fn test_summary_counting() {
        let checks = vec![
            make_check(CheckStatus::Pass, "system", "a"),
            make_check(CheckStatus::Pass, "system", "b"),
            make_check(CheckStatus::Warn, "gpu", "c"),
            make_check(CheckStatus::Fail, "config", "d"),
            make_check(CheckStatus::Skip, "data", "e"),
            make_check(CheckStatus::Info, "agent", "f"),
        ];
        let report = DoctorReport::from_checks(checks);
        assert_eq!(report.summary.pass, 2);
        assert_eq!(report.summary.warn, 1);
        assert_eq!(report.summary.fail, 1);
        assert_eq!(report.summary.skip, 1);
        assert_eq!(report.summary.info, 1);
    }

    #[test]
    fn test_autofix_config_exists() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        let path_opt = Some(config_path.clone());

        // Before: should warn (file doesn't exist)
        let before = check_config_exists_at(&path_opt);
        assert_eq!(before.status, CheckStatus::Warn);

        // Simulate fix: create the file
        std::fs::write(&config_path, "# Glass default config\n").unwrap();

        // After: should pass
        let after = check_config_exists_at(&path_opt);
        assert_eq!(after.status, CheckStatus::Pass);
    }

    #[test]
    fn test_exit_code_no_failures() {
        let report = DoctorReport::from_checks(vec![
            make_check(CheckStatus::Pass, "system", "a"),
            make_check(CheckStatus::Warn, "gpu", "b"),
            make_check(CheckStatus::Skip, "data", "c"),
            make_check(CheckStatus::Info, "agent", "d"),
        ]);
        assert!(!report.has_failures());
    }

    #[test]
    fn test_exit_code_with_failure() {
        let report = DoctorReport::from_checks(vec![
            make_check(CheckStatus::Pass, "system", "a"),
            make_check(CheckStatus::Fail, "gpu", "b"),
        ]);
        assert!(report.has_failures());
    }
}

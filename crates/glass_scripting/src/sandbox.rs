use glass_core::config::ScriptingSection;

/// Hard ceiling: maximum operations any script may perform.
pub const MAX_OPERATIONS_CEILING: u64 = 1_000_000;
/// Hard ceiling: maximum wall-clock timeout in milliseconds.
pub const MAX_TIMEOUT_MS_CEILING: u64 = 10_000;
/// Hard ceiling: maximum scripts attached to a single hook.
pub const MAX_SCRIPTS_PER_HOOK_CEILING: u32 = 25;
/// Hard ceiling: maximum total registered scripts.
pub const MAX_TOTAL_SCRIPTS_CEILING: u32 = 500;
/// Hard ceiling: maximum MCP tools a script may expose.
pub const MAX_MCP_TOOLS_CEILING: u32 = 50;

// Sensible defaults (well under hard ceilings).
const DEFAULT_MAX_OPERATIONS: u64 = 100_000;
const DEFAULT_MAX_TIMEOUT_MS: u64 = 2_000;
const DEFAULT_MAX_SCRIPTS_PER_HOOK: u32 = 10;
const DEFAULT_MAX_TOTAL_SCRIPTS: u32 = 100;
const DEFAULT_MAX_MCP_TOOLS: u32 = 20;

/// Resource limits for the scripting sandbox, clamped to hard ceilings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxConfig {
    /// Maximum operations a single script may perform.
    pub max_operations: u64,
    /// Maximum wall-clock timeout for a single script execution (ms).
    pub max_timeout_ms: u64,
    /// Maximum scripts attached to a single hook.
    pub max_scripts_per_hook: u32,
    /// Maximum total registered scripts.
    pub max_total_scripts: u32,
    /// Maximum MCP tools a script may expose.
    pub max_mcp_tools: u32,
}

impl SandboxConfig {
    /// Create a new `SandboxConfig`, clamping every value to its hard ceiling.
    pub fn new(
        max_operations: u64,
        max_timeout_ms: u64,
        max_scripts_per_hook: u32,
        max_total_scripts: u32,
        max_mcp_tools: u32,
    ) -> Self {
        Self {
            max_operations: max_operations.min(MAX_OPERATIONS_CEILING),
            max_timeout_ms: max_timeout_ms.min(MAX_TIMEOUT_MS_CEILING),
            max_scripts_per_hook: max_scripts_per_hook.min(MAX_SCRIPTS_PER_HOOK_CEILING),
            max_total_scripts: max_total_scripts.min(MAX_TOTAL_SCRIPTS_CEILING),
            max_mcp_tools: max_mcp_tools.min(MAX_MCP_TOOLS_CEILING),
        }
    }

    /// Build a `SandboxConfig` from the parsed config section, using defaults
    /// for any value the user did not specify and clamping to hard ceilings.
    pub fn from_config(section: &ScriptingSection) -> Self {
        Self::new(
            section.max_operations.unwrap_or(DEFAULT_MAX_OPERATIONS),
            section.max_timeout_ms.unwrap_or(DEFAULT_MAX_TIMEOUT_MS),
            section
                .max_scripts_per_hook
                .unwrap_or(DEFAULT_MAX_SCRIPTS_PER_HOOK),
            section
                .max_total_scripts
                .unwrap_or(DEFAULT_MAX_TOTAL_SCRIPTS),
            section.max_mcp_tools.unwrap_or(DEFAULT_MAX_MCP_TOOLS),
        )
    }
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max_operations: DEFAULT_MAX_OPERATIONS,
            max_timeout_ms: DEFAULT_MAX_TIMEOUT_MS,
            max_scripts_per_hook: DEFAULT_MAX_SCRIPTS_PER_HOOK,
            max_total_scripts: DEFAULT_MAX_TOTAL_SCRIPTS,
            max_mcp_tools: DEFAULT_MAX_MCP_TOOLS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_hard_ceilings() {
        let cfg = SandboxConfig::new(
            MAX_OPERATIONS_CEILING + 1,
            MAX_TIMEOUT_MS_CEILING + 500,
            MAX_SCRIPTS_PER_HOOK_CEILING + 10,
            MAX_TOTAL_SCRIPTS_CEILING + 100,
            MAX_MCP_TOOLS_CEILING + 5,
        );
        assert_eq!(cfg.max_operations, MAX_OPERATIONS_CEILING);
        assert_eq!(cfg.max_timeout_ms, MAX_TIMEOUT_MS_CEILING);
        assert_eq!(cfg.max_scripts_per_hook, MAX_SCRIPTS_PER_HOOK_CEILING);
        assert_eq!(cfg.max_total_scripts, MAX_TOTAL_SCRIPTS_CEILING);
        assert_eq!(cfg.max_mcp_tools, MAX_MCP_TOOLS_CEILING);
    }

    #[test]
    fn respects_values_under_ceiling() {
        let cfg = SandboxConfig::new(50_000, 1_000, 5, 50, 10);
        assert_eq!(cfg.max_operations, 50_000);
        assert_eq!(cfg.max_timeout_ms, 1_000);
        assert_eq!(cfg.max_scripts_per_hook, 5);
        assert_eq!(cfg.max_total_scripts, 50);
        assert_eq!(cfg.max_mcp_tools, 10);
    }

    #[test]
    fn default_values() {
        let cfg = SandboxConfig::default();
        assert_eq!(cfg.max_operations, 100_000);
        assert_eq!(cfg.max_timeout_ms, 2_000);
        assert_eq!(cfg.max_scripts_per_hook, 10);
        assert_eq!(cfg.max_total_scripts, 100);
        assert_eq!(cfg.max_mcp_tools, 20);
    }

    #[test]
    fn from_config_uses_defaults_for_none() {
        let section = ScriptingSection {
            enabled: true,
            max_operations: None,
            max_timeout_ms: None,
            max_scripts_per_hook: None,
            max_total_scripts: None,
            max_mcp_tools: None,
            script_generation: true,
        };
        let cfg = SandboxConfig::from_config(&section);
        assert_eq!(cfg, SandboxConfig::default());
    }

    #[test]
    fn from_config_clamps_overrides() {
        let section = ScriptingSection {
            enabled: true,
            max_operations: Some(2_000_000),
            max_timeout_ms: Some(50_000),
            max_scripts_per_hook: Some(100),
            max_total_scripts: Some(1_000),
            max_mcp_tools: Some(200),
            script_generation: true,
        };
        let cfg = SandboxConfig::from_config(&section);
        assert_eq!(cfg.max_operations, MAX_OPERATIONS_CEILING);
        assert_eq!(cfg.max_timeout_ms, MAX_TIMEOUT_MS_CEILING);
        assert_eq!(cfg.max_scripts_per_hook, MAX_SCRIPTS_PER_HOOK_CEILING);
        assert_eq!(cfg.max_total_scripts, MAX_TOTAL_SCRIPTS_CEILING);
        assert_eq!(cfg.max_mcp_tools, MAX_MCP_TOOLS_CEILING);
    }
}

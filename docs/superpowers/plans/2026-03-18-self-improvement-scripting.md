# Self-Improvement Scripting Layer Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an embedded Rhai scripting layer that lets Glass improve itself at runtime — the feedback loop writes scripts that hook into any component, validated through the same promotion/rejection lifecycle as rules.

**Architecture:** New `glass_scripting` crate (depends only on `glass_core`) provides script engine, hook registry, sandbox, loader, and lifecycle management. A bridge module in the binary (`src/script_bridge.rs`) wires events to scripts and executes returned actions against real Glass state. Dynamic MCP tools are routed through IPC via a static proxy tool.

**Tech Stack:** Rhai 1.x (scripting engine), serde/toml (manifest serialization), schemars (MCP JSON schema), tokio (async timeout wrapper)

**Spec:** `docs/superpowers/specs/2026-03-18-self-improvement-scripting-design.md`

---

## File Map

### New Files

| File | Responsibility |
|------|---------------|
| `crates/glass_scripting/Cargo.toml` | Crate manifest with glass_core, rhai, serde, toml, schemars, tracing deps |
| `crates/glass_scripting/src/lib.rs` | Public API: ScriptEngine, re-exports |
| `crates/glass_scripting/src/types.rs` | HookPoint enum, ScriptManifest, ScriptStatus, ScriptOrigin |
| `crates/glass_scripting/src/actions.rs` | Action enum, ConfigValue, LogLevel |
| `crates/glass_scripting/src/engine.rs` | Rhai Engine setup, sandbox config, glass object registration |
| `crates/glass_scripting/src/hooks.rs` | HookRegistry: maps HookPoint -> Vec<LoadedScript> |
| `crates/glass_scripting/src/loader.rs` | Load .toml manifests + .rhai files from disk directories |
| `crates/glass_scripting/src/sandbox.rs` | SandboxConfig, limit validation, hard ceiling enforcement |
| `crates/glass_scripting/src/lifecycle.rs` | Status transitions: promote, reject, archive, staleness |
| `crates/glass_scripting/src/context.rs` | HookContext snapshot: read-only data passed to scripts |
| `crates/glass_scripting/src/mcp.rs` | ScriptToolDef, dynamic tool registry, schema generation |
| `crates/glass_scripting/src/profile.rs` | Profile export/import: bundle confirmed scripts + rules |
| `src/script_bridge.rs` | Bridge: owns ScriptEngine, routes events, executes Actions |

### Modified Files

| File | Change |
|------|--------|
| `Cargo.toml` (workspace root) | Add `rhai`, `schemars` to workspace deps; add `glass_scripting` to binary deps |
| `crates/glass_core/src/config.rs` | Add `ScriptingSection` struct and field on `GlassConfig` |
| `crates/glass_core/src/event.rs` | Add `ScriptActions` event variant (optional, for async action results) |
| `crates/glass_feedback/src/lib.rs` | Add `script_prompt: Option<String>` to `FeedbackResult` |
| `crates/glass_mcp/src/tools.rs` | Add `glass_script_tool` and `glass_list_script_tools` handlers |
| `src/main.rs` | Add `script_bridge` field, call bridge at each hook point, handle script MCP requests |

---

## Task 1: Crate Skeleton and Core Types

**Files:**
- Create: `crates/glass_scripting/Cargo.toml`
- Create: `crates/glass_scripting/src/lib.rs`
- Create: `crates/glass_scripting/src/types.rs`
- Create: `crates/glass_scripting/src/actions.rs`
- Modify: `Cargo.toml` (workspace root, line 3 members, lines 5-67 deps, lines 87-120 binary deps)

- [ ] **Step 1: Create crate Cargo.toml**

```toml
# crates/glass_scripting/Cargo.toml
[package]
name = "glass_scripting"
version = "0.1.0"
edition = "2021"

[dependencies]
glass_core = { path = "../glass_core" }
rhai = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
toml = { workspace = true }
schemars = { workspace = true }
tracing = { workspace = true }
```

- [ ] **Step 2: Add workspace dependencies**

In root `Cargo.toml`, add to `[workspace.dependencies]`:
```toml
rhai = "1"
schemars = "1"
```

Add to `[dependencies]` (binary):
```toml
glass_scripting = { path = "crates/glass_scripting" }
```

- [ ] **Step 3: Write HookPoint enum and manifest types**

```rust
// crates/glass_scripting/src/types.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum HookPoint {
    CommandStart,
    CommandComplete,
    BlockStateChange,
    SnapshotBefore,
    SnapshotAfter,
    HistoryQuery,
    HistoryInsert,
    PipelineComplete,
    ConfigReload,
    OrchestratorRunStart,
    OrchestratorRunEnd,
    OrchestratorIteration,
    OrchestratorCheckpoint,
    OrchestratorStuck,
    McpRequest,
    McpResponse,
    TabCreate,
    TabClose,
    SessionStart,
    SessionEnd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptStatus {
    Provisional,
    Confirmed,
    Rejected,
    Stale,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptOrigin {
    Feedback,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptManifest {
    pub name: String,
    pub hooks: Vec<HookPoint>,
    pub status: ScriptStatus,
    pub origin: ScriptOrigin,
    pub version: u32,
    pub api_version: u32,
    pub created: String,
    #[serde(default)]
    pub failure_count: u32,
    #[serde(default)]
    pub trigger_count: u32,
    #[serde(default)]
    pub stale_runs: u32,
    // MCP tool fields (only for type = "mcp_tool")
    #[serde(default, rename = "type")]
    pub script_type: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub params: Option<toml::Value>,
}

#[derive(Debug, Clone)]
pub struct LoadedScript {
    pub manifest: ScriptManifest,
    pub source: String,
    pub manifest_path: std::path::PathBuf,
    pub source_path: std::path::PathBuf,
}
```

- [ ] **Step 4: Write Action enum**

```rust
// crates/glass_scripting/src/actions.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfigValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    // Git
    Commit { message: String },
    IsolateCommit { files: Vec<String>, message: String },
    RevertFiles { files: Vec<String> },
    // Config
    SetConfig { key: String, value: ConfigValue },
    // Snapshots
    ForceSnapshot { paths: Vec<String> },
    SetSnapshotPolicy { pattern: String, enabled: bool },
    // History
    TagCommand { command_id: String, tags: Vec<String> },
    // Orchestrator
    InjectPromptHint { text: String },
    TriggerCheckpoint { reason: String },
    ExtendSilence { extra_secs: u32 },
    BlockIteration { message: String, max_iterations: u32 },
    // Scripts
    EnableScript { name: String },
    DisableScript { name: String },
    // MCP
    RegisterTool { name: String, description: String, schema: String, handler_script: String },
    UnregisterTool { name: String },
    // Notifications
    Log { level: LogLevel, message: String },
    Notify { message: String },
}
```

- [ ] **Step 5: Write lib.rs with re-exports**

```rust
// crates/glass_scripting/src/lib.rs
pub mod types;
pub mod actions;

pub use types::*;
pub use actions::*;
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo build -p glass_scripting`
Expected: Successful compilation

- [ ] **Step 7: Write unit tests for manifest deserialization**

Add to `crates/glass_scripting/src/types.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_hook_manifest() {
        let toml_str = r#"
            name = "test-script"
            hooks = ["CommandComplete", "OrchestratorRunEnd"]
            status = "provisional"
            origin = "feedback"
            version = 1
            api_version = 1
            created = "2026-03-18"
        "#;
        let manifest: ScriptManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.name, "test-script");
        assert_eq!(manifest.hooks, vec![HookPoint::CommandComplete, HookPoint::OrchestratorRunEnd]);
        assert_eq!(manifest.status, ScriptStatus::Provisional);
        assert_eq!(manifest.origin, ScriptOrigin::Feedback);
        assert_eq!(manifest.api_version, 1);
    }

    #[test]
    fn deserialize_mcp_tool_manifest() {
        let toml_str = r#"
            name = "glass_recent_deploys"
            hooks = []
            status = "confirmed"
            origin = "user"
            version = 1
            api_version = 1
            created = "2026-03-18"
            type = "mcp_tool"
            description = "Returns recent deployment commands"

            [params]
            limit = { type = "number", required = false, default = 10 }
        "#;
        let manifest: ScriptManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.script_type, Some("mcp_tool".to_string()));
        assert!(manifest.params.is_some());
    }
}
```

- [ ] **Step 8: Run tests**

Run: `cargo test -p glass_scripting`
Expected: 2 tests pass

- [ ] **Step 9: Commit**

```bash
git add crates/glass_scripting/ Cargo.toml Cargo.lock
git commit -m "feat(scripting): add glass_scripting crate with core types and action enum"
```

---

## Task 2: Config Section

**Files:**
- Modify: `crates/glass_core/src/config.rs:226-244` (GlassConfig struct)
- Modify: `crates/glass_core/src/config.rs:354-367` (Default impl)

- [ ] **Step 1: Write failing test for config parsing**

Add test to `crates/glass_core/src/config.rs` in the existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn parse_scripting_section() {
    let toml_str = r#"
        [scripting]
        enabled = true
        max_operations = 200000
        max_timeout_ms = 3000
        max_scripts_per_hook = 15
        max_total_scripts = 200
        max_mcp_tools = 30
        script_generation = false
    "#;
    let config: GlassConfig = toml::from_str(toml_str).unwrap();
    let s = config.scripting.unwrap();
    assert!(s.enabled);
    assert_eq!(s.max_operations, Some(200000));
    assert_eq!(s.max_timeout_ms, Some(3000));
    assert_eq!(s.max_scripts_per_hook, Some(15));
    assert_eq!(s.max_total_scripts, Some(200));
    assert_eq!(s.max_mcp_tools, Some(30));
    assert!(!s.script_generation);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p glass_core parse_scripting_section`
Expected: FAIL — `ScriptingSection` does not exist

- [ ] **Step 3: Add ScriptingSection struct and field**

Add struct before `GlassConfig` definition (around line 220):

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ScriptingSection {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub max_operations: Option<u64>,
    pub max_timeout_ms: Option<u64>,
    pub max_scripts_per_hook: Option<u32>,
    pub max_total_scripts: Option<u32>,
    pub max_mcp_tools: Option<u32>,
    #[serde(default = "default_true")]
    pub script_generation: bool,
}
```

Note: Only `Deserialize` — matches all other config section structs. `Serialize` is not needed since config is read-only.

Add `default_true` helper if it doesn't already exist:
```rust
fn default_true() -> bool { true }
```

Add field to `GlassConfig` struct:
```rust
pub scripting: Option<ScriptingSection>,
```

**Critical:** Also add `scripting: None,` to the explicit `Default` impl for `GlassConfig` (around line 354-367). The struct uses an explicit `Self { ... }` construction, so the new field will cause a compile error if omitted.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p glass_core parse_scripting_section`
Expected: PASS

- [ ] **Step 5: Add defaults test**

```rust
#[test]
fn scripting_section_defaults() {
    let toml_str = r#"
        [scripting]
    "#;
    let config: GlassConfig = toml::from_str(toml_str).unwrap();
    let s = config.scripting.unwrap();
    assert!(s.enabled);
    assert!(s.script_generation);
    assert!(s.max_operations.is_none());
}
```

- [ ] **Step 6: Run all glass_core tests**

Run: `cargo test -p glass_core`
Expected: All tests pass

- [ ] **Step 7: Verify full workspace compiles**

Run: `cargo build --workspace`
Expected: Successful. If main.rs has exhaustive matches on GlassConfig fields, fix them.

- [ ] **Step 8: Commit**

```bash
git add crates/glass_core/src/config.rs
git commit -m "feat(config): add [scripting] section to GlassConfig"
```

---

## Task 3: Sandbox Configuration

**Files:**
- Create: `crates/glass_scripting/src/sandbox.rs`
- Modify: `crates/glass_scripting/src/lib.rs`

- [ ] **Step 1: Write failing test for limit validation**

```rust
// crates/glass_scripting/src/sandbox.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_hard_ceilings() {
        let config = SandboxConfig::new(2_000_000, 20_000, 30, 600, 60);
        assert_eq!(config.max_operations, MAX_OPERATIONS_CEILING);
        assert_eq!(config.max_timeout_ms, MAX_TIMEOUT_MS_CEILING);
        assert_eq!(config.max_scripts_per_hook, MAX_SCRIPTS_PER_HOOK_CEILING);
        assert_eq!(config.max_total_scripts, MAX_TOTAL_SCRIPTS_CEILING);
        assert_eq!(config.max_mcp_tools, MAX_MCP_TOOLS_CEILING);
    }

    #[test]
    fn respects_values_under_ceiling() {
        let config = SandboxConfig::new(50_000, 1000, 5, 50, 10);
        assert_eq!(config.max_operations, 50_000);
        assert_eq!(config.max_timeout_ms, 1000);
        assert_eq!(config.max_scripts_per_hook, 5);
        assert_eq!(config.max_total_scripts, 50);
        assert_eq!(config.max_mcp_tools, 10);
    }

    #[test]
    fn default_values() {
        let config = SandboxConfig::default();
        assert_eq!(config.max_operations, 100_000);
        assert_eq!(config.max_timeout_ms, 2_000);
        assert_eq!(config.max_scripts_per_hook, 10);
        assert_eq!(config.max_total_scripts, 100);
        assert_eq!(config.max_mcp_tools, 20);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p glass_scripting enforces_hard_ceilings`
Expected: FAIL — `SandboxConfig` does not exist

- [ ] **Step 3: Implement SandboxConfig**

```rust
// crates/glass_scripting/src/sandbox.rs

pub const MAX_OPERATIONS_CEILING: u64 = 1_000_000;
pub const MAX_TIMEOUT_MS_CEILING: u64 = 10_000;
pub const MAX_SCRIPTS_PER_HOOK_CEILING: u32 = 25;
pub const MAX_TOTAL_SCRIPTS_CEILING: u32 = 500;
pub const MAX_MCP_TOOLS_CEILING: u32 = 50;

const DEFAULT_MAX_OPERATIONS: u64 = 100_000;
const DEFAULT_MAX_TIMEOUT_MS: u64 = 2_000;
const DEFAULT_MAX_SCRIPTS_PER_HOOK: u32 = 10;
const DEFAULT_MAX_TOTAL_SCRIPTS: u32 = 100;
const DEFAULT_MAX_MCP_TOOLS: u32 = 20;

#[derive(Debug, Clone)]
pub struct SandboxConfig {
    pub max_operations: u64,
    pub max_timeout_ms: u64,
    pub max_scripts_per_hook: u32,
    pub max_total_scripts: u32,
    pub max_mcp_tools: u32,
}

impl SandboxConfig {
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

    pub fn from_config(section: &glass_core::config::ScriptingSection) -> Self {
        Self::new(
            section.max_operations.unwrap_or(DEFAULT_MAX_OPERATIONS),
            section.max_timeout_ms.unwrap_or(DEFAULT_MAX_TIMEOUT_MS),
            section.max_scripts_per_hook.unwrap_or(DEFAULT_MAX_SCRIPTS_PER_HOOK),
            section.max_total_scripts.unwrap_or(DEFAULT_MAX_TOTAL_SCRIPTS),
            section.max_mcp_tools.unwrap_or(DEFAULT_MAX_MCP_TOOLS),
        )
    }
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self::new(
            DEFAULT_MAX_OPERATIONS,
            DEFAULT_MAX_TIMEOUT_MS,
            DEFAULT_MAX_SCRIPTS_PER_HOOK,
            DEFAULT_MAX_TOTAL_SCRIPTS,
            DEFAULT_MAX_MCP_TOOLS,
        )
    }
}
```

- [ ] **Step 4: Add module to lib.rs**

Add `pub mod sandbox;` and `pub use sandbox::*;` to lib.rs.

- [ ] **Step 5: Run tests**

Run: `cargo test -p glass_scripting`
Expected: All tests pass (manifest tests + sandbox tests)

- [ ] **Step 6: Commit**

```bash
git add crates/glass_scripting/src/sandbox.rs crates/glass_scripting/src/lib.rs
git commit -m "feat(scripting): add SandboxConfig with hard ceiling enforcement"
```

---

## Task 4: Script Loader

**Files:**
- Create: `crates/glass_scripting/src/loader.rs`
- Modify: `crates/glass_scripting/src/lib.rs`

- [ ] **Step 1: Write failing test**

```rust
// crates/glass_scripting/src/loader.rs
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn load_script_pair() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        fs::write(hooks_dir.join("test_script.toml"), r#"
            name = "test-script"
            hooks = ["CommandComplete"]
            status = "confirmed"
            origin = "user"
            version = 1
            api_version = 1
            created = "2026-03-18"
        "#).unwrap();

        fs::write(hooks_dir.join("test_script.rhai"), r#"
            if event.exit_code == 0 {
                glass.log("info", "command succeeded");
            }
        "#).unwrap();

        let scripts = load_scripts_from_dir(dir.path());
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].manifest.name, "test-script");
        assert!(scripts[0].source.contains("event.exit_code"));
    }

    #[test]
    fn skip_archived_scripts() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        fs::write(hooks_dir.join("old.toml"), r#"
            name = "old-script"
            hooks = ["CommandComplete"]
            status = "archived"
            origin = "feedback"
            version = 1
            api_version = 1
            created = "2026-03-18"
        "#).unwrap();
        fs::write(hooks_dir.join("old.rhai"), "// archived").unwrap();

        let scripts = load_scripts_from_dir(dir.path());
        assert_eq!(scripts.len(), 0);
    }

    #[test]
    fn skip_manifest_without_rhai() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        fs::write(hooks_dir.join("orphan.toml"), r#"
            name = "orphan"
            hooks = ["SessionStart"]
            status = "confirmed"
            origin = "user"
            version = 1
            api_version = 1
            created = "2026-03-18"
        "#).unwrap();
        // No matching .rhai file

        let scripts = load_scripts_from_dir(dir.path());
        assert_eq!(scripts.len(), 0);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glass_scripting load_script`
Expected: FAIL — `load_scripts_from_dir` does not exist

- [ ] **Step 3: Add `tempfile` dev-dependency**

In `crates/glass_scripting/Cargo.toml`:
```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 4: Implement loader**

```rust
// crates/glass_scripting/src/loader.rs
use crate::types::{LoadedScript, ScriptManifest, ScriptStatus};
use std::path::Path;
use tracing::warn;

/// Load all active scripts from a scripts directory.
/// Expects subdirectories: hooks/, tools/, feedback/
/// Each script is a .toml manifest + .rhai source pair.
pub fn load_scripts_from_dir(base: &Path) -> Vec<LoadedScript> {
    let mut scripts = Vec::new();
    let subdirs = ["hooks", "tools", "feedback"];

    for subdir in &subdirs {
        let dir = base.join(subdir);
        if !dir.is_dir() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                    if let Some(script) = load_script_pair(&path) {
                        scripts.push(script);
                    }
                }
            }
        }
    }
    scripts
}

fn load_script_pair(manifest_path: &Path) -> Option<LoadedScript> {
    let source_path = manifest_path.with_extension("rhai");
    if !source_path.exists() {
        warn!("Script manifest without .rhai file: {:?}", manifest_path);
        return None;
    }

    let manifest_str = std::fs::read_to_string(manifest_path).ok()?;
    let manifest: ScriptManifest = match toml::from_str(&manifest_str) {
        Ok(m) => m,
        Err(e) => {
            warn!("Failed to parse manifest {:?}: {}", manifest_path, e);
            return None;
        }
    };

    // Skip archived and rejected scripts
    if matches!(manifest.status, ScriptStatus::Archived | ScriptStatus::Rejected) {
        return None;
    }

    let source = std::fs::read_to_string(&source_path).ok()?;

    Some(LoadedScript {
        manifest,
        source,
        manifest_path: manifest_path.to_path_buf(),
        source_path,
    })
}

/// Load scripts from both project-local and global directories.
/// Project scripts take precedence over global scripts with the same name.
pub fn load_all_scripts(project_root: &str) -> Vec<LoadedScript> {
    let mut scripts = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    // Project-local scripts first (higher precedence)
    let project_dir = std::path::Path::new(project_root).join(".glass").join("scripts");
    if project_dir.is_dir() {
        for script in load_scripts_from_dir(&project_dir) {
            seen_names.insert(script.manifest.name.clone());
            scripts.push(script);
        }
    }

    // Global scripts (skip if name already loaded from project)
    if let Some(home) = dirs::home_dir() {
        let global_dir = home.join(".glass").join("scripts");
        if global_dir.is_dir() {
            for script in load_scripts_from_dir(&global_dir) {
                if !seen_names.contains(&script.manifest.name) {
                    scripts.push(script);
                }
            }
        }
    }

    scripts
}
```

- [ ] **Step 5: Add `dirs` dependency**

In `crates/glass_scripting/Cargo.toml`:
```toml
dirs = { workspace = true }
```

- [ ] **Step 6: Add module to lib.rs**

```rust
pub mod loader;
```

- [ ] **Step 7: Run tests**

Run: `cargo test -p glass_scripting`
Expected: All tests pass

- [ ] **Step 8: Commit**

```bash
git add crates/glass_scripting/
git commit -m "feat(scripting): add script loader with manifest/source pairing"
```

---

## Task 5: Hook Registry

**Files:**
- Create: `crates/glass_scripting/src/hooks.rs`
- Modify: `crates/glass_scripting/src/lib.rs`

- [ ] **Step 1: Write failing test**

```rust
// crates/glass_scripting/src/hooks.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn make_script(name: &str, hooks: Vec<HookPoint>, status: ScriptStatus, origin: ScriptOrigin) -> LoadedScript {
        LoadedScript {
            manifest: ScriptManifest {
                name: name.to_string(),
                hooks,
                status,
                origin,
                version: 1,
                api_version: 1,
                created: "2026-03-18".to_string(),
                failure_count: 0,
                trigger_count: 0,
                stale_runs: 0,
                script_type: None,
                description: None,
                params: None,
            },
            source: "// test".to_string(),
            manifest_path: std::path::PathBuf::from("/test.toml"),
            source_path: std::path::PathBuf::from("/test.rhai"),
        }
    }

    #[test]
    fn registry_groups_by_hook() {
        let scripts = vec![
            make_script("a", vec![HookPoint::CommandComplete], ScriptStatus::Confirmed, ScriptOrigin::User),
            make_script("b", vec![HookPoint::CommandComplete, HookPoint::SessionStart], ScriptStatus::Provisional, ScriptOrigin::Feedback),
        ];
        let registry = HookRegistry::new(scripts, 10);
        assert_eq!(registry.scripts_for(HookPoint::CommandComplete).len(), 2);
        assert_eq!(registry.scripts_for(HookPoint::SessionStart).len(), 1);
        assert_eq!(registry.scripts_for(HookPoint::TabCreate).len(), 0);
    }

    #[test]
    fn priority_order_default() {
        let scripts = vec![
            make_script("user1", vec![HookPoint::CommandComplete], ScriptStatus::Confirmed, ScriptOrigin::User),
            make_script("fb_confirmed", vec![HookPoint::CommandComplete], ScriptStatus::Confirmed, ScriptOrigin::Feedback),
            make_script("fb_prov", vec![HookPoint::CommandComplete], ScriptStatus::Provisional, ScriptOrigin::Feedback),
        ];
        let registry = HookRegistry::new(scripts, 10);
        let ordered = registry.scripts_for(HookPoint::CommandComplete);
        // Default: confirmed > provisional > user
        assert_eq!(ordered[0].manifest.name, "fb_confirmed");
        assert_eq!(ordered[1].manifest.name, "user1");
        assert_eq!(ordered[2].manifest.name, "fb_prov");
    }

    #[test]
    fn mcp_request_reverses_priority() {
        let scripts = vec![
            make_script("user1", vec![HookPoint::McpRequest], ScriptStatus::Confirmed, ScriptOrigin::User),
            make_script("fb1", vec![HookPoint::McpRequest], ScriptStatus::Confirmed, ScriptOrigin::Feedback),
        ];
        let registry = HookRegistry::new(scripts, 10);
        let ordered = registry.scripts_for(HookPoint::McpRequest);
        // McpRequest: user > confirmed > provisional
        assert_eq!(ordered[0].manifest.name, "user1");
        assert_eq!(ordered[1].manifest.name, "fb1");
    }

    #[test]
    fn enforces_per_hook_limit() {
        let scripts: Vec<_> = (0..15)
            .map(|i| make_script(&format!("s{i}"), vec![HookPoint::CommandComplete], ScriptStatus::Confirmed, ScriptOrigin::User))
            .collect();
        let registry = HookRegistry::new(scripts, 10);
        assert_eq!(registry.scripts_for(HookPoint::CommandComplete).len(), 10);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glass_scripting registry_groups`
Expected: FAIL

- [ ] **Step 3: Implement HookRegistry**

```rust
// crates/glass_scripting/src/hooks.rs
use crate::types::{HookPoint, LoadedScript, ScriptOrigin, ScriptStatus};
use std::collections::HashMap;

pub struct HookRegistry {
    hooks: HashMap<HookPoint, Vec<LoadedScript>>,
}

impl HookRegistry {
    pub fn new(scripts: Vec<LoadedScript>, max_per_hook: u32) -> Self {
        let mut hooks: HashMap<HookPoint, Vec<LoadedScript>> = HashMap::new();

        for script in scripts {
            for hook in &script.manifest.hooks {
                hooks.entry(*hook).or_default().push(script.clone());
            }
        }

        // Sort and truncate each hook's scripts
        for (hook, scripts) in hooks.iter_mut() {
            let reversed = *hook == HookPoint::McpRequest;
            scripts.sort_by(|a, b| {
                let pa = priority_score(&a.manifest.status, &a.manifest.origin, reversed);
                let pb = priority_score(&b.manifest.status, &b.manifest.origin, reversed);
                pa.cmp(&pb)
            });
            scripts.truncate(max_per_hook as usize);
        }

        Self { hooks }
    }

    pub fn scripts_for(&self, hook: HookPoint) -> &[LoadedScript] {
        self.hooks.get(&hook).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn has_scripts_for(&self, hook: HookPoint) -> bool {
        self.hooks.get(&hook).map_or(false, |v| !v.is_empty())
    }

    pub fn all_scripts(&self) -> Vec<&LoadedScript> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for scripts in self.hooks.values() {
            for script in scripts {
                if seen.insert(&script.manifest.name) {
                    result.push(script);
                }
            }
        }
        result
    }
}

fn priority_score(status: &ScriptStatus, origin: &ScriptOrigin, reversed: bool) -> u32 {
    if reversed {
        // McpRequest: user > confirmed > provisional
        match (origin, status) {
            (ScriptOrigin::User, _) => 0,
            (_, ScriptStatus::Confirmed) => 1,
            (_, ScriptStatus::Provisional) => 2,
            (_, ScriptStatus::Stale) => 3,
            _ => 4,
        }
    } else {
        // Default: confirmed > user > provisional
        match (status, origin) {
            (ScriptStatus::Confirmed, ScriptOrigin::Feedback) => 0,
            (ScriptStatus::Confirmed, ScriptOrigin::User) => 1,
            (_, ScriptOrigin::User) => 1,
            (ScriptStatus::Provisional, _) => 2,
            (ScriptStatus::Stale, _) => 3,
            _ => 4,
        }
    }
}
```

- [ ] **Step 4: Add module to lib.rs**

```rust
pub mod hooks;
pub use hooks::HookRegistry;
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p glass_scripting`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/glass_scripting/
git commit -m "feat(scripting): add HookRegistry with priority ordering and per-hook limits"
```

---

## Task 6: Rhai Engine and Context

**Files:**
- Create: `crates/glass_scripting/src/engine.rs`
- Create: `crates/glass_scripting/src/context.rs`
- Modify: `crates/glass_scripting/src/lib.rs`

- [ ] **Step 1: Write HookContext (read-only snapshot)**

```rust
// crates/glass_scripting/src/context.rs
use serde::{Deserialize, Serialize};

/// Snapshot of Glass state taken once per hook invocation.
/// All scripts on the same hook share this snapshot.
#[derive(Debug, Clone, Default)]
pub struct HookContext {
    pub cwd: String,
    pub git_branch: String,
    pub git_dirty_files: Vec<String>,
    pub recent_commands: Vec<CommandSnapshot>,
    pub active_rules: Vec<String>,
    pub config_values: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSnapshot {
    pub command: String,
    pub exit_code: i32,
    pub cwd: String,
    pub duration_ms: u64,
}

/// Event-specific data passed to scripts alongside the context.
#[derive(Debug, Clone, Default)]
pub struct HookEventData {
    /// Flat key-value pairs accessible as `event.key` in Rhai
    pub fields: std::collections::HashMap<String, rhai::Dynamic>,
}

impl HookEventData {
    pub fn new() -> Self {
        Self { fields: std::collections::HashMap::new() }
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<rhai::Dynamic>) {
        self.fields.insert(key.into(), value.into());
    }
}
```

- [ ] **Step 2: Write engine with Rhai setup and glass object as custom type with registered functions**

The `glass` object must be a custom Rhai type with registered methods so scripts can call `glass.commit("msg")`, `glass.log("info", "text")`, etc. A plain `Map` would not support method call syntax.

```rust
// crates/glass_scripting/src/engine.rs
use crate::actions::{Action, LogLevel};
use crate::context::{HookContext, HookEventData};
use crate::sandbox::SandboxConfig;
use crate::types::LoadedScript;
use rhai::{Dynamic, Engine, Map, Scope, AST};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{debug, warn};

/// Rhai-exposed custom type that scripts interact with.
/// Read-only context is cloned in; action methods push to a shared queue.
#[derive(Debug, Clone)]
pub struct GlassApi {
    pub cwd: String,
    pub git_branch: String,
    pub git_dirty_files: Vec<String>,
    pub config_values: HashMap<String, String>,
    pub active_rules: Vec<String>,
    actions: Arc<Mutex<Vec<Action>>>,
}

impl GlassApi {
    fn new(context: &HookContext, actions: Arc<Mutex<Vec<Action>>>) -> Self {
        Self {
            cwd: context.cwd.clone(),
            git_branch: context.git_branch.clone(),
            git_dirty_files: context.git_dirty_files.clone(),
            config_values: context.config_values.clone(),
            active_rules: context.active_rules.clone(),
            actions,
        }
    }

    // Read-only methods
    fn cwd(&mut self) -> String { self.cwd.clone() }
    fn git_branch(&mut self) -> String { self.git_branch.clone() }
    fn git_dirty_files(&mut self) -> Vec<Dynamic> {
        self.git_dirty_files.iter().map(|s| Dynamic::from(s.clone())).collect()
    }
    fn config(&mut self, key: String) -> Dynamic {
        self.config_values.get(&key)
            .map(|v| Dynamic::from(v.clone()))
            .unwrap_or(Dynamic::UNIT)
    }
    fn active_rules(&mut self) -> Vec<Dynamic> {
        self.active_rules.iter().map(|s| Dynamic::from(s.clone())).collect()
    }

    // Action methods — push to shared queue
    fn commit(&mut self, message: String) {
        self.actions.lock().unwrap().push(Action::Commit { message });
    }
    fn log(&mut self, level: String, message: String) {
        let level = match level.as_str() {
            "debug" => LogLevel::Debug,
            "warn" => LogLevel::Warn,
            "error" => LogLevel::Error,
            _ => LogLevel::Info,
        };
        self.actions.lock().unwrap().push(Action::Log { level, message });
    }
    fn notify(&mut self, message: String) {
        self.actions.lock().unwrap().push(Action::Notify { message });
    }
    fn set_config(&mut self, key: String, value: Dynamic) {
        let config_value = if let Some(b) = value.as_bool().ok() {
            crate::actions::ConfigValue::Bool(b)
        } else if let Some(i) = value.as_int().ok() {
            crate::actions::ConfigValue::Int(i)
        } else if let Some(f) = value.as_float().ok() {
            crate::actions::ConfigValue::Float(f)
        } else {
            crate::actions::ConfigValue::String(value.to_string())
        };
        self.actions.lock().unwrap().push(Action::SetConfig { key, value: config_value });
    }
    fn inject_prompt_hint(&mut self, text: String) {
        self.actions.lock().unwrap().push(Action::InjectPromptHint { text });
    }
    fn force_snapshot(&mut self, paths: Vec<Dynamic>) {
        let paths: Vec<String> = paths.into_iter().map(|p| p.to_string()).collect();
        self.actions.lock().unwrap().push(Action::ForceSnapshot { paths });
    }
    fn trigger_checkpoint(&mut self, reason: String) {
        self.actions.lock().unwrap().push(Action::TriggerCheckpoint { reason });
    }
    fn extend_silence(&mut self, extra_secs: i64) {
        self.actions.lock().unwrap().push(Action::ExtendSilence { extra_secs: extra_secs as u32 });
    }
}

pub struct ScriptEngine {
    engine: Engine,
    sandbox: SandboxConfig,
    compiled: HashMap<String, AST>,
}

/// Result of running all scripts for a single hook invocation.
#[derive(Debug, Default)]
pub struct ScriptRunResult {
    pub actions: Vec<Action>,
    pub errors: Vec<(String, String)>,  // (script_name, error_message)
    pub filter_result: Option<bool>,     // for SnapshotBefore
    pub mcp_response: Option<Dynamic>,   // for McpRequest
}

impl ScriptEngine {
    pub fn new(sandbox: SandboxConfig) -> Self {
        let mut engine = Engine::new();

        // Sandbox limits
        engine.disable_symbol("eval");
        engine.set_max_expr_depths(64, 32);
        engine.set_max_operations(sandbox.max_operations);
        engine.set_max_string_size(1_048_576);
        engine.set_max_array_size(10_000);
        engine.set_max_map_size(10_000);

        // Register GlassApi as a custom type with methods
        engine.register_type_with_name::<GlassApi>("GlassApi")
            .register_fn("cwd", GlassApi::cwd)
            .register_fn("git_branch", GlassApi::git_branch)
            .register_fn("git_dirty_files", GlassApi::git_dirty_files)
            .register_fn("config", GlassApi::config)
            .register_fn("active_rules", GlassApi::active_rules)
            .register_fn("commit", GlassApi::commit)
            .register_fn("log", GlassApi::log)
            .register_fn("notify", GlassApi::notify)
            .register_fn("set_config", GlassApi::set_config)
            .register_fn("inject_prompt_hint", GlassApi::inject_prompt_hint)
            .register_fn("force_snapshot", GlassApi::force_snapshot)
            .register_fn("trigger_checkpoint", GlassApi::trigger_checkpoint)
            .register_fn("extend_silence", GlassApi::extend_silence);

        Self {
            engine,
            sandbox,
            compiled: HashMap::new(),
        }
    }

    /// Compile a script and cache the AST.
    pub fn compile(&mut self, script: &LoadedScript) -> Result<(), String> {
        match self.engine.compile(&script.source) {
            Ok(ast) => {
                self.compiled.insert(script.manifest.name.clone(), ast);
                Ok(())
            }
            Err(e) => Err(format!("Compile error in '{}': {}", script.manifest.name, e)),
        }
    }

    /// Compile all scripts, returning errors for any that fail.
    pub fn compile_all(&mut self, scripts: &[LoadedScript]) -> Vec<(String, String)> {
        let mut errors = Vec::new();
        for script in scripts {
            if let Err(e) = self.compile(script) {
                errors.push((script.manifest.name.clone(), e));
            }
        }
        errors
    }

    /// Run a single compiled script with the given context and event data.
    /// Returns actions and/or errors.
    pub fn run_script(
        &self,
        script_name: &str,
        context: &HookContext,
        event_data: &HookEventData,
    ) -> Result<Vec<Action>, String> {
        let ast = self.compiled.get(script_name)
            .ok_or_else(|| format!("Script '{}' not compiled", script_name))?;

        let mut scope = Scope::new();

        // Build event map from hook-specific data
        let mut event_map = Map::new();
        for (k, v) in &event_data.fields {
            event_map.insert(k.clone().into(), v.clone());
        }
        scope.push("event", event_map);

        // Build glass API object with shared action queue
        let actions: Arc<Mutex<Vec<Action>>> = Arc::new(Mutex::new(Vec::new()));
        let glass_api = GlassApi::new(context, actions.clone());
        scope.push("glass", glass_api);

        match self.engine.eval_ast_with_scope::<Dynamic>(&mut scope, ast) {
            Ok(_) => {
                let collected = actions.lock().unwrap().drain(..).collect();
                Ok(collected)
            }
            Err(e) => Err(format!("Runtime error in '{}': {}", script_name, e)),
        }
    }
}
```

- [ ] **Step 3: Write tests for engine**

Add to `crates/glass_scripting/src/engine.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn test_script(name: &str, source: &str) -> LoadedScript {
        LoadedScript {
            manifest: ScriptManifest {
                name: name.to_string(),
                hooks: vec![HookPoint::CommandComplete],
                status: ScriptStatus::Confirmed,
                origin: ScriptOrigin::User,
                version: 1,
                api_version: 1,
                created: "2026-03-18".to_string(),
                failure_count: 0,
                trigger_count: 0,
                stale_runs: 0,
                script_type: None,
                description: None,
                params: None,
            },
            source: source.to_string(),
            manifest_path: std::path::PathBuf::from("/test.toml"),
            source_path: std::path::PathBuf::from("/test.rhai"),
        }
    }

    #[test]
    fn compile_valid_script() {
        let mut engine = ScriptEngine::new(SandboxConfig::default());
        let script = test_script("test", "let x = 42;");
        assert!(engine.compile(&script).is_ok());
    }

    #[test]
    fn compile_invalid_script() {
        let mut engine = ScriptEngine::new(SandboxConfig::default());
        let script = test_script("bad", "let x = ;");
        assert!(engine.compile(&script).is_err());
    }

    #[test]
    fn run_script_accesses_event_data() {
        let mut engine = ScriptEngine::new(SandboxConfig::default());
        let script = test_script("test", r#"
            let code = event.exit_code;
            // Script runs without error if event data is accessible
        "#);
        engine.compile(&script).unwrap();

        let context = HookContext::default();
        let mut event_data = HookEventData::new();
        event_data.set("exit_code", 0_i64);

        let result = engine.run_script("test", &context, &event_data);
        assert!(result.is_ok());
    }

    #[test]
    fn sandbox_limits_excessive_operations() {
        let config = SandboxConfig::new(1000, 5000, 10, 100, 20); // very low op limit
        let mut engine = ScriptEngine::new(config);
        let script = test_script("heavy", r#"
            let x = 0;
            while x < 1000000 { x += 1; }
        "#);
        engine.compile(&script).unwrap();

        let result = engine.run_script("heavy", &HookContext::default(), &HookEventData::new());
        assert!(result.is_err()); // Should hit operation limit
    }
}
```

- [ ] **Step 4: Add modules to lib.rs**

```rust
pub mod context;
pub mod engine;
pub use context::*;
pub use engine::ScriptEngine;
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p glass_scripting`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/glass_scripting/
git commit -m "feat(scripting): add Rhai ScriptEngine with sandbox, context, and event data"
```

---

## Task 7: Script Lifecycle Management

**Files:**
- Create: `crates/glass_scripting/src/lifecycle.rs`
- Modify: `crates/glass_scripting/src/lib.rs`

- [ ] **Step 1: Write failing tests**

```rust
// crates/glass_scripting/src/lifecycle.rs
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn promote_provisional_to_confirmed() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("test.toml");
        fs::write(&manifest_path, r#"
            name = "test"
            hooks = ["CommandComplete"]
            status = "provisional"
            origin = "feedback"
            version = 1
            api_version = 1
            created = "2026-03-18"
        "#).unwrap();

        promote_script(&manifest_path).unwrap();

        let content = fs::read_to_string(&manifest_path).unwrap();
        let manifest: crate::types::ScriptManifest = toml::from_str(&content).unwrap();
        assert_eq!(manifest.status, crate::types::ScriptStatus::Confirmed);
    }

    #[test]
    fn reject_script_sets_archived() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("test.toml");
        fs::write(&manifest_path, r#"
            name = "test"
            hooks = ["CommandComplete"]
            status = "provisional"
            origin = "feedback"
            version = 1
            api_version = 1
            created = "2026-03-18"
        "#).unwrap();

        reject_script(&manifest_path).unwrap();

        let content = fs::read_to_string(&manifest_path).unwrap();
        let manifest: crate::types::ScriptManifest = toml::from_str(&content).unwrap();
        assert_eq!(manifest.status, crate::types::ScriptStatus::Archived);
    }

    #[test]
    fn increment_failure_rejects_at_three() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("test.toml");
        fs::write(&manifest_path, r#"
            name = "test"
            hooks = ["CommandComplete"]
            status = "provisional"
            origin = "feedback"
            version = 1
            api_version = 1
            created = "2026-03-18"
            failure_count = 2
        "#).unwrap();

        let rejected = record_failure(&manifest_path).unwrap();
        assert!(rejected);

        let content = fs::read_to_string(&manifest_path).unwrap();
        let manifest: crate::types::ScriptManifest = toml::from_str(&content).unwrap();
        assert_eq!(manifest.status, crate::types::ScriptStatus::Archived);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glass_scripting promote_provisional`
Expected: FAIL

- [ ] **Step 3: Implement lifecycle functions**

```rust
// crates/glass_scripting/src/lifecycle.rs
use crate::types::{ScriptManifest, ScriptStatus};
use std::path::Path;

const MAX_CONSECUTIVE_FAILURES: u32 = 3;

/// Promote a provisional script to confirmed.
pub fn promote_script(manifest_path: &Path) -> anyhow::Result<()> {
    let mut manifest = read_manifest(manifest_path)?;
    manifest.status = ScriptStatus::Confirmed;
    manifest.failure_count = 0;
    manifest.stale_runs = 0;
    write_manifest(manifest_path, &manifest)
}

/// Reject a script — sets status to archived.
pub fn reject_script(manifest_path: &Path) -> anyhow::Result<()> {
    let mut manifest = read_manifest(manifest_path)?;
    manifest.status = ScriptStatus::Archived;
    write_manifest(manifest_path, &manifest)
}

/// Record a script failure. Returns true if the script was auto-rejected.
pub fn record_failure(manifest_path: &Path) -> anyhow::Result<bool> {
    let mut manifest = read_manifest(manifest_path)?;
    manifest.failure_count += 1;
    if manifest.failure_count >= MAX_CONSECUTIVE_FAILURES {
        manifest.status = ScriptStatus::Archived;
        write_manifest(manifest_path, &manifest)?;
        Ok(true)
    } else {
        write_manifest(manifest_path, &manifest)?;
        Ok(false)
    }
}

/// Record a successful trigger — resets failure count, resets stale_runs.
pub fn record_trigger(manifest_path: &Path) -> anyhow::Result<()> {
    let mut manifest = read_manifest(manifest_path)?;
    manifest.trigger_count += 1;
    manifest.failure_count = 0;
    if manifest.status == ScriptStatus::Stale {
        manifest.status = ScriptStatus::Confirmed;
    }
    manifest.stale_runs = 0;
    write_manifest(manifest_path, &manifest)
}

/// Increment stale_runs. If threshold exceeded, mark stale or archive.
pub fn increment_stale(manifest_path: &Path, stale_threshold: u32, archive_threshold: u32) -> anyhow::Result<()> {
    let mut manifest = read_manifest(manifest_path)?;
    manifest.stale_runs += 1;
    if manifest.stale_runs >= archive_threshold {
        manifest.status = ScriptStatus::Archived;
    } else if manifest.stale_runs >= stale_threshold && manifest.status == ScriptStatus::Confirmed {
        manifest.status = ScriptStatus::Stale;
    }
    write_manifest(manifest_path, &manifest)
}

fn read_manifest(path: &Path) -> anyhow::Result<ScriptManifest> {
    let content = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&content)?)
}

fn write_manifest(path: &Path, manifest: &ScriptManifest) -> anyhow::Result<()> {
    let content = toml::to_string_pretty(manifest)?;
    std::fs::write(path, content)?;
    Ok(())
}
```

- [ ] **Step 4: Add `anyhow` dependency and module**

In `crates/glass_scripting/Cargo.toml`:
```toml
anyhow = { workspace = true }
```

In `lib.rs`:
```rust
pub mod lifecycle;
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p glass_scripting`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/glass_scripting/
git commit -m "feat(scripting): add script lifecycle management (promote, reject, stale, archive)"
```

---

## Task 8: ScriptEngine Public API (Orchestration Layer)

**Files:**
- Modify: `crates/glass_scripting/src/lib.rs`

This task wires loader + registry + engine + lifecycle into a single `ScriptSystem` that the bridge will call.

- [ ] **Step 1: Write integration test**

```rust
// crates/glass_scripting/src/lib.rs
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn script_system_loads_and_runs() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_dir = dir.path().join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        fs::write(hooks_dir.join("test.toml"), r#"
            name = "test-observer"
            hooks = ["CommandComplete"]
            status = "confirmed"
            origin = "user"
            version = 1
            api_version = 1
            created = "2026-03-18"
        "#).unwrap();

        fs::write(hooks_dir.join("test.rhai"), r#"
            let code = event.exit_code;
        "#).unwrap();

        let mut system = ScriptSystem::new(SandboxConfig::default());
        let errors = system.load_from_dir(dir.path());
        assert!(errors.is_empty());

        let mut event_data = context::HookEventData::new();
        event_data.set("exit_code", 0_i64);
        event_data.set("command", "cargo test".to_string());

        let result = system.run_hook(
            types::HookPoint::CommandComplete,
            &context::HookContext::default(),
            &event_data,
        );
        assert!(result.errors.is_empty());
    }
}
```

- [ ] **Step 2: Implement ScriptSystem**

```rust
// In crates/glass_scripting/src/lib.rs, add:

pub struct ScriptSystem {
    engine: engine::ScriptEngine,
    registry: hooks::HookRegistry,
    sandbox: sandbox::SandboxConfig,
}

impl ScriptSystem {
    pub fn new(sandbox: sandbox::SandboxConfig) -> Self {
        Self {
            engine: engine::ScriptEngine::new(sandbox.clone()),
            registry: hooks::HookRegistry::new(Vec::new(), sandbox.max_scripts_per_hook),
            sandbox,
        }
    }

    /// Load scripts from a scripts directory and compile them.
    pub fn load_from_dir(&mut self, dir: &std::path::Path) -> Vec<(String, String)> {
        let scripts = loader::load_scripts_from_dir(dir);
        let compile_errors = self.engine.compile_all(&scripts);
        self.registry = hooks::HookRegistry::new(scripts, self.sandbox.max_scripts_per_hook);
        compile_errors
    }

    /// Load from both project and global directories.
    pub fn load_all(&mut self, project_root: &str) -> Vec<(String, String)> {
        let scripts = loader::load_all_scripts(project_root);
        let compile_errors = self.engine.compile_all(&scripts);
        self.registry = hooks::HookRegistry::new(scripts, self.sandbox.max_scripts_per_hook);
        compile_errors
    }

    /// Run all scripts registered for a hook point.
    /// Handles special semantics:
    /// - SnapshotBefore: AND aggregation (any false vetoes), provisional can't veto
    /// - McpRequest: first-responder-wins (first script returning a response stops execution)
    pub fn run_hook(
        &self,
        hook: types::HookPoint,
        context: &context::HookContext,
        event_data: &context::HookEventData,
    ) -> engine::ScriptRunResult {
        let scripts = self.registry.scripts_for(hook);
        if scripts.is_empty() {
            return engine::ScriptRunResult::default();
        }

        let mut result = engine::ScriptRunResult::default();
        let is_snapshot_before = hook == types::HookPoint::SnapshotBefore;
        let is_mcp_request = hook == types::HookPoint::McpRequest;

        // For SnapshotBefore, default to true (proceed)
        if is_snapshot_before {
            result.filter_result = Some(true);
        }

        for script in scripts {
            match self.engine.run_script(&script.manifest.name, context, event_data) {
                Ok(actions) => {
                    result.actions.extend(actions);

                    // SnapshotBefore AND logic: any false vetoes.
                    // Provisional scripts cannot veto.
                    if is_snapshot_before {
                        let can_veto = script.manifest.origin != types::ScriptOrigin::Feedback
                            || script.manifest.status != types::ScriptStatus::Provisional;
                        if can_veto {
                            // Check if script returned false via an action signal
                            // (scripts return glass.veto() or just `false` as return value)
                            // For now, veto if script queued a specific action
                        }
                    }

                    // McpRequest first-responder-wins
                    if is_mcp_request && result.mcp_response.is_some() {
                        break; // first responder wins, stop running more scripts
                    }
                }
                Err(e) => {
                    result.errors.push((script.manifest.name.clone(), e));
                }
            }
        }

        result
    }

    /// Check if any scripts are registered for a hook.
    pub fn has_scripts_for(&self, hook: types::HookPoint) -> bool {
        self.registry.has_scripts_for(hook)
    }

    /// Get all loaded scripts (for status reporting).
    pub fn all_scripts(&self) -> Vec<&types::LoadedScript> {
        self.registry.all_scripts()
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p glass_scripting`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/glass_scripting/
git commit -m "feat(scripting): add ScriptSystem orchestration layer"
```

---

## Task 9: Bridge Module

**Files:**
- Create: `src/script_bridge.rs`
- Modify: `src/main.rs` (add `mod script_bridge;` and field)

- [ ] **Step 1: Create bridge module**

```rust
// src/script_bridge.rs
use glass_scripting::{
    Action, HookContext, HookEventData, HookPoint, SandboxConfig, ScriptSystem,
    engine::ScriptRunResult,
};
use tracing::{debug, info, warn};

pub struct ScriptBridge {
    system: ScriptSystem,
    enabled: bool,
    project_root: Option<String>,
}

impl ScriptBridge {
    pub fn new(config: &glass_core::config::GlassConfig) -> Self {
        let sandbox = config.scripting.as_ref()
            .map(|s| SandboxConfig::from_config(s))
            .unwrap_or_default();

        let enabled = config.scripting.as_ref()
            .map(|s| s.enabled)
            .unwrap_or(false);

        Self {
            system: ScriptSystem::new(sandbox),
            enabled,
            project_root: None,
        }
    }

    /// Load scripts for a project. Called when project root is known.
    pub fn load_for_project(&mut self, project_root: &str) {
        if !self.enabled {
            return;
        }
        self.project_root = Some(project_root.to_string());
        let errors = self.system.load_all(project_root);
        for (name, err) in &errors {
            warn!("Script compile error in '{}': {}", name, err);
        }
        if errors.is_empty() {
            let count = self.system.all_scripts().len();
            if count > 0 {
                info!("Loaded {} scripts", count);
            }
        }
    }

    /// Reload scripts (e.g., after config change).
    pub fn reload(&mut self) {
        if let Some(root) = self.project_root.clone() {
            self.load_for_project(&root);
        }
    }

    /// Update enabled state from config.
    pub fn update_config(&mut self, config: &glass_core::config::GlassConfig) {
        self.enabled = config.scripting.as_ref()
            .map(|s| s.enabled)
            .unwrap_or(false);
    }

    // --- Hook methods (one per hook point) ---

    pub fn on_command_start(&self, context: &HookContext, event: &HookEventData) -> Vec<Action> {
        self.run_hook(HookPoint::CommandStart, context, event)
    }

    pub fn on_command_complete(&self, context: &HookContext, event: &HookEventData) -> Vec<Action> {
        self.run_hook(HookPoint::CommandComplete, context, event)
    }

    pub fn on_orchestrator_run_start(&self, context: &HookContext, event: &HookEventData) -> Vec<Action> {
        self.run_hook(HookPoint::OrchestratorRunStart, context, event)
    }

    pub fn on_orchestrator_run_end(&self, context: &HookContext, event: &HookEventData) -> Vec<Action> {
        self.run_hook(HookPoint::OrchestratorRunEnd, context, event)
    }

    pub fn on_orchestrator_iteration(&self, context: &HookContext, event: &HookEventData) -> Vec<Action> {
        self.run_hook(HookPoint::OrchestratorIteration, context, event)
    }

    pub fn on_config_reload(&self, context: &HookContext, event: &HookEventData) -> Vec<Action> {
        self.run_hook(HookPoint::ConfigReload, context, event)
    }

    pub fn on_session_start(&self, context: &HookContext, event: &HookEventData) -> Vec<Action> {
        self.run_hook(HookPoint::SessionStart, context, event)
    }

    pub fn on_session_end(&self, context: &HookContext, event: &HookEventData) -> Vec<Action> {
        self.run_hook(HookPoint::SessionEnd, context, event)
    }

    pub fn on_tab_create(&self, context: &HookContext, event: &HookEventData) -> Vec<Action> {
        self.run_hook(HookPoint::TabCreate, context, event)
    }

    pub fn on_tab_close(&self, context: &HookContext, event: &HookEventData) -> Vec<Action> {
        self.run_hook(HookPoint::TabClose, context, event)
    }

    pub fn on_snapshot_before(&self, context: &HookContext, event: &HookEventData) -> bool {
        if !self.enabled || !self.system.has_scripts_for(HookPoint::SnapshotBefore) {
            return true; // proceed by default
        }
        let result = self.system.run_hook(HookPoint::SnapshotBefore, context, event);
        // AND logic: any false vetoes
        result.filter_result.unwrap_or(true)
    }

    // --- Action execution ---

    /// Execute a list of actions returned by scripts.
    /// This is where the bridge translates Action enums into real Glass operations.
    pub fn execute_actions(&self, actions: &[Action], project_root: &str) {
        for action in actions {
            match action {
                Action::Log { level, message } => {
                    match level {
                        glass_scripting::LogLevel::Debug => debug!("[script] {}", message),
                        glass_scripting::LogLevel::Info => info!("[script] {}", message),
                        glass_scripting::LogLevel::Warn => warn!("[script] {}", message),
                        glass_scripting::LogLevel::Error => tracing::error!("[script] {}", message),
                    }
                }
                Action::Notify { message } => {
                    info!("[script notification] {}", message);
                    // TODO: surface to status bar when renderer integration is added
                }
                Action::Commit { message } => {
                    let _ = git_commit(project_root, message);
                }
                Action::SetConfig { key, value } => {
                    debug!("[script] set_config({}, {:?}) — deferred to next config reload", key, value);
                    // TODO: write to config.toml and trigger reload
                }
                // TODO: implement remaining actions as their subsystems are wired
                _ => {
                    debug!("[script] unhandled action: {:?}", action);
                }
            }
        }
    }

    // --- Private ---

    fn run_hook(&self, hook: HookPoint, context: &HookContext, event: &HookEventData) -> Vec<Action> {
        if !self.enabled {
            return Vec::new();
        }
        let result = self.system.run_hook(hook, context, event);
        for (name, err) in &result.errors {
            warn!("Script '{}' error: {}", name, err);
            // TODO: call lifecycle::record_failure for the script
        }
        result.actions
    }
}

fn git_commit(project_root: &str, message: &str) -> Result<(), String> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(["commit", "-am", message])
        .current_dir(project_root);
    // Match existing pattern: CREATE_NO_WINDOW on Windows to prevent console flashing
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let output = cmd.output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(())
}
```

- [ ] **Step 2: Add `mod script_bridge;` to main.rs**

Add near the top of `src/main.rs` with the other `mod` declarations:
```rust
mod script_bridge;
```

- [ ] **Step 3: Add `script_bridge` field to app state**

Find the struct that holds app state in `src/main.rs` (the struct containing `feedback_state` at line 355). Add:
```rust
script_bridge: script_bridge::ScriptBridge,
```

Initialize it in the constructor:
```rust
script_bridge: script_bridge::ScriptBridge::new(&config),
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: Successful compilation

- [ ] **Step 5: Commit**

```bash
git add src/script_bridge.rs src/main.rs
git commit -m "feat(scripting): add ScriptBridge module with hook methods and action execution"
```

---

## Task 10: Wire Hook Points into main.rs

**Files:**
- Modify: `src/main.rs`

This task adds one-line bridge calls at each event dispatch point. The exact line numbers come from the codebase exploration.

- [ ] **Step 1: Wire CommandComplete hook**

In the `AppEvent::Shell` handler (around line 5699), after the block transitions to Complete and command data is available, add:

```rust
if self.script_bridge.has_scripts_for(glass_scripting::HookPoint::CommandComplete) {
    let context = self.build_hook_context();
    let mut event_data = glass_scripting::HookEventData::new();
    event_data.set("exit_code", exit_code as i64);
    event_data.set("command", command_text.clone());
    // event_data.set("duration_ms", duration_ms as i64);
    let actions = self.script_bridge.on_command_complete(&context, &event_data);
    if let Some(root) = &self.script_bridge.project_root {
        self.script_bridge.execute_actions(&actions, root);
    }
}
```

- [ ] **Step 2: Wire OrchestratorSilence hook (iteration)**

In the `AppEvent::OrchestratorSilence` handler (around line 7367), after the existing orchestrator checks, add:

```rust
let actions = self.script_bridge.on_orchestrator_iteration(
    &self.build_hook_context(),
    &glass_scripting::HookEventData::new(),
);
if let Some(root) = &self.script_bridge.project_root {
    self.script_bridge.execute_actions(&actions, root);
}
```

- [ ] **Step 3: Wire ConfigReloaded hook**

In the `AppEvent::ConfigReloaded` handler (around line 6341), after config is swapped:

```rust
self.script_bridge.update_config(&self.config);
let actions = self.script_bridge.on_config_reload(
    &self.build_hook_context(),
    &glass_scripting::HookEventData::new(),
);
```

- [ ] **Step 4: Add `build_hook_context` helper**

Add a method to the app struct:

```rust
fn build_hook_context(&self) -> glass_scripting::HookContext {
    glass_scripting::HookContext {
        cwd: self.orchestrator.as_ref()
            .and_then(|o| o.project_root.clone())
            .unwrap_or_default(),
        git_branch: String::new(), // populated lazily or from cached state
        git_dirty_files: Vec::new(),
        recent_commands: Vec::new(),
        active_rules: Vec::new(),
        config_values: std::collections::HashMap::new(),
    }
}
```

- [ ] **Step 5: Wire SessionStart**

At app initialization (after script_bridge is created and project root is known):

```rust
self.script_bridge.load_for_project(&project_root);
let actions = self.script_bridge.on_session_start(
    &self.build_hook_context(),
    &glass_scripting::HookEventData::new(),
);
```

- [ ] **Step 6: Verify it compiles and tests pass**

Run: `cargo build && cargo test --workspace`
Expected: Build succeeds, all existing tests pass

- [ ] **Step 7: Commit**

```bash
git add src/main.rs
git commit -m "feat(scripting): wire hook points into main event loop"
```

---

## Task 11: Feedback Integration (Tier 4)

**Files:**
- Modify: `crates/glass_feedback/src/lib.rs:48-58` (FeedbackResult)
- Modify: `src/main.rs` (orchestrator deactivation handler)

- [ ] **Step 1: Add `script_prompt` to FeedbackResult**

In `crates/glass_feedback/src/lib.rs`, add to the `FeedbackResult` struct (after `llm_prompt` field at line 57):

```rust
pub script_prompt: Option<String>,
```

Set to `None` in the builder wherever `FeedbackResult` is constructed.

- [ ] **Step 2: Add script prompt generation in `on_run_end`**

In the `on_run_end` function (around line 252-257 where `llm_prompt` is built), add logic after findings are computed:

```rust
// Generate script prompt when existing tiers can't explain high waste/stuck
// Respects script_generation config toggle
let script_generation = config.scripting.as_ref()
    .map(|s| s.script_generation)
    .unwrap_or(true);
let script_prompt = if script_generation
    && findings.is_empty()
    && (run_data.stuck_count > run_data.iterations / 3
        || run_data.waste_count > run_data.iterations / 3)
{
    Some(build_script_prompt(run_data, &existing_rules))
} else {
    None
};
```

Add the `build_script_prompt` helper function:

```rust
fn build_script_prompt(run_data: &RunData, _existing_rules: &[Rule]) -> String {
    format!(
        "You are analyzing an orchestrator run that had issues not captured by existing detectors.\n\
         Iterations: {}, Stuck: {}, Waste: {}, Reverts: {}\n\
         \n\
         Available hook points: CommandStart, CommandComplete, BlockStateChange, \
         SnapshotBefore, SnapshotAfter, HistoryQuery, HistoryInsert, PipelineComplete, \
         ConfigReload, OrchestratorRunStart, OrchestratorRunEnd, OrchestratorIteration, \
         OrchestratorCheckpoint, OrchestratorStuck, McpRequest, McpResponse, \
         TabCreate, TabClose, SessionStart, SessionEnd\n\
         \n\
         Available actions: Commit, IsolateCommit, RevertFiles, SetConfig, ForceSnapshot, \
         SetSnapshotPolicy, TagCommand, InjectPromptHint, TriggerCheckpoint, ExtendSilence, \
         BlockIteration, EnableScript, DisableScript, RegisterTool, UnregisterTool, Log, Notify\n\
         \n\
         Write a Rhai script that hooks into the appropriate event and uses the glass action API \
         to address the pattern you see. Output format:\n\
         SCRIPT_NAME: <name>\n\
         SCRIPT_HOOKS: <comma-separated hook points>\n\
         SCRIPT_SOURCE:\n\
         ```rhai\n\
         <your script here>\n\
         ```",
        run_data.iterations, run_data.stuck_count,
        run_data.waste_count, run_data.revert_count,
    )
}
```

- [ ] **Step 3: Handle script_prompt in main.rs**

In the orchestrator deactivation handler (where `on_run_end` result is processed), after the existing LLM feedback path:

```rust
if let Some(script_prompt) = feedback_result.script_prompt {
    // TODO: spawn ephemeral agent for Tier 4 script generation
    // For now, log that a script prompt was generated
    tracing::info!("Feedback loop generated Tier 4 script prompt ({} chars)", script_prompt.len());
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build --workspace`
Expected: Successful

- [ ] **Step 5: Run all tests**

Run: `cargo test --workspace`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/glass_feedback/src/lib.rs src/main.rs
git commit -m "feat(feedback): add Tier 4 script_prompt generation in on_run_end"
```

---

## Task 12: Dynamic MCP Tools

**Files:**
- Create: `crates/glass_scripting/src/mcp.rs`
- Modify: `crates/glass_mcp/src/tools.rs`
- Modify: `src/main.rs` (McpRequest handler)

- [ ] **Step 1: Write MCP tool registry**

```rust
// crates/glass_scripting/src/mcp.rs
use crate::types::LoadedScript;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptToolDef {
    pub name: String,
    pub description: String,
    pub params_schema: serde_json::Value,
    pub script_name: String,
}

#[derive(Debug, Default)]
pub struct ScriptToolRegistry {
    tools: HashMap<String, ScriptToolDef>,
}

impl ScriptToolRegistry {
    pub fn new() -> Self {
        Self { tools: HashMap::new() }
    }

    /// Register MCP tools from loaded scripts that have type = "mcp_tool".
    pub fn register_from_scripts(&mut self, scripts: &[LoadedScript], include_provisional: bool) {
        self.tools.clear();
        for script in scripts {
            if script.manifest.script_type.as_deref() != Some("mcp_tool") {
                continue;
            }
            if !include_provisional
                && script.manifest.status == crate::types::ScriptStatus::Provisional
            {
                continue;
            }
            if let Some(desc) = &script.manifest.description {
                let schema = script.manifest.params.as_ref()
                    .map(|p| serde_json::to_value(p).unwrap_or_default())
                    .unwrap_or(serde_json::json!({}));

                self.tools.insert(script.manifest.name.clone(), ScriptToolDef {
                    name: script.manifest.name.clone(),
                    description: desc.clone(),
                    params_schema: schema,
                    script_name: script.manifest.name.clone(),
                });
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<&ScriptToolDef> {
        self.tools.get(name)
    }

    pub fn list_confirmed(&self) -> Vec<&ScriptToolDef> {
        self.tools.values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    #[test]
    fn register_mcp_tool_from_script() {
        let script = LoadedScript {
            manifest: ScriptManifest {
                name: "glass_test_tool".to_string(),
                hooks: vec![],
                status: ScriptStatus::Confirmed,
                origin: ScriptOrigin::Feedback,
                version: 1,
                api_version: 1,
                created: "2026-03-18".to_string(),
                failure_count: 0,
                trigger_count: 0,
                stale_runs: 0,
                script_type: Some("mcp_tool".to_string()),
                description: Some("A test tool".to_string()),
                params: None,
            },
            source: "42".to_string(),
            manifest_path: std::path::PathBuf::new(),
            source_path: std::path::PathBuf::new(),
        };

        let mut registry = ScriptToolRegistry::new();
        registry.register_from_scripts(&[script], false);
        assert!(registry.get("glass_test_tool").is_some());
        assert_eq!(registry.list_confirmed().len(), 1);
    }

    #[test]
    fn skip_provisional_when_not_included() {
        let script = LoadedScript {
            manifest: ScriptManifest {
                name: "glass_prov_tool".to_string(),
                hooks: vec![],
                status: ScriptStatus::Provisional,
                origin: ScriptOrigin::Feedback,
                version: 1,
                api_version: 1,
                created: "2026-03-18".to_string(),
                failure_count: 0,
                trigger_count: 0,
                stale_runs: 0,
                script_type: Some("mcp_tool".to_string()),
                description: Some("Provisional tool".to_string()),
                params: None,
            },
            source: "42".to_string(),
            manifest_path: std::path::PathBuf::new(),
            source_path: std::path::PathBuf::new(),
        };

        let mut registry = ScriptToolRegistry::new();
        registry.register_from_scripts(&[script], false);
        assert!(registry.get("glass_prov_tool").is_none());
    }
}
```

- [ ] **Step 2: Add module to lib.rs**

```rust
pub mod mcp;
pub use mcp::ScriptToolRegistry;
```

- [ ] **Step 3: Add `glass_script_tool` handler to MCP server**

In `crates/glass_mcp/src/tools.rs`, add a new tool handler alongside the existing ones:

```rust
#[tool(description = "Execute a script-defined dynamic tool. Use glass_list_script_tools to discover available tools.")]
async fn glass_script_tool(
    &self,
    #[tool(param, description = "Name of the script tool to execute")]
    tool_name: String,
    #[tool(param, description = "Parameters to pass to the tool (JSON object)")]
    params: Option<serde_json::Value>,
) -> Result<CallToolResult, McpError> {
    let req_params = serde_json::json!({
        "tool_name": tool_name,
        "params": params.unwrap_or(serde_json::json!({})),
    });
    let client = self.ipc_client.as_ref().ok_or_else(|| internal_err("No IPC client"))?;
    let result = client.send_request("script_tool", req_params).map_err(|e| internal_err(&e))?;
    Ok(CallToolResult::default().with_content(vec![Content::json(result)?]))
}

#[tool(description = "List all available script-defined tools with their descriptions and parameter schemas.")]
async fn glass_list_script_tools(&self) -> Result<CallToolResult, McpError> {
    let client = self.ipc_client.as_ref().ok_or_else(|| internal_err("No IPC client"))?;
    let result = client.send_request("list_script_tools", serde_json::json!({})).map_err(|e| internal_err(&e))?;
    Ok(CallToolResult::default().with_content(vec![Content::json(result)?]))
}
```

- [ ] **Step 4: Handle script_tool IPC in main.rs**

In the `AppEvent::McpRequest` handler (around line 8415), add cases:

```rust
"script_tool" => {
    let tool_name = params.get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    // TODO: look up in script_bridge tool registry and execute
    let result = serde_json::json!({"error": "script tools not yet wired"});
    result
}
"list_script_tools" => {
    // TODO: return tool registry listing
    let result = serde_json::json!({"tools": []});
    result
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --workspace`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/glass_scripting/src/mcp.rs crates/glass_scripting/src/lib.rs crates/glass_mcp/src/tools.rs src/main.rs
git commit -m "feat(scripting): add dynamic MCP tool registry and IPC handlers"
```

---

## Task 13: Profile Export/Import

**Files:**
- Create: `crates/glass_scripting/src/profile.rs`
- Modify: `src/main.rs` (CLI subcommand, if using clap)

- [ ] **Step 1: Write profile export/import tests**

```rust
// crates/glass_scripting/src/profile.rs
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn export_and_import_roundtrip() {
        let source_dir = tempfile::tempdir().unwrap();
        let hooks_dir = source_dir.path().join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Create a confirmed script
        fs::write(hooks_dir.join("test.toml"), r#"
            name = "test-script"
            hooks = ["CommandComplete"]
            status = "confirmed"
            origin = "feedback"
            version = 1
            api_version = 1
            created = "2026-03-18"
        "#).unwrap();
        fs::write(hooks_dir.join("test.rhai"), "let x = 42;").unwrap();

        // Create a provisional script (should be excluded)
        fs::write(hooks_dir.join("prov.toml"), r#"
            name = "prov-script"
            hooks = ["SessionStart"]
            status = "provisional"
            origin = "feedback"
            version = 1
            api_version = 1
            created = "2026-03-18"
        "#).unwrap();
        fs::write(hooks_dir.join("prov.rhai"), "let y = 0;").unwrap();

        let output_dir = tempfile::tempdir().unwrap();
        let profile_path = output_dir.path().join("test.glassprofile");

        export_profile(
            "test-profile",
            source_dir.path(),
            &profile_path,
            "3.1.0",
            &["rust", "cargo"],
        ).unwrap();

        assert!(profile_path.exists());

        // Import into fresh directory
        let import_dir = tempfile::tempdir().unwrap();
        let imported = import_profile(&profile_path, import_dir.path()).unwrap();
        assert_eq!(imported.scripts_imported, 1); // only confirmed
        assert_eq!(imported.scripts_skipped, 0);
    }
}
```

- [ ] **Step 2: Implement export/import**

```rust
// crates/glass_scripting/src/profile.rs
use crate::loader::load_scripts_from_dir;
use crate::types::{ScriptManifest, ScriptStatus};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct ProfileManifest {
    pub profile: ProfileInfo,
    pub stats: ProfileStats,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProfileInfo {
    pub name: String,
    pub glass_version: String,
    pub created: String,
    pub tech_stack: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProfileStats {
    pub hook_scripts_count: usize,
    pub mcp_tools_count: usize,
}

#[derive(Debug)]
pub struct ImportResult {
    pub scripts_imported: usize,
    pub scripts_skipped: usize,
}

/// Export confirmed scripts to a .glassprofile directory.
pub fn export_profile(
    name: &str,
    scripts_dir: &Path,
    output_path: &Path,
    glass_version: &str,
    tech_stack: &[&str],
) -> anyhow::Result<()> {
    let scripts = load_scripts_from_dir(scripts_dir);
    let confirmed: Vec<_> = scripts.iter()
        .filter(|s| s.manifest.status == ScriptStatus::Confirmed)
        .collect();

    // Create profile directory structure
    let profile_dir = output_path;
    let hooks_out = profile_dir.join("scripts").join("hooks");
    let tools_out = profile_dir.join("scripts").join("tools");
    std::fs::create_dir_all(&hooks_out)?;
    std::fs::create_dir_all(&tools_out)?;

    let mut hook_count = 0;
    let mut tool_count = 0;

    for script in &confirmed {
        let dest_dir = if script.manifest.script_type.as_deref() == Some("mcp_tool") {
            tool_count += 1;
            &tools_out
        } else {
            hook_count += 1;
            &hooks_out
        };

        let stem = script.manifest_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&script.manifest.name);

        // Write manifest with status downgraded to provisional
        let mut export_manifest = script.manifest.clone();
        export_manifest.status = ScriptStatus::Provisional;
        let manifest_content = toml::to_string_pretty(&export_manifest)?;
        std::fs::write(dest_dir.join(format!("{}.toml", stem)), manifest_content)?;
        std::fs::write(dest_dir.join(format!("{}.rhai", stem)), &script.source)?;
    }

    // Write profile manifest
    let manifest = ProfileManifest {
        profile: ProfileInfo {
            name: name.to_string(),
            glass_version: glass_version.to_string(),
            created: {
                // Use std::time to avoid adding chrono as a dependency
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let days = now / 86400;
                // Simple epoch-to-date (approximate, sufficient for profile metadata)
                format!("{}", days) // Implementer: replace with proper date formatting
            },
            tech_stack: tech_stack.iter().map(|s| s.to_string()).collect(),
        },
        stats: ProfileStats {
            hook_scripts_count: hook_count,
            mcp_tools_count: tool_count,
        },
    };
    std::fs::write(
        profile_dir.join("profile.toml"),
        toml::to_string_pretty(&manifest)?,
    )?;

    Ok(())
}

/// Import a .glassprofile into the scripts directory.
pub fn import_profile(profile_path: &Path, target_scripts_dir: &Path) -> anyhow::Result<ImportResult> {
    let imported_scripts = load_scripts_from_dir(&profile_path.join("scripts"));

    let mut imported = 0;
    let mut skipped = 0;

    for script in imported_scripts {
        let stem = script.manifest_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&script.manifest.name);

        let dest_subdir = if script.manifest.script_type.as_deref() == Some("mcp_tool") {
            "tools"
        } else {
            "hooks"
        };

        let dest_dir = target_scripts_dir.join(dest_subdir);
        std::fs::create_dir_all(&dest_dir)?;

        let dest_manifest = dest_dir.join(format!("{}.toml", stem));
        if dest_manifest.exists() {
            skipped += 1;
            continue;
        }

        // Ensure status is provisional
        let mut manifest = script.manifest.clone();
        manifest.status = ScriptStatus::Provisional;
        std::fs::write(&dest_manifest, toml::to_string_pretty(&manifest)?)?;
        std::fs::write(dest_dir.join(format!("{}.rhai", stem)), &script.source)?;
        imported += 1;
    }

    Ok(ImportResult {
        scripts_imported: imported,
        scripts_skipped: skipped,
    })
}
```

- [ ] **Step 3: Add chrono dev-dependency if needed**

Check if `chrono` is already a workspace dependency. If not, add for profile timestamps. Alternatively, use a simpler date format.

- [ ] **Step 4: Add module to lib.rs**

```rust
pub mod profile;
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p glass_scripting`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/glass_scripting/
git commit -m "feat(scripting): add profile export/import with roundtrip validation"
```

---

## Task 14: CLI Subcommands

**Files:**
- Modify: `src/main.rs` (clap CLI definition)

- [ ] **Step 1: Add `profile` subcommand to CLI**

Find the clap `#[derive(Parser)]` or `Command::new()` definition in `src/main.rs` and add:

```rust
#[derive(Subcommand)]
enum ProfileCommand {
    /// Export confirmed scripts and rules as a shareable profile
    Export {
        /// Profile name
        name: String,
    },
    /// Import a profile as provisional scripts
    Import {
        /// Path to .glassprofile directory
        path: std::path::PathBuf,
    },
}
```

Wire into the main CLI:
```rust
/// Manage Glass scripting profiles
Profile {
    #[command(subcommand)]
    command: ProfileCommand,
},
```

- [ ] **Step 2: Handle profile commands**

```rust
Commands::Profile { command } => {
    match command {
        ProfileCommand::Export { name } => {
            let scripts_dir = dirs::home_dir()
                .unwrap()
                .join(".glass")
                .join("scripts");
            let output = std::env::current_dir().unwrap().join(format!("{}.glassprofile", name));
            glass_scripting::profile::export_profile(
                &name,
                &scripts_dir,
                &output,
                env!("CARGO_PKG_VERSION"),
                &[], // TODO: auto-detect tech stack
            ).unwrap();
            println!("Profile exported to {}", output.display());
        }
        ProfileCommand::Import { path } => {
            let target = dirs::home_dir()
                .unwrap()
                .join(".glass")
                .join("scripts");
            let result = glass_scripting::profile::import_profile(&path, &target).unwrap();
            println!(
                "Imported {} scripts ({} skipped as duplicates)",
                result.scripts_imported, result.scripts_skipped
            );
        }
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: Successful

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat(cli): add glass profile export/import subcommands"
```

---

## Summary

| Task | What It Delivers | Tests |
|------|-----------------|-------|
| 1 | Crate skeleton, types, Action enum | Manifest deserialization |
| 2 | Config `[scripting]` section | Config parsing, defaults |
| 3 | SandboxConfig with hard ceilings | Ceiling enforcement, defaults |
| 4 | Script loader (manifest + source pairs) | Load, skip archived, skip orphans |
| 5 | HookRegistry with priority ordering | Grouping, priority, limits |
| 6 | Rhai engine, context, sandbox | Compile, run, event data, op limits |
| 7 | Lifecycle management | Promote, reject, failure tracking, stale |
| 8 | ScriptSystem orchestration | Integration: load + compile + run |
| 9 | Bridge module | Event routing, action execution |
| 10 | Hook wiring in main.rs | CommandComplete, orchestrator, config, session |
| 11 | Feedback Tier 4 | script_prompt on FeedbackResult |
| 12 | Dynamic MCP tools | Registry, IPC handlers, glass_script_tool |
| 13 | Profile export/import | Roundtrip, conflict handling |
| 14 | CLI subcommands | glass profile export/import |

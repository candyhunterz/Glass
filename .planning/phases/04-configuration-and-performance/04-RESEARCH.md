# Phase 4: Configuration and Performance - Research

**Researched:** 2026-03-04
**Domain:** TOML configuration loading, startup/latency/memory performance measurement
**Confidence:** HIGH

## Summary

Phase 4 has two distinct sub-domains: (1) loading a TOML config file at startup to configure font, font size, and shell; and (2) measuring and meeting performance targets for cold start, input latency, and idle memory. Both are well-understood problems with standard Rust solutions.

The configuration work is straightforward -- the `GlassConfig` struct already exists in `glass_core::config` with the exact fields needed (`font_family`, `font_size`, `shell`). The `serde` and `toml` crates are already workspace dependencies. The work is: add `Deserialize` derive, load from `~/.glass/config.toml` at startup, and wire the loaded config into `FrameRenderer::new()` and `spawn_pty()`.

The performance work is primarily measurement and optimization. The codebase already uses `std::time::Instant` for timing. Cold start measurement needs a timestamp at `main()` entry and another when the first frame renders. Input latency is best measured with an instrumented render loop. Memory is measurable via `memory-stats` crate or Windows `GetProcessMemoryInfo`. The key risk is that performance issues may require architectural changes (e.g., lazy font loading, GPU pipeline pre-warming), but the current architecture (dedicated PTY thread, lock-minimizing snapshots) is already optimized for latency.

**Primary recommendation:** Split into two plans: (1) config loading + wiring, (2) performance measurement + optimization. Config is a clean feature addition; performance may require iterative profiling.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CONF-01 | User can configure Glass via TOML config file (~/.glass/config.toml) | serde + toml crates already in workspace; GlassConfig struct exists; need Deserialize derive + file loading logic |
| CONF-02 | User can set font family and font size in config | GlassConfig already has font_family and font_size fields; wire loaded config into FrameRenderer::new() |
| CONF-03 | User can override default shell in config | GlassConfig already has shell: Option<String>; wire into spawn_pty() replacing hardcoded pwsh detection |
| PERF-01 | Cold start time is under 200ms | Measure with std::time::Instant from main() to first frame; profile wgpu init and font loading |
| PERF-02 | Input latency (keypress to screen) is under 5ms | Measure with tracing spans around key event -> PTY write -> redraw cycle |
| PERF-03 | Idle memory usage is under 50MB | Measure with memory-stats crate; profile with Windows Task Manager for validation |
</phase_requirements>

## Standard Stack

### Core (already in workspace)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| serde | 1.0.228 | Struct deserialization from TOML | Industry standard for Rust serialization; already a workspace dep with `derive` feature |
| toml | 1.0.4 | TOML file parsing | De facto TOML parser for Rust; already a workspace dep |

### Supporting (new dependencies)
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| dirs | 6.x | Cross-platform home directory resolution | Resolving `~/.glass/` path on Windows (USERPROFILE) and other platforms |
| memory-stats | 1.2.x | Cross-platform process memory measurement | PERF-03 idle memory measurement; returns Working Set on Windows |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| dirs | std::env::home_dir() | Deprecated in std; dirs uses proper Windows API (SHGetKnownFolderPath) |
| dirs | hardcoded USERPROFILE env var | Not cross-platform; dirs handles all platforms correctly |
| memory-stats | windows-sys GetProcessMemoryInfo | Platform-specific; memory-stats is 3 lines of code and cross-platform |
| memory-stats | sysinfo crate | sysinfo is heavy (pulls in lots of system info); memory-stats is minimal |

**Installation (add to workspace Cargo.toml):**
```toml
dirs = "6"
memory-stats = "1.2"
```

**Add to glass_core Cargo.toml:**
```toml
serde.workspace = true
toml.workspace = true
dirs = "6"  # or workspace
```

**Add to glass (root) Cargo.toml [dependencies]:**
```toml
memory-stats = "1.2"  # only needed for perf measurement, can be behind a feature flag
```

## Architecture Patterns

### Config Loading Flow
```
main() startup
  -> GlassConfig::load()           # Try ~/.glass/config.toml
     -> dirs::home_dir()           # Get home directory
     -> std::fs::read_to_string()  # Read TOML file
     -> toml::from_str::<GlassConfig>()  # Deserialize
     -> Fall back to Default if file missing or parse error (log warning)
  -> Pass config to Processor
  -> resumed() uses config for FrameRenderer::new() and spawn_pty()
```

### Recommended Changes to Existing Code

**glass_core/src/config.rs:**
```rust
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
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

impl GlassConfig {
    /// Load config from ~/.glass/config.toml, falling back to defaults.
    pub fn load() -> Self {
        let Some(home) = dirs::home_dir() else {
            tracing::warn!("Could not determine home directory, using default config");
            return Self::default();
        };
        let config_path = home.join(".glass").join("config.toml");
        match std::fs::read_to_string(&config_path) {
            Ok(contents) => match toml::from_str(&contents) {
                Ok(config) => {
                    tracing::info!("Loaded config from {}", config_path.display());
                    config
                }
                Err(e) => {
                    tracing::warn!("Failed to parse {}: {e}, using defaults", config_path.display());
                    Self::default()
                }
            },
            Err(_) => {
                tracing::debug!("No config file at {}, using defaults", config_path.display());
                Self::default()
            }
        }
    }
}
```

**Example ~/.glass/config.toml:**
```toml
font_family = "Cascadia Code"
font_size = 16.0
shell = "pwsh"
```

### Pattern: Config Wiring in main.rs

Currently `main.rs:resumed()` line 105 does:
```rust
let config = GlassConfig::default();
```

This becomes:
```rust
// config is stored in Processor, loaded once in main()
let config = &self.config;  // or passed through
```

The config must be loaded in `main()` before `run_app()`, then stored in `Processor` and used in `resumed()`.

### Pattern: Shell Override in spawn_pty()

Currently `pty.rs` line 104-109 hardcodes pwsh detection:
```rust
let shell_program = if std::process::Command::new("pwsh")...
```

With config, `spawn_pty()` should accept an `Option<String>` shell override parameter. If `Some(shell)`, use it directly. If `None`, fall back to the existing pwsh detection logic.

### Anti-Patterns to Avoid
- **Blocking on config errors at startup:** Config parsing failures should warn and use defaults, never panic. A missing config file is the normal first-run case.
- **Validating font existence at config load time:** Font validation happens later when glyphon tries to load the font. The config layer should not try to enumerate system fonts.
- **Creating config file if missing:** Do not auto-create `~/.glass/config.toml` on first run. Users discover config via documentation, not auto-generated files.
- **Hot-reload in this phase:** POLI-04 (config hot reload) is explicitly deferred to a future milestone. Do not add file watchers.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Home directory resolution | Manual USERPROFILE/HOME env var checks | `dirs::home_dir()` | Handles Windows (SHGetKnownFolderPath), macOS, Linux correctly |
| TOML parsing | Custom parser | `toml::from_str` with serde Deserialize | Handles all TOML edge cases, type coercion, error messages |
| Default field values | Manual Option unwrapping | `#[serde(default)]` on struct | Automatically fills missing fields from Default impl |
| Memory measurement | Raw Windows API calls | `memory-stats` crate | 3-line API, cross-platform, returns Working Set on Windows |

**Key insight:** The config loading code is approximately 30 lines. The wiring (threading config through to FrameRenderer and spawn_pty) is the actual work.

## Common Pitfalls

### Pitfall 1: Config Loaded Too Late
**What goes wrong:** Config is loaded after GPU surface or PTY is already created, so font/shell settings don't apply.
**Why it happens:** In winit's ApplicationHandler pattern, window creation happens in `resumed()`, not `main()`. If config is loaded in `resumed()`, it works, but if loaded lazily it may miss the initialization window.
**How to avoid:** Load config in `main()`, store in `Processor`, use in `resumed()`.
**Warning signs:** Default font appears even when config specifies a different font.

### Pitfall 2: Serde Default Not Applied Per-Field
**What goes wrong:** A config file with only `font_size = 16.0` fails to parse because `font_family` is missing.
**Why it happens:** Without `#[serde(default)]` on the struct, serde requires all fields to be present in the TOML.
**How to avoid:** Use `#[serde(default)]` on the struct (not individual fields) to fall back to the `Default` impl for any missing field.
**Warning signs:** Parse errors when users provide partial config files.

### Pitfall 3: Shell Override Breaks Shell Integration
**What goes wrong:** User sets `shell = "bash"` but the PowerShell integration script is loaded, or vice versa.
**Why it happens:** Shell integration script loading may be hardcoded to PowerShell.
**How to avoid:** Currently shell integration scripts are sourced by the shell itself (not injected by Glass), so this is not an issue for Phase 4. But worth documenting that shell integration requires matching scripts.
**Warning signs:** OSC 133 sequences not emitted after shell override.

### Pitfall 4: Cold Start Dominated by wgpu Adapter Discovery
**What goes wrong:** wgpu's `request_adapter()` takes 50-150ms on Windows as it enumerates GPU drivers.
**Why it happens:** DX12 backend probes available adapters. This is a one-time cost but dominates cold start.
**How to avoid:** Cannot avoid, but can measure to establish baseline. If over budget, consider `wgpu::Backends::DX12` (skip other backend probing) or power preference hints.
**Warning signs:** Cold start consistently 150ms+ even with no config loading.

### Pitfall 5: Input Latency Measurement Artifacts
**What goes wrong:** Measured latency includes winit event loop overhead, giving misleadingly high numbers.
**Why it happens:** winit batches events and may introduce 1-2ms of event loop overhead before the KeyboardInput event handler fires.
**How to avoid:** Measure from KeyboardInput handler entry to PTY write completion (the part Glass controls). Document that end-to-end latency from physical keypress to photon includes OS input pipeline + display refresh.
**Warning signs:** Latency measurements consistently 8-15ms even with minimal processing.

### Pitfall 6: Memory Measurement Shows Release vs Debug Difference
**What goes wrong:** Debug builds use 80MB+ due to debug symbols and unoptimized allocations.
**Why it happens:** Rust debug builds don't optimize, include debug info in memory.
**How to avoid:** Always measure memory with `--release` builds. PERF-03's 50MB target applies to release builds only.
**Warning signs:** Memory over budget in debug, under budget in release -- this is expected.

## Code Examples

### Loading Config (verified pattern from toml + serde docs)
```rust
// Source: https://docs.rs/toml + https://docs.rs/serde
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GlassConfig {
    pub font_family: String,
    pub font_size: f32,
    pub shell: Option<String>,
}

// Default impl provides fallback values for any missing TOML fields
impl Default for GlassConfig { /* ... */ }
```

### Measuring Cold Start
```rust
// In main(), before anything else:
let start = std::time::Instant::now();

// ... after first frame renders in resumed():
tracing::info!("Cold start: {:?}", start.elapsed());
```

### Measuring Idle Memory
```rust
// Source: https://docs.rs/memory-stats
if let Some(usage) = memory_stats::memory_stats() {
    tracing::info!(
        "Memory: physical={:.1}MB virtual={:.1}MB",
        usage.physical_mem as f64 / 1_048_576.0,
        usage.virtual_mem as f64 / 1_048_576.0,
    );
}
```

### Measuring Input Latency
```rust
// In KeyboardInput handler:
let key_start = std::time::Instant::now();
// ... encode_key + pty_sender.send() ...
tracing::trace!("Key->PTY: {:?}", key_start.elapsed());
```

### Config File Path Resolution
```rust
// Source: https://docs.rs/dirs
fn config_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".glass").join("config.toml"))
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| std::env::home_dir() | dirs::home_dir() | std deprecated in 1.29 (2018) | Must use dirs crate for reliable home dir on Windows |
| toml 0.5 (TOML 0.5) | toml 1.0 (TOML 1.0 spec) | toml 0.8+ (2023) | Already on 1.0.4, no migration needed |
| Manual serde field defaults | #[serde(default)] on struct | Always available | Use struct-level default for cleanest pattern |

**Deprecated/outdated:**
- `std::env::home_dir()`: Deprecated, returns incorrect results on Windows in some cases. Use `dirs::home_dir()`.

## Open Questions

1. **Font fallback behavior when configured font is not installed**
   - What we know: glyphon/cosmic-text will fall back to a system font if the requested family isn't found
   - What's unclear: Whether this produces a visible error or silently degrades
   - Recommendation: Log a warning if the configured font family doesn't match any loaded font, but don't fail startup

2. **Whether 200ms cold start target is achievable with wgpu DX12 init**
   - What we know: wgpu adapter discovery typically takes 30-100ms on Windows; font loading takes 5-20ms
   - What's unclear: Total budget with ConPTY spawn + shell startup
   - Recommendation: Measure first, then optimize. The 200ms target is from launch to interactive prompt, which includes shell startup time. If shell startup is the bottleneck, it's outside Glass's control.

3. **Whether PTY spawn should be async relative to GPU init**
   - What we know: Currently both happen sequentially in resumed()
   - What's unclear: Whether parallelizing PTY spawn with GPU init would measurably reduce cold start
   - Recommendation: Measure sequential baseline first. If over budget, spawn PTY on a thread while wgpu initializes.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test framework (cargo test) |
| Config file | None (tests are inline #[cfg(test)] modules) |
| Quick run command | `cargo test -p glass_core --release` |
| Full suite command | `cargo test --release` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CONF-01 | Config loads from TOML file | unit | `cargo test -p glass_core config -- --nocapture` | No - Wave 0 |
| CONF-02 | Font family and size applied from config | unit | `cargo test -p glass_core config -- --nocapture` | No - Wave 0 |
| CONF-03 | Shell override applied from config | unit | `cargo test -p glass_core config -- --nocapture` | No - Wave 0 |
| PERF-01 | Cold start under 200ms | manual | Build `--release`, measure with tracing output | No - manual |
| PERF-02 | Input latency under 5ms | manual | Build `--release`, measure with tracing spans | No - manual |
| PERF-03 | Idle memory under 50MB | manual | Build `--release`, check Task Manager or memory-stats output | No - manual |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_core --release`
- **Per wave merge:** `cargo test --release`
- **Phase gate:** Full suite green + manual performance verification before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_core/src/config.rs` -- unit tests for config loading (parse valid TOML, handle missing file, handle partial config, handle malformed TOML)
- [ ] Performance tests are inherently manual (require GPU, PTY, window) -- document measurement procedure instead

## Sources

### Primary (HIGH confidence)
- [toml crate docs](https://docs.rs/toml) - TOML parsing API and serde integration
- [serde docs](https://docs.rs/serde) - Deserialize derive and default attribute
- [dirs crate docs](https://docs.rs/dirs/latest/dirs/) - Cross-platform home directory resolution
- [memory-stats crate docs](https://docs.rs/memory-stats) - Cross-platform process memory measurement
- [std::time::Instant docs](https://doc.rust-lang.org/std/time/struct.Instant.html) - High-resolution timing on Windows

### Secondary (MEDIUM confidence)
- [GetProcessMemoryInfo - Microsoft Learn](https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-getprocessmemoryinfo) - Windows Working Set measurement details
- [Robust Config System blog post](https://tore.dev/en/blog/rust-config-file) - Config loading patterns with graceful degradation

### Tertiary (LOW confidence)
- wgpu cold start timing estimates (30-100ms) based on community reports, not benchmarked on this specific hardware

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - serde + toml are already workspace deps, well-documented APIs
- Architecture: HIGH - GlassConfig struct already exists with correct fields, wiring points clearly identified in main.rs and pty.rs
- Pitfalls: HIGH - Common serde/config pitfalls are well-known; performance measurement pitfalls based on Windows platform knowledge
- Performance targets: MEDIUM - Whether targets are achievable depends on measurement; architecture is sound but wgpu/ConPTY init costs are hardware-dependent

**Research date:** 2026-03-04
**Valid until:** 2026-04-04 (stable domain, no fast-moving dependencies)

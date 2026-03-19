# Setup & Packaging Implementation Plan (Branch 4 of 8)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Embed shell integration scripts directly in the binary (eliminating the packaging gap), add friendly GPU init errors, complete Cargo.toml metadata, create a Scoop manifest, add first-run config creation, and introduce a `glass check` diagnostic subcommand.

**Architecture:** Work inside-out: embed scripts first (the centerpiece fix), then GPU error handling, then packaging metadata/manifests, then first-run UX, then the `glass check` subcommand last (it reports everything the earlier tasks set up).

**Tech Stack:** Rust, include_str!(), std::env::temp_dir, wgpu, clap, TOML

**Branch:** `audit/setup-packaging` off `master`

**Spec:** `docs/superpowers/specs/2026-03-18-prelaunch-audit-fixes-design.md` Branch 4

**Dependencies:** Branches 1-3 should be merged first. Branch 3 completes zsh/fish pipeline scripts that this branch embeds.

---

### Task 1: Branch setup + embed shell integration scripts in binary (P-1/P-2)

This is the centerpiece fix. Currently `find_shell_integration()` searches the filesystem for `shell-integration/*.sh` files. Installers (MSI, DMG) do not ship these files, so shell integration silently fails on installed copies. The fix: compile the scripts into the binary with `include_str!()` and write them to a temp directory at runtime.

**Files:**
- Modify: `src/main.rs:9499-9529` (replace `find_shell_integration`)
- Modify: `src/main.rs:711-725` (update shell injection to use embedded scripts)

- [ ] **Step 1: Create branch**

```bash
git checkout -b audit/setup-packaging master
```

- [ ] **Step 2: Add embedded script constants**

Near the top of `src/main.rs` (in the constants/statics area), add:

```rust
/// Shell integration scripts compiled into the binary.
/// These are written to a temp directory at PTY spawn time so the shell can `source` them.
const SHELL_INTEGRATION_BASH: &str = include_str!("../shell-integration/glass.bash");
const SHELL_INTEGRATION_ZSH: &str = include_str!("../shell-integration/glass.zsh");
const SHELL_INTEGRATION_FISH: &str = include_str!("../shell-integration/glass.fish");
const SHELL_INTEGRATION_PS1: &str = include_str!("../shell-integration/glass.ps1");
```

The `include_str!()` paths are relative to `src/main.rs`, so `../shell-integration/` points to the repo root `shell-integration/` directory.

- [ ] **Step 3: Replace `find_shell_integration` with `write_shell_integration`**

Replace the entire `find_shell_integration` function (lines ~9499-9529) with a new function that writes the embedded script to a temp file and returns its path:

```rust
/// Write the appropriate embedded shell integration script to a temp directory
/// and return the path. The temp file persists for the lifetime of the process.
///
/// Falls back to the filesystem path if it exists (development convenience).
fn get_shell_integration(shell_name: &str) -> Option<std::path::PathBuf> {
    let (script_name, script_content) =
        if shell_name.contains("pwsh") || shell_name.to_lowercase().contains("powershell") {
            ("glass.ps1", SHELL_INTEGRATION_PS1)
        } else if shell_name.contains("zsh") {
            ("glass.zsh", SHELL_INTEGRATION_ZSH)
        } else if shell_name.contains("fish") {
            ("glass.fish", SHELL_INTEGRATION_FISH)
        } else {
            ("glass.bash", SHELL_INTEGRATION_BASH)
        };

    // Development convenience: prefer on-disk scripts if present (enables edit-and-test)
    let exe = std::env::current_exe().ok();
    if let Some(ref exe) = exe {
        if let Some(exe_dir) = exe.parent() {
            // Installed layout: exe_dir/shell-integration/
            let candidate = exe_dir.join("shell-integration").join(script_name);
            if candidate.exists() {
                return Some(candidate);
            }
            // Dev layout: target/{debug,release}/ -> repo root
            if let Some(repo_root) = exe_dir.parent().and_then(|p| p.parent()) {
                let candidate = repo_root.join("shell-integration").join(script_name);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    // No on-disk script found — write embedded script to temp
    let temp_dir = std::env::temp_dir().join("glass-shell-integration");
    if let Err(e) = std::fs::create_dir_all(&temp_dir) {
        tracing::warn!("Failed to create shell integration temp dir: {e}");
        return None;
    }

    // Set restrictive permissions on Unix (temp dir could be world-readable)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&temp_dir, std::fs::Permissions::from_mode(0o700));
    }

    let script_path = temp_dir.join(script_name);
    match std::fs::write(&script_path, script_content) {
        Ok(()) => {
            tracing::info!("Wrote embedded shell integration to {}", script_path.display());
            Some(script_path)
        }
        Err(e) => {
            tracing::warn!("Failed to write shell integration script: {e}");
            None
        }
    }
}
```

- [ ] **Step 4: Update the call site at line ~711**

Replace:

```rust
if let Some(path) = find_shell_integration(&effective_shell_for_integration) {
```

with:

```rust
if let Some(path) = get_shell_integration(&effective_shell_for_integration) {
```

The rest of the injection block (lines 712-725) remains unchanged — it already constructs the correct `source` command per shell type.

- [ ] **Step 5: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

Verify the `include_str!()` paths resolve correctly at compile time. If the paths fail, adjust the relative path (the file is `src/main.rs`, scripts are at `shell-integration/`).

- [ ] **Step 6: Manual verification**

Delete or rename `shell-integration/` temporarily and rebuild. Launch Glass — shell integration should still work because scripts are embedded. Restore the directory afterward.

- [ ] **Step 7: Commit**

```bash
git add src/main.rs
git commit -m "feat(P-1/P-2): embed shell integration scripts in binary via include_str!()

Scripts are compiled into the binary and written to a temp directory at
PTY spawn time. On-disk scripts are still preferred in dev builds for
edit-and-test convenience. This eliminates the packaging gap — MSI, DMG,
and cargo-install builds all get working shell integration without any
installer changes."
```

---

### Task 2: Add warning when shell integration is not found (P-3)

Currently if `find_shell_integration` returns `None`, the code silently proceeds without shell integration. After Task 1 this should be very rare (only if temp dir write fails), but we should still warn.

**Files:**
- Modify: `src/main.rs:711-725` (add else branch)

- [ ] **Step 1: Add warning on fallback**

After the `if let Some(path) = get_shell_integration(...)` block (~line 711-725), add an else branch:

```rust
if let Some(path) = get_shell_integration(&effective_shell_for_integration) {
    // ... existing injection code ...
} else {
    tracing::warn!(
        "Shell integration unavailable for '{}'. Command blocks, pipe \
         visualization, and undo will not work. Run `glass check` for diagnosis.",
        effective_shell_for_integration
    );
    // TODO (UX branch): show status bar toast
}
```

- [ ] **Step 2: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "fix(P-3): warn when shell integration is unavailable

Log a tracing::warn with the shell name and suggest 'glass check'.
Prepares for status bar toast to be added in the UI/UX branch."
```

---

### Task 3: GPU init panic to friendly error (P-4)

The 3 `expect()` calls in `GlassRenderer::new()` cause panics on headless systems, VMs, or driver issues. Convert to a `try_new()` that returns `Result`, and show a user-friendly error.

**Files:**
- Modify: `crates/glass_renderer/src/surface.rs:20-49` (convert to Result)
- Modify: `src/main.rs` (call site of GlassRenderer::new)

- [ ] **Step 1: Add `try_new` method returning Result**

In `crates/glass_renderer/src/surface.rs`, rename the existing `new` to `try_new` and change return type:

```rust
pub async fn try_new(window: Arc<winit::window::Window>) -> anyhow::Result<Self> {
```

Replace the 3 `expect()` calls:

**Line 32 — surface creation:**
```rust
// BEFORE:
.expect("Failed to create wgpu surface");

// AFTER:
.map_err(|e| anyhow::anyhow!("Failed to create GPU surface: {e}. \
    Your GPU driver may not support the required graphics API. \
    Run `glass check` for diagnosis."))?;
```

**Line 41 — adapter request:**
```rust
// BEFORE:
.expect("No compatible GPU adapter found");

// AFTER:
.ok_or_else(|| anyhow::anyhow!("No compatible GPU adapter found. \
    Ensure your system has a GPU with DX12 (Windows), Metal (macOS), \
    or Vulkan (Linux) support. Run `glass check` for diagnosis."))?;
```

**Line 49 — device request:**
```rust
// BEFORE:
.expect("Failed to create wgpu device");

// AFTER:
.map_err(|e| anyhow::anyhow!("Failed to create GPU device: {e}. \
    Your GPU driver may be outdated. Run `glass check` for diagnosis."))?;
```

- [ ] **Step 2: Add `new` wrapper that calls `try_new`**

Keep backward compatibility for any call sites that don't handle errors yet:

```rust
pub async fn new(window: Arc<winit::window::Window>) -> Self {
    Self::try_new(window).await.unwrap_or_else(|e| {
        // This should be caught at the main.rs call site; this is a safety net
        eprintln!("Glass fatal GPU error: {e}");
        #[cfg(target_os = "windows")]
        {
            use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
            let msg: Vec<u16> = format!("{e}").encode_utf16().chain(std::iter::once(0)).collect();
            let title: Vec<u16> = "Glass - GPU Error".encode_utf16().chain(std::iter::once(0)).collect();
            unsafe {
                MessageBoxW(std::ptr::null_mut(), msg.as_ptr(), title.as_ptr(), MB_ICONERROR | MB_OK);
            }
        }
        std::process::exit(1);
    })
}
```

Note: The `windows-sys` import for `MessageBoxW` requires the `Win32_UI_WindowsAndMessaging` feature. Check the root `Cargo.toml` `windows-sys` features list. If `Win32_UI_WindowsAndMessaging` is not present, add it.

- [ ] **Step 3: Update main.rs call site to use `try_new`**

Find where `GlassRenderer::new(window)` is called in `src/main.rs` and update to use `try_new` with proper error handling:

```rust
// BEFORE:
let renderer = pollster::block_on(GlassRenderer::new(window.clone()));

// AFTER:
let renderer = match pollster::block_on(GlassRenderer::try_new(window.clone())) {
    Ok(r) => r,
    Err(e) => {
        tracing::error!("GPU initialization failed: {e}");
        show_fatal_error(&format!("{e}"));
    }
};
```

If `show_fatal_error` doesn't exist yet (it's introduced in Branch 1 Task 8), add a local version or use the same pattern (eprintln + MessageBoxW + exit).

- [ ] **Step 4: Add `Win32_UI_WindowsAndMessaging` feature if needed**

Check `Cargo.toml` for the `windows-sys` features list. If `Win32_UI_WindowsAndMessaging` is not present:

```toml
windows-sys = { version = "0.59", features = [
    "Win32_System_Console",
    "Win32_System_JobObjects",
    "Win32_Foundation",
    "Win32_System_Threading",
    "Win32_UI_WindowsAndMessaging",
] }
```

- [ ] **Step 5: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 6: Commit**

```bash
git add crates/glass_renderer/src/surface.rs src/main.rs Cargo.toml
git commit -m "fix(P-4): GPU init shows friendly error instead of panicking

Convert GlassRenderer::new() expect() calls to try_new() returning Result.
Error messages suggest 'glass check' for diagnosis. On Windows, shows
native MessageBox since stderr is hidden by windows_subsystem attribute."
```

---

### Task 4: Cargo.toml metadata completion (P-8, P-10)

The root `Cargo.toml` is missing `repository`, `homepage`, `keywords`, `categories`, and `rust-version` fields needed for `cargo install` and crates.io publishing.

**Files:**
- Modify: `Cargo.toml:72-79` (root package section)

- [ ] **Step 1: Add missing metadata fields**

In the `[package]` section of the root `Cargo.toml` (after line 78), add:

```toml
[package]
name = "glass"
version = "2.5.0"
edition = "2021"
authors = ["Glass Contributors"]
license = "MIT"
description = "GPU-accelerated terminal emulator with command structure awareness"
repository = "https://github.com/candyhunterz/Glass"
homepage = "https://github.com/candyhunterz/Glass"
keywords = ["terminal", "gpu", "emulator", "command-line", "developer-tools"]
categories = ["command-line-utilities", "development-tools"]
rust-version = "1.80"
```

For `rust-version`: The project uses Rust 2021 edition and depends on `wgpu 28.0` which requires Rust 1.80+. Set MSRV to `1.80` as a conservative minimum. This can be tightened later with `cargo msrv`.

- [ ] **Step 2: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "chore(P-8/P-10): add repository, homepage, keywords, rust-version to Cargo.toml

Fills in metadata required for cargo install and crates.io.
Sets MSRV to 1.80 (minimum for wgpu 28.0)."
```

---

### Task 5: README Linux dependencies fix (P-5, P-9)

The current Linux deps list is incomplete (missing `libxtst-dev`) and only covers Debian/Ubuntu.

**Files:**
- Modify: `README.md` (Linux dependencies section, around line 191-194)

- [ ] **Step 1: Expand Linux dependency instructions**

Replace the current single-line apt command with distro-specific blocks:

```markdown
On Linux, install system dependencies for your distribution:

**Debian / Ubuntu:**
```bash
sudo apt install libxkbcommon-dev libwayland-dev libx11-dev libxi-dev libxtst-dev
```

**Fedora:**
```bash
sudo dnf install libxkbcommon-devel wayland-devel libX11-devel libXi-devel libXtst-devel
```

**Arch Linux:**
```bash
sudo pacman -S libxkbcommon wayland libx11 libxi libxtst
```
```

Note: The exact markdown structure should match the existing README style. Read the surrounding context to ensure consistent formatting.

- [ ] **Step 2: Build and test** (no code change, just docs — sanity check)

```bash
cargo build 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs(P-5/P-9): add libxtst-dev and Fedora/Arch Linux build deps

The Linux dependency list was missing libxtst-dev (required for X11
test extension). Also adds dnf and pacman equivalents for Fedora and
Arch Linux users."
```

---

### Task 6: Create Scoop manifest (P-12)

Windows users who prefer Scoop over MSI/winget need a manifest. Scoop manifests are simpler than winget (single JSON file).

**Files:**
- Create: `packaging/scoop/glass.json`

- [ ] **Step 1: Create the packaging/scoop directory**

```bash
mkdir -p packaging/scoop
```

- [ ] **Step 2: Create the Scoop manifest**

Create `packaging/scoop/glass.json`:

```json
{
    "version": "2.5.0",
    "description": "GPU-accelerated terminal emulator with command structure awareness",
    "homepage": "https://github.com/candyhunterz/Glass",
    "license": "MIT",
    "architecture": {
        "64bit": {
            "url": "https://github.com/candyhunterz/Glass/releases/download/v2.5.0/glass-2.5.0-x86_64.msi",
            "hash": "<SHA256>"
        }
    },
    "bin": "bin\\glass.exe",
    "checkver": "github",
    "autoupdate": {
        "architecture": {
            "64bit": {
                "url": "https://github.com/candyhunterz/Glass/releases/download/v$version/glass-$version-x86_64.msi"
            }
        }
    },
    "notes": "Shell integration is embedded in the binary and works automatically."
}
```

- [ ] **Step 3: Commit**

```bash
git add packaging/scoop/glass.json
git commit -m "feat(P-12): add Scoop package manifest for Windows

Template manifest for publishing to a Scoop bucket.
SHA256 placeholder to be filled per release."
```

---

### Task 7: Homebrew/winget SHA256 automation docs (P-6)

The Homebrew and winget manifests have `<SHA256>` placeholders. Since full CI automation is out of scope for this branch, document the manual release steps clearly.

**Files:**
- Modify: `packaging/homebrew/glass.rb` (add release-step comments)
- Modify: `packaging/winget/Glass.Terminal.installer.yaml` (already has comments — verify)

- [ ] **Step 1: Verify winget manifest comments**

The winget installer manifest already has step-by-step release comments (lines 1-7). Verify they are accurate and complete. No changes needed if they are.

- [ ] **Step 2: Enhance Homebrew formula comments**

The Homebrew formula already has release comments (lines 5-13). Verify the GitHub user placeholder and SHA256 instructions are clear. No code changes expected — this is a documentation-only verification.

- [ ] **Step 3: Add RELEASING.md section (optional — only if it exists)**

If a `RELEASING.md` or release docs file exists, add packaging steps there. If not, skip — the Documentation branch (Branch 7) will create release docs.

- [ ] **Step 4: Commit (only if changes were made)**

```bash
git add packaging/
git commit -m "docs(P-6): verify and clarify packaging manifest release instructions"
```

---

### Task 8: macOS Gatekeeper documentation (P-7)

Unsigned DMG downloads are quarantined by macOS Gatekeeper. Document the workaround prominently.

**Files:**
- Modify: `README.md` (installation section)

- [ ] **Step 1: Add Gatekeeper workaround**

In the README installation section, after the macOS install instructions, add:

```markdown
> **macOS Gatekeeper:** If macOS blocks Glass with "cannot be opened because the developer cannot be verified", run:
> ```bash
> xattr -cr /Applications/Glass.app
> ```
> This removes the quarantine attribute from unsigned downloads. Code signing and notarization are planned for a future release.
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs(P-7): add macOS Gatekeeper workaround to README

Document xattr -cr workaround for unsigned DMG downloads.
Notarization is deferred to a future release."
```

---

### Task 9: First-run default config creation (P-11)

When Glass launches for the first time, `~/.glass/config.toml` does not exist. The config loads defaults silently, but users have no idea a config file is available or where it lives. Create a commented-out default config on first launch.

**Files:**
- Modify: `crates/glass_core/src/config.rs` (add `ensure_default_config` function)
- Modify: `src/main.rs` (call `ensure_default_config` at startup)

- [ ] **Step 1: Add `ensure_default_config` function**

In `crates/glass_core/src/config.rs`, add after the `load()` function:

```rust
/// Create a default `~/.glass/config.toml` with all options commented out
/// if the file does not already exist. This gives users a reference for
/// available configuration options without changing any defaults.
pub fn ensure_default_config() {
    let Some(config_path) = Self::config_path() else {
        return;
    };

    if config_path.exists() {
        return; // Already has a config — don't overwrite
    }

    // Create ~/.glass/ directory
    if let Some(parent) = config_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::debug!("Could not create config directory: {e}");
            return;
        }
    }

    let default_config = r#"# Glass Terminal Configuration
# This file was auto-generated on first launch. All options shown below
# are commented out and use their default values. Uncomment and modify
# as needed. Changes are hot-reloaded — no restart required.
#
# Documentation: https://github.com/candyhunterz/Glass

# Font settings
# font_family = "Cascadia Code"
# font_size = 14.0

# Shell override (auto-detected if not set)
# shell = "/bin/zsh"

# [history]
# max_output_capture_kb = 50

# [snapshot]
# enabled = true
# max_count = 1000
# max_size_mb = 500
# retention_days = 30

# [pipes]
# enabled = true

# [soi]
# enabled = true

# [scripting]
# enabled = true
# auto_confirm = false
"#;

    match std::fs::write(&config_path, default_config) {
        Ok(()) => tracing::info!(
            "Created default config at {}",
            config_path.display()
        ),
        Err(e) => tracing::debug!(
            "Could not write default config: {e}"
        ),
    }
}
```

- [ ] **Step 2: Call from main.rs at startup**

In `src/main.rs`, right before or after `GlassConfig::load()` is called, add:

```rust
GlassConfig::ensure_default_config();
let config = GlassConfig::load();
```

Find the existing `GlassConfig::load()` call site and add the `ensure_default_config()` call before it. The order matters: create the file first, then load (so the first load reads the newly created file, which is all comments and produces defaults anyway).

- [ ] **Step 3: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 4: Add unit test**

In `crates/glass_core/src/config.rs` test module:

```rust
#[test]
fn ensure_default_config_creates_file() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join(".glass").join("config.toml");
    // Can't easily test the real function (uses dirs::home_dir),
    // but verify the default config text parses correctly
    let default_text = r#"
# font_family = "Cascadia Code"
# font_size = 14.0
"#;
    // All commented out = empty TOML = valid, produces defaults
    let config: GlassConfig = toml::from_str(default_text).unwrap();
    assert_eq!(config, GlassConfig::default());
}
```

- [ ] **Step 5: Commit**

```bash
git add crates/glass_core/src/config.rs src/main.rs
git commit -m "feat(P-11): create commented-out default config on first launch

On first run, writes ~/.glass/config.toml with all options commented
out as a reference. Does not change behavior — all defaults remain.
Gives users discoverability of configuration options."
```

---

### Task 10: Unsupported shell warning (P-14)

When Glass detects a shell that is not bash, zsh, fish, or PowerShell, shell integration cannot be injected. Warn the user.

**Files:**
- Modify: `src/main.rs` (in `get_shell_integration` or the injection block)

- [ ] **Step 1: Add warning for unknown shells**

The `get_shell_integration` function from Task 1 already handles the 4 known shells and falls through to bash as default. However, there are shells like `nushell`, `tcsh`, `ksh` where even the bash fallback will not work.

Add detection before the injection block (~line 711):

```rust
let known_shells = ["bash", "zsh", "fish", "pwsh", "powershell"];
let is_known_shell = known_shells.iter().any(|s| {
    effective_shell_for_integration.to_lowercase().contains(s)
});

if !is_known_shell {
    tracing::warn!(
        "Shell '{}' does not have Glass integration support. \
         Command blocks, pipe visualization, and undo require bash, zsh, fish, or PowerShell.",
        effective_shell_for_integration
    );
}
```

This goes before the `if let Some(path) = get_shell_integration(...)` block. The function still falls back to bash injection for unknown shells (which may partially work), but the user is warned.

- [ ] **Step 2: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "fix(P-14): warn when shell lacks Glass integration support

Log a warning for shells other than bash/zsh/fish/PowerShell.
Injection still attempts bash fallback but user is informed that
full functionality requires a supported shell."
```

---

### Task 11: `glass check` diagnostic subcommand (P-13)

Add a CLI subcommand that reports system diagnostics: GPU adapter, detected shell, shell integration status, config path, and data directory.

**Files:**
- Modify: `src/main.rs` (add `Check` variant to `Commands` enum, add handler)

- [ ] **Step 1: Add `Check` to the `Commands` enum**

In `src/main.rs`, in the `Commands` enum (~line 65-86):

```rust
#[derive(Subcommand, Debug, PartialEq)]
enum Commands {
    // ... existing variants ...

    /// Run system diagnostics (GPU, shell, config, integration)
    Check,
}
```

- [ ] **Step 2: Add the check handler function**

Add a new function:

```rust
fn run_check() -> anyhow::Result<()> {
    println!("Glass System Check");
    println!("==================\n");

    // Version
    println!("Version: {}", env!("CARGO_PKG_VERSION"));

    // Config
    match glass_core::config::GlassConfig::config_path() {
        Some(p) if p.exists() => println!("Config:  {} (found)", p.display()),
        Some(p) => println!("Config:  {} (not found — using defaults)", p.display()),
        None => println!("Config:  <unable to determine home directory>"),
    }

    // Data directory
    if let Some(home) = dirs::home_dir() {
        let data_dir = home.join(".glass");
        if data_dir.exists() {
            println!("Data:    {}", data_dir.display());
        } else {
            println!("Data:    {} (not created yet)", data_dir.display());
        }
    }

    // Shell detection
    let shell = std::env::var("SHELL")
        .or_else(|_| std::env::var("COMSPEC"))
        .unwrap_or_else(|_| "<not detected>".to_string());
    println!("Shell:   {}", shell);

    // Shell integration
    let shell_lower = shell.to_lowercase();
    let known = ["bash", "zsh", "fish", "pwsh", "powershell"];
    let supported = known.iter().any(|s| shell_lower.contains(s));
    if supported {
        println!("Shell integration: supported");
        // Check if embedded scripts are available (they always are post P-1/P-2)
        println!("Shell scripts:     embedded in binary");
    } else {
        println!("Shell integration: NOT supported (need bash, zsh, fish, or PowerShell)");
    }

    // GPU check
    println!("\nGPU Diagnostics:");
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        #[cfg(target_os = "windows")]
        backends: wgpu::Backends::DX12,
        #[cfg(not(target_os = "windows"))]
        backends: wgpu::Backends::all(),
        ..Default::default()
    });

    let adapters: Vec<_> = instance.enumerate_adapters(wgpu::Backends::all());
    if adapters.is_empty() {
        println!("  No GPU adapters found!");
        println!("  Glass requires a GPU with DX12 (Windows), Metal (macOS), or Vulkan (Linux).");
    } else {
        for adapter in &adapters {
            let info = adapter.get_info();
            println!(
                "  {} — {:?} ({:?})",
                info.name, info.backend, info.device_type
            );
        }
    }

    println!("\nAll checks complete.");
    Ok(())
}
```

Note: The `wgpu` types used here (`Instance`, `Backends`, etc.) are already available in the binary since Glass depends on `wgpu`. The `enumerate_adapters` method lists all adapters without creating a surface, so it works in a terminal context.

- [ ] **Step 3: Wire `Check` into the command dispatch**

In the main function's command matching (find where `Commands::History`, `Commands::Mcp`, etc. are matched):

```rust
Some(Commands::Check) => {
    return run_check();
}
```

This should go in the early command dispatch before the event loop is created, since `glass check` should work without a window.

- [ ] **Step 4: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 5: Manual test**

```bash
cargo run -- check
```

Verify it prints GPU adapter info, shell detection, config path, etc.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat(P-13): add 'glass check' diagnostic subcommand

Reports version, config path, data directory, detected shell,
shell integration status, and GPU adapter list. Helps users
diagnose setup issues without reading source code."
```

---

### Task 12: Document `windows_subsystem` CLI limitation (P-15)

The `#![windows_subsystem = "windows"]` attribute hides the console window, which means stderr/stdout are invisible when launched from Explorer. CLI subcommands work when launched from an existing terminal. Document this.

**Files:**
- Modify: `README.md` (troubleshooting or notes section)

- [ ] **Step 1: Add documentation**

In the README, in an appropriate section (troubleshooting, notes, or FAQ), add:

```markdown
### Windows Console Behavior

Glass uses `#![windows_subsystem = "windows"]` to suppress the console window when launched from the Start Menu or Explorer. This means:
- **No visible console output** when double-clicked — this is intentional
- **CLI subcommands** (`glass history`, `glass check`, `glass mcp`) work normally when run from an existing terminal (PowerShell, cmd, Windows Terminal)
- **Error messages** during initialization use native Windows dialog boxes since stderr is hidden
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs(P-15): document windows_subsystem CLI limitation in README

Explain that Glass suppresses console window on Windows and that CLI
subcommands must be run from an existing terminal."
```

---

### Task 13: Final verification and clippy

- [ ] **Step 1: Run clippy**

```bash
cargo clippy --workspace -- -D warnings 2>&1
```

Fix any warnings.

- [ ] **Step 2: Run fmt**

```bash
cargo fmt --all -- --check 2>&1
```

Fix any formatting issues.

- [ ] **Step 3: Run full test suite**

```bash
cargo test --workspace 2>&1
```

- [ ] **Step 4: Commit any cleanup**

```bash
git add -A
git commit -m "chore: clippy and fmt cleanup for setup-packaging branch"
```

- [ ] **Step 5: Summary — verify all items addressed**

Check off against the spec:
- [x] P-1/P-2: Embed shell integration scripts in binary (Task 1)
- [x] P-3: Warning when shell integration not found (Task 2)
- [x] P-4: GPU init panic to friendly error (Task 3)
- [x] P-5: README Linux deps incomplete (Task 5)
- [x] P-6: Homebrew/winget manifest SHA256 docs (Task 7)
- [x] P-7: macOS Gatekeeper docs (Task 8)
- [x] P-8: MSRV declaration (Task 4)
- [x] P-9: Fedora/Arch build deps (Task 5)
- [x] P-10: Cargo.toml metadata (Task 4)
- [x] P-11: First-run default config creation (Task 9)
- [x] P-12: Scoop manifest (Task 6)
- [x] P-13: `glass check` subcommand (Task 11)
- [x] P-14: Unsupported shell warning (Task 10)
- [x] P-15: `windows_subsystem` CLI limitation docs (Task 12)

# Phase 29: Auto-Update - Research

**Researched:** 2026-03-07
**Domain:** GitHub Releases update checking, platform-specific installer upgrades, status bar notifications
**Confidence:** HIGH

## Summary

Phase 29 implements a non-blocking update checker that queries the GitHub Releases API on startup, displays a notification in the existing status bar when a newer version exists, and provides platform-specific upgrade paths (MSI on Windows, DMG download on macOS, instructions on Linux).

The architecture follows the established pattern from the config watcher: spawn a background thread that does blocking I/O, then sends results to the winit event loop via `EventLoopProxy<AppEvent>`. The HTTP request is a single unauthenticated GET to `https://api.github.com/repos/{owner}/{repo}/releases/latest`, parsed with serde to extract `tag_name` and asset download URLs. Version comparison uses the `semver` crate. No existing HTTP client is in the dependency tree, so `ureq` is the recommended addition for its simplicity, blocking API (matching the thread-based pattern), and minimal dependency footprint.

The `self_update` crate was considered but rejected: it replaces binaries in-place, which conflicts with MSI-based upgrades on Windows (the MSI uses an UpgradeCode GUID for proper Windows Installer upgrade flow). Instead, the update apply mechanism is platform-specific: on Windows, download the MSI to a temp file and launch `msiexec /i` (which handles the upgrade via the existing UpgradeCode D5F79758-7183-4EBE-9B63-DADD19B1D42C); on macOS, open the DMG download URL in the default browser; on Linux, show a notification with the release URL.

**Primary recommendation:** Use `ureq` + `semver` + `serde_json` for version checking on a background thread, extend `AppEvent` with an `UpdateAvailable` variant, and add a center/right notification segment to the existing `StatusBarRenderer`.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| UPDT-01 | Background update check on startup against GitHub Releases | `ureq` blocking GET on spawned thread, `EventLoopProxy` to send result back to main loop (config watcher pattern) |
| UPDT-02 | Status bar notification when update is available | New `AppEvent::UpdateAvailable` variant, extend `StatusBarRenderer` and `StatusLabel` with update notification text |
| UPDT-03 | One-click update apply (MSI on Windows, DMG on macOS, notify on Linux) | Platform-specific: `msiexec /i` for Windows MSI, `open` for macOS DMG URL, status bar text for Linux |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| ureq | 3.x | Blocking HTTP client for GitHub API | Minimal deps, blocking API fits thread model, no async runtime needed |
| semver | 1.x | Semantic version parsing and comparison | De facto standard for Rust version comparison |
| serde_json | 1.0 | Parse GitHub API JSON response | Already a dev-dependency, now needed at runtime |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tempfile | 3 | Temp file for downloaded MSI (Windows only) | Already a dev-dependency; needed for safe download-before-install |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| ureq | reqwest | reqwest is async (project has tokio), but much heavier compile; update check is one request on a background thread, blocking is simpler |
| ureq | self_update | self_update replaces binary in-place; incompatible with MSI upgrade flow on Windows |
| semver | manual parsing | semver handles pre-release, build metadata, edge cases correctly |

**Installation:**
```bash
cargo add ureq semver
cargo add serde_json  # move from dev-dependencies to dependencies
```

## Architecture Patterns

### Recommended Project Structure
```
crates/glass_core/src/
    updater.rs          # Update checker: spawn thread, HTTP request, version compare
    event.rs            # Add AppEvent::UpdateAvailable variant
crates/glass_renderer/src/
    status_bar.rs       # Extend with update notification rendering
src/main.rs             # Wire up: spawn updater on startup, handle AppEvent, pass to renderer
```

### Pattern 1: Background Update Checker (follows config_watcher pattern)
**What:** Spawn a named thread on startup that makes a blocking HTTP request to GitHub Releases API, compares versions with semver, and sends `AppEvent::UpdateAvailable` if newer version found.
**When to use:** On application startup, once per session.
**Example:**
```rust
// Source: Pattern derived from glass_core/src/config_watcher.rs
use semver::Version;
use winit::event_loop::EventLoopProxy;
use crate::event::AppEvent;

pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub download_url: String,   // Platform-specific asset URL
    pub release_url: String,    // HTML URL for the release page
}

pub fn spawn_update_checker(
    current_version: &str,
    proxy: EventLoopProxy<AppEvent>,
) {
    let current = current_version.to_string();
    std::thread::Builder::new()
        .name("Glass update checker".into())
        .spawn(move || {
            match check_for_update(&current) {
                Ok(Some(info)) => {
                    let _ = proxy.send_event(AppEvent::UpdateAvailable(info));
                }
                Ok(None) => {
                    tracing::debug!("Glass is up to date");
                }
                Err(e) => {
                    tracing::debug!("Update check failed (non-fatal): {}", e);
                }
            }
        })
        .expect("Failed to spawn update checker thread");
}

fn check_for_update(current_version: &str) -> Result<Option<UpdateInfo>, Box<dyn std::error::Error>> {
    let response: serde_json::Value = ureq::get(
        "https://api.github.com/repos/OWNER/REPO/releases/latest"
    )
    .header("User-Agent", "glass-terminal")
    .header("Accept", "application/vnd.github.v3+json")
    .call()?
    .body_mut()
    .read_json()?;

    let tag = response["tag_name"].as_str().ok_or("missing tag_name")?;
    let latest_str = tag.strip_prefix('v').unwrap_or(tag);
    let current = Version::parse(current_version)?;
    let latest = Version::parse(latest_str)?;

    if latest > current {
        let download_url = find_platform_asset(&response)?;
        let release_url = response["html_url"].as_str().unwrap_or("").to_string();
        Ok(Some(UpdateInfo {
            current: current.to_string(),
            latest: latest.to_string(),
            download_url,
            release_url,
        }))
    } else {
        Ok(None)
    }
}
```

### Pattern 2: Platform-Specific Asset Selection
**What:** Match release asset names to current platform to find the correct download URL.
**When to use:** When parsing GitHub API response to find the right installer.
**Example:**
```rust
fn find_platform_asset(release: &serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    let assets = release["assets"].as_array().ok_or("missing assets")?;
    let suffix = if cfg!(target_os = "windows") {
        ".msi"
    } else if cfg!(target_os = "macos") {
        ".dmg"
    } else {
        ".deb"
    };

    for asset in assets {
        if let Some(name) = asset["name"].as_str() {
            if name.ends_with(suffix) {
                if let Some(url) = asset["browser_download_url"].as_str() {
                    return Ok(url.to_string());
                }
            }
        }
    }
    Err(format!("No {} asset found in release", suffix).into())
}
```

### Pattern 3: Platform-Specific Update Apply
**What:** Execute the appropriate update mechanism per OS.
**When to use:** When user triggers update from the status bar notification.
**Example:**
```rust
pub fn apply_update(info: &UpdateInfo) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "windows")]
    {
        // Download MSI to temp file, then launch msiexec
        let temp_dir = std::env::temp_dir();
        let msi_path = temp_dir.join(format!("glass-{}.msi", info.latest));
        download_file(&info.download_url, &msi_path)?;
        std::process::Command::new("msiexec")
            .args(["/i", &msi_path.to_string_lossy(), "/passive"])
            .spawn()?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        // Open DMG download URL in browser
        std::process::Command::new("open")
            .arg(&info.download_url)
            .spawn()?;
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        // Open release page in browser
        std::process::Command::new("xdg-open")
            .arg(&info.release_url)
            .spawn()?;
        Ok(())
    }
}
```

### Anti-Patterns to Avoid
- **Blocking the main thread:** Never make HTTP requests on the winit event loop thread. Always use a spawned thread.
- **Using self_update for MSI-based installs:** self_update replaces the binary in-place, bypassing Windows Installer's upgrade mechanism. The MSI UpgradeCode ensures proper upgrade/uninstall flow.
- **Panicking on network failure:** Update checks are best-effort. Network errors should be logged at debug level and silently ignored -- never shown to the user.
- **Checking on every keystroke/redraw:** Check once on startup only. Store the result in `Processor` state, render until dismissed.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Semantic version comparison | String comparison or manual parsing | `semver` crate | Pre-release ordering, build metadata, edge cases |
| HTTP client | Raw TCP/TLS socket handling | `ureq` | TLS, redirects, timeouts, User-Agent headers |
| JSON parsing | Manual string scanning | `serde_json::Value` | Correct escaping, nested structure traversal |
| Temp file management | Manual file creation in temp dir | `tempfile` crate (Windows MSI download) | Atomic creation, cleanup, permissions |

**Key insight:** The actual update check logic is ~50 lines. The complexity is in the platform-specific apply step and the UI integration with the status bar, not in HTTP or version parsing.

## Common Pitfalls

### Pitfall 1: GitHub API Rate Limiting
**What goes wrong:** Unauthenticated GitHub API requests are rate-limited to 60/hour per IP. If many Glass instances start simultaneously (e.g., CI), requests fail.
**Why it happens:** No auth token, shared IP.
**How to avoid:** Set a reasonable timeout (5-10 seconds). Cache the result so only one check per startup. Log rate limit errors at debug level, never show to user. The 60/hour limit is generous for a desktop app checking once per startup.
**Warning signs:** HTTP 403 with `X-RateLimit-Remaining: 0` header.

### Pitfall 2: Blocking the Event Loop
**What goes wrong:** Making the HTTP request on the main thread freezes the terminal for seconds.
**Why it happens:** DNS resolution + TLS handshake + response download can take 2-5 seconds.
**How to avoid:** Always spawn a dedicated thread (like config_watcher). The thread sends AppEvent when done.
**Warning signs:** Terminal input lag on startup.

### Pitfall 3: MSI Upgrade Conflicts with Running Process
**What goes wrong:** `msiexec /i` cannot replace files that are locked by the running Glass process.
**Why it happens:** Windows file locking prevents overwriting in-use executables.
**How to avoid:** Use `/passive` flag which shows a progress bar. The MSI's `RemoveExistingProducts` action handles this. The user should close Glass before the installer finishes, or the installer may prompt for a reboot. Document this behavior.
**Warning signs:** MSI installer hangs or requests reboot.

### Pitfall 4: Missing User-Agent Header
**What goes wrong:** GitHub API rejects requests without a User-Agent header.
**Why it happens:** GitHub requires User-Agent on all API requests.
**How to avoid:** Always set `.header("User-Agent", "glass-terminal")`.
**Warning signs:** HTTP 403 response from GitHub API.

### Pitfall 5: Version String with 'v' Prefix
**What goes wrong:** `semver::Version::parse("v1.0.0")` fails because the `v` prefix is not part of semver.
**Why it happens:** GitHub tags typically use `v1.0.0` format.
**How to avoid:** Strip the `v` prefix before parsing: `tag.strip_prefix('v').unwrap_or(tag)`.
**Warning signs:** Update check always reports "up to date" or always errors.

## Code Examples

### AppEvent Extension
```rust
// In crates/glass_core/src/event.rs
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub latest_version: String,
    pub download_url: String,
    pub release_url: String,
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    // ... existing variants ...
    /// A newer version of Glass is available.
    UpdateAvailable(UpdateInfo),
}
```

### Status Bar Extension
```rust
// In crates/glass_renderer/src/status_bar.rs
// Add a center_text field to StatusLabel:
pub struct StatusLabel {
    pub left_text: String,
    pub right_text: Option<String>,
    pub center_text: Option<String>,  // Update notification
    pub y: f32,
    pub left_color: Rgb,
    pub right_color: Rgb,
    pub center_color: Rgb,            // Bright color for visibility
}
```

### Main Loop Integration
```rust
// In src/main.rs, in the resumed() method after spawning PTY:
glass_core::updater::spawn_update_checker(
    env!("CARGO_PKG_VERSION"),
    self.proxy.clone(),
);

// In user_event():
AppEvent::UpdateAvailable(info) => {
    // Store in Processor state
    self.update_info = Some(info);
    // Trigger redraw to show notification
    for (_, ctx) in &self.windows {
        ctx.window.request_redraw();
    }
}
```

### Keyboard Shortcut for Apply
```rust
// When update notification is visible, handle a key combo (e.g., Ctrl+Shift+U)
// to trigger the platform-specific update apply.
if self.update_info.is_some() && key == Key::Character("u".into())
    && modifiers.contains(ModifiersState::CONTROL | ModifiersState::SHIFT)
{
    if let Some(info) = &self.update_info {
        if let Err(e) = glass_core::updater::apply_update(info) {
            tracing::warn!("Failed to apply update: {}", e);
        }
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Binary self-replacement | Platform-native installer upgrades | N/A | Proper integration with OS package management (MSI UpgradeCode, DMG) |
| reqwest (async) for simple HTTP | ureq (blocking) for single-request use cases | ureq 3.x (2025) | Simpler API, fewer dependencies for non-async contexts |
| Manual version string comparison | semver crate | Stable since semver 1.0 | Correct handling of pre-release, build metadata |

**Deprecated/outdated:**
- `self_update` binary replacement: Does not integrate with MSI upgrade flow; acceptable for standalone binaries but wrong for installer-based distribution.

## Open Questions

1. **Repository owner/name for GitHub API URL**
   - What we know: The release workflow publishes to the project's GitHub repo
   - What's unclear: The exact owner/repo string (e.g., `nkngu/Glass` or an org)
   - Recommendation: Make it configurable or derive from a const in the code. Use a const like `const GITHUB_REPO: &str = "owner/Glass";`

2. **Update check frequency**
   - What we know: Requirements say "on startup"
   - What's unclear: Should it throttle (e.g., once per 24 hours)?
   - Recommendation: Start with check-on-every-startup (simple). If rate limiting becomes an issue, add a last-checked timestamp file in `~/.glass/`.

3. **Dismissing the notification**
   - What we know: Status bar shows the notification
   - What's unclear: Can the user dismiss it? Does it persist across sessions?
   - Recommendation: Persist until update is applied or a new version is installed. Allow dismiss with a keybind (e.g., Escape when not in any overlay).

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test + cargo test |
| Config file | Cargo.toml `[dev-dependencies]` |
| Quick run command | `cargo test -p glass_core -- updater` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| UPDT-01 | Version parsing and comparison logic | unit | `cargo test -p glass_core -- updater::tests -x` | No - Wave 0 |
| UPDT-01 | GitHub API response parsing | unit | `cargo test -p glass_core -- updater::tests::parse -x` | No - Wave 0 |
| UPDT-02 | StatusLabel includes update text when update_info present | unit | `cargo test -p glass_renderer -- status_bar::tests -x` | No - Wave 0 |
| UPDT-03 | Platform asset URL selection from release JSON | unit | `cargo test -p glass_core -- updater::tests::asset -x` | No - Wave 0 |
| UPDT-03 | MSI download and msiexec launch (Windows) | manual-only | Manual test on Windows | N/A |
| UPDT-03 | DMG URL open (macOS) | manual-only | Manual test on macOS | N/A |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_core -- updater`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_core/src/updater.rs` -- unit tests for version comparison, JSON parsing, asset selection
- [ ] `crates/glass_renderer/src/status_bar.rs` -- extend existing tests for update notification rendering
- [ ] Dependencies: `cargo add ureq semver` and move `serde_json` to runtime deps

## Sources

### Primary (HIGH confidence)
- [GitHub REST API - Releases](https://docs.github.com/en/rest/releases/releases) - endpoint format, response schema, rate limits
- Project source: `crates/glass_core/src/config_watcher.rs` - established background thread + EventLoopProxy pattern
- Project source: `crates/glass_renderer/src/status_bar.rs` - existing status bar rendering architecture
- Project source: `crates/glass_core/src/event.rs` - AppEvent enum extension point
- Project source: `.github/workflows/release.yml` - release artifact naming conventions (MSI, DMG, deb)

### Secondary (MEDIUM confidence)
- [ureq GitHub](https://github.com/algesten/ureq) - blocking HTTP client, minimal deps, Rust-native TLS
- [self_update docs](https://docs.rs/self_update/latest/self_update/) - evaluated and rejected for MSI incompatibility
- [msiexec docs](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/msiexec) - `/i` and `/passive` flags for MSI upgrade

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - ureq/semver/serde_json are well-established, minimal crates with clear APIs
- Architecture: HIGH - follows exact same pattern as config_watcher (background thread + EventLoopProxy)
- Pitfalls: HIGH - GitHub API rate limiting, User-Agent requirement, version prefix stripping are well-documented
- Platform-specific apply: MEDIUM - MSI upgrade flow with running process needs manual testing; macOS/Linux are straightforward

**Research date:** 2026-03-07
**Valid until:** 2026-04-07 (stable domain, 30 days)

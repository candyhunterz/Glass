//! Auto-update checker: queries GitHub Releases API on a background thread,
//! compares versions with semver, and sends `AppEvent::UpdateAvailable` when
//! a newer release exists. Platform-specific apply logic launches the
//! appropriate installer.

use winit::event_loop::EventLoopProxy;

use crate::event::AppEvent;

const GITHUB_REPO: &str = "nkngu/Glass";

/// Information about an available update.
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub download_url: String,
    pub release_url: String,
}

/// Spawn a background thread that checks for updates against GitHub Releases.
///
/// Follows the same pattern as `spawn_config_watcher`: a named thread does
/// blocking I/O and sends the result via `EventLoopProxy`.
pub fn spawn_update_checker(current_version: &str, proxy: EventLoopProxy<AppEvent>) {
    let current = current_version.to_string();
    std::thread::Builder::new()
        .name("Glass update checker".into())
        .spawn(move || match check_for_update(&current) {
            Ok(Some(info)) => {
                let _ = proxy.send_event(AppEvent::UpdateAvailable(info));
            }
            Ok(None) => {
                tracing::debug!("Glass is up to date");
            }
            Err(e) => {
                tracing::debug!("Update check failed (non-fatal): {}", e);
            }
        })
        .expect("Failed to spawn update checker thread");
}

/// Check GitHub Releases API for a newer version.
///
/// Returns `Ok(Some(UpdateInfo))` if a newer version is available,
/// `Ok(None)` if up-to-date, or `Err` on network/parse failure.
fn check_for_update(
    current_version: &str,
) -> Result<Option<UpdateInfo>, Box<dyn std::error::Error>> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        GITHUB_REPO
    );

    let body = ureq::get(&url)
        .header("User-Agent", "glass-terminal")
        .header("Accept", "application/vnd.github.v3+json")
        .call()?
        .body_mut()
        .read_to_string()?;

    let response: serde_json::Value = serde_json::from_str(&body)?;

    parse_update_from_response(current_version, &response)
}

/// Parse a GitHub release JSON response and determine if an update is available.
///
/// Extracted from `check_for_update` so it can be unit-tested with mock JSON
/// (no HTTP calls needed in tests).
fn parse_update_from_response(
    current_version: &str,
    response: &serde_json::Value,
) -> Result<Option<UpdateInfo>, Box<dyn std::error::Error>> {
    let tag = response["tag_name"]
        .as_str()
        .ok_or("missing tag_name in release response")?;
    let latest_str = tag.strip_prefix('v').unwrap_or(tag);

    let current = semver::Version::parse(current_version)?;
    let latest = semver::Version::parse(latest_str)?;

    if latest > current {
        let download_url = find_platform_asset(response)?;
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

/// Find the platform-specific installer asset from a GitHub release response.
///
/// Matches asset name suffix: `.msi` for Windows, `.dmg` for macOS, `.deb` for Linux.
fn find_platform_asset(release: &serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    let assets = release["assets"]
        .as_array()
        .ok_or("missing assets array in release response")?;

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

/// Apply an update using the platform-specific mechanism.
///
/// - **Windows:** Downloads the MSI to a temp file and launches `msiexec /i /passive`.
/// - **macOS:** Opens the DMG download URL in the default browser via `open`.
/// - **Linux:** Opens the release page in the default browser via `xdg-open`.
pub fn apply_update(info: &UpdateInfo) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "windows")]
    {
        let temp_dir = tempfile::tempdir()?;
        let msi_path = temp_dir.path().join(format!("glass-{}.msi", info.latest));
        download_file(&info.download_url, &msi_path)?;

        std::process::Command::new("msiexec")
            .args(["/i", &msi_path.to_string_lossy(), "/passive"])
            .spawn()?;

        // Leak the tempdir so it isn't cleaned up before msiexec finishes
        std::mem::forget(temp_dir);
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&info.download_url)
            .spawn()?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&info.release_url)
            .spawn()?;
        Ok(())
    }
}

/// Download a file from `url` to `path` using ureq.
#[cfg(target_os = "windows")]
fn download_file(url: &str, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;

    let mut response = ureq::get(url)
        .header("User-Agent", "glass-terminal")
        .call()?;

    let body = response.body_mut().read_to_vec()?;

    let mut file = std::fs::File::create(path)?;
    file.write_all(&body)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a mock GitHub release JSON response.
    fn mock_release(tag: &str, assets: &[(&str, &str)]) -> serde_json::Value {
        let asset_objects: Vec<serde_json::Value> = assets
            .iter()
            .map(|(name, url)| {
                serde_json::json!({
                    "name": name,
                    "browser_download_url": url
                })
            })
            .collect();

        serde_json::json!({
            "tag_name": tag,
            "html_url": format!("https://github.com/nkngu/Glass/releases/tag/{}", tag),
            "assets": asset_objects
        })
    }

    // --- Version parsing tests ---

    #[test]
    fn parse_version_strips_v_prefix() {
        let tag = "v1.2.3";
        let stripped = tag.strip_prefix('v').unwrap_or(tag);
        let version = semver::Version::parse(stripped).unwrap();
        assert_eq!(version, semver::Version::new(1, 2, 3));
    }

    #[test]
    fn parse_version_without_prefix() {
        let tag = "1.2.3";
        let stripped = tag.strip_prefix('v').unwrap_or(tag);
        let version = semver::Version::parse(stripped).unwrap();
        assert_eq!(version, semver::Version::new(1, 2, 3));
    }

    // --- Version comparison tests ---

    #[test]
    fn newer_version_detected_as_update() {
        let release = mock_release("v1.1.0", &[("glass.msi", "https://example.com/glass.msi")]);
        let result = parse_update_from_response("1.0.0", &release).unwrap();
        assert!(result.is_some(), "Expected update available");
        let info = result.unwrap();
        assert_eq!(info.latest, "1.1.0");
        assert_eq!(info.current, "1.0.0");
    }

    #[test]
    fn equal_version_is_not_update() {
        let release = mock_release("v1.0.0", &[("glass.msi", "https://example.com/glass.msi")]);
        let result = parse_update_from_response("1.0.0", &release).unwrap();
        assert!(result.is_none(), "Equal version should not be an update");
    }

    #[test]
    fn older_remote_is_not_update() {
        let release = mock_release("v0.9.0", &[("glass.msi", "https://example.com/glass.msi")]);
        let result = parse_update_from_response("1.0.0", &release).unwrap();
        assert!(result.is_none(), "Older remote should not be an update");
    }

    // --- Platform asset selection tests ---

    #[test]
    fn find_platform_asset_selects_correct_suffix() {
        let release = mock_release(
            "v2.0.0",
            &[
                ("glass-2.0.0.msi", "https://dl.example.com/glass-2.0.0.msi"),
                ("glass-2.0.0.dmg", "https://dl.example.com/glass-2.0.0.dmg"),
                ("glass-2.0.0.deb", "https://dl.example.com/glass-2.0.0.deb"),
            ],
        );

        let url = find_platform_asset(&release).unwrap();

        if cfg!(target_os = "windows") {
            assert!(
                url.ends_with(".msi"),
                "Windows should select .msi, got: {}",
                url
            );
        } else if cfg!(target_os = "macos") {
            assert!(
                url.ends_with(".dmg"),
                "macOS should select .dmg, got: {}",
                url
            );
        } else {
            assert!(
                url.ends_with(".deb"),
                "Linux should select .deb, got: {}",
                url
            );
        }
    }

    #[test]
    fn find_platform_asset_returns_error_when_no_match() {
        // Provide only assets that don't match the current platform
        let suffix = if cfg!(target_os = "windows") {
            ".msi"
        } else if cfg!(target_os = "macos") {
            ".dmg"
        } else {
            ".deb"
        };

        // Build assets that exclude the current platform's suffix
        let mut assets = vec![];
        if suffix != ".msi" {
            assets.push(("glass.msi", "https://example.com/glass.msi"));
        }
        if suffix != ".dmg" {
            assets.push(("glass.dmg", "https://example.com/glass.dmg"));
        }
        if suffix != ".deb" {
            assets.push(("glass.deb", "https://example.com/glass.deb"));
        }

        let release = mock_release("v1.0.0", &assets);
        let result = find_platform_asset(&release);
        assert!(
            result.is_err(),
            "Should error when no matching asset for platform"
        );
    }

    // --- GitHub API JSON parsing tests ---

    #[test]
    fn parse_github_release_json_extracts_tag_and_url() {
        let release = mock_release(
            "v1.5.0",
            &[(
                "glass-1.5.0.msi",
                "https://github.com/nkngu/Glass/releases/download/v1.5.0/glass-1.5.0.msi",
            )],
        );

        assert_eq!(release["tag_name"].as_str().unwrap(), "v1.5.0");
        assert!(release["html_url"].as_str().unwrap().contains("v1.5.0"));
    }

    #[test]
    fn parse_update_populates_release_url() {
        let release = mock_release(
            "v2.0.0",
            &[
                ("glass-2.0.0.msi", "https://dl.example.com/glass-2.0.0.msi"),
                ("glass-2.0.0.dmg", "https://dl.example.com/glass-2.0.0.dmg"),
                ("glass-2.0.0.deb", "https://dl.example.com/glass-2.0.0.deb"),
            ],
        );

        let result = parse_update_from_response("1.0.0", &release)
            .unwrap()
            .unwrap();
        assert!(result.release_url.contains("v2.0.0"));
        assert!(!result.download_url.is_empty());
    }
}

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::loader::load_scripts_from_dir;
use crate::types::ScriptStatus;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Top-level manifest written as `profile.toml` in the exported bundle.
#[derive(Serialize, Deserialize)]
pub struct ProfileManifest {
    pub profile: ProfileInfo,
    pub stats: ProfileStats,
}

/// Metadata about the profile bundle.
#[derive(Serialize, Deserialize)]
pub struct ProfileInfo {
    pub name: String,
    pub glass_version: String,
    pub created: String,
    pub tech_stack: Vec<String>,
}

/// Aggregate counts of exported scripts by type.
#[derive(Serialize, Deserialize)]
pub struct ProfileStats {
    pub hook_scripts_count: usize,
    pub mcp_tools_count: usize,
}

/// Result of an import operation.
pub struct ImportResult {
    pub scripts_imported: usize,
    pub scripts_skipped: usize,
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// Bundle all **Confirmed** scripts from `scripts_dir` into a shareable profile
/// at `output_path`.
///
/// Layout of the output directory:
/// ```text
/// output_path/
///   profile.toml          -- ProfileManifest
///   scripts/
///     hooks/              -- hook scripts (.toml + .rhai)
///     tools/              -- mcp_tool scripts (.toml + .rhai)
/// ```
///
/// Each exported manifest has its `status` downgraded to `provisional` so that
/// the importer treats them as untrusted until confirmed locally.
pub fn export_profile(
    name: &str,
    scripts_dir: &Path,
    output_path: &Path,
    glass_version: &str,
    tech_stack: Vec<String>,
) -> anyhow::Result<()> {
    // Create output directory structure.
    let scripts_out = output_path.join("scripts");
    let hooks_out = scripts_out.join("hooks");
    let tools_out = scripts_out.join("tools");
    fs::create_dir_all(&hooks_out)?;
    fs::create_dir_all(&tools_out)?;

    let scripts = load_scripts_from_dir(scripts_dir);

    let mut hook_count: usize = 0;
    let mut tool_count: usize = 0;

    for script in &scripts {
        // Only export confirmed scripts.
        if script.manifest.status != ScriptStatus::Confirmed {
            continue;
        }

        // Determine target subdirectory based on script type.
        let target_dir = if script.manifest.script_type == "mcp_tool" {
            &tools_out
        } else {
            &hooks_out
        };

        // Build a copy of the manifest with status downgraded to provisional.
        let mut manifest = script.manifest.clone();
        manifest.status = ScriptStatus::Provisional;
        let toml_str = toml::to_string_pretty(&manifest)?;

        // Derive filenames from the manifest name.
        let base_name = &manifest.name;
        fs::write(target_dir.join(format!("{base_name}.toml")), toml_str)?;
        fs::write(target_dir.join(format!("{base_name}.rhai")), &script.source)?;

        if script.manifest.script_type == "mcp_tool" {
            tool_count += 1;
        } else {
            hook_count += 1;
        }
    }

    // Generate a simple date string without chrono -- epoch days approach.
    let created = simple_date_string();

    let profile_manifest = ProfileManifest {
        profile: ProfileInfo {
            name: name.to_string(),
            glass_version: glass_version.to_string(),
            created,
            tech_stack,
        },
        stats: ProfileStats {
            hook_scripts_count: hook_count,
            mcp_tools_count: tool_count,
        },
    };

    let manifest_toml = toml::to_string_pretty(&profile_manifest)?;
    fs::write(output_path.join("profile.toml"), manifest_toml)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// Import scripts from a profile bundle into `target_scripts_dir`.
///
/// Reads from `profile_path/scripts/` (hooks + tools subdirectories), copies
/// each script pair (.toml + .rhai) into the matching subdirectory under
/// `target_scripts_dir` with `status = provisional`.
///
/// Scripts whose name already exists in `target_scripts_dir` are skipped.
pub fn import_profile(
    profile_path: &Path,
    target_scripts_dir: &Path,
) -> anyhow::Result<ImportResult> {
    let profile_scripts = profile_path.join("scripts");

    // Collect existing script names so we can skip duplicates.
    let existing = load_scripts_from_dir(target_scripts_dir);
    let existing_names: std::collections::HashSet<String> =
        existing.iter().map(|s| s.manifest.name.clone()).collect();

    // Ensure target subdirectories exist.
    let target_hooks = target_scripts_dir.join("hooks");
    let target_tools = target_scripts_dir.join("tools");
    fs::create_dir_all(&target_hooks)?;
    fs::create_dir_all(&target_tools)?;

    let incoming = load_scripts_from_dir(&profile_scripts);

    let mut imported: usize = 0;
    let mut skipped: usize = 0;

    for script in &incoming {
        if existing_names.contains(&script.manifest.name) {
            skipped += 1;
            continue;
        }

        // Determine target subdirectory based on script type.
        let target_dir = if script.manifest.script_type == "mcp_tool" {
            &target_tools
        } else {
            &target_hooks
        };

        // Ensure the manifest has provisional status.
        let mut manifest = script.manifest.clone();
        manifest.status = ScriptStatus::Provisional;
        let toml_str = toml::to_string_pretty(&manifest)?;

        let base_name = &manifest.name;
        fs::write(target_dir.join(format!("{base_name}.toml")), toml_str)?;
        fs::write(target_dir.join(format!("{base_name}.rhai")), &script.source)?;

        imported += 1;
    }

    Ok(ImportResult {
        scripts_imported: imported,
        scripts_skipped: skipped,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Produce a "YYYY-MM-DD" date string using `SystemTime` without external
/// date/time crates.
fn simple_date_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Seconds per day.
    const DAY: u64 = 86400;
    let mut days = secs / DAY;

    // Walk from 1970 forward to find the year.
    let mut year: u64 = 1970;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    // Walk months to find month + day.
    let leap = is_leap(year);
    let month_days: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];

    let mut month: u64 = 1;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }

    let day = days + 1;
    format!("{year:04}-{month:02}-{day:02}")
}

fn is_leap(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ScriptManifest;
    use tempfile::TempDir;

    /// Helper: write a manifest + source pair into `dir`.
    fn write_script(dir: &Path, name: &str, status: &str, script_type: &str) {
        let manifest = format!(
            r#"name = "{name}"
hooks = ["command_complete"]
status = "{status}"
origin = "feedback"
version = 1
api_version = "1"
type = "{script_type}"
"#
        );
        fs::write(dir.join(format!("{name}.toml")), manifest).unwrap();
        fs::write(
            dir.join(format!("{name}.rhai")),
            format!("// source for {name}\nlet x = 1;\n"),
        )
        .unwrap();
    }

    #[test]
    fn export_and_import_roundtrip() {
        // -- Setup: source scripts dir with confirmed + provisional scripts --
        let src_dir = TempDir::new().unwrap();
        let hooks_dir = src_dir.path().join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        write_script(&hooks_dir, "good-hook", "confirmed", "hook");
        write_script(&hooks_dir, "pending-hook", "provisional", "hook");

        // -- Export --
        let export_dir = TempDir::new().unwrap();
        export_profile(
            "test-profile",
            src_dir.path(),
            export_dir.path(),
            "2.5.0",
            vec!["rust".to_string()],
        )
        .unwrap();

        // Verify profile.toml was created.
        let profile_toml = fs::read_to_string(export_dir.path().join("profile.toml")).unwrap();
        let manifest: ProfileManifest = toml::from_str(&profile_toml).unwrap();
        assert_eq!(manifest.profile.name, "test-profile");
        assert_eq!(manifest.profile.glass_version, "2.5.0");
        assert_eq!(manifest.stats.hook_scripts_count, 1);
        assert_eq!(manifest.stats.mcp_tools_count, 0);

        // Verify only the confirmed script was exported.
        let exported_hooks = export_dir.path().join("scripts").join("hooks");
        assert!(exported_hooks.join("good-hook.toml").is_file());
        assert!(exported_hooks.join("good-hook.rhai").is_file());
        assert!(!exported_hooks.join("pending-hook.toml").exists());

        // Verify the exported manifest has status = provisional.
        let exported_manifest_str =
            fs::read_to_string(exported_hooks.join("good-hook.toml")).unwrap();
        let exported_manifest: ScriptManifest = toml::from_str(&exported_manifest_str).unwrap();
        assert_eq!(exported_manifest.status, ScriptStatus::Provisional);

        // -- Import into a fresh directory --
        let import_target = TempDir::new().unwrap();
        let result = import_profile(export_dir.path(), import_target.path()).unwrap();

        assert_eq!(result.scripts_imported, 1);
        assert_eq!(result.scripts_skipped, 0);

        // Verify the imported script exists and is provisional.
        let imported_hooks = import_target.path().join("hooks");
        assert!(imported_hooks.join("good-hook.toml").is_file());
        assert!(imported_hooks.join("good-hook.rhai").is_file());

        let imported_manifest_str =
            fs::read_to_string(imported_hooks.join("good-hook.toml")).unwrap();
        let imported_manifest: ScriptManifest = toml::from_str(&imported_manifest_str).unwrap();
        assert_eq!(imported_manifest.status, ScriptStatus::Provisional);

        // -- Import again: should skip the duplicate --
        let result2 = import_profile(export_dir.path(), import_target.path()).unwrap();
        assert_eq!(result2.scripts_imported, 0);
        assert_eq!(result2.scripts_skipped, 1);
    }

    #[test]
    fn simple_date_string_format() {
        let date = simple_date_string();
        // Should be "YYYY-MM-DD" format.
        assert_eq!(date.len(), 10);
        assert_eq!(&date[4..5], "-");
        assert_eq!(&date[7..8], "-");
    }
}

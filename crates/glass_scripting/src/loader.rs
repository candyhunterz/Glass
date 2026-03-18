use std::collections::HashSet;
use std::fs;
use std::path::Path;

use tracing::warn;

use crate::types::{LoadedScript, ScriptManifest, ScriptStatus};

/// Subdirectories within a scripts root that are scanned for script manifests.
const SCRIPT_SUBDIRS: &[&str] = &["hooks", "tools", "feedback"];

/// Loads all scripts from a base scripts directory.
///
/// Scans the `hooks/`, `tools/`, and `feedback/` subdirectories under `base`.
/// For each `.toml` manifest file found, attempts to load a matching `.rhai`
/// source file. Scripts with `Archived` or `Rejected` status are skipped.
pub fn load_scripts_from_dir(base: &Path) -> Vec<LoadedScript> {
    let mut scripts = Vec::new();

    for subdir in SCRIPT_SUBDIRS {
        let dir = base.join(subdir);
        if !dir.is_dir() {
            continue;
        }

        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                warn!("Failed to read script directory {}: {}", dir.display(), e);
                continue;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }

            let manifest_str = match fs::read_to_string(&path) {
                Ok(s) => s,
                Err(e) => {
                    warn!("Failed to read manifest {}: {}", path.display(), e);
                    continue;
                }
            };

            let manifest: ScriptManifest = match toml::from_str(&manifest_str) {
                Ok(m) => m,
                Err(e) => {
                    warn!("Failed to parse manifest {}: {}", path.display(), e);
                    continue;
                }
            };

            // Skip archived or rejected scripts
            if manifest.status == ScriptStatus::Archived
                || manifest.status == ScriptStatus::Rejected
            {
                continue;
            }

            // Look for the matching .rhai source file
            let rhai_path = path.with_extension("rhai");
            if !rhai_path.is_file() {
                warn!(
                    "Manifest {} has no matching .rhai source file, skipping",
                    path.display()
                );
                continue;
            }

            let source = match fs::read_to_string(&rhai_path) {
                Ok(s) => s,
                Err(e) => {
                    warn!("Failed to read source file {}: {}", rhai_path.display(), e);
                    continue;
                }
            };

            scripts.push(LoadedScript {
                manifest,
                source,
                manifest_path: path,
                source_path: rhai_path,
            });
        }
    }

    scripts
}

/// Loads scripts from both project-local and global directories.
///
/// Project-local scripts (from `{project_root}/.glass/scripts/`) take precedence
/// over global scripts (from `~/.glass/scripts/`). A global script whose name
/// matches a project-local script is skipped to avoid duplicates.
pub fn load_all_scripts(project_root: &str) -> Vec<LoadedScript> {
    let mut scripts = Vec::new();
    let mut seen_names = HashSet::new();

    // Load project-local scripts first (higher precedence)
    let project_scripts_dir = Path::new(project_root).join(".glass").join("scripts");
    if project_scripts_dir.is_dir() {
        for script in load_scripts_from_dir(&project_scripts_dir) {
            seen_names.insert(script.manifest.name.clone());
            scripts.push(script);
        }
    }

    // Load global scripts, skipping any whose name matches a project-local script
    if let Some(home) = dirs::home_dir() {
        let global_scripts_dir = home.join(".glass").join("scripts");
        if global_scripts_dir.is_dir() {
            for script in load_scripts_from_dir(&global_scripts_dir) {
                if seen_names.contains(&script.manifest.name) {
                    continue;
                }
                seen_names.insert(script.manifest.name.clone());
                scripts.push(script);
            }
        }
    }

    scripts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper to write a minimal valid manifest TOML.
    fn write_manifest(dir: &Path, name: &str, status: &str) {
        let content = format!(
            r#"name = "{name}"
hooks = ["command_start"]
status = "{status}"
type = "hook"
"#
        );
        fs::write(dir.join(format!("{name}.toml")), content).unwrap();
    }

    /// Helper to write a minimal .rhai source file.
    fn write_source(dir: &Path, name: &str) {
        fs::write(
            dir.join(format!("{name}.rhai")),
            "// script source\nlet x = 1;\n",
        )
        .unwrap();
    }

    #[test]
    fn load_script_pair() {
        let tmp = TempDir::new().unwrap();
        let hooks_dir = tmp.path().join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        write_manifest(&hooks_dir, "my-hook", "confirmed");
        write_source(&hooks_dir, "my-hook");

        let scripts = load_scripts_from_dir(tmp.path());
        assert_eq!(scripts.len(), 1);

        let script = &scripts[0];
        assert_eq!(script.manifest.name, "my-hook");
        assert_eq!(script.manifest.status, ScriptStatus::Confirmed);
        assert!(script.source.contains("let x = 1;"));
        assert!(script.manifest_path.ends_with("my-hook.toml"));
        assert!(script.source_path.ends_with("my-hook.rhai"));
    }

    #[test]
    fn skip_archived_scripts() {
        let tmp = TempDir::new().unwrap();
        let hooks_dir = tmp.path().join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        write_manifest(&hooks_dir, "archived-hook", "archived");
        write_source(&hooks_dir, "archived-hook");

        write_manifest(&hooks_dir, "rejected-hook", "rejected");
        write_source(&hooks_dir, "rejected-hook");

        let scripts = load_scripts_from_dir(tmp.path());
        assert_eq!(
            scripts.len(),
            0,
            "Archived and rejected scripts should be skipped"
        );
    }

    #[test]
    fn skip_manifest_without_rhai() {
        let tmp = TempDir::new().unwrap();
        let hooks_dir = tmp.path().join("hooks");
        fs::create_dir_all(&hooks_dir).unwrap();

        // Only write the manifest, no .rhai file
        write_manifest(&hooks_dir, "orphan-hook", "confirmed");

        let scripts = load_scripts_from_dir(tmp.path());
        assert_eq!(
            scripts.len(),
            0,
            "Manifest without .rhai source should be skipped"
        );
    }

    #[test]
    fn load_all_scripts_project_overrides_global() {
        let project_tmp = TempDir::new().unwrap();
        let project_root = project_tmp.path();
        let project_scripts = project_root.join(".glass").join("scripts").join("hooks");
        fs::create_dir_all(&project_scripts).unwrap();

        write_manifest(&project_scripts, "shared-hook", "confirmed");
        write_source(&project_scripts, "shared-hook");

        // load_all_scripts with a project that has the script should load it
        let scripts = load_all_scripts(project_root.to_str().unwrap());

        // We should find at least the project-local script
        let found = scripts.iter().any(|s| s.manifest.name == "shared-hook");
        assert!(found, "Project-local script should be loaded");
    }

    #[test]
    fn load_from_multiple_subdirs() {
        let tmp = TempDir::new().unwrap();

        for subdir in &["hooks", "tools", "feedback"] {
            let dir = tmp.path().join(subdir);
            fs::create_dir_all(&dir).unwrap();
            write_manifest(&dir, &format!("{subdir}-script"), "provisional");
            write_source(&dir, &format!("{subdir}-script"));
        }

        let scripts = load_scripts_from_dir(tmp.path());
        assert_eq!(scripts.len(), 3);

        let names: HashSet<String> = scripts.iter().map(|s| s.manifest.name.clone()).collect();
        assert!(names.contains("hooks-script"));
        assert!(names.contains("tools-script"));
        assert!(names.contains("feedback-script"));
    }
}

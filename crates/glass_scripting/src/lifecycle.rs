use std::path::Path;

use anyhow::Context;

use crate::types::{ScriptManifest, ScriptStatus};

/// Read a script manifest from a TOML file on disk.
pub fn read_manifest(path: &Path) -> anyhow::Result<ScriptManifest> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("reading manifest {}", path.display()))?;
    let manifest: ScriptManifest = toml::from_str(&contents)
        .with_context(|| format!("parsing manifest {}", path.display()))?;
    Ok(manifest)
}

/// Write a script manifest back to a TOML file on disk.
pub fn write_manifest(path: &Path, manifest: &ScriptManifest) -> anyhow::Result<()> {
    let contents = toml::to_string_pretty(manifest).context("serializing manifest to TOML")?;
    std::fs::write(path, contents)
        .with_context(|| format!("writing manifest {}", path.display()))?;
    Ok(())
}

/// Promote a script to Confirmed status. Resets failure_count and stale_runs.
pub fn promote_script(manifest_path: &Path) -> anyhow::Result<()> {
    let mut manifest = read_manifest(manifest_path)?;
    manifest.status = ScriptStatus::Confirmed;
    manifest.failure_count = 0;
    manifest.stale_runs = 0;
    write_manifest(manifest_path, &manifest)
}

/// Reject a script by setting its status to Archived.
pub fn reject_script(manifest_path: &Path) -> anyhow::Result<()> {
    let mut manifest = read_manifest(manifest_path)?;
    manifest.status = ScriptStatus::Archived;
    write_manifest(manifest_path, &manifest)
}

/// Record a failure. Increments failure_count and auto-archives if >= 3
/// consecutive failures. Returns true if the script was auto-archived.
pub fn record_failure(manifest_path: &Path) -> anyhow::Result<bool> {
    let mut manifest = read_manifest(manifest_path)?;
    manifest.failure_count += 1;
    let auto_rejected = manifest.failure_count >= 3;
    if auto_rejected {
        manifest.status = ScriptStatus::Archived;
    }
    write_manifest(manifest_path, &manifest)?;
    Ok(auto_rejected)
}

/// Record a successful trigger. Increments trigger_count, resets failure_count
/// and stale_runs. Promotes a Stale script back to Confirmed.
pub fn record_trigger(manifest_path: &Path) -> anyhow::Result<()> {
    let mut manifest = read_manifest(manifest_path)?;
    manifest.trigger_count += 1;
    manifest.failure_count = 0;
    manifest.stale_runs = 0;
    if manifest.status == ScriptStatus::Stale {
        manifest.status = ScriptStatus::Confirmed;
    }
    write_manifest(manifest_path, &manifest)
}

/// Increment stale_runs. Marks status as Stale at `stale_threshold` and
/// Archived at `archive_threshold`.
pub fn increment_stale(
    manifest_path: &Path,
    stale_threshold: u32,
    archive_threshold: u32,
) -> anyhow::Result<()> {
    let mut manifest = read_manifest(manifest_path)?;
    manifest.stale_runs += 1;
    if manifest.stale_runs >= archive_threshold {
        manifest.status = ScriptStatus::Archived;
    } else if manifest.stale_runs >= stale_threshold {
        manifest.status = ScriptStatus::Stale;
    }
    write_manifest(manifest_path, &manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_test_manifest(status: &str, failure_count: u32, stale_runs: u32) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            r#"name = "test-script"
hooks = ["command_complete"]
status = "{status}"
origin = "feedback"
version = 1
api_version = "1"
type = "hook"
failure_count = {failure_count}
trigger_count = 0
stale_runs = {stale_runs}
"#,
        )
        .unwrap();
        file
    }

    #[test]
    fn promote_provisional_to_confirmed() {
        let file = write_test_manifest("provisional", 2, 1);
        promote_script(file.path()).unwrap();
        let m = read_manifest(file.path()).unwrap();
        assert_eq!(m.status, ScriptStatus::Confirmed);
        assert_eq!(m.failure_count, 0);
        assert_eq!(m.stale_runs, 0);
    }

    #[test]
    fn reject_script_sets_archived() {
        let file = write_test_manifest("confirmed", 0, 0);
        reject_script(file.path()).unwrap();
        let m = read_manifest(file.path()).unwrap();
        assert_eq!(m.status, ScriptStatus::Archived);
    }

    #[test]
    fn increment_failure_rejects_at_three() {
        let file = write_test_manifest("confirmed", 2, 0);
        let auto_rejected = record_failure(file.path()).unwrap();
        assert!(auto_rejected);
        let m = read_manifest(file.path()).unwrap();
        assert_eq!(m.status, ScriptStatus::Archived);
        assert_eq!(m.failure_count, 3);
    }

    #[test]
    fn record_trigger_resets_stale() {
        let file = write_test_manifest("stale", 1, 5);
        record_trigger(file.path()).unwrap();
        let m = read_manifest(file.path()).unwrap();
        assert_eq!(m.status, ScriptStatus::Confirmed);
        assert_eq!(m.failure_count, 0);
        assert_eq!(m.stale_runs, 0);
        assert_eq!(m.trigger_count, 1);
    }
}

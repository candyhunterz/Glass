use std::collections::HashMap;

use crate::types::{HookPoint, LoadedScript, ScriptOrigin, ScriptStatus};

/// Registry that groups loaded scripts by hook point, sorts them by
/// priority, and enforces a per-hook cap.
#[derive(Debug)]
pub struct HookRegistry {
    hooks: HashMap<HookPoint, Vec<LoadedScript>>,
}

/// Return a numeric priority for a script under the **default** ordering.
///
/// Lower value = higher priority = runs first.
///
/// Ordering: Confirmed+Feedback > Confirmed+User / all User > Provisional > Stale
fn default_priority(script: &LoadedScript) -> u32 {
    match (&script.manifest.status, &script.manifest.origin) {
        (ScriptStatus::Confirmed, ScriptOrigin::Feedback) => 0,
        (ScriptStatus::Confirmed, ScriptOrigin::User) => 1,
        (_, ScriptOrigin::User) => 1, // any user script at same tier as confirmed-user
        (ScriptStatus::Provisional, _) => 2,
        (ScriptStatus::Stale, _) => 3,
        _ => 4,
    }
}

/// Return a numeric priority for a script under the **McpRequest** ordering.
///
/// Lower value = higher priority = runs first.
///
/// Ordering: User > Confirmed > Provisional > Stale
fn mcp_request_priority(script: &LoadedScript) -> u32 {
    match (&script.manifest.status, &script.manifest.origin) {
        (_, ScriptOrigin::User) => 0,
        (ScriptStatus::Confirmed, _) => 1,
        (ScriptStatus::Provisional, _) => 2,
        (ScriptStatus::Stale, _) => 3,
        _ => 4,
    }
}

impl HookRegistry {
    /// Build a new registry from a list of loaded scripts.
    ///
    /// Scripts are grouped by each hook point declared in their manifest,
    /// sorted by priority (ordering depends on the hook), and truncated
    /// to `max_per_hook`.
    pub fn new(scripts: Vec<LoadedScript>, max_per_hook: u32) -> Self {
        let mut hooks: HashMap<HookPoint, Vec<LoadedScript>> = HashMap::new();

        for script in scripts {
            // Skip rejected/archived scripts entirely.
            if script.manifest.status == ScriptStatus::Rejected
                || script.manifest.status == ScriptStatus::Archived
            {
                continue;
            }

            for hook in &script.manifest.hooks {
                hooks
                    .entry(hook.clone())
                    .or_default()
                    .push(script.clone());
            }
        }

        let limit = max_per_hook as usize;
        for (hook, list) in hooks.iter_mut() {
            list.sort_by_key(|s| {
                if *hook == HookPoint::McpRequest {
                    mcp_request_priority(s)
                } else {
                    default_priority(s)
                }
            });
            list.truncate(limit);
        }

        Self { hooks }
    }

    /// Return the sorted scripts registered for a given hook point.
    pub fn scripts_for(&self, hook: HookPoint) -> &[LoadedScript] {
        self.hooks.get(&hook).map_or(&[], |v| v.as_slice())
    }

    /// Check whether any scripts are registered for a given hook point.
    pub fn has_scripts_for(&self, hook: HookPoint) -> bool {
        self.hooks
            .get(&hook)
            .map_or(false, |v| !v.is_empty())
    }

    /// Return all unique scripts across every hook, deduplicated by
    /// `(name, source_path)`.
    pub fn all_scripts(&self) -> Vec<&LoadedScript> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();

        for list in self.hooks.values() {
            for script in list {
                let key = (
                    script.manifest.name.clone(),
                    script.source_path.clone(),
                );
                if seen.insert(key) {
                    result.push(script);
                }
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ScriptManifest, ScriptOrigin, ScriptStatus};
    use std::path::PathBuf;

    /// Helper to build a `LoadedScript` with minimal boilerplate.
    fn make_script(
        name: &str,
        hooks: Vec<HookPoint>,
        status: ScriptStatus,
        origin: ScriptOrigin,
    ) -> LoadedScript {
        LoadedScript {
            manifest: ScriptManifest {
                name: name.to_string(),
                hooks,
                status,
                origin,
                version: 1,
                api_version: "1".to_string(),
                created: None,
                failure_count: 0,
                trigger_count: 0,
                stale_runs: 0,
                script_type: "hook".to_string(),
                description: None,
                params: None,
            },
            source: String::new(),
            manifest_path: PathBuf::from(format!("/scripts/{name}/manifest.toml")),
            source_path: PathBuf::from(format!("/scripts/{name}/main.rhai")),
        }
    }

    #[test]
    fn registry_groups_by_hook() {
        let scripts = vec![
            make_script(
                "snap",
                vec![HookPoint::CommandStart, HookPoint::CommandComplete],
                ScriptStatus::Confirmed,
                ScriptOrigin::Feedback,
            ),
            make_script(
                "log",
                vec![HookPoint::CommandComplete],
                ScriptStatus::Confirmed,
                ScriptOrigin::User,
            ),
            make_script(
                "tab-hook",
                vec![HookPoint::TabCreate],
                ScriptStatus::Provisional,
                ScriptOrigin::User,
            ),
        ];

        let reg = HookRegistry::new(scripts, 10);

        assert_eq!(reg.scripts_for(HookPoint::CommandStart).len(), 1);
        assert_eq!(reg.scripts_for(HookPoint::CommandStart)[0].manifest.name, "snap");

        assert_eq!(reg.scripts_for(HookPoint::CommandComplete).len(), 2);

        assert_eq!(reg.scripts_for(HookPoint::TabCreate).len(), 1);
        assert_eq!(reg.scripts_for(HookPoint::TabCreate)[0].manifest.name, "tab-hook");

        assert!(!reg.has_scripts_for(HookPoint::SessionStart));
    }

    #[test]
    fn priority_order_default() {
        let scripts = vec![
            make_script(
                "provisional",
                vec![HookPoint::CommandStart],
                ScriptStatus::Provisional,
                ScriptOrigin::Feedback,
            ),
            make_script(
                "confirmed-feedback",
                vec![HookPoint::CommandStart],
                ScriptStatus::Confirmed,
                ScriptOrigin::Feedback,
            ),
            make_script(
                "user",
                vec![HookPoint::CommandStart],
                ScriptStatus::Confirmed,
                ScriptOrigin::User,
            ),
            make_script(
                "stale",
                vec![HookPoint::CommandStart],
                ScriptStatus::Stale,
                ScriptOrigin::Feedback,
            ),
        ];

        let reg = HookRegistry::new(scripts, 10);
        let ordered = reg.scripts_for(HookPoint::CommandStart);

        assert_eq!(ordered.len(), 4);
        assert_eq!(ordered[0].manifest.name, "confirmed-feedback");
        assert_eq!(ordered[1].manifest.name, "user");
        assert_eq!(ordered[2].manifest.name, "provisional");
        assert_eq!(ordered[3].manifest.name, "stale");
    }

    #[test]
    fn mcp_request_reverses_priority() {
        let scripts = vec![
            make_script(
                "confirmed-feedback",
                vec![HookPoint::McpRequest],
                ScriptStatus::Confirmed,
                ScriptOrigin::Feedback,
            ),
            make_script(
                "user",
                vec![HookPoint::McpRequest],
                ScriptStatus::Confirmed,
                ScriptOrigin::User,
            ),
            make_script(
                "provisional",
                vec![HookPoint::McpRequest],
                ScriptStatus::Provisional,
                ScriptOrigin::Feedback,
            ),
        ];

        let reg = HookRegistry::new(scripts, 10);
        let ordered = reg.scripts_for(HookPoint::McpRequest);

        assert_eq!(ordered.len(), 3);
        // User comes first for McpRequest
        assert_eq!(ordered[0].manifest.name, "user");
        assert_eq!(ordered[1].manifest.name, "confirmed-feedback");
        assert_eq!(ordered[2].manifest.name, "provisional");
    }

    #[test]
    fn enforces_per_hook_limit() {
        let scripts: Vec<LoadedScript> = (0..15)
            .map(|i| {
                make_script(
                    &format!("script-{i}"),
                    vec![HookPoint::CommandStart],
                    ScriptStatus::Confirmed,
                    ScriptOrigin::Feedback,
                )
            })
            .collect();

        let reg = HookRegistry::new(scripts, 10);
        assert_eq!(reg.scripts_for(HookPoint::CommandStart).len(), 10);
    }
}

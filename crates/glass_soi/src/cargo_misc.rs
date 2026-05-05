//! Parser for cargo subcommand output (`cargo add`, `cargo update`, `cargo fetch`, `cargo install`).
//!
//! Extracts `PackageEvent` records from cargo operations that are not
//! covered by `cargo_build` or `cargo_test` parsers.

use std::sync::OnceLock;

use regex::Regex;

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity};

/// Cargo status line: right-justified verb followed by content.
fn re_status_line() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^\s*(Updating|Adding|Removing|Removed|Compiling|Installing|Installed|Downloaded|Downloading|Locking|Fetching)\s+(.+)$",
        )
        .expect("cargo status regex")
    })
}

/// Extract "package vVERSION" from the content portion.
fn re_pkg_version() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(\S+)\s+v(\S+)").expect("cargo pkg version regex"))
}

/// Cargo update line: "package vOLD -> vNEW"
fn re_update_arrow() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(\S+)\s+v(\S+)\s*->\s*v(\S+)").expect("cargo update arrow regex")
    })
}

/// Cargo error line
fn re_error() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^error(?:\[E\d+\])?:\s*(.+)").expect("cargo error regex"))
}

/// Parse cargo subcommand output into structured `PackageEvent` records.
pub fn parse(output: &str) -> ParsedOutput {
    let raw_line_count = output.lines().count();
    let raw_byte_count = output.len();

    let mut records: Vec<OutputRecord> = Vec::new();
    let mut action_counts: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    let mut has_error = false;

    for line in output.lines() {
        if line.len() > 4096 {
            continue;
        }

        if let Some(caps) = re_error().captures(line) {
            has_error = true;
            records.push(OutputRecord::PackageEvent {
                action: "error".to_string(),
                package: String::new(),
                version: None,
                detail: Some(caps[1].to_string()),
            });
            continue;
        }

        if let Some(caps) = re_status_line().captures(line) {
            let verb = caps[1].to_lowercase();
            let content = caps[2].trim();

            *action_counts.entry(verb.clone()).or_insert(0) += 1;

            if let Some(arrow_caps) = re_update_arrow().captures(content) {
                let pkg = arrow_caps[1].to_string();
                let old_ver = arrow_caps[2].to_string();
                let new_ver = arrow_caps[3].to_string();
                records.push(OutputRecord::PackageEvent {
                    action: verb,
                    package: pkg,
                    version: Some(new_ver),
                    detail: Some(format!("from v{old_ver}")),
                });
                continue;
            }

            if let Some(pv_caps) = re_pkg_version().captures(content) {
                let pkg = pv_caps[1].to_string();
                let ver = pv_caps[2].to_string();
                records.push(OutputRecord::PackageEvent {
                    action: verb,
                    package: pkg,
                    version: Some(ver),
                    detail: None,
                });
                continue;
            }

            records.push(OutputRecord::PackageEvent {
                action: verb,
                package: content.to_string(),
                version: None,
                detail: None,
            });
            continue;
        }
    }

    if records.is_empty() {
        return crate::freeform_parse(output, Some(OutputType::Cargo), None);
    }

    let severity = if has_error {
        Severity::Error
    } else {
        Severity::Success
    };

    let mut parts: Vec<String> = Vec::new();
    let mut sorted_actions: Vec<_> = action_counts.into_iter().collect();
    sorted_actions.sort_by_key(|action| std::cmp::Reverse(action.1));
    for (action, count) in &sorted_actions {
        parts.push(format!("{count} {action}"));
    }
    let one_line = if parts.is_empty() {
        "cargo output parsed".to_string()
    } else {
        parts.join(", ")
    };

    let token_estimate = one_line.split_whitespace().count() + records.len() * 5 + raw_line_count;

    ParsedOutput {
        output_type: OutputType::Cargo,
        summary: OutputSummary {
            one_line,
            token_estimate,
            severity,
        },
        records,
        raw_line_count,
        raw_byte_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARGO_ADD: &str =
        "    Updating crates.io index\n      Adding serde v1.0.200 to dependencies\n";

    const CARGO_UPDATE: &str = "    Updating crates.io index\n    Locking 3 packages to latest compatible versions\n    Updating serde v1.0.199 -> v1.0.200\n    Updating serde_json v1.0.119 -> v1.0.120\n";

    const CARGO_INSTALL: &str = "    Updating crates.io index\n  Installing ripgrep v14.1.0\n   Compiling memchr v2.7.1\n   Compiling regex v1.10.3\n   Compiling ripgrep v14.1.0\n   Installed package `ripgrep v14.1.0` (executable `rg`)\n";

    const CARGO_FETCH: &str = "    Updating crates.io index\n  Downloaded serde v1.0.200\n  Downloaded serde_json v1.0.120\n  Downloaded 2 crates (512.3 KB) in 0.42s\n";

    #[test]
    fn cargo_add_output() {
        let parsed = parse(CARGO_ADD);
        assert_eq!(parsed.output_type, OutputType::Cargo);
        assert_eq!(parsed.summary.severity, Severity::Success);
        let adding = parsed.records.iter().find_map(|r| {
            if let OutputRecord::PackageEvent {
                action,
                package,
                version,
                ..
            } = r
            {
                if action == "adding" {
                    return Some((package.clone(), version.clone()));
                }
            }
            None
        });
        let (pkg, ver) = adding.expect("should have adding event");
        assert_eq!(pkg, "serde");
        assert_eq!(ver.as_deref(), Some("1.0.200"));
    }

    #[test]
    fn cargo_update_output() {
        let parsed = parse(CARGO_UPDATE);
        let updates: Vec<_> = parsed
            .records
            .iter()
            .filter(
                |r| matches!(r, OutputRecord::PackageEvent { action, .. } if action == "updating"),
            )
            .collect();
        assert!(updates.len() >= 2);
    }

    #[test]
    fn cargo_install_output() {
        let parsed = parse(CARGO_INSTALL);
        let actions: Vec<String> = parsed
            .records
            .iter()
            .filter_map(|r| {
                if let OutputRecord::PackageEvent { action, .. } = r {
                    Some(action.clone())
                } else {
                    None
                }
            })
            .collect();
        assert!(actions.contains(&"installing".to_string()));
        assert!(actions.contains(&"compiling".to_string()));
    }

    #[test]
    fn cargo_fetch_output() {
        let parsed = parse(CARGO_FETCH);
        let downloaded = parsed
            .records
            .iter()
            .filter(|r| {
                matches!(r, OutputRecord::PackageEvent { action, .. } if action == "downloaded")
            })
            .count();
        assert!(downloaded >= 2);
    }

    #[test]
    fn empty_fallback() {
        let parsed = parse("");
        assert_eq!(parsed.output_type, OutputType::Cargo);
        assert!(matches!(
            parsed.records[0],
            OutputRecord::FreeformChunk { .. }
        ));
    }
}

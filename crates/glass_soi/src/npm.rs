//! Parser for `npm` / `npx` output.
//!
//! Extracts `PackageEvent` records from npm install/update/audit output,
//! including added/removed/audited counts and vulnerability details.

use std::sync::OnceLock;

use regex::Regex;

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity};

// --- Compiled regex patterns (OnceLock — compiled once, reused across calls) ---

fn re_added() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)added (\d+) packages?").expect("npm added regex"))
}

fn re_removed() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)removed (\d+) packages?").expect("npm removed regex"))
}

fn re_audited() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)audited (\d+) packages?").expect("npm audited regex"))
}

fn re_vulns() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(\d+) vulnerabilit(?:y|ies)").expect("npm vulns regex")
    })
}

fn re_vuln_detail() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(\d+) (critical|high|moderate|low)").expect("npm vuln detail regex")
    })
}

fn re_deprecated() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)npm warn deprecated ([^:]+):\s*(.+)").expect("npm deprecated regex")
    })
}

/// Parse npm install/update/audit output into structured `PackageEvent` records.
///
/// Recognises:
/// - "added N packages" → `PackageEvent { action: "added", package: "N packages" }`
/// - "removed N packages" → `PackageEvent { action: "removed", ... }`
/// - "audited N packages" → `PackageEvent { action: "audited", ... }`
/// - "N vulnerabilities (1 moderate, 2 high)" → `PackageEvent { action: "vulnerabilities", detail: "..." }`
/// - "npm warn deprecated X: msg" → `PackageEvent { action: "deprecated", package: X, detail: msg }`
///
/// Returns a freeform fallback if no patterns match.
pub fn parse(output: &str) -> ParsedOutput {
    let raw_line_count = output.lines().count();
    let raw_byte_count = output.len();

    let mut records: Vec<OutputRecord> = Vec::new();
    let mut added_count: Option<u64> = None;
    let mut audited_count: Option<u64> = None;
    let mut vuln_count: Option<u64> = None;
    let mut has_critical_high = false;
    let mut has_moderate_low = false;

    for line in output.lines() {
        // Skip lines that are excessively long (guard against backtracking on adversarial input)
        if line.len() > 4096 {
            continue;
        }

        // npm warn deprecated X: message — check first (before package-count patterns)
        if let Some(caps) = re_deprecated().captures(line) {
            let pkg = caps[1].trim().to_string();
            let msg = caps[2].trim().to_string();
            records.push(OutputRecord::PackageEvent {
                action: "deprecated".to_string(),
                package: pkg,
                version: None,
                detail: Some(msg),
            });
            continue;
        }

        // N vulnerabilities (detail breakdown on same line)
        // Check before added/removed/audited because vuln lines don't contain those words.
        if let Some(vuln_caps) = re_vulns().captures(line) {
            let total: u64 = vuln_caps[1].parse().unwrap_or(0);
            vuln_count = Some(total);

            // Collect all "N severity" pairs on this line for the detail string
            let mut detail_parts: Vec<String> = Vec::new();
            for caps in re_vuln_detail().captures_iter(line) {
                let count = &caps[1];
                let severity = &caps[2];
                detail_parts.push(format!("{count} {severity}"));
                match severity {
                    "critical" | "high" => has_critical_high = true,
                    "moderate" | "low" => has_moderate_low = true,
                    _ => {}
                }
            }

            let detail = if detail_parts.is_empty() {
                None
            } else {
                Some(detail_parts.join(", "))
            };

            records.push(OutputRecord::PackageEvent {
                action: "vulnerabilities".to_string(),
                package: String::new(),
                version: None,
                detail,
            });
            continue;
        }

        // A single npm line can contain both "added N" and "audited M" — e.g.:
        //   "added 142 packages, and audited 143 packages in 3s"
        // So we do NOT use `continue` after matching added; we fall through to also check audited.
        let mut matched_on_line = false;

        // added N packages
        if let Some(caps) = re_added().captures(line) {
            let n: u64 = caps[1].parse().unwrap_or(0);
            added_count = Some(n);
            records.push(OutputRecord::PackageEvent {
                action: "added".to_string(),
                package: format!("{n} packages"),
                version: None,
                detail: None,
            });
            matched_on_line = true;
        }

        // removed N packages
        if let Some(caps) = re_removed().captures(line) {
            let n: u64 = caps[1].parse().unwrap_or(0);
            records.push(OutputRecord::PackageEvent {
                action: "removed".to_string(),
                package: format!("{n} packages"),
                version: None,
                detail: None,
            });
            matched_on_line = true;
        }

        // audited N packages (can appear on same line as "added")
        if let Some(caps) = re_audited().captures(line) {
            let n: u64 = caps[1].parse().unwrap_or(0);
            audited_count = Some(n);
            records.push(OutputRecord::PackageEvent {
                action: "audited".to_string(),
                package: format!("{n} packages"),
                version: None,
                detail: None,
            });
            matched_on_line = true;
        }

        let _ = matched_on_line; // suppress unused warning
    }

    // If nothing was extracted, fall back to freeform
    if records.is_empty() {
        return crate::freeform_parse(output, Some(OutputType::Npm), None);
    }

    // Determine severity
    let severity = if has_critical_high {
        Severity::Error
    } else if has_moderate_low || vuln_count.is_some_and(|v| v > 0) {
        Severity::Warning
    } else {
        Severity::Success
    };

    // Build summary one-liner
    let mut parts: Vec<String> = Vec::new();
    if let Some(n) = added_count {
        parts.push(format!("added {n}"));
    }
    if let Some(n) = audited_count {
        parts.push(format!("audited {n}"));
    }
    if let Some(n) = vuln_count {
        parts.push(format!("{n} vulnerabilities"));
    }
    let one_line = if parts.is_empty() {
        "npm output parsed".to_string()
    } else {
        parts.join(", ")
    };

    let token_estimate = one_line.split_whitespace().count()
        + records.len() * 5
        + raw_line_count;

    ParsedOutput {
        output_type: OutputType::Npm,
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

    // npm 7+ style: "added N packages, and audited M packages in Xs"
    const NPM_INSTALL_WITH_VULNS: &str = r#"npm warn deprecated lodash@4.17.20: Critical security issue
npm warn deprecated inflight@1.0.6: This module is not supported

added 142 packages, and audited 143 packages in 3s

14 packages are looking for funding
  run `npm fund` for details

3 vulnerabilities (1 moderate, 2 high)

To address all issues, run:
  npm audit fix
"#;

    // npm 6 style: "added N packages from M contributors"
    const NPM_INSTALL_NPM6: &str = r#"added 142 packages from 87 contributors and audited 143 packages in 4.219s
found 0 vulnerabilities
"#;

    const NPM_CLEAN_INSTALL: &str = r#"added 56 packages, and audited 57 packages in 2s

found 0 vulnerabilities
"#;

    const NPM_REMOVE: &str = r#"removed 3 packages, and audited 201 packages in 1s

found 0 vulnerabilities
"#;

    #[test]
    fn npm_install_with_vulns_produces_package_events() {
        let parsed = parse(NPM_INSTALL_WITH_VULNS);
        assert_eq!(parsed.output_type, OutputType::Npm);

        // Should have: 2 deprecated + added + audited + vulns = 5 records
        let events: Vec<_> = parsed
            .records
            .iter()
            .filter_map(|r| {
                if let OutputRecord::PackageEvent { action, .. } = r {
                    Some(action.as_str())
                } else {
                    None
                }
            })
            .collect();

        assert!(events.contains(&"added"), "should have added event");
        assert!(events.contains(&"audited"), "should have audited event");
        assert!(events.contains(&"vulnerabilities"), "should have vulns event");
        assert_eq!(
            events.iter().filter(|&&a| a == "deprecated").count(),
            2,
            "should have 2 deprecated events"
        );
    }

    #[test]
    fn npm_install_with_vulns_added_count() {
        let parsed = parse(NPM_INSTALL_WITH_VULNS);
        let added = parsed.records.iter().find_map(|r| {
            if let OutputRecord::PackageEvent { action, package, .. } = r {
                if action == "added" {
                    return Some(package.clone());
                }
            }
            None
        });
        assert_eq!(added, Some("142 packages".to_string()));
    }

    #[test]
    fn npm_install_with_vulns_audited_count() {
        let parsed = parse(NPM_INSTALL_WITH_VULNS);
        let audited = parsed.records.iter().find_map(|r| {
            if let OutputRecord::PackageEvent { action, package, .. } = r {
                if action == "audited" {
                    return Some(package.clone());
                }
            }
            None
        });
        assert_eq!(audited, Some("143 packages".to_string()));
    }

    #[test]
    fn npm_install_with_vulns_vuln_detail() {
        let parsed = parse(NPM_INSTALL_WITH_VULNS);
        let vuln = parsed.records.iter().find_map(|r| {
            if let OutputRecord::PackageEvent { action, detail, .. } = r {
                if action == "vulnerabilities" {
                    return detail.clone();
                }
            }
            None
        });
        let detail = vuln.expect("should have vulnerability detail");
        assert!(detail.contains("moderate"), "detail should mention moderate");
        assert!(detail.contains("high"), "detail should mention high");
    }

    #[test]
    fn npm_install_high_vulns_severity_is_error() {
        let parsed = parse(NPM_INSTALL_WITH_VULNS);
        assert_eq!(
            parsed.summary.severity,
            Severity::Error,
            "high vulns should produce Error severity"
        );
    }

    #[test]
    fn npm_deprecated_warning_parsed() {
        let parsed = parse(NPM_INSTALL_WITH_VULNS);
        let deprecated: Vec<_> = parsed
            .records
            .iter()
            .filter_map(|r| {
                if let OutputRecord::PackageEvent { action, package, detail, .. } = r {
                    if action == "deprecated" {
                        return Some((package.clone(), detail.clone()));
                    }
                }
                None
            })
            .collect();
        assert_eq!(deprecated.len(), 2);
        // First deprecated: lodash@4.17.20
        assert!(deprecated[0].0.contains("lodash"), "first deprecated package");
        assert!(
            deprecated[0].1.as_ref().map_or(false, |d| d.contains("Critical")),
            "first deprecated detail"
        );
    }

    #[test]
    fn npm6_permissive_regex_extracts_count() {
        let parsed = parse(NPM_INSTALL_NPM6);
        let added = parsed.records.iter().find_map(|r| {
            if let OutputRecord::PackageEvent { action, package, .. } = r {
                if action == "added" {
                    return Some(package.clone());
                }
            }
            None
        });
        assert_eq!(
            added,
            Some("142 packages".to_string()),
            "npm6 style should still extract count"
        );
    }

    #[test]
    fn npm_clean_install_severity_is_success() {
        // "found 0 vulnerabilities" doesn't match our vuln regex (no count + vulns pattern)
        // so severity should be Success
        let parsed = parse(NPM_CLEAN_INSTALL);
        assert_eq!(parsed.summary.severity, Severity::Success);
    }

    #[test]
    fn npm_remove_produces_removed_event() {
        let parsed = parse(NPM_REMOVE);
        let removed = parsed.records.iter().find_map(|r| {
            if let OutputRecord::PackageEvent { action, package, .. } = r {
                if action == "removed" {
                    return Some(package.clone());
                }
            }
            None
        });
        assert_eq!(removed, Some("3 packages".to_string()));
    }

    #[test]
    fn npm_empty_output_freeform_fallback() {
        let parsed = parse("");
        assert_eq!(parsed.output_type, OutputType::Npm);
        assert!(
            matches!(parsed.records[0], OutputRecord::FreeformChunk { .. }),
            "empty output should produce freeform chunk"
        );
    }

    #[test]
    fn npm_unrecognized_output_freeform_fallback() {
        let parsed = parse("some random text that doesn't match npm patterns\n");
        assert_eq!(parsed.output_type, OutputType::Npm);
        assert!(
            matches!(parsed.records[0], OutputRecord::FreeformChunk { .. }),
            "unrecognized output should produce freeform chunk"
        );
    }

    #[test]
    fn npm_summary_one_line_contains_counts() {
        let parsed = parse(NPM_INSTALL_WITH_VULNS);
        assert!(
            parsed.summary.one_line.contains("142"),
            "one_line should contain added count: {}",
            parsed.summary.one_line
        );
        assert!(
            parsed.summary.one_line.contains("143"),
            "one_line should contain audited count: {}",
            parsed.summary.one_line
        );
    }
}

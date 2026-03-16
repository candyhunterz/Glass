//! Parser for `pip` output.
//!
//! Extracts `PackageEvent` records from pip install/uninstall output,
//! including installed packages, satisfied requirements, warnings, and errors.

use std::sync::OnceLock;

use regex::Regex;

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity};

fn re_successfully_installed() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)Successfully installed\s+(.+)").expect("pip installed regex")
    })
}

fn re_pkg_version() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(\S+)-(\d\S*)").expect("pip pkg version regex"))
}

fn re_requirement_satisfied() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)Requirement already satisfied:\s*(\S+)").expect("pip satisfied regex")
    })
}

fn re_collecting() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)Collecting\s+(\S+)").expect("pip collecting regex"))
}

fn re_downloading() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)Downloading\s+(\S+)").expect("pip downloading regex"))
}

fn re_error() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^(?:ERROR|error):\s*(.+)").expect("pip error regex"))
}

fn re_warning() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^(?:WARNING|DEPRECATION):\s*(.+)").expect("pip warning regex")
    })
}

/// Parse pip install/uninstall output into structured `PackageEvent` records.
pub fn parse(output: &str) -> ParsedOutput {
    let raw_line_count = output.lines().count();
    let raw_byte_count = output.len();

    let mut records: Vec<OutputRecord> = Vec::new();
    let mut installed_count: u64 = 0;
    let mut warning_count: u64 = 0;
    let mut error_count: u64 = 0;

    for line in output.lines() {
        if line.len() > 4096 {
            continue;
        }

        if let Some(caps) = re_error().captures(line) {
            error_count += 1;
            records.push(OutputRecord::PackageEvent {
                action: "error".to_string(),
                package: String::new(),
                version: None,
                detail: Some(caps[1].to_string()),
            });
            continue;
        }

        if let Some(caps) = re_warning().captures(line) {
            warning_count += 1;
            records.push(OutputRecord::PackageEvent {
                action: "warning".to_string(),
                package: String::new(),
                version: None,
                detail: Some(caps[1].to_string()),
            });
            continue;
        }

        if let Some(caps) = re_successfully_installed().captures(line) {
            let pkg_str = &caps[1];
            for pkg_caps in re_pkg_version().captures_iter(pkg_str) {
                installed_count += 1;
                records.push(OutputRecord::PackageEvent {
                    action: "installed".to_string(),
                    package: pkg_caps[1].to_string(),
                    version: Some(pkg_caps[2].to_string()),
                    detail: None,
                });
            }
            continue;
        }

        if let Some(caps) = re_requirement_satisfied().captures(line) {
            records.push(OutputRecord::PackageEvent {
                action: "satisfied".to_string(),
                package: caps[1].to_string(),
                version: None,
                detail: None,
            });
            continue;
        }

        if let Some(caps) = re_collecting().captures(line) {
            records.push(OutputRecord::PackageEvent {
                action: "collecting".to_string(),
                package: caps[1].to_string(),
                version: None,
                detail: None,
            });
            continue;
        }

        if let Some(caps) = re_downloading().captures(line) {
            records.push(OutputRecord::PackageEvent {
                action: "downloading".to_string(),
                package: caps[1].to_string(),
                version: None,
                detail: None,
            });
            continue;
        }
    }

    if records.is_empty() {
        return crate::freeform_parse(output, Some(OutputType::Pip), None);
    }

    let severity = if error_count > 0 {
        Severity::Error
    } else if warning_count > 0 {
        Severity::Warning
    } else {
        Severity::Success
    };

    let mut parts: Vec<String> = Vec::new();
    if installed_count > 0 {
        parts.push(format!("installed {installed_count}"));
    }
    if warning_count > 0 {
        parts.push(format!("{warning_count} warnings"));
    }
    if error_count > 0 {
        parts.push(format!("{error_count} errors"));
    }
    let one_line = if parts.is_empty() {
        "pip output parsed".to_string()
    } else {
        parts.join(", ")
    };

    let token_estimate = one_line.split_whitespace().count() + records.len() * 5 + raw_line_count;

    ParsedOutput {
        output_type: OutputType::Pip,
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

    const PIP_INSTALL_SUCCESS: &str = "Collecting requests\n  Downloading requests-2.31.0-py3-none-any.whl (62 kB)\nCollecting flask\n  Downloading flask-3.0.0-py3-none-any.whl (99 kB)\nInstalling collected packages: urllib3, requests, flask\nSuccessfully installed flask-3.0.0 requests-2.31.0 urllib3-2.1.0\n";

    const PIP_REQUIREMENT_SATISFIED: &str = "Requirement already satisfied: requests in /usr/lib/python3/dist-packages (2.31.0)\nRequirement already satisfied: flask in /usr/lib/python3/dist-packages (3.0.0)\nRequirement already satisfied: urllib3 in /usr/lib/python3/dist-packages (2.1.0)\n";

    const PIP_WITH_WARNINGS: &str = "DEPRECATION: Python 2.7 reached the end of its life on January 1st, 2020.\nWARNING: pip is configured with locations that require TLS/SSL.\nCollecting requests\nSuccessfully installed requests-2.31.0\n";

    const PIP_ERROR: &str = "Collecting nonexistent-package\nERROR: Could not find a version that satisfies the requirement nonexistent-package\nERROR: No matching distribution found for nonexistent-package\n";

    #[test]
    fn pip_install_success() {
        let parsed = parse(PIP_INSTALL_SUCCESS);
        assert_eq!(parsed.output_type, OutputType::Pip);
        assert_eq!(parsed.summary.severity, Severity::Success);
        let installed: Vec<_> = parsed
            .records
            .iter()
            .filter_map(|r| {
                if let OutputRecord::PackageEvent {
                    action,
                    package,
                    version,
                    ..
                } = r
                {
                    if action == "installed" {
                        return Some((package.clone(), version.clone()));
                    }
                }
                None
            })
            .collect();
        assert_eq!(installed.len(), 3);
        assert!(installed.iter().any(|(p, _)| p == "flask"));
        assert!(installed
            .iter()
            .any(|(p, v)| p == "requests" && v.as_deref() == Some("2.31.0")));
    }

    #[test]
    fn pip_requirement_satisfied() {
        let parsed = parse(PIP_REQUIREMENT_SATISFIED);
        assert_eq!(parsed.output_type, OutputType::Pip);
        let satisfied: Vec<_> = parsed
            .records
            .iter()
            .filter_map(|r| {
                if let OutputRecord::PackageEvent {
                    action, package, ..
                } = r
                {
                    if action == "satisfied" {
                        return Some(package.clone());
                    }
                }
                None
            })
            .collect();
        assert_eq!(satisfied.len(), 3);
        assert!(satisfied.contains(&"requests".to_string()));
    }

    #[test]
    fn pip_install_with_warnings() {
        let parsed = parse(PIP_WITH_WARNINGS);
        assert_eq!(parsed.summary.severity, Severity::Warning);
        let warnings = parsed
            .records
            .iter()
            .filter(
                |r| matches!(r, OutputRecord::PackageEvent { action, .. } if action == "warning"),
            )
            .count();
        assert_eq!(warnings, 2);
    }

    #[test]
    fn pip_error_output() {
        let parsed = parse(PIP_ERROR);
        assert_eq!(parsed.summary.severity, Severity::Error);
        let errors = parsed
            .records
            .iter()
            .filter(|r| matches!(r, OutputRecord::PackageEvent { action, .. } if action == "error"))
            .count();
        assert_eq!(errors, 2);
    }

    #[test]
    fn pip_empty_fallback() {
        let parsed = parse("");
        assert_eq!(parsed.output_type, OutputType::Pip);
        assert!(matches!(
            parsed.records[0],
            OutputRecord::FreeformChunk { .. }
        ));
    }
}

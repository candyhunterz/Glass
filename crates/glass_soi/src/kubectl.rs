//! Parser for `kubectl` command output.
//!
//! Extracts `GenericDiagnostic` records from kubectl apply results and
//! get pods table output.

use std::sync::OnceLock;

use regex::Regex;

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity};

// --- Compiled regex patterns (OnceLock — compiled once, reused across calls) ---

fn re_apply_result() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(\S+) (configured|created|unchanged|deleted)$")
            .expect("kubectl apply result regex")
    })
}

fn re_pod_table_header() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^NAME\s+READY\s+STATUS").expect("kubectl pod table header regex")
    })
}

fn re_get_all_header() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^NAME\s+").expect("kubectl generic table header regex")
    })
}

/// Parse kubectl apply and get pods output into structured `GenericDiagnostic` records.
///
/// Recognizes:
/// - "deployment.apps/myapp configured" → `GenericDiagnostic { severity: Info, message: full line }`
/// - Pod table rows with NAME/STATUS columns → `GenericDiagnostic` per row, severity based on status
///
/// Status severity mapping:
/// - Running / Completed / Succeeded → Info
/// - Pending / Terminating → Warning
/// - Error / CrashLoopBackOff / ImagePullBackOff / OOMKilled / Failed → Error
///
/// Returns a freeform fallback if no patterns match.
pub fn parse(output: &str) -> ParsedOutput {
    let stripped = crate::strip_ansi(output);
    let raw_line_count = output.lines().count();
    let raw_byte_count = output.len();

    let mut records: Vec<OutputRecord> = Vec::new();
    let mut in_pod_table = false;

    for line in stripped.lines() {
        // Skip excessively long lines
        if line.len() > 4096 {
            continue;
        }

        let trimmed = line.trim();

        // Detect pod table header
        if re_pod_table_header().is_match(trimmed) {
            in_pod_table = true;
            continue;
        }

        // If in pod table, parse table rows
        if in_pod_table {
            if trimmed.is_empty() {
                in_pod_table = false;
                continue;
            }

            // Skip additional header lines (like for wide output)
            if re_get_all_header().is_match(trimmed) {
                continue;
            }

            // Parse table row: split on whitespace, NAME is col 0, STATUS is col 2 (for pods)
            let cols: Vec<&str> = trimmed.split_whitespace().collect();
            if cols.len() >= 3 {
                let pod_name = cols[0];
                // For pods: NAME READY STATUS RESTARTS AGE
                let status = cols[2];
                let severity = pod_status_severity(status);

                records.push(OutputRecord::GenericDiagnostic {
                    file: None,
                    line: None,
                    severity,
                    message: format!("pod/{pod_name}: {status}"),
                });
            }
            continue;
        }

        // kubectl apply result: "deployment.apps/myapp configured"
        if let Some(caps) = re_apply_result().captures(trimmed) {
            let resource = &caps[1];
            let result = &caps[2];
            records.push(OutputRecord::GenericDiagnostic {
                file: None,
                line: None,
                severity: Severity::Info,
                message: format!("{resource} {result}"),
            });
            continue;
        }
    }

    // If nothing was extracted, fall back to freeform
    if records.is_empty() {
        return crate::freeform_parse(output, Some(OutputType::Kubectl), None);
    }

    // Determine highest severity
    let severity = if records
        .iter()
        .any(|r| matches!(r, OutputRecord::GenericDiagnostic { severity: Severity::Error, .. }))
    {
        Severity::Error
    } else if records.iter().any(|r| {
        matches!(
            r,
            OutputRecord::GenericDiagnostic {
                severity: Severity::Warning,
                ..
            }
        )
    }) {
        Severity::Warning
    } else {
        Severity::Info
    };

    // Build summary one-liner
    let error_count = records
        .iter()
        .filter(|r| {
            matches!(
                r,
                OutputRecord::GenericDiagnostic {
                    severity: Severity::Error,
                    ..
                }
            )
        })
        .count();
    let warn_count = records
        .iter()
        .filter(|r| {
            matches!(
                r,
                OutputRecord::GenericDiagnostic {
                    severity: Severity::Warning,
                    ..
                }
            )
        })
        .count();
    let info_count = records
        .iter()
        .filter(|r| {
            matches!(
                r,
                OutputRecord::GenericDiagnostic {
                    severity: Severity::Info,
                    ..
                }
            )
        })
        .count();

    let one_line = if error_count > 0 || warn_count > 0 {
        format!(
            "{} kubectl records ({error_count} errors, {warn_count} warnings, {info_count} info)",
            records.len()
        )
    } else {
        format!("{} kubectl records", records.len())
    };

    let token_estimate = one_line.split_whitespace().count() + records.len() * 4 + raw_line_count;

    ParsedOutput {
        output_type: OutputType::Kubectl,
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

/// Map pod status strings to Severity levels.
fn pod_status_severity(status: &str) -> Severity {
    match status {
        "Running" | "Completed" | "Succeeded" => Severity::Info,
        "Pending" | "Terminating" | "ContainerCreating" | "Init:0/1" | "PodInitializing" => {
            Severity::Warning
        }
        _ if status.starts_with("Error")
            || status.starts_with("CrashLoop")
            || status.starts_with("ImagePullBackOff")
            || status.starts_with("ErrImagePull")
            || status == "OOMKilled"
            || status == "Failed" =>
        {
            Severity::Error
        }
        _ => Severity::Info,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KUBECTL_APPLY: &str = "deployment.apps/myapp configured\nservice/myapp-svc created\nconfigmap/myapp-config unchanged\n";

    const KUBECTL_GET_PODS: &str = "NAME                     READY   STATUS    RESTARTS   AGE\nmyapp-7d9f8b-abc12       1/1     Running   0          2d\nmyapp-7d9f8b-def34       1/1     Running   0          2d\ndb-pod-xyz               0/1     Pending   0          5m\ncrashing-pod-aaa         0/1     CrashLoopBackOff   5   10m\n";

    #[test]
    fn kubectl_apply_produces_generic_diagnostic_records() {
        let parsed = parse(KUBECTL_APPLY);
        assert_eq!(parsed.output_type, OutputType::Kubectl);

        let diag_records: Vec<_> = parsed
            .records
            .iter()
            .filter(|r| matches!(r, OutputRecord::GenericDiagnostic { .. }))
            .collect();

        assert_eq!(diag_records.len(), 3, "should have 3 apply result records");
    }

    #[test]
    fn kubectl_apply_configured_is_info() {
        let parsed = parse(KUBECTL_APPLY);
        let configured = parsed.records.iter().find(|r| {
            if let OutputRecord::GenericDiagnostic { message, .. } = r {
                message.contains("configured")
            } else {
                false
            }
        });
        assert!(configured.is_some(), "should find configured record");
        if let Some(OutputRecord::GenericDiagnostic { severity, .. }) = configured {
            assert_eq!(*severity, Severity::Info);
        }
    }

    #[test]
    fn kubectl_apply_messages_contain_resource_and_result() {
        let parsed = parse(KUBECTL_APPLY);
        let messages: Vec<_> = parsed
            .records
            .iter()
            .filter_map(|r| {
                if let OutputRecord::GenericDiagnostic { message, .. } = r {
                    Some(message.as_str())
                } else {
                    None
                }
            })
            .collect();

        assert!(
            messages.iter().any(|m| m.contains("deployment.apps/myapp")),
            "should have deployment message"
        );
        assert!(
            messages.iter().any(|m| m.contains("service/myapp-svc")),
            "should have service message"
        );
    }

    #[test]
    fn kubectl_get_pods_produces_diagnostic_per_pod() {
        let parsed = parse(KUBECTL_GET_PODS);
        assert_eq!(parsed.output_type, OutputType::Kubectl);

        assert_eq!(parsed.records.len(), 4, "should have 4 pod records");
    }

    #[test]
    fn kubectl_get_pods_running_is_info() {
        let parsed = parse(KUBECTL_GET_PODS);
        let running = parsed.records.iter().find(|r| {
            if let OutputRecord::GenericDiagnostic { message, .. } = r {
                message.contains("myapp-7d9f8b-abc12")
            } else {
                false
            }
        });
        assert!(running.is_some(), "should find running pod");
        if let Some(OutputRecord::GenericDiagnostic { severity, .. }) = running {
            assert_eq!(*severity, Severity::Info, "Running pod should be Info");
        }
    }

    #[test]
    fn kubectl_get_pods_pending_is_warning() {
        let parsed = parse(KUBECTL_GET_PODS);
        let pending = parsed.records.iter().find(|r| {
            if let OutputRecord::GenericDiagnostic { message, .. } = r {
                message.contains("db-pod")
            } else {
                false
            }
        });
        assert!(pending.is_some(), "should find pending pod");
        if let Some(OutputRecord::GenericDiagnostic { severity, .. }) = pending {
            assert_eq!(*severity, Severity::Warning, "Pending pod should be Warning");
        }
    }

    #[test]
    fn kubectl_get_pods_crashloop_is_error() {
        let parsed = parse(KUBECTL_GET_PODS);
        let crashing = parsed.records.iter().find(|r| {
            if let OutputRecord::GenericDiagnostic { message, .. } = r {
                message.contains("crashing-pod")
            } else {
                false
            }
        });
        assert!(crashing.is_some(), "should find crashing pod");
        if let Some(OutputRecord::GenericDiagnostic { severity, .. }) = crashing {
            assert_eq!(
                *severity,
                Severity::Error,
                "CrashLoopBackOff pod should be Error"
            );
        }
    }

    #[test]
    fn kubectl_unrecognized_output_falls_through_to_freeform() {
        let parsed = parse("some random text that is not kubectl output\n");
        assert_eq!(parsed.output_type, OutputType::Kubectl);
        assert!(
            matches!(parsed.records[0], OutputRecord::FreeformChunk { .. }),
            "unrecognized output should produce freeform chunk"
        );
    }

    #[test]
    fn kubectl_mixed_pod_statuses_highest_severity() {
        // Has CrashLoopBackOff (Error) and Pending (Warning) and Running (Info)
        let parsed = parse(KUBECTL_GET_PODS);
        assert_eq!(
            parsed.summary.severity,
            Severity::Error,
            "should report highest severity (Error for CrashLoopBackOff)"
        );
    }
}

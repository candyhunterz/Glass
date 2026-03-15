//! Parser for `terraform` output.
//!
//! Extracts structured records from `terraform plan` and `terraform apply` output,
//! including resource changes, plan summaries, and errors.

use std::sync::OnceLock;

use regex::Regex;

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity};

fn re_plan_summary() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"Plan:\s*(\d+)\s*to add,\s*(\d+)\s*to change,\s*(\d+)\s*to destroy")
            .expect("terraform plan regex")
    })
}

fn re_resource_action() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"#\s*(\S+)\s+will be\s+(created|destroyed|updated in-place|replaced)")
            .expect("terraform resource regex")
    })
}

fn re_apply_complete() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"Apply complete!\s*Resources:\s*(\d+)\s*added,\s*(\d+)\s*changed,\s*(\d+)\s*destroyed",
        )
        .expect("terraform apply regex")
    })
}

fn re_error() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^Error:\s*(.+)").expect("terraform error regex"))
}

/// Parse terraform plan/apply output into structured records.
pub fn parse(output: &str) -> ParsedOutput {
    let clean = crate::ansi::strip_ansi(output);
    let raw_line_count = output.lines().count();
    let raw_byte_count = output.len();

    let mut records: Vec<OutputRecord> = Vec::new();
    let mut has_error = false;
    let mut to_add: u32 = 0;
    let mut to_change: u32 = 0;
    let mut to_destroy: u32 = 0;
    let mut apply_complete = false;

    for line in clean.lines() {
        if line.len() > 4096 {
            continue;
        }

        if let Some(caps) = re_error().captures(line) {
            has_error = true;
            records.push(OutputRecord::GenericDiagnostic {
                file: None,
                line: None,
                severity: Severity::Error,
                message: caps[1].to_string(),
            });
            continue;
        }

        if let Some(caps) = re_plan_summary().captures(line) {
            to_add = caps[1].parse().unwrap_or(0);
            to_change = caps[2].parse().unwrap_or(0);
            to_destroy = caps[3].parse().unwrap_or(0);
            records.push(OutputRecord::GenericDiagnostic {
                file: None,
                line: None,
                severity: Severity::Info,
                message: format!(
                    "Plan: {to_add} to add, {to_change} to change, {to_destroy} to destroy"
                ),
            });
            continue;
        }

        if let Some(caps) = re_apply_complete().captures(line) {
            apply_complete = true;
            to_add = caps[1].parse().unwrap_or(0);
            to_change = caps[2].parse().unwrap_or(0);
            to_destroy = caps[3].parse().unwrap_or(0);
            records.push(OutputRecord::GenericDiagnostic {
                file: None,
                line: None,
                severity: Severity::Success,
                message: format!(
                    "Apply complete: {to_add} added, {to_change} changed, {to_destroy} destroyed"
                ),
            });
            continue;
        }

        if let Some(caps) = re_resource_action().captures(line) {
            let resource = caps[1].to_string();
            let action = &caps[2];
            let severity = if action == "destroyed" {
                Severity::Warning
            } else {
                Severity::Info
            };
            records.push(OutputRecord::GenericDiagnostic {
                file: None,
                line: None,
                severity,
                message: format!("{resource} will be {action}"),
            });
            continue;
        }
    }

    if records.is_empty() {
        return crate::freeform_parse(output, Some(OutputType::Terraform), None);
    }

    let severity = if has_error || to_destroy > 0 {
        Severity::Error
    } else if to_change > 0 {
        Severity::Warning
    } else if apply_complete || to_add > 0 {
        Severity::Success
    } else {
        Severity::Info
    };

    let one_line = if apply_complete {
        format!("Apply complete: {to_add} added, {to_change} changed, {to_destroy} destroyed")
    } else if to_add > 0 || to_change > 0 || to_destroy > 0 {
        format!("Plan: {to_add} to add, {to_change} to change, {to_destroy} to destroy")
    } else if has_error {
        "terraform error".to_string()
    } else {
        "terraform output parsed".to_string()
    };

    let token_estimate = one_line.split_whitespace().count() + records.len() * 5 + raw_line_count;

    ParsedOutput {
        output_type: OutputType::Terraform,
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

    #[test]
    fn terraform_plan_mixed() {
        let output = "  # aws_instance.web will be created\n  # aws_s3_bucket.logs will be destroyed\n  # aws_sg.ssh will be updated in-place\nPlan: 1 to add, 1 to change, 1 to destroy.\n";
        let parsed = parse(output);
        assert_eq!(parsed.output_type, OutputType::Terraform);
        assert_eq!(parsed.summary.severity, Severity::Error);
        assert!(parsed.summary.one_line.contains("1 to add"));
        assert!(parsed.summary.one_line.contains("1 to destroy"));
    }

    #[test]
    fn terraform_apply_complete() {
        let output = "Apply complete! Resources: 2 added, 0 changed, 1 destroyed.\n";
        let parsed = parse(output);
        assert!(parsed.summary.one_line.contains("Apply complete"));
    }

    #[test]
    fn terraform_error() {
        let output = "Error: Invalid provider configuration\n";
        let parsed = parse(output);
        assert_eq!(parsed.summary.severity, Severity::Error);
    }

    #[test]
    fn terraform_plan_add_only() {
        let output = "  # aws_instance.web will be created\nPlan: 2 to add, 0 to change, 0 to destroy.\n";
        let parsed = parse(output);
        assert_eq!(parsed.summary.severity, Severity::Success);
    }

    #[test]
    fn empty_fallback() {
        let parsed = parse("");
        assert_eq!(parsed.output_type, OutputType::Terraform);
        assert!(matches!(parsed.records[0], OutputRecord::FreeformChunk { .. }));
    }
}

//! Parser for `docker` and `docker-compose` command output.
//!
//! Extracts `DockerEvent` records from docker build (legacy and BuildKit)
//! and docker compose output.

use std::sync::OnceLock;

use regex::Regex;

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity};

// --- Compiled regex patterns (OnceLock — compiled once, reused across calls) ---

fn re_legacy_step() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^Step (\d+)/(\d+) : (.+)$").expect("docker legacy step regex")
    })
}

fn re_buildkit_step() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^#(\d+) (.+)$").expect("docker buildkit step regex"))
}

fn re_buildkit_error() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^#\d+ ERROR: (.+)$").expect("docker buildkit error regex")
    })
}

fn re_compose_container() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"Container (\S+)\s+(Started|Stopped|Error|Created|Running|Healthy|Exited)")
            .expect("docker compose container regex")
    })
}

fn re_compose_running() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^\[[\+\-]\] Running (\d+)/(\d+)").expect("docker compose running regex")
    })
}

fn re_image_tag() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"Successfully (?:built|tagged) (\S+)").expect("docker image tag regex")
    })
}

/// Parse docker build and compose output into structured `DockerEvent` records.
///
/// Recognizes:
/// - "Step N/M : FROM ..." → `DockerEvent { action: "build-step", ... }`
/// - "#N [1/3] FROM ..." (BuildKit) → `DockerEvent { action: "build-step", ... }`
/// - "#N ERROR: ..." (BuildKit) → `DockerEvent { action: "error", ... }`
/// - "Successfully built X" / "Successfully tagged X" → `DockerEvent { action: "built", ... }`
/// - "Container X Started/Stopped" → `DockerEvent { action: "compose-up" or "compose-error", ... }`
/// - "[+] Running N/M" → `DockerEvent { action: "compose-up", ... }`
///
/// Returns a freeform fallback if no patterns match.
pub fn parse(output: &str) -> ParsedOutput {
    let stripped = crate::strip_ansi(output);
    let raw_line_count = output.lines().count();
    let raw_byte_count = output.len();

    let mut records: Vec<OutputRecord> = Vec::new();
    let mut has_errors = false;

    for line in stripped.lines() {
        // Skip excessively long lines
        if line.len() > 4096 {
            continue;
        }

        // BuildKit error (check before general buildkit step to match more specific pattern)
        if let Some(caps) = re_buildkit_error().captures(line) {
            has_errors = true;
            records.push(OutputRecord::DockerEvent {
                action: "error".to_string(),
                image: None,
                detail: caps[1].trim().to_string(),
            });
            continue;
        }

        // Legacy build step: "Step 1/5 : FROM ubuntu:22.04"
        if let Some(caps) = re_legacy_step().captures(line) {
            let step_num = &caps[1];
            let step_total = &caps[2];
            let instruction = caps[3].trim();
            records.push(OutputRecord::DockerEvent {
                action: "build-step".to_string(),
                image: None,
                detail: format!("Step {step_num}/{step_total}: {instruction}"),
            });
            continue;
        }

        // BuildKit step: "#2 [1/3] FROM ubuntu:22.04"
        if let Some(caps) = re_buildkit_step().captures(line) {
            let step_id = &caps[1];
            let rest = caps[2].trim();
            // Only capture lines that look like build steps (contain instruction keywords)
            // Skip timing lines like "#2 DONE 0.0s"
            if rest.starts_with('[')
                || rest.starts_with("FROM")
                || rest.starts_with("RUN")
                || rest.starts_with("COPY")
                || rest.starts_with("ADD")
                || rest.starts_with("ENV")
                || rest.starts_with("EXPOSE")
                || rest.starts_with("CMD")
                || rest.starts_with("ENTRYPOINT")
                || rest.starts_with("WORKDIR")
                || rest.starts_with("LABEL")
                || rest.starts_with("ARG")
            {
                records.push(OutputRecord::DockerEvent {
                    action: "build-step".to_string(),
                    image: None,
                    detail: format!("#{step_id} {rest}"),
                });
                continue;
            }
        }

        // "Successfully built abc123" / "Successfully tagged myimage:latest"
        if let Some(caps) = re_image_tag().captures(line) {
            let image = caps[1].to_string();
            let action = if line.contains("tagged") {
                "tagged"
            } else {
                "built"
            };
            records.push(OutputRecord::DockerEvent {
                action: action.to_string(),
                image: Some(image.clone()),
                detail: format!("{action} {image}"),
            });
            continue;
        }

        // Compose "[+] Running 3/3"
        if let Some(caps) = re_compose_running().captures(line) {
            let running = &caps[1];
            let total = &caps[2];
            records.push(OutputRecord::DockerEvent {
                action: "compose-up".to_string(),
                image: None,
                detail: format!("Running {running}/{total}"),
            });
            continue;
        }

        // Compose container status: "Container myapp-db-1  Started"
        if let Some(caps) = re_compose_container().captures(line) {
            let container = caps[1].to_string();
            let status = &caps[2];
            let is_error = status == "Error" || status == "Exited";
            if is_error {
                has_errors = true;
            }
            let action = if is_error {
                "compose-error"
            } else {
                "compose-up"
            };
            records.push(OutputRecord::DockerEvent {
                action: action.to_string(),
                image: Some(container.clone()),
                detail: format!("{container} {status}"),
            });
            continue;
        }

        // Compose error lines (containing "Error" not already captured)
        if line.to_lowercase().contains("error") && line.starts_with(" ") {
            // Only capture indented error lines to avoid false positives
            if line.trim().starts_with("Error") || line.trim().starts_with("error:") {
                has_errors = true;
                records.push(OutputRecord::DockerEvent {
                    action: "error".to_string(),
                    image: None,
                    detail: line.trim().to_string(),
                });
                continue;
            }
        }
    }

    // If nothing was extracted, fall back to freeform
    if records.is_empty() {
        return crate::freeform_parse(output, Some(OutputType::Docker), None);
    }

    // Determine severity
    let severity = if has_errors {
        Severity::Error
    } else {
        Severity::Info
    };

    // Build summary one-liner
    let step_count = records
        .iter()
        .filter(|r| matches!(r, OutputRecord::DockerEvent { action, .. } if action == "build-step"))
        .count();
    let error_count = records
        .iter()
        .filter(|r| {
            matches!(r, OutputRecord::DockerEvent { action, .. } if action == "error" || action == "compose-error")
        })
        .count();

    let one_line = if error_count > 0 {
        format!("{error_count} docker errors, {step_count} build steps")
    } else if step_count > 0 {
        format!("{step_count} build steps")
    } else {
        format!("{} docker events", records.len())
    };

    let token_estimate = one_line.split_whitespace().count() + records.len() * 4 + raw_line_count;

    ParsedOutput {
        output_type: OutputType::Docker,
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

    const DOCKER_LEGACY_BUILD: &str = "Sending build context to Docker daemon  5.12kB\nStep 1/5 : FROM ubuntu:22.04\n ---> abc123def456\nStep 2/5 : RUN apt-get update\n ---> Running in xyz789\nStep 3/5 : COPY . /app\n ---> a1b2c3d4e5f6\nStep 4/5 : WORKDIR /app\n ---> 11223344556677\nStep 5/5 : CMD [\"./app\"]\n ---> 998877665544\nSuccessfully built 998877665544\nSuccessfully tagged myapp:latest\n";

    const DOCKER_BUILDKIT: &str = "#1 [internal] load build definition from Dockerfile\n#1 DONE 0.0s\n\n#2 [1/3] FROM ubuntu:22.04\n#2 DONE 1.2s\n\n#3 [2/3] RUN apt-get update\n#3 DONE 15.4s\n\n#4 ERROR: failed to solve: failed to read dockerfile\n";

    const DOCKER_COMPOSE: &str = "[+] Running 3/3\n Container myapp-db-1  Started\n Container myapp-web-1  Started\n Container myapp-proxy-1  Started\n";

    #[test]
    fn docker_legacy_build_produces_build_step_events() {
        let parsed = parse(DOCKER_LEGACY_BUILD);
        assert_eq!(parsed.output_type, OutputType::Docker);

        let step_records: Vec<_> = parsed
            .records
            .iter()
            .filter(|r| {
                matches!(r, OutputRecord::DockerEvent { action, .. } if action == "build-step")
            })
            .collect();

        assert!(!step_records.is_empty(), "should have build-step records");
    }

    #[test]
    fn docker_legacy_build_first_step_detail() {
        let parsed = parse(DOCKER_LEGACY_BUILD);
        let first_step = parsed.records.iter().find(|r| {
            matches!(r, OutputRecord::DockerEvent { action, .. } if action == "build-step")
        });
        if let Some(OutputRecord::DockerEvent { detail, .. }) = first_step {
            assert!(
                detail.contains("FROM ubuntu:22.04"),
                "first step detail should contain FROM instruction: {detail}"
            );
        } else {
            panic!("expected a build-step record");
        }
    }

    #[test]
    fn docker_legacy_build_success_event() {
        let parsed = parse(DOCKER_LEGACY_BUILD);
        let built_record = parsed
            .records
            .iter()
            .find(|r| matches!(r, OutputRecord::DockerEvent { action, .. } if action == "built"));
        assert!(built_record.is_some(), "should have built record");
        if let Some(OutputRecord::DockerEvent { image, .. }) = built_record {
            assert!(image.is_some(), "built record should have image");
        }
    }

    #[test]
    fn docker_buildkit_produces_error_event() {
        let parsed = parse(DOCKER_BUILDKIT);
        assert_eq!(parsed.output_type, OutputType::Docker);

        let error_records: Vec<_> = parsed
            .records
            .iter()
            .filter(|r| matches!(r, OutputRecord::DockerEvent { action, .. } if action == "error"))
            .collect();

        assert!(!error_records.is_empty(), "should have error records");
    }

    #[test]
    fn docker_buildkit_error_severity_is_error() {
        let parsed = parse(DOCKER_BUILDKIT);
        assert_eq!(
            parsed.summary.severity,
            Severity::Error,
            "build errors should produce Error severity"
        );
    }

    #[test]
    fn docker_buildkit_step_from_captured() {
        let parsed = parse(DOCKER_BUILDKIT);
        let step_records: Vec<_> = parsed
            .records
            .iter()
            .filter(|r| {
                matches!(r, OutputRecord::DockerEvent { action, .. } if action == "build-step")
            })
            .collect();
        assert!(!step_records.is_empty(), "should capture BuildKit FROM step");
    }

    #[test]
    fn docker_compose_produces_compose_up_events() {
        let parsed = parse(DOCKER_COMPOSE);
        assert_eq!(parsed.output_type, OutputType::Docker);

        let compose_records: Vec<_> = parsed
            .records
            .iter()
            .filter(|r| {
                matches!(r, OutputRecord::DockerEvent { action, .. } if action == "compose-up")
            })
            .collect();

        assert!(
            !compose_records.is_empty(),
            "should have compose-up records"
        );
    }

    #[test]
    fn docker_compose_container_names_captured() {
        let parsed = parse(DOCKER_COMPOSE);
        let container_records: Vec<_> = parsed
            .records
            .iter()
            .filter_map(|r| {
                if let OutputRecord::DockerEvent {
                    action,
                    image: Some(img),
                    ..
                } = r
                {
                    if action == "compose-up" {
                        return Some(img.as_str());
                    }
                }
                None
            })
            .collect();

        assert!(!container_records.is_empty(), "should have container names");
        assert!(
            container_records.contains(&"myapp-db-1"),
            "should contain db container"
        );
    }

    #[test]
    fn docker_unrecognized_output_falls_through_to_freeform() {
        let parsed = parse("some random text that is not docker output\n");
        assert_eq!(parsed.output_type, OutputType::Docker);
        assert!(
            matches!(parsed.records[0], OutputRecord::FreeformChunk { .. }),
            "unrecognized output should produce freeform chunk"
        );
    }

    #[test]
    fn docker_no_errors_severity_is_info() {
        let parsed = parse(DOCKER_COMPOSE);
        assert_eq!(
            parsed.summary.severity,
            Severity::Info,
            "successful compose should produce Info severity"
        );
    }
}

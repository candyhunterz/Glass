//! Parser for `git` command output.
//!
//! Extracts `GitEvent` records from git status, diff, log, and merge output.

use std::sync::OnceLock;

use regex::Regex;

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity};

// --- Compiled regex patterns (OnceLock — compiled once, reused across calls) ---

fn re_git_stat_summary() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(\d+) files? changed(?:, (\d+) insertions?\(\+\))?(?:, (\d+) deletions?\(-\))?",
        )
        .expect("git stat summary regex")
    })
}

fn re_git_log_oneline() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([0-9a-f]{7,12}) (.+)$").expect("git log oneline regex"))
}

fn re_git_status_file() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^\s+(modified|new file|deleted|renamed|copied|both modified|both deleted|added by us|added by them|deleted by us|deleted by them):\s+(.+)$")
            .expect("git status file regex")
    })
}

/// Parse git command output into structured `GitEvent` records.
///
/// Recognizes:
/// - "On branch X" → `GitEvent { action: "status", detail: "on branch X" }`
/// - File status lines ("modified: file.rs") → `GitEvent { action: "status", detail: "..." }`
/// - "N files changed, M insertions(+), K deletions(-)" → `GitEvent { action: "diff-stat", ... }`
/// - "HASH message" (log --oneline) → `GitEvent { action: "log", detail: "..." }`
/// - "CONFLICT ..." → `GitEvent { action: "conflict", detail: "..." }`
/// - "nothing to commit" / "working tree clean" → `GitEvent { action: "status", detail: "clean" }`
///
/// Returns a freeform fallback if no patterns match.
pub fn parse(output: &str) -> ParsedOutput {
    let stripped = crate::strip_ansi(output);
    let raw_line_count = output.lines().count();
    let raw_byte_count = output.len();

    let mut records: Vec<OutputRecord> = Vec::new();
    let mut has_conflicts = false;
    let mut branch_name: Option<String> = None;

    for line in stripped.lines() {
        // Skip excessively long lines
        if line.len() > 4096 {
            continue;
        }

        // Conflict markers
        if line.starts_with("CONFLICT ") {
            has_conflicts = true;
            records.push(OutputRecord::GitEvent {
                action: "conflict".to_string(),
                detail: line.trim().to_string(),
                files_changed: None,
                insertions: None,
                deletions: None,
            });
            continue;
        }

        // "On branch main"
        if let Some(branch) = line.strip_prefix("On branch ") {
            let b = branch.trim().to_string();
            branch_name = Some(b.clone());
            records.push(OutputRecord::GitEvent {
                action: "status".to_string(),
                detail: format!("on branch {b}"),
                files_changed: None,
                insertions: None,
                deletions: None,
            });
            continue;
        }

        // "HEAD detached at ..."
        if line.starts_with("HEAD detached at ") {
            records.push(OutputRecord::GitEvent {
                action: "status".to_string(),
                detail: line.trim().to_string(),
                files_changed: None,
                insertions: None,
                deletions: None,
            });
            continue;
        }

        // "nothing to commit" or "working tree clean"
        if line.contains("nothing to commit") || line.contains("working tree clean") {
            records.push(OutputRecord::GitEvent {
                action: "status".to_string(),
                detail: "clean".to_string(),
                files_changed: None,
                insertions: None,
                deletions: None,
            });
            continue;
        }

        // "Untracked files:" section header
        if line.trim() == "Untracked files:" {
            records.push(OutputRecord::GitEvent {
                action: "status".to_string(),
                detail: "untracked files present".to_string(),
                files_changed: None,
                insertions: None,
                deletions: None,
            });
            continue;
        }

        // "Changes not staged for commit:" / "Changes to be committed:" section headers
        if line.trim() == "Changes not staged for commit:"
            || line.trim() == "Changes to be committed:"
        {
            // Section headers — skip, let file lines below provide detail
            continue;
        }

        // Diff stat summary: "2 files changed, 42 insertions(+), 3 deletions(-)"
        if let Some(caps) = re_git_stat_summary().captures(line) {
            let files_changed: u32 = caps[1].parse().unwrap_or(0);
            let insertions: Option<u32> = caps.get(2).and_then(|m| m.as_str().parse().ok());
            let deletions: Option<u32> = caps.get(3).and_then(|m| m.as_str().parse().ok());
            records.push(OutputRecord::GitEvent {
                action: "diff-stat".to_string(),
                detail: line.trim().to_string(),
                files_changed: Some(files_changed),
                insertions,
                deletions,
            });
            continue;
        }

        // File status lines: "    modified: src/main.rs"
        if let Some(caps) = re_git_status_file().captures(line) {
            let status = caps[1].trim().to_string();
            let file = caps[2].trim().to_string();
            records.push(OutputRecord::GitEvent {
                action: "status".to_string(),
                detail: format!("{status}: {file}"),
                files_changed: None,
                insertions: None,
                deletions: None,
            });
            continue;
        }

        // git log --oneline: "a1b2c3d feat: add feature"
        if let Some(caps) = re_git_log_oneline().captures(line.trim()) {
            let hash = caps[1].to_string();
            let message = caps[2].to_string();
            records.push(OutputRecord::GitEvent {
                action: "log".to_string(),
                detail: format!("{hash} {message}"),
                files_changed: None,
                insertions: None,
                deletions: None,
            });
            continue;
        }
    }

    // If nothing was extracted, fall back to freeform
    if records.is_empty() {
        return crate::freeform_parse(output, Some(OutputType::Git), None);
    }

    // Determine severity
    let severity = if has_conflicts {
        Severity::Warning
    } else {
        Severity::Info
    };

    // Build summary one-liner
    let one_line = build_one_line(&records, branch_name.as_deref());
    let token_estimate = one_line.split_whitespace().count() + records.len() * 4 + raw_line_count;

    ParsedOutput {
        output_type: OutputType::Git,
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

fn build_one_line(records: &[OutputRecord], branch: Option<&str>) -> String {
    let n = records.len();

    // Check for diff-stat record to surface numbers
    for r in records {
        if let OutputRecord::GitEvent {
            action,
            files_changed,
            insertions,
            deletions,
            ..
        } = r
        {
            if action == "diff-stat" {
                let mut parts = Vec::new();
                if let Some(f) = files_changed {
                    parts.push(format!("{f} files changed"));
                }
                if let Some(i) = insertions {
                    parts.push(format!("+{i}"));
                }
                if let Some(d) = deletions {
                    parts.push(format!("-{d}"));
                }
                return parts.join(", ");
            }
        }
    }

    // Check for conflicts
    let conflict_count = records
        .iter()
        .filter(|r| matches!(r, OutputRecord::GitEvent { action, .. } if action == "conflict"))
        .count();
    if conflict_count > 0 {
        return format!("{conflict_count} merge conflicts");
    }

    if let Some(b) = branch {
        format!("{n} records from git output (branch: {b})")
    } else {
        format!("{n} records from git output")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIT_STATUS_CLEAN: &str = "On branch main\nYour branch is up to date with 'origin/main'.\n\nnothing to commit, working tree clean\n";

    const GIT_STATUS_MODIFIED: &str = "On branch main\nChanges not staged for commit:\n  (use \"git add <file>...\" to update what will be committed)\n\n\tmodified:   src/main.rs\n\tmodified:   Cargo.toml\n\nUntracked files:\n  (use \"git add <file>...\" to include in what will be committed)\n\n\tnew_file.txt\n\nno changes added to commit\n";

    const GIT_DIFF_STAT: &str =
        "2 files changed, 42 insertions(+), 3 deletions(-)\n src/main.rs | 40 ++++++++++++++++++++++++++++++++++++--\n Cargo.toml  |  5 +++--\n";

    const GIT_LOG_ONELINE: &str =
        "a1b2c3d feat: add feature\nb2c3d4e fix: resolve bug\nc3d4e5f chore: update deps\n";

    const GIT_CONFLICT: &str = "Auto-merging src/main.rs\nCONFLICT (content): Merge conflict in src/main.rs\nAutomatic merge failed; fix conflicts and then commit the result.\n";

    #[test]
    fn git_status_clean_produces_git_events() {
        let parsed = parse(GIT_STATUS_CLEAN);
        assert_eq!(parsed.output_type, OutputType::Git);

        let actions: Vec<_> = parsed
            .records
            .iter()
            .filter_map(|r| {
                if let OutputRecord::GitEvent { action, .. } = r {
                    Some(action.as_str())
                } else {
                    None
                }
            })
            .collect();

        assert!(actions.contains(&"status"), "should have status action");
    }

    #[test]
    fn git_status_clean_detail_is_clean() {
        let parsed = parse(GIT_STATUS_CLEAN);
        let clean_record = parsed.records.iter().find(|r| {
            if let OutputRecord::GitEvent { action, detail, .. } = r {
                action == "status" && detail == "clean"
            } else {
                false
            }
        });
        assert!(clean_record.is_some(), "should have clean status record");
    }

    #[test]
    fn git_status_branch_detected() {
        let parsed = parse(GIT_STATUS_CLEAN);
        let branch_record = parsed.records.iter().find(|r| {
            if let OutputRecord::GitEvent { action, detail, .. } = r {
                action == "status" && detail.contains("on branch main")
            } else {
                false
            }
        });
        assert!(branch_record.is_some(), "should detect branch name");
    }

    #[test]
    fn git_status_modified_files_detected() {
        let parsed = parse(GIT_STATUS_MODIFIED);
        let file_records: Vec<_> = parsed
            .records
            .iter()
            .filter(|r| {
                if let OutputRecord::GitEvent { action, detail, .. } = r {
                    action == "status" && detail.contains("modified")
                } else {
                    false
                }
            })
            .collect();
        assert!(
            !file_records.is_empty(),
            "should detect modified file records"
        );
    }

    #[test]
    fn git_diff_stat_produces_event_with_counts() {
        let parsed = parse(GIT_DIFF_STAT);
        let stat_record = parsed.records.iter().find(|r| {
            if let OutputRecord::GitEvent { action, .. } = r {
                action == "diff-stat"
            } else {
                false
            }
        });
        assert!(stat_record.is_some(), "should have diff-stat record");

        if let Some(OutputRecord::GitEvent {
            action,
            files_changed,
            insertions,
            deletions,
            ..
        }) = stat_record
        {
            assert_eq!(action, "diff-stat");
            assert_eq!(*files_changed, Some(2));
            assert_eq!(*insertions, Some(42));
            assert_eq!(*deletions, Some(3));
        }
    }

    #[test]
    fn git_log_oneline_produces_log_events() {
        let parsed = parse(GIT_LOG_ONELINE);
        assert_eq!(parsed.output_type, OutputType::Git);

        let log_records: Vec<_> = parsed
            .records
            .iter()
            .filter(|r| matches!(r, OutputRecord::GitEvent { action, .. } if action == "log"))
            .collect();

        assert_eq!(log_records.len(), 3, "should have 3 log records");
    }

    #[test]
    fn git_log_entry_detail_contains_hash_and_message() {
        let parsed = parse(GIT_LOG_ONELINE);
        let first_log = parsed
            .records
            .iter()
            .find(|r| matches!(r, OutputRecord::GitEvent { action, .. } if action == "log"));
        if let Some(OutputRecord::GitEvent { detail, .. }) = first_log {
            assert!(detail.contains("a1b2c3d"), "detail should contain hash");
            assert!(detail.contains("feat:"), "detail should contain message");
        } else {
            panic!("expected a log record");
        }
    }

    #[test]
    fn git_conflict_produces_conflict_events() {
        let parsed = parse(GIT_CONFLICT);
        assert_eq!(parsed.output_type, OutputType::Git);

        let conflict_records: Vec<_> = parsed
            .records
            .iter()
            .filter(|r| matches!(r, OutputRecord::GitEvent { action, .. } if action == "conflict"))
            .collect();

        assert!(!conflict_records.is_empty(), "should have conflict records");
    }

    #[test]
    fn git_conflict_severity_is_warning() {
        let parsed = parse(GIT_CONFLICT);
        assert_eq!(
            parsed.summary.severity,
            Severity::Warning,
            "conflicts should produce Warning severity"
        );
    }

    #[test]
    fn git_unrecognized_output_falls_through_to_freeform() {
        let parsed = parse("some random text that is not git output\n");
        assert_eq!(parsed.output_type, OutputType::Git);
        assert!(
            matches!(parsed.records[0], OutputRecord::FreeformChunk { .. }),
            "unrecognized output should produce freeform chunk"
        );
    }

    #[test]
    fn git_diff_stat_one_line_contains_numbers() {
        let parsed = parse(GIT_DIFF_STAT);
        assert!(
            parsed.summary.one_line.contains("2"),
            "one_line should mention file count: {}",
            parsed.summary.one_line
        );
        assert!(
            parsed.summary.one_line.contains("42"),
            "one_line should mention insertions: {}",
            parsed.summary.one_line
        );
    }
}

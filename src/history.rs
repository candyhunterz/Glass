//! History subcommand dispatch and display formatting.
//!
//! Handles `glass history`, `glass history list`, and `glass history search`.

use chrono::{DateTime, Utc};
use glass_history::db::CommandRecord;
use glass_history::query::{parse_time, QueryFilter};

use crate::{HistoryAction, HistoryFilters};

/// Entry point for all history subcommands.
///
/// Resolves the database, builds a QueryFilter from CLI args, executes the query,
/// and prints formatted results to stdout.
pub fn run_history(action: Option<HistoryAction>) {
    let db_path = glass_history::resolve_db_path(&std::env::current_dir().unwrap_or_default());
    let db = match glass_history::HistoryDb::open(&db_path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!(
                "Error: could not open history database at {}: {}",
                db_path.display(),
                e
            );
            std::process::exit(1);
        }
    };

    let filter = match &action {
        None | Some(HistoryAction::List { .. }) => {
            let filters = match &action {
                Some(HistoryAction::List { filters }) => filters,
                _ => &HistoryFilters::default(),
            };
            build_query_filter(None, filters)
        }
        Some(HistoryAction::Search { query, filters }) => build_query_filter(Some(query), filters),
    };

    let filter = match filter {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    match db.filtered_query(&filter) {
        Ok(records) => print_results(&records),
        Err(e) => {
            eprintln!("Error querying history: {}", e);
            std::process::exit(1);
        }
    }

    std::process::exit(0);
}

/// Build a QueryFilter from CLI arguments.
fn build_query_filter(
    text: Option<&String>,
    filters: &HistoryFilters,
) -> Result<QueryFilter, String> {
    let after = match &filters.after {
        Some(s) => {
            Some(parse_time(s).map_err(|e| format!("Invalid --after value '{}': {}", s, e))?)
        }
        None => None,
    };

    let before = match &filters.before {
        Some(s) => {
            Some(parse_time(s).map_err(|e| format!("Invalid --before value '{}': {}", s, e))?)
        }
        None => None,
    };

    Ok(QueryFilter {
        text: text.cloned(),
        exit_code: filters.exit,
        after,
        before,
        cwd: filters.cwd.clone(),
        limit: filters.limit,
    })
}

/// Format an epoch timestamp for display.
///
/// If within the last 24 hours, shows relative time (e.g. "2h ago", "45m ago").
/// Otherwise shows "YYYY-MM-DD HH:MM:SS".
fn format_timestamp(epoch: i64) -> String {
    let now = Utc::now().timestamp();
    let diff = now - epoch;

    if (0..86400).contains(&diff) {
        if diff < 60 {
            return format!("{}s ago", diff);
        } else if diff < 3600 {
            return format!("{}m ago", diff / 60);
        } else {
            return format!("{}h ago", diff / 3600);
        }
    }

    match DateTime::from_timestamp(epoch, 0) {
        Some(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        None => format!("{}", epoch),
    }
}

/// Format a duration in milliseconds for display.
///
/// < 1000ms: "Nms", < 60000ms: "N.Ns", else: "NmNs"
fn format_duration(ms: i64) -> String {
    if ms < 1000 {
        format!("{}ms", ms)
    } else if ms < 60000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let secs = ms / 1000;
        let m = secs / 60;
        let s = secs % 60;
        format!("{}m{}s", m, s)
    }
}

/// Truncate a string to max length, appending "..." if truncated.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 3 {
        ".".repeat(max)
    } else {
        format!("{}...", &s[..max - 3])
    }
}

/// Print query results as a formatted table.
fn print_results(records: &[CommandRecord]) {
    if records.is_empty() {
        println!("No matching commands found.");
        return;
    }

    // Header
    println!(
        "{:<50} {:>4} {:>8} {:>16} CWD",
        "COMMAND", "EXIT", "DURATION", "TIME"
    );
    println!("{}", "-".repeat(100));

    for record in records {
        let cmd = if record.command.is_empty() {
            "<empty>".to_string()
        } else {
            truncate(&record.command, 50)
        };

        let exit = match record.exit_code {
            Some(code) => format!("{}", code),
            None => "-".to_string(),
        };

        let duration = format_duration(record.duration_ms);
        let time = format_timestamp(record.started_at);
        let cwd = truncate(&record.cwd, 40);

        println!(
            "{:<50} {:>4} {:>8} {:>16} {}",
            cmd, exit, duration, time, cwd
        );
    }

    println!("\n{} result(s)", records.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_ms() {
        assert_eq!(format_duration(50), "50ms");
        assert_eq!(format_duration(999), "999ms");
    }

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(1500), "1.5s");
        assert_eq!(format_duration(30000), "30.0s");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(90000), "1m30s");
        assert_eq!(format_duration(120000), "2m0s");
    }

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long() {
        assert_eq!(truncate("hello world this is long", 10), "hello w...");
    }

    #[test]
    fn test_truncate_exact() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn test_format_timestamp_old() {
        // A timestamp from 2023 should show full date
        let ts = 1700000000; // 2023-11-14
        let result = format_timestamp(ts);
        assert!(
            result.contains("2023"),
            "Expected date format, got: {}",
            result
        );
    }
}

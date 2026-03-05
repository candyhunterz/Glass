//! Search overlay state management.
//!
//! Provides `SearchOverlay` for managing the in-terminal search overlay state,
//! including query accumulation, result selection, and debounced search execution.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use glass_history::db::CommandRecord;

/// Display-ready snapshot of the search overlay state.
#[derive(Debug, Clone)]
pub struct SearchOverlayData {
    /// Current query text.
    pub query: String,
    /// Formatted search results ready for rendering.
    pub results: Vec<SearchResultDisplay>,
    /// Index of the currently selected result.
    pub selected: usize,
}

/// A single search result formatted for display.
#[derive(Debug, Clone)]
pub struct SearchResultDisplay {
    /// Command text, truncated to 80 characters.
    pub command: String,
    /// Process exit code if available.
    pub exit_code: Option<i32>,
    /// Relative timestamp (e.g. "2h ago") or full datetime for older entries.
    pub timestamp: String,
    /// First 80 chars of output with newlines replaced by spaces.
    pub output_preview: String,
}

/// In-terminal search overlay state.
pub struct SearchOverlay {
    /// Current search query text.
    pub query: String,
    /// Cursor position in the query (currently always at end).
    pub cursor_pos: usize,
    /// Raw search results from the database.
    pub results: Vec<CommandRecord>,
    /// Index of the currently selected result.
    pub selected: usize,
    /// Timestamp of the last keystroke that modified the query.
    pub last_keystroke: Instant,
    /// Whether a search needs to be executed (query changed since last search).
    pub search_pending: bool,
}

impl SearchOverlay {
    /// Create a new empty search overlay.
    pub fn new() -> Self {
        Self {
            query: String::new(),
            cursor_pos: 0,
            results: Vec::new(),
            selected: 0,
            last_keystroke: Instant::now(),
            search_pending: false,
        }
    }

    /// Append character(s) to the query.
    pub fn push_char(&mut self, c: &str) {
        self.query.push_str(c);
        self.cursor_pos = self.query.len();
        self.search_pending = true;
        self.last_keystroke = Instant::now();
    }

    /// Remove the last character from the query. No-op if empty.
    pub fn pop_char(&mut self) {
        if self.query.pop().is_some() {
            self.cursor_pos = self.query.len();
            self.search_pending = true;
            self.last_keystroke = Instant::now();
        }
    }

    /// Move selection up (saturating at 0).
    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Move selection down (clamped to results.len() - 1). No-op when results empty.
    pub fn move_down(&mut self) {
        if !self.results.is_empty() && self.selected + 1 < self.results.len() {
            self.selected += 1;
        }
    }

    /// Replace results and reset selection to 0.
    pub fn set_results(&mut self, results: Vec<CommandRecord>) {
        self.results = results;
        self.selected = 0;
    }

    /// Check if a search should be executed (debounce elapsed and pending).
    pub fn should_search(&self, debounce: Duration) -> bool {
        self.search_pending && self.last_keystroke.elapsed() >= debounce
    }

    /// Mark the search as executed (clears pending flag).
    pub fn mark_searched(&mut self) {
        self.search_pending = false;
    }

    /// Extract display-ready data from the current state.
    pub fn extract_display_data(&self) -> SearchOverlayData {
        let now = Utc::now();
        let results = self
            .results
            .iter()
            .map(|record| {
                let command = truncate_str(&record.command, 80);
                let timestamp = format_relative_time(record.started_at, now);
                let output_preview = record
                    .output
                    .as_deref()
                    .map(|o| {
                        let cleaned = o.replace('\n', " ").replace('\r', " ");
                        truncate_str(&cleaned, 80)
                    })
                    .unwrap_or_default();

                SearchResultDisplay {
                    command,
                    exit_code: record.exit_code,
                    timestamp,
                    output_preview,
                }
            })
            .collect();

        SearchOverlayData {
            query: self.query.clone(),
            results,
            selected: self.selected,
        }
    }
}

/// Truncate a string to `max_chars` characters, appending "..." if truncated.
fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}

/// Format a Unix epoch timestamp as a relative time string.
/// Uses "Xm ago", "Xh ago", "Xd ago" for recent, full datetime for older.
fn format_relative_time(epoch_secs: i64, now: DateTime<Utc>) -> String {
    let then = DateTime::from_timestamp(epoch_secs, 0);
    let Some(then) = then else {
        return "unknown".to_string();
    };

    let diff = now.signed_duration_since(then);
    let secs = diff.num_seconds();

    if secs < 0 {
        return "just now".to_string();
    }
    if secs < 60 {
        return "just now".to_string();
    }
    if secs < 3600 {
        return format!("{}m ago", secs / 60);
    }
    if secs < 86400 {
        return format!("{}h ago", secs / 3600);
    }
    if secs < 7 * 86400 {
        return format!("{}d ago", secs / 86400);
    }

    then.format("%Y-%m-%d %H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record(command: &str, started_at: i64) -> CommandRecord {
        CommandRecord {
            id: Some(1),
            command: command.to_string(),
            cwd: "/home/user".to_string(),
            exit_code: Some(0),
            started_at,
            finished_at: started_at + 5,
            duration_ms: 5000,
            output: None,
        }
    }

    #[test]
    fn test_new_creates_empty_state() {
        let overlay = SearchOverlay::new();
        assert_eq!(overlay.query, "");
        assert!(overlay.results.is_empty());
        assert_eq!(overlay.selected, 0);
        assert!(!overlay.search_pending);
    }

    #[test]
    fn test_push_char_accumulates_query() {
        let mut overlay = SearchOverlay::new();
        overlay.push_char("a");
        assert_eq!(overlay.query, "a");
        assert!(overlay.search_pending);

        overlay.push_char("b");
        assert_eq!(overlay.query, "ab");

        overlay.push_char("cd");
        assert_eq!(overlay.query, "abcd");
    }

    #[test]
    fn test_push_char_updates_cursor() {
        let mut overlay = SearchOverlay::new();
        overlay.push_char("abc");
        assert_eq!(overlay.cursor_pos, 3);
    }

    #[test]
    fn test_pop_char_removes_last() {
        let mut overlay = SearchOverlay::new();
        overlay.push_char("abc");
        overlay.mark_searched(); // clear pending
        overlay.pop_char();
        assert_eq!(overlay.query, "ab");
        assert!(overlay.search_pending);
        assert_eq!(overlay.cursor_pos, 2);
    }

    #[test]
    fn test_pop_char_noop_on_empty() {
        let mut overlay = SearchOverlay::new();
        overlay.pop_char();
        assert_eq!(overlay.query, "");
        assert!(!overlay.search_pending); // should NOT set pending
    }

    #[test]
    fn test_move_up_saturates_at_zero() {
        let mut overlay = SearchOverlay::new();
        overlay.selected = 2;
        overlay.move_up();
        assert_eq!(overlay.selected, 1);
        overlay.move_up();
        assert_eq!(overlay.selected, 0);
        overlay.move_up();
        assert_eq!(overlay.selected, 0); // saturates
    }

    #[test]
    fn test_move_down_clamps_to_max() {
        let mut overlay = SearchOverlay::new();
        overlay.results = vec![
            sample_record("cmd1", 1000),
            sample_record("cmd2", 2000),
            sample_record("cmd3", 3000),
        ];
        overlay.move_down();
        assert_eq!(overlay.selected, 1);
        overlay.move_down();
        assert_eq!(overlay.selected, 2);
        overlay.move_down();
        assert_eq!(overlay.selected, 2); // clamped at len-1
    }

    #[test]
    fn test_move_down_noop_when_empty() {
        let mut overlay = SearchOverlay::new();
        overlay.move_down();
        assert_eq!(overlay.selected, 0);
    }

    #[test]
    fn test_set_results_resets_selected() {
        let mut overlay = SearchOverlay::new();
        overlay.selected = 5;
        overlay.set_results(vec![
            sample_record("cmd1", 1000),
            sample_record("cmd2", 2000),
        ]);
        assert_eq!(overlay.results.len(), 2);
        assert_eq!(overlay.selected, 0);
    }

    #[test]
    fn test_should_search_debounce() {
        let mut overlay = SearchOverlay::new();
        overlay.push_char("a");

        // Immediately after push, debounce not elapsed (using long debounce)
        assert!(!overlay.should_search(Duration::from_secs(10)));

        // With zero debounce, should be true
        assert!(overlay.should_search(Duration::from_millis(0)));
    }

    #[test]
    fn test_should_search_requires_pending() {
        let mut overlay = SearchOverlay::new();
        overlay.push_char("a");
        overlay.mark_searched();
        // Not pending anymore
        assert!(!overlay.should_search(Duration::from_millis(0)));
    }

    #[test]
    fn test_should_search_with_real_debounce() {
        let mut overlay = SearchOverlay::new();
        overlay.push_char("a");

        // Should not pass with 150ms debounce immediately
        assert!(!overlay.should_search(Duration::from_millis(150)));

        // Wait for debounce to elapse
        std::thread::sleep(Duration::from_millis(200));
        assert!(overlay.should_search(Duration::from_millis(150)));
    }

    #[test]
    fn test_mark_searched() {
        let mut overlay = SearchOverlay::new();
        overlay.push_char("a");
        assert!(overlay.search_pending);
        overlay.mark_searched();
        assert!(!overlay.search_pending);
    }

    #[test]
    fn test_extract_display_data_basic() {
        let now_epoch = Utc::now().timestamp();
        let mut overlay = SearchOverlay::new();
        overlay.push_char("test");
        overlay.set_results(vec![sample_record("cargo build", now_epoch - 120)]); // 2 min ago

        let data = overlay.extract_display_data();
        assert_eq!(data.query, "test");
        assert_eq!(data.results.len(), 1);
        assert_eq!(data.results[0].command, "cargo build");
        assert_eq!(data.results[0].exit_code, Some(0));
        assert_eq!(data.results[0].timestamp, "2m ago");
        assert_eq!(data.selected, 0);
    }

    #[test]
    fn test_extract_display_data_truncates_command() {
        let mut overlay = SearchOverlay::new();
        let long_command = "x".repeat(100);
        overlay.set_results(vec![sample_record(&long_command, 1700000000)]);

        let data = overlay.extract_display_data();
        assert!(data.results[0].command.len() <= 80);
        assert!(data.results[0].command.ends_with("..."));
    }

    #[test]
    fn test_extract_display_data_output_preview() {
        let mut overlay = SearchOverlay::new();
        let mut record = sample_record("echo hello", 1700000000);
        record.output = Some("line1\nline2\nline3".to_string());
        overlay.set_results(vec![record]);

        let data = overlay.extract_display_data();
        assert_eq!(data.results[0].output_preview, "line1 line2 line3");
    }

    #[test]
    fn test_extract_display_data_no_output() {
        let mut overlay = SearchOverlay::new();
        overlay.set_results(vec![sample_record("ls", 1700000000)]);

        let data = overlay.extract_display_data();
        assert_eq!(data.results[0].output_preview, "");
    }

    #[test]
    fn test_format_relative_time_minutes() {
        let now = Utc::now();
        let epoch = now.timestamp() - 300; // 5 minutes ago
        assert_eq!(format_relative_time(epoch, now), "5m ago");
    }

    #[test]
    fn test_format_relative_time_hours() {
        let now = Utc::now();
        let epoch = now.timestamp() - 7200; // 2 hours ago
        assert_eq!(format_relative_time(epoch, now), "2h ago");
    }

    #[test]
    fn test_format_relative_time_days() {
        let now = Utc::now();
        let epoch = now.timestamp() - 172800; // 2 days ago
        assert_eq!(format_relative_time(epoch, now), "2d ago");
    }

    #[test]
    fn test_format_relative_time_old() {
        let now = Utc::now();
        let epoch = now.timestamp() - 30 * 86400; // 30 days ago
        let result = format_relative_time(epoch, now);
        // Should be a full datetime
        assert!(result.contains("-"), "Expected full datetime, got: {}", result);
        assert!(!result.contains("ago"));
    }

    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("hello", 80), "hello");
    }

    #[test]
    fn test_truncate_str_exact() {
        let s = "x".repeat(80);
        assert_eq!(truncate_str(&s, 80), s);
    }

    #[test]
    fn test_truncate_str_long() {
        let s = "x".repeat(100);
        let result = truncate_str(&s, 80);
        assert!(result.len() <= 80);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_selected_preserved_in_display_data() {
        let mut overlay = SearchOverlay::new();
        overlay.set_results(vec![
            sample_record("cmd1", 1700000000),
            sample_record("cmd2", 1700000010),
        ]);
        overlay.move_down();
        let data = overlay.extract_display_data();
        assert_eq!(data.selected, 1);
    }
}

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

impl Default for SearchOverlay {
    fn default() -> Self {
        Self::new()
    }
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
                        let cleaned = o.replace(['\n', '\r'], " ");
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
pub fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}

/// Format a Unix epoch timestamp as a relative time string.
/// Uses "Xm ago", "Xh ago", "Xd ago" for recent, full datetime for older.
pub fn format_relative_time(epoch_secs: i64, now: DateTime<Utc>) -> String {
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

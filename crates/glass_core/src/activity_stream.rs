use std::collections::VecDeque;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// A single agent-observable activity event emitted by the SOI pipeline.
#[derive(Debug, Clone)]
pub struct ActivityEvent {
    /// History DB row id of the command that produced this event.
    pub command_id: i64,
    /// Terminal session that ran the command.
    pub session_id: crate::event::SessionId,
    /// One-line human/agent readable summary.
    pub summary: String,
    /// Outcome severity: "Error" | "Warning" | "Info" | "Success"
    pub severity: String,
    /// Unix timestamp (seconds) of when the event was emitted.
    pub timestamp_secs: u64,
    /// Estimated LLM token cost for this event (summary length / 4 + 8 overhead).
    pub token_cost: usize,
    /// How many consecutive identical events this entry represents (>=1).
    /// When > 1 the entry was collapsed from multiple duplicates.
    pub collapsed_count: u32,
}

/// Configuration for the activity stream channel and filter.
#[derive(Debug, Clone)]
pub struct ActivityStreamConfig {
    /// Capacity of the bounded sync_channel. Default: 256.
    pub channel_capacity: usize,
    /// Rolling token budget for the in-memory window. Default: 4096.
    pub token_budget: usize,
    /// Maximum events per second (leaky-bucket rate limiter). Default: 5.0.
    pub max_rate_per_sec: f64,
}

impl Default for ActivityStreamConfig {
    fn default() -> Self {
        Self {
            channel_capacity: 256,
            token_budget: 4096,
            max_rate_per_sec: 5.0,
        }
    }
}

/// Estimate the LLM token cost of a summary string.
///
/// Formula: `(summary.len() / 4) + 8` (4 chars ≈ 1 token, 8 tokens fixed overhead).
pub fn estimate_tokens(summary: &str) -> usize {
    (summary.len() / 4) + 8
}

/// Stateful filter that applies noise-reduction, rate-limiting, and a rolling
/// token-budget window to the raw SOI event stream.
///
/// # Noise reduction
/// Consecutive events with the same severity *and* summary that have severity
/// "Success" or "Info" are collapsed. The collapsed count is recorded on the
/// last emitted event when the fingerprint changes.
///
/// # Rate limiting
/// Implements a token-bucket algorithm. The bucket starts full at
/// `max_rate_per_sec` tokens and is refilled proportionally to elapsed time,
/// capped at `max_rate_per_sec`. Each accepted event consumes one token; events
/// arriving when the bucket is empty are silently dropped.
///
/// # Rolling budget window
/// Accepted events are appended to an in-memory `VecDeque`. When the cumulative
/// token cost exceeds `token_budget`, the oldest events are evicted until the
/// budget is satisfied again.
pub struct ActivityFilter {
    config: ActivityStreamConfig,
    window: VecDeque<ActivityEvent>,
    window_token_cost: usize,
    rate_tokens: f64,
    last_refill: Instant,
    last_fingerprint: Option<String>,
    pending_collapsed: u32,
}

impl ActivityFilter {
    /// Create a new filter. The rate-limiter bucket starts full.
    pub fn new(config: ActivityStreamConfig) -> Self {
        let rate_tokens = config.max_rate_per_sec;
        Self {
            config,
            window: VecDeque::new(),
            window_token_cost: 0,
            rate_tokens,
            last_refill: Instant::now(),
            last_fingerprint: None,
            pending_collapsed: 0,
        }
    }

    /// Process a raw SOI event through noise filter, rate limiter, and budget
    /// window. Returns `Some(ActivityEvent)` if the event should be forwarded,
    /// or `None` if it was dropped (collapsed or rate-limited).
    ///
    /// # Arguments
    /// * `command_id` – history DB row id
    /// * `session_id` – originating session
    /// * `summary`    – one-line outcome description
    /// * `severity`   – "Error" | "Warning" | "Info" | "Success"
    pub fn process(
        &mut self,
        command_id: i64,
        session_id: crate::event::SessionId,
        summary: String,
        severity: String,
    ) -> Option<ActivityEvent> {
        // a. Refill rate tokens based on elapsed time
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;
        self.rate_tokens = (self.rate_tokens + elapsed * self.config.max_rate_per_sec)
            .min(self.config.max_rate_per_sec);

        // b. Check rate limit
        if self.rate_tokens < 1.0 {
            return None;
        }

        // c. Compute fingerprint
        let fingerprint = format!("{}:{}", severity, summary);

        // d. Collapse identical Success/Info events
        let is_collapsible = severity == "Success" || severity == "Info";
        if is_collapsible {
            if let Some(ref last) = self.last_fingerprint {
                if *last == fingerprint {
                    self.pending_collapsed += 1;
                    return None;
                }
            }
        }

        // e. If arriving non-matching event and we have pending collapsed,
        //    retroactively update the collapsed_count on the last window event.
        if self.pending_collapsed > 0 {
            if let Some(last_event) = self.window.back_mut() {
                last_event.collapsed_count = 1 + self.pending_collapsed;
            }
            self.pending_collapsed = 0;
        }

        // f. Consume rate token
        self.rate_tokens = (self.rate_tokens - 1.0).max(0.0);

        // g. Build the event
        let token_cost = estimate_tokens(&summary);
        let timestamp_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let event = ActivityEvent {
            command_id,
            session_id,
            summary,
            severity,
            timestamp_secs,
            token_cost,
            collapsed_count: 1,
        };

        // h. Push to budget window; evict oldest until within budget
        self.window_token_cost += token_cost;
        self.window.push_back(event.clone());
        while self.window_token_cost > self.config.token_budget {
            if let Some(evicted) = self.window.pop_front() {
                self.window_token_cost = self.window_token_cost.saturating_sub(evicted.token_cost);
            } else {
                break;
            }
        }

        // i. Store fingerprint
        self.last_fingerprint = Some(fingerprint);

        // j. Return the event
        Some(event)
    }

    /// If there are pending collapsed events (i.e., the last run of identical
    /// Success/Info events has not yet been "closed" by a different fingerprint),
    /// retroactively update the last window event's `collapsed_count` and return
    /// a clone.
    ///
    /// Used by the Phase 56 agent runtime when draining the stream at shutdown.
    pub fn flush_collapsed(&mut self) -> Option<ActivityEvent> {
        if self.pending_collapsed > 0 {
            if let Some(last_event) = self.window.back_mut() {
                last_event.collapsed_count = 1 + self.pending_collapsed;
                self.pending_collapsed = 0;
                return Some(last_event.clone());
            }
        }
        None
    }

    /// Iterate over events currently held in the rolling budget window.
    pub fn window_events(&self) -> impl Iterator<Item = &ActivityEvent> {
        self.window.iter()
    }
}

/// Create a bounded `sync_channel` for `ActivityEvent`s.
///
/// The capacity is taken from `config.channel_capacity`. Callers **must** use
/// [`std::sync::mpsc::SyncSender::try_send`] — blocking `send()` is prohibited
/// on the main/render thread.
pub fn create_channel(
    config: &ActivityStreamConfig,
) -> (
    std::sync::mpsc::SyncSender<ActivityEvent>,
    std::sync::mpsc::Receiver<ActivityEvent>,
) {
    std::sync::mpsc::sync_channel(config.channel_capacity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::SessionId;

    fn sid() -> SessionId {
        SessionId::new(1)
    }

    // Helper: build a filter with default config
    fn default_filter() -> ActivityFilter {
        ActivityFilter::new(ActivityStreamConfig::default())
    }

    /// Channel send-and-receive round-trip.
    #[test]
    fn test_channel_receives_event() {
        let config = ActivityStreamConfig::default();
        let (tx, rx) = create_channel(&config);
        let mut filter = ActivityFilter::new(config);

        if let Some(event) = filter.process(
            1,
            sid(),
            "build succeeded".to_string(),
            "Success".to_string(),
        ) {
            tx.try_send(event).expect("channel should have capacity");
        } else {
            panic!("expected process() to return Some");
        }

        let received = rx.try_recv().expect("event should be in channel");
        assert_eq!(received.command_id, 1);
        assert_eq!(received.severity, "Success");
    }

    /// Rolling budget window evicts oldest events when token cost exceeds budget.
    #[test]
    fn test_budget_window_evicts_oldest() {
        // Use a tiny budget so we can force eviction with a few events.
        let config = ActivityStreamConfig {
            token_budget: 50,
            ..Default::default()
        };
        let mut filter = ActivityFilter::new(config);

        // Each event with a 32-char summary costs (32/4)+8 = 16 tokens.
        let long_summary = "a".repeat(32); // 16 tokens each

        // Push 4 events (4 * 16 = 64 > 50). After each push the window trims.
        for i in 0..4_i64 {
            // Use different summaries to avoid collapse
            let summary = format!("{}{}", long_summary, i);
            filter.process(i, sid(), summary, "Error".to_string());
        }

        // Window should have been trimmed. Total cost in window <= 50.
        let total: usize = filter.window_events().map(|e| e.token_cost).sum();
        assert!(
            total <= 50,
            "window token cost {} should be <= budget 50",
            total
        );
        // And we should have fewer than 4 events.
        let count = filter.window_events().count();
        assert!(
            count < 4,
            "oldest events should have been evicted, got {}",
            count
        );
    }

    /// 20 consecutive identical Success events collapse into 1 entry.
    #[test]
    fn test_noise_filter_collapses_identical() {
        let mut filter = default_filter();
        let summary = "tests passed".to_string();

        // Send 20 identical Success events.
        let mut emitted = 0usize;
        for _ in 0..20 {
            if filter
                .process(1, sid(), summary.clone(), "Success".to_string())
                .is_some()
            {
                emitted += 1;
            }
        }
        // Only the first one should have been emitted; the rest (19) are pending.
        assert_eq!(emitted, 1, "only 1 of 20 identical events should emit");

        // Send a different event to flush the collapse.
        let flush_result = filter.process(2, sid(), "different".to_string(), "Error".to_string());
        assert!(flush_result.is_some(), "different event must pass through");

        // Now check the window: it should have 2 entries.
        let window: Vec<&ActivityEvent> = filter.window_events().collect();
        assert_eq!(
            window.len(),
            2,
            "window should have 2 entries after collapse flush"
        );

        // The first entry (collapsed) should have collapsed_count >= 20.
        assert!(
            window[0].collapsed_count >= 20,
            "collapsed event should have count >= 20, got {}",
            window[0].collapsed_count
        );
    }

    /// Rate limiter drops events exceeding burst of 5/sec.
    #[test]
    fn test_rate_limiter_burst() {
        let config = ActivityStreamConfig {
            max_rate_per_sec: 5.0,
            ..Default::default()
        };
        let mut filter = ActivityFilter::new(config);

        // Fire 10 events as fast as possible (no sleep → negligible elapsed time).
        // The bucket starts full at 5.0. Each call refills ~0 tokens (no time passes).
        // So only 5 should pass.
        let mut passed = 0usize;
        for i in 0..10_i64 {
            let summary = format!("cmd {}", i); // unique so no collapse
            if filter
                .process(i, sid(), summary, "Error".to_string())
                .is_some()
            {
                passed += 1;
            }
        }
        // With a bucket starting at 5.0, exactly 5 should pass (bucket empties after 5).
        assert_eq!(
            passed, 5,
            "rate limiter should pass exactly 5 out of 10, got {}",
            passed
        );
    }

    /// Error events are never collapsed even when identical.
    #[test]
    fn test_error_events_not_collapsed() {
        let mut filter = default_filter();
        let summary = "build failed".to_string();

        let mut passed = 0usize;
        for _ in 0..5 {
            if filter
                .process(1, sid(), summary.clone(), "Error".to_string())
                .is_some()
            {
                passed += 1;
            }
        }
        assert_eq!(
            passed, 5,
            "Error events should never be collapsed, got {}",
            passed
        );
    }

    /// estimate_tokens("") must return the fixed overhead (8), not 0.
    #[test]
    fn test_empty_summary_token_cost() {
        assert_eq!(estimate_tokens(""), 8);
    }

    /// window_events() returns all events within budget.
    #[test]
    fn test_window_events_returns_current_window() {
        let mut filter = default_filter();

        filter.process(1, sid(), "first".to_string(), "Info".to_string());
        // second event has different summary so no collapse
        filter.process(2, sid(), "second".to_string(), "Info".to_string());
        filter.process(3, sid(), "third".to_string(), "Error".to_string());

        let events: Vec<&ActivityEvent> = filter.window_events().collect();
        assert_eq!(events.len(), 3, "window should contain 3 events");
    }

    /// Channel created via create_channel is bounded: try_send fails when full.
    #[test]
    fn test_create_channel_bounded() {
        let config = ActivityStreamConfig {
            channel_capacity: 2,
            ..Default::default()
        };
        let (tx, _rx) = create_channel(&config);

        let make_event = |i: i64| ActivityEvent {
            command_id: i,
            session_id: sid(),
            summary: format!("event {}", i),
            severity: "Info".to_string(),
            timestamp_secs: 0,
            token_cost: 8,
            collapsed_count: 1,
        };

        // Fill to capacity
        tx.try_send(make_event(1)).expect("send 1 should succeed");
        tx.try_send(make_event(2)).expect("send 2 should succeed");

        // Next try_send must fail (channel full)
        let result = tx.try_send(make_event(3));
        assert!(
            result.is_err(),
            "try_send on full channel should return Err"
        );
    }
}

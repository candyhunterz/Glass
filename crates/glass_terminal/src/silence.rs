use std::time::{Duration, Instant};

/// Tracks PTY silence for orchestrator polling.
/// Fires periodically (every `threshold`) while PTY is quiet,
/// instead of the old one-shot approach.
pub struct SilenceTracker {
    threshold: Duration,
    last_output_at: Instant,
    last_fired_at: Option<Instant>,
}

impl SilenceTracker {
    pub fn new(threshold_secs: u64) -> Self {
        Self {
            threshold: Duration::from_secs(threshold_secs),
            last_output_at: Instant::now(),
            last_fired_at: None,
        }
    }

    /// Call when PTY produces output. Resets all timers.
    pub fn on_output(&mut self) {
        self.last_output_at = Instant::now();
        self.last_fired_at = None;
    }

    /// Check if silence event should fire. Returns true at most once
    /// per `threshold` interval while the PTY remains quiet.
    pub fn should_fire(&mut self) -> bool {
        if self.last_output_at.elapsed() < self.threshold {
            return false;
        }
        let should = self
            .last_fired_at
            .map(|t| t.elapsed() >= self.threshold)
            .unwrap_or(true);
        if should {
            self.last_fired_at = Some(Instant::now());
        }
        should
    }

    /// Returns the maximum poll timeout to use, ensuring we wake up
    /// in time to check silence.
    pub fn poll_timeout(&self) -> Duration {
        let since_output = self.last_output_at.elapsed();
        if since_output >= self.threshold {
            let since_fired = self
                .last_fired_at
                .map(|t| t.elapsed())
                .unwrap_or(self.threshold);
            self.threshold
                .saturating_sub(since_fired)
                .max(Duration::from_secs(1))
        } else {
            self.threshold
                .saturating_sub(since_output)
                .max(Duration::from_secs(1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn fires_after_threshold() {
        let mut tracker = SilenceTracker::new(1);
        assert!(!tracker.should_fire());
        thread::sleep(Duration::from_millis(1100));
        assert!(tracker.should_fire());
    }

    #[test]
    fn does_not_double_fire_immediately() {
        let mut tracker = SilenceTracker::new(1);
        thread::sleep(Duration::from_millis(1100));
        assert!(tracker.should_fire());
        assert!(!tracker.should_fire());
    }

    #[test]
    fn re_fires_after_cooldown() {
        let mut tracker = SilenceTracker::new(1);
        thread::sleep(Duration::from_millis(1100));
        assert!(tracker.should_fire());
        thread::sleep(Duration::from_millis(1100));
        assert!(tracker.should_fire());
    }

    #[test]
    fn output_resets_timers() {
        let mut tracker = SilenceTracker::new(1);
        thread::sleep(Duration::from_millis(1100));
        tracker.on_output();
        assert!(!tracker.should_fire());
    }

    #[test]
    fn poll_timeout_respects_threshold() {
        let tracker = SilenceTracker::new(30);
        let timeout = tracker.poll_timeout();
        assert!(timeout <= Duration::from_secs(30));
        assert!(timeout >= Duration::from_secs(1));
    }
}

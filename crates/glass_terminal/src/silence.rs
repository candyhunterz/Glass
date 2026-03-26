use glass_core::event::TriggerSource;
use regex::Regex;
use std::time::{Duration, Instant};

/// Smart orchestrator trigger that fires on the fastest available signal.
///
/// Priority order:
/// 1. Prompt regex match (instant)
/// 2. Shell prompt returned / OSC 133;A (instant)
/// 3. Output velocity drop (fast_threshold seconds after output stops)
/// 4. Fixed silence threshold (slow fallback, periodic)
pub struct SmartTrigger {
    /// Slow fallback threshold (existing behavior).
    threshold: Duration,
    /// Fast trigger threshold (after output stops).
    fast_threshold: Duration,
    /// Last time PTY produced output.
    last_output_at: Instant,
    /// Last time should_fire() returned true (for periodic slow fallback).
    last_fired_at: Option<Instant>,
    /// Compiled prompt regex (None if not configured or invalid).
    prompt_regex: Option<Regex>,
    /// Latch: set on output, cleared when fast trigger fires.
    was_output_flowing: bool,
    /// Set when prompt regex matches end of output.
    prompt_detected: bool,
    /// Set when OSC 133;A (shell prompt) received.
    shell_prompt_returned: bool,
    /// Minimum output bytes required before fast trigger can arm.
    min_output_bytes: usize,
    /// Bytes accumulated since last fire.
    output_bytes_since_fire: usize,
}

impl SmartTrigger {
    pub fn new(
        threshold_secs: u64,
        fast_trigger_secs: u64,
        prompt_pattern: Option<String>,
    ) -> Self {
        let prompt_regex = prompt_pattern.and_then(|p| {
            Regex::new(&p)
                .map_err(|e| tracing::warn!("Invalid orchestrator prompt regex '{p}': {e}"))
                .ok()
        });
        Self {
            threshold: Duration::from_secs(threshold_secs),
            fast_threshold: Duration::from_secs(fast_trigger_secs),
            last_output_at: Instant::now(),
            last_fired_at: None,
            prompt_regex,
            was_output_flowing: false,
            prompt_detected: false,
            shell_prompt_returned: false,
            min_output_bytes: 0,
            output_bytes_since_fire: 0,
        }
    }

    pub fn set_min_output_bytes(&mut self, min: usize) {
        self.min_output_bytes = min;
    }

    /// Call when PTY produces output bytes. Resets timers, checks prompt regex.
    pub fn on_output_bytes(&mut self, bytes: &[u8]) {
        self.last_output_at = Instant::now();
        self.last_fired_at = None;
        self.output_bytes_since_fire += bytes.len();
        if self.output_bytes_since_fire >= self.min_output_bytes {
            self.was_output_flowing = true;
        }

        // Check prompt regex against the last line of output
        if let Some(ref regex) = self.prompt_regex {
            if let Ok(text) = std::str::from_utf8(bytes) {
                // Check the last non-empty line
                if let Some(last_line) = text.lines().rev().find(|l| !l.is_empty()) {
                    if regex.is_match(last_line) {
                        self.prompt_detected = true;
                    }
                }
            }
        }
    }

    /// Call when OSC 133;A (shell prompt start) is detected on the PTY thread.
    pub fn on_shell_prompt(&mut self) {
        self.shell_prompt_returned = true;
    }

    /// Check if the orchestrator should be triggered. Returns the trigger
    /// source if fired, or `None` if no trigger condition is met.
    pub fn should_fire(&mut self) -> Option<TriggerSource> {
        // Priority 1: Prompt regex matched
        if self.prompt_detected {
            self.prompt_detected = false;
            self.output_bytes_since_fire = 0;
            self.last_fired_at = Some(Instant::now());
            return Some(TriggerSource::Prompt);
        }

        // Priority 2: Shell prompt returned (agent exited)
        if self.shell_prompt_returned {
            self.shell_prompt_returned = false;
            self.output_bytes_since_fire = 0;
            self.last_fired_at = Some(Instant::now());
            return Some(TriggerSource::ShellPrompt);
        }

        let silence = self.last_output_at.elapsed();

        // Priority 3: Output was flowing and stopped for fast_threshold
        if self.was_output_flowing && silence >= self.fast_threshold {
            self.was_output_flowing = false;
            self.output_bytes_since_fire = 0;
            self.last_fired_at = Some(Instant::now());
            return Some(TriggerSource::Fast);
        }

        // Priority 4: Slow fallback (periodic fire after threshold)
        if silence >= self.threshold {
            let should = self
                .last_fired_at
                .map(|t| t.elapsed() >= self.threshold)
                .unwrap_or(true);
            if should {
                self.output_bytes_since_fire = 0;
                self.last_fired_at = Some(Instant::now());
            }
            return if should {
                Some(TriggerSource::Slow)
            } else {
                None
            };
        }

        None
    }

    /// Returns the maximum poll timeout, ensuring we wake in time for the next check.
    pub fn poll_timeout(&self) -> Duration {
        let since_output = self.last_output_at.elapsed();

        // If instant signals are pending, wake immediately
        if self.prompt_detected || self.shell_prompt_returned {
            return Duration::from_millis(50);
        }

        // If output was flowing, use fast threshold
        if self.was_output_flowing {
            let fast_remaining = self.fast_threshold.saturating_sub(since_output);
            let slow_remaining = self.threshold.saturating_sub(since_output);
            return fast_remaining
                .min(slow_remaining)
                .max(Duration::from_secs(1));
        }

        // Otherwise use slow threshold
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
    fn fires_on_prompt_detected() {
        let mut trigger = SmartTrigger::new(30, 5, Some("^❯".to_string()));
        assert!(trigger.should_fire().is_none());
        trigger.on_output_bytes(b"some output\n\xe2\x9d\xaf ");
        assert_eq!(trigger.should_fire(), Some(TriggerSource::Prompt));
        // Clears after firing
        assert!(trigger.should_fire().is_none());
    }

    #[test]
    fn fires_on_shell_prompt() {
        let mut trigger = SmartTrigger::new(30, 5, None);
        trigger.on_shell_prompt();
        assert_eq!(trigger.should_fire(), Some(TriggerSource::ShellPrompt));
        assert!(trigger.should_fire().is_none());
    }

    #[test]
    fn fires_on_fast_trigger_after_output_stops() {
        let mut trigger = SmartTrigger::new(30, 1, None);
        trigger.on_output_bytes(b"output");
        assert!(trigger.should_fire().is_none());
        thread::sleep(Duration::from_millis(1100));
        assert_eq!(trigger.should_fire(), Some(TriggerSource::Fast));
        // was_output_flowing cleared — fast trigger won't fire again
        assert!(trigger.should_fire().is_none());
    }

    #[test]
    fn fires_on_slow_fallback() {
        let mut trigger = SmartTrigger::new(1, 5, None);
        assert!(trigger.should_fire().is_none());
        thread::sleep(Duration::from_millis(1100));
        assert_eq!(trigger.should_fire(), Some(TriggerSource::Slow));
    }

    #[test]
    fn output_resets_timers() {
        let mut trigger = SmartTrigger::new(1, 1, None);
        thread::sleep(Duration::from_millis(1100));
        trigger.on_output_bytes(b"output");
        assert!(trigger.should_fire().is_none());
    }

    #[test]
    fn no_prompt_regex_means_no_prompt_detection() {
        let mut trigger = SmartTrigger::new(30, 5, None);
        trigger.on_output_bytes(b"\xe2\x9d\xaf ");
        // No regex configured, so prompt detection doesn't fire
        assert!(trigger.should_fire().is_none());
    }

    #[test]
    fn poll_timeout_respects_fast_threshold() {
        let mut trigger = SmartTrigger::new(30, 5, None);
        trigger.on_output_bytes(b"output");
        let timeout = trigger.poll_timeout();
        // Should be <= fast_threshold (5s), not the slow 30s
        assert!(timeout <= Duration::from_secs(5));
        assert!(timeout >= Duration::from_secs(1));
    }

    #[test]
    fn slow_fallback_fires_periodically() {
        let mut trigger = SmartTrigger::new(1, 5, None);
        thread::sleep(Duration::from_millis(1100));
        assert_eq!(trigger.should_fire(), Some(TriggerSource::Slow));
        // Shouldn't double-fire immediately
        assert!(trigger.should_fire().is_none());
        // Should fire again after another threshold
        thread::sleep(Duration::from_millis(1100));
        assert_eq!(trigger.should_fire(), Some(TriggerSource::Slow));
    }

    #[test]
    fn invalid_regex_is_ignored() {
        // Invalid regex pattern should not panic, just disable prompt detection
        let mut trigger = SmartTrigger::new(30, 5, Some("[invalid".to_string()));
        trigger.on_output_bytes(b"some output");
        assert!(trigger.should_fire().is_none());
    }

    #[test]
    fn fast_trigger_requires_minimum_output_volume() {
        let mut trigger = SmartTrigger::new(30, 1, None);
        trigger.set_min_output_bytes(256);
        trigger.on_output_bytes(b"short");
        thread::sleep(Duration::from_millis(1100));
        assert!(
            trigger.should_fire().is_none(),
            "fast trigger should not fire below min_output_bytes"
        );
    }

    #[test]
    fn fast_trigger_fires_above_volume_threshold() {
        let mut trigger = SmartTrigger::new(30, 1, None);
        trigger.set_min_output_bytes(256);
        let big_output = vec![b'x'; 300];
        trigger.on_output_bytes(&big_output);
        thread::sleep(Duration::from_millis(1100));
        assert_eq!(
            trigger.should_fire(),
            Some(TriggerSource::Fast),
            "fast trigger should fire above min_output_bytes"
        );
    }

    #[test]
    fn fast_trigger_accumulates_across_calls() {
        let mut trigger = SmartTrigger::new(30, 1, None);
        trigger.set_min_output_bytes(256);
        for _ in 0..30 {
            trigger.on_output_bytes(b"1234567890");
        }
        thread::sleep(Duration::from_millis(1100));
        assert_eq!(
            trigger.should_fire(),
            Some(TriggerSource::Fast),
            "accumulated output should arm fast trigger"
        );
    }
}

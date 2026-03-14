//! OAuth usage tracking for Anthropic API rate limits.
//!
//! Polls the usage API every 60 seconds and sends updates to the main thread.
//! Supports auto-pause at 80% and auto-resume below 20%.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Cached usage data from the Anthropic usage API.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct UsageData {
    /// 5-hour utilization (0.0 to 1.0).
    pub five_hour_utilization: f64,
    /// 5-hour reset time (ISO 8601).
    pub five_hour_resets_at: String,
    /// 7-day utilization (0.0 to 1.0).
    pub seven_day_utilization: f64,
    /// 7-day reset time (ISO 8601).
    pub seven_day_resets_at: String,
    /// When this data was fetched.
    pub fetched_at: Instant,
}

/// Shared usage state accessible from the main thread.
#[derive(Debug, Clone, Default)]
pub struct UsageState {
    /// Latest usage data, if available.
    pub data: Option<UsageData>,
    /// Whether the orchestrator is paused due to usage limits.
    pub paused: bool,
    /// Consecutive API failures (disable display after 3).
    pub consecutive_failures: u32,
}


/// Read the OAuth access token from `~/.claude/.credentials.json`.
fn read_oauth_token() -> Option<String> {
    let home = dirs::home_dir()?;
    let cred_path = home.join(".claude").join(".credentials.json");
    let contents = std::fs::read_to_string(&cred_path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&contents).ok()?;
    parsed
        .get("claudeAiOauth")
        .and_then(|o| o.get("accessToken"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
}

/// Poll the Anthropic usage API and return parsed usage data.
fn poll_usage(token: &str) -> Result<UsageData, String> {
    let response = ureq::get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("Accept", "application/json")
        .call()
        .map_err(|e| format!("Usage API request failed: {e}"))?;

    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Failed to read response body: {e}"))?;

    let parsed: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse usage JSON: {e}"))?;

    let five_hour = parsed
        .get("five_hour")
        .ok_or("Missing five_hour field")?;
    let seven_day = parsed
        .get("seven_day")
        .ok_or("Missing seven_day field")?;

    Ok(UsageData {
        five_hour_utilization: five_hour
            .get("utilization")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        five_hour_resets_at: five_hour
            .get("resets_at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        seven_day_utilization: seven_day
            .get("utilization")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        seven_day_resets_at: seven_day
            .get("resets_at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        fetched_at: Instant::now(),
    })
}

/// Start the background usage polling thread.
///
/// Returns a shared `UsageState` that the main thread can read for status bar display
/// and pause/resume decisions.
pub fn start_polling(
    proxy: winit::event_loop::EventLoopProxy<glass_core::event::AppEvent>,
) -> Arc<Mutex<UsageState>> {
    let state = Arc::new(Mutex::new(UsageState::default()));
    let state_clone = Arc::clone(&state);

    std::thread::Builder::new()
        .name("glass-usage-poller".into())
        .spawn(move || {
            let poll_interval = Duration::from_secs(60);

            loop {
                // Read token fresh each cycle (may be refreshed by Claude Code)
                let token = match read_oauth_token() {
                    Some(t) => t,
                    None => {
                        tracing::debug!("Usage tracker: no OAuth token found");
                        std::thread::sleep(poll_interval);
                        continue;
                    }
                };

                match poll_usage(&token) {
                    Ok(data) => {
                        let mut st = state_clone.lock().unwrap();
                        let five_hour = data.five_hour_utilization;
                        st.data = Some(data);
                        st.consecutive_failures = 0;

                        // Check thresholds
                        if five_hour >= 0.95 {
                            if !st.paused {
                                st.paused = true;
                                tracing::warn!(
                                    "Usage tracker: HARD STOP at {:.0}% — pausing orchestrator",
                                    five_hour * 100.0
                                );
                                let _ = proxy.send_event(
                                    glass_core::event::AppEvent::UsageHardStop,
                                );
                            }
                        } else if five_hour >= 0.80 {
                            if !st.paused {
                                st.paused = true;
                                tracing::warn!(
                                    "Usage tracker: auto-pause at {:.0}%",
                                    five_hour * 100.0
                                );
                                let _ = proxy.send_event(
                                    glass_core::event::AppEvent::UsagePause,
                                );
                            }
                        } else if five_hour < 0.20 && st.paused {
                            st.paused = false;
                            tracing::info!(
                                "Usage tracker: auto-resume at {:.0}%",
                                five_hour * 100.0
                            );
                            let _ = proxy.send_event(
                                glass_core::event::AppEvent::UsageResume,
                            );
                        }
                    }
                    Err(e) => {
                        let mut st = state_clone.lock().unwrap();
                        st.consecutive_failures += 1;
                        if st.consecutive_failures <= 3 {
                            tracing::warn!("Usage tracker: {e} (failure #{})", st.consecutive_failures);
                        }
                        if st.consecutive_failures == 3 {
                            tracing::warn!("Usage tracker: 3 consecutive failures — disabling usage display");
                        }
                    }
                }

                std::thread::sleep(poll_interval);
            }
        })
        .ok();

    state
}

/// Format usage for status bar display.
/// Returns something like "5h: 42% | 7d: 15%" or "5h: --% | 7d: --%" if unavailable.
pub fn format_status_bar(state: &UsageState) -> String {
    if state.consecutive_failures >= 3 {
        return "5h: --% | 7d: --%".to_string();
    }
    match &state.data {
        Some(data) => {
            format!(
                "5h: {:.0}% | 7d: {:.0}%",
                data.five_hour_utilization * 100.0,
                data.seven_day_utilization * 100.0
            )
        }
        None => "5h: --% | 7d: --%".to_string(),
    }
}

/// Get the color tier for a utilization value.
/// Returns 0 (green, 0-70%), 1 (yellow, 70-85%), or 2 (red, 85%+).
#[allow(dead_code)]
pub fn usage_color_tier(utilization: f64) -> u8 {
    if utilization >= 0.85 {
        2 // red
    } else if utilization >= 0.70 {
        1 // yellow
    } else {
        0 // green
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_status_bar_with_data() {
        let state = UsageState {
            data: Some(UsageData {
                five_hour_utilization: 0.42,
                five_hour_resets_at: "2026-03-14T08:00:00Z".to_string(),
                seven_day_utilization: 0.15,
                seven_day_resets_at: "2026-03-20T00:00:00Z".to_string(),
                fetched_at: Instant::now(),
            }),
            paused: false,
            consecutive_failures: 0,
        };
        assert_eq!(format_status_bar(&state), "5h: 42% | 7d: 15%");
    }

    #[test]
    fn format_status_bar_unavailable() {
        let state = UsageState::default();
        assert_eq!(format_status_bar(&state), "5h: --% | 7d: --%");
    }

    #[test]
    fn format_status_bar_disabled_after_failures() {
        let state = UsageState {
            data: Some(UsageData {
                five_hour_utilization: 0.50,
                five_hour_resets_at: String::new(),
                seven_day_utilization: 0.10,
                seven_day_resets_at: String::new(),
                fetched_at: Instant::now(),
            }),
            paused: false,
            consecutive_failures: 3,
        };
        assert_eq!(format_status_bar(&state), "5h: --% | 7d: --%");
    }

    #[test]
    fn usage_color_tiers() {
        assert_eq!(usage_color_tier(0.0), 0);
        assert_eq!(usage_color_tier(0.50), 0);
        assert_eq!(usage_color_tier(0.69), 0);
        assert_eq!(usage_color_tier(0.70), 1);
        assert_eq!(usage_color_tier(0.84), 1);
        assert_eq!(usage_color_tier(0.85), 2);
        assert_eq!(usage_color_tier(1.0), 2);
    }
}

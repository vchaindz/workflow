//! Per-service rate limiting for notification delivery.
//!
//! Uses an Instant-based sliding window to track sends per service.
//! In-process only — resets on restart.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Configuration for per-service rate limiting.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RateLimitConfig {
    /// Maximum number of sends allowed per window (default: 60).
    #[serde(default = "default_max_per_window")]
    pub max_per_window: u32,
    /// Window duration in seconds (default: 60).
    #[serde(default = "default_window_secs")]
    pub window_secs: u64,
}

fn default_max_per_window() -> u32 {
    60
}
fn default_window_secs() -> u64 {
    60
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_per_window: default_max_per_window(),
            window_secs: default_window_secs(),
        }
    }
}

/// Per-service rate limiter using a sliding window of timestamps.
///
/// Thread-safe via internal `Mutex`. Each service name has its own
/// independent window and counter.
#[derive(Debug)]
pub struct RateLimiter {
    /// Per-service send timestamps within the current window.
    windows: Mutex<HashMap<String, Vec<Instant>>>,
    /// Per-service config overrides. Services not in this map use defaults.
    service_configs: HashMap<String, RateLimitConfig>,
    /// Default config for services without a specific override.
    default_config: RateLimitConfig,
}

impl RateLimiter {
    /// Create a new rate limiter with default limits.
    ///
    /// Default limits:
    /// - Discord: 30/min
    /// - Telegram: 30/sec
    /// - Others: 60/min
    pub fn new() -> Self {
        let mut service_configs = HashMap::new();
        service_configs.insert(
            "discord".to_string(),
            RateLimitConfig {
                max_per_window: 30,
                window_secs: 60,
            },
        );
        service_configs.insert(
            "telegram".to_string(),
            RateLimitConfig {
                max_per_window: 30,
                window_secs: 1,
            },
        );

        Self {
            windows: Mutex::new(HashMap::new()),
            service_configs,
            default_config: RateLimitConfig::default(),
        }
    }

    /// Create a rate limiter with custom per-service configs.
    ///
    /// Services not in the map use built-in defaults (Discord 30/min,
    /// Telegram 30/sec, others 60/min).
    pub fn with_configs(configs: HashMap<String, RateLimitConfig>) -> Self {
        let mut limiter = Self::new();
        for (service, config) in configs {
            limiter.service_configs.insert(service, config);
        }
        limiter
    }

    /// Check if a send is allowed for the given service.
    ///
    /// Returns `true` if allowed (and records the send), `false` if rate limited.
    pub fn check_and_record(&self, service: &str) -> bool {
        let config = self
            .service_configs
            .get(service)
            .unwrap_or(&self.default_config);
        let window = Duration::from_secs(config.window_secs);
        let now = Instant::now();
        let cutoff = now - window;

        let mut windows = self.windows.lock().unwrap();
        let timestamps = windows.entry(service.to_string()).or_default();

        // Remove expired timestamps (outside the sliding window)
        timestamps.retain(|t| *t > cutoff);

        if timestamps.len() as u32 >= config.max_per_window {
            return false;
        }

        timestamps.push(now);
        true
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_rate_limiter_allows_within_limit() {
        let limiter = RateLimiter::new();
        // Default for "slack" is 60/min — should allow many sends
        for _ in 0..60 {
            assert!(limiter.check_and_record("slack"));
        }
        // 61st should be blocked
        assert!(!limiter.check_and_record("slack"));
    }

    #[test]
    fn test_rate_limiter_discord_default_limit() {
        let limiter = RateLimiter::new();
        // Discord default: 30/min
        for _ in 0..30 {
            assert!(limiter.check_and_record("discord"));
        }
        assert!(!limiter.check_and_record("discord"));
    }

    #[test]
    fn test_rate_limiter_telegram_default_limit() {
        let limiter = RateLimiter::new();
        // Telegram default: 30/sec
        for _ in 0..30 {
            assert!(limiter.check_and_record("telegram"));
        }
        assert!(!limiter.check_and_record("telegram"));
    }

    #[test]
    fn test_rate_limiter_independent_services() {
        let limiter = RateLimiter::new();
        // Fill up discord (30/min)
        for _ in 0..30 {
            assert!(limiter.check_and_record("discord"));
        }
        assert!(!limiter.check_and_record("discord"));
        // Slack should still work (independent window)
        assert!(limiter.check_and_record("slack"));
    }

    #[test]
    fn test_rate_limiter_window_expiry() {
        // Use a very short window (100ms) to test expiry
        let mut configs = HashMap::new();
        configs.insert(
            "test".to_string(),
            RateLimitConfig {
                max_per_window: 2,
                window_secs: 0, // We'll use a custom approach
            },
        );

        // Instead, create a limiter with a short window via custom config
        let mut service_configs = HashMap::new();
        service_configs.insert(
            "test".to_string(),
            RateLimitConfig {
                max_per_window: 2,
                window_secs: 1, // 1 second window
            },
        );
        let limiter = RateLimiter::with_configs(service_configs);

        // Send 2 — should be allowed
        assert!(limiter.check_and_record("test"));
        assert!(limiter.check_and_record("test"));
        // 3rd should be blocked
        assert!(!limiter.check_and_record("test"));

        // Wait for the window to expire
        thread::sleep(Duration::from_millis(1100));

        // Should be allowed again
        assert!(limiter.check_and_record("test"));
    }

    #[test]
    fn test_rate_limiter_custom_config_overrides_default() {
        let mut configs = HashMap::new();
        configs.insert(
            "slack".to_string(),
            RateLimitConfig {
                max_per_window: 5,
                window_secs: 60,
            },
        );
        let limiter = RateLimiter::with_configs(configs);

        // Custom config: 5/min for slack (instead of default 60/min)
        for _ in 0..5 {
            assert!(limiter.check_and_record("slack"));
        }
        assert!(!limiter.check_and_record("slack"));
    }

    #[test]
    fn test_rate_limit_config_defaults() {
        let config = RateLimitConfig::default();
        assert_eq!(config.max_per_window, 60);
        assert_eq!(config.window_secs, 60);
    }

    #[test]
    fn test_rate_limit_config_serde() {
        let toml_str = r#"
max_per_window = 10
window_secs = 30
"#;
        let config: RateLimitConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.max_per_window, 10);
        assert_eq!(config.window_secs, 30);
    }

    #[test]
    fn test_rate_limit_config_serde_defaults() {
        let toml_str = "";
        let config: RateLimitConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.max_per_window, 60);
        assert_eq!(config.window_secs, 60);
    }

    #[test]
    fn test_rate_limiter_with_configs_preserves_builtins() {
        // Custom config for "ntfy" shouldn't affect discord/telegram builtins
        let mut configs = HashMap::new();
        configs.insert(
            "ntfy".to_string(),
            RateLimitConfig {
                max_per_window: 10,
                window_secs: 60,
            },
        );
        let limiter = RateLimiter::with_configs(configs);

        // Discord should still have its 30/min builtin
        for _ in 0..30 {
            assert!(limiter.check_and_record("discord"));
        }
        assert!(!limiter.check_and_record("discord"));

        // ntfy should use the custom 10/min
        for _ in 0..10 {
            assert!(limiter.check_and_record("ntfy"));
        }
        assert!(!limiter.check_and_record("ntfy"));
    }
}

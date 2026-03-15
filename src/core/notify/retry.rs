//! Retry logic for notification delivery with exponential backoff.

use std::thread;
use std::time::Duration;

use super::{Notification, NotifyError, Notifier};

/// Configuration for notification retry behavior.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RetryConfig {
    /// Maximum number of send attempts (default: 3).
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    /// Initial delay in milliseconds before the first retry (default: 1000).
    #[serde(default = "default_initial_delay_ms")]
    pub initial_delay_ms: u64,
    /// Multiplier applied to the delay after each retry (default: 2.0).
    #[serde(default = "default_backoff_factor")]
    pub backoff_factor: f64,
}

fn default_max_attempts() -> u32 {
    3
}
fn default_initial_delay_ms() -> u64 {
    1000
}
fn default_backoff_factor() -> f64 {
    2.0
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: default_max_attempts(),
            initial_delay_ms: default_initial_delay_ms(),
            backoff_factor: default_backoff_factor(),
        }
    }
}

/// Send a notification with retry and exponential backoff.
///
/// Returns `Ok(())` if any attempt succeeds. Returns the final `NotifyError`
/// with all attempt messages if all attempts fail.
pub fn send_with_retry(
    notifier: &dyn Notifier,
    notification: &Notification,
    config: &RetryConfig,
) -> Result<(), NotifyError> {
    let max = config.max_attempts.max(1);
    let mut errors: Vec<String> = Vec::new();
    let mut delay_ms = config.initial_delay_ms;

    for attempt in 1..=max {
        match notifier.send(notification) {
            Ok(()) => return Ok(()),
            Err(e) => {
                errors.push(format!("attempt {}: {}", attempt, e.message));
                if attempt < max {
                    thread::sleep(Duration::from_millis(delay_ms));
                    delay_ms = (delay_ms as f64 * config.backoff_factor) as u64;
                }
            }
        }
    }

    Err(NotifyError::new(
        notifier.name(),
        format!(
            "all {} attempts failed: {}",
            max,
            errors.join("; ")
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::time::Instant;

    /// A mock notifier that fails a configurable number of times then succeeds.
    #[derive(Debug)]
    struct CountingNotifier {
        name: String,
        fail_count: u32,
        calls: Arc<AtomicU32>,
    }

    impl CountingNotifier {
        fn new(name: &str, fail_count: u32) -> Self {
            Self {
                name: name.to_string(),
                fail_count,
                calls: Arc::new(AtomicU32::new(0)),
            }
        }

        fn call_count(&self) -> u32 {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl Notifier for CountingNotifier {
        fn name(&self) -> &str {
            &self.name
        }

        fn send(&self, _notification: &Notification) -> Result<(), NotifyError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n < self.fail_count {
                Err(NotifyError::new(&self.name, format!("fail #{}", n + 1)))
            } else {
                Ok(())
            }
        }
    }

    fn test_notification() -> Notification {
        Notification::new("test", "body", crate::core::notify::Severity::Info)
    }

    #[test]
    fn test_retry_succeeds_first_attempt() {
        let notifier = CountingNotifier::new("ok", 0);
        let config = RetryConfig {
            max_attempts: 3,
            initial_delay_ms: 10,
            backoff_factor: 2.0,
        };
        let result = send_with_retry(&notifier, &test_notification(), &config);
        assert!(result.is_ok());
        assert_eq!(notifier.call_count(), 1);
    }

    #[test]
    fn test_retry_succeeds_after_failures() {
        let notifier = CountingNotifier::new("flaky", 2);
        let config = RetryConfig {
            max_attempts: 3,
            initial_delay_ms: 10,
            backoff_factor: 2.0,
        };
        let result = send_with_retry(&notifier, &test_notification(), &config);
        assert!(result.is_ok());
        assert_eq!(notifier.call_count(), 3);
    }

    #[test]
    fn test_retry_all_attempts_fail() {
        let notifier = CountingNotifier::new("bad", 10);
        let config = RetryConfig {
            max_attempts: 3,
            initial_delay_ms: 10,
            backoff_factor: 2.0,
        };
        let result = send_with_retry(&notifier, &test_notification(), &config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.service, "bad");
        assert!(err.message.contains("all 3 attempts failed"));
        assert!(err.message.contains("attempt 1: fail #1"));
        assert!(err.message.contains("attempt 2: fail #2"));
        assert!(err.message.contains("attempt 3: fail #3"));
        assert_eq!(notifier.call_count(), 3);
    }

    #[test]
    fn test_retry_exponential_backoff_timing() {
        // With delay 50ms and factor 2.0, retries should wait ~50ms then ~100ms
        let notifier = CountingNotifier::new("slow", 3);
        let config = RetryConfig {
            max_attempts: 3,
            initial_delay_ms: 50,
            backoff_factor: 2.0,
        };
        let start = Instant::now();
        let _ = send_with_retry(&notifier, &test_notification(), &config);
        let elapsed = start.elapsed();
        // Should sleep 50ms + 100ms = 150ms total (with some tolerance)
        assert!(elapsed >= Duration::from_millis(130), "elapsed: {:?}", elapsed);
    }

    #[test]
    fn test_retry_max_attempts_one() {
        let notifier = CountingNotifier::new("once", 5);
        let config = RetryConfig {
            max_attempts: 1,
            initial_delay_ms: 10,
            backoff_factor: 2.0,
        };
        let result = send_with_retry(&notifier, &test_notification(), &config);
        assert!(result.is_err());
        assert_eq!(notifier.call_count(), 1);
    }

    #[test]
    fn test_retry_config_defaults() {
        let config = RetryConfig::default();
        assert_eq!(config.max_attempts, 3);
        assert_eq!(config.initial_delay_ms, 1000);
        assert_eq!(config.backoff_factor, 2.0);
    }

    #[test]
    fn test_retry_config_serde() {
        let toml_str = r#"
max_attempts = 5
initial_delay_ms = 500
backoff_factor = 1.5
"#;
        let config: RetryConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.max_attempts, 5);
        assert_eq!(config.initial_delay_ms, 500);
        assert_eq!(config.backoff_factor, 1.5);
    }

    #[test]
    fn test_retry_config_serde_defaults() {
        let toml_str = "";
        let config: RetryConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.max_attempts, 3);
        assert_eq!(config.initial_delay_ms, 1000);
        assert_eq!(config.backoff_factor, 2.0);
    }
}

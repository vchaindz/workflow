//! Notification system with trait-based backends.
//!
//! Use the `Notifier` trait and `MultiNotifier` dispatcher for all notification
//! delivery. URL-scheme resolution is handled by `resolve::resolve_notifier()`.

mod error;
mod message;
pub mod rate_limit;
pub mod resolve;
pub mod retry;
#[cfg(feature = "slack")]
pub mod slack;
#[cfg(feature = "webhook")]
pub mod webhook;
#[cfg(feature = "discord")]
pub mod discord;
#[cfg(feature = "email")]
pub mod email;
#[cfg(feature = "telegram")]
pub mod telegram;
#[cfg(feature = "ntfy")]
pub mod ntfy;
#[cfg(feature = "msteams")]
pub mod msteams;
#[cfg(feature = "gotify")]
pub mod gotify;

pub use error::NotifyError;
pub use message::{Notification, Severity};
pub use rate_limit::{RateLimitConfig, RateLimiter};
pub use retry::RetryConfig;

use std::fmt;

/// Trait implemented by each notification backend.
///
/// Implementors must be `Send + Sync` so they can be shared across threads.
pub trait Notifier: Send + Sync + fmt::Debug {
    /// A human-readable name for this backend (e.g. "slack", "email").
    fn name(&self) -> &str;

    /// Send a notification. Returns `Ok(())` on success or a `NotifyError` on failure.
    fn send(&self, notification: &Notification) -> Result<(), NotifyError>;
}

/// Dispatcher that fans out a notification to multiple backends.
#[derive(Debug, Default)]
pub struct MultiNotifier {
    backends: Vec<Box<dyn Notifier>>,
}

impl MultiNotifier {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a notification backend.
    pub fn add(&mut self, backend: Box<dyn Notifier>) {
        self.backends.push(backend);
    }

    /// Send a notification to all backends. Returns errors from any that failed.
    /// A failure in one backend does not prevent others from being tried.
    pub fn send_all(&self, notification: &Notification) -> Vec<NotifyError> {
        let mut errors = Vec::new();
        for backend in &self.backends {
            if let Err(e) = backend.send(notification) {
                errors.push(e);
            }
        }
        errors
    }

    /// Number of registered backends.
    pub fn len(&self) -> usize {
        self.backends.len()
    }

    /// Returns true if no backends are registered.
    pub fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A test notifier that records received notifications.
    #[derive(Debug, Clone)]
    struct RecordingNotifier {
        name: String,
        received: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingNotifier {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                received: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn received(&self) -> Vec<String> {
            self.received.lock().unwrap().clone()
        }
    }

    impl Notifier for RecordingNotifier {
        fn name(&self) -> &str {
            &self.name
        }

        fn send(&self, notification: &Notification) -> Result<(), NotifyError> {
            self.received
                .lock()
                .unwrap()
                .push(notification.subject.clone());
            Ok(())
        }
    }

    /// A test notifier that always fails.
    #[derive(Debug)]
    struct FailingNotifier {
        name: String,
    }

    impl Notifier for FailingNotifier {
        fn name(&self) -> &str {
            &self.name
        }

        fn send(&self, _notification: &Notification) -> Result<(), NotifyError> {
            Err(NotifyError::new(&self.name, "always fails"))
        }
    }

    #[test]
    fn test_multi_notifier_sends_to_all() {
        let a = RecordingNotifier::new("a");
        let b = RecordingNotifier::new("b");
        let a_clone = a.clone();
        let b_clone = b.clone();

        let mut multi = MultiNotifier::new();
        multi.add(Box::new(a));
        multi.add(Box::new(b));

        assert_eq!(multi.len(), 2);
        assert!(!multi.is_empty());

        let notif = Notification::new("test subject", "test body", Severity::Info);
        let errors = multi.send_all(&notif);

        assert!(errors.is_empty());
        assert_eq!(a_clone.received(), vec!["test subject"]);
        assert_eq!(b_clone.received(), vec!["test subject"]);
    }

    #[test]
    fn test_multi_notifier_collects_errors() {
        let ok = RecordingNotifier::new("ok");
        let ok_clone = ok.clone();

        let mut multi = MultiNotifier::new();
        multi.add(Box::new(FailingNotifier {
            name: "bad1".to_string(),
        }));
        multi.add(Box::new(ok));
        multi.add(Box::new(FailingNotifier {
            name: "bad2".to_string(),
        }));

        let notif = Notification::new("hello", "", Severity::Success);
        let errors = multi.send_all(&notif);

        // The ok notifier still received the message
        assert_eq!(ok_clone.received(), vec!["hello"]);
        // Two errors collected
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].service, "bad1");
        assert_eq!(errors[1].service, "bad2");
    }

    #[test]
    fn test_multi_notifier_empty() {
        let multi = MultiNotifier::new();
        assert!(multi.is_empty());
        assert_eq!(multi.len(), 0);

        let notif = Notification::new("x", "y", Severity::Warning);
        let errors = multi.send_all(&notif);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_notifier_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MultiNotifier>();
        assert_send_sync::<Notification>();
        assert_send_sync::<NotifyError>();
        assert_send_sync::<Severity>();
    }
}

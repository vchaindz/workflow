use std::fmt;

/// Error from a notification backend.
#[derive(Debug, Clone)]
pub struct NotifyError {
    /// Name of the notification service that failed (e.g. "slack", "email").
    pub service: String,
    /// Human-readable error message.
    pub message: String,
}

impl NotifyError {
    pub fn new(service: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for NotifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.service, self.message)
    }
}

impl std::error::Error for NotifyError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notify_error_display() {
        let err = NotifyError::new("slack", "connection refused");
        assert_eq!(err.to_string(), "[slack] connection refused");
    }
}

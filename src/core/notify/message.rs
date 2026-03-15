use std::collections::HashMap;
use std::fmt;

/// Severity level for a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Success,
    Failure,
    Warning,
    Info,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Success => write!(f, "success"),
            Severity::Failure => write!(f, "failure"),
            Severity::Warning => write!(f, "warning"),
            Severity::Info => write!(f, "info"),
        }
    }
}

/// A structured notification message sent to one or more backends.
#[derive(Debug, Clone)]
pub struct Notification {
    pub subject: String,
    pub body: String,
    pub severity: Severity,
    pub fields: HashMap<String, String>,
}

impl Notification {
    pub fn new(subject: impl Into<String>, body: impl Into<String>, severity: Severity) -> Self {
        Self {
            subject: subject.into(),
            body: body.into(),
            severity,
            fields: HashMap::new(),
        }
    }

    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_builder() {
        let n = Notification::new("Deploy failed", "Step build returned exit 1", Severity::Failure)
            .with_field("task", "infra/deploy")
            .with_field("exit_code", "1");

        assert_eq!(n.subject, "Deploy failed");
        assert_eq!(n.body, "Step build returned exit 1");
        assert_eq!(n.severity, Severity::Failure);
        assert_eq!(n.fields.get("task").unwrap(), "infra/deploy");
        assert_eq!(n.fields.get("exit_code").unwrap(), "1");
    }

    #[test]
    fn test_severity_display() {
        assert_eq!(Severity::Success.to_string(), "success");
        assert_eq!(Severity::Failure.to_string(), "failure");
        assert_eq!(Severity::Warning.to_string(), "warning");
        assert_eq!(Severity::Info.to_string(), "info");
    }
}

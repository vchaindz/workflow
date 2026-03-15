//! Generic webhook notification backend.

use super::error::NotifyError;
use super::message::{Notification, Severity};
use super::Notifier;

/// Sends notifications to a generic HTTP webhook endpoint.
#[derive(Debug)]
pub struct GenericWebhook {
    webhook_url: String,
}

impl GenericWebhook {
    pub fn new(webhook_url: impl Into<String>) -> Self {
        Self {
            webhook_url: webhook_url.into(),
        }
    }

    /// Build the JSON payload with subject, body, severity, and fields.
    fn build_payload(&self, notification: &Notification) -> serde_json::Value {
        let severity_str = match notification.severity {
            Severity::Success => "success",
            Severity::Failure => "failure",
            Severity::Warning => "warning",
            Severity::Info => "info",
        };

        let mut fields = serde_json::Map::new();
        let mut keys: Vec<&String> = notification.fields.keys().collect();
        keys.sort();
        for key in keys {
            fields.insert(
                key.clone(),
                serde_json::Value::String(notification.fields[key].clone()),
            );
        }

        serde_json::json!({
            "subject": notification.subject,
            "body": notification.body,
            "severity": severity_str,
            "fields": fields
        })
    }
}

impl Notifier for GenericWebhook {
    fn name(&self) -> &str {
        "webhook"
    }

    fn send(&self, notification: &Notification) -> Result<(), NotifyError> {
        let payload = self.build_payload(notification);

        ureq::post(&self.webhook_url)
            .set("Content-Type", "application/json")
            .send_string(&payload.to_string())
            .map_err(|e| NotifyError::new("webhook", format!("HTTP request failed: {e}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payload_structure() {
        let wh = GenericWebhook::new("https://example.com/hook");
        let notif = Notification::new("Deploy succeeded", "All steps passed", Severity::Success)
            .with_field("task", "infra/deploy")
            .with_field("duration", "12s");

        let payload = wh.build_payload(&notif);

        assert_eq!(payload["subject"], "Deploy succeeded");
        assert_eq!(payload["body"], "All steps passed");
        assert_eq!(payload["severity"], "success");

        let fields = payload["fields"].as_object().unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields["duration"], "12s");
        assert_eq!(fields["task"], "infra/deploy");
    }

    #[test]
    fn test_payload_severity_values() {
        let wh = GenericWebhook::new("https://example.com/hook");

        for (severity, expected) in [
            (Severity::Success, "success"),
            (Severity::Failure, "failure"),
            (Severity::Warning, "warning"),
            (Severity::Info, "info"),
        ] {
            let notif = Notification::new("test", "", severity);
            let payload = wh.build_payload(&notif);
            assert_eq!(
                payload["severity"], expected,
                "wrong severity string for {severity}"
            );
        }
    }

    #[test]
    fn test_payload_no_fields() {
        let wh = GenericWebhook::new("https://example.com/hook");
        let notif = Notification::new("hello", "world", Severity::Info);
        let payload = wh.build_payload(&notif);

        let fields = payload["fields"].as_object().unwrap();
        assert!(fields.is_empty());
    }

    #[test]
    fn test_payload_fields_sorted() {
        let wh = GenericWebhook::new("https://example.com/hook");
        let notif = Notification::new("test", "", Severity::Info)
            .with_field("zebra", "z")
            .with_field("alpha", "a")
            .with_field("middle", "m");

        let payload = wh.build_payload(&notif);
        let fields = payload["fields"].as_object().unwrap();
        let keys: Vec<&String> = fields.keys().collect();
        assert_eq!(keys, vec!["alpha", "middle", "zebra"]);
    }
}

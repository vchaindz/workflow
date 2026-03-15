//! Mattermost webhook notification backend.
//!
//! Mattermost incoming webhooks accept the same JSON payload format as Slack,
//! so this backend reuses the same attachment/fields structure.

use super::error::NotifyError;
use super::message::{Notification, Severity};
use super::Notifier;

/// Sends notifications to Mattermost via an incoming webhook URL.
#[derive(Debug)]
pub struct MattermostWebhook {
    webhook_url: String,
}

impl MattermostWebhook {
    pub fn new(webhook_url: impl Into<String>) -> Self {
        Self {
            webhook_url: webhook_url.into(),
        }
    }

    /// Map severity to a hex color string for the sidebar.
    fn severity_color(severity: &Severity) -> &'static str {
        match severity {
            Severity::Success => "#2ecc71", // green
            Severity::Failure => "#e74c3c", // red
            Severity::Warning => "#f39c12", // orange
            Severity::Info => "#3498db",    // blue
        }
    }

    /// Build the Mattermost JSON payload using attachments with a colored sidebar.
    ///
    /// Mattermost supports Slack-compatible attachments with:
    /// - `fallback`: plain text summary
    /// - `color`: sidebar color
    /// - `title`: attachment title
    /// - `text`: body text with Markdown support
    /// - `fields`: key/value pairs rendered inline
    fn build_payload(&self, notification: &Notification) -> serde_json::Value {
        let color = Self::severity_color(&notification.severity);

        let mut fields: Vec<serde_json::Value> = Vec::new();
        if !notification.fields.is_empty() {
            let mut sorted_keys: Vec<&String> = notification.fields.keys().collect();
            sorted_keys.sort();
            for k in sorted_keys {
                fields.push(serde_json::json!({
                    "short": true,
                    "title": k,
                    "value": notification.fields[k]
                }));
            }
        }

        let mut attachment = serde_json::json!({
            "fallback": notification.subject,
            "color": color,
            "title": notification.subject,
        });

        if !notification.body.is_empty() {
            attachment["text"] = serde_json::Value::String(notification.body.clone());
        }
        if !fields.is_empty() {
            attachment["fields"] = serde_json::Value::Array(fields);
        }

        serde_json::json!({
            "attachments": [attachment]
        })
    }
}

impl Notifier for MattermostWebhook {
    fn name(&self) -> &str {
        "mattermost"
    }

    fn send(&self, notification: &Notification) -> Result<(), NotifyError> {
        let payload = self.build_payload(notification);

        ureq::post(&self.webhook_url)
            .set("Content-Type", "application/json")
            .send_string(&payload.to_string())
            .map_err(|e| {
                NotifyError::new("mattermost", format!("HTTP request failed: {e}"))
            })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attachment_structure_success() {
        let mm = MattermostWebhook::new("https://mattermost.example.com/hooks/xxx");
        let notif = Notification::new("Deploy succeeded", "All steps passed", Severity::Success)
            .with_field("task", "infra/deploy")
            .with_field("duration", "12s");

        let payload = mm.build_payload(&notif);

        let attachments = payload["attachments"].as_array().unwrap();
        assert_eq!(attachments.len(), 1);

        let att = &attachments[0];
        assert_eq!(att["color"], "#2ecc71");
        assert_eq!(att["title"], "Deploy succeeded");
        assert_eq!(att["text"], "All steps passed");
        assert_eq!(att["fallback"], "Deploy succeeded");

        let fields = att["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 2);
        // Fields sorted alphabetically
        assert_eq!(fields[0]["title"], "duration");
        assert_eq!(fields[0]["value"], "12s");
        assert!(fields[0]["short"].as_bool().unwrap());
        assert_eq!(fields[1]["title"], "task");
        assert_eq!(fields[1]["value"], "infra/deploy");
    }

    #[test]
    fn test_attachment_structure_failure_no_fields() {
        let mm = MattermostWebhook::new("https://mm.local/hooks/abc");
        let notif = Notification::new("Build failed", "Step compile exited 1", Severity::Failure);

        let payload = mm.build_payload(&notif);
        let att = &payload["attachments"][0];
        assert_eq!(att["color"], "#e74c3c");
        assert_eq!(att["title"], "Build failed");
        assert_eq!(att["text"], "Step compile exited 1");
        assert!(att.get("fields").is_none());
    }

    #[test]
    fn test_severity_colors() {
        let mm = MattermostWebhook::new("https://example.com/hooks/x");

        for (severity, expected_color) in [
            (Severity::Success, "#2ecc71"),
            (Severity::Failure, "#e74c3c"),
            (Severity::Warning, "#f39c12"),
            (Severity::Info, "#3498db"),
        ] {
            let notif = Notification::new("test", "", severity);
            let payload = mm.build_payload(&notif);
            assert_eq!(
                payload["attachments"][0]["color"],
                expected_color,
                "wrong color for {severity}"
            );
        }
    }

    #[test]
    fn test_no_body_no_fields() {
        let mm = MattermostWebhook::new("https://example.com/hooks/x");
        let notif = Notification::new("hello", "", Severity::Info);
        let payload = mm.build_payload(&notif);

        let att = &payload["attachments"][0];
        assert_eq!(att["title"], "hello");
        assert!(att.get("text").is_none());
        assert!(att.get("fields").is_none());
    }
}

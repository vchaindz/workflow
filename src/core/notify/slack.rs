//! Slack webhook notification backend.

use super::error::NotifyError;
use super::message::{Notification, Severity};
use super::Notifier;

/// Sends notifications to Slack via an incoming webhook URL.
#[derive(Debug)]
pub struct SlackWebhook {
    webhook_url: String,
}

impl SlackWebhook {
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

    /// Build the Slack JSON payload using Block Kit with a colored attachment sidebar.
    ///
    /// Layout:
    /// - Header block with subject text
    /// - Section block with body (if non-empty)
    /// - Section block with fields (if any), rendered as `*key:* value` mrkdwn fields
    /// - Color sidebar via attachment wrapper
    fn build_payload(&self, notification: &Notification) -> serde_json::Value {
        match self.build_rich_payload(notification) {
            Some(payload) => payload,
            None => self.build_plain_payload(notification),
        }
    }

    /// Build a Block Kit rich payload. Returns None if construction fails.
    fn build_rich_payload(&self, notification: &Notification) -> Option<serde_json::Value> {
        let color = Self::severity_color(&notification.severity);

        let mut blocks: Vec<serde_json::Value> = Vec::new();

        // Header block
        blocks.push(serde_json::json!({
            "type": "header",
            "text": {
                "type": "plain_text",
                "text": notification.subject,
                "emoji": true
            }
        }));

        // Body section (if non-empty)
        if !notification.body.is_empty() {
            blocks.push(serde_json::json!({
                "type": "section",
                "text": {
                    "type": "mrkdwn",
                    "text": notification.body
                }
            }));
        }

        // Fields section (if any)
        if !notification.fields.is_empty() {
            let mut sorted_keys: Vec<&String> = notification.fields.keys().collect();
            sorted_keys.sort();
            let field_elements: Vec<serde_json::Value> = sorted_keys
                .iter()
                .map(|k| {
                    serde_json::json!({
                        "type": "mrkdwn",
                        "text": format!("*{}:* {}", k, notification.fields[*k])
                    })
                })
                .collect();
            blocks.push(serde_json::json!({
                "type": "section",
                "fields": field_elements
            }));
        }

        Some(serde_json::json!({
            "attachments": [{
                "color": color,
                "blocks": blocks
            }]
        }))
    }

    /// Build a plain-text fallback payload (legacy attachment format).
    fn build_plain_payload(&self, notification: &Notification) -> serde_json::Value {
        let mut text = notification.subject.clone();
        if !notification.body.is_empty() {
            text.push('\n');
            text.push_str(&notification.body);
        }
        if !notification.fields.is_empty() {
            let mut sorted_keys: Vec<&String> = notification.fields.keys().collect();
            sorted_keys.sort();
            for k in sorted_keys {
                text.push_str(&format!("\n{}: {}", k, notification.fields[k]));
            }
        }
        serde_json::json!({
            "text": text
        })
    }
}

impl Notifier for SlackWebhook {
    fn name(&self) -> &str {
        "slack"
    }

    fn send(&self, notification: &Notification) -> Result<(), NotifyError> {
        let payload = self.build_payload(notification);

        ureq::post(&self.webhook_url)
            .set("Content-Type", "application/json")
            .send_string(&payload.to_string())
            .map_err(|e| NotifyError::new("slack", format!("HTTP request failed: {e}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_kit_structure_success() {
        let slack = SlackWebhook::new("https://hooks.slack.com/services/T00/B00/xxx");
        let notif = Notification::new("Deploy succeeded", "All steps passed", Severity::Success)
            .with_field("task", "infra/deploy")
            .with_field("duration", "12s");

        let payload = slack.build_payload(&notif);

        let attachments = payload["attachments"].as_array().unwrap();
        assert_eq!(attachments.len(), 1);

        let att = &attachments[0];
        assert_eq!(att["color"], "#2ecc71");

        let blocks = att["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 3); // header + body section + fields section

        // Header block
        assert_eq!(blocks[0]["type"], "header");
        assert_eq!(blocks[0]["text"]["type"], "plain_text");
        assert_eq!(blocks[0]["text"]["text"], "Deploy succeeded");

        // Body section
        assert_eq!(blocks[1]["type"], "section");
        assert_eq!(blocks[1]["text"]["type"], "mrkdwn");
        assert_eq!(blocks[1]["text"]["text"], "All steps passed");

        // Fields section (sorted alphabetically)
        assert_eq!(blocks[2]["type"], "section");
        let fields = blocks[2]["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0]["type"], "mrkdwn");
        assert_eq!(fields[0]["text"], "*duration:* 12s");
        assert_eq!(fields[1]["text"], "*task:* infra/deploy");
    }

    #[test]
    fn test_block_kit_structure_failure() {
        let slack = SlackWebhook::new("https://hooks.slack.com/services/T00/B00/xxx");
        let notif = Notification::new("Build failed", "Step compile exited 1", Severity::Failure);

        let payload = slack.build_payload(&notif);
        let att = &payload["attachments"][0];
        assert_eq!(att["color"], "#e74c3c");

        let blocks = att["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 2); // header + body, no fields

        assert_eq!(blocks[0]["type"], "header");
        assert_eq!(blocks[0]["text"]["text"], "Build failed");

        assert_eq!(blocks[1]["type"], "section");
        assert_eq!(blocks[1]["text"]["text"], "Step compile exited 1");
    }

    #[test]
    fn test_payload_severity_colors() {
        let slack = SlackWebhook::new("https://example.com");

        for (severity, expected_color) in [
            (Severity::Success, "#2ecc71"),
            (Severity::Failure, "#e74c3c"),
            (Severity::Warning, "#f39c12"),
            (Severity::Info, "#3498db"),
        ] {
            let notif = Notification::new("test", "", severity);
            let payload = slack.build_payload(&notif);
            assert_eq!(
                payload["attachments"][0]["color"],
                expected_color,
                "wrong color for {severity}"
            );
        }
    }

    #[test]
    fn test_block_kit_no_fields_no_body() {
        let slack = SlackWebhook::new("https://example.com");
        let notif = Notification::new("hello", "", Severity::Info);
        let payload = slack.build_payload(&notif);

        let blocks = payload["attachments"][0]["blocks"].as_array().unwrap();
        // Only header block, no body section, no fields section
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "header");
        assert_eq!(blocks[0]["text"]["text"], "hello");
    }

    #[test]
    fn test_plain_fallback_payload() {
        let slack = SlackWebhook::new("https://example.com");
        let notif = Notification::new("Deploy done", "All passed", Severity::Success)
            .with_field("env", "prod");

        let fallback = slack.build_plain_payload(&notif);
        let text = fallback["text"].as_str().unwrap();
        assert!(text.contains("Deploy done"));
        assert!(text.contains("All passed"));
        assert!(text.contains("env: prod"));
        // Plain fallback has no attachments/blocks
        assert!(fallback.get("attachments").is_none());
    }
}

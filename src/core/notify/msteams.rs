//! Microsoft Teams webhook notification backend.
//!
//! Uses Adaptive Card JSON format for the new Workflows/Power Automate connector.

use super::error::NotifyError;
use super::message::{Notification, Severity};
use super::Notifier;

/// Sends notifications to Microsoft Teams via a webhook URL using Adaptive Cards.
#[derive(Debug)]
pub struct TeamsWebhook {
    webhook_url: String,
}

impl TeamsWebhook {
    pub fn new(webhook_url: impl Into<String>) -> Self {
        Self {
            webhook_url: webhook_url.into(),
        }
    }

    /// Map severity to an accent color hex string for the card header.
    fn accent_color(severity: &Severity) -> &'static str {
        match severity {
            Severity::Success => "Good",   // green
            Severity::Failure => "Attention", // red
            Severity::Warning => "Warning",   // orange/yellow
            Severity::Info => "Accent",       // blue
        }
    }

    /// Severity label for the header.
    fn severity_label(severity: &Severity) -> &'static str {
        match severity {
            Severity::Success => "SUCCESS",
            Severity::Failure => "FAILURE",
            Severity::Warning => "WARNING",
            Severity::Info => "INFO",
        }
    }

    /// Build the Adaptive Card JSON payload.
    ///
    /// Uses Adaptive Cards with TextBlock header, FactSet for fields, and colored accent.
    /// Falls back to plain text MessageCard if rich formatting construction fails.
    fn build_payload(&self, notification: &Notification) -> serde_json::Value {
        match self.build_rich_payload(notification) {
            Some(payload) => payload,
            None => self.build_plain_payload(notification),
        }
    }

    /// Build a plain-text fallback payload (simple MessageCard).
    fn build_plain_payload(&self, notification: &Notification) -> serde_json::Value {
        let label = Self::severity_label(&notification.severity);
        let mut text = format!("[{}] {}", label, notification.subject);
        if !notification.body.is_empty() {
            text.push_str(&format!("\n{}", notification.body));
        }
        if !notification.fields.is_empty() {
            let mut sorted_keys: Vec<&String> = notification.fields.keys().collect();
            sorted_keys.sort();
            for k in sorted_keys {
                text.push_str(&format!("\n{}: {}", k, notification.fields[k]));
            }
        }
        serde_json::json!({
            "type": "message",
            "attachments": [{
                "contentType": "application/vnd.microsoft.card.adaptive",
                "content": {
                    "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
                    "type": "AdaptiveCard",
                    "version": "1.4",
                    "body": [{
                        "type": "TextBlock",
                        "text": text,
                        "wrap": true
                    }]
                }
            }]
        })
    }

    /// Build a rich Adaptive Card payload. Returns None if construction fails.
    fn build_rich_payload(&self, notification: &Notification) -> Option<serde_json::Value> {
        let color = Self::accent_color(&notification.severity);
        let label = Self::severity_label(&notification.severity);

        // Build facts from workflow fields, sorted alphabetically for deterministic output.
        let mut fact_keys: Vec<&String> = notification.fields.keys().collect();
        fact_keys.sort();
        let facts: Vec<serde_json::Value> = fact_keys
            .iter()
            .map(|k| {
                serde_json::json!({
                    "title": *k,
                    "value": notification.fields[*k]
                })
            })
            .collect();

        // Build the card body items.
        let mut body_items: Vec<serde_json::Value> = vec![
            // Colored header with severity label
            serde_json::json!({
                "type": "TextBlock",
                "size": "Medium",
                "weight": "Bolder",
                "text": format!("[{label}] {}", notification.subject),
                "color": color,
                "wrap": true
            }),
        ];

        // Add body text if present.
        if !notification.body.is_empty() {
            body_items.push(serde_json::json!({
                "type": "TextBlock",
                "text": notification.body,
                "wrap": true
            }));
        }

        // Add facts if present.
        if !facts.is_empty() {
            body_items.push(serde_json::json!({
                "type": "FactSet",
                "facts": facts
            }));
        }

        Some(serde_json::json!({
            "type": "message",
            "attachments": [{
                "contentType": "application/vnd.microsoft.card.adaptive",
                "contentUrl": null,
                "content": {
                    "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
                    "type": "AdaptiveCard",
                    "version": "1.4",
                    "body": body_items
                }
            }]
        }))
    }
}

impl Notifier for TeamsWebhook {
    fn name(&self) -> &str {
        "msteams"
    }

    fn send(&self, notification: &Notification) -> Result<(), NotifyError> {
        let payload = self.build_payload(notification);

        ureq::post(&self.webhook_url)
            .set("Content-Type", "application/json")
            .send_string(&payload.to_string())
            .map_err(|e| NotifyError::new("msteams", format!("HTTP request failed: {e}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adaptive_card_structure_success() {
        let teams = TeamsWebhook::new("https://outlook.office.com/webhook/test");
        let notif = Notification::new("Deploy succeeded", "All steps passed", Severity::Success)
            .with_field("task", "infra/deploy")
            .with_field("duration", "12s");

        let payload = teams.build_payload(&notif);

        assert_eq!(payload["type"], "message");
        let attachments = payload["attachments"].as_array().unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!(
            attachments[0]["contentType"],
            "application/vnd.microsoft.card.adaptive"
        );

        let card = &attachments[0]["content"];
        assert_eq!(card["type"], "AdaptiveCard");
        assert_eq!(card["version"], "1.4");

        let body = card["body"].as_array().unwrap();
        // Header TextBlock
        assert_eq!(body[0]["type"], "TextBlock");
        assert_eq!(body[0]["text"], "[SUCCESS] Deploy succeeded");
        assert_eq!(body[0]["color"], "Good");
        assert_eq!(body[0]["weight"], "Bolder");

        // Body TextBlock
        assert_eq!(body[1]["type"], "TextBlock");
        assert_eq!(body[1]["text"], "All steps passed");

        // FactSet with sorted fields
        assert_eq!(body[2]["type"], "FactSet");
        let facts = body[2]["facts"].as_array().unwrap();
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0]["title"], "duration");
        assert_eq!(facts[0]["value"], "12s");
        assert_eq!(facts[1]["title"], "task");
        assert_eq!(facts[1]["value"], "infra/deploy");
    }

    #[test]
    fn test_adaptive_card_failure() {
        let teams = TeamsWebhook::new("https://outlook.office.com/webhook/test");
        let notif = Notification::new("Build failed", "Step compile exited 1", Severity::Failure);

        let payload = teams.build_payload(&notif);
        let body = payload["attachments"][0]["content"]["body"]
            .as_array()
            .unwrap();

        assert_eq!(body[0]["text"], "[FAILURE] Build failed");
        assert_eq!(body[0]["color"], "Attention");
        // Body text present, no FactSet
        assert_eq!(body[1]["text"], "Step compile exited 1");
        assert_eq!(body.len(), 2); // header + body, no facts
    }

    #[test]
    fn test_severity_colors() {
        let teams = TeamsWebhook::new("https://example.com");

        for (severity, expected_color) in [
            (Severity::Success, "Good"),
            (Severity::Failure, "Attention"),
            (Severity::Warning, "Warning"),
            (Severity::Info, "Accent"),
        ] {
            let notif = Notification::new("test", "", severity);
            let payload = teams.build_payload(&notif);
            let header = &payload["attachments"][0]["content"]["body"][0];
            assert_eq!(
                header["color"], expected_color,
                "wrong color for {severity}"
            );
        }
    }

    #[test]
    fn test_no_fields_no_body() {
        let teams = TeamsWebhook::new("https://example.com");
        let notif = Notification::new("hello", "", Severity::Info);
        let payload = teams.build_payload(&notif);

        let body = payload["attachments"][0]["content"]["body"]
            .as_array()
            .unwrap();
        // Only header, no body text, no facts
        assert_eq!(body.len(), 1);
        assert_eq!(body[0]["text"], "[INFO] hello");
        assert_eq!(body[0]["color"], "Accent");
    }

    #[test]
    fn test_fields_only_no_body() {
        let teams = TeamsWebhook::new("https://example.com");
        let notif = Notification::new("Status", "", Severity::Warning)
            .with_field("env", "prod");
        let payload = teams.build_payload(&notif);

        let body = payload["attachments"][0]["content"]["body"]
            .as_array()
            .unwrap();
        // Header + FactSet (no body text since it's empty)
        assert_eq!(body.len(), 2);
        assert_eq!(body[0]["type"], "TextBlock");
        assert_eq!(body[1]["type"], "FactSet");
        let facts = body[1]["facts"].as_array().unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0]["title"], "env");
        assert_eq!(facts[0]["value"], "prod");
    }
}

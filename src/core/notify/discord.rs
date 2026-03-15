//! Discord webhook notification backend.

use super::error::NotifyError;
use super::message::{Notification, Severity};
use super::Notifier;

/// Sends notifications to Discord via a webhook URL.
#[derive(Debug)]
pub struct DiscordWebhook {
    webhook_url: String,
}

impl DiscordWebhook {
    pub fn new(webhook_url: impl Into<String>) -> Self {
        Self {
            webhook_url: webhook_url.into(),
        }
    }

    /// Map severity to an integer color value.
    fn severity_color(severity: &Severity) -> u32 {
        match severity {
            Severity::Success => 0x2ECC71, // green
            Severity::Failure => 0xE74C3C, // red
            Severity::Warning => 0xF39C12, // orange
            Severity::Info => 0x3498DB,    // blue
        }
    }

    /// Build the Discord JSON payload with a severity-colored embed.
    ///
    /// Uses embeds with title, description, colored sidebar, and inline fields.
    /// Falls back to plain text content if rich formatting construction fails.
    fn build_payload(&self, notification: &Notification) -> serde_json::Value {
        match self.build_rich_payload(notification) {
            Some(payload) => payload,
            None => self.build_plain_payload(notification),
        }
    }

    /// Build a rich embed payload. Returns None if construction fails.
    fn build_rich_payload(&self, notification: &Notification) -> Option<serde_json::Value> {
        let color = Self::severity_color(&notification.severity);

        let mut fields: Vec<serde_json::Value> = notification
            .fields
            .iter()
            .map(|(k, v)| {
                serde_json::json!({
                    "name": k,
                    "value": v,
                    "inline": true
                })
            })
            .collect();

        // Sort fields by name for deterministic output in tests.
        fields.sort_by(|a, b| {
            let ak = a["name"].as_str().unwrap_or("");
            let bk = b["name"].as_str().unwrap_or("");
            ak.cmp(bk)
        });

        Some(serde_json::json!({
            "embeds": [{
                "title": notification.subject,
                "description": notification.body,
                "color": color,
                "fields": fields
            }]
        }))
    }

    /// Build a plain-text fallback payload.
    fn build_plain_payload(&self, notification: &Notification) -> serde_json::Value {
        let mut text = notification.subject.clone();
        if !notification.body.is_empty() {
            text.push_str("\n");
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
            "content": text
        })
    }
}

impl Notifier for DiscordWebhook {
    fn name(&self) -> &str {
        "discord"
    }

    fn send(&self, notification: &Notification) -> Result<(), NotifyError> {
        let payload = self.build_payload(notification);

        ureq::post(&self.webhook_url)
            .set("Content-Type", "application/json")
            .send_string(&payload.to_string())
            .map_err(|e| NotifyError::new("discord", format!("HTTP request failed: {e}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embed_structure_success() {
        let discord = DiscordWebhook::new("https://discord.com/api/webhooks/123/abc");
        let notif = Notification::new("Deploy succeeded", "All steps passed", Severity::Success)
            .with_field("task", "infra/deploy")
            .with_field("duration", "12s");

        let payload = discord.build_payload(&notif);

        let embeds = payload["embeds"].as_array().unwrap();
        assert_eq!(embeds.len(), 1);

        let embed = &embeds[0];
        assert_eq!(embed["title"], "Deploy succeeded");
        assert_eq!(embed["description"], "All steps passed");
        assert_eq!(embed["color"], 0x2ECC71);

        let fields = embed["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 2);
        // Fields are sorted alphabetically by name
        assert_eq!(fields[0]["name"], "duration");
        assert_eq!(fields[0]["value"], "12s");
        assert_eq!(fields[0]["inline"], true);
        assert_eq!(fields[1]["name"], "task");
        assert_eq!(fields[1]["value"], "infra/deploy");
        assert_eq!(fields[1]["inline"], true);
    }

    #[test]
    fn test_embed_structure_failure() {
        let discord = DiscordWebhook::new("https://discord.com/api/webhooks/123/abc");
        let notif = Notification::new("Build failed", "Step compile exited 1", Severity::Failure);

        let payload = discord.build_payload(&notif);
        let embed = &payload["embeds"][0];
        assert_eq!(embed["color"], 0xE74C3C);
        assert_eq!(embed["title"], "Build failed");
        assert_eq!(embed["fields"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_severity_colors() {
        let discord = DiscordWebhook::new("https://example.com");

        for (severity, expected_color) in [
            (Severity::Success, 0x2ECC71),
            (Severity::Failure, 0xE74C3C),
            (Severity::Warning, 0xF39C12),
            (Severity::Info, 0x3498DB),
        ] {
            let notif = Notification::new("test", "", severity);
            let payload = discord.build_payload(&notif);
            assert_eq!(
                payload["embeds"][0]["color"],
                expected_color,
                "wrong color for {severity}"
            );
        }
    }

    #[test]
    fn test_no_fields() {
        let discord = DiscordWebhook::new("https://example.com");
        let notif = Notification::new("hello", "world", Severity::Info);
        let payload = discord.build_payload(&notif);

        let fields = payload["embeds"][0]["fields"].as_array().unwrap();
        assert!(fields.is_empty());
    }
}

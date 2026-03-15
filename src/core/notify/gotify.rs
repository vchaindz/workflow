//! Gotify notification backend.

use super::error::NotifyError;
use super::message::{Notification, Severity};
use super::Notifier;

/// Sends notifications via Gotify (https://gotify.net).
///
/// Config URL format: `gotify://my-server.local?token=APP_TOKEN`
/// Examples:
///   - `gotify://gotify.example.com?token=AxxxxxxxxxxxxxxB`
///   - `gotify://gotify.local:8080?token=mytoken`
#[derive(Debug)]
pub struct Gotify {
    /// Server base URL (e.g. `https://gotify.example.com`)
    server_url: String,
    /// Application token for authentication
    app_token: String,
}

impl Gotify {
    pub fn new(server_url: impl Into<String>, app_token: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
            app_token: app_token.into(),
        }
    }

    /// Parse from a `gotify://server?token=APP_TOKEN` URL.
    pub fn from_url(url: &str) -> Result<Self, NotifyError> {
        let stripped = url
            .strip_prefix("gotify://")
            .ok_or_else(|| NotifyError::new("gotify", "URL must start with gotify://"))?;

        // Split off query string
        let (host, query) = match stripped.split_once('?') {
            Some((h, q)) => (h, Some(q)),
            None => (stripped, None),
        };

        if host.is_empty() {
            return Err(NotifyError::new("gotify", "missing server in URL"));
        }

        let server_url = format!("https://{host}");

        // Parse token from query
        let app_token = query
            .and_then(|q| {
                q.split('&').find_map(|param| {
                    let (key, value) = param.split_once('=')?;
                    if key == "token" {
                        Some(value.to_string())
                    } else {
                        None
                    }
                })
            })
            .ok_or_else(|| NotifyError::new("gotify", "missing token parameter in URL"))?;

        Ok(Self::new(server_url, app_token))
    }

    /// Map severity to Gotify priority level.
    fn priority(severity: &Severity) -> u8 {
        match severity {
            Severity::Failure => 8,  // high
            Severity::Warning => 5,  // normal
            Severity::Success => 2,  // low
            Severity::Info => 2,     // low
        }
    }

    /// Build the JSON payload for the Gotify /message endpoint.
    fn build_payload(&self, notification: &Notification) -> serde_json::Value {
        let mut message = notification.body.clone();

        if !notification.fields.is_empty() {
            let mut keys: Vec<&String> = notification.fields.keys().collect();
            keys.sort();
            if !message.is_empty() {
                message.push_str("\n\n");
            }
            for key in keys {
                message.push_str(&format!("{}: {}\n", key, notification.fields[key]));
            }
            // Remove trailing newline
            message.truncate(message.trim_end().len());
        }

        serde_json::json!({
            "title": notification.subject,
            "message": message,
            "priority": Self::priority(&notification.severity),
        })
    }
}

impl Notifier for Gotify {
    fn name(&self) -> &str {
        "gotify"
    }

    fn send(&self, notification: &Notification) -> Result<(), NotifyError> {
        let payload = self.build_payload(notification);
        let url = format!("{}/message", self.server_url);

        ureq::post(&url)
            .set("X-Gotify-Key", &self.app_token)
            .send_json(payload)
            .map_err(|e| NotifyError::new("gotify", format!("HTTP request failed: {e}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_mapping() {
        assert_eq!(Gotify::priority(&Severity::Failure), 8);
        assert_eq!(Gotify::priority(&Severity::Warning), 5);
        assert_eq!(Gotify::priority(&Severity::Success), 2);
        assert_eq!(Gotify::priority(&Severity::Info), 2);
    }

    #[test]
    fn test_from_url_basic() {
        let g = Gotify::from_url("gotify://gotify.example.com?token=AxxxxB").unwrap();
        assert_eq!(g.server_url, "https://gotify.example.com");
        assert_eq!(g.app_token, "AxxxxB");
    }

    #[test]
    fn test_from_url_with_port() {
        let g = Gotify::from_url("gotify://gotify.local:8080?token=mytoken").unwrap();
        assert_eq!(g.server_url, "https://gotify.local:8080");
        assert_eq!(g.app_token, "mytoken");
    }

    #[test]
    fn test_from_url_invalid_prefix() {
        let err = Gotify::from_url("http://gotify.example.com?token=x").unwrap_err();
        assert_eq!(err.service, "gotify");
        assert!(err.message.contains("gotify://"));
    }

    #[test]
    fn test_from_url_empty_host() {
        let err = Gotify::from_url("gotify://?token=x").unwrap_err();
        assert_eq!(err.service, "gotify");
        assert!(err.message.contains("missing server"));
    }

    #[test]
    fn test_from_url_missing_token() {
        let err = Gotify::from_url("gotify://gotify.example.com").unwrap_err();
        assert_eq!(err.service, "gotify");
        assert!(err.message.contains("missing token"));
    }

    #[test]
    fn test_build_payload_simple() {
        let g = Gotify::new("https://gotify.example.com", "token");
        let notif = Notification::new("Deploy done", "All steps passed", Severity::Success);
        let payload = g.build_payload(&notif);

        assert_eq!(payload["title"], "Deploy done");
        assert_eq!(payload["message"], "All steps passed");
        assert_eq!(payload["priority"], 2);
    }

    #[test]
    fn test_build_payload_failure() {
        let g = Gotify::new("https://gotify.example.com", "token");
        let notif = Notification::new("Build failed", "Step compile errored", Severity::Failure);
        let payload = g.build_payload(&notif);

        assert_eq!(payload["title"], "Build failed");
        assert_eq!(payload["message"], "Step compile errored");
        assert_eq!(payload["priority"], 8);
    }

    #[test]
    fn test_build_payload_with_fields() {
        let g = Gotify::new("https://gotify.example.com", "token");
        let notif = Notification::new("Deploy done", "Success", Severity::Success)
            .with_field("task", "infra/deploy")
            .with_field("duration", "12s");
        let payload = g.build_payload(&notif);

        assert_eq!(payload["title"], "Deploy done");
        assert_eq!(payload["message"], "Success\n\nduration: 12s\ntask: infra/deploy");
        assert_eq!(payload["priority"], 2);
    }

    #[test]
    fn test_build_payload_empty_body_with_fields() {
        let g = Gotify::new("https://gotify.example.com", "token");
        let notif = Notification::new("Alert", "", Severity::Info)
            .with_field("status", "ok");
        let payload = g.build_payload(&notif);

        assert_eq!(payload["message"], "status: ok");
        assert_eq!(payload["priority"], 2);
    }

    #[test]
    fn test_name() {
        let g = Gotify::new("https://gotify.example.com", "token");
        assert_eq!(g.name(), "gotify");
    }
}

//! ntfy notification backend.

use super::error::NotifyError;
use super::message::{Notification, Severity};
use super::Notifier;

/// Sends notifications via ntfy (https://ntfy.sh).
///
/// Config URL format: `ntfy://server/topic` or `ntfy://server/topic?token=tk_xxx`
/// Examples:
///   - `ntfy://ntfy.sh/my-topic`
///   - `ntfy://my-server.local/topic?token=tk_xxx`
#[derive(Debug)]
pub struct Ntfy {
    /// Full topic URL (e.g. `https://ntfy.sh/my-topic`)
    topic_url: String,
    /// Optional auth token
    auth_token: Option<String>,
}

impl Ntfy {
    pub fn new(topic_url: impl Into<String>, auth_token: Option<String>) -> Self {
        Self {
            topic_url: topic_url.into(),
            auth_token,
        }
    }

    /// Parse from a `ntfy://server/topic?token=tk_xxx` URL.
    pub fn from_url(url: &str) -> Result<Self, NotifyError> {
        let stripped = url
            .strip_prefix("ntfy://")
            .ok_or_else(|| NotifyError::new("ntfy", "URL must start with ntfy://"))?;

        // Split off query string
        let (path, query) = match stripped.split_once('?') {
            Some((p, q)) => (p, Some(q)),
            None => (stripped, None),
        };

        if path.is_empty() {
            return Err(NotifyError::new("ntfy", "missing server/topic in URL"));
        }

        // path is "server/topic" or "server:port/topic"
        let topic_url = format!("https://{path}");

        // Parse optional token from query
        let auth_token = query.and_then(|q| {
            q.split('&')
                .find_map(|param| {
                    let (key, value) = param.split_once('=')?;
                    if key == "token" {
                        Some(value.to_string())
                    } else {
                        None
                    }
                })
        });

        Ok(Self::new(topic_url, auth_token))
    }

    /// Map severity to ntfy priority level.
    fn priority(severity: &Severity) -> &'static str {
        match severity {
            Severity::Failure => "5",  // urgent
            Severity::Warning => "4",  // high
            Severity::Success => "3",  // default
            Severity::Info => "3",     // default
        }
    }

    /// Map severity to ntfy tag emoji.
    fn tags(severity: &Severity) -> &'static str {
        match severity {
            Severity::Success => "white_check_mark",
            Severity::Failure => "x",
            Severity::Warning => "warning",
            Severity::Info => "information_source",
        }
    }

    /// Build the message body including fields.
    fn build_body(&self, notification: &Notification) -> String {
        let mut body = notification.body.clone();

        if !notification.fields.is_empty() {
            let mut keys: Vec<&String> = notification.fields.keys().collect();
            keys.sort();
            if !body.is_empty() {
                body.push_str("\n\n");
            }
            for key in keys {
                body.push_str(&format!("{}: {}\n", key, notification.fields[key]));
            }
            // Remove trailing newline
            body.truncate(body.trim_end().len());
        }

        body
    }
}

impl Notifier for Ntfy {
    fn name(&self) -> &str {
        "ntfy"
    }

    fn send(&self, notification: &Notification) -> Result<(), NotifyError> {
        let body = self.build_body(notification);

        let mut req = ureq::post(&self.topic_url)
            .set("Title", &notification.subject)
            .set("Priority", Self::priority(&notification.severity))
            .set("Tags", Self::tags(&notification.severity));

        if let Some(ref token) = self.auth_token {
            req = req.set("Authorization", &format!("Bearer {token}"));
        }

        req.send_string(&body)
            .map_err(|e| NotifyError::new("ntfy", format!("HTTP request failed: {e}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_mapping() {
        assert_eq!(Ntfy::priority(&Severity::Failure), "5");
        assert_eq!(Ntfy::priority(&Severity::Warning), "4");
        assert_eq!(Ntfy::priority(&Severity::Success), "3");
        assert_eq!(Ntfy::priority(&Severity::Info), "3");
    }

    #[test]
    fn test_tags_mapping() {
        assert_eq!(Ntfy::tags(&Severity::Success), "white_check_mark");
        assert_eq!(Ntfy::tags(&Severity::Failure), "x");
        assert_eq!(Ntfy::tags(&Severity::Warning), "warning");
        assert_eq!(Ntfy::tags(&Severity::Info), "information_source");
    }

    #[test]
    fn test_from_url_basic() {
        let ntfy = Ntfy::from_url("ntfy://ntfy.sh/my-topic").unwrap();
        assert_eq!(ntfy.topic_url, "https://ntfy.sh/my-topic");
        assert!(ntfy.auth_token.is_none());
    }

    #[test]
    fn test_from_url_with_token() {
        let ntfy = Ntfy::from_url("ntfy://my-server.local/topic?token=tk_xxx").unwrap();
        assert_eq!(ntfy.topic_url, "https://my-server.local/topic");
        assert_eq!(ntfy.auth_token.as_deref(), Some("tk_xxx"));
    }

    #[test]
    fn test_from_url_with_port() {
        let ntfy = Ntfy::from_url("ntfy://my-server.local:8080/alerts?token=tk_abc").unwrap();
        assert_eq!(ntfy.topic_url, "https://my-server.local:8080/alerts");
        assert_eq!(ntfy.auth_token.as_deref(), Some("tk_abc"));
    }

    #[test]
    fn test_from_url_invalid_prefix() {
        let err = Ntfy::from_url("http://ntfy.sh/topic").unwrap_err();
        assert_eq!(err.service, "ntfy");
        assert!(err.message.contains("ntfy://"));
    }

    #[test]
    fn test_from_url_empty_path() {
        let err = Ntfy::from_url("ntfy://").unwrap_err();
        assert_eq!(err.service, "ntfy");
    }

    #[test]
    fn test_build_body_simple() {
        let ntfy = Ntfy::new("https://ntfy.sh/test", None);
        let notif = Notification::new("Deploy done", "All steps passed", Severity::Success);
        let body = ntfy.build_body(&notif);
        assert_eq!(body, "All steps passed");
    }

    #[test]
    fn test_build_body_with_fields() {
        let ntfy = Ntfy::new("https://ntfy.sh/test", None);
        let notif = Notification::new("Deploy done", "Success", Severity::Success)
            .with_field("task", "infra/deploy")
            .with_field("duration", "12s");
        let body = ntfy.build_body(&notif);
        assert_eq!(body, "Success\n\nduration: 12s\ntask: infra/deploy");
    }

    #[test]
    fn test_build_body_empty_body_with_fields() {
        let ntfy = Ntfy::new("https://ntfy.sh/test", None);
        let notif = Notification::new("Deploy done", "", Severity::Info)
            .with_field("status", "ok");
        let body = ntfy.build_body(&notif);
        assert_eq!(body, "status: ok");
    }

    #[test]
    fn test_name() {
        let ntfy = Ntfy::new("https://ntfy.sh/test", None);
        assert_eq!(ntfy.name(), "ntfy");
    }
}

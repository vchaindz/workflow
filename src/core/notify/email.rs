//! Email notification backend using lettre for SMTP delivery.

use super::error::NotifyError;
use super::message::{Notification, Severity};
use super::Notifier;

use lettre::message::{header::ContentType, Mailbox, MessageBuilder};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

/// Sends notifications via SMTP email using the lettre crate.
///
/// Config is parsed from a URL like:
///   `email://recipient@example.com?smtp=smtp.example.com&port=587&from=sender@example.com`
///
/// - The recipient is taken from the URL path (user@host).
/// - `smtp` query param: SMTP server hostname (required).
/// - `port` query param: SMTP port (default 587).
/// - `from` query param: sender address (default: recipient address).
/// - `tls` query param: "starttls" (default for port 587), "implicit" (default for port 465), or "none".
///
/// SMTP credentials are read from `SMTP_USERNAME` and `SMTP_PASSWORD` env vars.
#[derive(Debug)]
pub struct EmailNotifier {
    recipient: String,
    from: String,
    smtp_host: String,
    smtp_port: u16,
    tls_mode: TlsMode,
    username: Option<String>,
    password: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TlsMode {
    Starttls,
    Implicit,
    None,
}

impl EmailNotifier {
    /// Create an EmailNotifier from a URL string.
    ///
    /// Format: `email://recipient@host?smtp=smtp.example.com&port=587&from=sender@host&tls=starttls`
    pub fn new(url: &str) -> Result<Self, NotifyError> {
        let stripped = url
            .strip_prefix("email://")
            .ok_or_else(|| NotifyError::new("email", "URL must start with email://"))?;

        let (recipient, query_str) = match stripped.split_once('?') {
            Some((r, q)) => (r.to_string(), q),
            None => {
                return Err(NotifyError::new(
                    "email",
                    "URL must include query params (at least ?smtp=<host>)",
                ))
            }
        };

        if recipient.is_empty() || !recipient.contains('@') {
            return Err(NotifyError::new(
                "email",
                "recipient must be a valid email (user@host)",
            ));
        }

        let params: std::collections::HashMap<&str, &str> = query_str
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .collect();

        let smtp_host = params
            .get("smtp")
            .ok_or_else(|| NotifyError::new("email", "missing required 'smtp' query parameter"))?
            .to_string();

        let smtp_port: u16 = params
            .get("port")
            .unwrap_or(&"587")
            .parse()
            .map_err(|_| NotifyError::new("email", "invalid port number"))?;

        let from = params
            .get("from")
            .map(|s| s.to_string())
            .unwrap_or_else(|| recipient.clone());

        let tls_mode = match params.get("tls").copied() {
            Some("starttls") => TlsMode::Starttls,
            Some("implicit") => TlsMode::Implicit,
            Some("none") => TlsMode::None,
            Some(other) => {
                return Err(NotifyError::new(
                    "email",
                    format!("unknown tls mode: {other} (use starttls, implicit, or none)"),
                ))
            }
            None => {
                if smtp_port == 465 {
                    TlsMode::Implicit
                } else {
                    TlsMode::Starttls
                }
            }
        };

        let username = std::env::var("SMTP_USERNAME").ok();
        let password = std::env::var("SMTP_PASSWORD").ok();

        Ok(Self {
            recipient,
            from,
            smtp_host,
            smtp_port,
            tls_mode,
            username,
            password,
        })
    }

    /// Build the email message (without sending). Useful for testing.
    fn build_message(&self, notification: &Notification) -> Result<Message, NotifyError> {
        let severity_prefix = match notification.severity {
            Severity::Success => "[SUCCESS]",
            Severity::Failure => "[FAILURE]",
            Severity::Warning => "[WARNING]",
            Severity::Info => "[INFO]",
        };

        let subject = format!("{} {}", severity_prefix, notification.subject);

        let mut body = notification.body.clone();

        if !notification.fields.is_empty() {
            body.push_str("\n\n--- Details ---\n");
            let mut fields: Vec<_> = notification.fields.iter().collect();
            fields.sort_by_key(|(k, _)| (*k).clone());
            for (key, value) in fields {
                body.push_str(&format!("{}: {}\n", key, value));
            }
        }

        let from_mailbox: Mailbox = self
            .from
            .parse()
            .map_err(|e| NotifyError::new("email", format!("invalid from address: {e}")))?;

        let to_mailbox: Mailbox = self
            .recipient
            .parse()
            .map_err(|e| NotifyError::new("email", format!("invalid recipient address: {e}")))?;

        MessageBuilder::new()
            .from(from_mailbox)
            .to(to_mailbox)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body)
            .map_err(|e| NotifyError::new("email", format!("failed to build message: {e}")))
    }

    /// Build the SMTP transport (without connecting).
    fn build_transport(&self) -> Result<SmtpTransport, NotifyError> {
        let mut builder = match self.tls_mode {
            TlsMode::Starttls => SmtpTransport::starttls_relay(&self.smtp_host)
                .map_err(|e| NotifyError::new("email", format!("SMTP relay error: {e}")))?,
            TlsMode::Implicit => SmtpTransport::relay(&self.smtp_host)
                .map_err(|e| NotifyError::new("email", format!("SMTP relay error: {e}")))?,
            TlsMode::None => SmtpTransport::builder_dangerous(&self.smtp_host),
        };

        builder = builder.port(self.smtp_port);

        if let (Some(user), Some(pass)) = (&self.username, &self.password) {
            builder = builder.credentials(Credentials::new(user.clone(), pass.clone()));
        }

        Ok(builder.build())
    }
}

impl Notifier for EmailNotifier {
    fn name(&self) -> &str {
        "email"
    }

    fn send(&self, notification: &Notification) -> Result<(), NotifyError> {
        let message = self.build_message(notification)?;
        let transport = self.build_transport()?;

        transport
            .send(&message)
            .map_err(|e| NotifyError::new("email", format!("SMTP send failed: {e}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_url_basic() {
        let notifier =
            EmailNotifier::new("email://admin@example.com?smtp=smtp.example.com&port=587")
                .unwrap();
        assert_eq!(notifier.recipient, "admin@example.com");
        assert_eq!(notifier.from, "admin@example.com");
        assert_eq!(notifier.smtp_host, "smtp.example.com");
        assert_eq!(notifier.smtp_port, 587);
        assert_eq!(notifier.tls_mode, TlsMode::Starttls);
    }

    #[test]
    fn test_parse_url_with_from_and_tls() {
        let notifier = EmailNotifier::new(
            "email://admin@example.com?smtp=mail.example.com&port=465&from=alerts@example.com&tls=implicit",
        )
        .unwrap();
        assert_eq!(notifier.recipient, "admin@example.com");
        assert_eq!(notifier.from, "alerts@example.com");
        assert_eq!(notifier.smtp_host, "mail.example.com");
        assert_eq!(notifier.smtp_port, 465);
        assert_eq!(notifier.tls_mode, TlsMode::Implicit);
    }

    #[test]
    fn test_parse_url_port465_defaults_to_implicit() {
        let notifier =
            EmailNotifier::new("email://admin@example.com?smtp=mail.example.com&port=465")
                .unwrap();
        assert_eq!(notifier.tls_mode, TlsMode::Implicit);
    }

    #[test]
    fn test_parse_url_missing_smtp() {
        let err = EmailNotifier::new("email://admin@example.com?port=587").unwrap_err();
        assert_eq!(err.service, "email");
        assert!(err.message.contains("smtp"));
    }

    #[test]
    fn test_parse_url_invalid_scheme() {
        let err = EmailNotifier::new("slack://something").unwrap_err();
        assert_eq!(err.service, "email");
        assert!(err.message.contains("email://"));
    }

    #[test]
    fn test_parse_url_invalid_recipient() {
        let err = EmailNotifier::new("email://notanemail?smtp=smtp.example.com").unwrap_err();
        assert_eq!(err.service, "email");
        assert!(err.message.contains("recipient"));
    }

    /// Helper: get the full RFC822 message as a string for assertion.
    fn message_to_string(msg: &Message) -> String {
        String::from_utf8_lossy(&msg.formatted()).to_string()
    }

    #[test]
    fn test_build_message_success() {
        let notifier =
            EmailNotifier::new("email://admin@example.com?smtp=smtp.example.com&port=587")
                .unwrap();
        let notification =
            Notification::new("Deploy OK", "All steps passed", Severity::Success)
                .with_field("task", "infra/deploy")
                .with_field("duration", "12s");

        let message = notifier.build_message(&notification).unwrap();
        let raw = message_to_string(&message);

        // Check subject contains severity prefix
        assert!(raw.contains("[SUCCESS]"));
        assert!(raw.contains("Deploy OK"));

        // Check body contains fields sorted alphabetically
        assert!(raw.contains("All steps passed"));
        assert!(raw.contains("--- Details ---"));
        assert!(raw.contains("duration: 12s"));
        assert!(raw.contains("task: infra/deploy"));
        // "duration" comes before "task" alphabetically
        let dur_pos = raw.find("duration: 12s").unwrap();
        let task_pos = raw.find("task: infra/deploy").unwrap();
        assert!(dur_pos < task_pos, "fields should be sorted alphabetically");
    }

    #[test]
    fn test_build_message_failure_severity() {
        let notifier =
            EmailNotifier::new("email://admin@example.com?smtp=smtp.example.com&port=587")
                .unwrap();
        let notification = Notification::new("Build failed", "Exit code 1", Severity::Failure);

        let message = notifier.build_message(&notification).unwrap();
        let raw = message_to_string(&message);
        assert!(raw.contains("[FAILURE]"));
    }

    #[test]
    fn test_build_message_no_fields() {
        let notifier =
            EmailNotifier::new("email://admin@example.com?smtp=smtp.example.com&port=587")
                .unwrap();
        let notification = Notification::new("Test", "Body text", Severity::Info);

        let message = notifier.build_message(&notification).unwrap();
        let raw = message_to_string(&message);
        assert!(!raw.contains("--- Details ---"));
        assert!(raw.contains("Body text"));
    }

    #[test]
    fn test_all_severity_prefixes() {
        let notifier =
            EmailNotifier::new("email://admin@example.com?smtp=smtp.example.com&port=587")
                .unwrap();

        for (severity, prefix) in [
            (Severity::Success, "[SUCCESS]"),
            (Severity::Failure, "[FAILURE]"),
            (Severity::Warning, "[WARNING]"),
            (Severity::Info, "[INFO]"),
        ] {
            let notification = Notification::new("test", "", severity);
            let message = notifier.build_message(&notification).unwrap();
            let raw = message_to_string(&message);
            assert!(
                raw.contains(prefix),
                "expected {prefix} in subject for {severity}"
            );
        }
    }
}

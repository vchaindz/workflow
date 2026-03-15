//! Telegram Bot API notification backend.

use super::error::NotifyError;
use super::message::{Notification, Severity};
use super::Notifier;

/// Sends notifications to Telegram via the Bot API.
///
/// Config URL format: `telegram://BOT_TOKEN@CHAT_ID`
#[derive(Debug)]
pub struct TelegramBot {
    bot_token: String,
    chat_id: String,
}

impl TelegramBot {
    pub fn new(bot_token: impl Into<String>, chat_id: impl Into<String>) -> Self {
        Self {
            bot_token: bot_token.into(),
            chat_id: chat_id.into(),
        }
    }

    /// Parse a `telegram://BOT_TOKEN@CHAT_ID` URL into a TelegramBot.
    pub fn from_url(url: &str) -> Result<Self, NotifyError> {
        let rest = url
            .strip_prefix("telegram://")
            .ok_or_else(|| NotifyError::new("telegram", "URL must start with telegram://"))?;

        let (token, chat_id) = rest
            .rsplit_once('@')
            .ok_or_else(|| NotifyError::new("telegram", "URL must be telegram://BOT_TOKEN@CHAT_ID"))?;

        if token.is_empty() {
            return Err(NotifyError::new("telegram", "bot token cannot be empty"));
        }
        if chat_id.is_empty() {
            return Err(NotifyError::new("telegram", "chat_id cannot be empty"));
        }

        Ok(Self::new(token, chat_id))
    }

    /// Build the MarkdownV2-formatted message text.
    fn build_message(&self, notification: &Notification) -> String {
        let icon = match notification.severity {
            Severity::Success => "\u{2705}",  // ✅
            Severity::Failure => "\u{274C}",  // ❌
            Severity::Warning => "\u{26A0}\u{FE0F}", // ⚠️
            Severity::Info => "\u{2139}\u{FE0F}",    // ℹ️
        };

        let mut text = format!(
            "{} *{}*",
            icon,
            escape_markdown_v2(&notification.subject)
        );

        if !notification.body.is_empty() {
            text.push_str(&format!("\n{}", escape_markdown_v2(&notification.body)));
        }

        if !notification.fields.is_empty() {
            text.push('\n');
            let mut sorted_fields: Vec<(&String, &String)> = notification.fields.iter().collect();
            sorted_fields.sort_by_key(|(k, _)| (*k).clone());
            for (k, v) in sorted_fields {
                text.push_str(&format!(
                    "\n*{}:* `{}`",
                    escape_markdown_v2(k),
                    escape_markdown_v2_code(v)
                ));
            }
        }

        text
    }

    /// Build the JSON payload for the sendMessage API call.
    ///
    /// Uses MarkdownV2 rich formatting with bold subject, inline code for fields,
    /// and severity icons. Falls back to plain text if rich formatting fails.
    fn build_payload(&self, notification: &Notification) -> serde_json::Value {
        match self.build_rich_payload(notification) {
            Some(payload) => payload,
            None => self.build_plain_payload(notification),
        }
    }

    /// Build a MarkdownV2 rich payload. Returns None if construction fails.
    fn build_rich_payload(&self, notification: &Notification) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "chat_id": self.chat_id,
            "text": self.build_message(notification),
            "parse_mode": "MarkdownV2"
        }))
    }

    /// Build a plain-text fallback payload (no parse_mode).
    fn build_plain_payload(&self, notification: &Notification) -> serde_json::Value {
        let mut text = format!("{}", notification.subject);
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
            "chat_id": self.chat_id,
            "text": text
        })
    }

    /// The sendMessage API endpoint URL.
    fn api_url(&self) -> String {
        format!("https://api.telegram.org/bot{}/sendMessage", self.bot_token)
    }
}

/// Escape special characters for Telegram MarkdownV2 format.
/// See: https://core.telegram.org/bots/api#markdownv2-style
fn escape_markdown_v2(text: &str) -> String {
    let special_chars = [
        '_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!',
    ];
    let mut escaped = String::with_capacity(text.len() * 2);
    for ch in text.chars() {
        if special_chars.contains(&ch) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

/// Escape characters inside inline code blocks (only `` ` `` and `\` need escaping).
fn escape_markdown_v2_code(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len() * 2);
    for ch in text.chars() {
        if ch == '`' || ch == '\\' {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

impl Notifier for TelegramBot {
    fn name(&self) -> &str {
        "telegram"
    }

    fn send(&self, notification: &Notification) -> Result<(), NotifyError> {
        let payload = self.build_payload(notification);

        ureq::post(&self.api_url())
            .set("Content-Type", "application/json")
            .send_string(&payload.to_string())
            .map_err(|e| NotifyError::new("telegram", format!("HTTP request failed: {e}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_formatting_success() {
        let bot = TelegramBot::new("123:ABC", "456");
        let notif = Notification::new("Deploy succeeded", "All steps passed", Severity::Success)
            .with_field("task", "infra/deploy")
            .with_field("duration", "12s");

        let payload = bot.build_payload(&notif);

        assert_eq!(payload["chat_id"], "456");
        assert_eq!(payload["parse_mode"], "MarkdownV2");

        let text = payload["text"].as_str().unwrap();
        // Severity icon
        assert!(text.starts_with("\u{2705}"));
        // Subject in bold (escaped)
        assert!(text.contains("*Deploy succeeded*"));
        // Body present
        assert!(text.contains("All steps passed"));
        // Fields sorted alphabetically, key bold, value in code
        assert!(text.contains("*duration:* `12s`"));
        assert!(text.contains("*task:* `infra/deploy`"));
        // duration comes before task alphabetically
        let dur_pos = text.find("*duration:*").unwrap();
        let task_pos = text.find("*task:*").unwrap();
        assert!(dur_pos < task_pos);
    }

    #[test]
    fn test_message_formatting_failure() {
        let bot = TelegramBot::new("123:ABC", "456");
        let notif = Notification::new("Build failed", "Step compile exited 1", Severity::Failure);

        let payload = bot.build_payload(&notif);
        let text = payload["text"].as_str().unwrap();

        assert!(text.starts_with("\u{274C}"));
        assert!(text.contains("*Build failed*"));
    }

    #[test]
    fn test_severity_icons() {
        let bot = TelegramBot::new("tok", "cid");

        for (severity, icon) in [
            (Severity::Success, "\u{2705}"),
            (Severity::Failure, "\u{274C}"),
            (Severity::Warning, "\u{26A0}\u{FE0F}"),
            (Severity::Info, "\u{2139}\u{FE0F}"),
        ] {
            let notif = Notification::new("test", "", severity);
            let text = bot.build_message(&notif);
            assert!(text.starts_with(icon), "wrong icon for severity");
        }
    }

    #[test]
    fn test_markdown_v2_escaping() {
        assert_eq!(escape_markdown_v2("hello_world"), "hello\\_world");
        assert_eq!(escape_markdown_v2("a*b*c"), "a\\*b\\*c");
        assert_eq!(escape_markdown_v2("1.2.3"), "1\\.2\\.3");
        assert_eq!(escape_markdown_v2("no special"), "no special");
        assert_eq!(
            escape_markdown_v2("all: _*[]()~`>#+-=|{}.!"),
            "all: \\_\\*\\[\\]\\(\\)\\~\\`\\>\\#\\+\\-\\=\\|\\{\\}\\.\\!"
        );
    }

    #[test]
    fn test_markdown_v2_code_escaping() {
        assert_eq!(escape_markdown_v2_code("hello"), "hello");
        assert_eq!(escape_markdown_v2_code("a`b"), "a\\`b");
        assert_eq!(escape_markdown_v2_code("a\\b"), "a\\\\b");
    }

    #[test]
    fn test_from_url_valid() {
        let bot = TelegramBot::from_url("telegram://123:ABCdef@-100999").unwrap();
        assert_eq!(bot.bot_token, "123:ABCdef");
        assert_eq!(bot.chat_id, "-100999");
    }

    #[test]
    fn test_from_url_invalid() {
        assert!(TelegramBot::from_url("https://example.com").is_err());
        assert!(TelegramBot::from_url("telegram://noatsign").is_err());
        assert!(TelegramBot::from_url("telegram://@chatid").is_err());
        assert!(TelegramBot::from_url("telegram://token@").is_err());
    }

    #[test]
    fn test_no_fields_no_body() {
        let bot = TelegramBot::new("tok", "cid");
        let notif = Notification::new("hello", "", Severity::Info);
        let text = bot.build_message(&notif);
        // Should just be icon + bold subject, no trailing newlines for fields
        assert_eq!(text, "\u{2139}\u{FE0F} *hello*");
    }

    #[test]
    fn test_api_url() {
        let bot = TelegramBot::new("123:ABC", "456");
        assert_eq!(bot.api_url(), "https://api.telegram.org/bot123:ABC/sendMessage");
    }
}

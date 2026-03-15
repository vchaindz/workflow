//! URL-scheme resolver that parses notification config strings into Notifier trait objects.
//!
//! Supported schemes:
//!   - `slack://hooks.slack.com/services/T.../B.../xxx`
//!   - `discord://discord.com/api/webhooks/...`
//!   - `webhook://example.com/hook`
//!   - `teams://example.webhook.office.com/...`
//!   - `telegram://BOT_TOKEN@CHAT_ID`
//!   - `ntfy://ntfy.sh/my-topic` or `ntfy://server/topic?token=tk_xxx`
//!   - `gotify://server?token=APP_TOKEN`
//!   - `email://recipient@host?smtp=smtp.host&port=587`
//!
//! Environment variable references (`$VAR` or `${VAR}`) are expanded before parsing.

use super::error::NotifyError;
use super::Notifier;

/// Expand `$VAR` and `${VAR}` references in a string using environment variables.
fn expand_env_vars(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            // Check for ${VAR} syntax
            if chars.peek() == Some(&'{') {
                chars.next(); // consume '{'
                let var_name: String = chars.by_ref().take_while(|&c| c != '}').collect();
                if let Ok(val) = std::env::var(&var_name) {
                    result.push_str(&val);
                }
            } else {
                // $VAR syntax: collect alphanumeric + underscore via peek
                let mut var_name = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        var_name.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if var_name.is_empty() {
                    result.push('$');
                } else if let Ok(val) = std::env::var(&var_name) {
                    result.push_str(&val);
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Parse a notification URL string into a boxed `Notifier`.
///
/// Environment variable references (`$VAR`, `${VAR}`) are expanded before parsing.
pub fn resolve_notifier(url: &str) -> Result<Box<dyn Notifier>, NotifyError> {
    let expanded = expand_env_vars(url);

    if let Some(rest) = expanded.strip_prefix("slack://") {
        resolve_slack(rest)
    } else if let Some(rest) = expanded.strip_prefix("discord://") {
        resolve_discord(rest)
    } else if let Some(rest) = expanded.strip_prefix("webhook://") {
        resolve_webhook(rest)
    } else if expanded.starts_with("teams://") {
        resolve_teams(&expanded)
    } else if expanded.starts_with("telegram://") {
        resolve_telegram(&expanded)
    } else if expanded.starts_with("ntfy://") {
        resolve_ntfy(&expanded)
    } else if expanded.starts_with("gotify://") {
        resolve_gotify(&expanded)
    } else if expanded.starts_with("email://") {
        resolve_email(&expanded)
    } else {
        let scheme = expanded.split("://").next().unwrap_or("unknown");
        Err(NotifyError::new(
            "resolve",
            format!("unknown notification scheme: '{scheme}://'"),
        ))
    }
}

#[cfg(feature = "slack")]
fn resolve_slack(rest: &str) -> Result<Box<dyn Notifier>, NotifyError> {
    if rest.is_empty() {
        return Err(NotifyError::new("slack", "empty webhook URL"));
    }
    let webhook_url = format!("https://{rest}");
    Ok(Box::new(super::slack::SlackWebhook::new(webhook_url)))
}

#[cfg(not(feature = "slack"))]
fn resolve_slack(_rest: &str) -> Result<Box<dyn Notifier>, NotifyError> {
    Err(NotifyError::new(
        "slack",
        "slack feature is not enabled; recompile with --features slack",
    ))
}

#[cfg(feature = "discord")]
fn resolve_discord(rest: &str) -> Result<Box<dyn Notifier>, NotifyError> {
    if rest.is_empty() {
        return Err(NotifyError::new("discord", "empty webhook URL"));
    }
    let webhook_url = format!("https://{rest}");
    Ok(Box::new(super::discord::DiscordWebhook::new(webhook_url)))
}

#[cfg(not(feature = "discord"))]
fn resolve_discord(_rest: &str) -> Result<Box<dyn Notifier>, NotifyError> {
    Err(NotifyError::new(
        "discord",
        "discord feature is not enabled; recompile with --features discord",
    ))
}

#[cfg(feature = "webhook")]
fn resolve_webhook(rest: &str) -> Result<Box<dyn Notifier>, NotifyError> {
    if rest.is_empty() {
        return Err(NotifyError::new("webhook", "empty webhook URL"));
    }
    let webhook_url = format!("https://{rest}");
    Ok(Box::new(super::webhook::GenericWebhook::new(webhook_url)))
}

#[cfg(not(feature = "webhook"))]
fn resolve_webhook(_rest: &str) -> Result<Box<dyn Notifier>, NotifyError> {
    Err(NotifyError::new(
        "webhook",
        "webhook feature is not enabled; recompile with --features webhook",
    ))
}

#[cfg(feature = "msteams")]
fn resolve_teams(url: &str) -> Result<Box<dyn Notifier>, NotifyError> {
    let rest = url.strip_prefix("teams://").unwrap();
    if rest.is_empty() {
        return Err(NotifyError::new("msteams", "empty webhook URL"));
    }
    let webhook_url = format!("https://{rest}");
    Ok(Box::new(super::msteams::TeamsWebhook::new(webhook_url)))
}

#[cfg(not(feature = "msteams"))]
fn resolve_teams(_url: &str) -> Result<Box<dyn Notifier>, NotifyError> {
    Err(NotifyError::new(
        "msteams",
        "msteams feature is not enabled; recompile with --features msteams",
    ))
}

#[cfg(feature = "telegram")]
fn resolve_telegram(url: &str) -> Result<Box<dyn Notifier>, NotifyError> {
    Ok(Box::new(super::telegram::TelegramBot::from_url(url)?))
}

#[cfg(not(feature = "telegram"))]
fn resolve_telegram(_url: &str) -> Result<Box<dyn Notifier>, NotifyError> {
    Err(NotifyError::new(
        "telegram",
        "telegram feature is not enabled; recompile with --features telegram",
    ))
}

#[cfg(feature = "ntfy")]
fn resolve_ntfy(url: &str) -> Result<Box<dyn Notifier>, NotifyError> {
    Ok(Box::new(super::ntfy::Ntfy::from_url(url)?))
}

#[cfg(not(feature = "ntfy"))]
fn resolve_ntfy(_url: &str) -> Result<Box<dyn Notifier>, NotifyError> {
    Err(NotifyError::new(
        "ntfy",
        "ntfy feature is not enabled; recompile with --features ntfy",
    ))
}

#[cfg(feature = "gotify")]
fn resolve_gotify(url: &str) -> Result<Box<dyn Notifier>, NotifyError> {
    Ok(Box::new(super::gotify::Gotify::from_url(url)?))
}

#[cfg(not(feature = "gotify"))]
fn resolve_gotify(_url: &str) -> Result<Box<dyn Notifier>, NotifyError> {
    Err(NotifyError::new(
        "gotify",
        "gotify feature is not enabled; recompile with --features gotify",
    ))
}

#[cfg(feature = "email")]
fn resolve_email(url: &str) -> Result<Box<dyn Notifier>, NotifyError> {
    Ok(Box::new(super::email::EmailNotifier::new(url)?))
}

#[cfg(not(feature = "email"))]
fn resolve_email(_url: &str) -> Result<Box<dyn Notifier>, NotifyError> {
    Err(NotifyError::new(
        "email",
        "email feature is not enabled; recompile with --features email",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- env var expansion tests ---

    #[test]
    fn test_expand_env_vars_dollar_syntax() {
        std::env::set_var("TEST_RESOLVE_HOST", "hooks.slack.com");
        let result = expand_env_vars("slack://$TEST_RESOLVE_HOST/services/T/B/x");
        assert_eq!(result, "slack://hooks.slack.com/services/T/B/x");
        std::env::remove_var("TEST_RESOLVE_HOST");
    }

    #[test]
    fn test_expand_env_vars_brace_syntax() {
        std::env::set_var("TEST_RESOLVE_TOKEN", "tk_abc123");
        let result = expand_env_vars("ntfy://ntfy.sh/topic?token=${TEST_RESOLVE_TOKEN}");
        assert_eq!(result, "ntfy://ntfy.sh/topic?token=tk_abc123");
        std::env::remove_var("TEST_RESOLVE_TOKEN");
    }

    #[test]
    fn test_expand_env_vars_missing_var() {
        // Missing vars are silently removed (the var name is dropped, surrounding text kept)
        std::env::remove_var("TEST_RESOLVE_NONEXISTENT");
        let result = expand_env_vars("slack://$TEST_RESOLVE_NONEXISTENT/path");
        assert_eq!(result, "slack:///path");
    }

    #[test]
    fn test_expand_env_vars_no_vars() {
        let result = expand_env_vars("slack://hooks.slack.com/services/T/B/x");
        assert_eq!(result, "slack://hooks.slack.com/services/T/B/x");
    }

    #[test]
    fn test_expand_env_vars_bare_dollar() {
        let result = expand_env_vars("price is $");
        assert_eq!(result, "price is $");
    }

    // --- scheme resolution tests ---

    #[cfg(feature = "slack")]
    #[test]
    fn test_resolve_slack() {
        let notifier = resolve_notifier("slack://hooks.slack.com/services/T/B/x").unwrap();
        assert_eq!(notifier.name(), "slack");
    }

    #[cfg(feature = "discord")]
    #[test]
    fn test_resolve_discord() {
        let notifier =
            resolve_notifier("discord://discord.com/api/webhooks/123/abc").unwrap();
        assert_eq!(notifier.name(), "discord");
    }

    #[cfg(feature = "webhook")]
    #[test]
    fn test_resolve_webhook() {
        let notifier = resolve_notifier("webhook://example.com/hook").unwrap();
        assert_eq!(notifier.name(), "webhook");
    }

    #[cfg(feature = "msteams")]
    #[test]
    fn test_resolve_teams() {
        let notifier =
            resolve_notifier("teams://example.webhook.office.com/webhook/xxx").unwrap();
        assert_eq!(notifier.name(), "msteams");
    }

    #[cfg(feature = "telegram")]
    #[test]
    fn test_resolve_telegram() {
        let notifier = resolve_notifier("telegram://123:ABCdef@-100999").unwrap();
        assert_eq!(notifier.name(), "telegram");
    }

    #[cfg(feature = "ntfy")]
    #[test]
    fn test_resolve_ntfy() {
        let notifier = resolve_notifier("ntfy://ntfy.sh/my-topic").unwrap();
        assert_eq!(notifier.name(), "ntfy");
    }

    #[cfg(feature = "gotify")]
    #[test]
    fn test_resolve_gotify() {
        let notifier = resolve_notifier("gotify://gotify.local?token=abc123").unwrap();
        assert_eq!(notifier.name(), "gotify");
    }

    #[cfg(feature = "email")]
    #[test]
    fn test_resolve_email() {
        let notifier =
            resolve_notifier("email://user@example.com?smtp=smtp.example.com").unwrap();
        assert_eq!(notifier.name(), "email");
    }

    #[test]
    fn test_resolve_unknown_scheme() {
        let err = resolve_notifier("foobar://something").unwrap_err();
        assert_eq!(err.service, "resolve");
        assert!(err.message.contains("unknown notification scheme"));
        assert!(err.message.contains("foobar://"));
    }

    #[test]
    fn test_resolve_no_scheme() {
        let err = resolve_notifier("just-a-string").unwrap_err();
        assert_eq!(err.service, "resolve");
    }

    #[cfg(feature = "slack")]
    #[test]
    fn test_resolve_with_env_var() {
        std::env::set_var("TEST_RESOLVE_SLACK_URL", "hooks.slack.com/services/T/B/x");
        let notifier = resolve_notifier("slack://$TEST_RESOLVE_SLACK_URL").unwrap();
        assert_eq!(notifier.name(), "slack");
        std::env::remove_var("TEST_RESOLVE_SLACK_URL");
    }

    #[cfg(feature = "slack")]
    #[test]
    fn test_resolve_empty_after_scheme() {
        let err = resolve_notifier("slack://").unwrap_err();
        assert_eq!(err.service, "slack");
    }
}

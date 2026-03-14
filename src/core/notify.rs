use std::collections::HashMap;

/// Escape a string for safe inclusion in a JSON string value.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            _ => out.push(c),
        }
    }
    out
}

/// Resolve a notification command from a potentially scheme-prefixed string.
///
/// Supported schemes:
/// - `slack://WEBHOOK_URL` → curl POST to Slack with JSON text payload
/// - `webhook://URL` → curl POST with all vars as JSON object
/// - `email://ADDRESS` → printf + mail command
/// - No prefix → returned as-is (plain bash command)
pub fn resolve_notify_command(raw: &str, vars: &HashMap<String, String>) -> String {
    if let Some(url) = raw.strip_prefix("slack://") {
        let status = vars.get("status").map(|s| s.as_str()).unwrap_or("unknown");
        let wf_name = vars.get("workflow_name").map(|s| s.as_str()).unwrap_or("");
        let task_ref = vars.get("task_ref").map(|s| s.as_str()).unwrap_or("");
        let failed = vars.get("failed_steps").map(|s| s.as_str()).unwrap_or("");

        let text = if failed.is_empty() {
            format!("[{status}] {wf_name} ({task_ref})")
        } else {
            format!("[{status}] {wf_name} ({task_ref}) — failed steps: {failed}")
        };

        format!(
            "curl -sS -X POST -H 'Content-Type: application/json' -d '{{\"text\":\"{}\"}}' '{}'",
            json_escape(&text),
            json_escape(url),
        )
    } else if let Some(url) = raw.strip_prefix("webhook://") {
        // Build JSON object from all vars
        let pairs: Vec<String> = vars.iter()
            .map(|(k, v)| format!("\"{}\":\"{}\"", json_escape(k), json_escape(v)))
            .collect();
        let payload = format!("{{{}}}", pairs.join(","));

        format!(
            "curl -sS -X POST -H 'Content-Type: application/json' -d '{}' '{}'",
            payload,
            json_escape(url),
        )
    } else if let Some(address) = raw.strip_prefix("email://") {
        let task_ref = vars.get("task_ref").map(|s| s.as_str()).unwrap_or("");
        let status = vars.get("status").map(|s| s.as_str()).unwrap_or("unknown");
        let exit_code = vars.get("exit_code").map(|s| s.as_str()).unwrap_or("?");
        let failed = vars.get("failed_steps").map(|s| s.as_str()).unwrap_or("");
        let duration = vars.get("duration_ms").map(|s| s.as_str()).unwrap_or("?");

        let body = format!(
            "Task: {task_ref}\\nStatus: {status}\\nExit code: {exit_code}\\nFailed steps: {failed}\\nDuration: {duration}ms"
        );

        format!(
            "printf '{}' | mail -s 'Workflow {} {}' '{}'",
            body,
            json_escape(task_ref),
            status,
            json_escape(address),
        )
    } else {
        raw.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_vars() -> HashMap<String, String> {
        let mut v = HashMap::new();
        v.insert("status".to_string(), "failure".to_string());
        v.insert("workflow_name".to_string(), "deploy-prod".to_string());
        v.insert("task_ref".to_string(), "infra/deploy".to_string());
        v.insert("failed_steps".to_string(), "build,test".to_string());
        v.insert("exit_code".to_string(), "1".to_string());
        v.insert("duration_ms".to_string(), "5432".to_string());
        v
    }

    #[test]
    fn test_resolve_slack_scheme() {
        let vars = sample_vars();
        let result = resolve_notify_command("slack://https://hooks.slack.com/services/T/B/x", &vars);
        assert!(result.starts_with("curl -sS -X POST"));
        assert!(result.contains("Content-Type: application/json"));
        assert!(result.contains("hooks.slack.com"));
        assert!(result.contains("[failure]"));
        assert!(result.contains("deploy-prod"));
        assert!(result.contains("failed steps: build,test"));
    }

    #[test]
    fn test_resolve_webhook_scheme() {
        let vars = sample_vars();
        let result = resolve_notify_command("webhook://https://status.example.com/api", &vars);
        assert!(result.starts_with("curl -sS -X POST"));
        assert!(result.contains("status.example.com"));
        // Should contain all var keys as JSON
        assert!(result.contains("\"status\":\"failure\""));
        assert!(result.contains("\"exit_code\":\"1\""));
    }

    #[test]
    fn test_resolve_email_scheme() {
        let vars = sample_vars();
        let result = resolve_notify_command("email://ops@example.com", &vars);
        assert!(result.contains("mail -s"));
        assert!(result.contains("ops@example.com"));
        assert!(result.contains("infra/deploy"));
    }

    #[test]
    fn test_resolve_plain_passthrough() {
        let vars = sample_vars();
        let cmd = "echo 'workflow failed' | tee /tmp/notify.log";
        let result = resolve_notify_command(cmd, &vars);
        assert_eq!(result, cmd);
    }

    #[test]
    fn test_json_escape_special_chars() {
        assert_eq!(json_escape(r#"hello "world""#), r#"hello \"world\""#);
        assert_eq!(json_escape("line1\nline2"), "line1\\nline2");
        assert_eq!(json_escape("back\\slash"), "back\\\\slash");
        assert_eq!(json_escape("tab\there"), "tab\\there");
    }
}

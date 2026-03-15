use std::collections::HashMap;

/// A resolved notification command — either structured (safe from shell injection)
/// or a raw shell string for user-provided passthrough commands.
#[derive(Debug, Clone)]
pub enum NotifyCommand {
    /// Executes program with explicit args — no shell interpretation.
    Structured {
        program: String,
        args: Vec<String>,
    },
    /// Executes program with explicit args and writes body to stdin.
    StructuredWithStdin {
        program: String,
        args: Vec<String>,
        stdin_data: String,
    },
    /// Raw shell command passed to `bash -c` (only for user-provided passthrough).
    Shell(String),
}

/// Resolve a notification command from a potentially scheme-prefixed string.
///
/// Supported schemes:
/// - `slack://WEBHOOK_URL` → curl POST to Slack with JSON text payload
/// - `webhook://URL` → curl POST with all vars as JSON object
/// - `email://ADDRESS` → mail command with body on stdin
/// - No prefix → returned as-is (plain bash command)
pub fn resolve_notify_command(raw: &str, vars: &HashMap<String, String>) -> NotifyCommand {
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

        let payload = serde_json::json!({ "text": text }).to_string();

        NotifyCommand::Structured {
            program: "curl".to_string(),
            args: vec![
                "-sS".to_string(),
                "-X".to_string(),
                "POST".to_string(),
                "-H".to_string(),
                "Content-Type: application/json".to_string(),
                "-d".to_string(),
                payload,
                url.to_string(),
            ],
        }
    } else if let Some(url) = raw.strip_prefix("webhook://") {
        let payload = serde_json::json!(vars).to_string();

        NotifyCommand::Structured {
            program: "curl".to_string(),
            args: vec![
                "-sS".to_string(),
                "-X".to_string(),
                "POST".to_string(),
                "-H".to_string(),
                "Content-Type: application/json".to_string(),
                "-d".to_string(),
                payload,
                url.to_string(),
            ],
        }
    } else if let Some(address) = raw.strip_prefix("email://") {
        let task_ref = vars.get("task_ref").map(|s| s.as_str()).unwrap_or("");
        let status = vars.get("status").map(|s| s.as_str()).unwrap_or("unknown");
        let exit_code = vars.get("exit_code").map(|s| s.as_str()).unwrap_or("?");
        let failed = vars.get("failed_steps").map(|s| s.as_str()).unwrap_or("");
        let duration = vars.get("duration_ms").map(|s| s.as_str()).unwrap_or("?");

        let body = format!(
            "Task: {task_ref}\nStatus: {status}\nExit code: {exit_code}\nFailed steps: {failed}\nDuration: {duration}ms"
        );

        let subject = format!("Workflow {} {}", task_ref, status);

        NotifyCommand::StructuredWithStdin {
            program: "mail".to_string(),
            args: vec![
                "-s".to_string(),
                subject,
                address.to_string(),
            ],
            stdin_data: body,
        }
    } else {
        NotifyCommand::Shell(raw.to_string())
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
        match result {
            NotifyCommand::Structured { program, args } => {
                assert_eq!(program, "curl");
                assert!(args.contains(&"Content-Type: application/json".to_string()));
                assert!(args.contains(&"https://hooks.slack.com/services/T/B/x".to_string()));
                // Find the -d payload arg
                let d_idx = args.iter().position(|a| a == "-d").unwrap();
                let payload = &args[d_idx + 1];
                assert!(payload.contains("[failure]"));
                assert!(payload.contains("deploy-prod"));
                assert!(payload.contains("failed steps: build,test"));
            }
            _ => panic!("expected Structured variant"),
        }
    }

    #[test]
    fn test_resolve_webhook_scheme() {
        let vars = sample_vars();
        let result = resolve_notify_command("webhook://https://status.example.com/api", &vars);
        match result {
            NotifyCommand::Structured { program, args } => {
                assert_eq!(program, "curl");
                assert!(args.contains(&"https://status.example.com/api".to_string()));
                let d_idx = args.iter().position(|a| a == "-d").unwrap();
                let payload = &args[d_idx + 1];
                assert!(payload.contains("\"status\":\"failure\""));
                assert!(payload.contains("\"exit_code\":\"1\""));
            }
            _ => panic!("expected Structured variant"),
        }
    }

    #[test]
    fn test_resolve_email_scheme() {
        let vars = sample_vars();
        let result = resolve_notify_command("email://ops@example.com", &vars);
        match result {
            NotifyCommand::StructuredWithStdin { program, args, stdin_data } => {
                assert_eq!(program, "mail");
                assert!(args.contains(&"ops@example.com".to_string()));
                assert!(args.contains(&"-s".to_string()));
                assert!(stdin_data.contains("infra/deploy"));
                assert!(stdin_data.contains("Status: failure"));
            }
            _ => panic!("expected StructuredWithStdin variant"),
        }
    }

    #[test]
    fn test_resolve_plain_passthrough() {
        let vars = sample_vars();
        let cmd = "echo 'workflow failed' | tee /tmp/notify.log";
        let result = resolve_notify_command(cmd, &vars);
        match result {
            NotifyCommand::Shell(s) => assert_eq!(s, cmd),
            _ => panic!("expected Shell variant"),
        }
    }


    #[test]
    fn test_shell_injection_in_workflow_name() {
        let mut vars = sample_vars();
        // Attempt shell injection via single-quote breakout
        vars.insert("workflow_name".to_string(), "x';curl evil;echo '".to_string());

        let result = resolve_notify_command("slack://https://hooks.slack.com/services/T/B/x", &vars);
        match result {
            NotifyCommand::Structured { program: _, args } => {
                // The malicious name should appear as a literal string inside
                // the JSON payload arg, NOT as separate shell syntax
                let d_idx = args.iter().position(|a| a == "-d").unwrap();
                let payload = &args[d_idx + 1];
                // Must contain the literal single quotes as data, not shell breakout
                assert!(payload.contains("x';curl evil;echo '"));
                // The payload must be valid JSON
                let parsed: serde_json::Value = serde_json::from_str(payload).unwrap();
                assert!(parsed["text"].as_str().unwrap().contains("x';curl evil;echo '"));
            }
            _ => panic!("expected Structured variant"),
        }

        // Same test for webhook
        let result = resolve_notify_command("webhook://https://example.com/hook", &vars);
        match result {
            NotifyCommand::Structured { program: _, args } => {
                let d_idx = args.iter().position(|a| a == "-d").unwrap();
                let payload = &args[d_idx + 1];
                let parsed: serde_json::Value = serde_json::from_str(payload).unwrap();
                assert!(parsed["workflow_name"].as_str().unwrap().contains("x';curl evil;echo '"));
            }
            _ => panic!("expected Structured variant"),
        }
    }
}

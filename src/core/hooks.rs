use std::process::Command;

use crate::core::config::HooksConfig;

/// Run a hook command via `bash -c`. Failures are logged to stderr but never
/// propagate — hooks must not block workflow execution.
fn run_hook(label: &str, cmd: &str, env: &[(String, String)]) {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return;
    }
    let mut c = Command::new("bash");
    c.arg("-c").arg(trimmed);
    for (k, v) in env {
        c.env(k, v);
    }
    match c.status() {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!("hook {label} exited with {}", s.code().unwrap_or(-1)),
        Err(e) => eprintln!("hook {label} failed to spawn: {e}"),
    }
}

/// Invoke the configured `pre_run` hook with `WORKFLOW_TASK_REF` in env.
pub fn run_pre(hooks: &HooksConfig, task_ref: &str) {
    if let Some(cmd) = hooks.pre_run.as_ref() {
        let env = vec![("WORKFLOW_TASK_REF".to_string(), task_ref.to_string())];
        run_hook("pre_run", cmd, &env);
    }
}

/// Invoke the configured `post_run` hook with `WORKFLOW_TASK_REF` and
/// `WORKFLOW_EXIT_CODE` in env.
pub fn run_post(hooks: &HooksConfig, task_ref: &str, exit_code: i32) {
    if let Some(cmd) = hooks.post_run.as_ref() {
        let env = vec![
            ("WORKFLOW_TASK_REF".to_string(), task_ref.to_string()),
            ("WORKFLOW_EXIT_CODE".to_string(), exit_code.to_string()),
        ];
        run_hook("post_run", cmd, &env);
    }
}

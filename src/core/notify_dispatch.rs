use std::collections::HashMap;

use crate::core::models::{NotifyConfig, RunLog, StepStatus};
use crate::core::notify::{Notification, Notifier, RateLimiter, RateLimitConfig, RetryConfig, Severity};
use crate::core::notify::resolve::resolve_notifier;
use crate::core::notify::retry::send_with_retry;

/// Build a `Notification` from a completed workflow run.
pub fn build_notification(
    task_ref: &str,
    run_log: &RunLog,
    workflow_name: &str,
) -> Notification {
    let severity = if run_log.exit_code == 0 {
        Severity::Success
    } else {
        Severity::Failure
    };

    let failed: Vec<&str> = run_log.steps.iter()
        .filter(|s| s.status == StepStatus::Failed)
        .map(|s| s.id.as_str())
        .collect();

    let total_ms: u64 = run_log.steps.iter().map(|s| s.duration_ms).sum();

    let status_label = if run_log.exit_code == 0 { "succeeded" } else { "failed" };
    let subject = format!("[{}] {} {}", severity, workflow_name, status_label);

    let mut body = format!("Task: {}\nDuration: {}ms\nExit code: {}",
        task_ref, total_ms, run_log.exit_code);
    if !failed.is_empty() {
        body.push_str(&format!("\nFailed steps: {}", failed.join(", ")));
    }

    let mut notification = Notification::new(subject, body, severity)
        .with_field("task_ref", task_ref)
        .with_field("exit_code", run_log.exit_code.to_string())
        .with_field("workflow_name", workflow_name)
        .with_field("duration_ms", total_ms.to_string())
        .with_field("hostname", crate::core::db::current_hostname())
        .with_field("timestamp", run_log.started.to_rfc3339());

    if !failed.is_empty() {
        notification = notification.with_field("failed_steps", failed.join(","));
    }

    notification
}

/// Collect target URLs for a given severity from channels config.
fn targets_from_channels(channels: &[crate::core::models::NotifyChannel], severity: &Severity) -> Vec<String> {
    channels
        .iter()
        .filter(|ch| ch.on.contains(severity))
        .map(|ch| ch.target.clone())
        .collect()
}

/// Build notifiers from workflow and global notify config.
///
/// Workflow-level config takes precedence over global config.
/// Returns notifiers matching the current severity (success vs failure).
///
/// Target resolution order:
/// 1. If workflow has `channels`, use severity-based routing from channels
/// 2. Else if workflow has `on_failure`/`on_success`, use those
/// 3. Else if global has `channels`, use severity-based routing from channels
/// 4. Else fall back to global `on_failure`/`on_success`
pub fn build_notifiers_for_run(
    wf_notify: &NotifyConfig,
    global_notify: &NotifyConfig,
    success: bool,
    secret_env: &HashMap<String, String>,
) -> Vec<Box<dyn Notifier>> {
    let severity = if success { Severity::Success } else { Severity::Failure };
    let targets = resolve_targets(wf_notify, global_notify, &severity);

    let mut notifiers: Vec<Box<dyn Notifier>> = Vec::new();
    for url in &targets {
        match resolve_notifier(url, secret_env) {
            Ok(n) => notifiers.push(n),
            Err(e) => eprintln!("Warning: failed to resolve notifier '{}': {}", url, e),
        }
    }
    notifiers
}

/// Collect targets from a single NotifyConfig for a given severity.
///
/// Channels take precedence over on_failure/on_success within the same config.
fn collect_targets(notify: &NotifyConfig, severity: &Severity) -> Vec<String> {
    if !notify.channels.is_empty() {
        return targets_from_channels(&notify.channels, severity);
    }
    if matches!(severity, Severity::Success | Severity::Failure) {
        let legacy = if *severity == Severity::Success {
            &notify.on_success
        } else {
            &notify.on_failure
        };
        if !legacy.is_empty() {
            return legacy.clone();
        }
    }
    Vec::new()
}

/// Resolve target URLs from config for a given severity.
///
/// By default, workflow-level targets are **merged** with global targets
/// (duplicates removed by URL). If `notify_override: true` is set on the
/// workflow config, workflow targets fully replace global targets.
pub fn resolve_targets(
    wf_notify: &NotifyConfig,
    global_notify: &NotifyConfig,
    severity: &Severity,
) -> Vec<String> {
    let wf_targets = collect_targets(wf_notify, severity);

    // Override mode: workflow replaces global (old behavior)
    if wf_notify.notify_override {
        return wf_targets;
    }

    // Merge mode (default): combine workflow + global, dedup by URL
    let global_targets = collect_targets(global_notify, severity);

    if wf_targets.is_empty() {
        return global_targets;
    }
    if global_targets.is_empty() {
        return wf_targets;
    }

    // Merge: workflow targets first, then global targets not already present
    let mut merged = wf_targets;
    for t in global_targets {
        if !merged.contains(&t) {
            merged.push(t);
        }
    }
    merged
}

/// Resolve the effective retry config from workflow and global notify configs.
///
/// Workflow-level retry takes precedence over global. If neither is set, returns `None`
/// (no retry — single attempt only).
pub fn resolve_retry_config(
    wf_notify: &NotifyConfig,
    global_notify: &NotifyConfig,
) -> Option<RetryConfig> {
    wf_notify.retry.clone().or_else(|| global_notify.retry.clone())
}

/// Resolve rate limit configs by merging workflow and global settings.
///
/// Workflow-level rate_limit entries override global ones for the same service.
pub fn resolve_rate_limit_configs(
    wf_notify: &NotifyConfig,
    global_notify: &NotifyConfig,
) -> std::collections::HashMap<String, RateLimitConfig> {
    let mut configs = global_notify.rate_limit.clone();
    for (service, config) in &wf_notify.rate_limit {
        configs.insert(service.clone(), config.clone());
    }
    configs
}

/// Send notifications for a completed workflow run.
///
/// Notification errors are logged but never block workflow completion.
/// Retries are per-notifier: one failing service does not delay others.
/// Rate-limited notifications are dropped with a warning.
pub fn send_notifications(
    task_ref: &str,
    run_log: &RunLog,
    workflow_name: &str,
    wf_notify: &NotifyConfig,
    global_notify: &NotifyConfig,
    secret_env: &HashMap<String, String>,
) {
    let success = run_log.exit_code == 0;
    let notifiers = build_notifiers_for_run(wf_notify, global_notify, success, secret_env);
    if notifiers.is_empty() {
        return;
    }

    let notification = build_notification(task_ref, run_log, workflow_name);
    let retry_config = resolve_retry_config(wf_notify, global_notify);
    let rate_limit_configs = resolve_rate_limit_configs(wf_notify, global_notify);
    let rate_limiter = RateLimiter::with_configs(rate_limit_configs);

    match retry_config {
        Some(ref rc) => {
            // Per-notifier retry with exponential backoff
            for notifier in &notifiers {
                if !rate_limiter.check_and_record(notifier.name()) {
                    eprintln!("Rate limited: dropping notification for service '{}'", notifier.name());
                    continue;
                }
                if let Err(e) = send_with_retry(notifier.as_ref(), &notification, rc) {
                    eprintln!("Notification error: {}", e);
                }
            }
        }
        None => {
            // No retry configured — single attempt, with rate limiting
            for notifier in &notifiers {
                if !rate_limiter.check_and_record(notifier.name()) {
                    eprintln!("Rate limited: dropping notification for service '{}'", notifier.name());
                    continue;
                }
                if let Err(e) = notifier.send(&notification) {
                    eprintln!("Notification error: {}", e);
                }
            }
        }
    }
}

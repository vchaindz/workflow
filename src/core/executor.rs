use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read as IoRead};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use chrono::Utc;
use uuid::Uuid;
use wait_timeout::ChildExt;

use crate::core::detect;
use crate::core::models::{ExecutionEvent, ForEachSource, NotifyConfig, RunLog, Step, StepResult, StepStatus, Workflow};
use crate::core::notify::{Notification, Notifier, RateLimiter, RetryConfig, Severity};
use crate::core::notify::resolve::resolve_notifier;
use crate::core::notify::retry::send_with_retry;
use crate::core::parser::{resolve_env, compute_execution_levels};
use crate::core::template::expand_template;
use crate::error::Result;

/// Gracefully terminate a child process: SIGTERM first, wait grace period, then SIGKILL.
#[cfg(unix)]
fn graceful_kill(child: &mut std::process::Child, grace_secs: u64) {
    use std::time::{Duration, Instant};
    let pid = child.id() as i32;
    // Send SIGTERM
    let ret = unsafe { libc::kill(pid, libc::SIGTERM) };
    if ret != 0 {
        // Process already gone or invalid PID — just ensure cleanup
        let _ = child.kill();
        let _ = child.wait();
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(grace_secs);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return, // exited
            Ok(None) => {
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => break,
        }
    }
    // Grace period expired — force kill
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(not(unix))]
fn graceful_kill(child: &mut std::process::Child, _grace_secs: u64) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Classify a step failure from stderr and return an actionable hint.
fn classify_error(stderr: &str) -> Option<String> {
    let lower = stderr.to_lowercase();
    if lower.contains("permission denied") {
        Some("Hint: check file permissions or try with sudo".to_string())
    } else if lower.contains("command not found") || lower.contains("no such file or directory") {
        Some("Hint: command not found — check PATH or install the missing tool".to_string())
    } else if lower.contains("connection refused") {
        Some("Hint: connection refused — is the target service running?".to_string())
    } else if lower.contains("connection timed out") || lower.contains("operation timed out") {
        Some("Hint: connection timed out — check network/firewall".to_string())
    } else if lower.contains("disk full") || lower.contains("no space left on device") {
        Some("Hint: disk full — free up space".to_string())
    } else if lower.contains("authentication fail") || lower.contains("access denied") {
        Some("Hint: authentication failed — check credentials".to_string())
    } else {
        None
    }
}

/// Check if a command contains sudo and warn if sudo access is unavailable.
fn check_sudo_access(cmd: &str) -> Option<String> {
    if !cmd.split_whitespace().any(|w| w == "sudo" || w.ends_with("/sudo")) {
        return None;
    }
    // Test passwordless sudo
    let result = Command::new("sudo")
        .arg("-n")
        .arg("true")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match result {
        Ok(s) if s.success() => None,
        _ => Some("This step uses sudo but passwordless sudo is not available — you may be prompted for a password".to_string()),
    }
}

/// Check if a command matches known dangerous patterns.
/// Returns a warning message if dangerous, None if safe.
pub fn check_dangerous(cmd: &str) -> Option<&'static str> {
    let trimmed = cmd.trim();

    // Fork bomb
    if trimmed.contains(":(){ :|:& };:") || trimmed.contains(":(){:|:&};:") {
        return Some("Fork bomb detected — will crash the system");
    }

    // rm -rf / or rm -rf /* (but not rm -rf /tmp/something)
    let rm_re = regex::Regex::new(r"rm\s+(-[a-zA-Z]*f[a-zA-Z]*\s+(-[a-zA-Z]*r[a-zA-Z]*\s+)?|(-[a-zA-Z]*r[a-zA-Z]*\s+)?-[a-zA-Z]*f[a-zA-Z]*\s+)/\*?\s*$").unwrap();
    if rm_re.is_match(trimmed) {
        return Some("Recursive force-delete of root filesystem detected");
    }

    // dd writing to block devices
    if trimmed.contains("dd ") && trimmed.contains("of=/dev/sd") {
        return Some("Direct write to block device via dd detected");
    }
    if trimmed.contains("dd ") && trimmed.contains("of=/dev/nvme") {
        return Some("Direct write to block device via dd detected");
    }

    // mkfs on real devices (not loop or files)
    let mkfs_re = regex::Regex::new(r"mkfs[\.\s]\S*\s+/dev/sd").unwrap();
    if mkfs_re.is_match(trimmed) {
        return Some("Filesystem creation on real device detected");
    }

    // Redirect to block device
    let dev_redirect_re = regex::Regex::new(r">\s*/dev/sd[a-z]").unwrap();
    if dev_redirect_re.is_match(trimmed) {
        return Some("Output redirect to block device detected");
    }

    // chmod -R 777 /
    let chmod_re = regex::Regex::new(r"chmod\s+(-[a-zA-Z]*R[a-zA-Z]*\s+)?777\s+/\s*$").unwrap();
    if chmod_re.is_match(trimmed) {
        return Some("Recursive chmod 777 on root filesystem detected");
    }

    // mv /* /dev/null
    if trimmed.contains("mv") && trimmed.contains("/dev/null") && trimmed.contains("/*") {
        return Some("Moving filesystem contents to /dev/null detected");
    }

    None
}

/// Request from the executor to the TUI to suspend for an interactive step.
pub struct InteractiveRequest {
    pub step_id: String,
    pub ack: mpsc::Sender<()>,
}

/// Request from the executor to the TUI to show streaming output modal.
pub struct StreamingRequest {
    pub step_id: String,
    pub cmd_preview: String,
    /// Receiver for the child process's kill sender.
    /// TUI sends () to kill the child.
    pub kill_tx: mpsc::Sender<()>,
}

pub struct ExecuteOpts {
    pub dry_run: bool,
    pub force: bool,
    pub env_overrides: HashMap<String, String>,
    pub default_timeout: Option<u64>,
    pub secrets: Vec<String>,
    pub interactive_tx: Option<mpsc::Sender<InteractiveRequest>>,
    pub streaming_tx: Option<mpsc::Sender<StreamingRequest>>,
    pub workflows_dir: Option<std::path::PathBuf>,
    pub call_depth: u16,
    pub max_call_depth: u16,
    /// SSH private key path for decrypting secrets store
    pub secrets_ssh_key: Option<std::path::PathBuf>,
}

impl Default for ExecuteOpts {
    fn default() -> Self {
        Self {
            dry_run: false,
            force: false,
            env_overrides: HashMap::new(),
            default_timeout: None,
            secrets: Vec::new(),
            interactive_tx: None,
            streaming_tx: None,
            workflows_dir: None,
            call_depth: 0,
            max_call_depth: 10,
            secrets_ssh_key: None,
        }
    }
}


/// Replace secret values with [REDACTED] in text.
pub fn mask_secrets(text: &str, secret_values: &[String]) -> String {
    let mut result = text.to_string();
    for secret in secret_values {
        if !secret.is_empty() {
            result = result.replace(secret, "[REDACTED]");
        }
    }
    result
}

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
) -> std::collections::HashMap<String, crate::core::notify::RateLimitConfig> {
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
/// Load secret values from the encrypted secrets store.
///
/// Returns a map of secret names → values, suitable for passing to `send_notifications`
/// so that notification URL templates (e.g. `mattermost://$WEBHOOK_URL`) can be expanded.
pub fn load_secret_env(
    secret_names: &[String],
    workflows_dir: &std::path::Path,
    secrets_ssh_key: Option<&std::path::Path>,
) -> HashMap<String, String> {
    let mut env = HashMap::new();
    if secret_names.is_empty() {
        return env;
    }
    if let Some(ssh_key) = secrets_ssh_key {
        if workflows_dir.join("secrets.age").exists() {
            if let Ok(store) = crate::core::secrets::SecretsStore::load(workflows_dir, ssh_key) {
                for name in secret_names {
                    if let Some(val) = store.get(name) {
                        env.insert(name.clone(), val.to_string());
                    }
                }
            }
        }
    }
    env
}

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

/// Outcome of executing a single step
struct StepOutcome {
    result: StepResult,
    captured_vars: HashMap<String, String>,
    failed: bool,
}

/// Execute a single non-interactive step. Used by both sequential and parallel paths.
#[allow(clippy::too_many_arguments)]
fn execute_single_step(
    step: &Step,
    workdir: Option<&std::path::PathBuf>,
    template_vars: &HashMap<String, String>,
    env: &HashMap<String, String>,
    secret_values: &[String],
    dry_run: bool,
    force: bool,
    default_timeout: Option<u64>,
    event_tx: Option<&mpsc::Sender<ExecutionEvent>>,
    failed_steps: &std::collections::HashSet<String>,
    workflows_dir: Option<&std::path::PathBuf>,
    call_depth: u16,
    max_call_depth: u16,
    secret_names: &[String],
    secrets_ssh_key: Option<&std::path::PathBuf>,
) -> StepOutcome {
    let send = |evt: ExecutionEvent| {
        if let Some(tx) = event_tx {
            let _ = tx.send(evt);
        }
    };

    let mut captured_vars = HashMap::new();

    // Check if any dependency failed
    let dep_failed = step
        .needs
        .iter()
        .any(|dep| failed_steps.contains(dep.as_str()));

    if dep_failed {
        send(ExecutionEvent::StepSkipped { step_id: step.id.clone() });
        return StepOutcome {
            result: StepResult {
                id: step.id.clone(),
                status: StepStatus::Skipped,
                output: "skipped: dependency failed".to_string(),
                duration_ms: 0,
            },
            captured_vars,
            failed: true,
        };
    }

    // Conditional step: evaluate run_if condition (template-expanded)
    if let Some(ref condition) = step.run_if {
        let expanded_condition = expand_template(condition, template_vars);
        if dry_run {
            println!("[dry-run] step '{}' run_if: {}", step.id, expanded_condition);
        } else {
            let cond_result = Command::new("bash")
                .arg("-c")
                .arg(&expanded_condition)
                .envs(env)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            match cond_result {
                Ok(status) if !status.success() => {
                    send(ExecutionEvent::StepSkipped { step_id: step.id.clone() });
                    return StepOutcome {
                        result: StepResult {
                            id: step.id.clone(),
                            status: StepStatus::Skipped,
                            output: format!("condition not met: {expanded_condition}"),
                            duration_ms: 0,
                        },
                        captured_vars,
                        failed: false,
                    };
                }
                Err(e) => {
                    send(ExecutionEvent::StepSkipped { step_id: step.id.clone() });
                    return StepOutcome {
                        result: StepResult {
                            id: step.id.clone(),
                            status: StepStatus::Skipped,
                            output: format!("condition error: {e}"),
                            duration_ms: 0,
                        },
                        captured_vars,
                        failed: false,
                    };
                }
                _ => {} // condition met, proceed
            }
        }
    }

    // Conditional step: evaluate skip_if condition (inverse of run_if — skip when true)
    if let Some(ref condition) = step.skip_if {
        let expanded_condition = expand_template(condition, template_vars);
        if dry_run {
            println!("[dry-run] step '{}' skip_if: {}", step.id, expanded_condition);
        } else {
            let cond_result = Command::new("bash")
                .arg("-c")
                .arg(&expanded_condition)
                .envs(env)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            match cond_result {
                Ok(status) if status.success() => {
                    send(ExecutionEvent::StepSkipped { step_id: step.id.clone() });
                    return StepOutcome {
                        result: StepResult {
                            id: step.id.clone(),
                            status: StepStatus::Skipped,
                            output: format!("skip condition met: {expanded_condition}"),
                            duration_ms: 0,
                        },
                        captured_vars,
                        failed: false,
                    };
                }
                Err(e) => {
                    send(ExecutionEvent::StepSkipped { step_id: step.id.clone() });
                    return StepOutcome {
                        result: StepResult {
                            id: step.id.clone(),
                            status: StepStatus::Skipped,
                            output: format!("skip_if condition error: {e}"),
                            duration_ms: 0,
                        },
                        captured_vars,
                        failed: false,
                    };
                }
                _ => {} // skip condition not met, proceed
            }
        }
    }

    // Sub-workflow call
    if let Some(ref call_ref) = step.call {
        if call_depth >= max_call_depth {
            send(ExecutionEvent::StepCompleted {
                step_id: step.id.clone(),
                status: StepStatus::Failed,
                duration_ms: 0,
            });
            return StepOutcome {
                result: StepResult {
                    id: step.id.clone(),
                    status: StepStatus::Failed,
                    output: format!("sub-workflow call depth exceeded (max {})", max_call_depth),
                    duration_ms: 0,
                },
                captured_vars: HashMap::new(),
                failed: true,
            };
        }

        let Some(wf_dir) = workflows_dir else {
            send(ExecutionEvent::StepCompleted {
                step_id: step.id.clone(),
                status: StepStatus::Failed,
                duration_ms: 0,
            });
            return StepOutcome {
                result: StepResult {
                    id: step.id.clone(),
                    status: StepStatus::Failed,
                    output: "sub-workflow call requires workflows_dir".to_string(),
                    duration_ms: 0,
                },
                captured_vars: HashMap::new(),
                failed: true,
            };
        };

        if dry_run {
            send(ExecutionEvent::StepStarted {
                step_id: step.id.clone(),
                cmd_preview: format!("[dry-run] call: {call_ref}"),
            });
            println!("[dry-run] step '{}': call {}", step.id, call_ref);
            send(ExecutionEvent::StepCompleted {
                step_id: step.id.clone(),
                status: StepStatus::Success,
                duration_ms: 0,
            });
            return StepOutcome {
                result: StepResult {
                    id: step.id.clone(),
                    status: StepStatus::Success,
                    output: format!("[dry-run] call: {call_ref}"),
                    duration_ms: 0,
                },
                captured_vars: HashMap::new(),
                failed: false,
            };
        }

        send(ExecutionEvent::SubWorkflowStarted {
            parent_step_id: step.id.clone(),
            sub_task_ref: call_ref.clone(),
        });

        let timer = Instant::now();

        use crate::core::discovery::{scan_workflows, resolve_task_ref};
        use crate::core::parser::{parse_workflow, parse_shell_task};
        use crate::core::models::TaskKind;

        let sub_result = (|| -> crate::error::Result<RunLog> {
            let categories = scan_workflows(wf_dir)?;
            let task = resolve_task_ref(&categories, call_ref)?;
            let sub_wf = match task.kind {
                TaskKind::ShellScript => parse_shell_task(&task.path)?,
                TaskKind::YamlWorkflow => parse_workflow(&task.path)?,
            };

            // Build sub-workflow opts: inherit parent template_vars as env overrides
            let mut sub_env = template_vars.clone();
            for (k, v) in env.iter() {
                sub_env.insert(k.clone(), v.clone());
            }

            let sub_opts = ExecuteOpts {
                dry_run,
                force,
                env_overrides: sub_env,
                default_timeout,
                secrets: secret_names.to_vec(),
                interactive_tx: None,
                streaming_tx: None,
                workflows_dir: Some(wf_dir.to_path_buf()),
                call_depth: call_depth + 1,
                max_call_depth,
                secrets_ssh_key: secrets_ssh_key.map(|p| p.to_path_buf()),
            };

            execute_workflow(&sub_wf, call_ref, &sub_opts, event_tx)
        })();

        let duration_ms = timer.elapsed().as_millis() as u64;

        return match sub_result {
            Ok(run_log) => {
                let exit_code = run_log.exit_code;
                send(ExecutionEvent::SubWorkflowFinished {
                    parent_step_id: step.id.clone(),
                    sub_task_ref: call_ref.clone(),
                    exit_code,
                });

                let combined_output: String = run_log.steps.iter()
                    .map(|s| s.output.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");

                let mut captured = HashMap::new();
                for out_def in &step.outputs {
                    if let Ok(re) = regex::Regex::new(&out_def.pattern) {
                        if let Some(caps) = re.captures(&combined_output) {
                            if let Some(val) = caps.get(1) {
                                let key = format!("{}.{}", step.id, out_def.name);
                                captured.insert(key, val.as_str().to_string());
                            }
                        }
                    }
                }

                let failed = exit_code != 0;
                let status = if failed { StepStatus::Failed } else { StepStatus::Success };

                StepOutcome {
                    result: StepResult {
                        id: step.id.clone(),
                        status,
                        output: combined_output,
                        duration_ms,
                    },
                    captured_vars: captured,
                    failed,
                }
            }
            Err(e) => {
                send(ExecutionEvent::SubWorkflowFinished {
                    parent_step_id: step.id.clone(),
                    sub_task_ref: call_ref.clone(),
                    exit_code: 1,
                });
                StepOutcome {
                    result: StepResult {
                        id: step.id.clone(),
                        status: StepStatus::Failed,
                        output: format!("sub-workflow error: {e}"),
                        duration_ms,
                    },
                    captured_vars: HashMap::new(),
                    failed: true,
                }
            }
        };
    }

    // Sudo access warning
    if !dry_run {
        let expanded_for_check = expand_template(&step.cmd, template_vars);
        if let Some(sudo_warning) = check_sudo_access(&expanded_for_check) {
            send(ExecutionEvent::Warning {
                step_id: step.id.clone(),
                message: sudo_warning,
            });
        }
    }

    let expanded_cmd = expand_template(&step.cmd, template_vars);
    let masked_cmd = mask_secrets(&expanded_cmd, secret_values);

    // Dangerous command detection
    if !force && !dry_run {
        if let Some(warning) = check_dangerous(&expanded_cmd) {
            send(ExecutionEvent::DangerousCommand {
                step_id: step.id.clone(),
                warning: warning.to_string(),
            });
            if event_tx.is_none() {
                eprintln!("WARNING: {warning}");
                eprintln!("Blocked: dangerous command detected in step '{}'. Use --force to override.", step.id);
            }
            send(ExecutionEvent::StepCompleted {
                step_id: step.id.clone(),
                status: StepStatus::Failed,
                duration_ms: 0,
            });
            return StepOutcome {
                result: StepResult {
                    id: step.id.clone(),
                    status: StepStatus::Failed,
                    output: format!("Blocked: {warning}. Use --force to override."),
                    duration_ms: 0,
                },
                captured_vars,
                failed: true,
            };
        }
    }

    if dry_run {
        send(ExecutionEvent::StepStarted {
            step_id: step.id.clone(),
            cmd_preview: format!("[dry-run] {masked_cmd}"),
        });
        if let Some(dir) = workdir {
            println!("[dry-run] step '{}' (in {}): {}", step.id, dir.display(), masked_cmd);
        } else {
            println!("[dry-run] step '{}': {}", step.id, masked_cmd);
        }
        send(ExecutionEvent::StepCompleted {
            step_id: step.id.clone(),
            status: StepStatus::Success,
            duration_ms: 0,
        });
        return StepOutcome {
            result: StepResult {
                id: step.id.clone(),
                status: StepStatus::Success,
                output: format!("[dry-run] {masked_cmd}"),
                duration_ms: 0,
            },
            captured_vars,
            failed: false,
        };
    }

    send(ExecutionEvent::StepStarted {
        step_id: step.id.clone(),
        cmd_preview: mask_secrets(&truncate_cmd(&expanded_cmd, 60), secret_values),
    });

    let max_attempts = step.retry.unwrap_or(0) + 1;
    let retry_delay_secs = step.retry_delay.unwrap_or(0);
    let mut last_output = String::new();
    let mut _last_exit_code = 0i32;
    let mut step_succeeded = false;
    let mut step_timed_out = false;
    let timer = Instant::now();

    for attempt in 1..=max_attempts {
        if attempt > 1 {
            send(ExecutionEvent::StepRetrying {
                step_id: step.id.clone(),
                attempt,
                max: max_attempts,
                delay_secs: retry_delay_secs,
            });
            if retry_delay_secs > 0 {
                std::thread::sleep(Duration::from_secs(retry_delay_secs));
            }
        }

        let timeout_secs = step.timeout.or(default_timeout);

        let mut cmd = Command::new("bash");
        cmd.arg("-c").arg(&expanded_cmd).envs(env);
        if let Some(dir) = workdir {
            cmd.current_dir(dir);
        }

        if timeout_secs.is_some() {
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        }

        let result = if let Some(secs) = timeout_secs {
            match cmd.spawn() {
                Ok(mut child) => {
                    match child.wait_timeout(Duration::from_secs(secs)) {
                        Ok(Some(status)) => {
                            let mut stdout_buf = String::new();
                            let mut stderr_buf = String::new();
                            if let Some(mut out) = child.stdout.take() {
                                let _ = out.read_to_string(&mut stdout_buf);
                            }
                            if let Some(mut err) = child.stderr.take() {
                                let _ = err.read_to_string(&mut stderr_buf);
                            }
                            last_output = format!("{stdout_buf}{stderr_buf}");
                            if status.success() {
                                step_succeeded = true;
                            } else {
                                _last_exit_code = status.code().unwrap_or(1);
                            }
                            Ok(())
                        }
                        Ok(None) => {
                            graceful_kill(&mut child, 5);
                            last_output = format!("step timed out after {secs}s");
                            step_timed_out = true;
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                }
                Err(e) => Err(e),
            }
        } else {
            match cmd.output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    last_output = format!("{stdout}{stderr}");
                    if output.status.success() {
                        step_succeeded = true;
                    } else {
                        _last_exit_code = output.status.code().unwrap_or(1);
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            }
        };

        if let Err(e) = result {
            last_output = format!("failed to execute: {e}");
            _last_exit_code = 1;
        }

        if step_timed_out || step_succeeded {
            break;
        }
    }

    let duration_ms = timer.elapsed().as_millis() as u64;
    let masked_output = mask_secrets(&last_output, secret_values);

    if step_succeeded {
        // Capture step outputs as template variables
        if !step.outputs.is_empty() {
            for out_def in &step.outputs {
                if let Ok(re) = regex::Regex::new(&out_def.pattern) {
                    if let Some(caps) = re.captures(&last_output) {
                        if let Some(val) = caps.get(1) {
                            let key = format!("{}.{}", step.id, out_def.name);
                            captured_vars.insert(key, val.as_str().to_string());
                        }
                    }
                }
            }
        }

        send(ExecutionEvent::StepCompleted {
            step_id: step.id.clone(),
            status: StepStatus::Success,
            duration_ms,
        });
        StepOutcome {
            result: StepResult {
                id: step.id.clone(),
                status: StepStatus::Success,
                output: masked_output,
                duration_ms,
            },
            captured_vars,
            failed: false,
        }
    } else if step_timed_out {
        let secs = step.timeout.or(default_timeout).unwrap_or(0);
        send(ExecutionEvent::StepTimedOut {
            step_id: step.id.clone(),
            timeout_secs: secs,
            duration_ms,
        });
        StepOutcome {
            result: StepResult {
                id: step.id.clone(),
                status: StepStatus::Timedout,
                output: masked_output,
                duration_ms,
            },
            captured_vars,
            failed: true,
        }
    } else {
        let error_hint = classify_error(&masked_output);
        let output = if max_attempts > 1 {
            format!("{masked_output} (after {max_attempts} attempts)")
        } else {
            masked_output
        };
        send(ExecutionEvent::StepCompleted {
            step_id: step.id.clone(),
            status: StepStatus::Failed,
            duration_ms,
        });
        if let Some(hint) = error_hint {
            send(ExecutionEvent::Warning {
                step_id: step.id.clone(),
                message: hint,
            });
        }
        StepOutcome {
            result: StepResult {
                id: step.id.clone(),
                status: StepStatus::Failed,
                output,
                duration_ms,
            },
            captured_vars,
            failed: true,
        }
    }
}

/// Resolve the items for a for_each step. Returns None if the step has no for_each config.
fn resolve_for_each_items(
    step: &Step,
    template_vars: &HashMap<String, String>,
    env: &HashMap<String, String>,
    workdir: Option<&std::path::PathBuf>,
) -> Result<Option<Vec<String>>> {
    if let Some(ref source) = step.for_each {
        match source {
            ForEachSource::StaticList(items) => {
                // Expand templates in each static item
                let expanded: Vec<String> = items
                    .iter()
                    .map(|item| expand_template(item, template_vars))
                    .collect();
                Ok(Some(expanded))
            }
            ForEachSource::TemplateRef(tpl) => {
                let expanded = expand_template(tpl, template_vars);
                let items: Vec<String> = expanded
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                Ok(Some(items))
            }
        }
    } else if let Some(ref cmd_str) = step.for_each_cmd {
        let expanded_cmd = expand_template(cmd_str, template_vars);
        let mut cmd = Command::new("bash");
        cmd.arg("-c").arg(&expanded_cmd).envs(env);
        if let Some(dir) = workdir {
            cmd.current_dir(dir);
        }
        let output = cmd.output().map_err(|e| {
            crate::error::DzError::Execution(format!(
                "for_each_cmd failed for step '{}': {e}",
                step.id
            ))
        })?;
        if !output.status.success() {
            return Err(crate::error::DzError::Execution(format!(
                "for_each_cmd failed for step '{}': {}",
                step.id,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let items: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        Ok(Some(items))
    } else {
        Ok(None)
    }
}

/// Execute a step with for_each iteration. Returns outcomes for all iterations.
#[allow(clippy::too_many_arguments)]
fn execute_for_each_step(
    step: &Step,
    items: &[String],
    workdir: Option<&std::path::PathBuf>,
    template_vars: &HashMap<String, String>,
    env: &HashMap<String, String>,
    secret_values: &[String],
    dry_run: bool,
    force: bool,
    default_timeout: Option<u64>,
    event_tx: Option<&mpsc::Sender<ExecutionEvent>>,
    failed_steps: &std::collections::HashSet<String>,
    workflows_dir: Option<&std::path::PathBuf>,
    call_depth: u16,
    max_call_depth: u16,
    secret_names: &[String],
    secrets_ssh_key: Option<&std::path::PathBuf>,
) -> Vec<StepOutcome> {
    let send = |evt: ExecutionEvent| {
        if let Some(tx) = event_tx {
            let _ = tx.send(evt);
        }
    };

    let item_count = items.len();
    send(ExecutionEvent::ForEachStarted {
        step_id: step.id.clone(),
        item_count,
    });

    if step.for_each_parallel && !dry_run {
        // Parallel execution
        let vars_snapshot = template_vars.clone();
        let secrets_owned: Vec<String> = secret_values.to_vec();
        let mut handles = Vec::new();

        for (index, item) in items.iter().enumerate() {
            let mut iter_vars = vars_snapshot.clone();
            iter_vars.insert("item".to_string(), item.clone());
            iter_vars.insert("item_index".to_string(), index.to_string());
            iter_vars.insert("item_count".to_string(), item_count.to_string());

            let iter_step = Step {
                id: format!("{}[{}]", step.id, item),
                ..step.clone()
            };

            let env_clone = env.clone();
            let secrets_clone = secrets_owned.clone();
            let failed_clone = failed_steps.clone();
            let tx_clone = event_tx.cloned();
            let workdir_clone = workdir.cloned();
            let wf_dir_clone = workflows_dir.cloned();
            let item_clone = item.clone();
            let step_id = step.id.clone();
            let snames_clone = secret_names.to_vec();
            let sshkey_clone = secrets_ssh_key.map(|p| p.to_path_buf());

            handles.push(std::thread::spawn(move || {
                let outcome = execute_single_step(
                    &iter_step,
                    workdir_clone.as_ref(),
                    &iter_vars,
                    &env_clone,
                    &secrets_clone,
                    false,
                    force,
                    default_timeout,
                    tx_clone.as_ref(),
                    &failed_clone,
                    wf_dir_clone.as_ref(),
                    call_depth,
                    max_call_depth,
                    &snames_clone,
                    sshkey_clone.as_ref(),
                );
                if let Some(ref tx) = tx_clone {
                    let _ = tx.send(ExecutionEvent::ForEachIterationCompleted {
                        step_id,
                        item: item_clone,
                        index,
                        status: outcome.result.status.clone(),
                        duration_ms: outcome.result.duration_ms,
                    });
                }
                outcome
            }));
        }

        handles.into_iter().map(|h| h.join().unwrap()).collect()
    } else {
        // Sequential execution
        let mut outcomes = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let mut iter_vars = template_vars.clone();
            iter_vars.insert("item".to_string(), item.clone());
            iter_vars.insert("item_index".to_string(), index.to_string());
            iter_vars.insert("item_count".to_string(), item_count.to_string());

            let iter_step = Step {
                id: format!("{}[{}]", step.id, item),
                ..step.clone()
            };

            let outcome = execute_single_step(
                &iter_step,
                workdir,
                &iter_vars,
                env,
                secret_values,
                dry_run,
                force,
                default_timeout,
                event_tx,
                failed_steps,
                workflows_dir,
                call_depth,
                max_call_depth,
                secret_names,
                secrets_ssh_key,
            );

            send(ExecutionEvent::ForEachIterationCompleted {
                step_id: step.id.clone(),
                item: item.clone(),
                index,
                status: outcome.result.status.clone(),
                duration_ms: outcome.result.duration_ms,
            });

            let failed = outcome.failed;
            outcomes.push(outcome);

            if failed && !step.for_each_continue_on_error {
                break;
            }
        }
        outcomes
    }
}

pub fn execute_workflow(
    workflow: &Workflow,
    task_ref: &str,
    opts: &ExecuteOpts,
    event_tx: Option<&std::sync::mpsc::Sender<ExecutionEvent>>,
) -> Result<RunLog> {
    let send = |evt: ExecutionEvent| {
        if let Some(tx) = event_tx {
            let _ = tx.send(evt);
        }
    };

    let levels = compute_execution_levels(&workflow.steps)?;
    let started = Utc::now();
    let mut step_results: Vec<StepResult> = Vec::new();
    let mut failed_steps: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut overall_exit = 0i32;

    // Resolve dynamic env values at execution time (not during parsing)
    let resolved_env = resolve_env(&workflow.env)?;

    // Merge env: resolved workflow env + overrides
    let mut env: HashMap<String, String> = resolved_env;
    env.extend(opts.env_overrides.clone());

    // Expand {{var}} placeholders in env values (e.g. DZ_CONTAINER: "{{container}}")
    let snapshot: HashMap<String, String> = env.clone();
    env = env
        .into_iter()
        .map(|(k, v)| (k, expand_template(&v, &snapshot)))
        .collect();

    // Auto-inject secrets from encrypted store (don't overwrite explicit env)
    if !opts.secrets.is_empty() {
        if let Some(ref ssh_key) = opts.secrets_ssh_key {
            if let Some(ref wdir) = opts.workflows_dir {
                if wdir.join("secrets.age").exists() {
                    if let Ok(store) = crate::core::secrets::SecretsStore::load(wdir, ssh_key) {
                        for name in &opts.secrets {
                            if !env.contains_key(name) {
                                if let Some(val) = store.get(name) {
                                    env.insert(name.clone(), val.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Collect actual values of secret-named env vars for masking
    let secret_values: Vec<String> = opts
        .secrets
        .iter()
        .filter_map(|name| env.get(name).cloned())
        .filter(|v| !v.is_empty())
        .collect();

    // Expand templates in env values
    let mut template_vars: HashMap<String, String> = env.clone();

    let step_map: HashMap<&str, &Step> = workflow
        .steps
        .iter()
        .map(|s| (s.id.as_str(), s))
        .collect();

    for (level_idx, level) in levels.iter().enumerate() {
        if level.len() > 1 {
            send(ExecutionEvent::LevelStarted {
                level: level_idx,
                step_count: level.len(),
            });
        }

        // Partition: interactive steps run sequentially after normal steps
        let mut normal_ids: Vec<&str> = Vec::new();
        let mut interactive_ids: Vec<&str> = Vec::new();

        for step_id in level {
            let step = step_map[step_id.as_str()];
            let expanded_cmd = expand_template(&step.cmd, &template_vars);
            let is_interactive = match step.interactive {
                Some(v) => v,
                None => detect::is_interactive_command(&expanded_cmd),
            };
            if is_interactive {
                interactive_ids.push(step_id.as_str());
            } else {
                normal_ids.push(step_id.as_str());
            }
        }

        if normal_ids.len() <= 1 {
            // Single step or empty: run directly without thread overhead
            for &sid in &normal_ids {
                let step = step_map[sid];

                // Check for for_each iteration
                match resolve_for_each_items(step, &template_vars, &env, workflow.workdir.as_ref()) {
                    Ok(Some(items)) => {
                        let outcomes = execute_for_each_step(
                            step, &items, workflow.workdir.as_ref(), &template_vars, &env,
                            &secret_values, opts.dry_run, opts.force, opts.default_timeout,
                            event_tx, &failed_steps,
                            opts.workflows_dir.as_ref(), opts.call_depth, opts.max_call_depth,
                            &opts.secrets, opts.secrets_ssh_key.as_ref(),
                        );
                        let mut any_failed = false;
                        for outcome in outcomes {
                            if outcome.failed {
                                any_failed = true;
                                if overall_exit == 0 {
                                    overall_exit = if outcome.result.status == StepStatus::Timedout { 124 } else { 1 };
                                }
                            }
                            // Inject step status variable
                            let status_str = match outcome.result.status {
                                StepStatus::Success => "success",
                                StepStatus::Failed => "failed",
                                StepStatus::Skipped => "skipped",
                                StepStatus::Timedout => "timedout",
                                _ => "unknown",
                            };
                            template_vars.insert(format!("{}.status", outcome.result.id), status_str.to_string());
                            template_vars.extend(outcome.captured_vars);
                            step_results.push(outcome.result);
                        }
                        if any_failed {
                            failed_steps.insert(step.id.clone());
                        }
                    }
                    Ok(None) => {
                        let outcome = execute_single_step(
                            step, workflow.workdir.as_ref(), &template_vars, &env, &secret_values,
                            opts.dry_run, opts.force, opts.default_timeout,
                            event_tx, &failed_steps,
                            opts.workflows_dir.as_ref(), opts.call_depth, opts.max_call_depth,
                            &opts.secrets, opts.secrets_ssh_key.as_ref(),
                        );
                        if outcome.failed {
                            failed_steps.insert(step.id.clone());
                            if overall_exit == 0 {
                                overall_exit = if outcome.result.status == StepStatus::Timedout { 124 } else { 1 };
                            }
                        }
                        // Inject step status variable
                        let status_str = match outcome.result.status {
                            StepStatus::Success => "success",
                            StepStatus::Failed => "failed",
                            StepStatus::Skipped => "skipped",
                            StepStatus::Timedout => "timedout",
                            _ => "unknown",
                        };
                        template_vars.insert(format!("{}.status", outcome.result.id), status_str.to_string());
                        template_vars.extend(outcome.captured_vars);
                        step_results.push(outcome.result);
                    }
                    Err(e) => {
                        send(ExecutionEvent::Warning {
                            step_id: step.id.clone(),
                            message: format!("for_each resolution failed: {e}"),
                        });
                        failed_steps.insert(step.id.clone());
                        if overall_exit == 0 { overall_exit = 1; }
                        template_vars.insert(format!("{}.status", step.id), "failed".to_string());
                        step_results.push(StepResult {
                            id: step.id.clone(),
                            status: StepStatus::Failed,
                            output: format!("for_each resolution failed: {e}"),
                            duration_ms: 0,
                        });
                        send(ExecutionEvent::StepCompleted {
                            step_id: step.id.clone(),
                            status: StepStatus::Failed,
                            duration_ms: 0,
                        });
                    }
                }
            }
        } else {
            // Multiple steps: run in parallel threads
            let vars_snapshot = template_vars.clone();
            let failed_snapshot = failed_steps.clone();

            let mut handles = Vec::new();
            for &sid in &normal_ids {
                let step = step_map[sid].clone();
                let vars = vars_snapshot.clone();
                let env_clone = env.clone();
                let secrets_clone = secret_values.clone();
                let failed_clone = failed_snapshot.clone();
                let tx_clone = event_tx.cloned();
                let dry_run = opts.dry_run;
                let force = opts.force;
                let default_timeout = opts.default_timeout;
                let workdir = workflow.workdir.clone();
                let wf_dir_clone = opts.workflows_dir.clone();
                let call_depth = opts.call_depth;
                let max_call_depth = opts.max_call_depth;
                let snames_clone = opts.secrets.clone();
                let sshkey_clone = opts.secrets_ssh_key.clone();

                handles.push(std::thread::spawn(move || {
                    // Check for for_each iteration
                    match resolve_for_each_items(&step, &vars, &env_clone, workdir.as_ref()) {
                        Ok(Some(items)) => {
                            execute_for_each_step(
                                &step, &items, workdir.as_ref(), &vars, &env_clone,
                                &secrets_clone, dry_run, force, default_timeout,
                                tx_clone.as_ref(), &failed_clone,
                                wf_dir_clone.as_ref(), call_depth, max_call_depth,
                                &snames_clone, sshkey_clone.as_ref(),
                            )
                        }
                        Ok(None) => {
                            vec![execute_single_step(
                                &step, workdir.as_ref(), &vars, &env_clone, &secrets_clone,
                                dry_run, force, default_timeout,
                                tx_clone.as_ref(), &failed_clone,
                                wf_dir_clone.as_ref(), call_depth, max_call_depth,
                                &snames_clone, sshkey_clone.as_ref(),
                            )]
                        }
                        Err(e) => {
                            if let Some(ref tx) = tx_clone {
                                let _ = tx.send(ExecutionEvent::StepCompleted {
                                    step_id: step.id.clone(),
                                    status: StepStatus::Failed,
                                    duration_ms: 0,
                                });
                            }
                            vec![StepOutcome {
                                result: StepResult {
                                    id: step.id.clone(),
                                    status: StepStatus::Failed,
                                    output: format!("for_each resolution failed: {e}"),
                                    duration_ms: 0,
                                },
                                captured_vars: HashMap::new(),
                                failed: true,
                            }]
                        }
                    }
                }));
            }

            for handle in handles {
                let outcomes = handle.join().unwrap();
                for outcome in outcomes {
                    if outcome.failed {
                        failed_steps.insert(outcome.result.id.clone());
                        if overall_exit == 0 {
                            overall_exit = if outcome.result.status == StepStatus::Timedout { 124 } else { 1 };
                        }
                    }
                    // Inject step status variable
                    let status_str = match outcome.result.status {
                        StepStatus::Success => "success",
                        StepStatus::Failed => "failed",
                        StepStatus::Skipped => "skipped",
                        StepStatus::Timedout => "timedout",
                        _ => "unknown",
                    };
                    template_vars.insert(format!("{}.status", outcome.result.id), status_str.to_string());
                    template_vars.extend(outcome.captured_vars);
                    step_results.push(outcome.result);
                }
            }
        }

        // Interactive steps always run sequentially
        for &sid in &interactive_ids {
            let step = step_map[sid];
            let expanded_cmd = expand_template(&step.cmd, &template_vars);
            let _masked_cmd = mask_secrets(&expanded_cmd, &secret_values);

            let cmd_label = format!("(streaming) {}", mask_secrets(&truncate_cmd(&expanded_cmd, 50), &secret_values));
            send(ExecutionEvent::StepStarted {
                step_id: step.id.clone(),
                cmd_preview: cmd_label.clone(),
            });

            let timer = Instant::now();

            if let Some(ref stx) = opts.streaming_tx {
                let (kill_tx, kill_rx) = mpsc::channel::<()>();
                let _ = stx.send(StreamingRequest {
                    step_id: step.id.clone(),
                    cmd_preview: cmd_label,
                    kill_tx,
                });

                let mut cmd = Command::new("bash");
                cmd.arg("-c").arg(&expanded_cmd).envs(&env)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                if let Some(ref dir) = workflow.workdir {
                    cmd.current_dir(dir);
                }

                match cmd.spawn() {
                    Ok(mut child) => {
                        let stdout = child.stdout.take();
                        let stderr = child.stderr.take();
                        let step_id_clone = step.id.clone();
                        let tx_clone = event_tx.cloned();

                        let stdout_handle = stdout.map(|out| {
                            let sid = step_id_clone.clone();
                            let txc = tx_clone.clone();
                            let secrets_clone = secret_values.clone();
                            std::thread::spawn(move || {
                                let reader = BufReader::new(out);
                                for line in reader.lines() {
                                    match line {
                                        Ok(l) => {
                                            if let Some(ref tx) = txc {
                                                let l_masked = mask_secrets(&l, &secrets_clone);
                                                let _ = tx.send(ExecutionEvent::StepOutput {
                                                    step_id: sid.clone(),
                                                    line: l_masked,
                                                });
                                            }
                                        }
                                        Err(_) => break,
                                    }
                                }
                            })
                        });

                        let stderr_handle = stderr.map(|err| {
                            let sid = step.id.clone();
                            let txc = event_tx.cloned();
                            let secrets_clone = secret_values.clone();
                            std::thread::spawn(move || {
                                let reader = BufReader::new(err);
                                for line in reader.lines() {
                                    match line {
                                        Ok(l) => {
                                            if let Some(ref tx) = txc {
                                                let l_masked = mask_secrets(&l, &secrets_clone);
                                                let _ = tx.send(ExecutionEvent::StepOutput {
                                                    step_id: sid.clone(),
                                                    line: l_masked,
                                                });
                                            }
                                        }
                                        Err(_) => break,
                                    }
                                }
                            })
                        });

                        loop {
                            if let Ok(()) = kill_rx.try_recv() {
                                graceful_kill(&mut child, 5);
                                break;
                            }
                            match child.try_wait() {
                                Ok(Some(_)) => break,
                                Ok(None) => {
                                    std::thread::sleep(Duration::from_millis(50));
                                }
                                Err(_) => break,
                            }
                        }

                        if let Some(h) = stdout_handle { let _ = h.join(); }
                        if let Some(h) = stderr_handle { let _ = h.join(); }

                        let status = child.try_wait().ok().flatten();
                        let duration_ms = timer.elapsed().as_millis() as u64;
                        let (step_status, exit_code) = match status {
                            Some(s) if s.success() => (StepStatus::Success, 0),
                            Some(s) => (StepStatus::Failed, s.code().unwrap_or(1)),
                            None => (StepStatus::Success, 0),
                        };

                        if step_status == StepStatus::Failed {
                            overall_exit = exit_code;
                            failed_steps.insert(step.id.clone());
                        }

                        step_results.push(StepResult {
                            id: step.id.clone(),
                            status: step_status.clone(),
                            output: "(streaming session)".to_string(),
                            duration_ms,
                        });
                        send(ExecutionEvent::StepCompleted {
                            step_id: step.id.clone(),
                            status: step_status,
                            duration_ms,
                        });
                    }
                    Err(e) => {
                        let duration_ms = timer.elapsed().as_millis() as u64;
                        step_results.push(StepResult {
                            id: step.id.clone(),
                            status: StepStatus::Failed,
                            output: format!("failed to execute: {e}"),
                            duration_ms,
                        });
                        overall_exit = 1;
                        failed_steps.insert(step.id.clone());
                        send(ExecutionEvent::StepCompleted {
                            step_id: step.id.clone(),
                            status: StepStatus::Failed,
                            duration_ms,
                        });
                    }
                }
            } else if let Some(ref itx) = opts.interactive_tx {
                let (ack_tx, ack_rx) = mpsc::channel();
                let _ = itx.send(InteractiveRequest {
                    step_id: step.id.clone(),
                    ack: ack_tx,
                });
                let _ = ack_rx.recv();

                let mut cmd = Command::new("bash");
                cmd.arg("-c").arg(&expanded_cmd).envs(&env);
                if let Some(ref dir) = workflow.workdir {
                    cmd.current_dir(dir);
                }

                let status = cmd.status();
                let duration_ms = timer.elapsed().as_millis() as u64;

                let (step_status, exit_code) = match status {
                    Ok(s) if s.success() => (StepStatus::Success, 0),
                    Ok(s) => (StepStatus::Failed, s.code().unwrap_or(1)),
                    Err(e) => {
                        step_results.push(StepResult {
                            id: step.id.clone(),
                            status: StepStatus::Failed,
                            output: format!("failed to execute: {e}"),
                            duration_ms,
                        });
                        overall_exit = 1;
                        failed_steps.insert(step.id.clone());
                        send(ExecutionEvent::StepCompleted {
                            step_id: step.id.clone(),
                            status: StepStatus::Failed,
                            duration_ms,
                        });
                        continue;
                    }
                };

                if step_status == StepStatus::Failed {
                    overall_exit = exit_code;
                    failed_steps.insert(step.id.clone());
                }

                step_results.push(StepResult {
                    id: step.id.clone(),
                    status: step_status.clone(),
                    output: "(interactive session)".to_string(),
                    duration_ms,
                });
                send(ExecutionEvent::StepCompleted {
                    step_id: step.id.clone(),
                    status: step_status,
                    duration_ms,
                });
            } else {
                let mut cmd = Command::new("bash");
                cmd.arg("-c").arg(&expanded_cmd).envs(&env);
                if let Some(ref dir) = workflow.workdir {
                    cmd.current_dir(dir);
                }

                let status = cmd.status();
                let duration_ms = timer.elapsed().as_millis() as u64;
                let (step_status, _) = match status {
                    Ok(s) if s.success() => (StepStatus::Success, 0),
                    Ok(s) => (StepStatus::Failed, s.code().unwrap_or(1)),
                    Err(_) => (StepStatus::Failed, 1),
                };

                step_results.push(StepResult {
                    id: step.id.clone(),
                    status: step_status.clone(),
                    output: "(interactive session)".to_string(),
                    duration_ms,
                });
                send(ExecutionEvent::StepCompleted {
                    step_id: step.id.clone(),
                    status: step_status,
                    duration_ms,
                });
            }
        }
    }

    // Execute cleanup steps (run regardless of success/failure)
    for cleanup_step in &workflow.cleanup {
        let expanded_cmd = expand_template(&cleanup_step.cmd, &template_vars);
        let masked_cmd = mask_secrets(&expanded_cmd, &secret_values);

        send(ExecutionEvent::StepStarted {
            step_id: cleanup_step.id.clone(),
            cmd_preview: format!("(cleanup) {}", mask_secrets(&truncate_cmd(&expanded_cmd, 60), &secret_values)),
        });

        let timer = Instant::now();

        if opts.dry_run {
            println!("[dry-run] cleanup '{}': {}", cleanup_step.id, masked_cmd);
            step_results.push(StepResult {
                id: cleanup_step.id.clone(),
                status: StepStatus::Success,
                output: format!("[dry-run] {masked_cmd}"),
                duration_ms: 0,
            });
            send(ExecutionEvent::StepCompleted {
                step_id: cleanup_step.id.clone(),
                status: StepStatus::Success,
                duration_ms: 0,
            });
            continue;
        }

        let mut cmd = Command::new("bash");
        cmd.arg("-c").arg(&expanded_cmd).envs(&env);
        if let Some(ref dir) = workflow.workdir {
            cmd.current_dir(dir);
        }

        match cmd.output() {
            Ok(output) => {
                let duration_ms = timer.elapsed().as_millis() as u64;
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let combined = format!("{stdout}{stderr}");
                let status = if output.status.success() {
                    StepStatus::Success
                } else {
                    StepStatus::Failed
                };
                step_results.push(StepResult {
                    id: cleanup_step.id.clone(),
                    status: status.clone(),
                    output: mask_secrets(&combined, &secret_values),
                    duration_ms,
                });
                send(ExecutionEvent::StepCompleted {
                    step_id: cleanup_step.id.clone(),
                    status,
                    duration_ms,
                });
            }
            Err(e) => {
                let duration_ms = timer.elapsed().as_millis() as u64;
                step_results.push(StepResult {
                    id: cleanup_step.id.clone(),
                    status: StepStatus::Failed,
                    output: format!("cleanup failed: {e}"),
                    duration_ms,
                });
                send(ExecutionEvent::StepCompleted {
                    step_id: cleanup_step.id.clone(),
                    status: StepStatus::Failed,
                    duration_ms,
                });
            }
        }
        // Cleanup failures do NOT affect overall exit code
    }

    Ok(RunLog {
        id: Uuid::new_v4().to_string(),
        task_ref: task_ref.to_string(),
        started,
        ended: Some(Utc::now()),
        steps: step_results,
        exit_code: overall_exit,
    })
}

fn truncate_cmd(cmd: &str, max_len: usize) -> String {
    let first_line = cmd.lines().next().unwrap_or(cmd);
    if first_line.len() > max_len {
        format!("{}...", &first_line[..max_len])
    } else {
        first_line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{EnvValue, Step};

    fn echo_workflow() -> Workflow {
        Workflow {
            name: "test".to_string(),
            steps: vec![
                Step {
                    id: "s1".into(),
                    cmd: "echo hello".into(),
                    needs: vec![],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
                Step {
                    id: "s2".into(),
                    cmd: "echo world".into(),
                    needs: vec!["s1".into()],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
            ],
            env: HashMap::new(),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
        }
    }

    #[test]
    fn test_dry_run() {
        let wf = echo_workflow();
        let opts = ExecuteOpts {
            dry_run: true,
            ..Default::default()
        };
        let log = execute_workflow(&wf, "test/echo", &opts, None).unwrap();
        assert_eq!(log.exit_code, 0);
        assert_eq!(log.steps.len(), 2);
        assert!(log.steps[0].output.contains("[dry-run]"));
    }

    #[test]
    fn test_real_execution() {
        let wf = echo_workflow();
        let opts = ExecuteOpts::default();
        let log = execute_workflow(&wf, "test/echo", &opts, None).unwrap();
        assert_eq!(log.exit_code, 0);
        assert!(log.steps[0].output.contains("hello"));
        assert!(log.steps[1].output.contains("world"));
    }

    #[test]
    fn test_step_failure_skips_dependents() {
        let wf = Workflow {
            name: "fail-test".to_string(),
            steps: vec![
                Step {
                    id: "bad".into(),
                    cmd: "exit 1".into(),
                    needs: vec![],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
                Step {
                    id: "dependent".into(),
                    cmd: "echo should-not-run".into(),
                    needs: vec!["bad".into()],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
                Step {
                    id: "independent".into(),
                    cmd: "echo runs-fine".into(),
                    needs: vec![],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
            ],
            env: HashMap::new(),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
        };

        let log = execute_workflow(&wf, "test/fail", &ExecuteOpts::default(), None).unwrap();
        assert_ne!(log.exit_code, 0);

        let dep = log.steps.iter().find(|s| s.id == "dependent").unwrap();
        assert_eq!(dep.status, StepStatus::Skipped);

        let ind = log.steps.iter().find(|s| s.id == "independent").unwrap();
        assert_eq!(ind.status, StepStatus::Success);
    }

    #[test]
    fn test_env_override() {
        let wf = Workflow {
            name: "env-test".to_string(),
            steps: vec![Step {
                id: "s1".into(),
                cmd: "echo $MY_VAR".into(),
                needs: vec![],
                parallel: false,
                timeout: None,
                run_if: None,
                skip_if: None,
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            }],
            env: HashMap::from([("MY_VAR".to_string(), EnvValue::Static("original".to_string()))]),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
        };

        let opts = ExecuteOpts {
            env_overrides: HashMap::from([("MY_VAR".to_string(), "overridden".to_string())]),
            ..Default::default()
        };

        let log = execute_workflow(&wf, "test/env", &opts, None).unwrap();
        assert!(log.steps[0].output.contains("overridden"));
    }

    #[test]
    fn test_workdir() {
        let wf = Workflow {
            name: "workdir-test".to_string(),
            steps: vec![Step {
                id: "pwd".into(),
                cmd: "pwd".into(),
                needs: vec![],
                parallel: false,
                timeout: None,
                run_if: None,
                skip_if: None,
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            }],
            env: HashMap::new(),
            workdir: Some(std::path::PathBuf::from("/tmp")),
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
        };

        let log = execute_workflow(&wf, "test/workdir", &ExecuteOpts::default(), None).unwrap();
        assert_eq!(log.exit_code, 0);
        assert!(log.steps[0].output.trim().starts_with("/tmp"));
    }

    #[test]
    fn test_step_timeout() {
        let wf = Workflow {
            name: "timeout-test".to_string(),
            steps: vec![Step {
                id: "slow".into(),
                cmd: "sleep 10".into(),
                needs: vec![],
                parallel: false,
                timeout: Some(1),
                run_if: None,
                skip_if: None,
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            }],
            env: HashMap::new(),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
        };

        let log = execute_workflow(&wf, "test/timeout", &ExecuteOpts::default(), None).unwrap();
        assert_eq!(log.steps[0].status, StepStatus::Timedout);
        assert_eq!(log.exit_code, 124);
        assert!(log.steps[0].output.contains("timed out"));
    }

    #[test]
    fn test_default_timeout_override() {
        let wf = Workflow {
            name: "default-timeout-test".to_string(),
            steps: vec![Step {
                id: "slow".into(),
                cmd: "sleep 10".into(),
                needs: vec![],
                parallel: false,
                timeout: None,
                run_if: None,
                skip_if: None,
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            }],
            env: HashMap::new(),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
        };

        let opts = ExecuteOpts {
            default_timeout: Some(1),
            ..Default::default()
        };

        let log = execute_workflow(&wf, "test/default-timeout", &opts, None).unwrap();
        assert_eq!(log.steps[0].status, StepStatus::Timedout);
    }

    #[test]
    fn test_step_timeout_overrides_default() {
        // Step timeout of 1s should override the default of 999s
        let wf = Workflow {
            name: "override-test".to_string(),
            steps: vec![Step {
                id: "slow".into(),
                cmd: "sleep 10".into(),
                needs: vec![],
                parallel: false,
                timeout: Some(1),
                run_if: None,
                skip_if: None,
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            }],
            env: HashMap::new(),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
        };

        let opts = ExecuteOpts {
            default_timeout: Some(999),
            ..Default::default()
        };

        let log = execute_workflow(&wf, "test/override", &opts, None).unwrap();
        assert_eq!(log.steps[0].status, StepStatus::Timedout);
    }

    #[test]
    fn test_timeout_skips_dependents() {
        let wf = Workflow {
            name: "timeout-deps-test".to_string(),
            steps: vec![
                Step {
                    id: "slow".into(),
                    cmd: "sleep 10".into(),
                    needs: vec![],
                    parallel: false,
                    timeout: Some(1),
                    run_if: None,
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
                Step {
                    id: "dependent".into(),
                    cmd: "echo should-not-run".into(),
                    needs: vec!["slow".into()],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
            ],
            env: HashMap::new(),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
        };

        let log = execute_workflow(&wf, "test/timeout-deps", &ExecuteOpts::default(), None).unwrap();
        assert_eq!(log.steps[0].status, StepStatus::Timedout);
        assert_eq!(log.steps[1].status, StepStatus::Skipped);
    }

    #[test]
    fn test_run_if_condition_met() {
        let wf = Workflow {
            name: "run-if-true".to_string(),
            steps: vec![Step {
                id: "s1".into(),
                cmd: "echo ran".into(),
                needs: vec![],
                parallel: false,
                timeout: None,
                run_if: Some("true".to_string()),
                skip_if: None,
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            }],
            env: HashMap::new(),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
        };

        let log = execute_workflow(&wf, "test/run-if", &ExecuteOpts::default(), None).unwrap();
        assert_eq!(log.steps[0].status, StepStatus::Success);
        assert!(log.steps[0].output.contains("ran"));
    }

    #[test]
    fn test_run_if_condition_not_met() {
        let wf = Workflow {
            name: "run-if-false".to_string(),
            steps: vec![
                Step {
                    id: "maybe".into(),
                    cmd: "echo should-not-run".into(),
                    needs: vec![],
                    parallel: false,
                    timeout: None,
                    run_if: Some("false".to_string()),
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
                Step {
                    id: "after".into(),
                    cmd: "echo runs-anyway".into(),
                    needs: vec!["maybe".into()],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
            ],
            env: HashMap::new(),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
        };

        let log = execute_workflow(&wf, "test/run-if-false", &ExecuteOpts::default(), None).unwrap();
        assert_eq!(log.exit_code, 0);
        let maybe = log.steps.iter().find(|s| s.id == "maybe").unwrap();
        assert_eq!(maybe.status, StepStatus::Skipped);
        assert!(maybe.output.contains("condition not met"));
        // Dependent still runs because condition-skip doesn't cascade
        let after = log.steps.iter().find(|s| s.id == "after").unwrap();
        assert_eq!(after.status, StepStatus::Success);
    }

    #[test]
    fn test_retry_succeeds_eventually() {
        // Use a file counter to succeed on attempt 2
        let dir = tempfile::TempDir::new().unwrap();
        let counter = dir.path().join("counter");
        let cmd = format!(
            "if [ -f '{}' ]; then echo ok; else touch '{}'; exit 1; fi",
            counter.display(),
            counter.display()
        );
        let wf = Workflow {
            name: "retry-test".to_string(),
            steps: vec![Step {
                id: "flaky".into(),
                cmd,
                needs: vec![],
                parallel: false,
                timeout: None,
                run_if: None,
                skip_if: None,
                retry: Some(2),
                retry_delay: Some(0),
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            }],
            env: HashMap::new(),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
        };

        let log = execute_workflow(&wf, "test/retry", &ExecuteOpts::default(), None).unwrap();
        assert_eq!(log.exit_code, 0);
        assert_eq!(log.steps[0].status, StepStatus::Success);
    }

    #[test]
    fn test_retry_exhausted() {
        let wf = Workflow {
            name: "retry-fail".to_string(),
            steps: vec![Step {
                id: "always-fail".into(),
                cmd: "exit 1".into(),
                needs: vec![],
                parallel: false,
                timeout: None,
                run_if: None,
                skip_if: None,
                retry: Some(2),
                retry_delay: Some(0),
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            }],
            env: HashMap::new(),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
        };

        let log = execute_workflow(&wf, "test/retry-fail", &ExecuteOpts::default(), None).unwrap();
        assert_ne!(log.exit_code, 0);
        assert_eq!(log.steps[0].status, StepStatus::Failed);
        assert!(log.steps[0].output.contains("3 attempts"));
    }

    #[test]
    fn test_secret_masking() {
        let wf = Workflow {
            name: "secret-test".to_string(),
            steps: vec![Step {
                id: "s1".into(),
                cmd: "echo hunter2".into(),
                needs: vec![],
                parallel: false,
                timeout: None,
                run_if: None,
                skip_if: None,
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            }],
            env: HashMap::from([("MY_PASS".to_string(), EnvValue::Static("hunter2".to_string()))]),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
        };

        let opts = ExecuteOpts {
            secrets: vec!["MY_PASS".to_string()],
            ..Default::default()
        };

        let log = execute_workflow(&wf, "test/secret", &opts, None).unwrap();
        assert!(!log.steps[0].output.contains("hunter2"), "secret should be masked");
        assert!(log.steps[0].output.contains("[REDACTED]"), "should contain redacted marker");
    }

    #[test]
    fn test_mask_secrets_fn() {
        let secrets = vec!["s3cret".to_string(), "p@ssword".to_string()];
        let text = "connecting with s3cret and p@ssword";
        let masked = mask_secrets(text, &secrets);
        assert_eq!(masked, "connecting with [REDACTED] and [REDACTED]");
    }

    #[test]
    fn test_run_if_template_expansion() {
        // run_if with a template variable should be expanded before evaluation
        let wf = Workflow {
            name: "run_if_tpl".into(),
            steps: vec![Step {
                id: "s1".into(),
                cmd: "echo hello".into(),
                needs: vec![],
                parallel: false,
                timeout: None,
                run_if: Some("test '{{myvar}}' = 'go'".to_string()),
                skip_if: None,
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            }],
            env: HashMap::new(),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
        };
        // With myvar=go, step should run
        let opts = ExecuteOpts {
            env_overrides: HashMap::from([("myvar".to_string(), "go".to_string())]),
            ..Default::default()
        };
        let log = execute_workflow(&wf, "test/run_if_tpl", &opts, None).unwrap();
        assert_eq!(log.steps[0].status, StepStatus::Success);

        // With myvar=stop, step should be skipped
        let opts2 = ExecuteOpts {
            env_overrides: HashMap::from([("myvar".to_string(), "stop".to_string())]),
            ..Default::default()
        };
        let log2 = execute_workflow(&wf, "test/run_if_tpl", &opts2, None).unwrap();
        assert_eq!(log2.steps[0].status, StepStatus::Skipped);
    }

    #[test]
    fn test_step_status_var_success() {
        let wf = Workflow {
            name: "status_var".into(),
            steps: vec![
                Step {
                    id: "first".into(),
                    cmd: "echo ok".into(),
                    needs: vec![],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
                Step {
                    id: "check".into(),
                    cmd: "echo status={{first.status}}".into(),
                    needs: vec!["first".into()],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
            ],
            env: HashMap::new(),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
        };
        let log = execute_workflow(&wf, "test/status", &ExecuteOpts::default(), None).unwrap();
        assert_eq!(log.steps[1].status, StepStatus::Success);
        assert!(log.steps[1].output.contains("status=success"));
    }

    #[test]
    fn test_step_status_var_failure() {
        // Step 1 fails, step 2 is independent (same level), step 3 depends on step 2
        // and checks step 1's status via run_if template expansion.
        let wf = Workflow {
            name: "status_fail".into(),
            steps: vec![
                Step {
                    id: "failing".into(),
                    cmd: "exit 1".into(),
                    needs: vec![],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
                Step {
                    id: "independent".into(),
                    cmd: "echo ok".into(),
                    needs: vec![],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
                Step {
                    id: "rollback".into(),
                    cmd: "echo rolling-back".into(),
                    needs: vec!["independent".into()],
                    parallel: false,
                    timeout: None,
                    run_if: Some("test '{{failing.status}}' = 'failed'".to_string()),
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
            ],
            env: HashMap::new(),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
        };
        let log = execute_workflow(&wf, "test/status_fail", &ExecuteOpts::default(), None).unwrap();
        // Find steps by id since parallel level ordering may vary
        let failing = log.steps.iter().find(|s| s.id == "failing").unwrap();
        let rollback = log.steps.iter().find(|s| s.id == "rollback").unwrap();
        assert_eq!(failing.status, StepStatus::Failed);
        // rollback step should run because failing.status == "failed"
        assert_eq!(rollback.status, StepStatus::Success);
        assert!(rollback.output.contains("rolling-back"));
    }

    #[test]
    fn test_skip_if_true_skips() {
        let wf = Workflow {
            name: "skip_if_test".into(),
            steps: vec![Step {
                id: "s1".into(),
                cmd: "echo should-not-run".into(),
                needs: vec![],
                parallel: false,
                timeout: None,
                run_if: None,
                skip_if: Some("true".to_string()),
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            }],
            env: HashMap::new(),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
        };
        let log = execute_workflow(&wf, "test/skip_if", &ExecuteOpts::default(), None).unwrap();
        assert_eq!(log.steps[0].status, StepStatus::Skipped);
    }

    #[test]
    fn test_skip_if_false_runs() {
        let wf = Workflow {
            name: "skip_if_false".into(),
            steps: vec![Step {
                id: "s1".into(),
                cmd: "echo runs-fine".into(),
                needs: vec![],
                parallel: false,
                timeout: None,
                run_if: None,
                skip_if: Some("false".to_string()),
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            }],
            env: HashMap::new(),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
        };
        let log = execute_workflow(&wf, "test/skip_if_false", &ExecuteOpts::default(), None).unwrap();
        assert_eq!(log.steps[0].status, StepStatus::Success);
        assert!(log.steps[0].output.contains("runs-fine"));
    }

    #[test]
    fn test_build_notification_failure() {
        let run_log = RunLog {
            id: "test-123".into(),
            task_ref: "infra/deploy".into(),
            started: Utc::now(),
            ended: None,
            steps: vec![
                StepResult { id: "build".into(), status: StepStatus::Success, output: String::new(), duration_ms: 100 },
                StepResult { id: "deploy".into(), status: StepStatus::Failed, output: String::new(), duration_ms: 200 },
            ],
            exit_code: 1,
        };
        let notif = build_notification("infra/deploy", &run_log, "Deploy Prod");

        assert_eq!(notif.severity, Severity::Failure);
        assert!(notif.subject.contains("Deploy Prod"));
        assert!(notif.subject.contains("failed"));
        assert!(notif.body.contains("infra/deploy"));
        assert!(notif.body.contains("300ms"));
        assert!(notif.body.contains("deploy"));
        assert_eq!(notif.fields.get("task_ref").unwrap(), "infra/deploy");
        assert_eq!(notif.fields.get("exit_code").unwrap(), "1");
        assert_eq!(notif.fields.get("duration_ms").unwrap(), "300");
        assert!(notif.fields.contains_key("hostname"));
        assert!(notif.fields.contains_key("timestamp"));
        assert_eq!(notif.fields.get("failed_steps").unwrap(), "deploy");
    }

    #[test]
    fn test_build_notification_success() {
        let run_log = RunLog {
            id: "test-789".into(),
            task_ref: "ops/restart".into(),
            started: Utc::now(),
            ended: None,
            steps: vec![
                StepResult { id: "restart".into(), status: StepStatus::Success, output: String::new(), duration_ms: 50 },
            ],
            exit_code: 0,
        };
        let notif = build_notification("ops/restart", &run_log, "Restart");

        assert_eq!(notif.severity, Severity::Success);
        assert!(notif.subject.contains("succeeded"));
        assert!(!notif.body.contains("Failed steps"));
        assert!(!notif.fields.contains_key("failed_steps"));
    }

    #[test]
    fn test_build_notification_multiple_failures() {
        let run_log = RunLog {
            id: "test-456".into(),
            task_ref: "ci/build".into(),
            started: Utc::now(),
            ended: None,
            steps: vec![
                StepResult { id: "lint".into(), status: StepStatus::Failed, output: String::new(), duration_ms: 50 },
                StepResult { id: "test".into(), status: StepStatus::Failed, output: String::new(), duration_ms: 75 },
                StepResult { id: "build".into(), status: StepStatus::Success, output: String::new(), duration_ms: 100 },
            ],
            exit_code: 1,
        };
        let notif = build_notification("ci/build", &run_log, "CI Build");
        let failed = notif.fields.get("failed_steps").unwrap();
        assert!(failed.contains("lint"));
        assert!(failed.contains("test"));
        assert!(!failed.contains("build"));
    }

    #[test]
    fn test_build_notifiers_for_run_empty_config() {
        let wf_notify = NotifyConfig::default();
        let global_notify = NotifyConfig::default();
        let notifiers = build_notifiers_for_run(&wf_notify, &global_notify, true, &Default::default());
        assert!(notifiers.is_empty());
    }

    #[test]
    fn test_build_notifiers_for_run_invalid_url() {
        let wf_notify = NotifyConfig {
            on_failure: vec!["foobar://invalid".to_string()],
            ..Default::default()
        };
        let global_notify = NotifyConfig::default();
        // Invalid scheme logs warning but returns empty
        let notifiers = build_notifiers_for_run(&wf_notify, &global_notify, false, &Default::default());
        assert!(notifiers.is_empty());
    }

    #[test]
    fn test_build_notifiers_multi_target_failure() {
        // Two invalid URLs should both be attempted (and both fail gracefully)
        let wf_notify = NotifyConfig {
            on_failure: vec!["foobar://a".to_string(), "baz://b".to_string()],
            ..Default::default()
        };
        let global_notify = NotifyConfig::default();
        let notifiers = build_notifiers_for_run(&wf_notify, &global_notify, false, &Default::default());
        // Both are invalid schemes, so empty, but both were tried
        assert!(notifiers.is_empty());
    }

    #[test]
    fn test_build_notifiers_wf_merges_global_multi() {
        // Workflow-level targets merge with global by default
        let wf_notify = NotifyConfig {
            on_success: vec!["foobar://wf1".to_string()],
            ..Default::default()
        };
        let global_notify = NotifyConfig {
            on_success: vec!["foobar://global1".to_string(), "foobar://global2".to_string()],
            ..Default::default()
        };
        // All 3 targets are resolved (invalid schemes, so 0 valid notifiers, but merge logic works)
        let targets = resolve_targets(&wf_notify, &global_notify, &Severity::Success);
        assert_eq!(targets, vec!["foobar://wf1", "foobar://global1", "foobar://global2"]);
    }

    #[test]
    fn test_build_notifiers_falls_back_to_global_multi() {
        // Empty workflow targets → fall back to global
        let wf_notify = NotifyConfig::default();
        let global_notify = NotifyConfig {
            on_failure: vec!["foobar://g1".to_string(), "foobar://g2".to_string()],
            ..Default::default()
        };
        let notifiers = build_notifiers_for_run(&wf_notify, &global_notify, false, &Default::default());
        assert!(notifiers.is_empty()); // invalid schemes, but global was used
    }

    #[test]
    fn test_resolve_targets_channels_failure_only() {
        use crate::core::models::NotifyChannel;
        let wf_notify = NotifyConfig {
            channels: vec![
                NotifyChannel { target: "slack://fail".into(), on: vec![Severity::Failure] },
                NotifyChannel { target: "ntfy://all".into(), on: vec![Severity::Failure, Severity::Success] },
                NotifyChannel { target: "webhook://success-only".into(), on: vec![Severity::Success] },
            ],
            ..Default::default()
        };
        let global_notify = NotifyConfig::default();
        let targets = resolve_targets(&wf_notify, &global_notify, &Severity::Failure);
        assert_eq!(targets, vec!["slack://fail", "ntfy://all"]);
    }

    #[test]
    fn test_resolve_targets_channels_success_only() {
        use crate::core::models::NotifyChannel;
        let wf_notify = NotifyConfig {
            channels: vec![
                NotifyChannel { target: "slack://fail".into(), on: vec![Severity::Failure] },
                NotifyChannel { target: "ntfy://ok".into(), on: vec![Severity::Success] },
            ],
            ..Default::default()
        };
        let global_notify = NotifyConfig::default();
        let targets = resolve_targets(&wf_notify, &global_notify, &Severity::Success);
        assert_eq!(targets, vec!["ntfy://ok"]);
    }

    #[test]
    fn test_resolve_targets_channels_warning() {
        use crate::core::models::NotifyChannel;
        let wf_notify = NotifyConfig {
            channels: vec![
                NotifyChannel { target: "slack://warn".into(), on: vec![Severity::Warning, Severity::Failure] },
                NotifyChannel { target: "ntfy://info".into(), on: vec![Severity::Info] },
            ],
            ..Default::default()
        };
        let global_notify = NotifyConfig::default();
        let targets = resolve_targets(&wf_notify, &global_notify, &Severity::Warning);
        assert_eq!(targets, vec!["slack://warn"]);
    }

    #[test]
    fn test_resolve_targets_channels_override_legacy() {
        use crate::core::models::NotifyChannel;
        // When channels are present, on_failure/on_success are ignored
        let wf_notify = NotifyConfig {
            on_failure: vec!["slack://legacy".into()],
            channels: vec![
                NotifyChannel { target: "ntfy://channel".into(), on: vec![Severity::Failure] },
            ],
            ..Default::default()
        };
        let global_notify = NotifyConfig::default();
        let targets = resolve_targets(&wf_notify, &global_notify, &Severity::Failure);
        assert_eq!(targets, vec!["ntfy://channel"]);
    }

    #[test]
    fn test_resolve_targets_global_channels_fallback() {
        use crate::core::models::NotifyChannel;
        // No workflow config → fall back to global channels
        let wf_notify = NotifyConfig::default();
        let global_notify = NotifyConfig {
            channels: vec![
                NotifyChannel { target: "slack://global-fail".into(), on: vec![Severity::Failure] },
                NotifyChannel { target: "ntfy://global-ok".into(), on: vec![Severity::Success] },
            ],
            ..Default::default()
        };
        let targets = resolve_targets(&wf_notify, &global_notify, &Severity::Failure);
        assert_eq!(targets, vec!["slack://global-fail"]);
        let targets = resolve_targets(&wf_notify, &global_notify, &Severity::Success);
        assert_eq!(targets, vec!["ntfy://global-ok"]);
    }

    #[test]
    fn test_resolve_targets_legacy_still_works() {
        // No channels → legacy on_failure/on_success still works
        let wf_notify = NotifyConfig {
            on_failure: vec!["slack://wf-fail".into()],
            ..Default::default()
        };
        let global_notify = NotifyConfig::default();
        let targets = resolve_targets(&wf_notify, &global_notify, &Severity::Failure);
        assert_eq!(targets, vec!["slack://wf-fail"]);
        // Success has no workflow targets, no global → empty
        let targets = resolve_targets(&wf_notify, &global_notify, &Severity::Success);
        assert!(targets.is_empty());
    }

    #[test]
    fn test_resolve_targets_merge_dedup() {
        // Duplicate URLs across workflow and global are deduplicated
        let wf_notify = NotifyConfig {
            on_failure: vec!["slack://shared".into(), "ntfy://wf-only".into()],
            ..Default::default()
        };
        let global_notify = NotifyConfig {
            on_failure: vec!["slack://shared".into(), "webhook://global-only".into()],
            ..Default::default()
        };
        let targets = resolve_targets(&wf_notify, &global_notify, &Severity::Failure);
        assert_eq!(targets, vec!["slack://shared", "ntfy://wf-only", "webhook://global-only"]);
    }

    #[test]
    fn test_resolve_targets_override_replaces_global() {
        // With notify_override: true, workflow fully replaces global
        let wf_notify = NotifyConfig {
            on_failure: vec!["slack://wf".into()],
            notify_override: true,
            ..Default::default()
        };
        let global_notify = NotifyConfig {
            on_failure: vec!["ntfy://global".into()],
            ..Default::default()
        };
        let targets = resolve_targets(&wf_notify, &global_notify, &Severity::Failure);
        assert_eq!(targets, vec!["slack://wf"]);
    }

    #[test]
    fn test_resolve_targets_override_empty_wf_returns_empty() {
        // With notify_override: true and no workflow targets, global is NOT used
        let wf_notify = NotifyConfig {
            notify_override: true,
            ..Default::default()
        };
        let global_notify = NotifyConfig {
            on_failure: vec!["ntfy://global".into()],
            ..Default::default()
        };
        let targets = resolve_targets(&wf_notify, &global_notify, &Severity::Failure);
        assert!(targets.is_empty());
    }

    #[test]
    fn test_resolve_targets_merge_channels_with_global_legacy() {
        use crate::core::models::NotifyChannel;
        // Workflow channels + global legacy are merged
        let wf_notify = NotifyConfig {
            channels: vec![
                NotifyChannel { target: "slack://wf-ch".into(), on: vec![Severity::Failure] },
            ],
            ..Default::default()
        };
        let global_notify = NotifyConfig {
            on_failure: vec!["ntfy://global-legacy".into()],
            ..Default::default()
        };
        let targets = resolve_targets(&wf_notify, &global_notify, &Severity::Failure);
        assert_eq!(targets, vec!["slack://wf-ch", "ntfy://global-legacy"]);
    }

    #[test]
    fn test_resolve_targets_merge_channels_both_levels() {
        use crate::core::models::NotifyChannel;
        // Workflow channels + global channels are merged
        let wf_notify = NotifyConfig {
            channels: vec![
                NotifyChannel { target: "slack://wf".into(), on: vec![Severity::Failure] },
            ],
            ..Default::default()
        };
        let global_notify = NotifyConfig {
            channels: vec![
                NotifyChannel { target: "ntfy://global".into(), on: vec![Severity::Failure] },
                NotifyChannel { target: "slack://wf".into(), on: vec![Severity::Failure] }, // dup
            ],
            ..Default::default()
        };
        let targets = resolve_targets(&wf_notify, &global_notify, &Severity::Failure);
        assert_eq!(targets, vec!["slack://wf", "ntfy://global"]);
    }

    // --- Retry config resolution tests ---

    #[test]
    fn test_resolve_retry_config_none() {
        let wf = NotifyConfig::default();
        let global = NotifyConfig::default();
        assert!(resolve_retry_config(&wf, &global).is_none());
    }

    #[test]
    fn test_resolve_retry_config_global_only() {
        let wf = NotifyConfig::default();
        let global = NotifyConfig {
            retry: Some(RetryConfig {
                max_attempts: 5,
                initial_delay_ms: 500,
                backoff_factor: 1.5,
            }),
            ..Default::default()
        };
        let rc = resolve_retry_config(&wf, &global).unwrap();
        assert_eq!(rc.max_attempts, 5);
        assert_eq!(rc.initial_delay_ms, 500);
        assert_eq!(rc.backoff_factor, 1.5);
    }

    #[test]
    fn test_resolve_retry_config_workflow_overrides_global() {
        let wf = NotifyConfig {
            retry: Some(RetryConfig {
                max_attempts: 2,
                initial_delay_ms: 100,
                backoff_factor: 3.0,
            }),
            ..Default::default()
        };
        let global = NotifyConfig {
            retry: Some(RetryConfig {
                max_attempts: 5,
                initial_delay_ms: 500,
                backoff_factor: 1.5,
            }),
            ..Default::default()
        };
        let rc = resolve_retry_config(&wf, &global).unwrap();
        assert_eq!(rc.max_attempts, 2);
        assert_eq!(rc.initial_delay_ms, 100);
        assert_eq!(rc.backoff_factor, 3.0);
    }

    #[test]
    fn test_resolve_retry_config_workflow_only() {
        let wf = NotifyConfig {
            retry: Some(RetryConfig::default()),
            ..Default::default()
        };
        let global = NotifyConfig::default();
        let rc = resolve_retry_config(&wf, &global).unwrap();
        assert_eq!(rc.max_attempts, 3);
    }

    #[test]
    fn test_resolve_rate_limit_configs_empty() {
        let wf = NotifyConfig::default();
        let global = NotifyConfig::default();
        let configs = resolve_rate_limit_configs(&wf, &global);
        assert!(configs.is_empty());
    }

    #[test]
    fn test_resolve_rate_limit_configs_global_only() {
        let wf = NotifyConfig::default();
        let mut global = NotifyConfig::default();
        global.rate_limit.insert(
            "slack".to_string(),
            crate::core::notify::RateLimitConfig {
                max_per_window: 10,
                window_secs: 60,
            },
        );
        let configs = resolve_rate_limit_configs(&wf, &global);
        assert_eq!(configs.len(), 1);
        assert_eq!(configs["slack"].max_per_window, 10);
    }

    #[test]
    fn test_resolve_rate_limit_configs_workflow_overrides_global() {
        let mut wf = NotifyConfig::default();
        wf.rate_limit.insert(
            "slack".to_string(),
            crate::core::notify::RateLimitConfig {
                max_per_window: 5,
                window_secs: 30,
            },
        );
        let mut global = NotifyConfig::default();
        global.rate_limit.insert(
            "slack".to_string(),
            crate::core::notify::RateLimitConfig {
                max_per_window: 100,
                window_secs: 60,
            },
        );
        let configs = resolve_rate_limit_configs(&wf, &global);
        assert_eq!(configs["slack"].max_per_window, 5);
        assert_eq!(configs["slack"].window_secs, 30);
    }

    #[test]
    fn test_resolve_rate_limit_configs_merge() {
        let mut wf = NotifyConfig::default();
        wf.rate_limit.insert(
            "slack".to_string(),
            crate::core::notify::RateLimitConfig {
                max_per_window: 5,
                window_secs: 30,
            },
        );
        let mut global = NotifyConfig::default();
        global.rate_limit.insert(
            "discord".to_string(),
            crate::core::notify::RateLimitConfig {
                max_per_window: 20,
                window_secs: 60,
            },
        );
        let configs = resolve_rate_limit_configs(&wf, &global);
        assert_eq!(configs.len(), 2);
        assert_eq!(configs["slack"].max_per_window, 5);
        assert_eq!(configs["discord"].max_per_window, 20);
    }
}

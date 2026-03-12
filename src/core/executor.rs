use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read as IoRead};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use chrono::Utc;
use uuid::Uuid;
use wait_timeout::ChildExt;

use crate::core::detect;
use crate::core::models::{ExecutionEvent, RunLog, Step, StepResult, StepStatus, Workflow};
use crate::core::parser::{resolve_env, topological_sort};
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

/// Run a notification command after workflow completion.
/// Expands template variables in the command string, then runs via bash fire-and-forget.
pub fn run_notify(cmd: &str, vars: &HashMap<String, String>) {
    let expanded = expand_template(cmd, vars);
    let _ = Command::new("bash")
        .arg("-c")
        .arg(&expanded)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
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

    let order = topological_sort(&workflow.steps)?;
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

    for step_id in &order {
        let step = step_map[step_id.as_str()];

        // Check if any dependency failed
        let dep_failed = step
            .needs
            .iter()
            .any(|dep| failed_steps.contains(dep.as_str()));

        if dep_failed {
            send(ExecutionEvent::StepSkipped { step_id: step.id.clone() });
            step_results.push(StepResult {
                id: step.id.clone(),
                status: StepStatus::Skipped,
                output: "skipped: dependency failed".to_string(),
                duration_ms: 0,
            });
            failed_steps.insert(step.id.clone());
            continue;
        }

        // Conditional step: evaluate run_if condition
        if let Some(ref condition) = step.run_if {
            if opts.dry_run {
                println!("[dry-run] step '{}' run_if: {}", step.id, condition);
            } else {
                let cond_result = Command::new("bash")
                    .arg("-c")
                    .arg(condition)
                    .envs(&env)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                match cond_result {
                    Ok(status) if !status.success() => {
                        send(ExecutionEvent::StepSkipped { step_id: step.id.clone() });
                        step_results.push(StepResult {
                            id: step.id.clone(),
                            status: StepStatus::Skipped,
                            output: format!("condition not met: {condition}"),
                            duration_ms: 0,
                        });
                        // Do NOT add to failed_steps — dependents still run
                        continue;
                    }
                    Err(e) => {
                        send(ExecutionEvent::StepSkipped { step_id: step.id.clone() });
                        step_results.push(StepResult {
                            id: step.id.clone(),
                            status: StepStatus::Skipped,
                            output: format!("condition error: {e}"),
                            duration_ms: 0,
                        });
                        continue;
                    }
                    _ => {} // condition met, proceed
                }
            }
        }

        if step.parallel {
            eprintln!(
                "warning: parallel execution not supported in MVP, running '{}' sequentially",
                step.id
            );
        }

        // Sudo access warning
        if !opts.dry_run {
            let expanded_for_check = expand_template(&step.cmd, &template_vars);
            if let Some(sudo_warning) = check_sudo_access(&expanded_for_check) {
                send(ExecutionEvent::Warning {
                    step_id: step.id.clone(),
                    message: sudo_warning,
                });
            }
        }

        let expanded_cmd = expand_template(&step.cmd, &template_vars);
        let masked_cmd = mask_secrets(&expanded_cmd, &secret_values);

        // Dangerous command detection
        if !opts.force && !opts.dry_run {
            if let Some(warning) = check_dangerous(&expanded_cmd) {
                send(ExecutionEvent::DangerousCommand {
                    step_id: step.id.clone(),
                    warning: warning.to_string(),
                });
                // In CLI mode (no event sender), block the step
                if event_tx.is_none() {
                    eprintln!("WARNING: {warning}");
                    eprintln!("Blocked: dangerous command detected in step '{}'. Use --force to override.", step.id);
                }
                step_results.push(StepResult {
                    id: step.id.clone(),
                    status: StepStatus::Failed,
                    output: format!("Blocked: {warning}. Use --force to override."),
                    duration_ms: 0,
                });
                overall_exit = 1;
                failed_steps.insert(step.id.clone());
                send(ExecutionEvent::StepCompleted {
                    step_id: step.id.clone(),
                    status: StepStatus::Failed,
                    duration_ms: 0,
                });
                continue;
            }
        }

        if opts.dry_run {
            send(ExecutionEvent::StepStarted {
                step_id: step.id.clone(),
                cmd_preview: format!("[dry-run] {masked_cmd}"),
            });
            if let Some(ref dir) = workflow.workdir {
                println!("[dry-run] step '{}' (in {}): {}", step.id, dir.display(), masked_cmd);
            } else {
                println!("[dry-run] step '{}': {}", step.id, masked_cmd);
            }
            step_results.push(StepResult {
                id: step.id.clone(),
                status: StepStatus::Success,
                output: format!("[dry-run] {masked_cmd}"),
                duration_ms: 0,
            });
            send(ExecutionEvent::StepCompleted {
                step_id: step.id.clone(),
                status: StepStatus::Success,
                duration_ms: 0,
            });
            continue;
        }

        // Check if this step should run interactively
        let is_interactive = match step.interactive {
            Some(v) => v,
            None => detect::is_interactive_command(&expanded_cmd),
        };

        if is_interactive {
            let cmd_label = format!("(streaming) {}", mask_secrets(&truncate_cmd(&expanded_cmd, 50), &secret_values));
            send(ExecutionEvent::StepStarted {
                step_id: step.id.clone(),
                cmd_preview: cmd_label.clone(),
            });

            let timer = Instant::now();

            // Prefer streaming modal (TUI stays in alternate screen)
            if let Some(ref stx) = opts.streaming_tx {
                let (kill_tx, kill_rx) = mpsc::channel::<()>();
                let _ = stx.send(StreamingRequest {
                    step_id: step.id.clone(),
                    cmd_preview: cmd_label,
                    kill_tx,
                });

                // Spawn child with piped stdout/stderr
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
                        let tx_clone = event_tx.map(|t| t.clone());

                        // Stream stdout in a thread (with secret masking)
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

                        // Stream stderr in a thread (with secret masking)
                        let stderr_handle = stderr.map(|err| {
                            let sid = step.id.clone();
                            let txc = event_tx.map(|t| t.clone());
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

                        // Wait for either process to exit or kill signal
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

                        // Wait for reader threads to finish
                        if let Some(h) = stdout_handle { let _ = h.join(); }
                        if let Some(h) = stderr_handle { let _ = h.join(); }

                        let status = child.try_wait().ok().flatten();
                        let duration_ms = timer.elapsed().as_millis() as u64;
                        let (step_status, exit_code) = match status {
                            Some(s) if s.success() => (StepStatus::Success, 0),
                            Some(s) => (StepStatus::Failed, s.code().unwrap_or(1)),
                            None => (StepStatus::Success, 0), // killed by user
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
                // Fallback: suspend TUI for interactive step (CLI mode)
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
                // No TUI: run with inherited stdio directly
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
            continue;
        }

        send(ExecutionEvent::StepStarted {
            step_id: step.id.clone(),
            cmd_preview: mask_secrets(&truncate_cmd(&expanded_cmd, 60), &secret_values),
        });

        let max_attempts = step.retry.unwrap_or(0) + 1;
        let retry_delay_secs = step.retry_delay.unwrap_or(0);
        let mut last_output = String::new();
        let mut last_exit_code = 0i32;
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

            let timeout_secs = step.timeout.or(opts.default_timeout);

            let mut cmd = Command::new("bash");
            cmd.arg("-c").arg(&expanded_cmd).envs(&env);
            if let Some(ref dir) = workflow.workdir {
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
                                    last_exit_code = status.code().unwrap_or(1);
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
                            last_exit_code = output.status.code().unwrap_or(1);
                        }
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            };

            if let Err(e) = result {
                last_output = format!("failed to execute: {e}");
                last_exit_code = 1;
            }

            // Timeout: don't retry
            if step_timed_out || step_succeeded {
                break;
            }
        }

        let duration_ms = timer.elapsed().as_millis() as u64;
        let masked_output = mask_secrets(&last_output, &secret_values);

        if step_succeeded {
            // Capture step outputs as template variables
            if !step.outputs.is_empty() {
                for out_def in &step.outputs {
                    if let Ok(re) = regex::Regex::new(&out_def.pattern) {
                        if let Some(caps) = re.captures(&last_output) {
                            if let Some(val) = caps.get(1) {
                                let key = format!("{}.{}", step.id, out_def.name);
                                template_vars.insert(key, val.as_str().to_string());
                            }
                        }
                    }
                }
            }

            step_results.push(StepResult {
                id: step.id.clone(),
                status: StepStatus::Success,
                output: masked_output,
                duration_ms,
            });
            send(ExecutionEvent::StepCompleted {
                step_id: step.id.clone(),
                status: StepStatus::Success,
                duration_ms,
            });
        } else if step_timed_out {
            let secs = step.timeout.or(opts.default_timeout).unwrap_or(0);
            overall_exit = 124;
            failed_steps.insert(step.id.clone());
            step_results.push(StepResult {
                id: step.id.clone(),
                status: StepStatus::Timedout,
                output: masked_output,
                duration_ms,
            });
            send(ExecutionEvent::StepTimedOut {
                step_id: step.id.clone(),
                timeout_secs: secs,
                duration_ms,
            });
        } else {
            overall_exit = last_exit_code;
            failed_steps.insert(step.id.clone());
            // Compute error hint before consuming masked_output
            let error_hint = classify_error(&masked_output);
            let output = if max_attempts > 1 {
                format!("{masked_output} (after {max_attempts} attempts)")
            } else {
                masked_output
            };
            step_results.push(StepResult {
                id: step.id.clone(),
                status: StepStatus::Failed,
                output,
                duration_ms,
            });
            send(ExecutionEvent::StepCompleted {
                step_id: step.id.clone(),
                status: StepStatus::Failed,
                duration_ms,
            });
            // Error classification hint
            if let Some(hint) = error_hint {
                send(ExecutionEvent::Warning {
                    step_id: step.id.clone(),
                    message: hint,
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
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(),
                },
                Step {
                    id: "s2".into(),
                    cmd: "echo world".into(),
                    needs: vec!["s1".into()],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(),
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
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(),
                },
                Step {
                    id: "dependent".into(),
                    cmd: "echo should-not-run".into(),
                    needs: vec!["bad".into()],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(),
                },
                Step {
                    id: "independent".into(),
                    cmd: "echo runs-fine".into(),
                    needs: vec![],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(),
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
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(),
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
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(),
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
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(),
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
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(),
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
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(),
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
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(),
                },
                Step {
                    id: "dependent".into(),
                    cmd: "echo should-not-run".into(),
                    needs: vec!["slow".into()],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(),
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
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(),
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
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(),
                },
                Step {
                    id: "after".into(),
                    cmd: "echo runs-anyway".into(),
                    needs: vec!["maybe".into()],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(),
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
                retry: Some(2),
                retry_delay: Some(0),
                interactive: None, outputs: Vec::new(),
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
                retry: Some(2),
                retry_delay: Some(0),
                interactive: None, outputs: Vec::new(),
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
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(),
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
}

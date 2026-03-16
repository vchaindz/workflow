use std::collections::HashMap;
use std::path::PathBuf;

use crate::core::config::Config;
use crate::core::db;
use crate::core::discovery::{resolve_task_ref, scan_workflows};
use crate::core::executor::{execute_workflow, load_secret_env, send_notifications, ExecuteOpts};
use crate::core::models::{StepStatus, TaskKind};
use crate::core::parser::{parse_shell_task, parse_workflow};
use crate::error::Result;

pub fn cmd_run(
    config: &Config,
    task_ref: &str,
    dry_run: bool,
    env_vars: &[String],
    timeout: Option<u64>,
    background: bool,
    force: bool,
) -> Result<i32> {
    // Background mode: re-exec self without --background, detached from terminal
    if background {
        let exe = std::env::current_exe()?;
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("run").arg(task_ref);
        if dry_run {
            cmd.arg("--dry-run");
        }
        for ev in env_vars {
            cmd.arg("--env").arg(ev);
        }
        if let Some(t) = timeout {
            cmd.arg("--timeout").arg(t.to_string());
        }
        if let Ok(ref dir) = config.workflows_dir.canonicalize() {
            cmd.arg("--dir").arg(dir);
        }
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
        let child = cmd.spawn()?;
        eprintln!("background: started as PID {}", child.id());
        return Ok(0);
    }

    let categories = scan_workflows(&config.workflows_dir)?;
    let task = resolve_task_ref(&categories, task_ref)?;

    let workflow = match task.kind {
        TaskKind::ShellScript => parse_shell_task(&task.path)?,
        TaskKind::YamlWorkflow => parse_workflow(&task.path)?,
    };

    let env_overrides: HashMap<String, String> = env_vars
        .iter()
        .filter_map(|s| {
            let mut parts = s.splitn(2, '=');
            let key = parts.next()?.to_string();
            let value = parts.next().unwrap_or("").to_string();
            Some((key, value))
        })
        .collect();

    // Resolve timeout: CLI flag overrides config default. --timeout 0 means no timeout.
    let effective_timeout = match timeout {
        Some(0) => None,
        Some(t) => Some(t),
        None => config.default_timeout,
    };

    // Merge secrets: workflow + config
    let mut secrets = workflow.secrets.clone();
    for s in &config.secrets {
        if !secrets.contains(s) {
            secrets.push(s.clone());
        }
    }

    let opts = ExecuteOpts {
        dry_run,
        force,
        env_overrides,
        default_timeout: effective_timeout,
        secrets,
        interactive_tx: None,
        streaming_tx: None,
        workflows_dir: Some(config.workflows_dir.clone()),
        call_depth: 0,
        max_call_depth: 10,
        secrets_ssh_key: config.secrets_ssh_key.as_ref().map(PathBuf::from),
        mcp_servers: config.mcp.servers.clone(),
    };

    let canonical_ref = format!("{}/{}", task.category, task.name);
    let run_log = execute_workflow(&workflow, &canonical_ref, &opts, None)?;
    let exit_code = run_log.exit_code;

    if !dry_run {
        let conn = db::open_db(&config.db_path())?;
        db::insert_run_log_with_source(&conn, &run_log, "cli")?;
        if exit_code == 0 {
            eprintln!("success: run logged to database");
        } else {
            eprintln!("failed (exit {}): run logged to database", exit_code);
            for step in &run_log.steps {
                if step.status == StepStatus::Failed || step.status == StepStatus::Timedout {
                    let label = if step.status == StepStatus::Timedout {
                        "timed out"
                    } else {
                        "failed"
                    };
                    eprintln!("--- step '{}' {} ---", step.id, label);
                    eprintln!("{}", step.output);
                }
            }
        }

        // Send trait-based notifications
        let secret_env = load_secret_env(
            &opts.secrets,
            &config.workflows_dir,
            config.secrets_ssh_key.as_ref().map(std::path::Path::new),
        );
        send_notifications(&canonical_ref, &run_log, &workflow.name, &workflow.notify, &config.notify, &secret_env);
    }

    Ok(exit_code)
}

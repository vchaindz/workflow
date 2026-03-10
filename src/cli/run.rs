use std::collections::HashMap;

use crate::core::config::Config;
use crate::core::discovery::{resolve_task_ref, scan_workflows};
use crate::core::executor::{execute_workflow, ExecuteOpts};
use crate::core::logger::write_run_log;
use crate::core::models::TaskKind;
use crate::core::parser::{parse_shell_task, parse_workflow};
use crate::error::Result;

pub fn cmd_run(
    config: &Config,
    task_ref: &str,
    dry_run: bool,
    env_vars: &[String],
) -> Result<i32> {
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

    let opts = ExecuteOpts {
        dry_run,
        env_overrides,
    };

    let canonical_ref = format!("{}/{}", task.category, task.name);
    let run_log = execute_workflow(&workflow, &canonical_ref, &opts)?;
    let exit_code = run_log.exit_code;

    if !dry_run {
        let log_path = write_run_log(&config.logs_dir(), &run_log)?;
        if exit_code == 0 {
            eprintln!("success: log written to {}", log_path.display());
        } else {
            eprintln!("failed (exit {}): log written to {}", exit_code, log_path.display());
            // Print failed step output
            for step in &run_log.steps {
                if step.status == crate::core::models::StepStatus::Failed {
                    eprintln!("--- step '{}' output ---", step.id);
                    eprintln!("{}", step.output);
                }
            }
        }
    }

    Ok(exit_code)
}

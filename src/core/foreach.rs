use std::collections::HashMap;
use std::process::Command;
use std::sync::mpsc;

use crate::core::config::McpServerConfig;
use crate::core::models::{ExecutionEvent, ForEachSource, Step};
use crate::core::template::expand_template;
use crate::error::Result;

use super::executor::{execute_single_step, StepOutcome};

/// Resolve the items for a for_each step. Returns None if the step has no for_each config.
pub(crate) fn resolve_for_each_items(
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
pub(crate) fn execute_for_each_step(
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
    mcp_servers: &HashMap<String, McpServerConfig>,
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
            let mcp_servers_clone = mcp_servers.clone();

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
                    &mcp_servers_clone,
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
                mcp_servers,
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

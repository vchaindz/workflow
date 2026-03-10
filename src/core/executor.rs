use std::collections::HashMap;
use std::process::Command;
use std::time::Instant;

use chrono::Utc;
use uuid::Uuid;

use crate::core::models::{ExecutionEvent, RunLog, Step, StepResult, StepStatus, Workflow};
use crate::core::parser::topological_sort;
use crate::core::template::expand_template;
use crate::error::Result;

pub struct ExecuteOpts {
    pub dry_run: bool,
    pub env_overrides: HashMap<String, String>,
}

impl Default for ExecuteOpts {
    fn default() -> Self {
        Self {
            dry_run: false,
            env_overrides: HashMap::new(),
        }
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

    let order = topological_sort(&workflow.steps)?;
    let started = Utc::now();
    let mut step_results: Vec<StepResult> = Vec::new();
    let mut failed_steps: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut overall_exit = 0i32;

    // Merge env: workflow env + overrides
    let mut env: HashMap<String, String> = workflow.env.clone();
    env.extend(opts.env_overrides.clone());

    // Expand templates in env values
    let template_vars: HashMap<String, String> = env.clone();

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

        if step.parallel {
            eprintln!(
                "warning: parallel execution not supported in MVP, running '{}' sequentially",
                step.id
            );
        }

        let expanded_cmd = expand_template(&step.cmd, &template_vars);

        if opts.dry_run {
            send(ExecutionEvent::StepStarted {
                step_id: step.id.clone(),
                cmd_preview: format!("[dry-run] {expanded_cmd}"),
            });
            if let Some(ref dir) = workflow.workdir {
                println!("[dry-run] step '{}' (in {}): {}", step.id, dir.display(), expanded_cmd);
            } else {
                println!("[dry-run] step '{}': {}", step.id, expanded_cmd);
            }
            step_results.push(StepResult {
                id: step.id.clone(),
                status: StepStatus::Success,
                output: format!("[dry-run] {expanded_cmd}"),
                duration_ms: 0,
            });
            send(ExecutionEvent::StepCompleted {
                step_id: step.id.clone(),
                status: StepStatus::Success,
                duration_ms: 0,
            });
            continue;
        }

        send(ExecutionEvent::StepStarted {
            step_id: step.id.clone(),
            cmd_preview: truncate_cmd(&expanded_cmd, 60),
        });

        let timer = Instant::now();
        let mut cmd = Command::new("bash");
        cmd.arg("-c").arg(&expanded_cmd).envs(&env);
        if let Some(ref dir) = workflow.workdir {
            cmd.current_dir(dir);
        }
        let result = cmd.output();

        let duration_ms = timer.elapsed().as_millis() as u64;

        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let combined = format!("{stdout}{stderr}");

                if output.status.success() {
                    step_results.push(StepResult {
                        id: step.id.clone(),
                        status: StepStatus::Success,
                        output: combined,
                        duration_ms,
                    });
                    send(ExecutionEvent::StepCompleted {
                        step_id: step.id.clone(),
                        status: StepStatus::Success,
                        duration_ms,
                    });
                } else {
                    let code = output.status.code().unwrap_or(1);
                    overall_exit = code;
                    failed_steps.insert(step.id.clone());
                    step_results.push(StepResult {
                        id: step.id.clone(),
                        status: StepStatus::Failed,
                        output: combined,
                        duration_ms,
                    });
                    send(ExecutionEvent::StepCompleted {
                        step_id: step.id.clone(),
                        status: StepStatus::Failed,
                        duration_ms,
                    });
                }
            }
            Err(e) => {
                overall_exit = 1;
                failed_steps.insert(step.id.clone());
                step_results.push(StepResult {
                    id: step.id.clone(),
                    status: StepStatus::Failed,
                    output: format!("failed to execute: {e}"),
                    duration_ms,
                });
                send(ExecutionEvent::StepCompleted {
                    step_id: step.id.clone(),
                    status: StepStatus::Failed,
                    duration_ms,
                });
            }
        }
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
    use crate::core::models::Step;

    fn echo_workflow() -> Workflow {
        Workflow {
            name: "test".to_string(),
            steps: vec![
                Step {
                    id: "s1".into(),
                    cmd: "echo hello".into(),
                    needs: vec![],
                    parallel: false,
                },
                Step {
                    id: "s2".into(),
                    cmd: "echo world".into(),
                    needs: vec!["s1".into()],
                    parallel: false,
                },
            ],
            env: HashMap::new(),
            workdir: None,
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
                },
                Step {
                    id: "dependent".into(),
                    cmd: "echo should-not-run".into(),
                    needs: vec!["bad".into()],
                    parallel: false,
                },
                Step {
                    id: "independent".into(),
                    cmd: "echo runs-fine".into(),
                    needs: vec![],
                    parallel: false,
                },
            ],
            env: HashMap::new(),
            workdir: None,
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
            }],
            env: HashMap::from([("MY_VAR".to_string(), "original".to_string())]),
            workdir: None,
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
            }],
            env: HashMap::new(),
            workdir: Some(std::path::PathBuf::from("/tmp")),
        };

        let log = execute_workflow(&wf, "test/workdir", &ExecuteOpts::default(), None).unwrap();
        assert_eq!(log.exit_code, 0);
        assert!(log.steps[0].output.trim().starts_with("/tmp"));
    }
}

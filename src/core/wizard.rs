use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::core::models::{RunLog, Step, StepStatus, Workflow};
use crate::error::Result;

/// Apply optimizations to a workflow based on run data and user options.
pub fn optimize_workflow(
    workflow: &Workflow,
    run: Option<&RunLog>,
    remove_failed: bool,
    remove_skipped: bool,
    parallelize: bool,
) -> Workflow {
    let mut wf = workflow.clone();

    if let Some(run) = run {
        let failed_ids: HashSet<&str> = run
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Failed)
            .map(|s| s.id.as_str())
            .collect();

        let skipped_ids: HashSet<&str> = run
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Skipped)
            .map(|s| s.id.as_str())
            .collect();

        let mut removed: HashSet<String> = HashSet::new();

        if remove_failed {
            removed.extend(failed_ids.iter().map(|s| s.to_string()));
        }
        if remove_skipped {
            removed.extend(skipped_ids.iter().map(|s| s.to_string()));
        }

        if !removed.is_empty() {
            wf.steps.retain(|s| !removed.contains(&s.id));
            // Clean up dangling dependencies
            for step in &mut wf.steps {
                step.needs.retain(|dep| !removed.contains(dep));
            }
        }
    }

    if parallelize {
        parallelize_steps(&mut wf.steps);
    }

    // Renumber auto-generated step IDs if there are gaps
    renumber_steps(&mut wf.steps);

    wf
}

/// Remove implicit sequential dependencies between steps that don't share
/// environment/output references. Only removes deps that look auto-generated
/// (step-N pattern depending on step-(N-1)).
fn parallelize_steps(steps: &mut [Step]) {
    for i in 0..steps.len() {
        let new_needs: Vec<String> = steps[i]
            .needs
            .iter()
            .filter(|dep| {
                // Keep explicit dependencies (non-sequential patterns)
                // Only consider removing if both are auto-generated IDs
                if !is_auto_id(&steps[i].id) || !is_auto_id(dep) {
                    return true;
                }
                // Keep if the dependent step references the dep step's output in its command
                let dep_step = steps.iter().find(|s| &s.id == *dep);
                if let Some(dep_step) = dep_step {
                    // Heuristic: if the cmd references the dep's id or its output patterns
                    steps[i].cmd.contains(&dep_step.id)
                } else {
                    true
                }
            })
            .cloned()
            .collect();
        steps[i].needs = new_needs;
    }
}

fn is_auto_id(id: &str) -> bool {
    id.starts_with("step-") && id[5..].parse::<u32>().is_ok()
}

/// Renumber auto-generated step IDs (step-N) to be sequential without gaps.
fn renumber_steps(steps: &mut [Step]) {
    // Build a mapping of old auto-IDs to new sequential IDs
    let mut remap: Vec<(String, String)> = Vec::new();
    let mut counter = 1u32;

    for step in steps.iter() {
        if is_auto_id(&step.id) {
            let new_id = format!("step-{}", counter);
            if step.id != new_id {
                remap.push((step.id.clone(), new_id));
            }
            counter += 1;
        }
    }

    // Apply renames
    for (old, new) in &remap {
        for step in steps.iter_mut() {
            if step.id == *old {
                step.id = new.clone();
            }
            for dep in step.needs.iter_mut() {
                if dep == old {
                    *dep = new.clone();
                }
            }
        }
    }
}

/// Build a Workflow from raw shell commands.
/// Generates step IDs from first word of each command (docker-1, git-1, etc.)
/// Chains steps sequentially via `needs`.
pub fn workflow_from_commands(name: &str, commands: &[String]) -> Workflow {
    let mut id_counts: HashMap<String, u32> = HashMap::new();
    let mut steps = Vec::with_capacity(commands.len());
    let mut prev_id: Option<String> = None;

    for cmd in commands {
        let first_word = cmd
            .split_whitespace()
            .next()
            .unwrap_or("step")
            .rsplit('/')
            .next()
            .unwrap_or("step");

        // Sanitize to [a-z0-9-]
        let base: String = first_word
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");

        let base = if base.is_empty() {
            "step".to_string()
        } else {
            base
        };

        let count = id_counts.entry(base.clone()).or_insert(0);
        *count += 1;
        let id = format!("{}-{}", base, count);

        let needs = prev_id.take().into_iter().collect();
        prev_id = Some(id.clone());

        steps.push(Step {
            id,
            cmd: cmd.clone(),
            needs,
            parallel: false,
            timeout: None,
            run_if: None,
            skip_if: None,
            retry: None,
            retry_delay: None,
            interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
        });
    }

    Workflow {
        name: name.to_string(),
        steps,
        env: HashMap::new(),
        workdir: None,
        secrets: Vec::new(),
        notify: Default::default(),
        overdue: None,
        variables: Vec::new(),
        cleanup: Vec::new(),
    }
}

/// Check if a YAML scalar value needs quoting.
fn needs_yaml_quoting(s: &str) -> bool {
    // Quote if contains characters that are special in YAML
    s.contains(':') || s.contains('#') || s.contains('"')
        || s.contains('\'') || s.contains('{') || s.contains('}')
        || s.contains('[') || s.contains(']') || s.contains('&')
        || s.contains('*') || s.contains('!') || s.contains('|')
        || s.contains('>') || s.contains('%') || s.contains('@')
        || s.contains('`')
}

/// Serialize a Workflow to clean YAML matching the project's format.
pub fn generate_yaml(workflow: &Workflow) -> String {
    let mut out = format!("name: {}\n", workflow.name);

    if !workflow.env.is_empty() {
        out.push_str("env:\n");
        let mut keys: Vec<&String> = workflow.env.keys().collect();
        keys.sort();
        for key in keys {
            match &workflow.env[key] {
                crate::core::models::EnvValue::Static(val) => {
                    out.push_str(&format!("  {}: \"{}\"\n", key, val));
                }
                crate::core::models::EnvValue::Dynamic { cmd } => {
                    out.push_str(&format!("  {}:\n    cmd: \"{}\"\n", key, cmd));
                }
            }
        }
    }

    if let Some(ref dir) = workflow.workdir {
        out.push_str(&format!("workdir: {}\n", dir.display()));
    }

    out.push_str("steps:\n");
    for step in &workflow.steps {
        out.push_str(&format!("  - id: {}\n", step.id));
        if needs_yaml_quoting(&step.cmd) {
            out.push_str(&format!("    cmd: \"{}\"\n", step.cmd.replace('\\', "\\\\").replace('"', "\\\"")));
        } else {
            out.push_str(&format!("    cmd: {}\n", step.cmd));
        }
        if !step.needs.is_empty() {
            out.push_str(&format!("    needs: [{}]\n", step.needs.join(", ")));
        }
        if let Some(t) = step.timeout {
            out.push_str(&format!("    timeout: {}\n", t));
        }
        if let Some(ref cond) = step.run_if {
            if needs_yaml_quoting(cond) {
                out.push_str(&format!("    run_if: \"{}\"\n", cond.replace('\\', "\\\\").replace('"', "\\\"")));
            } else {
                out.push_str(&format!("    run_if: {}\n", cond));
            }
        }
        if let Some(r) = step.retry {
            out.push_str(&format!("    retry: {}\n", r));
        }
        if let Some(d) = step.retry_delay {
            out.push_str(&format!("    retry_delay: {}\n", d));
        }
        if step.parallel {
            out.push_str("    parallel: true\n");
        }
        if let Some(interactive) = step.interactive {
            out.push_str(&format!("    interactive: {}\n", interactive));
        }
    }

    out
}

/// Save a workflow YAML file to the workflows directory.
/// Creates the category directory if it doesn't exist.
/// Returns the path of the created file.
pub fn save_task(
    workflows_dir: &Path,
    category: &str,
    task_name: &str,
    yaml_content: &str,
) -> Result<PathBuf> {
    let cat_dir = workflows_dir.join(category);
    std::fs::create_dir_all(&cat_dir)?;

    let file_path = cat_dir.join(format!("{}.yaml", task_name));
    std::fs::write(&file_path, yaml_content)?;

    Ok(file_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{StepResult, Workflow};
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn sample_workflow() -> Workflow {
        Workflow {
            name: "Test Workflow".to_string(),
            env: HashMap::from([("DB".to_string(), crate::core::models::EnvValue::Static("mydb".to_string()))]),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
            steps: vec![
                Step {
                    id: "step-1".to_string(),
                    cmd: "echo setup".to_string(),
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
                    id: "step-2".to_string(),
                    cmd: "echo build".to_string(),
                    needs: vec!["step-1".to_string()],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
                Step {
                    id: "step-3".to_string(),
                    cmd: "echo test".to_string(),
                    needs: vec!["step-2".to_string()],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
                Step {
                    id: "deploy".to_string(),
                    cmd: "echo deploy".to_string(),
                    needs: vec!["step-3".to_string()],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
            ],
        }
    }

    fn sample_run_log() -> RunLog {
        RunLog {
            id: "run-1".to_string(),
            task_ref: "test/wf".to_string(),
            started: chrono::Utc::now(),
            ended: None,
            exit_code: 1,
            steps: vec![
                StepResult {
                    id: "step-1".to_string(),
                    status: StepStatus::Success,
                    output: String::new(),
                    duration_ms: 100,
                },
                StepResult {
                    id: "step-2".to_string(),
                    status: StepStatus::Failed,
                    output: "error".to_string(),
                    duration_ms: 200,
                },
                StepResult {
                    id: "step-3".to_string(),
                    status: StepStatus::Skipped,
                    output: String::new(),
                    duration_ms: 0,
                },
                StepResult {
                    id: "deploy".to_string(),
                    status: StepStatus::Skipped,
                    output: String::new(),
                    duration_ms: 0,
                },
            ],
            captured_vars: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn test_optimize_removes_failed_steps() {
        let wf = sample_workflow();
        let run = sample_run_log();
        let result = optimize_workflow(&wf, Some(&run), true, false, false);

        // Original step-2 ("echo build", Failed) should be removed
        // Remaining: step-1, step-3 (renumbered to step-2), deploy
        assert_eq!(result.steps.len(), 3);
        let cmds: Vec<&str> = result.steps.iter().map(|s| s.cmd.as_str()).collect();
        assert!(!cmds.contains(&"echo build"), "failed step cmd should be removed");
        assert!(cmds.contains(&"echo setup"), "successful step should remain");
        // The old step-3 (echo test) had needs=[step-2], that dep should be cleaned
        let test_step = result.steps.iter().find(|s| s.cmd == "echo test").unwrap();
        assert!(test_step.needs.is_empty(), "dep on removed step should be cleaned");
    }

    #[test]
    fn test_optimize_removes_skipped_steps() {
        let wf = sample_workflow();
        let run = sample_run_log();
        let result = optimize_workflow(&wf, Some(&run), false, true, false);

        let ids: Vec<&str> = result.steps.iter().map(|s| s.id.as_str()).collect();
        assert!(!ids.contains(&"step-3"), "skipped step should be removed");
        assert!(ids.contains(&"step-1"));
        assert!(ids.contains(&"step-2"));
    }

    #[test]
    fn test_optimize_removes_failed_and_skipped() {
        let wf = sample_workflow();
        let run = sample_run_log();
        // step-2=Failed, step-3=Skipped, deploy=Skipped → all removed except step-1
        let result = optimize_workflow(&wf, Some(&run), true, true, false);

        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.steps[0].cmd, "echo setup");
    }

    #[test]
    fn test_optimize_no_run_data() {
        let wf = sample_workflow();
        let result = optimize_workflow(&wf, None, true, true, false);
        assert_eq!(result.steps.len(), 4, "without run data, no steps removed");
    }

    #[test]
    fn test_parallelize_removes_auto_deps() {
        let wf = Workflow {
            name: "Pipeline".to_string(),
            env: HashMap::new(),
            workdir: None,
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
            steps: vec![
                Step {
                    id: "step-1".to_string(),
                    cmd: "echo a".to_string(),
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
                    id: "step-2".to_string(),
                    cmd: "echo b".to_string(),
                    needs: vec!["step-1".to_string()],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
            ],
        };

        let result = optimize_workflow(&wf, None, false, false, true);
        // step-2's cmd doesn't reference step-1, so dep should be removed
        assert!(
            result.steps[1].needs.is_empty(),
            "auto dep should be removed when cmds are independent"
        );
    }

    #[test]
    fn test_generate_yaml_roundtrip() {
        let wf = Workflow {
            name: "My Workflow".to_string(),
            env: HashMap::from([("HOST".to_string(), crate::core::models::EnvValue::Static("localhost".to_string()))]),
            workdir: Some(PathBuf::from("/tmp")),
            secrets: Vec::new(),
            notify: Default::default(),
            overdue: None,
            variables: Vec::new(),
            cleanup: Vec::new(),
            steps: vec![
                Step {
                    id: "build".to_string(),
                    cmd: "make build".to_string(),
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
                    id: "test".to_string(),
                    cmd: "make test".to_string(),
                    needs: vec!["build".to_string()],
                    parallel: false,
                    timeout: None,
                    run_if: None,
                    skip_if: None,
                    retry: None,
                    retry_delay: None,
                    interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
                },
            ],
        };

        let yaml = generate_yaml(&wf);
        assert!(yaml.contains("name: My Workflow"));
        assert!(yaml.contains("HOST: \"localhost\""));
        assert!(yaml.contains("workdir: /tmp"));
        assert!(yaml.contains("- id: build"));
        assert!(yaml.contains("needs: [build]"));
    }

    #[test]
    fn test_save_task_creates_file() {
        let dir = TempDir::new().unwrap();
        let yaml = "name: test\nsteps:\n  - echo hello\n";

        let path = save_task(dir.path(), "mycat", "mytask", yaml).unwrap();

        assert!(path.exists());
        assert_eq!(path, dir.path().join("mycat/mytask.yaml"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), yaml);
    }

    #[test]
    fn test_save_task_creates_category_dir() {
        let dir = TempDir::new().unwrap();
        let yaml = "name: test\nsteps:\n  - echo hello\n";

        let path = save_task(dir.path(), "newcat", "newtask", yaml).unwrap();

        assert!(dir.path().join("newcat").is_dir());
        assert!(path.exists());
    }

    #[test]
    fn test_workflow_from_commands() {
        let commands = vec![
            "docker compose up -d".to_string(),
            "docker ps".to_string(),
            "git status".to_string(),
        ];
        let wf = workflow_from_commands("my-task", &commands);

        assert_eq!(wf.name, "my-task");
        assert_eq!(wf.steps.len(), 3);

        // IDs derived from first word with counter
        assert_eq!(wf.steps[0].id, "docker-1");
        assert_eq!(wf.steps[1].id, "docker-2");
        assert_eq!(wf.steps[2].id, "git-1");

        // Sequential chaining
        assert!(wf.steps[0].needs.is_empty());
        assert_eq!(wf.steps[1].needs, vec!["docker-1"]);
        assert_eq!(wf.steps[2].needs, vec!["docker-2"]);

        // Commands preserved
        assert_eq!(wf.steps[0].cmd, "docker compose up -d");
    }

    #[test]
    fn test_renumber_fills_gaps() {
        let mut steps = vec![
            Step {
                id: "step-1".to_string(),
                cmd: "echo a".to_string(),
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
                id: "step-5".to_string(),
                cmd: "echo b".to_string(),
                needs: vec!["step-1".to_string()],
                parallel: false,
                timeout: None,
                run_if: None,
                skip_if: None,
                retry: None,
                retry_delay: None,
                interactive: None, outputs: Vec::new(), call: None, for_each: None, for_each_cmd: None, for_each_parallel: false, for_each_continue_on_error: false, mcp: None,
            },
        ];
        renumber_steps(&mut steps);
        assert_eq!(steps[0].id, "step-1");
        assert_eq!(steps[1].id, "step-2");
        assert_eq!(steps[1].needs, vec!["step-1"]);
    }
}

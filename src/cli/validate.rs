use serde::Serialize;

use crate::core::config::Config;
use crate::core::discovery::{resolve_task_ref, scan_workflows};
use crate::core::models::TaskKind;
use crate::core::parser::{parse_shell_task, parse_workflow};
use crate::error::Result;

#[derive(Serialize)]
struct ValidationResult {
    task_ref: String,
    valid: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    errors: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

pub fn cmd_validate(config: &Config, task: Option<&str>, json: bool) -> Result<i32> {
    let categories = scan_workflows(&config.workflows_dir)?;
    let mut results: Vec<ValidationResult> = Vec::new();

    if let Some(task_ref) = task {
        // Validate a single task
        let t = resolve_task_ref(&categories, task_ref)?;
        let canonical = format!("{}/{}", t.category, t.name);
        results.push(validate_one(&t.path, &canonical, t.kind.clone()));
    } else {
        // Validate all tasks
        for cat in &categories {
            for t in &cat.tasks {
                let canonical = format!("{}/{}", t.category, t.name);
                results.push(validate_one(&t.path, &canonical, t.kind.clone()));
            }
        }
    }

    let total = results.len();
    let errors = results.iter().filter(|r| !r.valid).count();
    let ok = total - errors;

    if json {
        let json_out = serde_json::to_string_pretty(&results)?;
        println!("{json_out}");
    } else {
        for r in &results {
            if r.valid {
                if r.warnings.is_empty() {
                    println!("  OK  {}", r.task_ref);
                } else {
                    println!("  OK  {} ({})", r.task_ref, r.warnings.join("; "));
                }
            } else {
                println!(" ERR  {} — {}", r.task_ref, r.errors.join("; "));
            }
        }
        println!();
        if errors > 0 {
            println!("Validated {total} tasks: {ok} OK, {errors} error(s)");
        } else {
            println!("Validated {total} tasks: all OK");
        }
    }

    if errors > 0 {
        Ok(1)
    } else {
        Ok(0)
    }
}

fn validate_one(
    path: &std::path::Path,
    task_ref: &str,
    kind: TaskKind,
) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let parse_result = match kind {
        TaskKind::ShellScript => parse_shell_task(path),
        TaskKind::YamlWorkflow => parse_workflow(path),
    };

    match parse_result {
        Ok(wf) => {
            for step in &wf.steps {
                if step.cmd.trim().is_empty() {
                    warnings.push(format!("step '{}' has empty cmd", step.id));
                }
                if step.parallel {
                    warnings.push(format!("step '{}' uses parallel (not yet implemented)", step.id));
                }
            }
        }
        Err(e) => {
            errors.push(e.to_string());
        }
    }

    ValidationResult {
        task_ref: task_ref.to_string(),
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}

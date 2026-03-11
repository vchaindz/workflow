use crate::core::config::Config;
use crate::core::db;
use crate::core::models::StepStatus;
use crate::error::Result;

pub fn cmd_logs(config: &Config, task: Option<&str>, json: bool, limit: usize) -> Result<()> {
    let conn = db::open_db(&config.db_path())?;

    let history = if let Some(task_ref) = task {
        db::get_task_history(&conn, task_ref, limit)?
    } else {
        db::get_recent_runs(&conn, limit)?
    };

    if json {
        let output = serde_json::to_string_pretty(&history)?;
        println!("{output}");
    } else if history.is_empty() {
        if let Some(task_ref) = task {
            println!("No logs found for {task_ref}");
        } else {
            println!("No logs found");
        }
    } else {
        for log in &history {
            let status = if log.exit_code == 0 { "OK" } else { "FAIL" };
            let duration = log
                .ended
                .map(|e| format!("{}ms", (e - log.started).num_milliseconds()))
                .unwrap_or_else(|| "?".to_string());
            println!(
                "[{status}] {} @ {} ({duration})",
                log.task_ref,
                log.started.format("%Y-%m-%d %H:%M:%S")
            );
            for step in &log.steps {
                let icon = match step.status {
                    StepStatus::Success => "+",
                    StepStatus::Failed => "!",
                    StepStatus::Skipped => "-",
                    _ => "?",
                };
                println!("  [{icon}] {} ({}ms)", step.id, step.duration_ms);
            }
        }
    }

    Ok(())
}

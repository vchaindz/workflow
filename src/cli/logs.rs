use crate::core::config::Config;
use crate::core::logger::get_task_history;
use crate::core::models::StepStatus;
use crate::error::Result;

pub fn cmd_logs(config: &Config, task: Option<&str>, json: bool, limit: usize) -> Result<()> {
    let log_dir = config.logs_dir();

    if let Some(task_ref) = task {
        let history = get_task_history(&log_dir, task_ref, limit)?;

        if json {
            let output = serde_json::to_string_pretty(&history)?;
            println!("{output}");
        } else if history.is_empty() {
            println!("No logs found for {task_ref}");
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
    } else {
        // List all recent logs
        if !log_dir.exists() {
            println!("No logs directory found");
            return Ok(());
        }

        let mut entries: Vec<_> = std::fs::read_dir(&log_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|ext| ext == "json")
            })
            .collect();

        entries.sort_by(|a, b| {
            b.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                .cmp(
                    &a.metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                )
        });

        entries.truncate(limit);

        for entry in entries {
            let name = entry.file_name();
            println!("{}", name.to_string_lossy());
        }
    }

    Ok(())
}

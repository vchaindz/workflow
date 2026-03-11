use crate::core::config::Config;
use crate::core::db;
use crate::core::discovery::{resolve_task_ref, scan_workflows};
use crate::error::Result;

pub fn cmd_status(config: &Config, task_ref: &str, json: bool) -> Result<()> {
    let categories = scan_workflows(&config.workflows_dir)?;
    let task = resolve_task_ref(&categories, task_ref)?;
    let canonical_ref = format!("{}/{}", task.category, task.name);

    let conn = db::open_db(&config.db_path())?;
    let summary = db::get_run_summary(&conn, &canonical_ref)?;

    match summary {
        Some(summary) => {
            if json {
                let output = serde_json::to_string_pretty(&summary)?;
                println!("{output}");
            } else {
                println!("Task: {canonical_ref}");
                if let Some(ts) = summary.last_success {
                    println!("  Last success: {ts}");
                }
                if let Some(ts) = summary.last_failure {
                    println!("  Last failure: {ts}");
                }
                println!("  Fail count:   {}", summary.fail_count);
                if let Some(ms) = summary.last_duration_ms {
                    println!("  Last duration: {ms}ms");
                }
            }
        }
        None => {
            println!("No run history for {canonical_ref}");
        }
    }

    Ok(())
}

use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::core::models::{RunLog, RunSummary};
use crate::error::Result;

/// Write a run log as a JSON file.
pub fn write_run_log(log_dir: &Path, run_log: &RunLog) -> Result<PathBuf> {
    std::fs::create_dir_all(log_dir)?;

    let filename = format!(
        "{}_{}.json",
        run_log.task_ref.replace('/', "_"),
        run_log.started.format("%Y%m%d_%H%M%S")
    );
    let path = log_dir.join(&filename);
    let json = serde_json::to_string_pretty(run_log)?;
    std::fs::write(&path, json)?;
    Ok(path)
}

/// Get run history for a task, sorted newest first.
pub fn get_task_history(log_dir: &Path, task_ref: &str, limit: usize) -> Result<Vec<RunLog>> {
    if !log_dir.exists() {
        return Ok(Vec::new());
    }

    let prefix = task_ref.replace('/', "_");
    let mut logs = Vec::new();

    for entry in std::fs::read_dir(log_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with(&prefix) && name_str.ends_with(".json") {
            let contents = std::fs::read_to_string(entry.path())?;
            if let Ok(log) = serde_json::from_str::<RunLog>(&contents) {
                logs.push(log);
            }
        }
    }

    logs.sort_by(|a, b| b.started.cmp(&a.started));
    logs.truncate(limit);
    Ok(logs)
}

/// Get a summary of recent runs for a task.
pub fn get_run_summary(log_dir: &Path, task_ref: &str) -> Result<Option<RunSummary>> {
    let history = get_task_history(log_dir, task_ref, 100)?;

    if history.is_empty() {
        return Ok(None);
    }

    let mut last_success = None;
    let mut last_failure = None;
    let mut fail_count = 0u32;

    for log in &history {
        if log.exit_code == 0 {
            if last_success.is_none() {
                last_success = Some(log.started);
            }
        } else {
            if last_failure.is_none() {
                last_failure = Some(log.started);
            }
            fail_count += 1;
        }
    }

    let last_duration_ms = history.first().and_then(|log| {
        log.ended
            .map(|ended| (ended - log.started).num_milliseconds() as u64)
    });

    Ok(Some(RunSummary {
        last_success,
        last_failure,
        fail_count,
        last_duration_ms,
    }))
}

/// Delete log files older than retention_days. Returns count of deleted files.
pub fn rotate_logs(log_dir: &Path, retention_days: u32) -> Result<u32> {
    if !log_dir.exists() {
        return Ok(0);
    }

    let cutoff = Utc::now() - chrono::Duration::days(retention_days as i64);
    let mut deleted = 0u32;

    for entry in std::fs::read_dir(log_dir)? {
        let entry = entry?;
        if !entry.path().extension().is_some_and(|e| e == "json") {
            continue;
        }

        let metadata = entry.metadata()?;
        if let Ok(modified) = metadata.modified() {
            let modified: chrono::DateTime<Utc> = modified.into();
            if modified < cutoff {
                std::fs::remove_file(entry.path())?;
                deleted += 1;
            }
        }
    }

    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{StepResult, StepStatus};
    use tempfile::TempDir;

    fn sample_log() -> RunLog {
        RunLog {
            id: "test-id".to_string(),
            task_ref: "backup/db-full".to_string(),
            started: Utc::now(),
            ended: Some(Utc::now()),
            steps: vec![StepResult {
                id: "run".to_string(),
                status: StepStatus::Success,
                output: "done".to_string(),
                duration_ms: 150,
            }],
            exit_code: 0,
        }
    }

    #[test]
    fn test_write_and_read_log() {
        let dir = TempDir::new().unwrap();
        let log = sample_log();
        let path = write_run_log(dir.path(), &log).unwrap();
        assert!(path.exists());

        let history = get_task_history(dir.path(), "backup/db-full", 10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].task_ref, "backup/db-full");
    }

    #[test]
    fn test_run_summary() {
        let dir = TempDir::new().unwrap();
        let log = sample_log();
        write_run_log(dir.path(), &log).unwrap();

        let summary = get_run_summary(dir.path(), "backup/db-full")
            .unwrap()
            .unwrap();
        assert!(summary.last_success.is_some());
        assert_eq!(summary.fail_count, 0);
    }

    #[test]
    fn test_empty_history() {
        let dir = TempDir::new().unwrap();
        let history = get_task_history(dir.path(), "nonexistent", 10).unwrap();
        assert!(history.is_empty());
    }
}

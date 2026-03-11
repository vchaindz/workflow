use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use crate::core::models::{RunLog, RunSummary, StepResult, StepStatus};
use crate::error::Result;

pub fn open_db(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS runs (
            id        TEXT PRIMARY KEY,
            task_ref  TEXT NOT NULL,
            started   TEXT NOT NULL,
            ended     TEXT,
            exit_code INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_runs_task_ref ON runs(task_ref);
        CREATE INDEX IF NOT EXISTS idx_runs_started ON runs(started);

        CREATE TABLE IF NOT EXISTS steps (
            run_id      TEXT NOT NULL REFERENCES runs(id),
            step_id     TEXT NOT NULL,
            status      TEXT NOT NULL,
            output      TEXT NOT NULL DEFAULT '',
            duration_ms INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (run_id, step_id)
        );",
    )?;

    Ok(conn)
}

pub fn insert_run_log(conn: &Connection, log: &RunLog) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    tx.execute(
        "INSERT INTO runs (id, task_ref, started, ended, exit_code) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            log.id,
            log.task_ref,
            log.started.to_rfc3339(),
            log.ended.map(|e| e.to_rfc3339()),
            log.exit_code,
        ],
    )?;

    for step in &log.steps {
        tx.execute(
            "INSERT INTO steps (run_id, step_id, status, output, duration_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                log.id,
                step.id,
                format!("{:?}", step.status),
                step.output,
                step.duration_ms as i64,
            ],
        )?;
    }

    tx.commit()?;
    Ok(())
}

fn parse_status(s: &str) -> StepStatus {
    match s {
        "Success" => StepStatus::Success,
        "Failed" => StepStatus::Failed,
        "Skipped" => StepStatus::Skipped,
        "Timedout" => StepStatus::Timedout,
        "Running" => StepStatus::Running,
        _ => StepStatus::Pending,
    }
}

fn parse_dt(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc))
}

fn load_steps(conn: &Connection, run_id: &str) -> Result<Vec<StepResult>> {
    let mut stmt = conn.prepare(
        "SELECT step_id, status, output, duration_ms FROM steps WHERE run_id = ?1",
    )?;
    let rows = stmt.query_map(params![run_id], |row| {
        Ok(StepResult {
            id: row.get(0)?,
            status: parse_status(&row.get::<_, String>(1)?),
            output: row.get(2)?,
            duration_ms: row.get::<_, i64>(3)? as u64,
        })
    })?;
    let mut steps = Vec::new();
    for r in rows {
        steps.push(r?);
    }
    Ok(steps)
}

fn rows_to_run_logs(conn: &Connection, stmt: &mut rusqlite::Statement, p: &[&dyn rusqlite::types::ToSql]) -> Result<Vec<RunLog>> {
    let rows = stmt.query_map(p, |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, i32>(4)?,
        ))
    })?;

    let mut logs = Vec::new();
    for r in rows {
        let (id, task_ref, started_str, ended_str, exit_code) = r?;
        let started = parse_dt(&started_str).unwrap_or_else(Utc::now);
        let ended = ended_str.as_deref().and_then(parse_dt);
        let steps = load_steps(conn, &id)?;
        logs.push(RunLog {
            id,
            task_ref,
            started,
            ended,
            steps,
            exit_code,
        });
    }
    Ok(logs)
}

pub fn get_task_history(conn: &Connection, task_ref: &str, limit: usize) -> Result<Vec<RunLog>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_ref, started, ended, exit_code FROM runs WHERE task_ref = ?1 ORDER BY started DESC LIMIT ?2",
    )?;
    rows_to_run_logs(conn, &mut stmt, &[&task_ref, &(limit as i64)])
}

pub fn get_recent_runs(conn: &Connection, limit: usize) -> Result<Vec<RunLog>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_ref, started, ended, exit_code FROM runs ORDER BY started DESC LIMIT ?1",
    )?;
    rows_to_run_logs(conn, &mut stmt, &[&(limit as i64)])
}

pub fn get_run_by_id(conn: &Connection, run_id: &str) -> Result<Option<RunLog>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_ref, started, ended, exit_code FROM runs WHERE id = ?1",
    )?;
    let mut logs = rows_to_run_logs(conn, &mut stmt, &[&run_id])?;
    Ok(if logs.is_empty() { None } else { Some(logs.remove(0)) })
}

pub fn get_run_summary(conn: &Connection, task_ref: &str) -> Result<Option<RunSummary>> {
    let mut stmt = conn.prepare(
        "SELECT exit_code, started, ended FROM runs WHERE task_ref = ?1 ORDER BY started DESC LIMIT 100",
    )?;
    let rows = stmt.query_map(params![task_ref], |row| {
        Ok((
            row.get::<_, i32>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;

    let mut entries = Vec::new();
    for r in rows {
        entries.push(r?);
    }

    if entries.is_empty() {
        return Ok(None);
    }

    let mut last_success = None;
    let mut last_failure = None;
    let mut fail_count = 0u32;

    for (exit_code, started_str, _) in &entries {
        let ts = parse_dt(started_str);
        if *exit_code == 0 {
            if last_success.is_none() {
                last_success = ts;
            }
        } else {
            if last_failure.is_none() {
                last_failure = ts;
            }
            fail_count += 1;
        }
    }

    let last_duration_ms = {
        let (_, started_str, ended_str) = &entries[0];
        match (parse_dt(started_str), ended_str.as_deref().and_then(parse_dt)) {
            (Some(s), Some(e)) => Some((e - s).num_milliseconds() as u64),
            _ => None,
        }
    };

    Ok(Some(RunSummary {
        last_success,
        last_failure,
        fail_count,
        last_duration_ms,
    }))
}

pub struct GlobalStats {
    pub total_runs: u64,
    pub failed_runs: u64,
}

pub fn get_global_stats(conn: &Connection) -> Result<GlobalStats> {
    let mut stmt = conn.prepare(
        "SELECT COUNT(*), COUNT(CASE WHEN exit_code != 0 THEN 1 END) FROM runs",
    )?;
    let stats = stmt.query_row([], |row| {
        Ok(GlobalStats {
            total_runs: row.get::<_, i64>(0)? as u64,
            failed_runs: row.get::<_, i64>(1)? as u64,
        })
    })?;
    Ok(stats)
}

pub fn rotate_runs(conn: &Connection, retention_days: u32) -> Result<u32> {
    let cutoff = Utc::now() - chrono::Duration::days(retention_days as i64);
    let cutoff_str = cutoff.to_rfc3339();

    conn.execute("DELETE FROM steps WHERE run_id IN (SELECT id FROM runs WHERE started < ?1)", params![cutoff_str])?;
    let deleted = conn.execute("DELETE FROM runs WHERE started < ?1", params![cutoff_str])?;

    Ok(deleted as u32)
}

#[derive(Debug, Clone)]
pub struct OverdueTask {
    pub task_ref: String,
    pub category: String,
    pub name: String,
    pub overdue_days: i64,
}

pub fn check_overdue_tasks(conn: &Connection, categories: &[crate::core::models::Category]) -> Result<Vec<OverdueTask>> {
    let mut overdue = Vec::new();
    let now = Utc::now();

    for cat in categories {
        for task in &cat.tasks {
            let threshold = match task.overdue {
                Some(d) => d,
                None => continue,
            };

            let task_ref = format!("{}/{}", cat.name, task.name);
            let summary = get_run_summary(conn, &task_ref)?;

            let overdue_days = match summary.and_then(|s| s.last_success) {
                Some(last) => {
                    let elapsed = (now - last).num_days();
                    if elapsed > threshold as i64 {
                        elapsed - threshold as i64
                    } else {
                        continue;
                    }
                }
                None => threshold as i64, // never run — report threshold
            };

            overdue.push(OverdueTask {
                task_ref,
                category: cat.name.clone(),
                name: task.name.clone(),
                overdue_days,
            });
        }
    }

    overdue.sort_by(|a, b| b.overdue_days.cmp(&a.overdue_days));
    Ok(overdue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_log() -> RunLog {
        RunLog {
            id: "test-id-1".to_string(),
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
    fn test_insert_and_query() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("history.db");
        let conn = open_db(&db_path).unwrap();

        let log = sample_log();
        insert_run_log(&conn, &log).unwrap();

        let history = get_task_history(&conn, "backup/db-full", 10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].task_ref, "backup/db-full");
        assert_eq!(history[0].steps.len(), 1);
        assert_eq!(history[0].steps[0].id, "run");
    }

    #[test]
    fn test_run_summary() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("history.db");
        let conn = open_db(&db_path).unwrap();

        insert_run_log(&conn, &sample_log()).unwrap();

        let summary = get_run_summary(&conn, "backup/db-full").unwrap().unwrap();
        assert!(summary.last_success.is_some());
        assert_eq!(summary.fail_count, 0);
    }

    #[test]
    fn test_empty_history() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("history.db");
        let conn = open_db(&db_path).unwrap();

        let history = get_task_history(&conn, "nonexistent", 10).unwrap();
        assert!(history.is_empty());
    }

    #[test]
    fn test_recent_runs() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("history.db");
        let conn = open_db(&db_path).unwrap();

        insert_run_log(&conn, &sample_log()).unwrap();

        let mut log2 = sample_log();
        log2.id = "test-id-2".to_string();
        log2.task_ref = "deploy/staging".to_string();
        insert_run_log(&conn, &log2).unwrap();

        let recent = get_recent_runs(&conn, 10).unwrap();
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn test_rotate_runs() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("history.db");
        let conn = open_db(&db_path).unwrap();

        // Insert a log with old timestamp
        let old_time = Utc::now() - chrono::Duration::days(60);
        let log = RunLog {
            id: "old-run".to_string(),
            task_ref: "backup/old".to_string(),
            started: old_time,
            ended: Some(old_time),
            steps: vec![],
            exit_code: 0,
        };
        insert_run_log(&conn, &log).unwrap();
        insert_run_log(&conn, &sample_log()).unwrap();

        let deleted = rotate_runs(&conn, 30).unwrap();
        assert_eq!(deleted, 1);

        let remaining = get_recent_runs(&conn, 10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].task_ref, "backup/db-full");
    }

    #[test]
    fn test_run_summary_empty() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("history.db");
        let conn = open_db(&db_path).unwrap();

        let summary = get_run_summary(&conn, "nonexistent").unwrap();
        assert!(summary.is_none());
    }
}

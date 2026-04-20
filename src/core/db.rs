use std::collections::HashMap;
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

    // Non-breaking schema migration: add audit columns if missing
    let has_username: bool = conn
        .prepare("SELECT username FROM runs LIMIT 0")
        .is_ok();
    if !has_username {
        conn.execute_batch(
            "ALTER TABLE runs ADD COLUMN username TEXT NOT NULL DEFAULT '';
             ALTER TABLE runs ADD COLUMN hostname TEXT NOT NULL DEFAULT '';
             ALTER TABLE runs ADD COLUMN source   TEXT NOT NULL DEFAULT '';",
        )?;
    }

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS snapshots (
            task_ref TEXT NOT NULL,
            key      TEXT NOT NULL,
            value    TEXT NOT NULL,
            created  TEXT NOT NULL,
            PRIMARY KEY (task_ref, key)
        );",
    )?;

    // Memory system tables for anomaly detection & trend tracking
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memory_baselines (
            task_ref TEXT NOT NULL, step_id TEXT NOT NULL,
            metric_key TEXT NOT NULL, mean REAL, stddev REAL, median REAL,
            min_val REAL, max_val REAL, p95 REAL, sample_count INTEGER,
            window_start TEXT, updated TEXT,
            PRIMARY KEY (task_ref, step_id, metric_key)
        );
        CREATE TABLE IF NOT EXISTS memory_metrics (
            run_id TEXT NOT NULL, task_ref TEXT NOT NULL, step_id TEXT NOT NULL,
            metric_key TEXT NOT NULL, value REAL, unit TEXT, recorded TEXT,
            PRIMARY KEY (run_id, step_id, metric_key)
        );
        CREATE INDEX IF NOT EXISTS idx_metrics_task ON memory_metrics(task_ref, metric_key, recorded);
        CREATE TABLE IF NOT EXISTS memory_anomalies (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id TEXT, task_ref TEXT, step_id TEXT,
            anomaly_type TEXT, severity TEXT, description TEXT,
            metric_key TEXT, expected_value REAL, actual_value REAL, z_score REAL,
            detected TEXT, acknowledged INTEGER DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_anomalies_task ON memory_anomalies(task_ref, detected);
        CREATE TABLE IF NOT EXISTS memory_trends (
            task_ref TEXT, metric_key TEXT, period TEXT, period_type TEXT,
            mean REAL, min_val REAL, max_val REAL, sample_count INTEGER, fail_count INTEGER DEFAULT 0,
            PRIMARY KEY (task_ref, metric_key, period, period_type)
        );",
    )?;

    Ok(conn)
}

// ── Snapshot store ──────────────────────────────────────────────────

pub fn store_snapshot(conn: &Connection, task_ref: &str, key: &str, value: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR REPLACE INTO snapshots (task_ref, key, value, created) VALUES (?1, ?2, ?3, ?4)",
        params![task_ref, key, value, now],
    )?;
    Ok(())
}

pub fn get_snapshot(conn: &Connection, task_ref: &str, key: &str) -> Result<Option<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT value, created FROM snapshots WHERE task_ref = ?1 AND key = ?2",
    )?;
    let result = stmt.query_row(params![task_ref, key], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    });
    match result {
        Ok(pair) => Ok(Some(pair)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn delete_snapshot(conn: &Connection, task_ref: &str, key: &str) -> Result<bool> {
    let count = conn.execute(
        "DELETE FROM snapshots WHERE task_ref = ?1 AND key = ?2",
        params![task_ref, key],
    )?;
    Ok(count > 0)
}

pub fn list_snapshots(conn: &Connection, task_ref: Option<&str>) -> Result<Vec<(String, String, String)>> {
    let mut rows_out = Vec::new();
    if let Some(tr) = task_ref {
        let mut stmt = conn.prepare(
            "SELECT task_ref, key, created FROM snapshots WHERE task_ref = ?1 ORDER BY task_ref, key",
        )?;
        let rows = stmt.query_map(params![tr], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;
        for r in rows { rows_out.push(r?); }
    } else {
        let mut stmt = conn.prepare(
            "SELECT task_ref, key, created FROM snapshots ORDER BY task_ref, key",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;
        for r in rows { rows_out.push(r?); }
    }
    Ok(rows_out)
}

/// Get the current username from the environment.
pub fn current_username() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Get the current hostname.
pub fn current_hostname() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string())
}

pub fn insert_run_log(conn: &Connection, log: &RunLog) -> Result<()> {
    insert_run_log_with_source(conn, log, "")
}

pub fn insert_run_log_with_source(conn: &Connection, log: &RunLog, source: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    tx.execute(
        "INSERT INTO runs (id, task_ref, started, ended, exit_code, username, hostname, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            log.id,
            log.task_ref,
            log.started.to_rfc3339(),
            log.ended.map(|e| e.to_rfc3339()),
            log.exit_code,
            current_username(),
            current_hostname(),
            source,
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
            captured_vars: std::collections::HashMap::new(),
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

/// Fetch run summaries for all tasks in a single query (batch version of get_run_summary).
pub fn get_all_run_summaries(conn: &Connection) -> Result<HashMap<String, RunSummary>> {
    let mut stmt = conn.prepare(
        "SELECT task_ref, exit_code, started, ended FROM runs ORDER BY started DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i32>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;

    // Group by task_ref, keeping at most 100 entries per task (matching get_run_summary behavior)
    let mut per_task: HashMap<String, Vec<(i32, String, Option<String>)>> = HashMap::new();
    for r in rows {
        let (task_ref, exit_code, started, ended) = r?;
        let entries = per_task.entry(task_ref).or_default();
        if entries.len() < 100 {
            entries.push((exit_code, started, ended));
        }
    }

    let mut result = HashMap::new();
    for (task_ref, entries) in per_task {
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

        result.insert(task_ref, RunSummary {
            last_success,
            last_failure,
            fail_count,
            last_duration_ms,
        });
    }

    Ok(result)
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

    // Also rotate memory tables
    let _ = crate::core::memory::rotate_memory(conn, retention_days);

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

/// Update task_ref in run history after a rename.
pub fn rename_task_ref(conn: &Connection, old_ref: &str, new_ref: &str) -> Result<usize> {
    let count = conn.execute(
        "UPDATE runs SET task_ref = ?1 WHERE task_ref = ?2",
        params![new_ref, old_ref],
    )?;
    Ok(count)
}

/// Returns (task_ref, run_count_last_30d) for all tasks that have any runs.
pub fn get_task_heat(conn: &Connection) -> Result<HashMap<String, u32>> {
    let cutoff = (Utc::now() - chrono::Duration::days(30))
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string();
    let mut stmt = conn.prepare(
        "SELECT task_ref, COUNT(*) as cnt FROM runs WHERE started >= ?1 GROUP BY task_ref",
    )?;
    let mut map = HashMap::new();
    let rows = stmt.query_map(params![cutoff], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
    })?;
    for r in rows {
        let (task_ref, cnt) = r?;
        map.insert(task_ref, cnt);
    }
    Ok(map)
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
            captured_vars: std::collections::HashMap::new(),
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
            captured_vars: std::collections::HashMap::new(),
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
    fn test_snapshot_store_and_get() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("history.db");
        let conn = open_db(&db_path).unwrap();

        store_snapshot(&conn, "sbom.sh/check", "baseline", r#"{"status":"200"}"#).unwrap();
        let result = get_snapshot(&conn, "sbom.sh/check", "baseline").unwrap();
        assert!(result.is_some());
        let (val, _created) = result.unwrap();
        assert_eq!(val, r#"{"status":"200"}"#);
    }

    #[test]
    fn test_snapshot_upsert() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("history.db");
        let conn = open_db(&db_path).unwrap();

        store_snapshot(&conn, "t/a", "k", "v1").unwrap();
        store_snapshot(&conn, "t/a", "k", "v2").unwrap();
        let (val, _) = get_snapshot(&conn, "t/a", "k").unwrap().unwrap();
        assert_eq!(val, "v2");
    }

    #[test]
    fn test_snapshot_delete() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("history.db");
        let conn = open_db(&db_path).unwrap();

        store_snapshot(&conn, "t/a", "k", "v").unwrap();
        assert!(delete_snapshot(&conn, "t/a", "k").unwrap());
        assert!(!delete_snapshot(&conn, "t/a", "k").unwrap());
        assert!(get_snapshot(&conn, "t/a", "k").unwrap().is_none());
    }

    #[test]
    fn test_snapshot_list() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("history.db");
        let conn = open_db(&db_path).unwrap();

        store_snapshot(&conn, "a/1", "x", "v1").unwrap();
        store_snapshot(&conn, "a/1", "y", "v2").unwrap();
        store_snapshot(&conn, "b/2", "x", "v3").unwrap();

        let all = list_snapshots(&conn, None).unwrap();
        assert_eq!(all.len(), 3);

        let filtered = list_snapshots(&conn, Some("a/1")).unwrap();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_snapshot_get_missing() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("history.db");
        let conn = open_db(&db_path).unwrap();

        assert!(get_snapshot(&conn, "nope", "nope").unwrap().is_none());
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

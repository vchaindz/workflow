use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::Serialize;
use statrs::statistics::{Data, Distribution, OrderStatistics};

use crate::core::compare::extract_metrics;
use crate::core::models::{RunLog, StepStatus};
use crate::error::Result;

// ── Types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum AnomalyType {
    DurationSpike,
    NewFailure,
    MetricShift,
    OutputDrift,
    Flapping,
}

impl AnomalyType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DurationSpike => "duration_spike",
            Self::NewFailure => "new_failure",
            Self::MetricShift => "metric_shift",
            Self::OutputDrift => "output_drift",
            Self::Flapping => "flapping",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "duration_spike" => Self::DurationSpike,
            "new_failure" => Self::NewFailure,
            "metric_shift" => Self::MetricShift,
            "output_drift" => Self::OutputDrift,
            "flapping" => Self::Flapping,
            _ => Self::DurationSpike,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "warning" => Self::Warning,
            "critical" => Self::Critical,
            _ => Self::Info,
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Anomaly {
    pub run_id: String,
    pub task_ref: String,
    pub step_id: String,
    pub anomaly_type: AnomalyType,
    pub severity: Severity,
    pub description: String,
    pub metric_key: Option<String>,
    pub expected: Option<f64>,
    pub actual: Option<f64>,
    pub z_score: Option<f64>,
    pub detected: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Baseline {
    pub task_ref: String,
    pub step_id: String,
    pub metric_key: String,
    pub mean: f64,
    pub stddev: f64,
    pub median: f64,
    pub min_val: f64,
    pub max_val: f64,
    pub p95: f64,
    pub sample_count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrendPoint {
    pub period: String,
    pub mean: f64,
    pub min_val: f64,
    pub max_val: f64,
    pub sample_count: u32,
    pub fail_count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskMemory {
    pub task_ref: String,
    pub baselines: Vec<Baseline>,
    pub recent_anomalies: Vec<Anomaly>,
    pub duration_trend: Vec<TrendPoint>,
    pub health_score: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct PostRunAnalysis {
    pub anomalies: Vec<Anomaly>,
    pub baselines_updated: bool,
    pub health_score: u8,
    pub summary: String,
}

// ── Statistics helpers ──────────────────────────────────────────────

/// Median absolute deviation (robust alternative to stddev).
fn mad(sorted: &[f64]) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let mut data = Data::new(sorted.to_vec());
    let med = data.median();
    let mut deviations: Vec<f64> = sorted.iter().map(|v| (v - med).abs()).collect();
    deviations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut dev_data = Data::new(deviations);
    dev_data.median() * 1.4826 // consistency constant for normal distribution
}

/// Modified z-score using MAD (more robust to outliers than standard z-score).
fn modified_z_score(value: f64, sorted: &[f64]) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let mut data = Data::new(sorted.to_vec());
    let med = data.median();
    let m = mad(sorted);
    if m < f64::EPSILON {
        // All values are identical — any deviation is infinite
        if (value - med).abs() < f64::EPSILON {
            0.0
        } else {
            10.0 // arbitrary large z-score
        }
    } else {
        (value - med) / m
    }
}

fn severity_from_z(z: f64) -> Option<Severity> {
    let az = z.abs();
    if az > 3.0 {
        Some(Severity::Critical)
    } else if az > 2.0 {
        Some(Severity::Warning)
    } else if az > 1.5 {
        Some(Severity::Info)
    } else {
        None
    }
}

/// Detect pass/fail oscillation: ≥3 transitions in the last `window` exit codes.
fn detect_flapping(exit_codes: &[i32], window: usize) -> bool {
    let w = &exit_codes[..window.min(exit_codes.len())];
    if w.len() < 4 {
        return false;
    }
    let transitions = w.windows(2).filter(|p| (p[0] == 0) != (p[1] == 0)).count();
    transitions >= 3
}

/// Success rate over a slice of exit codes.
fn success_rate(exit_codes: &[i32]) -> f64 {
    if exit_codes.is_empty() {
        return 1.0;
    }
    exit_codes.iter().filter(|&&c| c == 0).count() as f64 / exit_codes.len() as f64
}

/// Simple FNV-1a hash for output fingerprinting (not cryptographic).
fn output_fingerprint(output: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in output.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

// ── DB operations ───────────────────────────────────────────────────

pub fn insert_metrics(
    conn: &Connection,
    run_id: &str,
    task_ref: &str,
    step_id: &str,
    metric_key: &str,
    value: f64,
    unit: &str,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR REPLACE INTO memory_metrics (run_id, task_ref, step_id, metric_key, value, unit, recorded)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![run_id, task_ref, step_id, metric_key, value, unit, now],
    )?;
    Ok(())
}

pub fn upsert_baseline(conn: &Connection, b: &Baseline) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR REPLACE INTO memory_baselines
         (task_ref, step_id, metric_key, mean, stddev, median, min_val, max_val, p95, sample_count, window_start, updated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            b.task_ref, b.step_id, b.metric_key,
            b.mean, b.stddev, b.median, b.min_val, b.max_val, b.p95,
            b.sample_count, now, now
        ],
    )?;
    Ok(())
}

pub fn insert_anomaly(conn: &Connection, a: &Anomaly) -> Result<()> {
    conn.execute(
        "INSERT INTO memory_anomalies
         (run_id, task_ref, step_id, anomaly_type, severity, description, metric_key, expected_value, actual_value, z_score, detected)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            a.run_id, a.task_ref, a.step_id,
            a.anomaly_type.as_str(), a.severity.as_str(), a.description,
            a.metric_key, a.expected, a.actual, a.z_score,
            a.detected.to_rfc3339()
        ],
    )?;
    Ok(())
}

fn load_baselines(conn: &Connection, task_ref: &str) -> Result<Vec<Baseline>> {
    let mut stmt = conn.prepare(
        "SELECT task_ref, step_id, metric_key, mean, stddev, median, min_val, max_val, p95, sample_count
         FROM memory_baselines WHERE task_ref = ?1",
    )?;
    let rows = stmt.query_map(params![task_ref], |row| {
        Ok(Baseline {
            task_ref: row.get(0)?,
            step_id: row.get(1)?,
            metric_key: row.get(2)?,
            mean: row.get(3)?,
            stddev: row.get(4)?,
            median: row.get(5)?,
            min_val: row.get(6)?,
            max_val: row.get(7)?,
            p95: row.get(8)?,
            sample_count: row.get(9)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Load historical metric values for a specific task/step/metric.
fn load_metric_history(
    conn: &Connection,
    task_ref: &str,
    step_id: &str,
    metric_key: &str,
    limit: usize,
) -> Result<Vec<f64>> {
    let mut stmt = conn.prepare(
        "SELECT value FROM memory_metrics
         WHERE task_ref = ?1 AND step_id = ?2 AND metric_key = ?3
         ORDER BY recorded DESC LIMIT ?4",
    )?;
    let rows = stmt.query_map(params![task_ref, step_id, metric_key, limit as i64], |row| {
        row.get::<_, f64>(0)
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Load recent exit codes for a task (newest first).
fn load_exit_codes(conn: &Connection, task_ref: &str, limit: usize) -> Result<Vec<i32>> {
    let mut stmt = conn.prepare(
        "SELECT exit_code FROM runs WHERE task_ref = ?1 ORDER BY started DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![task_ref, limit as i64], |row| {
        row.get::<_, i32>(0)
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Load recent step outputs for fingerprinting (newest first).
fn load_step_outputs(
    conn: &Connection,
    task_ref: &str,
    step_id: &str,
    limit: usize,
) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT s.output FROM steps s
         INNER JOIN runs r ON r.id = s.run_id
         WHERE r.task_ref = ?1 AND s.step_id = ?2
         ORDER BY r.started DESC LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![task_ref, step_id, limit as i64], |row| {
        row.get::<_, String>(0)
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

// ── Public query functions ──────────────────────────────────────────

pub fn get_anomalies(
    conn: &Connection,
    task_ref: Option<&str>,
    min_severity: Severity,
    limit: usize,
) -> Result<Vec<Anomaly>> {
    let sql = if task_ref.is_some() {
        "SELECT run_id, task_ref, step_id, anomaly_type, severity, description,
                metric_key, expected_value, actual_value, z_score, detected
         FROM memory_anomalies WHERE task_ref = ?1 AND acknowledged = 0
         ORDER BY detected DESC LIMIT ?2"
    } else {
        "SELECT run_id, task_ref, step_id, anomaly_type, severity, description,
                metric_key, expected_value, actual_value, z_score, detected
         FROM memory_anomalies WHERE acknowledged = 0
         ORDER BY detected DESC LIMIT ?1"
    };

    let mut stmt = conn.prepare(sql)?;
    let rows = if let Some(tr) = task_ref {
        stmt.query_map(params![tr, limit as i64], row_to_anomaly)?
    } else {
        stmt.query_map(params![limit as i64], row_to_anomaly)?
    };

    let mut out = Vec::new();
    for r in rows {
        let a = r?;
        if a.severity >= min_severity {
            out.push(a);
        }
    }
    Ok(out)
}

fn row_to_anomaly(row: &rusqlite::Row) -> rusqlite::Result<Anomaly> {
    let detected_str: String = row.get(10)?;
    let detected = DateTime::parse_from_rfc3339(&detected_str)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    Ok(Anomaly {
        run_id: row.get(0)?,
        task_ref: row.get(1)?,
        step_id: row.get(2)?,
        anomaly_type: AnomalyType::parse(&row.get::<_, String>(3)?),
        severity: Severity::parse(&row.get::<_, String>(4)?),
        description: row.get(5)?,
        metric_key: row.get(6)?,
        expected: row.get(7)?,
        actual: row.get(8)?,
        z_score: row.get(9)?,
        detected,
    })
}

pub fn get_trends(
    conn: &Connection,
    task_ref: &str,
    metric_key: &str,
    days: u32,
) -> Result<Vec<TrendPoint>> {
    let cutoff = Utc::now() - chrono::Duration::days(days as i64);
    let cutoff_str = cutoff.to_rfc3339();

    let mut stmt = conn.prepare(
        "SELECT substr(recorded, 1, 10) as day, AVG(value), MIN(value), MAX(value), COUNT(*)
         FROM memory_metrics
         WHERE task_ref = ?1 AND metric_key = ?2 AND recorded > ?3
         GROUP BY day ORDER BY day",
    )?;
    let rows = stmt.query_map(params![task_ref, metric_key, cutoff_str], |row| {
        Ok(TrendPoint {
            period: row.get(0)?,
            mean: row.get(1)?,
            min_val: row.get(2)?,
            max_val: row.get(3)?,
            sample_count: row.get::<_, u32>(4)?,
            fail_count: 0,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get_health_scores(conn: &Connection) -> Result<HashMap<String, u8>> {
    let cutoff = Utc::now() - chrono::Duration::days(30);
    let cutoff_str = cutoff.to_rfc3339();

    let mut stmt = conn.prepare(
        "SELECT task_ref, severity, COUNT(*) FROM memory_anomalies
         WHERE detected > ?1 AND acknowledged = 0
         GROUP BY task_ref, severity",
    )?;
    let rows = stmt.query_map(params![cutoff_str], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, u32>(2)?))
    })?;

    let mut scores: HashMap<String, i32> = HashMap::new();
    for r in rows {
        let (task_ref, sev, count) = r?;
        let penalty = match Severity::parse(&sev) {
            Severity::Critical => 25,
            Severity::Warning => 10,
            Severity::Info => 2,
        };
        *scores.entry(task_ref).or_insert(100) -= penalty * count as i32;
    }

    Ok(scores.into_iter().map(|(k, v)| (k, v.clamp(0, 100) as u8)).collect())
}

pub fn get_task_memory(conn: &Connection, task_ref: &str) -> Result<TaskMemory> {
    let baselines = load_baselines(conn, task_ref)?;
    let recent_anomalies = get_anomalies(conn, Some(task_ref), Severity::Info, 10)?;
    let duration_trend = get_trends(conn, task_ref, "duration_ms", 30)?;
    let health_scores = get_health_scores(conn)?;
    let health_score = health_scores.get(task_ref).copied().unwrap_or(100);

    Ok(TaskMemory {
        task_ref: task_ref.to_string(),
        baselines,
        recent_anomalies,
        duration_trend,
        health_score,
    })
}

pub fn acknowledge_anomaly(conn: &Connection, id: i64) -> Result<bool> {
    let count = conn.execute(
        "UPDATE memory_anomalies SET acknowledged = 1 WHERE id = ?1",
        params![id],
    )?;
    Ok(count > 0)
}

pub fn acknowledge_all(conn: &Connection, task_ref: &str) -> Result<u32> {
    let count = conn.execute(
        "UPDATE memory_anomalies SET acknowledged = 1 WHERE task_ref = ?1 AND acknowledged = 0",
        params![task_ref],
    )?;
    Ok(count as u32)
}

// ── Core analysis ───────────────────────────────────────────────────

/// Run post-execution analysis: extract metrics, detect anomalies, update baselines.
pub fn analyze_post_run(conn: &Connection, run_log: &RunLog) -> Result<PostRunAnalysis> {
    let task_ref = &run_log.task_ref;
    let run_id = &run_log.id;
    let mut anomalies = Vec::new();
    let now = Utc::now();

    // 1. Extract and store metrics for each step
    let total_duration_ms: u64 = run_log.steps.iter().map(|s| s.duration_ms).sum();
    insert_metrics(conn, run_id, task_ref, "__total__", "duration_ms", total_duration_ms as f64, "ms")?;

    for step in &run_log.steps {
        // Duration metric per step
        insert_metrics(conn, run_id, task_ref, &step.id, "duration_ms", step.duration_ms as f64, "ms")?;

        // Extract metrics from step output
        let extracted = extract_metrics(&step.output);
        for (label, value, unit) in &extracted {
            let key = format!("{}:{}", label, unit);
            insert_metrics(conn, run_id, task_ref, &step.id, &key, *value, unit)?;
        }
    }

    // 2. Check sample count — need at least 5 historical data points
    let history = load_metric_history(conn, task_ref, "__total__", "duration_ms", 50)?;
    if history.len() < 5 {
        return Ok(PostRunAnalysis {
            anomalies: Vec::new(),
            baselines_updated: false,
            health_score: 100,
            summary: format!("collecting data ({}/{} runs)", history.len(), 5),
        });
    }

    // 3. Duration anomaly detection (total workflow)
    let mut sorted = history.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let z = modified_z_score(total_duration_ms as f64, &sorted);
    if let Some(sev) = severity_from_z(z) {
        let mut data = Data::new(sorted.clone());
        let med = data.median();
        let m = mad(&sorted);
        anomalies.push(Anomaly {
            run_id: run_id.clone(),
            task_ref: task_ref.clone(),
            step_id: "__total__".to_string(),
            anomaly_type: AnomalyType::DurationSpike,
            severity: sev,
            description: format!(
                "total duration {:.0}ms (baseline {:.0}ms ±{:.0}ms, z={:.1})",
                total_duration_ms, med, m, z
            ),
            metric_key: Some("duration_ms".to_string()),
            expected: Some(med),
            actual: Some(total_duration_ms as f64),
            z_score: Some(z),
            detected: now,
        });
    }

    // 4. Per-step duration anomalies
    for step in &run_log.steps {
        if step.status == StepStatus::Skipped {
            continue;
        }
        let step_hist = load_metric_history(conn, task_ref, &step.id, "duration_ms", 50)?;
        if step_hist.len() < 5 {
            continue;
        }
        let mut step_sorted = step_hist.clone();
        step_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let sz = modified_z_score(step.duration_ms as f64, &step_sorted);
        if let Some(sev) = severity_from_z(sz) {
            let mut sdata = Data::new(step_sorted.clone());
            let smed = sdata.median();
            let sm = mad(&step_sorted);
            anomalies.push(Anomaly {
                run_id: run_id.clone(),
                task_ref: task_ref.clone(),
                step_id: step.id.clone(),
                anomaly_type: AnomalyType::DurationSpike,
                severity: sev,
                description: format!(
                    "step '{}' {:.0}ms (baseline {:.0}ms ±{:.0}ms, z={:.1})",
                    step.id, step.duration_ms, smed, sm, sz
                ),
                metric_key: Some("duration_ms".to_string()),
                expected: Some(smed),
                actual: Some(step.duration_ms as f64),
                z_score: Some(sz),
                detected: now,
            });
        }
    }

    // 5. New failure detection
    let exit_codes = load_exit_codes(conn, task_ref, 30)?;
    if run_log.exit_code != 0 && exit_codes.len() >= 5 {
        let rate = success_rate(&exit_codes[1..]); // exclude current run
        if rate >= 0.9 {
            anomalies.push(Anomaly {
                run_id: run_id.clone(),
                task_ref: task_ref.clone(),
                step_id: "__total__".to_string(),
                anomaly_type: AnomalyType::NewFailure,
                severity: Severity::Critical,
                description: format!(
                    "task failed (historical success rate {:.0}%)",
                    rate * 100.0
                ),
                metric_key: None,
                expected: Some(0.0),
                actual: Some(run_log.exit_code as f64),
                z_score: None,
                detected: now,
            });
        }
    }

    // 6. Flapping detection
    if exit_codes.len() >= 6 && detect_flapping(&exit_codes, 6) {
        anomalies.push(Anomaly {
            run_id: run_id.clone(),
            task_ref: task_ref.clone(),
            step_id: "__total__".to_string(),
            anomaly_type: AnomalyType::Flapping,
            severity: Severity::Warning,
            description: "task is flapping (alternating pass/fail)".to_string(),
            metric_key: None,
            expected: None,
            actual: None,
            z_score: None,
            detected: now,
        });
    }

    // 7. Output drift detection (per step)
    for step in &run_log.steps {
        if step.status != StepStatus::Success || step.output.is_empty() {
            continue;
        }
        let prev_outputs = load_step_outputs(conn, task_ref, &step.id, 5)?;
        if prev_outputs.len() < 3 {
            continue;
        }
        let current_fp = output_fingerprint(&step.output);
        let prev_fps: Vec<u64> = prev_outputs.iter().map(|o| output_fingerprint(o)).collect();
        // If all previous outputs were identical but current differs
        let all_same = prev_fps.windows(2).all(|w| w[0] == w[1]);
        if all_same && !prev_fps.is_empty() && current_fp != prev_fps[0] {
            anomalies.push(Anomaly {
                run_id: run_id.clone(),
                task_ref: task_ref.clone(),
                step_id: step.id.clone(),
                anomaly_type: AnomalyType::OutputDrift,
                severity: Severity::Info,
                description: format!("step '{}' output changed (was stable for {} runs)", step.id, prev_fps.len()),
                metric_key: None,
                expected: None,
                actual: None,
                z_score: None,
                detected: now,
            });
        }
    }

    // 8. Store anomalies
    for a in &anomalies {
        let _ = insert_anomaly(conn, a);
    }

    // 9. Update baselines from history
    let baselines_updated = update_baselines(conn, task_ref).unwrap_or(false);

    // 10. Compute health score
    let health_scores = get_health_scores(conn)?;
    let health_score = health_scores.get(task_ref).copied().unwrap_or(100);

    // 11. Build summary
    let summary = if anomalies.is_empty() {
        "no anomalies".to_string()
    } else {
        let critical = anomalies.iter().filter(|a| a.severity == Severity::Critical).count();
        let warning = anomalies.iter().filter(|a| a.severity == Severity::Warning).count();
        let info = anomalies.iter().filter(|a| a.severity == Severity::Info).count();
        let mut parts = Vec::new();
        if critical > 0 { parts.push(format!("{} critical", critical)); }
        if warning > 0 { parts.push(format!("{} warning", warning)); }
        if info > 0 { parts.push(format!("{} info", info)); }
        parts.join(", ")
    };

    Ok(PostRunAnalysis {
        anomalies,
        baselines_updated,
        health_score,
        summary,
    })
}

/// Recompute baselines for a task from its metric history.
fn update_baselines(conn: &Connection, task_ref: &str) -> Result<bool> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT step_id, metric_key FROM memory_metrics WHERE task_ref = ?1",
    )?;
    let pairs: Vec<(String, String)> = stmt
        .query_map(params![task_ref], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if pairs.is_empty() {
        return Ok(false);
    }

    for (step_id, metric_key) in &pairs {
        let values = load_metric_history(conn, task_ref, step_id, metric_key, 50)?;
        if values.len() < 3 {
            continue;
        }
        let mut sorted = values.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let mut data = Data::new(sorted.clone());
        let mean = data.mean().unwrap_or(0.0);
        let median = data.median();
        let stddev = data.std_dev().unwrap_or(0.0);
        let min_val = sorted.first().copied().unwrap_or(0.0);
        let max_val = sorted.last().copied().unwrap_or(0.0);
        let p95 = data.quantile(0.95);

        let baseline = Baseline {
            task_ref: task_ref.to_string(),
            step_id: step_id.clone(),
            metric_key: metric_key.clone(),
            mean,
            stddev,
            median,
            min_val,
            max_val,
            p95,
            sample_count: values.len() as u32,
        };
        upsert_baseline(conn, &baseline)?;
    }

    Ok(true)
}

/// Recompute baselines for a specific task or all tasks (maintenance).
pub fn recompute_all_baselines(conn: &Connection, task_ref: Option<&str>) -> Result<u32> {
    if let Some(tr) = task_ref {
        update_baselines(conn, tr)?;
        return Ok(1);
    }

    let mut stmt = conn.prepare("SELECT DISTINCT task_ref FROM memory_metrics")?;
    let refs: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();

    let count = refs.len() as u32;
    for tr in &refs {
        let _ = update_baselines(conn, tr);
    }
    Ok(count)
}

// ── Cleanup ─────────────────────────────────────────────────────────

/// Rotate memory tables alongside run rotation.
pub fn rotate_memory(conn: &Connection, retention_days: u32) -> Result<u32> {
    let cutoff = Utc::now() - chrono::Duration::days(retention_days as i64);
    let cutoff_str = cutoff.to_rfc3339();

    let d1 = conn.execute(
        "DELETE FROM memory_metrics WHERE recorded < ?1",
        params![cutoff_str],
    )?;
    let d2 = conn.execute(
        "DELETE FROM memory_anomalies WHERE detected < ?1",
        params![cutoff_str],
    )?;

    Ok((d1 + d2) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modified_z_score_normal() {
        let data = vec![100.0, 102.0, 98.0, 101.0, 99.0, 100.0, 103.0, 97.0];
        let z = modified_z_score(100.0, &data);
        assert!(z.abs() < 1.0, "normal value should have low z-score: {z}");
    }

    #[test]
    fn test_modified_z_score_outlier() {
        let mut data = vec![100.0, 102.0, 98.0, 101.0, 99.0, 100.0, 103.0, 97.0];
        data.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let z = modified_z_score(200.0, &data);
        assert!(z.abs() > 3.0, "outlier should have high z-score: {z}");
    }

    #[test]
    fn test_modified_z_score_identical() {
        let data = vec![100.0, 100.0, 100.0, 100.0];
        let z_same = modified_z_score(100.0, &data);
        assert_eq!(z_same, 0.0);
        let z_diff = modified_z_score(101.0, &data);
        assert!(z_diff > 3.0, "any deviation from identical values should flag: {z_diff}");
    }

    #[test]
    fn test_detect_flapping() {
        assert!(detect_flapping(&[0, 1, 0, 1, 0, 1], 6));
        assert!(!detect_flapping(&[0, 0, 0, 0, 0, 0], 6));
        assert!(!detect_flapping(&[0, 0, 1, 1, 1, 1], 6));
        assert!(!detect_flapping(&[0, 1], 6)); // too few
    }

    #[test]
    fn test_success_rate() {
        assert_eq!(success_rate(&[0, 0, 0, 0, 0]), 1.0);
        assert_eq!(success_rate(&[0, 0, 0, 0, 1]), 0.8);
        assert_eq!(success_rate(&[1, 1, 1, 1, 1]), 0.0);
        assert_eq!(success_rate(&[]), 1.0);
    }

    #[test]
    fn test_output_fingerprint() {
        let fp1 = output_fingerprint("hello world");
        let fp2 = output_fingerprint("hello world");
        let fp3 = output_fingerprint("hello world!");
        assert_eq!(fp1, fp2);
        assert_ne!(fp1, fp3);
    }

    #[test]
    fn test_severity_from_z() {
        assert_eq!(severity_from_z(0.5), None);
        assert_eq!(severity_from_z(1.6), Some(Severity::Info));
        assert_eq!(severity_from_z(2.5), Some(Severity::Warning));
        assert_eq!(severity_from_z(3.5), Some(Severity::Critical));
        assert_eq!(severity_from_z(-3.5), Some(Severity::Critical));
    }
}

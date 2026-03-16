use crate::cli::args::MemoryAction;
use crate::core::config::Config;
use crate::core::db;
use crate::core::memory;
use crate::error::Result;

pub fn cmd_memory(config: &Config, action: MemoryAction) -> Result<()> {
    let conn = db::open_db(&config.db_path())?;

    match action {
        MemoryAction::Anomalies { task, min_severity, limit, json } => {
            let sev = memory::Severity::from_str(&min_severity);
            let anomalies = memory::get_anomalies(&conn, task.as_deref(), sev, limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&anomalies)?);
            } else if anomalies.is_empty() {
                println!("No anomalies found.");
            } else {
                for a in &anomalies {
                    let ts = a.detected.format("%Y-%m-%d %H:%M");
                    println!("[{}] [{}] {} — {}", ts, a.severity, a.task_ref, a.description);
                }
            }
        }

        MemoryAction::Baseline { task, json } => {
            let tm = memory::get_task_memory(&conn, &task)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&tm.baselines)?);
            } else if tm.baselines.is_empty() {
                println!("No baselines yet for '{}'. Run the task a few more times.", task);
            } else {
                println!("Baselines for '{}':", task);
                println!("{:<15} {:<20} {:>10} {:>10} {:>10} {:>10} {:>8}",
                    "step", "metric", "mean", "median", "stddev", "p95", "samples");
                for b in &tm.baselines {
                    println!("{:<15} {:<20} {:>10.1} {:>10.1} {:>10.1} {:>10.1} {:>8}",
                        b.step_id, b.metric_key, b.mean, b.median, b.stddev, b.p95, b.sample_count);
                }
            }
        }

        MemoryAction::Trends { task, metric, days, json } => {
            let trends = memory::get_trends(&conn, &task, &metric, days)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&trends)?);
            } else if trends.is_empty() {
                println!("No trend data for '{}' metric '{}' in last {} days.", task, metric, days);
            } else {
                println!("Trends for '{}' — {} (last {} days):", task, metric, days);
                println!("{:<12} {:>10} {:>10} {:>10} {:>8}", "date", "mean", "min", "max", "runs");
                for t in &trends {
                    println!("{:<12} {:>10.1} {:>10.1} {:>10.1} {:>8}",
                        t.period, t.mean, t.min_val, t.max_val, t.sample_count);
                }
            }
        }

        MemoryAction::Health { json } => {
            let scores = memory::get_health_scores(&conn)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&scores)?);
            } else if scores.is_empty() {
                println!("No health data yet. Run some tasks to build baselines.");
            } else {
                println!("{:<30} {:>6}", "task", "health");
                let mut sorted: Vec<_> = scores.iter().collect();
                sorted.sort_by(|a, b| a.1.cmp(b.1));
                for (task, score) in &sorted {
                    let bar_len = (**score as usize) / 5;
                    let bar: String = "\u{2588}".repeat(bar_len);
                    let color = if **score >= 80 { "\x1b[32m" } else if **score >= 50 { "\x1b[33m" } else { "\x1b[31m" };
                    println!("{:<30} {}{:>3}/100\x1b[0m {}{}\x1b[0m", task, color, score, color, bar);
                }
            }
        }

        MemoryAction::Ack { id, task } => {
            if id == "all" {
                if let Some(tr) = &task {
                    let count = memory::acknowledge_all(&conn, tr)?;
                    println!("Acknowledged {} anomalies for '{}'.", count, tr);
                } else {
                    eprintln!("Error: --task is required when using 'all'");
                }
            } else {
                let num: i64 = id.parse().unwrap_or(-1);
                if num < 0 {
                    eprintln!("Error: invalid anomaly ID '{}'", id);
                } else if memory::acknowledge_anomaly(&conn, num)? {
                    println!("Acknowledged anomaly #{}.", num);
                } else {
                    eprintln!("Anomaly #{} not found.", num);
                }
            }
        }

        MemoryAction::Recompute { task } => {
            let count = memory::recompute_all_baselines(&conn, task.as_deref())?;
            println!("Recomputed baselines for {} task(s).", count);
        }
    }

    Ok(())
}

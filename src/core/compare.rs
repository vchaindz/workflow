use regex::Regex;
use serde::Serialize;
use similar::{ChangeTag, TextDiff};

use crate::core::models::RunLog;

#[derive(Debug, Clone, Serialize)]
pub struct CompareResult {
    pub task_ref: String,
    pub base: RunRef,
    pub current: RunRef,
    pub exit_code_changed: bool,
    pub base_exit: i32,
    pub current_exit: i32,
    pub duration: DurationDelta,
    pub step_comparisons: Vec<StepComparison>,
    pub metrics: Vec<MetricDelta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_analysis: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunRef {
    pub id: String,
    pub started: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepComparison {
    pub step_id: String,
    pub base_status: Option<String>,
    pub current_status: Option<String>,
    pub status_changed: bool,
    pub duration: Option<DurationDelta>,
    pub output_diff: OutputDiff,
}

#[derive(Debug, Clone, Serialize)]
pub struct DurationDelta {
    pub base_ms: u64,
    pub current_ms: u64,
    pub delta_ms: i64,
    pub delta_percent: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum OutputDiff {
    Identical,
    Changed { added: Vec<String>, removed: Vec<String> },
    OnlyInBase { content: String },
    OnlyInCurrent { content: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricDelta {
    pub label: String,
    pub base_value: f64,
    pub current_value: f64,
    pub unit: String,
    pub delta: f64,
    pub delta_percent: f64,
}

pub fn compare_runs(base: &RunLog, current: &RunLog) -> CompareResult {
    let task_ref = current.task_ref.clone();

    let base_total_ms: u64 = base.steps.iter().map(|s| s.duration_ms).sum();
    let current_total_ms: u64 = current.steps.iter().map(|s| s.duration_ms).sum();

    // Match steps by id
    let mut step_comparisons = Vec::new();
    let mut all_step_ids: Vec<String> = Vec::new();

    for s in &base.steps {
        if !all_step_ids.contains(&s.id) {
            all_step_ids.push(s.id.clone());
        }
    }
    for s in &current.steps {
        if !all_step_ids.contains(&s.id) {
            all_step_ids.push(s.id.clone());
        }
    }

    for step_id in &all_step_ids {
        let base_step = base.steps.iter().find(|s| &s.id == step_id);
        let cur_step = current.steps.iter().find(|s| &s.id == step_id);

        let (base_status, current_status) = (
            base_step.map(|s| format!("{:?}", s.status)),
            cur_step.map(|s| format!("{:?}", s.status)),
        );

        let status_changed = base_status != current_status;

        let duration = match (base_step, cur_step) {
            (Some(b), Some(c)) => Some(make_duration_delta(b.duration_ms, c.duration_ms)),
            _ => None,
        };

        let output_diff = match (base_step, cur_step) {
            (Some(b), Some(c)) => diff_output(&b.output, &c.output),
            (Some(b), None) => OutputDiff::OnlyInBase { content: truncate(&b.output, 500) },
            (None, Some(c)) => OutputDiff::OnlyInCurrent { content: truncate(&c.output, 500) },
            (None, None) => OutputDiff::Identical,
        };

        step_comparisons.push(StepComparison {
            step_id: step_id.clone(),
            base_status,
            current_status,
            status_changed,
            duration,
            output_diff,
        });
    }

    // Extract and compare metrics from all step outputs
    let base_all_output: String = base.steps.iter().map(|s| s.output.as_str()).collect::<Vec<_>>().join("\n");
    let cur_all_output: String = current.steps.iter().map(|s| s.output.as_str()).collect::<Vec<_>>().join("\n");
    let metrics = compare_metrics(&base_all_output, &cur_all_output);

    CompareResult {
        task_ref,
        base: RunRef {
            id: base.id.clone(),
            started: base.started.format("%Y-%m-%d %H:%M:%S").to_string(),
        },
        current: RunRef {
            id: current.id.clone(),
            started: current.started.format("%Y-%m-%d %H:%M:%S").to_string(),
        },
        exit_code_changed: base.exit_code != current.exit_code,
        base_exit: base.exit_code,
        current_exit: current.exit_code,
        duration: make_duration_delta(base_total_ms, current_total_ms),
        step_comparisons,
        metrics,
        ai_analysis: None,
    }
}

fn make_duration_delta(base_ms: u64, current_ms: u64) -> DurationDelta {
    let delta_ms = current_ms as i64 - base_ms as i64;
    let delta_percent = if base_ms == 0 {
        if current_ms == 0 { 0.0 } else { 100.0 }
    } else {
        (delta_ms as f64 / base_ms as f64) * 100.0
    };
    DurationDelta { base_ms, current_ms, delta_ms, delta_percent }
}

pub fn diff_output(base: &str, current: &str) -> OutputDiff {
    let base = base.trim();
    let current = current.trim();

    if base == current {
        return OutputDiff::Identical;
    }

    let diff = TextDiff::from_lines(base, current);
    let mut added = Vec::new();
    let mut removed = Vec::new();

    for change in diff.iter_all_changes() {
        let line = change.value().trim_end().to_string();
        match change.tag() {
            ChangeTag::Insert => added.push(line),
            ChangeTag::Delete => removed.push(line),
            ChangeTag::Equal => {}
        }
    }

    if added.is_empty() && removed.is_empty() {
        OutputDiff::Identical
    } else {
        OutputDiff::Changed { added, removed }
    }
}

pub fn extract_metrics(output: &str) -> Vec<(String, f64, String)> {
    let mut metrics = Vec::new();

    // Pattern: number followed by unit
    let re_sized = Regex::new(r"(\d+(?:\.\d+)?)\s*(MB|GB|KB|bytes|ms|s|min|%|MiB|GiB)").unwrap();
    for cap in re_sized.captures_iter(output) {
        let value: f64 = cap[1].parse().unwrap_or(0.0);
        let unit = cap[2].to_string();
        // Use surrounding context as label
        let start = cap.get(0).unwrap().start();
        let label_start = output[..start].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let label = output[label_start..start].trim().trim_end_matches(':').trim().to_string();
        let label = if label.is_empty() { format!("value ({})", unit) } else { label };
        metrics.push((label, value, unit));
    }

    // Pattern: number followed by entity name
    let re_counted = Regex::new(r"(\d+)\s+(containers?|images?|errors?|warnings?|services?)").unwrap();
    for cap in re_counted.captures_iter(output) {
        let value: f64 = cap[1].parse().unwrap_or(0.0);
        let entity = cap[2].to_string();
        metrics.push((entity.clone(), value, "count".to_string()));
    }

    metrics
}

fn compare_metrics(base_output: &str, current_output: &str) -> Vec<MetricDelta> {
    let base_metrics = extract_metrics(base_output);
    let cur_metrics = extract_metrics(current_output);

    let mut deltas = Vec::new();

    // Match metrics by label
    for (bl, bv, bu) in &base_metrics {
        if let Some((_, cv, _)) = cur_metrics.iter().find(|(cl, _, cu)| cl == bl && cu == bu) {
            if (bv - cv).abs() > f64::EPSILON {
                let delta = cv - bv;
                let delta_percent = if bv.abs() < f64::EPSILON { 100.0 } else { (delta / bv) * 100.0 };
                deltas.push(MetricDelta {
                    label: bl.clone(),
                    base_value: *bv,
                    current_value: *cv,
                    unit: bu.clone(),
                    delta,
                    delta_percent,
                });
            }
        }
    }

    deltas
}

pub fn format_compare(result: &CompareResult, use_color: bool) -> String {
    let mut out = String::new();

    out.push_str(&format!("Compare: {}\n", result.task_ref));
    out.push_str(&format!("  Previous: {} @ {}\n", short_id(&result.base.id), result.base.started));
    out.push_str(&format!("  Current:  {} @ {}\n\n", short_id(&result.current.id), result.current.started));

    // Exit code
    if result.exit_code_changed {
        let label = if use_color {
            format!("Exit code: {} -> {}  \x1b[31mREGRESSION\x1b[0m", result.base_exit, result.current_exit)
        } else {
            format!("Exit code: {} -> {}  REGRESSION", result.base_exit, result.current_exit)
        };
        out.push_str(&label);
        out.push('\n');
    } else {
        out.push_str(&format!("Exit code: {} (unchanged)\n", result.current_exit));
    }
    out.push('\n');

    // Steps
    for sc in &result.step_comparisons {
        let base_s = sc.base_status.as_deref().unwrap_or("(absent)");
        let cur_s = sc.current_status.as_deref().unwrap_or("(absent)");

        if sc.status_changed {
            out.push_str(&format!("  {}: {} -> {}", sc.step_id, base_s, cur_s));
        } else {
            out.push_str(&format!("  {}: {}", sc.step_id, cur_s));
        }

        if let Some(ref dur) = sc.duration {
            let sign = if dur.delta_ms >= 0 { "+" } else { "" };
            out.push_str(&format!("  ({}{}ms, {}{:.1}%)", sign, dur.delta_ms, sign, dur.delta_percent));
        }
        out.push('\n');

        match &sc.output_diff {
            OutputDiff::Changed { added, removed } => {
                for line in removed.iter().take(5) {
                    out.push_str(&format!("    - {}\n", line));
                }
                for line in added.iter().take(5) {
                    out.push_str(&format!("    + {}\n", line));
                }
                let total = added.len() + removed.len();
                if total > 10 {
                    out.push_str(&format!("    ... {} more diff lines\n", total - 10));
                }
            }
            OutputDiff::OnlyInBase { .. } => {
                out.push_str("    (step only in previous run)\n");
            }
            OutputDiff::OnlyInCurrent { .. } => {
                out.push_str("    (step only in current run)\n");
            }
            OutputDiff::Identical => {}
        }
    }

    // Metrics
    if !result.metrics.is_empty() {
        out.push_str("\nMetrics:\n");
        for m in &result.metrics {
            let sign = if m.delta >= 0.0 { "+" } else { "" };
            out.push_str(&format!(
                "  {}: {} {} -> {} {}  ({}{} {}, {}{:.1}%)\n",
                m.label, m.base_value, m.unit, m.current_value, m.unit,
                sign, m.delta, m.unit, sign, m.delta_percent,
            ));
        }
    }

    // Total duration
    let dur = &result.duration;
    let sign = if dur.delta_ms >= 0 { "+" } else { "" };
    out.push_str(&format!(
        "\nDuration: {}ms -> {}ms  ({}{:.1}%)\n",
        dur.base_ms, dur.current_ms, sign, dur.delta_percent,
    ));

    // AI analysis
    if let Some(ref analysis) = result.ai_analysis {
        out.push_str(&format!("\nAI Analysis:\n{}\n", analysis));
    }

    out
}

fn short_id(id: &str) -> &str {
    if id.len() > 8 { &id[..8] } else { id }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

pub fn build_ai_prompt(base: &RunLog, current: &RunLog) -> String {
    let mut prompt = String::from(
        "Compare these two workflow runs and provide a brief analysis (3-5 sentences).\n\
         Focus on: status regressions, performance changes, error patterns.\n\n"
    );

    prompt.push_str("=== PREVIOUS RUN ===\n");
    prompt.push_str(&format!("Task: {} | Exit: {} | {}\n", base.task_ref, base.exit_code, base.started));
    for step in &base.steps {
        let out = truncate(&step.output, 500);
        prompt.push_str(&format!("{}: {:?} ({}ms)\n{}\n", step.id, step.status, step.duration_ms, out));
    }

    prompt.push_str("\n=== LATEST RUN ===\n");
    prompt.push_str(&format!("Task: {} | Exit: {} | {}\n", current.task_ref, current.exit_code, current.started));
    for step in &current.steps {
        let out = truncate(&step.output, 500);
        prompt.push_str(&format!("{}: {:?} ({}ms)\n{}\n", step.id, step.status, step.duration_ms, out));
    }

    prompt.push_str("\nWhat changed? Is anything concerning?\n");
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{RunLog, StepResult, StepStatus};
    use chrono::Utc;

    fn make_run(id: &str, exit: i32, steps: Vec<StepResult>) -> RunLog {
        RunLog {
            id: id.to_string(),
            task_ref: "backup/db-full".to_string(),
            started: Utc::now(),
            ended: Some(Utc::now()),
            steps,
            exit_code: exit,
            captured_vars: std::collections::HashMap::new(),
        }
    }

    fn make_step(id: &str, status: StepStatus, ms: u64, output: &str) -> StepResult {
        StepResult {
            id: id.to_string(),
            status,
            output: output.to_string(),
            duration_ms: ms,
        }
    }

    #[test]
    fn test_compare_identical_runs() {
        let steps = vec![make_step("check", StepStatus::Success, 100, "ok")];
        let base = make_run("run-1", 0, steps.clone());
        let current = make_run("run-2", 0, steps);

        let result = compare_runs(&base, &current);
        assert!(!result.exit_code_changed);
        assert_eq!(result.step_comparisons.len(), 1);
        assert!(!result.step_comparisons[0].status_changed);
    }

    #[test]
    fn test_compare_regression() {
        let base = make_run("run-1", 0, vec![
            make_step("check", StepStatus::Success, 100, "healthy: true"),
        ]);
        let current = make_run("run-2", 1, vec![
            make_step("check", StepStatus::Failed, 270, "healthy: false"),
        ]);

        let result = compare_runs(&base, &current);
        assert!(result.exit_code_changed);
        assert!(result.step_comparisons[0].status_changed);
    }

    #[test]
    fn test_diff_output_identical() {
        let diff = diff_output("hello\nworld", "hello\nworld");
        assert!(matches!(diff, OutputDiff::Identical));
    }

    #[test]
    fn test_diff_output_changed() {
        let diff = diff_output("line1\nline2", "line1\nline3");
        match diff {
            OutputDiff::Changed { added, removed } => {
                assert_eq!(removed, vec!["line2"]);
                assert_eq!(added, vec!["line3"]);
            }
            _ => panic!("expected Changed"),
        }
    }

    #[test]
    fn test_extract_metrics_sized() {
        let output = "image size: 245 MB\nduration: 3.5 s";
        let metrics = extract_metrics(output);
        assert!(metrics.iter().any(|(_, v, u)| *v == 245.0 && u == "MB"));
        assert!(metrics.iter().any(|(_, v, u)| *v == 3.5 && u == "s"));
    }

    #[test]
    fn test_extract_metrics_counted() {
        let output = "found 12 containers running\n3 errors detected";
        let metrics = extract_metrics(output);
        assert!(metrics.iter().any(|(_, v, _)| *v == 12.0));
        assert!(metrics.iter().any(|(_, v, _)| *v == 3.0));
    }

    #[test]
    fn test_format_compare_basic() {
        let base = make_run("run-aaa111", 0, vec![
            make_step("check", StepStatus::Success, 150, "ok"),
        ]);
        let current = make_run("run-bbb222", 1, vec![
            make_step("check", StepStatus::Failed, 320, "error"),
        ]);

        let result = compare_runs(&base, &current);
        let formatted = format_compare(&result, false);
        assert!(formatted.contains("REGRESSION"));
        assert!(formatted.contains("check"));
        assert!(formatted.contains("Duration:"));
    }

    #[test]
    fn test_duration_delta_zero_base() {
        let d = make_duration_delta(0, 100);
        assert_eq!(d.delta_ms, 100);
        assert_eq!(d.delta_percent, 100.0);
    }

    #[test]
    fn test_build_ai_prompt() {
        let base = make_run("r1", 0, vec![make_step("s1", StepStatus::Success, 100, "ok")]);
        let current = make_run("r2", 1, vec![make_step("s1", StepStatus::Failed, 200, "err")]);
        let prompt = build_ai_prompt(&base, &current);
        assert!(prompt.contains("PREVIOUS RUN"));
        assert!(prompt.contains("LATEST RUN"));
        assert!(prompt.contains("What changed?"));
    }
}

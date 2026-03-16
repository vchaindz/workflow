use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::core::models::{RunLog, StepStatus};

use super::app::{App};

/// Simple line-level diff between old and new text.
#[derive(Debug)]
pub(super) enum DiffLine {
    Same(String),
    Added(String),
    Removed(String),
}

pub(super) fn simple_line_diff(old: &str, new: &str) -> Vec<DiffLine> {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let (n, m) = (old_lines.len(), new_lines.len());

    // Build LCS table
    let mut dp = vec![vec![0u16; m + 1]; n + 1];
    for i in 1..=n {
        for j in 1..=m {
            dp[i][j] = if old_lines[i - 1] == new_lines[j - 1] {
                dp[i - 1][j - 1] + 1
            } else {
                dp[i - 1][j].max(dp[i][j - 1])
            };
        }
    }

    // Backtrack
    let mut result = Vec::new();
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && old_lines[i - 1] == new_lines[j - 1] {
            result.push(DiffLine::Same(old_lines[i - 1].to_string()));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            result.push(DiffLine::Added(new_lines[j - 1].to_string()));
            j -= 1;
        } else {
            result.push(DiffLine::Removed(old_lines[i - 1].to_string()));
            i -= 1;
        }
    }
    result.reverse();
    result
}

pub(super) fn format_live_progress_styled(app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(ref task_ref) = app.executing_task_ref {
        lines.push(Line::from(vec![
            Span::styled("Running: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(task_ref.clone(), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(""));
    }

    if app.step_states.is_empty() {
        lines.push(Line::from(Span::styled("Preparing...", Style::default().fg(Color::DarkGray))));
        return lines;
    }

    for (i, state) in app.step_states.iter().enumerate() {
        let (icon, icon_color) = match state.status {
            StepStatus::Running => ("▶", Color::Yellow),
            StepStatus::Success => ("✓", Color::Green),
            StepStatus::Failed => ("✗", Color::Red),
            StepStatus::Skipped => ("⊘", Color::DarkGray),
            StepStatus::Timedout => ("⏱", Color::Yellow),
            StepStatus::Interactive => ("⇄", Color::Cyan),
            StepStatus::Pending => ("·", Color::DarkGray),
        };

        let duration = match state.duration_ms {
            Some(ms) if ms >= 1000 => format!(" ({:.1}s)", ms as f64 / 1000.0),
            Some(ms) => format!(" ({}ms)", ms),
            None if state.status == StepStatus::Running => " ...".to_string(),
            None => String::new(),
        };

        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(icon, Style::default().fg(icon_color)),
            Span::raw(" "),
            Span::styled(format!("{}.", i + 1), Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled(state.id.clone(), Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(duration, Style::default().fg(Color::DarkGray)),
        ]));

        if !state.cmd_preview.is_empty() && matches!(state.status, StepStatus::Running | StepStatus::Failed | StepStatus::Timedout) {
            let cmd_color = if matches!(state.status, StepStatus::Failed | StepStatus::Timedout) {
                Color::Red
            } else {
                Color::DarkGray
            };
            lines.push(Line::from(vec![
                Span::raw("       "),
                Span::styled("$ ", Style::default().fg(Color::Green)),
                Span::styled(state.cmd_preview.replace('\t', "  "), Style::default().fg(cmd_color)),
            ]));
        }

        if let Some(ref output) = state.last_output {
            if !output.trim().is_empty() && state.status != StepStatus::Pending {
                let truncated: String = output.chars().take(80).collect();
                lines.push(Line::from(vec![
                    Span::raw("       "),
                    colorize_output_line(&truncated),
                ]));
            }
        }
    }

    lines
}

/// Format a timestamp as a short relative time like "2d", "5h", "3m".
pub(super) fn format_relative_short(ts: Option<chrono::DateTime<chrono::Utc>>) -> String {
    let ts = match ts {
        Some(t) => t,
        None => return String::new(),
    };
    let elapsed = chrono::Utc::now() - ts;
    let mins = elapsed.num_minutes();
    if mins < 1 {
        "now".to_string()
    } else if mins < 60 {
        format!("{}m", mins)
    } else if mins < 1440 {
        format!("{}h", mins / 60)
    } else {
        format!("{}d", mins / 1440)
    }
}

pub(super) fn format_task_preview_styled(task: &crate::core::models::Task) -> Vec<Line<'static>> {
    use crate::core::models::{EnvValue, TaskKind};
    use crate::core::parser::{parse_shell_task, parse_workflow};

    let label_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let value_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);

    let workflow = match task.kind {
        TaskKind::ShellScript => parse_shell_task(&task.path),
        TaskKind::YamlWorkflow => parse_workflow(&task.path),
    };

    match workflow {
        Ok(wf) => {
            let mut lines = Vec::new();
            lines.push(Line::from(vec![
                Span::styled("Workflow: ", label_style),
                Span::styled(wf.name.clone(), value_style),
            ]));
            if let Some(ref dir) = wf.workdir {
                lines.push(Line::from(vec![
                    Span::styled("Workdir:  ", label_style),
                    Span::styled(dir.display().to_string(), value_style),
                ]));
            }
            if !wf.env.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("Env vars: ", label_style),
                    Span::styled(wf.env.len().to_string(), value_style),
                ]));
                for (k, v) in &wf.env {
                    let val = match v {
                        EnvValue::Static(s) => s.clone(),
                        EnvValue::Dynamic { cmd } => format!("$({cmd})"),
                    };
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(format!("{k}: "), Style::default().fg(Color::White)),
                        Span::styled(val, Style::default().fg(Color::DarkGray)),
                    ]));
                }
            }
            lines.push(Line::from(vec![
                Span::styled("Steps:    ", label_style),
                Span::styled(wf.steps.len().to_string(), value_style),
            ]));
            lines.push(Line::from(""));

            for (i, step) in wf.steps.iter().enumerate() {
                let mut spans = vec![
                    Span::raw("  "),
                    Span::styled(format!("{}.", i + 1), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::raw(" "),
                    Span::styled(format!("[{}]", step.id), Style::default().fg(Color::Yellow)),
                ];
                if !step.needs.is_empty() {
                    spans.push(Span::styled(
                        format!(" (needs: {})", step.needs.join(", ")),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                lines.push(Line::from(spans));

                // Render MCP steps differently from shell commands
                #[cfg(feature = "mcp")]
                let is_mcp = step.mcp.is_some();
                #[cfg(not(feature = "mcp"))]
                let is_mcp = false;

                if is_mcp {
                    #[cfg(feature = "mcp")]
                    if let Some(ref mcp_cfg) = step.mcp {
                        let server_name = match &mcp_cfg.server {
                            crate::core::models::McpServerRef::Alias(s) => s.clone(),
                            crate::core::models::McpServerRef::Inline { command, .. } => {
                                let truncated = if command.len() > 40 {
                                    format!("{}...", &command[..37])
                                } else {
                                    command.clone()
                                };
                                truncated
                            }
                        };
                        let mcp_label = format!("{}/{}", server_name, mcp_cfg.tool);
                        let mcp_display = if mcp_label.len() > 120 {
                            format!("{}...", &mcp_label[..117])
                        } else {
                            mcp_label
                        };
                        lines.push(Line::from(vec![
                            Span::raw("     "),
                            Span::styled("mcp: ", Style::default().fg(Color::Magenta)),
                            Span::styled(mcp_display, Style::default().fg(Color::White)),
                        ]));
                        if let Some(ref args) = mcp_cfg.args {
                            if let Some(obj) = args.as_object() {
                                for (key, val) in obj {
                                    let val_str = match val {
                                        serde_json::Value::String(s) => s.clone(),
                                        other => other.to_string(),
                                    };
                                    let truncated = if val_str.len() > 100 {
                                        format!("{}...", &val_str[..97])
                                    } else {
                                        val_str
                                    };
                                    lines.push(Line::from(vec![
                                        Span::raw("       "),
                                        Span::styled(format!("{}: ", key), Style::default().fg(Color::Cyan)),
                                        Span::styled(truncated, Style::default().fg(Color::DarkGray)),
                                    ]));
                                }
                            }
                        }
                    }
                } else {
                    let sanitized = step.cmd.replace('\t', "  ");
                    let cmd = if sanitized.len() > 120 {
                        format!("{}...", &sanitized[..117])
                    } else {
                        sanitized
                    };
                    lines.push(Line::from(vec![
                        Span::raw("     "),
                        Span::styled("$ ", Style::default().fg(Color::Green)),
                        Span::styled(cmd, Style::default().fg(Color::White)),
                    ]));
                }
            }
            lines
        }
        Err(_) => {
            match std::fs::read_to_string(&task.path) {
                Ok(contents) => {
                    let file_lines: Vec<&str> = contents.lines().collect();
                    let max = 50.min(file_lines.len());
                    file_lines[..max].iter().map(|l| Line::from(l.to_string())).collect()
                }
                Err(e) => vec![Line::from(Span::styled(format!("Cannot read file: {e}"), Style::default().fg(Color::Red)))],
            }
        }
    }
}

pub(super) fn format_run_log_styled(log: &RunLog) -> Vec<Line<'static>> {
    let label_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let mut lines = Vec::new();

    // Structured failure banner when workflow failed
    if log.exit_code != 0 {
        lines.push(Line::from(Span::styled(
            "  WORKFLOW FAILED  ",
            Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        let passed = log.steps.iter().filter(|s| s.status == StepStatus::Success).count();
        let failed = log.steps.iter().filter(|s| s.status == StepStatus::Failed).count();
        let skipped = log.steps.iter().filter(|s| s.status == StepStatus::Skipped).count();
        let total_ms: u64 = log.steps.iter().map(|s| s.duration_ms).sum();
        lines.push(Line::from(vec![
            Span::styled(format!("Passed: {}", passed), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled("  ", Style::default()),
            Span::styled(format!("Failed: {}", failed), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled("  ", Style::default()),
            Span::styled(format!("Skipped: {}", skipped), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("  Duration: {:.1}s", total_ms as f64 / 1000.0), Style::default().fg(Color::DarkGray)),
        ]));
        lines.push(Line::from(""));

        // Show failed step details
        for step in &log.steps {
            if step.status == StepStatus::Failed {
                lines.push(Line::from(Span::styled(
                    format!("  {} (exit failure)", step.id),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )));
                let out_lines: Vec<&str> = step.output.trim().lines().collect();
                let tail = if out_lines.len() > 5 { &out_lines[out_lines.len()-5..] } else { &out_lines };
                for ol in tail {
                    lines.push(Line::from(vec![
                        Span::raw("    "),
                        Span::styled(ol.replace('\t', "  "), Style::default().fg(Color::DarkGray)),
                    ]));
                }
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Press 'a' to auto-fix with AI",
            Style::default().fg(Color::Cyan),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("─────────────────────────────", Style::default().fg(Color::DarkGray))));
        lines.push(Line::from(""));
    }

    lines.push(Line::from(vec![
        Span::styled("Run:     ", label_style),
        Span::styled(log.id.clone(), Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Task:    ", label_style),
        Span::styled(log.task_ref.clone(), Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Started: ", label_style),
        Span::styled(log.started.to_string(), Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Exit:    ", label_style),
        Span::styled(
            log.exit_code.to_string(),
            if log.exit_code == 0 {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            },
        ),
    ]));
    lines.push(Line::from(""));

    for step in &log.steps {
        let (icon, icon_style) = match step.status {
            StepStatus::Success => ("[OK]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            StepStatus::Failed => ("[FAIL]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            StepStatus::Skipped => ("[SKIP]", Style::default().fg(Color::DarkGray)),
            StepStatus::Timedout => ("[TIMEOUT]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            StepStatus::Running => ("[...]", Style::default().fg(Color::Yellow)),
            StepStatus::Interactive => ("[INTERACTIVE]", Style::default().fg(Color::Cyan)),
            StepStatus::Pending => ("[--]", Style::default().fg(Color::DarkGray)),
        };
        lines.push(Line::from(vec![
            Span::styled(icon, icon_style),
            Span::raw(" "),
            Span::styled(step.id.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" ({}ms)", step.duration_ms), Style::default().fg(Color::DarkGray)),
        ]));
        if !step.output.is_empty() {
            for out_line in step.output.trim().lines() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    colorize_output_line(&out_line.replace('\t', "  ")),
                ]));
            }
        }
    }

    lines
}

/// Format a failed run's step failures as a compact summary.
pub(super) fn format_failed_run_summary(log: &RunLog) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let failed: Vec<_> = log.steps.iter()
        .filter(|s| s.status == StepStatus::Failed)
        .collect();
    if failed.is_empty() {
        return lines;
    }
    let names: Vec<String> = failed.iter().map(|s| s.id.clone()).collect();
    lines.push(Line::from(Span::styled(
        format!("Fixing: {}", names.join(", ")),
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));
    for step in &failed {
        let snippet: String = step.output.trim().lines().last().unwrap_or("(no output)").to_string();
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", step.id), Style::default().fg(Color::Red)),
            Span::styled(snippet, Style::default().fg(Color::DarkGray)),
        ]));
    }
    lines.push(Line::from(""));
    lines
}

/// Colorize a single output line based on pattern matching.
pub(super) fn colorize_output_line(line: &str) -> Span<'static> {
    let trimmed = line.trim();
    let lower = trimmed.to_lowercase();

    // Unicode check/cross marks (cargo, npm, etc.)
    if trimmed.contains('✓') || trimmed.contains('✔') {
        return Span::styled(line.to_string(), Style::default().fg(Color::Green));
    }
    if trimmed.contains('✗') || trimmed.contains('✘') {
        return Span::styled(line.to_string(), Style::default().fg(Color::Red));
    }
    // Stack traces / line-number noise — de-emphasize
    if trimmed.starts_with("at ") || (trimmed.len() > 2 && trimmed.as_bytes()[0].is_ascii_digit() && (trimmed.contains("):") || trimmed.contains(")."))) {
        return Span::styled(line.to_string(), Style::default().fg(Color::DarkGray));
    }
    // File path with line number (e.g. src/foo.rs:42:)
    if trimmed.contains(':') && trimmed.len() > 3 {
        let parts: Vec<&str> = trimmed.splitn(3, ':').collect();
        if parts.len() >= 2 && parts[1].chars().all(|c| c.is_ascii_digit()) && !parts[0].is_empty() {
            return Span::styled(line.to_string(), Style::default().fg(Color::Cyan));
        }
    }

    if trimmed.starts_with('#') {
        return Span::styled(line.to_string(), Style::default().fg(Color::DarkGray));
    }
    if trimmed.starts_with('+') && !trimmed.starts_with("+++") {
        return Span::styled(line.to_string(), Style::default().fg(Color::Green));
    }
    if trimmed.starts_with('-') && !trimmed.starts_with("---") {
        return Span::styled(line.to_string(), Style::default().fg(Color::Red));
    }
    if trimmed.starts_with('$') || trimmed.starts_with('>') {
        return Span::styled(line.to_string(), Style::default().fg(Color::Green));
    }
    if lower.contains("error") || lower.contains("fatal") || lower.contains("panic") {
        return Span::styled(line.to_string(), Style::default().fg(Color::Red));
    }
    if lower.contains("warning") || lower.contains("warn") || lower.contains("deprecated") {
        return Span::styled(line.to_string(), Style::default().fg(Color::Yellow));
    }
    if lower.contains("ok") || lower.contains("pass") || lower.contains("success") {
        return Span::styled(line.to_string(), Style::default().fg(Color::Green));
    }
    Span::styled(line.to_string(), Style::default().fg(Color::White))
}

/// Colorize a comparison output line based on diff-like patterns.
pub(super) fn colorize_compare_line(line: &str) -> Span<'static> {
    let trimmed = line.trim();
    let upper = trimmed.to_uppercase();

    // Section headers (step IDs, labels ending with ':')
    if trimmed.ends_with(':') && !trimmed.contains(' ') {
        return Span::styled(line.to_string(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    }
    if upper.contains("ADDED") {
        return Span::styled(line.to_string(), Style::default().fg(Color::Green));
    }
    if upper.contains("REMOVED") {
        return Span::styled(line.to_string(), Style::default().fg(Color::Red));
    }
    if upper.contains("CHANGED") {
        return Span::styled(line.to_string(), Style::default().fg(Color::Yellow));
    }
    // Positive/negative deltas
    if trimmed.contains('+') && trimmed.chars().any(|c| c.is_ascii_digit()) {
        return Span::styled(line.to_string(), Style::default().fg(Color::Green));
    }
    if trimmed.starts_with('-') || (trimmed.contains('-') && trimmed.chars().any(|c| c.is_ascii_digit()) && !trimmed.starts_with("---")) {
        return Span::styled(line.to_string(), Style::default().fg(Color::Red));
    }
    // Separator lines
    if trimmed.chars().all(|c| c == '─' || c == '-' || c == '=') && trimmed.len() > 2 {
        return Span::styled(line.to_string(), Style::default().fg(Color::DarkGray));
    }
    Span::styled(line.to_string(), Style::default().fg(Color::White))
}

pub(super) fn format_logs_styled(logs: &[crate::core::models::RunLog]) -> Vec<Line<'static>> {
    if logs.is_empty() {
        return vec![Line::from(Span::styled("No logs available", Style::default().fg(Color::DarkGray)))];
    }

    let mut lines = Vec::new();
    for log in logs {
        let (status, status_style) = if log.exit_code == 0 {
            ("[OK]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        } else {
            ("[FAIL]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        };
        lines.push(Line::from(vec![
            Span::styled(status, status_style),
            Span::raw(" "),
            Span::styled(log.task_ref.clone(), Style::default().fg(Color::White)),
            Span::styled(
                format!(" @ {}", log.started.format("%Y-%m-%d %H:%M:%S")),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    lines
}

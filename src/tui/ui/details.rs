use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, LineGauge, Paragraph, Wrap};
use ratatui::Frame;

use crate::core::compare;
use crate::core::db;
use crate::core::models::StepStatus;

use super::app::{App, AppMode, Focus};
use super::helpers::{
    colorize_compare_line, format_live_progress_styled, format_logs_styled,
    format_run_log_styled, format_task_preview_styled,
};

pub(super) fn draw_details(f: &mut Frame, app: &mut App, area: Rect) {
    let style = if app.focus == Focus::Details {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let block = Block::default()
        .title("Details")
        .borders(Borders::ALL)
        .border_style(style);

    let lines: Vec<Line> = if app.mode == AppMode::Comparing {
        if let Some(ref result) = app.compare_result {
            compare::format_compare(result, false)
                .lines()
                .map(|l| Line::from(colorize_compare_line(l)))
                .collect()
        } else {
            vec![Line::from(Span::styled("No comparison data", Style::default().fg(Color::DarkGray)))]
        }
    } else if app.mode == AppMode::Running {
        format_live_progress_styled(app)
    } else if app.mode == AppMode::ViewingLogs {
        format_logs_styled(&app.viewing_logs)
    } else if let Some(ref run_log) = app.run_output {
        format_run_log_styled(run_log)
    } else if let Some(task) = app.selected_task_ref() {
        let task_ref = format!("{}/{}", task.category, task.name);
        let last_run = db::open_db(&app.config.db_path())
            .ok()
            .and_then(|conn| db::get_task_history(&conn, &task_ref, 1).ok())
            .and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) });
        let mut styled = format_task_preview_styled(task);
        if let Some(run_log) = last_run {
            styled.push(Line::from(""));
            styled.push(Line::from(Span::styled("--- Last Run ---", Style::default().fg(Color::DarkGray))));
            styled.push(Line::from(""));
            styled.extend(format_run_log_styled(&run_log));
        }
        styled
    } else {
        vec![Line::from(Span::styled("Select a task to preview", Style::default().fg(Color::DarkGray)))]
    };

    // Cache plain-text content lines for fold navigation keybindings
    app.detail_content_lines = lines.iter().map(|l| {
        l.spans.iter().map(|s| s.content.as_ref()).collect::<String>()
    }).collect();

    // Apply folding: replace folded regions with summary lines
    let lines = apply_folds(lines, &app.detail_folded_lines, &app.detail_content_lines);

    let text = Text::from(lines);

    // When running, show a progress gauge at the top
    if app.mode == AppMode::Running && !app.step_states.is_empty() {
        let total = app.step_states.len() as f64;
        let completed = app.step_states.iter()
            .filter(|s| matches!(s.status, StepStatus::Success | StepStatus::Failed | StepStatus::Skipped | StepStatus::Timedout))
            .count() as f64;
        let ratio = if total > 0.0 { (completed / total).min(1.0) } else { 0.0 };
        let label = format!(" {}/{} steps ", completed as usize, total as usize);

        let inner = block.inner(area);
        f.render_widget(block, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(inner);

        let gauge = LineGauge::default()
            .ratio(ratio)
            .label(label)
            .filled_style(Style::default().fg(Color::Green))
            .unfilled_style(Style::default().fg(Color::DarkGray));
        f.render_widget(gauge, chunks[0]);

        let iw = chunks[1].width as usize;
        let cl: u16 = text.lines.iter()
            .map(|l| if iw == 0 { 1u16 } else { 1 + (l.width() / iw.max(1)) as u16 })
            .sum();
        let max_scroll = cl.saturating_sub(chunks[1].height);
        let scroll = app.detail_scroll.min(max_scroll);

        let para = Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0));
        f.render_widget(para, chunks[1]);
        return;
    }

    // Approximate line count (including wraps) to clamp scroll
    let inner_width = area.width.saturating_sub(2) as usize; // borders
    let content_lines: u16 = text
        .lines
        .iter()
        .map(|l| {
            if inner_width == 0 {
                1u16
            } else {
                1 + (l.width() / inner_width.max(1)) as u16
            }
        })
        .sum();
    let inner_height = area.height.saturating_sub(2); // borders
    let max_scroll = content_lines.saturating_sub(inner_height);
    let scroll = app.detail_scroll.min(max_scroll);

    let para = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    f.render_widget(para, area);
}

/// Apply JSON folding: for each folded line that opens a `{` or `[`,
/// skip all lines until the matching close, and insert a summary line.
fn apply_folds(
    lines: Vec<Line<'static>>,
    folded_lines: &std::collections::HashSet<u16>,
    content_lines: &[String],
) -> Vec<Line<'static>> {
    if folded_lines.is_empty() {
        return lines;
    }

    let mut result = Vec::with_capacity(lines.len());
    let mut i = 0;

    while i < lines.len() {
        if folded_lines.contains(&(i as u16)) {
            // Find the matching close brace/bracket
            let trimmed = content_lines.get(i).map(|s| s.trim().to_string()).unwrap_or_default();
            let open_char = if trimmed.contains('{') { '{' } else { '[' };
            let close_char = if open_char == '{' { '}' } else { ']' };

            // Count nesting to find the matching close
            let mut depth = 0i32;
            let fold_start = i;
            let mut fold_end = i;
            for j in i..content_lines.len() {
                let line = content_lines[j].trim();
                for ch in line.chars() {
                    if ch == open_char { depth += 1; }
                    if ch == close_char { depth -= 1; }
                }
                if depth <= 0 {
                    fold_end = j;
                    break;
                }
                if j == content_lines.len() - 1 {
                    fold_end = j;
                }
            }

            let hidden = fold_end.saturating_sub(fold_start);
            // Show the original line + fold indicator
            result.push(lines[i].clone());
            let fold_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC);
            result.push(Line::from(Span::styled(
                format!("  ... {} lines folded ...", hidden),
                fold_style,
            )));

            i = fold_end + 1;
        } else {
            result.push(lines[i].clone());
            i += 1;
        }
    }

    result
}

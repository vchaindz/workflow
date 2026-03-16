use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
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

pub(super) fn draw_details(f: &mut Frame, app: &App, area: Rect) {
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

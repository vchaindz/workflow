use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

use crate::core::models::{TaskHeat, TaskKind};

use super::app::{App, Focus};
use super::helpers::format_relative_short;

pub(super) fn draw_task_list(f: &mut Frame, app: &App, area: Rect) {
    let style = if app.focus == Focus::TaskList {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let tasks = app.filtered_tasks();

    let mut items: Vec<ListItem> = Vec::new();

    for (i, task) in tasks.iter().enumerate() {
        let marker = if i == app.selected_task { ">" } else { " " };
        let kind = match task.kind {
            TaskKind::ShellScript => "sh",
            TaskKind::YamlWorkflow => "yaml",
        };
        let task_ref = format!("{}/{}", task.category, task.name);
        let star = if app.config.bookmarks.contains(&task_ref) { "★ " } else { "" };
        let (heat_icon, heat_color) = match task.heat {
            TaskHeat::Hot => ("▲", Color::Green),
            TaskHeat::Warm => ("·", Color::Reset),
            TaskHeat::Cold => ("▽", Color::Blue),
        };
        let name_style = if i == app.selected_task {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        // Last-run indicator
        let run_indicator = if let Some(ref summary) = task.last_run {
            let (icon, time) = if summary.fail_count > 0 && summary.last_failure > summary.last_success {
                let t = format_relative_short(summary.last_failure);
                ("\u{2717}", t) // ✗
            } else if summary.last_success.is_some() {
                let t = format_relative_short(summary.last_success);
                ("\u{2713}", t) // ✓
            } else {
                ("", String::new())
            };
            if icon.is_empty() {
                String::new()
            } else {
                format!(" {} {}", icon, time)
            }
        } else {
            String::new()
        };
        let run_color = if let Some(ref summary) = task.last_run {
            if summary.fail_count > 0 && summary.last_failure > summary.last_success {
                Color::Red
            } else {
                Color::Green
            }
        } else {
            Color::DarkGray
        };

        let mut spans = vec![
            Span::styled(format!("{marker} "), name_style),
            Span::styled(format!("{heat_icon} "), Style::default().fg(heat_color)),
            Span::styled(format!("{star}{} [{kind}]", task.name), name_style),
        ];
        if !run_indicator.is_empty() {
            spans.push(Span::styled(run_indicator, Style::default().fg(run_color)));
        }
        let line = Line::from(spans);
        items.push(ListItem::new(line));
    }

    let title = if app.filtered_indices.is_some() {
        format!("Tasks (search: {})", app.search_query)
    } else if app.status_filter != super::app::StatusFilter::All {
        format!("Tasks [{}]", app.status_filter.label())
    } else {
        "Tasks".to_string()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(style);

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

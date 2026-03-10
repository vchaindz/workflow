use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::core::models::{StepStatus, TaskKind};

use super::app::{App, AppMode, Focus};

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(f.area());

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(35),
            Constraint::Percentage(45),
        ])
        .split(chunks[0]);

    draw_sidebar(f, app, main_chunks[0]);
    draw_task_list(f, app, main_chunks[1]);
    draw_details(f, app, main_chunks[2]);
    draw_status_bar(f, app, chunks[1]);
}

fn draw_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let style = if app.focus == Focus::Sidebar {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let items: Vec<ListItem> = app
        .categories
        .iter()
        .enumerate()
        .map(|(i, cat)| {
            let marker = if i == app.selected_category {
                ">"
            } else {
                " "
            };
            let s = if i == app.selected_category {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(format!("{marker} {} ({})", cat.name, cat.tasks.len())).style(s)
        })
        .collect();

    let block = Block::default()
        .title("Categories")
        .borders(Borders::ALL)
        .border_style(style);

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn draw_task_list(f: &mut Frame, app: &App, area: Rect) {
    let style = if app.focus == Focus::TaskList {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let tasks = app.filtered_tasks();

    let items: Vec<ListItem> = tasks
        .iter()
        .enumerate()
        .map(|(i, task)| {
            let marker = if i == app.selected_task { ">" } else { " " };
            let kind = match task.kind {
                TaskKind::ShellScript => "sh",
                TaskKind::YamlWorkflow => "yaml",
            };

            let s = if i == app.selected_task {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(format!("{marker} {} [{kind}]", task.name)).style(s)
        })
        .collect();

    let title = if app.filtered_indices.is_some() {
        format!("Tasks (search: {})", app.search_query)
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

fn draw_details(f: &mut Frame, app: &App, area: Rect) {
    let style = if app.focus == Focus::Details {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let block = Block::default()
        .title("Details")
        .borders(Borders::ALL)
        .border_style(style);

    let content = if app.mode == AppMode::Running {
        app.running_message
            .as_deref()
            .unwrap_or("Running...")
            .to_string()
    } else if app.mode == AppMode::ViewingLogs {
        format_logs(&app.viewing_logs)
    } else if let Some(ref run_log) = app.run_output {
        format_run_log(run_log)
    } else if let Some(task) = app.selected_task_ref() {
        // Show file preview
        match std::fs::read_to_string(&task.path) {
            Ok(contents) => {
                let lines: Vec<&str> = contents.lines().collect();
                let max = 50.min(lines.len());
                lines[..max].join("\n")
            }
            Err(e) => format!("Cannot read file: {e}"),
        }
    } else {
        "Select a task to preview".to_string()
    };

    let para = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));

    f.render_widget(para, area);
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let content = match app.mode {
        AppMode::Search => {
            format!("Search: {}_ | ESC cancel", app.search_query)
        }
        AppMode::Running => "Running... please wait".to_string(),
        _ => {
            "j/k:nav  Tab:pane  r:run  e:edit  l:logs  /:search  d:dry-run  q:quit".to_string()
        }
    };

    let bar = Paragraph::new(Line::from(vec![Span::styled(
        content,
        Style::default()
            .fg(Color::Black)
            .bg(Color::White),
    )]));
    f.render_widget(bar, area);
}

fn format_run_log(log: &crate::core::models::RunLog) -> String {
    let mut out = format!(
        "Run: {}\nTask: {}\nStarted: {}\nExit: {}\n\n",
        log.id, log.task_ref, log.started, log.exit_code
    );

    for step in &log.steps {
        let icon = match step.status {
            StepStatus::Success => "[OK]",
            StepStatus::Failed => "[FAIL]",
            StepStatus::Skipped => "[SKIP]",
            StepStatus::Running => "[...]",
            StepStatus::Pending => "[--]",
        };
        out.push_str(&format!(
            "{} {} ({}ms)\n",
            icon, step.id, step.duration_ms
        ));
        if !step.output.is_empty() {
            out.push_str(&format!("  {}\n", step.output.trim()));
        }
    }

    out
}

fn format_logs(logs: &[crate::core::models::RunLog]) -> String {
    if logs.is_empty() {
        return "No logs available".to_string();
    }

    let mut out = String::new();
    for log in logs {
        let status = if log.exit_code == 0 { "OK" } else { "FAIL" };
        out.push_str(&format!(
            "[{status}] {} @ {}\n",
            log.task_ref,
            log.started.format("%Y-%m-%d %H:%M:%S")
        ));
    }
    out
}

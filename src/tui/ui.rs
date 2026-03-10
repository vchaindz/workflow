use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::core::db;
use crate::core::models::{StepStatus, TaskKind};
use crate::core::parser::{parse_shell_task, parse_workflow};
use crate::core::wizard;

use super::app::{App, AppMode, Focus, WizardStage};

pub fn draw(f: &mut Frame, app: &App) {
    let has_footer = !app.footer_log.is_empty();

    let chunks = if has_footer {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(7),
                Constraint::Length(1),
            ])
            .split(f.area())
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(f.area())
    };

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

    if has_footer {
        draw_footer(f, app, chunks[1]);
        draw_status_bar(f, app, chunks[2]);
    } else {
        draw_status_bar(f, app, chunks[1]);
    }

    if app.mode == AppMode::Help {
        draw_help(f);
    }
}

fn draw_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let style = if app.focus == Focus::Sidebar {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let mut items: Vec<ListItem> = Vec::new();

    for (i, cat) in app.categories.iter().enumerate() {
        let marker = if i == app.selected_category {
            ">"
        } else {
            " "
        };
        let cat_style = if i == app.selected_category {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        items.push(
            ListItem::new(format!(
                "{marker} {} ({})",
                cat.name,
                cat.tasks.len()
            ))
            .style(cat_style),
        );
    }

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

    let mut items: Vec<ListItem> = Vec::new();

    for (i, task) in tasks.iter().enumerate() {
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

        items.push(
            ListItem::new(format!("{marker} {} [{kind}]", task.name)).style(s),
        );
    }

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

    let content = if app.mode == AppMode::Wizard {
        format_wizard(app)
    } else if app.mode == AppMode::Running {
        format_live_progress(app)
    } else if app.mode == AppMode::ViewingLogs {
        format_logs(&app.viewing_logs)
    } else if let Some(ref run_log) = app.run_output {
        format_run_log(run_log)
    } else if let Some(task) = app.selected_task_ref() {
        let task_ref = format!("{}/{}", task.category, task.name);
        let last_run = db::open_db(&app.config.db_path())
            .ok()
            .and_then(|conn| db::get_task_history(&conn, &task_ref, 1).ok())
            .and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) });
        let mut preview = format_task_preview(task);
        if let Some(run_log) = last_run {
            preview.push_str("\n--- Last Run ---\n");
            preview.push_str(&format_run_log(&run_log));
        }
        preview
    } else {
        "Select a task to preview".to_string()
    };

    // Approximate line count (including wraps) to clamp scroll
    let inner_width = area.width.saturating_sub(2) as usize; // borders
    let content_lines: u16 = content
        .lines()
        .map(|l| {
            if inner_width == 0 {
                1u16
            } else {
                1 + (l.len() / inner_width.max(1)) as u16
            }
        })
        .sum();
    let inner_height = area.height.saturating_sub(2); // borders
    let max_scroll = content_lines.saturating_sub(inner_height);
    let scroll = app.detail_scroll.min(max_scroll);

    let para = Paragraph::new(content)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    f.render_widget(para, area);
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let content = match app.mode {
        AppMode::Search => {
            format!("Search: {}_ | ESC cancel", app.search_query)
        }
        AppMode::Running => "Running... (output in footer below)".to_string(),
        AppMode::Wizard => {
            if let Some(ref wiz) = app.wizard {
                if wiz.save_message.is_some() {
                    "Task saved! Press any key to continue".to_string()
                } else {
                    match wiz.stage {
                        WizardStage::Category => "Category: type name or Up/Down to pick | Tab/Enter:next  Esc:cancel".to_string(),
                        WizardStage::TaskName => "Task name: type name | Tab/Enter:next  Esc:cancel".to_string(),
                        WizardStage::Options => "Space:toggle  Up/Down:select  Tab/Enter:next  Esc:cancel".to_string(),
                        WizardStage::Preview => "Up/Down:scroll  Enter:save  Esc:cancel".to_string(),
                    }
                }
            } else {
                String::new()
            }
        }
        _ => {
            "arrows:nav  +/-:collapse  r:run  d:dry-run  e:edit  w:wizard  L:logs  /:search  h:help  q:quit"
                .to_string()
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

fn format_wizard(app: &App) -> String {
    let wiz = match app.wizard.as_ref() {
        Some(w) => w,
        None => return "Wizard not active".to_string(),
    };

    if let Some(ref msg) = wiz.save_message {
        return format!("  {}\n\n  Press any key to continue.", msg);
    }

    let mut out = String::new();
    out.push_str("  Create Task from Template\n");
    out.push_str(&format!("  Source: {}\n\n", wiz.source_task_ref));

    match wiz.stage {
        WizardStage::Category => {
            out.push_str("  Step 1/4: Category\n\n");
            out.push_str(&format!("  > {}_ \n\n", wiz.category));
            out.push_str("  Existing categories:\n");
            for (i, cat) in app.categories.iter().enumerate() {
                let marker = if wiz.category_cursor == Some(i) {
                    ">"
                } else {
                    " "
                };
                out.push_str(&format!("  {} {}\n", marker, cat.name));
            }
            out.push_str("\n  Type a new name or Up/Down to pick existing\n");
        }
        WizardStage::TaskName => {
            out.push_str("  Step 2/4: Task Name\n\n");
            out.push_str(&format!("  Category: {}\n", wiz.category));
            out.push_str(&format!("  > {}_ \n", wiz.task_name));
        }
        WizardStage::Options => {
            out.push_str("  Step 3/4: Optimization Options\n\n");
            out.push_str(&format!("  Category: {}\n", wiz.category));
            out.push_str(&format!("  Task:     {}\n\n", wiz.task_name));

            let has_run = wiz.source_run.is_some();
            let opts = [
                ("Remove failed steps", wiz.remove_failed, has_run),
                ("Remove skipped steps", wiz.remove_skipped, has_run),
                ("Parallelize independent steps", wiz.parallelize, true),
            ];

            for (i, (label, checked, enabled)) in opts.iter().enumerate() {
                let marker = if i == wiz.active_toggle { ">" } else { " " };
                let check = if *checked { "x" } else { " " };
                let suffix = if !enabled { " (no run data)" } else { "" };
                out.push_str(&format!("  {} [{}] {}{}\n", marker, check, label, suffix));
            }
        }
        WizardStage::Preview => {
            out.push_str("  Step 4/4: Preview\n\n");
            out.push_str(&format!(
                "  Will save to: {}/{}.yaml\n\n",
                wiz.category, wiz.task_name
            ));

            let optimized = wizard::optimize_workflow(
                &wiz.source_workflow,
                wiz.source_run.as_ref(),
                wiz.remove_failed,
                wiz.remove_skipped,
                wiz.parallelize,
            );
            let yaml = wizard::generate_yaml(&optimized);

            for line in yaml.lines() {
                out.push_str(&format!("  {}\n", line));
            }
        }
    }

    out
}

fn format_live_progress(app: &App) -> String {
    let mut out = String::new();
    if let Some(ref task_ref) = app.executing_task_ref {
        out.push_str(&format!("Running: {}\n\n", task_ref));
    }

    if app.step_states.is_empty() {
        out.push_str("Preparing...\n");
        return out;
    }

    for (i, state) in app.step_states.iter().enumerate() {
        let icon = match state.status {
            StepStatus::Running => "▶",
            StepStatus::Success => "✓",
            StepStatus::Failed => "✗",
            StepStatus::Skipped => "⊘",
            StepStatus::Pending => "·",
        };

        let duration = match state.duration_ms {
            Some(ms) if ms >= 1000 => format!(" ({:.1}s)", ms as f64 / 1000.0),
            Some(ms) => format!(" ({}ms)", ms),
            None if state.status == StepStatus::Running => " ...".to_string(),
            None => String::new(),
        };

        out.push_str(&format!("  {} {}. {}{}\n", icon, i + 1, state.id, duration));

        if !state.cmd_preview.is_empty() && state.status == StepStatus::Running {
            out.push_str(&format!("       $ {}\n", state.cmd_preview));
        }
    }

    out
}

fn format_task_preview(task: &crate::core::models::Task) -> String {
    let workflow = match task.kind {
        TaskKind::ShellScript => parse_shell_task(&task.path),
        TaskKind::YamlWorkflow => parse_workflow(&task.path),
    };

    match workflow {
        Ok(wf) => {
            let mut out = format!("Workflow: {}\n", wf.name);
            if let Some(ref dir) = wf.workdir {
                out.push_str(&format!("Workdir:  {}\n", dir.display()));
            }
            if !wf.env.is_empty() {
                out.push_str(&format!("Env vars: {}\n", wf.env.len()));
            }
            out.push_str(&format!("Steps:    {}\n\n", wf.steps.len()));

            for (i, step) in wf.steps.iter().enumerate() {
                out.push_str(&format!("  {}. [{}]", i + 1, step.id));
                if !step.needs.is_empty() {
                    out.push_str(&format!(" (needs: {})", step.needs.join(", ")));
                }
                out.push('\n');
                // Show command, truncated per line
                let cmd = if step.cmd.len() > 80 {
                    format!("{}...", &step.cmd[..77])
                } else {
                    step.cmd.clone()
                };
                out.push_str(&format!("     $ {}\n\n", cmd));
            }
            out
        }
        Err(_) => {
            // Fallback to raw file content
            match std::fs::read_to_string(&task.path) {
                Ok(contents) => {
                    let lines: Vec<&str> = contents.lines().collect();
                    let max = 50.min(lines.len());
                    lines[..max].join("\n")
                }
                Err(e) => format!("Cannot read file: {e}"),
            }
        }
    }
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let (title, border_color) = if app.is_executing {
        (" Running... ", Color::Yellow)
    } else {
        (" Execution Log ", Color::Green)
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner_height = area.height.saturating_sub(2) as usize; // borders take 2 lines
    let start = app.footer_log.len().saturating_sub(inner_height);
    let visible_lines: Vec<Line> = app.footer_log[start..]
        .iter()
        .map(|line| {
            let color = if line.contains("✓") || line.contains("Done") {
                Color::Green
            } else if line.contains("✗") || line.contains("Error") || line.contains("FAIL") {
                Color::Red
            } else if line.contains("▶") || line.contains("Starting") {
                Color::Yellow
            } else if line.contains("⊘") || line.contains("skipped") {
                Color::DarkGray
            } else {
                Color::White
            };
            Line::from(Span::styled(line.as_str(), Style::default().fg(color)))
        })
        .collect();

    let para = Paragraph::new(visible_lines).block(block);
    f.render_widget(para, area);
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

fn draw_help(f: &mut Frame) {
    let area = f.area();
    let w = 50.min(area.width.saturating_sub(4));
    let h = 20.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2;
    let y = (area.height.saturating_sub(h)) / 2;
    let popup = Rect::new(x, y, w, h);

    let help_text = vec![
        Line::from(Span::styled(
            " dzworkflows ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  Up/Down     Navigate items"),
        Line::from("  Left/Right  Switch panes"),
        Line::from("  Tab         Next pane"),
        Line::from("  Enter       Expand/enter category"),
        Line::from("  +           Expand category"),
        Line::from("  -           Collapse category"),
        Line::from("  r           Run selected task"),
        Line::from("  d           Dry-run selected task"),
        Line::from("  e           Edit task in $EDITOR"),
        Line::from("  w           Create task from template"),
        Line::from("  L           View run logs"),
        Line::from("  /           Search tasks"),
        Line::from("  q           Quit / close"),
        Line::from("  Esc         Cancel / dismiss"),
        Line::from(""),
        Line::from(Span::styled(
            "  Press any key to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let block = Block::default()
        .title(" Help ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    f.render_widget(Clear, popup);
    let para = Paragraph::new(help_text).block(block);
    f.render_widget(para, popup);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::Config;
    use crate::core::models::{Category, StepStatus, Task, TaskKind};
    use crate::tui::app::{App, AppMode, StepState};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::io::Write as IoWrite;
    use tempfile::TempDir;

    fn render_app(app: &App, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn buffer_text(buf: &ratatui::buffer::Buffer) -> String {
        let area = buf.area;
        let mut text = String::new();
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                let cell = &buf[(x, y)];
                text.push_str(cell.symbol());
            }
            text.push('\n');
        }
        text
    }

    fn make_test_categories(tmp: &TempDir) -> Vec<Category> {
        // Create a YAML workflow file for render tests that need file parsing
        let backup_dir = tmp.path().join("backup");
        std::fs::create_dir_all(&backup_dir).unwrap();

        let yaml_path = backup_dir.join("db-full.yaml");
        let yaml_content = r#"name: db-full
workdir: /var/backups
env:
  DB_HOST: localhost
  DB_PORT: "5432"
steps:
  - id: dump
    cmd: pg_dump mydb > dump.sql
  - id: compress
    cmd: gzip dump.sql
    needs: [dump]
"#;
        std::fs::write(&yaml_path, yaml_content).unwrap();

        let sh_path = backup_dir.join("files.sh");
        let mut f = std::fs::File::create(&sh_path).unwrap();
        writeln!(f, "#!/bin/bash\ntar czf /tmp/backup.tar.gz /home").unwrap();

        vec![
            Category {
                name: "backup".into(),
                path: backup_dir.clone(),
                tasks: vec![
                    Task {
                        name: "db-full".into(),
                        kind: TaskKind::YamlWorkflow,
                        path: yaml_path,
                        category: "backup".into(),
                        last_run: None,
                    },
                    Task {
                        name: "files".into(),
                        kind: TaskKind::ShellScript,
                        path: sh_path,
                        category: "backup".into(),
                        last_run: None,
                    },
                ],
            },
            Category {
                name: "deploy".into(),
                path: tmp.path().join("deploy"),
                tasks: vec![],
            },
        ]
    }

    fn make_render_app(tmp: &TempDir) -> App {
        let categories = make_test_categories(tmp);
        let config = Config {
            workflows_dir: tmp.path().to_path_buf(),
            ..Config::default()
        };
        App::new(categories, config)
    }

    #[test]
    fn test_render_categories_in_sidebar() {
        let tmp = TempDir::new().unwrap();
        let app = make_render_app(&tmp);
        let buf = render_app(&app, 120, 30);
        let text = buffer_text(&buf);

        assert!(text.contains("backup"), "sidebar should show 'backup' category");
        assert!(text.contains("deploy"), "sidebar should show 'deploy' category");
        // Task count in parentheses
        assert!(text.contains("(2)"), "sidebar should show task count for backup");
    }

    #[test]
    fn test_render_task_list() {
        let tmp = TempDir::new().unwrap();
        let app = make_render_app(&tmp);
        let buf = render_app(&app, 120, 30);
        let text = buffer_text(&buf);

        assert!(text.contains("db-full"), "task list should show 'db-full'");
        assert!(text.contains("[yaml]"), "task list should show [yaml] kind label");
        assert!(text.contains("files"), "task list should show 'files'");
        assert!(text.contains("[sh]"), "task list should show [sh] kind label");
    }

    #[test]
    fn test_render_details_env_vars() {
        let tmp = TempDir::new().unwrap();
        let app = make_render_app(&tmp);
        // Default selection is first task (db-full.yaml) which has 2 env vars
        let buf = render_app(&app, 120, 30);
        let text = buffer_text(&buf);

        assert!(text.contains("Env vars: 2"), "details should show 'Env vars: 2'");
    }

    #[test]
    fn test_render_details_workdir() {
        let tmp = TempDir::new().unwrap();
        let app = make_render_app(&tmp);
        let buf = render_app(&app, 120, 30);
        let text = buffer_text(&buf);

        assert!(
            text.contains("/var/backups"),
            "details should show workdir '/var/backups'"
        );
    }

    #[test]
    fn test_render_details_steps() {
        let tmp = TempDir::new().unwrap();
        let app = make_render_app(&tmp);
        let buf = render_app(&app, 120, 30);
        let text = buffer_text(&buf);

        assert!(text.contains("dump"), "details should show step id 'dump'");
        assert!(text.contains("compress"), "details should show step id 'compress'");
        assert!(
            text.contains("pg_dump"),
            "details should show step command 'pg_dump'"
        );
        assert!(
            text.contains("gzip"),
            "details should show step command 'gzip'"
        );
    }

    #[test]
    fn test_render_search_mode() {
        let tmp = TempDir::new().unwrap();
        let mut app = make_render_app(&tmp);
        app.mode = AppMode::Search;
        app.search_query = "db".into();

        let buf = render_app(&app, 120, 30);
        let text = buffer_text(&buf);

        assert!(
            text.contains("Search: db"),
            "status bar should show 'Search: db' in search mode"
        );
    }

    #[test]
    fn test_render_help_overlay() {
        let tmp = TempDir::new().unwrap();
        let mut app = make_render_app(&tmp);
        app.mode = AppMode::Help;

        let buf = render_app(&app, 120, 30);
        let text = buffer_text(&buf);

        assert!(
            text.contains("dzworkflows"),
            "help overlay should show 'dzworkflows'"
        );
        assert!(
            text.contains("Dry-run"),
            "help overlay should show key binding for dry-run"
        );
        assert!(
            text.contains("Search tasks"),
            "help overlay should show 'Search tasks'"
        );
    }

    #[test]
    fn test_render_running_dry_run() {
        let tmp = TempDir::new().unwrap();
        let mut app = make_render_app(&tmp);
        app.mode = AppMode::Running;
        app.is_executing = true;
        app.executing_task_ref = Some("backup/db-full".into());
        app.step_states = vec![
            StepState {
                id: "dump".into(),
                cmd_preview: "[dry-run] pg_dump mydb".into(),
                status: StepStatus::Success,
                duration_ms: Some(0),
            },
            StepState {
                id: "compress".into(),
                cmd_preview: "[dry-run] gzip dump.sql".into(),
                status: StepStatus::Running,
                duration_ms: None,
            },
        ];
        app.footer_log = vec!["[12:00:00] Starting backup/db-full (dry-run)...".into()];

        let buf = render_app(&app, 120, 30);
        let text = buffer_text(&buf);

        assert!(
            text.contains("[dry-run]"),
            "running details should show [dry-run] prefix in commands"
        );
        assert!(
            text.contains("backup/db-full"),
            "running details should show task ref"
        );
    }

    #[test]
    fn test_render_footer_log() {
        let tmp = TempDir::new().unwrap();
        let mut app = make_render_app(&tmp);
        app.footer_log = vec![
            "[12:00:00] Starting backup/db-full...".into(),
            "[12:00:01] ✓ dump (150ms)".into(),
        ];

        let buf = render_app(&app, 120, 30);
        let text = buffer_text(&buf);

        assert!(
            text.contains("Execution Log"),
            "footer should show 'Execution Log' title when not executing"
        );
        // Footer log content is rendered
        assert!(
            text.contains("Starting backup/db-full"),
            "footer should show log entries"
        );
    }
}

mod details;
mod header;
mod helpers;
mod modals;
mod sidebar;
mod task_list;
mod wizard;

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Frame;

use super::app;

use details::draw_details;
use header::{draw_footer, draw_header, draw_status_bar};
use modals::{
    draw_edit_task, draw_getting_started, draw_git_sync, draw_help, draw_memory_view,
    draw_overdue_reminder, draw_recent_runs, draw_saved_tasks, draw_secrets,
    draw_streaming_modal,
};
use sidebar::draw_sidebar;
use task_list::draw_task_list;
use wizard::{draw_confirm_delete, draw_rename, draw_variable_prompt, draw_wizard};

pub fn draw(f: &mut Frame, app: &mut app::App) {
    let has_footer = !app.footer_log.is_empty();

    let chunks = if has_footer {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(7),
                Constraint::Length(1),
            ])
            .split(f.area())
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(f.area())
    };

    draw_header(f, app, chunks[0]);

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(25),
            Constraint::Percentage(55),
        ])
        .split(chunks[1]);

    draw_sidebar(f, app, main_chunks[0]);
    draw_task_list(f, app, main_chunks[1]);
    draw_details(f, app, main_chunks[2]);

    if has_footer {
        draw_footer(f, app, chunks[2]);
        draw_status_bar(f, app, chunks[3]);
    } else {
        draw_status_bar(f, app, chunks[2]);
    }

    if app.mode == app::AppMode::Help {
        draw_help(f, app);
    }

    if app.mode == app::AppMode::Wizard {
        draw_wizard(f, app);
    }

    if app.mode == app::AppMode::ConfirmDelete {
        draw_confirm_delete(f, app);
    }

    if app.mode == app::AppMode::Rename {
        draw_rename(f, app);
    }

    if app.mode == app::AppMode::StreamingOutput {
        draw_streaming_modal(f, app);
    }

    if app.mode == app::AppMode::RecentRuns {
        draw_recent_runs(f, app);
    }

    if app.mode == app::AppMode::SavedTasks {
        draw_saved_tasks(f, app);
    }

    if app.mode == app::AppMode::OverdueReminder {
        draw_overdue_reminder(f, app);
    }

    if app.mode == app::AppMode::GettingStarted {
        draw_getting_started(f, app);
    }

    if app.mode == app::AppMode::MemoryView {
        draw_memory_view(f, app);
    }

    if app.mode == app::AppMode::VariablePrompt {
        draw_variable_prompt(f, app);
    }

    if app.mode == app::AppMode::GitSync {
        draw_git_sync(f, app);
    }

    if app.mode == app::AppMode::EditTask {
        draw_edit_task(f, app);
    }

    if app.mode == app::AppMode::Secrets {
        draw_secrets(f, app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::Config;
    use crate::core::models::{Category, StepStatus, Task, TaskHeat, TaskKind};
    use crate::tui::app::{App, AppMode, StepState};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::io::Write as IoWrite;
    use tempfile::TempDir;

    fn render_app(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
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
                        overdue: None,
                        heat: TaskHeat::Cold,
                    },
                    Task {
                        name: "files".into(),
                        kind: TaskKind::ShellScript,
                        path: sh_path,
                        category: "backup".into(),
                        last_run: None,
                        overdue: None,
                        heat: TaskHeat::Cold,
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
        let mut app = make_render_app(&tmp);
        let buf = render_app(&mut app, 120, 30);
        let text = buffer_text(&buf);

        assert!(text.contains("backup"), "sidebar should show 'backup' category");
        assert!(text.contains("deploy"), "sidebar should show 'deploy' category");
        // Task count in parentheses
        assert!(text.contains("(2)"), "sidebar should show task count for backup");
    }

    #[test]
    fn test_render_task_list() {
        let tmp = TempDir::new().unwrap();
        let mut app = make_render_app(&tmp);
        let buf = render_app(&mut app, 120, 30);
        let text = buffer_text(&buf);

        assert!(text.contains("db-full"), "task list should show 'db-full'");
        assert!(text.contains("[yaml]"), "task list should show [yaml] kind label");
        assert!(text.contains("files"), "task list should show 'files'");
        assert!(text.contains("[sh]"), "task list should show [sh] kind label");
    }

    #[test]
    fn test_render_details_env_vars() {
        let tmp = TempDir::new().unwrap();
        let mut app = make_render_app(&tmp);
        // Default selection is first task (db-full.yaml) which has 2 env vars
        let buf = render_app(&mut app, 120, 30);
        let text = buffer_text(&buf);

        assert!(text.contains("Env vars: 2"), "details should show 'Env vars: 2'");
    }

    #[test]
    fn test_render_details_workdir() {
        let tmp = TempDir::new().unwrap();
        let mut app = make_render_app(&tmp);
        let buf = render_app(&mut app, 120, 30);
        let text = buffer_text(&buf);

        assert!(
            text.contains("/var/backups"),
            "details should show workdir '/var/backups'"
        );
    }

    #[test]
    fn test_render_details_steps() {
        let tmp = TempDir::new().unwrap();
        let mut app = make_render_app(&tmp);
        let buf = render_app(&mut app, 120, 30);
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

        let buf = render_app(&mut app, 120, 30);
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

        let buf = render_app(&mut app, 120, 36);
        let text = buffer_text(&buf);

        assert!(
            text.contains("workflow"),
            "help overlay should show 'workflow'"
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
                last_output: None,
            },
            StepState {
                id: "compress".into(),
                cmd_preview: "[dry-run] gzip dump.sql".into(),
                status: StepStatus::Running,
                duration_ms: None,
                last_output: None,
            },
        ];
        app.footer_log = vec!["[12:00:00] Starting backup/db-full (dry-run)...".into()];

        let buf = render_app(&mut app, 120, 30);
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

        let buf = render_app(&mut app, 120, 30);
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

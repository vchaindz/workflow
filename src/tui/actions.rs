use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::executor::{execute_workflow, ExecuteOpts};
use crate::core::logger::{get_task_history, write_run_log};
use crate::core::models::TaskKind;
use crate::core::parser::{parse_shell_task, parse_workflow};
use crate::error::Result;

use super::app::{App, AppMode, Focus};

pub fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match app.mode {
        AppMode::Search => handle_search_key(app, key),
        AppMode::Running => Ok(()), // Block input during execution
        _ => handle_normal_key(app, key),
    }
}

fn handle_normal_key(app: &mut App, key: KeyEvent) -> Result<()> {
    // Ctrl-C always quits
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return Ok(());
    }

    match key.code {
        KeyCode::Char('q') => {
            if app.mode == AppMode::ViewingLogs {
                app.mode = AppMode::Normal;
                app.viewing_logs.clear();
            } else {
                app.should_quit = true;
            }
        }
        KeyCode::Esc => {
            if app.mode == AppMode::ViewingLogs {
                app.mode = AppMode::Normal;
                app.viewing_logs.clear();
            }
            app.run_output = None;
        }
        KeyCode::Char('j') | KeyCode::Down => app.move_down(),
        KeyCode::Char('k') | KeyCode::Up => app.move_up(),
        KeyCode::Tab | KeyCode::Char('l') if app.focus != Focus::Details => app.focus_next(),
        KeyCode::BackTab | KeyCode::Char('h') if app.focus != Focus::Sidebar => app.focus_prev(),
        KeyCode::Char('/') => app.start_search(),
        KeyCode::Char('r') => run_selected(app, false)?,
        KeyCode::Char('d') => run_selected(app, true)?,
        KeyCode::Char('e') => edit_selected(app)?,
        KeyCode::Char('l') if app.focus == Focus::Details => {
            // scroll down in details
            app.detail_scroll = app.detail_scroll.saturating_add(1);
        }
        KeyCode::Char('L') => view_logs(app)?,
        KeyCode::Enter => {
            if app.focus == Focus::Sidebar {
                app.focus = Focus::TaskList;
                app.selected_task = 0;
            }
        }
        _ => {}
    }

    Ok(())
}

fn handle_search_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => app.cancel_search(),
        KeyCode::Enter => {
            app.mode = AppMode::Normal;
            app.focus = Focus::TaskList;
        }
        KeyCode::Backspace => {
            app.search_query.pop();
            app.update_search();
        }
        KeyCode::Char(c) => {
            app.search_query.push(c);
            app.update_search();
        }
        _ => {}
    }
    Ok(())
}

fn run_selected(app: &mut App, dry_run: bool) -> Result<()> {
    let task = match app.selected_task_ref() {
        Some(t) => t.clone(),
        None => return Ok(()),
    };

    let workflow = match task.kind {
        TaskKind::ShellScript => parse_shell_task(&task.path)?,
        TaskKind::YamlWorkflow => parse_workflow(&task.path)?,
    };

    let task_ref = format!("{}/{}", task.category, task.name);

    app.mode = AppMode::Running;
    app.running_message = Some(format!("Running {}...", task_ref));

    let opts = ExecuteOpts {
        dry_run,
        env_overrides: HashMap::new(),
    };

    let run_log = execute_workflow(&workflow, &task_ref, &opts)?;

    if !dry_run {
        write_run_log(&app.config.logs_dir(), &run_log)?;
    }

    app.mode = AppMode::Normal;
    app.run_output = Some(run_log);
    app.focus = Focus::Details;
    app.detail_scroll = 0;

    Ok(())
}

fn edit_selected(app: &App) -> Result<()> {
    let task = match app.selected_task_ref() {
        Some(t) => t,
        None => return Ok(()),
    };

    let editor = &app.config.editor;
    let path = task.path.display().to_string();

    // We need to restore terminal, run editor, then re-init
    // This is handled in the TUI mod by returning a special action
    std::process::Command::new(editor)
        .arg(&path)
        .status()
        .ok();

    Ok(())
}

fn view_logs(app: &mut App) -> Result<()> {
    let task = match app.selected_task_ref() {
        Some(t) => t,
        None => return Ok(()),
    };

    let task_ref = format!("{}/{}", task.category, task.name);
    let logs = get_task_history(&app.config.logs_dir(), &task_ref, 20)?;

    app.viewing_logs = logs;
    app.mode = AppMode::ViewingLogs;
    app.focus = Focus::Details;
    app.detail_scroll = 0;

    Ok(())
}

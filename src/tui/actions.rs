use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::executor::{execute_workflow, ExecuteOpts};
use crate::core::logger::{get_task_history, write_run_log};
use crate::core::models::{ExecutionEvent, TaskKind};
use crate::core::parser::{parse_shell_task, parse_workflow};
use crate::error::Result;

use super::app::{App, AppMode, Focus};

pub fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match app.mode {
        AppMode::Search => handle_search_key(app, key),
        AppMode::Running => Ok(()),
        AppMode::Help => {
            // Any key dismisses help
            app.mode = AppMode::Normal;
            Ok(())
        }
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
        KeyCode::Down => app.move_down(),
        KeyCode::Up => app.move_up(),
        KeyCode::Tab | KeyCode::Right if app.focus != Focus::Details => app.focus_next(),
        KeyCode::BackTab | KeyCode::Left if app.focus != Focus::Sidebar => app.focus_prev(),
        KeyCode::Char('/') => app.start_search(),
        KeyCode::Char('r') => run_selected(app, false)?,
        KeyCode::Char('d') => run_selected(app, true)?,
        KeyCode::Char('e') => edit_selected(app)?,
        KeyCode::Char('l') if app.focus == Focus::Details => {
            // scroll down in details
            app.detail_scroll = app.detail_scroll.saturating_add(1);
        }
        KeyCode::Char('L') => view_logs(app)?,
        KeyCode::Char('h') => {
            app.mode = AppMode::Help;
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            if app.focus == Focus::TaskList {
                app.expanded_tasks.insert(app.selected_task);
            } else {
                app.collapsed.remove(&app.selected_category);
            }
        }
        KeyCode::Char('-') => {
            if app.focus == Focus::TaskList {
                app.expanded_tasks.remove(&app.selected_task);
            } else {
                app.collapsed.insert(app.selected_category);
            }
        }
        KeyCode::Enter => {
            if app.focus == Focus::Sidebar {
                app.toggle_collapse();
                if !app.is_collapsed(app.selected_category) {
                    app.focus = Focus::TaskList;
                    app.selected_task = 0;
                }
            } else if app.focus == Focus::TaskList {
                if app.expanded_tasks.contains(&app.selected_task) {
                    app.expanded_tasks.remove(&app.selected_task);
                } else {
                    app.expanded_tasks.insert(app.selected_task);
                }
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
    if app.is_executing {
        return Ok(());
    }

    let task = match app.selected_task_ref() {
        Some(t) => t.clone(),
        None => return Ok(()),
    };

    let workflow = match task.kind {
        TaskKind::ShellScript => parse_shell_task(&task.path)?,
        TaskKind::YamlWorkflow => parse_workflow(&task.path)?,
    };

    let task_ref = format!("{}/{}", task.category, task.name);

    let (tx, rx) = mpsc::channel::<ExecutionEvent>();
    app.event_rx = Some(rx);
    app.is_executing = true;
    app.executing_task_ref = Some(task_ref.clone());
    app.step_states.clear();
    app.footer_log.clear();
    app.footer_log.push(format!(
        "[{}] Starting {}{}...",
        chrono::Local::now().format("%H:%M:%S"),
        task_ref,
        if dry_run { " (dry-run)" } else { "" },
    ));
    app.mode = AppMode::Running;
    app.running_message = Some(format!("Running {}...", task_ref));

    let logs_dir = app.config.logs_dir();

    thread::spawn(move || {
        let opts = ExecuteOpts {
            dry_run,
            env_overrides: HashMap::new(),
        };

        match execute_workflow(&workflow, &task_ref, &opts, Some(&tx)) {
            Ok(run_log) => {
                if !dry_run {
                    let _ = write_run_log(&logs_dir, &run_log);
                }
                let _ = tx.send(ExecutionEvent::WorkflowFinished { run_log });
            }
            Err(e) => {
                let _ = tx.send(ExecutionEvent::WorkflowError {
                    message: e.to_string(),
                });
            }
        }
    });

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

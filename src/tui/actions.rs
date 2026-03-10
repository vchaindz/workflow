use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::db;
use crate::core::executor::{execute_workflow, ExecuteOpts};
use crate::core::models::{ExecutionEvent, TaskKind};
use crate::core::parser::{parse_shell_task, parse_workflow};
use crate::core::wizard;
use crate::error::Result;

use super::app::{App, AppMode, Focus, WizardStage, WizardState};

pub fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match app.mode {
        AppMode::Search => handle_search_key(app, key),
        AppMode::Running => Ok(()),
        AppMode::Help => {
            // Any key dismisses help
            app.mode = AppMode::Normal;
            Ok(())
        }
        AppMode::Wizard => handle_wizard_key(app, key),
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
        KeyCode::Char('w') => start_wizard(app)?,
        KeyCode::Char('h') => {
            app.mode = AppMode::Help;
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            app.collapsed.remove(&app.selected_category);
        }
        KeyCode::Char('-') => {
            app.collapsed.insert(app.selected_category);
        }
        KeyCode::Enter => {
            if app.focus == Focus::Sidebar {
                app.toggle_collapse();
                if !app.is_collapsed(app.selected_category) {
                    app.focus = Focus::TaskList;
                    app.selected_task = 0;
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

    let db_path = app.config.db_path();

    thread::spawn(move || {
        let opts = ExecuteOpts {
            dry_run,
            env_overrides: HashMap::new(),
        };

        match execute_workflow(&workflow, &task_ref, &opts, Some(&tx)) {
            Ok(run_log) => {
                if !dry_run {
                    if let Ok(conn) = db::open_db(&db_path) {
                        let _ = db::insert_run_log(&conn, &run_log);
                    }
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
    let conn = db::open_db(&app.config.db_path())?;
    let logs = db::get_task_history(&conn, &task_ref, 20)?;

    app.viewing_logs = logs;
    app.mode = AppMode::ViewingLogs;
    app.focus = Focus::Details;
    app.detail_scroll = 0;

    Ok(())
}

fn start_wizard(app: &mut App) -> Result<()> {
    let task = match app.selected_task_ref() {
        Some(t) => t.clone(),
        None => return Ok(()),
    };

    let workflow = match task.kind {
        TaskKind::ShellScript => parse_shell_task(&task.path)?,
        TaskKind::YamlWorkflow => parse_workflow(&task.path)?,
    };

    let task_ref = format!("{}/{}", task.category, task.name);

    // Try to load last run from DB
    let source_run = db::open_db(&app.config.db_path())
        .ok()
        .and_then(|conn| db::get_task_history(&conn, &task_ref, 1).ok())
        .and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) });

    let has_run = source_run.is_some();

    app.wizard = Some(WizardState {
        stage: WizardStage::Category,
        source_task_ref: task_ref,
        source_workflow: workflow,
        source_run,
        category: task.category.clone(),
        task_name: format!("{}-copy", task.name),
        category_cursor: None,
        remove_failed: has_run,
        remove_skipped: has_run,
        parallelize: false,
        preview_scroll: 0,
        active_toggle: 0,
        save_message: None,
    });
    app.mode = AppMode::Wizard;
    app.focus = Focus::Details;
    app.detail_scroll = 0;

    Ok(())
}

fn handle_wizard_key(app: &mut App, key: KeyEvent) -> Result<()> {
    // Ctrl-C always quits
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return Ok(());
    }

    let wiz = match app.wizard.as_mut() {
        Some(w) => w,
        None => {
            app.mode = AppMode::Normal;
            return Ok(());
        }
    };

    // If we just saved, any key dismisses
    if wiz.save_message.is_some() {
        app.wizard = None;
        app.mode = AppMode::Normal;
        return Ok(());
    }

    match key.code {
        KeyCode::Esc => {
            app.wizard = None;
            app.mode = AppMode::Normal;
        }
        _ => match wiz.stage {
            WizardStage::Category => handle_wizard_category(app, key)?,
            WizardStage::TaskName => handle_wizard_taskname(app, key),
            WizardStage::Options => handle_wizard_options(app, key),
            WizardStage::Preview => handle_wizard_preview(app, key)?,
        },
    }

    Ok(())
}

fn handle_wizard_category(app: &mut App, key: KeyEvent) -> Result<()> {
    let cat_names: Vec<String> = app.categories.iter().map(|c| c.name.clone()).collect();
    let wiz = app.wizard.as_mut().unwrap();

    match key.code {
        KeyCode::Enter | KeyCode::Tab => {
            if !wiz.category.is_empty() {
                wiz.stage = WizardStage::TaskName;
            }
        }
        KeyCode::Up => {
            if cat_names.is_empty() {
                return Ok(());
            }
            let cur = wiz.category_cursor.unwrap_or(0);
            let new = if cur == 0 { cat_names.len() - 1 } else { cur - 1 };
            wiz.category_cursor = Some(new);
            wiz.category = cat_names[new].clone();
        }
        KeyCode::Down => {
            if cat_names.is_empty() {
                return Ok(());
            }
            let cur = wiz.category_cursor.map(|c| c + 1).unwrap_or(0);
            let new = cur % cat_names.len();
            wiz.category_cursor = Some(new);
            wiz.category = cat_names[new].clone();
        }
        KeyCode::Backspace => {
            wiz.category.pop();
            wiz.category_cursor = None;
        }
        KeyCode::Char(c) => {
            wiz.category.push(c);
            wiz.category_cursor = None;
        }
        _ => {}
    }
    Ok(())
}

fn handle_wizard_taskname(app: &mut App, key: KeyEvent) {
    let wiz = app.wizard.as_mut().unwrap();
    match key.code {
        KeyCode::Enter | KeyCode::Tab => {
            if !wiz.task_name.is_empty() {
                wiz.stage = WizardStage::Options;
            }
        }
        KeyCode::Backspace => {
            wiz.task_name.pop();
        }
        KeyCode::Char(c) => {
            wiz.task_name.push(c);
        }
        _ => {}
    }
}

fn handle_wizard_options(app: &mut App, key: KeyEvent) {
    let wiz = app.wizard.as_mut().unwrap();
    match key.code {
        KeyCode::Enter | KeyCode::Tab => {
            wiz.stage = WizardStage::Preview;
            wiz.preview_scroll = 0;
        }
        KeyCode::Up => {
            wiz.active_toggle = wiz.active_toggle.saturating_sub(1);
        }
        KeyCode::Down => {
            if wiz.active_toggle < 2 {
                wiz.active_toggle += 1;
            }
        }
        KeyCode::Char(' ') => match wiz.active_toggle {
            0 => wiz.remove_failed = !wiz.remove_failed,
            1 => wiz.remove_skipped = !wiz.remove_skipped,
            2 => wiz.parallelize = !wiz.parallelize,
            _ => {}
        },
        _ => {}
    }
}

fn handle_wizard_preview(app: &mut App, key: KeyEvent) -> Result<()> {
    let wiz = app.wizard.as_ref().unwrap();
    match key.code {
        KeyCode::Up => {
            let wiz = app.wizard.as_mut().unwrap();
            wiz.preview_scroll = wiz.preview_scroll.saturating_sub(1);
        }
        KeyCode::Down => {
            let wiz = app.wizard.as_mut().unwrap();
            wiz.preview_scroll = wiz.preview_scroll.saturating_add(1);
        }
        KeyCode::Enter => {
            // Generate and save
            let optimized = wizard::optimize_workflow(
                &wiz.source_workflow,
                wiz.source_run.as_ref(),
                wiz.remove_failed,
                wiz.remove_skipped,
                wiz.parallelize,
            );
            let yaml = wizard::generate_yaml(&optimized);
            let category = wiz.category.clone();
            let task_name = wiz.task_name.clone();

            let path = wizard::save_task(
                &app.config.workflows_dir,
                &category,
                &task_name,
                &yaml,
            )?;

            let msg = format!("Saved: {}", path.display());
            app.wizard.as_mut().unwrap().save_message = Some(msg.clone());
            app.footer_log.push(format!(
                "[{}] Wizard: {}",
                chrono::Local::now().format("%H:%M:%S"),
                msg,
            ));
        }
        _ => {}
    }
    Ok(())
}

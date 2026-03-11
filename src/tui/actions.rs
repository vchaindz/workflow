use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::ai;
use crate::core::compare;
use crate::core::db;
use crate::core::executor::{execute_workflow, run_notify, ExecuteOpts};
use crate::core::history;
use crate::core::models::{ExecutionEvent, TaskKind};
use crate::core::parser::{parse_shell_task, parse_workflow};
use crate::core::wizard;
use crate::error::Result;

use super::app::{App, AppMode, DeleteState, Focus, WizardMode, WizardStage, WizardState};

pub fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match app.mode {
        AppMode::Search => handle_search_key(app, key),
        AppMode::Running => Ok(()),
        AppMode::Comparing => {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    app.compare_result = None;
                    app.mode = AppMode::Normal;
                }
                KeyCode::Up => app.detail_scroll = app.detail_scroll.saturating_sub(1),
                KeyCode::Down => app.detail_scroll = app.detail_scroll.saturating_add(1),
                _ => {}
            }
            Ok(())
        }
        AppMode::Help => {
            // Any key dismisses help
            app.mode = AppMode::Normal;
            Ok(())
        }
        AppMode::Wizard => handle_wizard_key(app, key),
        AppMode::ConfirmDelete => handle_confirm_delete_key(app, key),
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
        KeyCode::Char('w') => start_history_wizard(app),
        KeyCode::Char('W') => start_clone_wizard(app)?,
        KeyCode::Char('c') => compare_selected(app)?,
        KeyCode::Char('a') => start_ai_wizard(app),
        KeyCode::Delete => start_delete(app),
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

    let default_timeout = app.config.default_timeout;

    // Merge secrets: workflow + config
    let mut secrets = workflow.secrets.clone();
    for s in &app.config.secrets {
        if !secrets.contains(s) {
            secrets.push(s.clone());
        }
    }

    // Clone notify config for background thread
    let wf_notify = workflow.notify.clone();
    let cfg_notify = app.config.notify.clone();
    let task_ref_for_notify = task_ref.clone();

    thread::spawn(move || {
        let opts = ExecuteOpts {
            dry_run,
            env_overrides: HashMap::new(),
            default_timeout,
            secrets,
        };

        match execute_workflow(&workflow, &task_ref, &opts, Some(&tx)) {
            Ok(run_log) => {
                if !dry_run {
                    if let Ok(conn) = db::open_db(&db_path) {
                        let _ = db::insert_run_log(&conn, &run_log);
                    }

                    // Run notifications
                    let notify_on_failure = wf_notify.on_failure.as_ref()
                        .or(cfg_notify.on_failure.as_ref());
                    let notify_on_success = wf_notify.on_success.as_ref()
                        .or(cfg_notify.on_success.as_ref());

                    let mut notify_vars: HashMap<String, String> = HashMap::new();
                    notify_vars.insert("task_ref".to_string(), task_ref_for_notify);
                    notify_vars.insert("exit_code".to_string(), run_log.exit_code.to_string());

                    if run_log.exit_code == 0 {
                        if let Some(cmd) = notify_on_success {
                            run_notify(cmd, &notify_vars);
                        }
                    } else if let Some(cmd) = notify_on_failure {
                        run_notify(cmd, &notify_vars);
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

fn compare_selected(app: &mut App) -> Result<()> {
    let task = match app.selected_task_ref() {
        Some(t) => t.clone(),
        None => return Ok(()),
    };

    let task_ref = format!("{}/{}", task.category, task.name);
    let conn = db::open_db(&app.config.db_path())?;
    let history = db::get_task_history(&conn, &task_ref, 2)?;

    if history.len() < 2 {
        app.footer_log.push(format!(
            "[{}] Compare: need at least 2 runs (found {})",
            chrono::Local::now().format("%H:%M:%S"),
            history.len(),
        ));
        return Ok(());
    }

    // history is newest-first
    let result = compare::compare_runs(&history[1], &history[0]);
    app.compare_result = Some(result);
    app.mode = AppMode::Comparing;
    app.focus = Focus::Details;
    app.detail_scroll = 0;

    Ok(())
}

fn start_delete(app: &mut App) {
    let task = match app.selected_task_ref() {
        Some(t) => t.clone(),
        None => return,
    };

    app.delete_state = Some(DeleteState {
        task_name: task.name.clone(),
        task_path: task.path.clone(),
        category: task.category.clone(),
    });
    app.mode = AppMode::ConfirmDelete;
}

fn handle_confirm_delete_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return Ok(());
    }

    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            if let Some(ref state) = app.delete_state {
                let path = state.task_path.clone();
                let name = format!("{}/{}", state.category, state.task_name);

                match std::fs::remove_file(&path) {
                    Ok(()) => {
                        app.footer_log.push(format!(
                            "[{}] Deleted: {}",
                            chrono::Local::now().format("%H:%M:%S"),
                            name,
                        ));

                        // Remove from in-memory categories
                        for cat in &mut app.categories {
                            cat.tasks.retain(|t| t.path != path);
                        }
                        // Remove empty categories
                        app.categories.retain(|c| !c.tasks.is_empty());

                        // Fix selection bounds
                        if app.selected_category >= app.categories.len() && !app.categories.is_empty() {
                            app.selected_category = app.categories.len() - 1;
                        }
                        let task_count = app.categories.get(app.selected_category)
                            .map(|c| c.tasks.len()).unwrap_or(0);
                        if app.selected_task >= task_count && task_count > 0 {
                            app.selected_task = task_count - 1;
                        }
                    }
                    Err(e) => {
                        app.footer_log.push(format!(
                            "[{}] Delete failed: {}",
                            chrono::Local::now().format("%H:%M:%S"),
                            e,
                        ));
                    }
                }
            }
            app.delete_state = None;
            app.mode = AppMode::Normal;
        }
        _ => {
            // Any other key cancels
            app.delete_state = None;
            app.mode = AppMode::Normal;
        }
    }
    Ok(())
}

fn start_history_wizard(app: &mut App) {
    let entries = history::load_shell_history(5000);
    let filtered: Vec<usize> = (0..entries.len()).collect();

    app.wizard = Some(WizardState {
        mode: WizardMode::FromHistory,
        stage: WizardStage::ShellHistory,
        history_entries: entries,
        history_filter: String::new(),
        history_filtered: filtered,
        history_cursor: 0,
        history_selected: Vec::new(),
        history_scroll_offset: 0,
        source_task_ref: None,
        source_workflow: None,
        source_run: None,
        ai_prompt: String::new(),
        ai_tool: None,
        ai_result_rx: None,
        ai_commands: Vec::new(),
        ai_error: None,
        ai_tick: 0,
        category: String::new(),
        task_name: String::new(),
        category_cursor: None,
        remove_failed: false,
        remove_skipped: false,
        parallelize: false,
        active_toggle: 0,
        preview_scroll: 0,
        save_message: None,
    });
    app.mode = AppMode::Wizard;
}

fn start_clone_wizard(app: &mut App) -> Result<()> {
    let task = match app.selected_task_ref() {
        Some(t) => t.clone(),
        None => return Ok(()),
    };

    let workflow = match task.kind {
        TaskKind::ShellScript => parse_shell_task(&task.path)?,
        TaskKind::YamlWorkflow => parse_workflow(&task.path)?,
    };

    let task_ref = format!("{}/{}", task.category, task.name);

    let source_run = db::open_db(&app.config.db_path())
        .ok()
        .and_then(|conn| db::get_task_history(&conn, &task_ref, 1).ok())
        .and_then(|mut v| if v.is_empty() { None } else { Some(v.remove(0)) });

    let has_run = source_run.is_some();

    app.wizard = Some(WizardState {
        mode: WizardMode::CloneTask,
        stage: WizardStage::Category,
        history_entries: Vec::new(),
        history_filter: String::new(),
        history_filtered: Vec::new(),
        history_cursor: 0,
        history_selected: Vec::new(),
        history_scroll_offset: 0,
        source_task_ref: Some(task_ref),
        source_workflow: Some(workflow),
        source_run,
        ai_prompt: String::new(),
        ai_tool: None,
        ai_result_rx: None,
        ai_commands: Vec::new(),
        ai_error: None,
        ai_tick: 0,
        category: task.category.clone(),
        task_name: format!("{}-copy", task.name),
        category_cursor: None,
        remove_failed: has_run,
        remove_skipped: has_run,
        parallelize: false,
        active_toggle: 0,
        preview_scroll: 0,
        save_message: None,
    });
    app.mode = AppMode::Wizard;
    app.focus = Focus::Details;
    app.detail_scroll = 0;

    Ok(())
}

fn start_ai_wizard(app: &mut App) {
    let tool = match ai::detect_ai_tool() {
        Some(t) => t,
        None => {
            app.footer_log.push(format!(
                "[{}] No AI tool found (install `claude` or `codex`)",
                chrono::Local::now().format("%H:%M:%S"),
            ));
            return;
        }
    };

    app.wizard = Some(WizardState {
        mode: WizardMode::AiChat,
        stage: WizardStage::AiPrompt,
        history_entries: Vec::new(),
        history_filter: String::new(),
        history_filtered: Vec::new(),
        history_cursor: 0,
        history_selected: Vec::new(),
        history_scroll_offset: 0,
        source_task_ref: None,
        source_workflow: None,
        source_run: None,
        ai_prompt: String::new(),
        ai_tool: Some(tool),
        ai_result_rx: None,
        ai_commands: Vec::new(),
        ai_error: None,
        ai_tick: 0,
        category: String::new(),
        task_name: String::new(),
        category_cursor: None,
        remove_failed: false,
        remove_skipped: false,
        parallelize: false,
        active_toggle: 0,
        preview_scroll: 0,
        save_message: None,
    });
    app.mode = AppMode::Wizard;
}

fn handle_wizard_ai_prompt(app: &mut App, key: KeyEvent) {
    let wiz = app.wizard.as_mut().unwrap();

    match key.code {
        KeyCode::Enter => {
            if !wiz.ai_prompt.trim().is_empty() {
                let tool = wiz.ai_tool.unwrap();
                let prompt = wiz.ai_prompt.clone();
                let (tx, rx) = mpsc::channel();

                wiz.ai_result_rx = Some(rx);
                wiz.ai_error = None;
                wiz.ai_tick = 0;
                wiz.stage = WizardStage::AiThinking;

                thread::spawn(move || {
                    let result = ai::invoke_ai(tool, &prompt);
                    let _ = tx.send(result);
                });
            }
        }
        KeyCode::Backspace => {
            wiz.ai_prompt.pop();
        }
        KeyCode::Char(c) => {
            wiz.ai_prompt.push(c);
        }
        _ => {}
    }
}

fn handle_wizard_ai_thinking(app: &mut App, key: KeyEvent) {
    let wiz = app.wizard.as_mut().unwrap();
    match key.code {
        KeyCode::Esc => {
            // If there's an error displayed, go back to prompt to retry
            if wiz.ai_error.is_some() {
                wiz.ai_error = None;
                wiz.stage = WizardStage::AiPrompt;
            }
            // Otherwise spinner is running — Esc cancels the whole wizard
            // (handled by parent match in handle_wizard_key)
        }
        _ => {}
    }
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
            WizardStage::ShellHistory => handle_wizard_history(app, key),
            WizardStage::AiPrompt => handle_wizard_ai_prompt(app, key),
            WizardStage::AiThinking => handle_wizard_ai_thinking(app, key),
            WizardStage::Category => handle_wizard_category(app, key)?,
            WizardStage::TaskName => handle_wizard_taskname(app, key),
            WizardStage::Options => handle_wizard_options(app, key),
            WizardStage::Preview => handle_wizard_preview(app, key)?,
        },
    }

    Ok(())
}

fn handle_wizard_history(app: &mut App, key: KeyEvent) {
    let wiz = app.wizard.as_mut().unwrap();

    match key.code {
        KeyCode::Up => {
            if wiz.history_cursor > 0 {
                wiz.history_cursor -= 1;
                if wiz.history_cursor < wiz.history_scroll_offset {
                    wiz.history_scroll_offset = wiz.history_cursor;
                }
            }
        }
        KeyCode::Down => {
            if !wiz.history_filtered.is_empty()
                && wiz.history_cursor + 1 < wiz.history_filtered.len()
            {
                wiz.history_cursor += 1;
                // Scroll viewport (visible area ~20 lines for history)
                let visible = 20usize;
                if wiz.history_cursor >= wiz.history_scroll_offset + visible {
                    wiz.history_scroll_offset = wiz.history_cursor.saturating_sub(visible - 1);
                }
            }
        }
        KeyCode::Char(' ') => {
            if let Some(&real_idx) = wiz.history_filtered.get(wiz.history_cursor) {
                // Toggle selection (ordered)
                if let Some(pos) = wiz.history_selected.iter().position(|&i| i == real_idx) {
                    wiz.history_selected.remove(pos);
                } else {
                    wiz.history_selected.push(real_idx);
                }
            }
        }
        KeyCode::Enter => {
            if !wiz.history_selected.is_empty() {
                // Gather selected commands in selection order
                let commands: Vec<&str> = wiz
                    .history_selected
                    .iter()
                    .filter_map(|&i| wiz.history_entries.get(i))
                    .map(|e| e.command.as_str())
                    .collect();

                wiz.category = history::suggest_category(&commands);
                if let Some(first) = commands.first() {
                    wiz.task_name = history::derive_task_name(first);
                }
                wiz.stage = WizardStage::Category;
            }
        }
        KeyCode::Backspace => {
            wiz.history_filter.pop();
            update_history_filter(wiz);
        }
        KeyCode::Char(c) => {
            wiz.history_filter.push(c);
            update_history_filter(wiz);
        }
        _ => {}
    }
}

fn update_history_filter(wiz: &mut WizardState) {
    let query = wiz.history_filter.to_lowercase();
    wiz.history_filtered = if query.is_empty() {
        (0..wiz.history_entries.len()).collect()
    } else {
        wiz.history_entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.command.to_lowercase().contains(&query))
            .map(|(i, _)| i)
            .collect()
    };
    wiz.history_cursor = 0;
    wiz.history_scroll_offset = 0;
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
        KeyCode::BackTab => {
            match wiz.mode {
                WizardMode::FromHistory => wiz.stage = WizardStage::ShellHistory,
                WizardMode::AiChat => wiz.stage = WizardStage::AiPrompt,
                WizardMode::CloneTask => {}
            }
            return Ok(());
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
                match wiz.mode {
                    WizardMode::CloneTask => wiz.stage = WizardStage::Options,
                    WizardMode::FromHistory | WizardMode::AiChat => {
                        wiz.stage = WizardStage::Preview;
                        wiz.preview_scroll = 0;
                    }
                }
            }
        }
        KeyCode::BackTab => {
            wiz.stage = WizardStage::Category;
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
        KeyCode::BackTab => {
            wiz.stage = WizardStage::TaskName;
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
        KeyCode::BackTab => {
            let wiz = app.wizard.as_mut().unwrap();
            match wiz.mode {
                WizardMode::CloneTask => wiz.stage = WizardStage::Options,
                WizardMode::FromHistory | WizardMode::AiChat => wiz.stage = WizardStage::TaskName,
            }
            return Ok(());
        }
        KeyCode::Up => {
            let wiz = app.wizard.as_mut().unwrap();
            wiz.preview_scroll = wiz.preview_scroll.saturating_sub(1);
        }
        KeyCode::Down => {
            let wiz = app.wizard.as_mut().unwrap();
            wiz.preview_scroll = wiz.preview_scroll.saturating_add(1);
        }
        KeyCode::Enter => {
            let yaml = match wiz.mode {
                WizardMode::FromHistory => {
                    let commands: Vec<String> = wiz
                        .history_selected
                        .iter()
                        .filter_map(|&i| wiz.history_entries.get(i))
                        .map(|e| e.command.clone())
                        .collect();
                    let wf = wizard::workflow_from_commands(&wiz.task_name, &commands);
                    wizard::generate_yaml(&wf)
                }
                WizardMode::AiChat => {
                    let wf = wizard::workflow_from_commands(&wiz.task_name, &wiz.ai_commands);
                    wizard::generate_yaml(&wf)
                }
                WizardMode::CloneTask => {
                    let source_wf = wiz.source_workflow.as_ref().unwrap();
                    let optimized = wizard::optimize_workflow(
                        source_wf,
                        wiz.source_run.as_ref(),
                        wiz.remove_failed,
                        wiz.remove_skipped,
                        wiz.parallelize,
                    );
                    wizard::generate_yaml(&optimized)
                }
            };
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

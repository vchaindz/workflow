use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::ai;
use crate::core::catalog;
use crate::core::compare;
use crate::core::db;
use crate::core::executor::{execute_workflow, load_secret_env, send_notifications, ExecuteOpts, StreamingRequest};
use crate::core::history;
use crate::core::models::{ExecutionEvent, RuntimeVariable, Task, TaskHeat, TaskKind, Workflow};
use crate::core::parser::{parse_shell_task, parse_workflow, parse_workflow_from_str};
use crate::core::wizard;
use crate::error::Result;

use super::app::{App, AppMode, DeleteState, EditState, Focus, RenameState, RenameTarget, SecretsMode, SecretsState, WizardMode, WizardStage, WizardState};

pub fn handle_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match app.mode {
        AppMode::Search => handle_search_key(app, key),
        AppMode::Running => {
            if key.code == KeyCode::Char('b') {
                app.background_current_task();
            }
            Ok(())
        }
        AppMode::StreamingOutput => {
            if key.code == KeyCode::Char('b') {
                app.background_current_task();
                return Ok(());
            }
            handle_streaming_key(app, key)
        }
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
        AppMode::RecentRuns => handle_recent_runs_key(app, key),
        AppMode::SavedTasks => handle_saved_tasks_key(app, key),
        AppMode::OverdueReminder => handle_overdue_key(app, key),
        AppMode::VariablePrompt => handle_variable_prompt_key(app, key),
        AppMode::GitSync => handle_git_sync_key(app, key),
        AppMode::Wizard => handle_wizard_key(app, key),
        AppMode::ConfirmDelete => handle_confirm_delete_key(app, key),
        AppMode::Rename => handle_rename_key(app, key),
        AppMode::EditTask => handle_edit_task_key(app, key),
        AppMode::Secrets => handle_secrets_key(app, key),
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
            app.run_output_task_path = None;
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
        KeyCode::Char('B') => app.view_background_result(),
        KeyCode::Char('L') => view_logs(app)?,
        KeyCode::Char('w') => start_history_wizard(app),
        KeyCode::Char('W') => start_clone_wizard(app)?,
        KeyCode::Char('c') => compare_selected(app)?,
        KeyCode::Char('a') => {
            if app.run_output.as_ref().map(|r| r.exit_code != 0).unwrap_or(false)
                && app.run_output_task_path.is_some()
            {
                start_ai_fix_from_run(app);
            } else {
                start_ai_wizard(app);
            }
        }
        KeyCode::Char('A') => start_ai_update_wizard(app),
        KeyCode::Char('t') => start_template_wizard(app),
        KeyCode::Char('R') => open_recent_runs(app)?,
        KeyCode::Char('s') => open_saved_tasks(app),
        KeyCode::Char('S') => toggle_bookmark(app),
        KeyCode::Char('o') => app.toggle_sort(),
        KeyCode::Char('F') => app.cycle_status_filter(),
        KeyCode::Char('g') => {
            app.refresh_sync_status();
            app.sync_menu_cursor = 0;
            app.sync_message = None;
            app.sync_setup_stage = super::app::SyncSetupStage::Menu;
            app.sync_setup_input.clear();
            app.mode = AppMode::GitSync;
        }
        KeyCode::Char('K') => open_secrets(app),
        KeyCode::Char('m') | KeyCode::F(2) => start_rename(app),
        KeyCode::Char('T') => empty_trash(app),
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

fn handle_streaming_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.close_streaming_modal();
        return Ok(());
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.close_streaming_modal();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.streaming_auto_scroll = false;
            app.streaming_scroll = app.streaming_scroll.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let max = (app.streaming_lines.len() as u16).saturating_sub(1);
            if app.streaming_scroll < max {
                app.streaming_scroll += 1;
            }
            // Re-enable auto-scroll if we're at the bottom
            if app.streaming_scroll >= max {
                app.streaming_auto_scroll = true;
            }
        }
        KeyCode::Home => {
            app.streaming_auto_scroll = false;
            app.streaming_scroll = 0;
        }
        KeyCode::End => {
            app.streaming_auto_scroll = true;
            app.streaming_scroll = (app.streaming_lines.len() as u16).saturating_sub(1);
        }
        KeyCode::PageUp => {
            app.streaming_auto_scroll = false;
            app.streaming_scroll = app.streaming_scroll.saturating_sub(20);
        }
        KeyCode::PageDown => {
            let max = (app.streaming_lines.len() as u16).saturating_sub(1);
            app.streaming_scroll = (app.streaming_scroll + 20).min(max);
            if app.streaming_scroll >= max {
                app.streaming_auto_scroll = true;
            }
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

    // Check for variables with choices_cmd that need prompting
    let prompt_vars: Vec<RuntimeVariable> = workflow
        .variables
        .iter()
        .filter(|v| v.choices_cmd.is_some())
        .cloned()
        .collect();

    if !prompt_vars.is_empty() {
        // Seed resolved map with defaults for non-prompt variables
        let mut resolved = HashMap::new();
        for v in &workflow.variables {
            if v.choices_cmd.is_none() {
                if let Some(ref default) = v.default {
                    resolved.insert(v.name.clone(), default.clone());
                }
            }
        }
        app.var_prompt_vars = prompt_vars;
        app.var_prompt_index = 0;
        app.var_prompt_resolved = resolved;
        app.var_prompt_dry_run = dry_run;
        app.var_prompt_task = Some(task);
        app.var_prompt_workflow = Some(workflow);
        app.var_prompt_error = None;
        start_loading_choices(app);
        return Ok(());
    }

    launch_workflow(app, &task, workflow, dry_run, HashMap::new())
}

fn start_loading_choices(app: &mut App) {
    let idx = app.var_prompt_index;
    let var = &app.var_prompt_vars[idx];
    let cmd = match var.choices_cmd.as_ref() {
        Some(c) => c.clone(),
        None => return,
    };

    // Run choices_cmd with a 5-second timeout to avoid hanging the TUI.
    // NOTE: choices_cmd runs shell commands defined in workflow YAML templates.
    // Only use templates from trusted sources — a malicious template could execute
    // arbitrary commands at variable-selection time, before the workflow itself runs.
    let child = std::process::Command::new("bash")
        .arg("-c")
        .arg(&cmd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    match child {
        Ok(child) => {
            use std::time::{Duration, Instant};
            let timeout = Duration::from_secs(5);
            let start = Instant::now();
            let mut child = child;

            // Poll for completion with timeout
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        let output = child.wait_with_output().unwrap_or_else(|_| {
                            std::process::Output {
                                status,
                                stdout: Vec::new(),
                                stderr: Vec::new(),
                            }
                        });
                        if output.status.success() {
                            let stdout = String::from_utf8_lossy(&output.stdout);
                            let choices: Vec<String> = stdout
                                .lines()
                                .map(|l| l.trim().to_string())
                                .filter(|l| !l.is_empty())
                                .collect();
                            if choices.is_empty() {
                                app.var_prompt_error = Some(format!("'{}' returned no results", cmd));
                            }
                            app.var_prompt_choices = choices;
                            app.var_prompt_cursor = 0;
                            app.var_prompt_scroll = 0;
                        } else {
                            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                            let msg = if stderr.is_empty() { "non-zero exit".to_string() } else { stderr };
                            app.var_prompt_error = Some(format!("Command failed: {}", msg));
                            app.var_prompt_choices = Vec::new();
                        }
                        break;
                    }
                    Ok(None) => {
                        if start.elapsed() >= timeout {
                            let _ = child.kill();
                            let _ = child.wait();
                            app.var_prompt_error = Some(format!("'{}' timed out after {}s", cmd, timeout.as_secs()));
                            app.var_prompt_choices = Vec::new();
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(e) => {
                        app.var_prompt_error = Some(format!("Failed to wait: {}", e));
                        app.var_prompt_choices = Vec::new();
                        break;
                    }
                }
            }
        }
        Err(e) => {
            app.var_prompt_error = Some(format!("Failed to run: {}", e));
            app.var_prompt_choices = Vec::new();
        }
    }
    app.mode = AppMode::VariablePrompt;
}

fn handle_variable_prompt_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            // Cancel — return to normal
            app.mode = AppMode::Normal;
            app.var_prompt_vars.clear();
            app.var_prompt_choices.clear();
            app.var_prompt_resolved.clear();
            app.var_prompt_task = None;
            app.var_prompt_workflow = None;
            app.var_prompt_error = None;
        }
        KeyCode::Up => {
            if app.var_prompt_cursor > 0 {
                app.var_prompt_cursor -= 1;
                if app.var_prompt_cursor < app.var_prompt_scroll {
                    app.var_prompt_scroll = app.var_prompt_cursor;
                }
            }
        }
        KeyCode::Down => {
            if !app.var_prompt_choices.is_empty()
                && app.var_prompt_cursor + 1 < app.var_prompt_choices.len()
            {
                app.var_prompt_cursor += 1;
            }
        }
        KeyCode::Enter => {
            if app.var_prompt_choices.is_empty() {
                // If error/empty, Enter dismisses like Esc
                app.mode = AppMode::Normal;
                app.var_prompt_vars.clear();
                app.var_prompt_task = None;
                app.var_prompt_workflow = None;
                return Ok(());
            }
            // Record selection
            let var_name = app.var_prompt_vars[app.var_prompt_index].name.clone();
            let chosen = app.var_prompt_choices[app.var_prompt_cursor].clone();
            app.var_prompt_resolved.insert(var_name, chosen);

            // Advance to next variable or execute
            if app.var_prompt_index + 1 < app.var_prompt_vars.len() {
                app.var_prompt_index += 1;
                app.var_prompt_error = None;
                start_loading_choices(app);
            } else {
                // All variables resolved — launch
                let task = match app.var_prompt_task.take() {
                    Some(t) => t,
                    None => {
                        app.mode = AppMode::Normal;
                        return Ok(());
                    }
                };
                let workflow = match app.var_prompt_workflow.take() {
                    Some(w) => w,
                    None => {
                        app.mode = AppMode::Normal;
                        return Ok(());
                    }
                };
                let dry_run = app.var_prompt_dry_run;
                let env_overrides = app.var_prompt_resolved.clone();
                app.var_prompt_vars.clear();
                app.var_prompt_choices.clear();
                app.var_prompt_resolved.clear();
                app.var_prompt_error = None;
                launch_workflow(app, &task, workflow, dry_run, env_overrides)?;
            }
        }
        KeyCode::Char('a') if app.var_prompt_error.is_some() => {
            start_ai_fix_from_var_prompt(app)?;
        }
        _ => {}
    }
    Ok(())
}

fn launch_workflow(
    app: &mut App,
    task: &Task,
    workflow: Workflow,
    dry_run: bool,
    env_overrides: HashMap<String, String>,
) -> Result<()> {
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

    // Create streaming channel for in-TUI output modal
    let (streaming_tx, streaming_rx) = mpsc::channel::<StreamingRequest>();
    app.streaming_rx = Some(streaming_rx);
    app.interactive_rx = None;

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
    let wf_name = workflow.name.clone();
    let cfg_notify = app.config.notify.clone();
    let task_ref_for_notify = task_ref.clone();
    let workflows_dir_for_thread = app.config.workflows_dir.clone();
    let secrets_ssh_key = app.config.secrets_ssh_key.as_ref().map(std::path::PathBuf::from);
    let mcp_servers_for_thread = app.config.mcp.servers.clone();

    thread::spawn(move || {
        let workflows_dir = workflows_dir_for_thread;
        let secrets_clone = secrets.clone();
        let workflows_dir_clone = workflows_dir.clone();
        let ssh_key_clone = secrets_ssh_key.clone();
        let opts = ExecuteOpts {
            dry_run,
            force: false,
            env_overrides,
            default_timeout,
            secrets,
            interactive_tx: None,
            streaming_tx: Some(streaming_tx),
            workflows_dir: Some(workflows_dir),
            call_depth: 0,
            max_call_depth: 10,
            secrets_ssh_key,
            mcp_servers: mcp_servers_for_thread,
        };

        match execute_workflow(&workflow, &task_ref, &opts, Some(&tx)) {
            Ok(run_log) => {
                if !dry_run {
                    if let Ok(conn) = db::open_db(&db_path) {
                        let _ = db::insert_run_log_with_source(&conn, &run_log, "tui");
                    }
                }
                // Send WorkflowFinished before notifications so the TUI
                // exits Running mode immediately (notifications may be slow).
                let _ = tx.send(ExecutionEvent::WorkflowFinished { run_log: run_log.clone() });

                if !dry_run {
                    let secret_env = load_secret_env(
                        &secrets_clone,
                        &workflows_dir_clone,
                        ssh_key_clone.as_deref(),
                    );
                    send_notifications(&task_ref_for_notify, &run_log, &wf_name, &wf_notify, &cfg_notify, &secret_env);
                }
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

fn edit_selected(app: &mut App) -> Result<()> {
    let task = match app.selected_task_ref() {
        Some(t) => t.clone(),
        None => return Ok(()),
    };

    let display_name = format!("{}/{}", task.category, task.name);
    match EditState::from_file(&task.path, &display_name) {
        Ok(state) => {
            app.edit_state = Some(state);
            app.mode = AppMode::EditTask;
        }
        Err(e) => {
            app.footer_log.push(format!(
                "[{}] Edit failed: {}",
                chrono::Local::now().format("%H:%M:%S"),
                e,
            ));
        }
    }
    Ok(())
}

fn handle_edit_task_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return Ok(());
    }

    // Ctrl+S: save
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
        if let Some(ref mut state) = app.edit_state {
            match state.save() {
                Ok(()) => {
                    app.footer_log.push(format!(
                        "[{}] Saved {}",
                        chrono::Local::now().format("%H:%M:%S"),
                        state.file_name,
                    ));
                }
                Err(e) => {
                    app.footer_log.push(format!(
                        "[{}] Save failed: {}",
                        chrono::Local::now().format("%H:%M:%S"),
                        e,
                    ));
                }
            }
        }
        return Ok(());
    }

    // Handle cases that need to drop the borrow before modifying app
    {
        let state = match app.edit_state.as_ref() {
            Some(s) => s,
            None => return Ok(()),
        };

        // Discard confirmation: 'y' closes editor
        if state.confirm_discard && matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
            app.edit_state = None;
            app.mode = AppMode::Normal;
            return Ok(());
        }

        // Esc on unmodified file: close editor
        if key.code == KeyCode::Esc && !state.modified {
            app.edit_state = None;
            app.mode = AppMode::Normal;
            return Ok(());
        }
    }

    let state = match app.edit_state.as_mut() {
        Some(s) => s,
        None => return Ok(()),
    };

    // Handle discard confirmation (non-close keys)
    if state.confirm_discard {
        state.confirm_discard = false;
        return Ok(());
    }

    match key.code {
        KeyCode::Esc => {
            // Modified file: show discard prompt
            state.confirm_discard = true;
        }
        KeyCode::Up => {
            if state.cursor_row > 0 {
                state.cursor_row -= 1;
                state.clamp_cursor();
            }
        }
        KeyCode::Down => {
            if state.cursor_row + 1 < state.lines.len() {
                state.cursor_row += 1;
                state.clamp_cursor();
            }
        }
        KeyCode::Left => {
            if state.cursor_col > 0 {
                state.cursor_col -= 1;
            } else if state.cursor_row > 0 {
                state.cursor_row -= 1;
                state.cursor_col = state.lines[state.cursor_row].len();
            }
        }
        KeyCode::Right => {
            let line_len = state.lines[state.cursor_row].len();
            if state.cursor_col < line_len {
                state.cursor_col += 1;
            } else if state.cursor_row + 1 < state.lines.len() {
                state.cursor_row += 1;
                state.cursor_col = 0;
            }
        }
        KeyCode::Home => {
            state.cursor_col = 0;
        }
        KeyCode::End => {
            state.cursor_col = state.lines[state.cursor_row].len();
        }
        KeyCode::PageUp => {
            state.cursor_row = state.cursor_row.saturating_sub(20);
            state.clamp_cursor();
        }
        KeyCode::PageDown => {
            state.cursor_row = (state.cursor_row + 20).min(state.lines.len().saturating_sub(1));
            state.clamp_cursor();
        }
        KeyCode::Char(c) => {
            let row = state.cursor_row;
            let col = state.cursor_col;
            state.lines[row].insert(col, c);
            state.cursor_col += 1;
            state.modified = true;
        }
        KeyCode::Tab => {
            let row = state.cursor_row;
            let col = state.cursor_col;
            state.lines[row].insert_str(col, "  ");
            state.cursor_col += 2;
            state.modified = true;
        }
        KeyCode::Backspace => {
            let row = state.cursor_row;
            let col = state.cursor_col;
            if col > 0 {
                state.lines[row].remove(col - 1);
                state.cursor_col -= 1;
                state.modified = true;
            } else if row > 0 {
                let removed = state.lines.remove(row);
                state.cursor_row -= 1;
                state.cursor_col = state.lines[state.cursor_row].len();
                state.lines[state.cursor_row].push_str(&removed);
                state.modified = true;
            }
        }
        KeyCode::Delete => {
            let row = state.cursor_row;
            let col = state.cursor_col;
            let line_len = state.lines[row].len();
            if col < line_len {
                state.lines[row].remove(col);
                state.modified = true;
            } else if row + 1 < state.lines.len() {
                let next = state.lines.remove(row + 1);
                state.lines[row].push_str(&next);
                state.modified = true;
            }
        }
        KeyCode::Enter => {
            let row = state.cursor_row;
            let col = state.cursor_col;
            let rest = state.lines[row][col..].to_string();
            state.lines[row].truncate(col);
            state.cursor_row += 1;
            state.lines.insert(state.cursor_row, rest);
            state.cursor_col = 0;
            state.modified = true;
        }
        _ => {}
    }

    // Adjust scroll
    if let Ok((w, h)) = crossterm::terminal::size() {
        // Approximate visible area: full screen minus margins and gutter
        let gutter = 5;
        let editor_h = (h as usize).saturating_sub(5); // top/bottom margin + status bar
        let editor_w = (w as usize).saturating_sub(8 + gutter); // side margins + gutter
        state.ensure_visible(editor_h, editor_w);
    }

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

fn handle_git_sync_key(app: &mut App, key: KeyEvent) -> Result<()> {
    use crate::core::sync;
    use super::app::SyncSetupStage;

    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return Ok(());
    }

    match app.sync_setup_stage {
        SyncSetupStage::RepoUrl => {
            match key.code {
                KeyCode::Esc => {
                    app.sync_setup_stage = SyncSetupStage::Menu;
                    app.sync_setup_input.clear();
                }
                KeyCode::Backspace => { app.sync_setup_input.pop(); }
                KeyCode::Char(c) => { app.sync_setup_input.push(c); }
                KeyCode::Enter => {
                    let url = app.sync_setup_input.trim().to_string();
                    if !url.is_empty() {
                        let dir = app.config.workflows_dir.clone();
                        match sync::setup_remote(&dir, &url) {
                            Ok(()) => {
                                app.config.sync.remote_url = Some(url);
                                app.config.sync.enabled = true;
                                let _ = app.config.save_sync_config();
                                app.sync_message = Some(("Remote configured. Sync enabled.".to_string(), false));
                                app.refresh_sync_status();
                            }
                            Err(e) => {
                                app.sync_message = Some((format!("Error: {e}"), true));
                            }
                        }
                    }
                    app.sync_setup_stage = SyncSetupStage::Menu;
                    app.sync_setup_input.clear();
                }
                _ => {}
            }
            return Ok(());
        }

        SyncSetupStage::Menu => {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    app.mode = AppMode::Normal;
                }
                KeyCode::Up => {
                    app.sync_menu_cursor = app.sync_menu_cursor.saturating_sub(1);
                }
                KeyCode::Down => {
                    app.sync_menu_cursor = app.sync_menu_cursor.saturating_add(1);
                }
                KeyCode::Enter => {
                    let dir = app.config.workflows_dir.clone();
                    let is_repo = sync::is_repo(&dir);
                    let has_remote = app.sync_info.as_ref()
                        .and_then(|i| i.remote_url.as_ref())
                        .is_some();

                    if !is_repo {
                        // Menu: [Init repo] [Clone existing]
                        match app.sync_menu_cursor {
                            0 => {
                                // Init
                                match sync::init_repo(&dir) {
                                    Ok(()) => {
                                        app.config.sync.enabled = true;
                                        let _ = app.config.save_sync_config();
                                        app.sync_message = Some(("Git repo initialized.".to_string(), false));
                                        app.refresh_sync_status();
                                    }
                                    Err(e) => {
                                        app.sync_message = Some((format!("Error: {e}"), true));
                                    }
                                }
                            }
                            _ => {
                                // Prompt for URL to clone
                                app.sync_setup_stage = SyncSetupStage::RepoUrl;
                                app.sync_setup_input.clear();
                            }
                        }
                    } else if !has_remote {
                        // Menu: [Add remote URL] [Create GitHub repo (gh)]
                        match app.sync_menu_cursor {
                            0 => {
                                app.sync_setup_stage = SyncSetupStage::RepoUrl;
                                app.sync_setup_input.clear();
                            }
                            _ => {
                                // Create via gh
                                if sync::detect_gh() {
                                    match sync::create_private_repo(&dir) {
                                        Ok(url) => {
                                            app.config.sync.remote_url = Some(url.clone());
                                            app.config.sync.enabled = true;
                                            let _ = app.config.save_sync_config();
                                            app.sync_message = Some((format!("Created: {url}"), false));
                                            app.refresh_sync_status();
                                        }
                                        Err(e) => {
                                            app.sync_message = Some((format!("Error: {e}"), true));
                                        }
                                    }
                                } else {
                                    app.sync_message = Some(("gh CLI not found".to_string(), true));
                                }
                            }
                        }
                    } else {
                        // Menu: [Push] [Pull] [Status] [Toggle auto-sync]
                        match app.sync_menu_cursor {
                            0 => {
                                // Push
                                match sync::auto_commit(&dir) {
                                    Ok(Some(msg)) => {
                                        app.footer_log.push(format!(
                                            "[{}] ● sync: {msg}",
                                            chrono::Local::now().format("%H:%M:%S"),
                                        ));
                                    }
                                    Ok(None) => {}
                                    Err(e) => {
                                        app.sync_message = Some((format!("Commit error: {e}"), true));
                                        return Ok(());
                                    }
                                }
                                let branch = app.config.sync.branch.clone();
                                match sync::push(&dir, &branch) {
                                    Ok(()) => {
                                        app.sync_message = Some((format!("Pushed to origin/{branch}"), false));
                                    }
                                    Err(e) => {
                                        app.sync_message = Some((format!("Push error: {e}"), true));
                                    }
                                }
                                app.refresh_sync_status();
                            }
                            1 => {
                                // Pull
                                let branch = app.config.sync.branch.clone();
                                match sync::pull(&dir, &branch) {
                                    Ok(sync::PullResult::UpToDate) => {
                                        app.sync_message = Some(("Already up to date.".to_string(), false));
                                    }
                                    Ok(sync::PullResult::Updated(n)) => {
                                        app.sync_message = Some((format!("Pulled {n} update(s)."), false));
                                    }
                                    Ok(sync::PullResult::Conflict(files)) => {
                                        app.sync_message = Some((format!("Conflicts in {} file(s)! Resolve manually.", files.len()), true));
                                    }
                                    Err(e) => {
                                        app.sync_message = Some((format!("Pull error: {e}"), true));
                                    }
                                }
                                app.refresh_sync_status();
                            }
                            2 => {
                                // Refresh status
                                app.refresh_sync_status();
                                app.sync_message = Some(("Status refreshed.".to_string(), false));
                            }
                            3 => {
                                // Toggle auto-sync
                                app.config.sync.enabled = !app.config.sync.enabled;
                                let _ = app.config.save_sync_config();
                                let state = if app.config.sync.enabled { "enabled" } else { "disabled" };
                                app.sync_message = Some((format!("Auto-sync {state}."), false));
                            }
                            4 => {
                                // Switch branch
                                match sync::list_branches(&dir) {
                                    Ok(branches) => {
                                        let current_idx = branches.iter().position(|b| b.is_current).unwrap_or(0);
                                        app.branch_list = branches;
                                        app.branch_list_cursor = current_idx;
                                        app.sync_setup_stage = SyncSetupStage::BranchList;
                                        app.sync_message = None;
                                    }
                                    Err(e) => {
                                        app.sync_message = Some((format!("Error: {e}"), true));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        SyncSetupStage::BranchList => {
            match key.code {
                KeyCode::Esc => {
                    app.sync_setup_stage = SyncSetupStage::Menu;
                }
                KeyCode::Up => {
                    app.branch_list_cursor = app.branch_list_cursor.saturating_sub(1);
                }
                KeyCode::Down => {
                    if !app.branch_list.is_empty() {
                        app.branch_list_cursor = (app.branch_list_cursor + 1).min(app.branch_list.len() - 1);
                    }
                }
                KeyCode::Enter => {
                    if let Some(branch) = app.branch_list.get(app.branch_list_cursor) {
                        let target = branch.name.clone();
                        let dir = app.config.workflows_dir.clone();
                        match sync::switch_branch(&dir, &target) {
                            Ok(sync::SwitchResult::Switched { from, to, committed }) => {
                                let mut msg = format!("Switched from '{from}' to '{to}'.");
                                if committed {
                                    msg = format!("Auto-committed on '{from}'. {msg}");
                                }
                                app.config.sync.branch = to;
                                let _ = app.config.save_sync_config();
                                app.sync_message = Some((msg, false));

                                // Rescan workflows for new branch content
                                if let Ok(cats) = crate::core::discovery::scan_workflows(&dir) {
                                    app.categories = cats;
                                    app.selected_category = 0;
                                    app.selected_task = 0;
                                    app.load_heat_data();
                                    app.load_last_run_data();
                                    app.build_step_cmd_cache();
                                    app.refresh_stats();
                                    if app.sort_by_heat {
                                        app.apply_sort();
                                    }
                                }
                            }
                            Ok(sync::SwitchResult::AlreadyOnBranch) => {
                                app.sync_message = Some((format!("Already on '{target}'."), false));
                            }
                            Ok(sync::SwitchResult::Conflict(msg)) => {
                                app.sync_message = Some((format!("Switch failed: {msg}"), true));
                            }
                            Err(e) => {
                                app.sync_message = Some((format!("Error: {e}"), true));
                            }
                        }
                        app.sync_setup_stage = SyncSetupStage::Menu;
                        app.refresh_sync_status();
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
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

                // Soft delete: move to .trash/ directory
                let trash_dir = app.config.workflows_dir.join(".trash");
                let trash_result = (|| -> std::io::Result<()> {
                    std::fs::create_dir_all(&trash_dir)?;
                    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
                    let filename = path.file_name().unwrap_or_default().to_string_lossy();
                    let trash_name = format!("{}_{}", timestamp, filename);
                    std::fs::rename(&path, trash_dir.join(&trash_name))?;
                    Ok(())
                })();

                match trash_result {
                    Ok(()) => {
                        app.footer_log.push(format!(
                            "[{}] Trashed: {} (moved to .trash/)",
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
            app.trigger_auto_sync();
        }
        _ => {
            // Any other key cancels
            app.delete_state = None;
            app.mode = AppMode::Normal;
        }
    }
    Ok(())
}

fn empty_trash(app: &mut App) {
    let trash_dir = app.config.workflows_dir.join(".trash");

    let count = if trash_dir.exists() {
        std::fs::read_dir(&trash_dir)
            .map(|entries| entries.filter_map(|e| e.ok()).count())
            .unwrap_or(0)
    } else {
        0
    };

    if count == 0 {
        app.footer_log.push(format!(
            "[{}] Trash is empty",
            chrono::Local::now().format("%H:%M:%S"),
        ));
        return;
    }

    match std::fs::remove_dir_all(&trash_dir).and_then(|_| std::fs::create_dir_all(&trash_dir)) {
        Ok(()) => {
            app.footer_log.push(format!(
                "[{}] Emptied trash ({} file(s) removed)",
                chrono::Local::now().format("%H:%M:%S"),
                count,
            ));
        }
        Err(e) => {
            app.footer_log.push(format!(
                "[{}] Failed to empty trash: {}",
                chrono::Local::now().format("%H:%M:%S"),
                e,
            ));
        }
    }
}

fn start_rename(app: &mut App) {
    match app.focus {
        Focus::Sidebar => {
            let cat = match app.categories.get(app.selected_category) {
                Some(c) => c,
                None => return,
            };
            app.rename_state = Some(RenameState {
                target: RenameTarget::Category,
                old_name: cat.name.clone(),
                new_name: cat.name.clone(),
                task_path: cat.path.clone(),
                category: cat.name.clone(),
                extension: String::new(),
            });
            app.mode = AppMode::Rename;
        }
        _ => {
            let task = match app.selected_task_ref() {
                Some(t) => t.clone(),
                None => return,
            };
            let extension = task.path.extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            app.rename_state = Some(RenameState {
                target: RenameTarget::Task,
                old_name: task.name.clone(),
                new_name: task.name.clone(),
                task_path: task.path.clone(),
                category: task.category.clone(),
                extension,
            });
            app.mode = AppMode::Rename;
        }
    }
}

fn handle_rename_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return Ok(());
    }

    match key.code {
        KeyCode::Esc => {
            app.rename_state = None;
            app.mode = AppMode::Normal;
        }
        KeyCode::Backspace => {
            if let Some(ref mut state) = app.rename_state {
                state.new_name.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(ref mut state) = app.rename_state {
                state.new_name.push(c);
            }
        }
        KeyCode::Enter => {
            if let Some(state) = app.rename_state.take() {
                let new_name = state.new_name.trim().to_string();

                // Validate
                if new_name.is_empty()
                    || new_name.contains('/')
                    || new_name.contains('\\')
                    || new_name.starts_with('.')
                {
                    app.footer_log.push(format!(
                        "[{}] Rename failed: invalid name",
                        chrono::Local::now().format("%H:%M:%S"),
                    ));
                    app.mode = AppMode::Normal;
                    return Ok(());
                }

                if new_name == state.old_name {
                    app.mode = AppMode::Normal;
                    return Ok(());
                }

                match state.target {
                    RenameTarget::Task => {
                        let new_filename = format!("{}{}", new_name, state.extension);
                        let new_path = state.task_path.parent().unwrap().join(&new_filename);

                        if new_path.exists() {
                            app.footer_log.push(format!(
                                "[{}] Rename failed: '{}' already exists",
                                chrono::Local::now().format("%H:%M:%S"),
                                new_filename,
                            ));
                            app.mode = AppMode::Normal;
                            return Ok(());
                        }

                        match std::fs::rename(&state.task_path, &new_path) {
                            Ok(()) => {
                                let old_ref = format!("{}/{}", state.category, state.old_name);
                                let new_ref = format!("{}/{}", state.category, new_name);

                                // Update SQLite history
                                let db_path = app.config.workflows_dir.join("history.db");
                                if let Ok(conn) = db::open_db(&db_path) {
                                    let _ = db::rename_task_ref(&conn, &old_ref, &new_ref);
                                }

                                // Update bookmarks
                                if let Some(pos) = app.config.bookmarks.iter().position(|s| *s == old_ref) {
                                    app.config.bookmarks[pos] = new_ref.clone();
                                    let _ = app.config.save_bookmarks();
                                }

                                // Update in-memory task
                                for cat in &mut app.categories {
                                    for task in &mut cat.tasks {
                                        if task.path == state.task_path {
                                            task.name = new_name.clone();
                                            task.path = new_path.clone();
                                        }
                                    }
                                }

                                // Rebuild caches
                                app.build_step_cmd_cache();
                                app.apply_sort();

                                app.footer_log.push(format!(
                                    "[{}] Renamed: {} → {}",
                                    chrono::Local::now().format("%H:%M:%S"),
                                    old_ref,
                                    new_ref,
                                ));
                            }
                            Err(e) => {
                                app.footer_log.push(format!(
                                    "[{}] Rename failed: {}",
                                    chrono::Local::now().format("%H:%M:%S"),
                                    e,
                                ));
                            }
                        }
                    }
                    RenameTarget::Category => {
                        let new_path = state.task_path.parent().unwrap().join(&new_name);

                        if new_path.exists() {
                            app.footer_log.push(format!(
                                "[{}] Rename failed: category '{}' already exists",
                                chrono::Local::now().format("%H:%M:%S"),
                                new_name,
                            ));
                            app.mode = AppMode::Normal;
                            return Ok(());
                        }

                        match std::fs::rename(&state.task_path, &new_path) {
                            Ok(()) => {
                                let old_cat = &state.old_name;

                                // Update SQLite history and bookmarks for all tasks in this category
                                let db_path = app.config.workflows_dir.join("history.db");
                                let conn = db::open_db(&db_path).ok();

                                if let Some(cat) = app.categories.iter_mut().find(|c| c.name == *old_cat) {
                                    for task in &mut cat.tasks {
                                        let old_ref = format!("{}/{}", old_cat, task.name);
                                        let new_ref = format!("{}/{}", new_name, task.name);

                                        // Update SQLite
                                        if let Some(ref c) = conn {
                                            let _ = db::rename_task_ref(c, &old_ref, &new_ref);
                                        }

                                        // Update bookmarks
                                        if let Some(pos) = app.config.bookmarks.iter().position(|s| *s == old_ref) {
                                            app.config.bookmarks[pos] = new_ref;
                                        }

                                        // Update task path and category field
                                        task.path = new_path.join(task.path.file_name().unwrap());
                                        task.category = new_name.clone();
                                    }
                                    cat.name = new_name.clone();
                                    cat.path = new_path;
                                }

                                let _ = app.config.save_bookmarks();

                                // Rebuild caches
                                app.build_step_cmd_cache();
                                app.apply_sort();

                                app.footer_log.push(format!(
                                    "[{}] Renamed category: {} → {}",
                                    chrono::Local::now().format("%H:%M:%S"),
                                    old_cat,
                                    new_name,
                                ));
                            }
                            Err(e) => {
                                app.footer_log.push(format!(
                                    "[{}] Rename failed: {}",
                                    chrono::Local::now().format("%H:%M:%S"),
                                    e,
                                ));
                            }
                        }
                    }
                }
            }
            app.mode = AppMode::Normal;
            app.trigger_auto_sync();
        }
        _ => {}
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
        ai_source_yaml: String::new(),
        ai_source_path: None,
        ai_updated_yaml: None,
        template_entries: Vec::new(),
        template_filter: String::new(),
        template_filtered: Vec::new(),
        template_cursor: 0,
        template_scroll_offset: 0,
        template_var_values: Vec::new(),
        template_var_cursor: 0,
        category: String::new(),
        task_name: String::new(),
        category_cursor: None,
        remove_failed: false,
        remove_skipped: false,
        parallelize: false,
        active_toggle: 0,
        preview_scroll: 0,
        ai_refine_prompt: String::new(),
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
        ai_source_yaml: String::new(),
        ai_source_path: None,
        ai_updated_yaml: None,
        template_entries: Vec::new(),
        template_filter: String::new(),
        template_filtered: Vec::new(),
        template_cursor: 0,
        template_scroll_offset: 0,
        template_var_values: Vec::new(),
        template_var_cursor: 0,
        category: task.category.clone(),
        task_name: format!("{}-copy", task.name),
        category_cursor: None,
        remove_failed: has_run,
        remove_skipped: has_run,
        parallelize: false,
        active_toggle: 0,
        preview_scroll: 0,
        ai_refine_prompt: String::new(),
        save_message: None,
    });
    app.mode = AppMode::Wizard;
    app.focus = Focus::Details;
    app.detail_scroll = 0;

    Ok(())
}

fn start_ai_wizard(app: &mut App) {
    let tool = match app.ai_tool() {
        Some(t) => t,
        None => {
            app.footer_log.push(format!(
                "[{}] No AI tool found (install `claude`, `codex`, or `gemini`)",
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
        ai_source_yaml: String::new(),
        ai_source_path: None,
        ai_updated_yaml: None,
        template_entries: Vec::new(),
        template_filter: String::new(),
        template_filtered: Vec::new(),
        template_cursor: 0,
        template_scroll_offset: 0,
        template_var_values: Vec::new(),
        template_var_cursor: 0,
        category: String::new(),
        task_name: String::new(),
        category_cursor: None,
        remove_failed: false,
        remove_skipped: false,
        parallelize: false,
        active_toggle: 0,
        preview_scroll: 0,
        ai_refine_prompt: String::new(),
        save_message: None,
    });
    app.mode = AppMode::Wizard;
}

fn start_ai_update_wizard(app: &mut App) {
    let tool = match app.ai_tool() {
        Some(t) => t,
        None => {
            app.footer_log.push(format!(
                "[{}] No AI tool found (install `claude`, `codex`, or `gemini`)",
                chrono::Local::now().format("%H:%M:%S"),
            ));
            return;
        }
    };

    let task = match app.selected_task_ref() {
        Some(t) => t.clone(),
        None => return,
    };

    let yaml = match std::fs::read_to_string(&task.path) {
        Ok(content) => content,
        Err(e) => {
            app.footer_log.push(format!(
                "[{}] Failed to read task: {}",
                chrono::Local::now().format("%H:%M:%S"),
                e,
            ));
            return;
        }
    };

    app.wizard = Some(WizardState {
        mode: WizardMode::AiUpdate,
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
        ai_source_yaml: yaml,
        ai_source_path: Some(task.path.clone()),
        ai_updated_yaml: None,
        template_entries: Vec::new(),
        template_filter: String::new(),
        template_filtered: Vec::new(),
        template_cursor: 0,
        template_scroll_offset: 0,
        template_var_values: Vec::new(),
        template_var_cursor: 0,
        category: task.category.clone(),
        task_name: task.name.clone(),
        category_cursor: None,
        remove_failed: false,
        remove_skipped: false,
        parallelize: false,
        active_toggle: 0,
        preview_scroll: 0,
        ai_refine_prompt: String::new(),
        save_message: None,
    });
    app.mode = AppMode::Wizard;
}

fn start_ai_fix_from_run(app: &mut App) {
    let tool = match app.ai_tool() {
        Some(t) => t,
        None => {
            app.footer_log.push(format!(
                "[{}] No AI tool found (install `claude`, `codex`, or `gemini`)",
                chrono::Local::now().format("%H:%M:%S"),
            ));
            return;
        }
    };

    let task_path = match app.run_output_task_path.as_ref() {
        Some(p) => p.clone(),
        None => return,
    };

    let yaml = match std::fs::read_to_string(&task_path) {
        Ok(content) => content,
        Err(e) => {
            app.footer_log.push(format!(
                "[{}] Failed to read task: {}",
                chrono::Local::now().format("%H:%M:%S"),
                e,
            ));
            return;
        }
    };

    // Build error context from failed steps
    let mut error_context = String::new();
    if let Some(ref run_log) = app.run_output {
        for step in &run_log.steps {
            if step.status == crate::core::models::StepStatus::Failed {
                error_context.push_str(&format!("Step '{}' FAILED:\n", step.id));
                if !step.output.is_empty() {
                    error_context.push_str(&step.output);
                    error_context.push('\n');
                }
            }
        }
    }

    let prompt = format!(
        "This workflow failed. Fix the commands.\n\nFAILED STEPS:\n{}",
        error_context
    );

    // Resolve category/name from path for the wizard state
    let (category, task_name) = app.run_output.as_ref()
        .and_then(|r| {
            let parts: Vec<&str> = r.task_ref.splitn(2, '/').collect();
            if parts.len() == 2 { Some((parts[0].to_string(), parts[1].to_string())) } else { None }
        })
        .unwrap_or_default();

    let (tx, rx) = mpsc::channel();

    let source_yaml = yaml.clone();
    let prompt_clone = prompt.clone();
    let mcp_aliases: Vec<String> = app.config.mcp.servers.keys().cloned().collect();
    thread::spawn(move || {
        let result = ai::invoke_ai_update(tool, &source_yaml, &prompt_clone, &mcp_aliases);
        let _ = tx.send(result);
    });

    app.run_output = None;
    app.run_output_task_path = None;

    app.wizard = Some(WizardState {
        mode: WizardMode::AiUpdate,
        stage: WizardStage::AiThinking,
        history_entries: Vec::new(),
        history_filter: String::new(),
        history_filtered: Vec::new(),
        history_cursor: 0,
        history_selected: Vec::new(),
        history_scroll_offset: 0,
        source_task_ref: None,
        source_workflow: None,
        source_run: None,
        ai_prompt: prompt,
        ai_tool: Some(tool),
        ai_result_rx: Some(rx),
        ai_commands: Vec::new(),
        ai_error: None,
        ai_tick: 0,
        ai_source_yaml: yaml,
        ai_source_path: Some(task_path),
        ai_updated_yaml: None,
        template_entries: Vec::new(),
        template_filter: String::new(),
        template_filtered: Vec::new(),
        template_cursor: 0,
        template_scroll_offset: 0,
        template_var_values: Vec::new(),
        template_var_cursor: 0,
        category,
        task_name,
        category_cursor: None,
        remove_failed: false,
        remove_skipped: false,
        parallelize: false,
        active_toggle: 0,
        preview_scroll: 0,
        ai_refine_prompt: String::new(),
        save_message: None,
    });
    app.mode = AppMode::Wizard;
}

fn start_ai_fix_from_var_prompt(app: &mut App) -> Result<()> {
    let tool = match app.ai_tool() {
        Some(t) => t,
        None => {
            app.footer_log.push(format!(
                "[{}] No AI tool found (install `claude`, `codex`, or `gemini`)",
                chrono::Local::now().format("%H:%M:%S"),
            ));
            return Ok(());
        }
    };

    let task = match app.var_prompt_task.as_ref() {
        Some(t) => t.clone(),
        None => return Ok(()),
    };

    let yaml = match std::fs::read_to_string(&task.path) {
        Ok(content) => content,
        Err(e) => {
            app.footer_log.push(format!(
                "[{}] Failed to read task: {}",
                chrono::Local::now().format("%H:%M:%S"),
                e,
            ));
            return Ok(());
        }
    };

    let var_name = app.var_prompt_vars.get(app.var_prompt_index)
        .map(|v| v.name.clone())
        .unwrap_or_default();
    let error_msg = app.var_prompt_error.clone().unwrap_or_default();

    let prompt = format!(
        "The choices_cmd for variable '{}' failed: {}. Fix it to be robust and portable.",
        var_name, error_msg
    );

    // Clean up var_prompt state
    app.var_prompt_vars.clear();
    app.var_prompt_choices.clear();
    app.var_prompt_resolved.clear();
    app.var_prompt_task = None;
    app.var_prompt_workflow = None;
    app.var_prompt_error = None;

    let (tx, rx) = mpsc::channel();

    let source_yaml = yaml.clone();
    let prompt_clone = prompt.clone();
    let mcp_aliases: Vec<String> = app.config.mcp.servers.keys().cloned().collect();
    thread::spawn(move || {
        let result = ai::invoke_ai_update(tool, &source_yaml, &prompt_clone, &mcp_aliases);
        let _ = tx.send(result);
    });

    app.wizard = Some(WizardState {
        mode: WizardMode::AiUpdate,
        stage: WizardStage::AiThinking,
        history_entries: Vec::new(),
        history_filter: String::new(),
        history_filtered: Vec::new(),
        history_cursor: 0,
        history_selected: Vec::new(),
        history_scroll_offset: 0,
        source_task_ref: None,
        source_workflow: None,
        source_run: None,
        ai_prompt: prompt,
        ai_tool: Some(tool),
        ai_result_rx: Some(rx),
        ai_commands: Vec::new(),
        ai_error: None,
        ai_tick: 0,
        ai_source_yaml: yaml,
        ai_source_path: Some(task.path.clone()),
        ai_updated_yaml: None,
        template_entries: Vec::new(),
        template_filter: String::new(),
        template_filtered: Vec::new(),
        template_cursor: 0,
        template_scroll_offset: 0,
        template_var_values: Vec::new(),
        template_var_cursor: 0,
        category: task.category.clone(),
        task_name: task.name.clone(),
        category_cursor: None,
        remove_failed: false,
        remove_skipped: false,
        parallelize: false,
        active_toggle: 0,
        preview_scroll: 0,
        ai_refine_prompt: String::new(),
        save_message: None,
    });
    app.mode = AppMode::Wizard;
    Ok(())
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

                let mcp_aliases: Vec<String> = app.config.mcp.servers.keys().cloned().collect();
                if wiz.mode == WizardMode::AiUpdate {
                    let source_yaml = wiz.ai_source_yaml.clone();
                    thread::spawn(move || {
                        let result = ai::invoke_ai_update(tool, &source_yaml, &prompt, &mcp_aliases);
                        let _ = tx.send(result);
                    });
                } else {
                    thread::spawn(move || {
                        let result = ai::invoke_ai(tool, &prompt, &mcp_aliases);
                        let _ = tx.send(result);
                    });
                }
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
    if key.code == KeyCode::Esc {
        // If there's an error displayed, go back to refine prompt or initial prompt
        if wiz.ai_error.is_some() {
            wiz.ai_error = None;
            if !wiz.ai_refine_prompt.is_empty() {
                wiz.stage = WizardStage::AiRefinePrompt;
            } else {
                wiz.stage = WizardStage::AiPrompt;
            }
        }
        // Otherwise spinner is running — Esc cancels the whole wizard
        // (handled by parent match in handle_wizard_key)
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
            WizardStage::TemplateBrowse => handle_wizard_template_browse(app, key),
            WizardStage::TemplateVariables => handle_wizard_template_variables(app, key),
            WizardStage::Category => handle_wizard_category(app, key)?,
            WizardStage::TaskName => handle_wizard_taskname(app, key),
            WizardStage::Options => handle_wizard_options(app, key),
            WizardStage::Preview => handle_wizard_preview(app, key)?,
            WizardStage::AiRefinePrompt => handle_wizard_ai_refine_prompt(app, key),
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
                WizardMode::AiChat | WizardMode::AiUpdate => wiz.stage = WizardStage::AiPrompt,
                WizardMode::FromTemplate => {
                    if wiz.template_var_values.is_empty() {
                        wiz.stage = WizardStage::TemplateBrowse;
                    } else {
                        wiz.stage = WizardStage::TemplateVariables;
                    }
                }
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
                    WizardMode::FromHistory | WizardMode::AiChat | WizardMode::AiUpdate | WizardMode::FromTemplate => {
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

fn handle_wizard_ai_refine_prompt(app: &mut App, key: KeyEvent) {
    let wiz = app.wizard.as_mut().unwrap();

    match key.code {
        KeyCode::Enter => {
            if !wiz.ai_refine_prompt.trim().is_empty() {
                let tool = wiz.ai_tool.unwrap();
                let refine_prompt = wiz.ai_refine_prompt.clone();

                // Generate current YAML: use ai_updated_yaml if refined before, else from ai_commands
                let current_yaml = if let Some(ref yaml) = wiz.ai_updated_yaml {
                    yaml.clone()
                } else {
                    let wf = wizard::workflow_from_commands(&wiz.task_name, &wiz.ai_commands);
                    wizard::generate_yaml(&wf)
                };

                let (tx, rx) = mpsc::channel();
                wiz.ai_result_rx = Some(rx);
                wiz.ai_error = None;
                wiz.ai_tick = 0;
                wiz.stage = WizardStage::AiThinking;

                let mcp_aliases: Vec<String> = app.config.mcp.servers.keys().cloned().collect();
                thread::spawn(move || {
                    let result = ai::invoke_ai_update(tool, &current_yaml, &refine_prompt, &mcp_aliases);
                    let _ = tx.send(result);
                });
            }
        }
        KeyCode::BackTab => {
            wiz.stage = WizardStage::Preview;
        }
        KeyCode::Backspace => {
            wiz.ai_refine_prompt.pop();
        }
        KeyCode::Char(c) => {
            wiz.ai_refine_prompt.push(c);
        }
        _ => {}
    }
}

/// Build the preview YAML string from the current wizard state.
fn get_wizard_preview_yaml(wiz: &WizardState) -> String {
    match wiz.mode {
        WizardMode::AiUpdate => {
            wiz.ai_updated_yaml.clone().unwrap_or_default()
        }
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
            if let Some(ref yaml) = wiz.ai_updated_yaml {
                yaml.clone()
            } else {
                let wf = wizard::workflow_from_commands(&wiz.task_name, &wiz.ai_commands);
                wizard::generate_yaml(&wf)
            }
        }
        WizardMode::FromTemplate => {
            let idx = wiz.template_cursor;
            if let Some(entry) = wiz.template_entries.get(idx) {
                let mut values = HashMap::new();
                for (name, val, _default) in &wiz.template_var_values {
                    values.insert(name.clone(), val.clone());
                }
                catalog::instantiate_template(entry, &values)
            } else {
                String::new()
            }
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
    }
}

fn handle_wizard_preview(app: &mut App, key: KeyEvent) -> Result<()> {
    let wiz = app.wizard.as_ref().unwrap();
    match key.code {
        KeyCode::BackTab => {
            let wiz = app.wizard.as_mut().unwrap();
            match wiz.mode {
                WizardMode::CloneTask => wiz.stage = WizardStage::Options,
                WizardMode::AiUpdate => {
                    wiz.stage = WizardStage::AiPrompt;
                }
                WizardMode::FromHistory | WizardMode::AiChat | WizardMode::FromTemplate => {
                    wiz.stage = WizardStage::TaskName;
                }
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
        KeyCode::Char('d') => {
            let yaml = get_wizard_preview_yaml(wiz);
            if yaml.is_empty() {
                return Ok(());
            }

            let workflow = match parse_workflow_from_str(&yaml) {
                Ok(wf) => wf,
                Err(e) => {
                    app.footer_log.push(format!(
                        "[{}] Dry-run parse error: {}",
                        chrono::Local::now().format("%H:%M:%S"),
                        e,
                    ));
                    return Ok(());
                }
            };

            let task = Task {
                name: wiz.task_name.clone(),
                category: wiz.category.clone(),
                kind: TaskKind::YamlWorkflow,
                path: std::path::PathBuf::from("(preview)"),
                last_run: None,
                overdue: None,
                heat: TaskHeat::Cold,
            };

            app.pending_wizard_return = true;
            launch_workflow(app, &task, workflow, true, HashMap::new())?;
        }
        KeyCode::Enter => {
            let yaml = get_wizard_preview_yaml(wiz);
            let category = wiz.category.clone();
            let task_name = wiz.task_name.clone();

            let path = if wiz.mode == WizardMode::AiUpdate {
                let p = wiz.ai_source_path.clone().unwrap();
                std::fs::write(&p, &yaml)?;
                p
            } else {
                wizard::save_task(
                    &app.config.workflows_dir,
                    &category,
                    &task_name,
                    &yaml,
                )?
            };

            let msg = format!("Saved: {}", path.display());
            app.wizard.as_mut().unwrap().save_message = Some(msg.clone());
            app.footer_log.push(format!(
                "[{}] Wizard: {}",
                chrono::Local::now().format("%H:%M:%S"),
                msg,
            ));
            app.trigger_auto_sync();
        }
        KeyCode::Char('r') if wiz.mode == WizardMode::AiChat || wiz.mode == WizardMode::AiUpdate => {
            let wiz = app.wizard.as_mut().unwrap();
            wiz.ai_refine_prompt.clear();
            wiz.stage = WizardStage::AiRefinePrompt;
        }
        _ => {}
    }
    Ok(())
}

fn start_template_wizard(app: &mut App) {
    let cache_dir = app.config.workflows_dir.join(".template-cache");
    let entries = catalog::all_templates(&cache_dir);

    if entries.is_empty() {
        app.footer_log.push(format!(
            "[{}] No templates available",
            chrono::Local::now().format("%H:%M:%S"),
        ));
        return;
    }

    let filtered: Vec<usize> = (0..entries.len()).collect();

    app.wizard = Some(WizardState {
        mode: WizardMode::FromTemplate,
        stage: WizardStage::TemplateBrowse,
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
        ai_tool: None,
        ai_result_rx: None,
        ai_commands: Vec::new(),
        ai_error: None,
        ai_tick: 0,
        ai_source_yaml: String::new(),
        ai_source_path: None,
        ai_updated_yaml: None,
        template_entries: entries,
        template_filter: String::new(),
        template_filtered: filtered,
        template_cursor: 0,
        template_scroll_offset: 0,
        template_var_values: Vec::new(),
        template_var_cursor: 0,
        category: String::new(),
        task_name: String::new(),
        category_cursor: None,
        remove_failed: false,
        remove_skipped: false,
        parallelize: false,
        active_toggle: 0,
        preview_scroll: 0,
        ai_refine_prompt: String::new(),
        save_message: None,
    });
    app.mode = AppMode::Wizard;
}

fn handle_wizard_template_browse(app: &mut App, key: KeyEvent) {
    let wiz = app.wizard.as_mut().unwrap();

    match key.code {
        KeyCode::Up => {
            if wiz.template_cursor > 0 {
                wiz.template_cursor -= 1;
                if wiz.template_cursor < wiz.template_scroll_offset {
                    wiz.template_scroll_offset = wiz.template_cursor;
                }
            }
        }
        KeyCode::Down => {
            if !wiz.template_filtered.is_empty()
                && wiz.template_cursor + 1 < wiz.template_filtered.len()
            {
                wiz.template_cursor += 1;
                let visible = 16usize;
                if wiz.template_cursor >= wiz.template_scroll_offset + visible {
                    wiz.template_scroll_offset = wiz.template_cursor.saturating_sub(visible - 1);
                }
            }
        }
        KeyCode::Enter => {
            if let Some(&real_idx) = wiz.template_filtered.get(wiz.template_cursor) {
                wiz.template_cursor = real_idx;
                let entry = &wiz.template_entries[real_idx];

                // Pre-fill category and task name from template
                wiz.category = entry.category.clone();
                wiz.task_name = entry.slug.clone();

                // Set up variable values
                if entry.variables.is_empty() {
                    wiz.template_var_values = Vec::new();
                    wiz.stage = WizardStage::Category;
                } else {
                    wiz.template_var_values = entry
                        .variables
                        .iter()
                        .map(|v| {
                            (
                                v.name.clone(),
                                v.default.clone().unwrap_or_default(),
                                v.default.clone(),
                            )
                        })
                        .collect();
                    wiz.template_var_cursor = 0;
                    wiz.stage = WizardStage::TemplateVariables;
                }
            }
        }
        KeyCode::Backspace => {
            wiz.template_filter.pop();
            update_template_filter(wiz);
        }
        KeyCode::Char(c) => {
            wiz.template_filter.push(c);
            update_template_filter(wiz);
        }
        _ => {}
    }
}

fn update_template_filter(wiz: &mut WizardState) {
    let query = wiz.template_filter.to_lowercase();
    wiz.template_filtered = if query.is_empty() {
        (0..wiz.template_entries.len()).collect()
    } else {
        wiz.template_entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.name.to_lowercase().contains(&query)
                    || e.slug.to_lowercase().contains(&query)
                    || e.category.to_lowercase().contains(&query)
                    || e.description
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&query)
            })
            .map(|(i, _)| i)
            .collect()
    };
    wiz.template_cursor = 0;
    wiz.template_scroll_offset = 0;
}

fn handle_wizard_template_variables(app: &mut App, key: KeyEvent) {
    let wiz = app.wizard.as_mut().unwrap();

    match key.code {
        KeyCode::Up => {
            if wiz.template_var_cursor > 0 {
                wiz.template_var_cursor -= 1;
            }
        }
        KeyCode::Down => {
            if wiz.template_var_cursor + 1 < wiz.template_var_values.len() {
                wiz.template_var_cursor += 1;
            }
        }
        KeyCode::Enter | KeyCode::Tab => {
            wiz.stage = WizardStage::Category;
        }
        KeyCode::BackTab => {
            wiz.stage = WizardStage::TemplateBrowse;
        }
        KeyCode::Backspace => {
            let idx = wiz.template_var_cursor;
            if let Some(entry) = wiz.template_var_values.get_mut(idx) {
                entry.1.pop();
            }
        }
        KeyCode::Char(c) => {
            let idx = wiz.template_var_cursor;
            if let Some(entry) = wiz.template_var_values.get_mut(idx) {
                entry.1.push(c);
            }
        }
        _ => {}
    }
}

fn open_recent_runs(app: &mut App) -> Result<()> {
    let conn = db::open_db(&app.config.db_path())?;
    let runs = db::get_recent_runs(&conn, 10)?;
    app.recent_runs = runs;
    app.recent_runs_cursor = 0;
    app.mode = AppMode::RecentRuns;
    Ok(())
}

fn open_saved_tasks(app: &mut App) {
    app.saved_tasks_cursor = 0;
    app.mode = AppMode::SavedTasks;
}

fn toggle_bookmark(app: &mut App) {
    let task = match app.selected_task_ref() {
        Some(t) => t.clone(),
        None => return,
    };

    let task_ref = format!("{}/{}", task.category, task.name);
    let added = app.config.toggle_bookmark(&task_ref);

    let msg = if added {
        format!("Bookmarked: {}", task_ref)
    } else {
        format!("Unbookmarked: {}", task_ref)
    };
    app.footer_log.push(format!(
        "[{}] {}",
        chrono::Local::now().format("%H:%M:%S"),
        msg,
    ));

    if let Err(e) = app.config.save_bookmarks() {
        app.footer_log.push(format!(
            "[{}] Failed to save bookmarks: {}",
            chrono::Local::now().format("%H:%M:%S"),
            e,
        ));
    }
}

fn handle_recent_runs_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.recent_runs.clear();
            app.mode = AppMode::Normal;
        }
        KeyCode::Up => {
            if app.recent_runs_cursor > 0 {
                app.recent_runs_cursor -= 1;
            }
        }
        KeyCode::Down => {
            if !app.recent_runs.is_empty() && app.recent_runs_cursor + 1 < app.recent_runs.len() {
                app.recent_runs_cursor += 1;
            }
        }
        KeyCode::Enter => {
            if let Some(run) = app.recent_runs.get(app.recent_runs_cursor) {
                let task_ref = run.task_ref.clone();
                app.recent_runs.clear();
                app.mode = AppMode::Normal;
                app.navigate_to_task(&task_ref);
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_overdue_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.overdue_tasks.clear();
            app.mode = AppMode::Normal;
        }
        KeyCode::Up => {
            if app.overdue_cursor > 0 {
                app.overdue_cursor -= 1;
            }
        }
        KeyCode::Down => {
            if !app.overdue_tasks.is_empty() && app.overdue_cursor + 1 < app.overdue_tasks.len() {
                app.overdue_cursor += 1;
            }
        }
        KeyCode::Enter => {
            if let Some(task) = app.overdue_tasks.get(app.overdue_cursor) {
                let task_ref = task.task_ref.clone();
                app.overdue_tasks.clear();
                app.mode = AppMode::Normal;
                app.navigate_to_task(&task_ref);
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_saved_tasks_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let bookmark_count = app.config.bookmarks.len();

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.mode = AppMode::Normal;
        }
        KeyCode::Up => {
            if app.saved_tasks_cursor > 0 {
                app.saved_tasks_cursor -= 1;
            }
        }
        KeyCode::Down => {
            if bookmark_count > 0 && app.saved_tasks_cursor + 1 < bookmark_count {
                app.saved_tasks_cursor += 1;
            }
        }
        KeyCode::Enter => {
            if let Some(task_ref) = app.config.bookmarks.get(app.saved_tasks_cursor).cloned() {
                app.mode = AppMode::Normal;
                app.navigate_to_task(&task_ref);
            }
        }
        _ => {}
    }
    Ok(())
}

fn open_secrets(app: &mut App) {
    use crate::core::secrets;

    let config_dir = app.config.workflows_dir.clone();
    let ssh_key = app.config.secrets_ssh_key.as_ref().map(std::path::PathBuf::from);

    match ssh_key {
        Some(key_path) => {
            let store_path = std::path::Path::new(&config_dir).join("secrets.age");
            if !store_path.exists() {
                app.secrets_state = Some(SecretsState {
                    mode: SecretsMode::NotInitialized,
                    names: Vec::new(),
                    cursor: 0,
                    input: String::new(),
                    pending_name: String::new(),
                    revealed_value: None,
                    error: None,
                    store_initialized: false,
                });
            } else {
                match secrets::SecretsStore::load(std::path::Path::new(&config_dir), &key_path) {
                    Ok(store) => {
                        let names = store.list();
                        app.secrets_state = Some(SecretsState {
                            mode: SecretsMode::List,
                            names,
                            cursor: 0,
                            input: String::new(),
                            pending_name: String::new(),
                            revealed_value: None,
                            error: None,
                            store_initialized: true,
                        });
                    }
                    Err(e) => {
                        app.secrets_state = Some(SecretsState {
                            mode: SecretsMode::List,
                            names: Vec::new(),
                            cursor: 0,
                            input: String::new(),
                            pending_name: String::new(),
                            revealed_value: None,
                            error: Some(format!("Failed to load secrets: {e}")),
                            store_initialized: true,
                        });
                    }
                }
            }
        }
        None => {
            app.secrets_state = Some(SecretsState {
                mode: SecretsMode::NotInitialized,
                names: Vec::new(),
                cursor: 0,
                input: String::new(),
                pending_name: String::new(),
                revealed_value: None,
                error: None,
                store_initialized: false,
            });
        }
    }

    app.mode = AppMode::Secrets;
}

fn handle_secrets_key(app: &mut App, key: KeyEvent) -> Result<()> {
    use crate::core::secrets;

    let state = match app.secrets_state.as_mut() {
        Some(s) => s,
        None => {
            app.mode = AppMode::Normal;
            return Ok(());
        }
    };

    match state.mode {
        SecretsMode::List => match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.secrets_state = None;
                app.mode = AppMode::Normal;
            }
            KeyCode::Up => {
                if state.cursor > 0 {
                    state.cursor -= 1;
                }
            }
            KeyCode::Down => {
                if !state.names.is_empty() && state.cursor < state.names.len() - 1 {
                    state.cursor += 1;
                }
            }
            KeyCode::Char('a') => {
                state.mode = SecretsMode::AddName;
                state.input.clear();
                state.error = None;
            }
            KeyCode::Enter | KeyCode::Char('v') => {
                if let Some(name) = state.names.get(state.cursor).cloned() {
                    let config_dir = app.config.workflows_dir.clone();
                    if let Some(ref key_path) = app.config.secrets_ssh_key {
                        let key = std::path::PathBuf::from(key_path);
                        match secrets::SecretsStore::load(std::path::Path::new(&config_dir), &key) {
                            Ok(store) => {
                                state.revealed_value = store.get(&name).map(|s| s.to_string());
                                state.pending_name = name;
                                state.mode = SecretsMode::ViewValue;
                                state.error = None;
                            }
                            Err(e) => {
                                state.error = Some(format!("Decrypt failed: {e}"));
                            }
                        }
                    }
                }
            }
            KeyCode::Char('e') => {
                if let Some(name) = state.names.get(state.cursor).cloned() {
                    state.pending_name = name;
                    state.input.clear();
                    state.mode = SecretsMode::EditValue;
                    state.error = None;
                }
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                if !state.names.is_empty() {
                    state.mode = SecretsMode::ConfirmDelete;
                    state.error = None;
                }
            }
            _ => {}
        },

        SecretsMode::ViewValue => {
            // Any key dismisses
            state.revealed_value = None;
            state.mode = SecretsMode::List;
        }

        SecretsMode::AddName => match key.code {
            KeyCode::Esc => {
                state.mode = SecretsMode::List;
                state.input.clear();
            }
            KeyCode::Enter => {
                let name = state.input.trim().to_string();
                if name.is_empty() {
                    state.error = Some("Name cannot be empty".into());
                } else if state.names.contains(&name) {
                    state.error = Some(format!("Secret '{name}' already exists"));
                } else {
                    state.pending_name = name;
                    state.input.clear();
                    state.mode = SecretsMode::AddValue;
                    state.error = None;
                }
            }
            KeyCode::Backspace => { state.input.pop(); }
            KeyCode::Char(c) => { state.input.push(c); }
            _ => {}
        },

        SecretsMode::AddValue | SecretsMode::EditValue => match key.code {
            KeyCode::Esc => {
                state.mode = SecretsMode::List;
                state.input.clear();
                state.pending_name.clear();
            }
            KeyCode::Enter => {
                let value = state.input.clone();
                if value.is_empty() {
                    state.error = Some("Value cannot be empty".into());
                } else {
                    let config_dir = app.config.workflows_dir.clone();
                    if let Some(ref key_str) = app.config.secrets_ssh_key {
                        let key_path = std::path::PathBuf::from(key_str);
                        let pub_path = secrets::pubkey_path_from(&key_path);
                        match secrets::SecretsStore::load(std::path::Path::new(&config_dir), &key_path) {
                            Ok(mut store) => {
                                store.set(state.pending_name.clone(), value);
                                if let Err(e) = store.save(std::path::Path::new(&config_dir), &pub_path) {
                                    state.error = Some(format!("Save failed: {e}"));
                                } else {
                                    state.names = store.list();
                                    if let Some(pos) = state.names.iter().position(|n| n == &state.pending_name) {
                                        state.cursor = pos;
                                    }
                                    state.input.clear();
                                    state.pending_name.clear();
                                    state.mode = SecretsMode::List;
                                    state.error = None;
                                }
                            }
                            Err(e) => {
                                state.error = Some(format!("Load failed: {e}"));
                            }
                        }
                    }
                }
            }
            KeyCode::Backspace => { state.input.pop(); }
            KeyCode::Char(c) => { state.input.push(c); }
            _ => {}
        },

        SecretsMode::ConfirmDelete => match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                if let Some(name) = state.names.get(state.cursor).cloned() {
                    let config_dir = app.config.workflows_dir.clone();
                    if let Some(ref key_str) = app.config.secrets_ssh_key {
                        let key_path = std::path::PathBuf::from(key_str);
                        let pub_path = secrets::pubkey_path_from(&key_path);
                        match secrets::SecretsStore::load(std::path::Path::new(&config_dir), &key_path) {
                            Ok(mut store) => {
                                store.remove(&name);
                                if let Err(e) = store.save(std::path::Path::new(&config_dir), &pub_path) {
                                    state.error = Some(format!("Save failed: {e}"));
                                } else {
                                    state.names = store.list();
                                    if state.cursor >= state.names.len() && state.cursor > 0 {
                                        state.cursor -= 1;
                                    }
                                    state.error = None;
                                }
                            }
                            Err(e) => {
                                state.error = Some(format!("Load failed: {e}"));
                            }
                        }
                    }
                }
                state.mode = SecretsMode::List;
            }
            _ => {
                state.mode = SecretsMode::List;
            }
        },

        SecretsMode::NotInitialized => match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.secrets_state = None;
                app.mode = AppMode::Normal;
            }
            KeyCode::Enter | KeyCode::Char('y') => {
                match secrets::detect_ssh_key() {
                    Ok((private, public)) => {
                        let config_dir = app.config.workflows_dir.clone();
                        match secrets::init_store(std::path::Path::new(&config_dir), &public) {
                            Ok(()) => {
                                app.config.secrets_ssh_key = Some(private.display().to_string());
                                let _ = app.config.save_bookmarks();
                                // Reload as list
                                if let Some(ref mut st) = app.secrets_state {
                                    st.store_initialized = true;
                                    st.names = Vec::new();
                                    st.mode = SecretsMode::List;
                                    st.error = None;
                                }
                            }
                            Err(e) => {
                                if let Some(ref mut st) = app.secrets_state {
                                    st.error = Some(format!("Init failed: {e}"));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        if let Some(ref mut st) = app.secrets_state {
                            st.error = Some(format!("No SSH key found: {e}"));
                        }
                    }
                }
            }
            _ => {}
        },
    }

    Ok(())
}

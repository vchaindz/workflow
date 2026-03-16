use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::compare;
use crate::core::db;
use crate::core::executor::{execute_workflow, load_secret_env, send_notifications, ExecuteOpts, StreamingRequest};
use crate::core::models::{ExecutionEvent, RuntimeVariable, Task, TaskKind, Workflow};
use crate::core::parser::{parse_shell_task, parse_workflow};
use crate::error::Result;

use super::super::app::{App, AppMode, Focus};
use super::editing::{start_delete, start_rename, empty_trash};
use super::execution::{open_recent_runs, open_saved_tasks, toggle_bookmark, open_memory_view, open_secrets};
use super::streaming::start_loading_choices;
use super::wizard::{start_new_workflow_menu, start_clone_wizard, start_ai_update_wizard, start_ai_fix_from_run};

pub(super) fn handle_normal_key(app: &mut App, key: KeyEvent) -> Result<()> {
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
        KeyCode::Char('n') => start_new_workflow_menu(app),
        KeyCode::Char('W') => start_clone_wizard(app)?,
        KeyCode::Char('c') => compare_selected(app)?,
        KeyCode::Char('a') => {
            if app.run_output.as_ref().map(|r| r.exit_code != 0).unwrap_or(false)
                && app.run_output_task_path.is_some()
            {
                start_ai_fix_from_run(app);
            }
        }
        KeyCode::Char('A') => start_ai_update_wizard(app),
        KeyCode::Char('R') => open_recent_runs(app)?,
        KeyCode::Char('s') => open_saved_tasks(app),
        KeyCode::Char('S') => toggle_bookmark(app),
        KeyCode::Char('o') => app.toggle_sort(),
        KeyCode::Char('F') => app.cycle_status_filter(),
        KeyCode::Char('g') => {
            app.refresh_sync_status();
            app.sync_menu_cursor = 0;
            app.sync_message = None;
            app.sync_setup_stage = super::super::app::SyncSetupStage::Menu;
            app.sync_setup_input.clear();
            app.mode = AppMode::GitSync;
        }
        KeyCode::Char('M') => open_memory_view(app)?,
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

pub(super) fn launch_workflow(
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
                        // Post-run memory analysis
                        if let Ok(analysis) = crate::core::memory::analyze_post_run(&conn, &run_log) {
                            if !analysis.anomalies.is_empty() {
                                let _ = tx.send(ExecutionEvent::MemoryAnomaly {
                                    count: analysis.anomalies.len(),
                                    summary: analysis.summary.clone(),
                                });
                            }
                        }
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
    match super::super::app::EditState::from_file(&task.path, &display_name) {
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

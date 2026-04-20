use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::ai;
use crate::core::catalog;
use crate::core::db;
use crate::core::history;
use crate::core::models::{Task, TaskHeat, TaskKind};
use crate::core::parser::parse_workflow_from_str;
use crate::core::wizard;
use crate::error::Result;

use super::super::app::{App, AppMode, Focus, WizardMode, WizardStage, WizardState};
use super::normal::launch_workflow;

pub(super) fn start_new_workflow_menu(app: &mut App) {
    app.wizard = Some(WizardState {
        mode: WizardMode::FromHistory, // placeholder, overridden on selection
        stage: WizardStage::PickMode,
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
        failed_run: None,
        preview_diff_mode: false,
        pick_mode_cursor: 0,
    });
    app.mode = AppMode::Wizard;
}

/// Build the list of options for the PickMode menu.
/// Index 0 is always "From shell history", index 1 is "AI generate" (only when
/// an AI tool is detected), and the last entry is "From template".
fn pick_mode_options(has_ai: bool) -> Vec<(&'static str, &'static str)> {
    let mut opts = vec![("From shell history", "w")];
    if has_ai {
        opts.push(("AI generate", "a"));
    }
    opts.push(("From template", "t"));
    opts
}

fn handle_wizard_pick_mode(app: &mut App, key: KeyEvent) {
    let has_ai = app.ai_tool().is_some();
    let options = pick_mode_options(has_ai);
    let max = options.len().saturating_sub(1);
    let cursor = app.wizard.as_ref().unwrap().pick_mode_cursor;

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            let wiz = app.wizard.as_mut().unwrap();
            wiz.pick_mode_cursor = cursor.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let wiz = app.wizard.as_mut().unwrap();
            wiz.pick_mode_cursor = (cursor + 1).min(max);
        }
        KeyCode::Enter => {
            let label = options[cursor].0;
            match label {
                "From shell history" => {
                    let entries = history::load_shell_history(5000);
                    let filtered: Vec<usize> = (0..entries.len()).collect();
                    let wiz = app.wizard.as_mut().unwrap();
                    wiz.mode = WizardMode::FromHistory;
                    wiz.stage = WizardStage::ShellHistory;
                    wiz.history_entries = entries;
                    wiz.history_filtered = filtered;
                }
                "AI generate" => {
                    // has_ai is guaranteed true here
                    let tool = app.ai_tool().unwrap();
                    let wiz = app.wizard.as_mut().unwrap();
                    wiz.mode = WizardMode::AiChat;
                    wiz.stage = WizardStage::AiPrompt;
                    wiz.ai_tool = Some(tool);
                }
                "From template" => {
                    let cache_dir = app.config.workflows_dir.join(".template-cache");
                    let entries = catalog::all_templates(&cache_dir);
                    if entries.is_empty() {
                        app.footer_log.push(format!(
                            "[{}] No templates available",
                            chrono::Local::now().format("%H:%M:%S"),
                        ));
                        app.wizard = None;
                        app.mode = AppMode::Normal;
                        return;
                    }
                    let filtered: Vec<usize> = (0..entries.len()).collect();
                    let wiz = app.wizard.as_mut().unwrap();
                    wiz.mode = WizardMode::FromTemplate;
                    wiz.stage = WizardStage::TemplateBrowse;
                    wiz.template_entries = entries;
                    wiz.template_filtered = filtered;
                }
                _ => {}
            }
        }
        KeyCode::Esc => {
            app.wizard = None;
            app.mode = AppMode::Normal;
        }
        _ => {}
    }
}

pub(super) fn start_clone_wizard(app: &mut App) -> Result<()> {
    use crate::core::parser::{parse_shell_task, parse_workflow};

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
        failed_run: None,
        preview_diff_mode: false,
        pick_mode_cursor: 0,
    });
    app.mode = AppMode::Wizard;
    app.focus = Focus::Details;
    app.detail_scroll = 0;

    Ok(())
}

pub(super) fn start_ai_update_wizard(app: &mut App) {
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
        failed_run: None,
        preview_diff_mode: false,
        pick_mode_cursor: 0,
    });
    app.mode = AppMode::Wizard;
}

pub(super) fn start_ai_fix_from_run(app: &mut App) {
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

    let failed_run = app.run_output.clone();
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
        failed_run,
        preview_diff_mode: true,
        pick_mode_cursor: 0,
    });
    app.mode = AppMode::Wizard;
}

pub(super) fn start_ai_fix_from_var_prompt(app: &mut App) -> Result<()> {
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
        failed_run: None,
        preview_diff_mode: false,
        pick_mode_cursor: 0,
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
        // Otherwise spinner is running -- Esc cancels the whole wizard
        // (handled by parent match in handle_wizard_key)
    }
}

pub(super) fn handle_wizard_key(app: &mut App, key: KeyEvent) -> Result<()> {
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
            WizardStage::PickMode => handle_wizard_pick_mode(app, key),
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
        KeyCode::Char('D') => {
            // Toggle diff view in AiUpdate mode
            let wiz = app.wizard.as_mut().unwrap();
            if wiz.mode == WizardMode::AiUpdate && wiz.ai_updated_yaml.is_some() {
                wiz.preview_diff_mode = !wiz.preview_diff_mode;
            }
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

fn handle_wizard_template_browse(app: &mut App, key: KeyEvent) {
    // F5 refreshes the catalog from the default remote and reloads entries.
    if matches!(key.code, KeyCode::F(5)) {
        let cache_dir = app.config.workflows_dir.join(".template-cache");
        let repo = crate::cli::templates::DEFAULT_REPO;
        let msg = match catalog::fetch_templates(&cache_dir, repo) {
            Ok(n) => format!("fetched {n} template(s) from remote"),
            Err(e) => format!("fetch failed: {e}"),
        };
        let entries = catalog::all_templates(&cache_dir);
        let filtered: Vec<usize> = (0..entries.len()).collect();
        let wiz = app.wizard.as_mut().unwrap();
        wiz.template_entries = entries;
        wiz.template_filtered = filtered;
        wiz.template_cursor = 0;
        wiz.template_scroll_offset = 0;
        app.footer_log.push(format!(
            "[{}] ● templates: {msg}",
            chrono::Local::now().format("%H:%M:%S"),
        ));
        return;
    }

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

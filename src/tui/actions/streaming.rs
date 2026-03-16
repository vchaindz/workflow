use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::error::Result;

use super::super::app::{App, AppMode};
use super::normal::launch_workflow;
use super::wizard::start_ai_fix_from_var_prompt;

pub(super) fn handle_streaming_key(app: &mut App, key: KeyEvent) -> Result<()> {
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

pub(super) fn handle_variable_prompt_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            // Cancel -- return to normal
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
                // All variables resolved -- launch
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

pub(super) fn start_loading_choices(app: &mut App) {
    let idx = app.var_prompt_index;
    let var = &app.var_prompt_vars[idx];
    let cmd = match var.choices_cmd.as_ref() {
        Some(c) => c.clone(),
        None => return,
    };

    // Run choices_cmd with a 5-second timeout to avoid hanging the TUI.
    // NOTE: choices_cmd runs shell commands defined in workflow YAML templates.
    // Only use templates from trusted sources -- a malicious template could execute
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

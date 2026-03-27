use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::sync;
use crate::error::Result;

use super::super::app::{App, AppMode, SyncSetupStage};

pub(super) fn handle_git_sync_key(app: &mut App, key: KeyEvent) -> Result<()> {
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
                                            "[{}] \u{25cf} sync: {msg}",
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
                                if let Ok(cats) = crate::core::discovery::scan_all_workflows(&dir) {
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

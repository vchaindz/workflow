use crossterm::event::{KeyCode, KeyEvent};

use crate::core::db;
use crate::error::Result;

use super::super::app::{App, AppMode, SecretsMode, SecretsState};
use super::wizard::start_new_workflow_menu;

pub(super) fn open_recent_runs(app: &mut App) -> Result<()> {
    let conn = db::open_db(&app.config.db_path())?;
    let runs = db::get_recent_runs(&conn, 10)?;
    app.recent_runs = runs;
    app.recent_runs_cursor = 0;
    app.mode = AppMode::RecentRuns;
    Ok(())
}

pub(super) fn open_memory_view(app: &mut App) -> Result<()> {
    if let Some(task) = app.selected_task_ref() {
        let task_ref = format!("{}/{}", task.category, task.name);
        let conn = db::open_db(&app.config.db_path())?;
        let tm = crate::core::memory::get_task_memory(&conn, &task_ref)?;
        app.task_memory_cache.insert(task_ref, tm);
        app.detail_scroll = 0;
        app.mode = AppMode::MemoryView;
    }
    Ok(())
}

pub(super) fn open_saved_tasks(app: &mut App) {
    app.saved_tasks_cursor = 0;
    app.mode = AppMode::SavedTasks;
}

pub(super) fn toggle_bookmark(app: &mut App) {
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

pub(super) fn handle_recent_runs_key(app: &mut App, key: KeyEvent) -> Result<()> {
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

pub(super) fn handle_overdue_key(app: &mut App, key: KeyEvent) -> Result<()> {
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

pub(super) fn handle_getting_started_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.mode = AppMode::Normal;
        }
        KeyCode::Up => {
            if app.getting_started_cursor > 0 {
                app.getting_started_cursor -= 1;
            }
        }
        KeyCode::Down => {
            if app.getting_started_cursor < 1 {
                app.getting_started_cursor += 1;
            }
        }
        KeyCode::Char('n') => {
            app.mode = AppMode::Normal;
            start_new_workflow_menu(app);
        }
        KeyCode::Enter => {
            if app.getting_started_cursor == 0 {
                app.mode = AppMode::Normal;
                start_new_workflow_menu(app);
            } else {
                app.mode = AppMode::Normal;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn handle_memory_view_key(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.mode = AppMode::Normal;
        }
        KeyCode::Up => {
            if app.detail_scroll > 0 {
                app.detail_scroll -= 1;
            }
        }
        KeyCode::Down => {
            app.detail_scroll += 1;
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn handle_saved_tasks_key(app: &mut App, key: KeyEvent) -> Result<()> {
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

pub(super) fn open_secrets(app: &mut App) {
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

pub(super) fn handle_secrets_key(app: &mut App, key: KeyEvent) -> Result<()> {
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

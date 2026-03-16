use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::db;
use crate::error::Result;

use super::super::app::{App, AppMode, DeleteState, Focus, RenameState, RenameTarget};

pub(super) fn handle_edit_task_key(app: &mut App, key: KeyEvent) -> Result<()> {
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

pub(super) fn start_delete(app: &mut App) {
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

pub(super) fn handle_confirm_delete_key(app: &mut App, key: KeyEvent) -> Result<()> {
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

pub(super) fn empty_trash(app: &mut App) {
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

pub(super) fn start_rename(app: &mut App) {
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

pub(super) fn handle_rename_key(app: &mut App, key: KeyEvent) -> Result<()> {
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
                                    "[{}] Renamed: {} \u{2192} {}",
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
                                    "[{}] Renamed category: {} \u{2192} {}",
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

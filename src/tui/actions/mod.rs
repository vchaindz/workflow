mod editing;
mod execution;
mod git_sync;
mod normal;
mod search;
mod streaming;
mod wizard;

use crossterm::event::{KeyCode, KeyEvent};

use crate::error::Result;

use super::app::{App, AppMode};

use editing::{handle_confirm_delete_key, handle_edit_task_key, handle_rename_key};
use execution::{
    handle_getting_started_key, handle_memory_view_key, handle_overdue_key,
    handle_recent_runs_key, handle_saved_tasks_key, handle_secrets_key,
};
use git_sync::handle_git_sync_key;
use normal::handle_normal_key;
use search::handle_search_key;
use streaming::{handle_streaming_key, handle_variable_prompt_key};
use wizard::handle_wizard_key;

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
                KeyCode::PageUp => app.detail_scroll = app.detail_scroll.saturating_sub(20),
                KeyCode::PageDown => app.detail_scroll = app.detail_scroll.saturating_add(20),
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
        AppMode::GettingStarted => handle_getting_started_key(app, key),
        AppMode::MemoryView => handle_memory_view_key(app, key),
        _ => handle_normal_key(app, key),
    }
}

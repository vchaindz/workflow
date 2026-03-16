use crossterm::event::{KeyCode, KeyEvent};

use crate::error::Result;

use super::super::app::{App, AppMode, Focus};

pub(super) fn handle_search_key(app: &mut App, key: KeyEvent) -> Result<()> {
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

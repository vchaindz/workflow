pub mod actions;
pub mod app;
pub mod event;
pub mod ui;
pub mod widgets;

use std::io;
use std::time::Duration;

use crossterm::event::KeyEventKind;
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::core::config::Config;
use crate::core::discovery::scan_workflows;
use crate::error::Result;

use app::App;
use event::{poll_event, AppEvent};

const TICK_RATE: Duration = Duration::from_millis(250);

pub fn run_tui(config: Config) -> Result<()> {
    let categories = scan_workflows(&config.workflows_dir)?;
    let mut app = App::new(categories, config);

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let result = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        match poll_event(TICK_RATE)? {
            AppEvent::Key(key) => {
                // Only handle Press events (not Release/Repeat)
                if key.kind == KeyEventKind::Press {
                    // For edit action, we need to restore terminal
                    if key.code == crossterm::event::KeyCode::Char('e')
                        && app.mode == app::AppMode::Normal
                    {
                        if app.selected_task_ref().is_some() {
                            // Restore terminal for editor
                            disable_raw_mode()?;
                            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                            terminal.show_cursor()?;

                            actions::handle_key(app, key)?;

                            // Re-init terminal
                            enable_raw_mode()?;
                            execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                            terminal.clear()?;

                            // Refresh categories in case file was edited
                            app.categories = scan_workflows(&app.config.workflows_dir)?;
                            continue;
                        }
                    }

                    actions::handle_key(app, key)?;
                }
            }
            AppEvent::Tick => {}
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

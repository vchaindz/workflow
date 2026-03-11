pub mod actions;
pub mod app;
pub mod event;
pub mod ui;
pub mod widgets;

use std::io;
use std::time::{Duration, Instant};

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
const RESCAN_INTERVAL: Duration = Duration::from_secs(5);

pub fn run_tui(config: Config) -> Result<()> {
    let categories = scan_workflows(&config.workflows_dir)?;
    let mut app = App::new(categories, config);
    app.refresh_stats();
    app.load_heat_data();
    app.build_step_cmd_cache();
    app.check_overdue();

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
    let mut last_rescan = Instant::now();

    loop {
        // Check for streaming output requests from executor
        app.check_streaming_requests();

        app.drain_execution_events();
        app.drain_ai_events();
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
                            rescan(app);
                            last_rescan = Instant::now();
                            continue;
                        }
                    }

                    actions::handle_key(app, key)?;
                }
            }
            AppEvent::Tick => {
                if last_rescan.elapsed() >= RESCAN_INTERVAL {
                    rescan(app);
                    last_rescan = Instant::now();
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

/// Rescan workflows directory and update categories, preserving selection.
fn rescan(app: &mut App) {
    let Ok(new_categories) = scan_workflows(&app.config.workflows_dir) else {
        return;
    };
    if categories_equal(&app.categories, &new_categories) {
        app.refresh_stats();
        return;
    }

    // Preserve selected category by name
    let prev_cat_name = app
        .categories
        .get(app.selected_category)
        .map(|c| c.name.clone());
    let prev_task_name = app.selected_task_ref().map(|t| t.name.clone());

    app.categories = new_categories;
    app.load_heat_data();
    app.build_step_cmd_cache();
    if app.sort_by_heat {
        app.apply_sort();
    }

    // Restore category selection
    if let Some(ref name) = prev_cat_name {
        if let Some(idx) = app.categories.iter().position(|c| &c.name == name) {
            app.selected_category = idx;
        } else {
            app.selected_category = app.selected_category.min(app.category_count().saturating_sub(1));
        }
    }

    // Restore task selection
    if let Some(ref name) = prev_task_name {
        let tasks = app.current_tasks();
        if let Some(idx) = tasks.iter().position(|t| &t.name == name) {
            app.selected_task = idx;
        } else {
            app.selected_task = app.selected_task.min(tasks.len().saturating_sub(1));
        }
    }
}

fn categories_equal(
    a: &[crate::core::models::Category],
    b: &[crate::core::models::Category],
) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(ca, cb)| {
        ca.name == cb.name
            && ca.tasks.len() == cb.tasks.len()
            && ca.tasks.iter().zip(cb.tasks.iter()).all(|(ta, tb)| {
                ta.name == tb.name && ta.kind == tb.kind && ta.path == tb.path
            })
    })
}

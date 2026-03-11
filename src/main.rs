use std::io::IsTerminal;

use clap::Parser;

use dzworkflows::cli::args::Cli;
use dzworkflows::cli::dispatch;
use dzworkflows::core::config::Config;
use dzworkflows::core::db;

fn main() {
    let cli = Cli::parse();

    let mut config = Config::load().unwrap_or_else(|e| {
        eprintln!("warning: failed to load config: {e}");
        Config::default()
    });

    let no_tui = cli.no_tui;

    // Override workflows dir if specified
    if let Some(dir) = cli.dir {
        config.workflows_dir = dir;
    }

    // Ensure workflows directory exists
    if !config.workflows_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&config.workflows_dir) {
            eprintln!("failed to create workflows dir: {e}");
            std::process::exit(1);
        }
    }

    // Rotate old runs on startup
    match db::open_db(&config.db_path()) {
        Ok(conn) => {
            if let Err(e) = db::rotate_runs(&conn, config.log_retention_days) {
                eprintln!("warning: run rotation failed: {e}");
            }
        }
        Err(e) => eprintln!("warning: failed to open database: {e}"),
    }

    match cli.command {
        Some(cmd) => {
            let exit_code = match dispatch(&config, cmd) {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            };
            std::process::exit(exit_code);
        }
        None => {
            // No subcommand → launch TUI (only if interactive terminal)
            if no_tui || !std::io::stdin().is_terminal() {
                eprintln!("error: no subcommand given and stdin is not a terminal (or --no-tui set)");
                eprintln!("hint: use a subcommand like `dzworkflows list` or `dzworkflows run <task>`");
                std::process::exit(1);
            }
            if let Err(e) = dzworkflows::tui::run_tui(config) {
                eprintln!("TUI error: {e}");
                std::process::exit(1);
            }
        }
    }
}

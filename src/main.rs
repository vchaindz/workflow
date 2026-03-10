use clap::Parser;

use dzworkflows::cli::args::Cli;
use dzworkflows::cli::dispatch;
use dzworkflows::core::config::Config;
use dzworkflows::core::logger::rotate_logs;

fn main() {
    let cli = Cli::parse();

    let mut config = Config::load().unwrap_or_else(|e| {
        eprintln!("warning: failed to load config: {e}");
        Config::default()
    });

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

    // Rotate old logs on startup
    if let Err(e) = rotate_logs(&config.logs_dir(), config.log_retention_days) {
        eprintln!("warning: log rotation failed: {e}");
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
            // No subcommand → launch TUI
            if let Err(e) = dzworkflows::tui::run_tui(config) {
                eprintln!("TUI error: {e}");
                std::process::exit(1);
            }
        }
    }
}

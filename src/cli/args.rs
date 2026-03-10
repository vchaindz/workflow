use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "dzworkflows",
    about = "A lightweight, file-based workflow orchestrator",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Override workflows directory
    #[arg(long, global = true)]
    pub dir: Option<std::path::PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run a workflow or script task
    Run {
        /// Task reference (e.g., backup/db-full or backup.db-full)
        task: String,

        /// Preview commands without executing
        #[arg(long)]
        dry_run: bool,

        /// Set environment variables (KEY=value)
        #[arg(long = "env", value_name = "KEY=VALUE")]
        env_vars: Vec<String>,
    },

    /// List all discovered workflows and tasks
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Show run status for a task
    Status {
        /// Task reference
        task: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// View run logs
    Logs {
        /// Task reference (omit for all recent)
        task: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Number of log entries to show
        #[arg(long, default_value = "10")]
        limit: usize,
    },
}

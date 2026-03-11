use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "workflow",
    about = "A file-based workflow orchestrator for humans and AI agents",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Override workflows directory
    #[arg(long, global = true)]
    pub dir: Option<std::path::PathBuf>,

    /// Disable TUI, fail if no subcommand given
    #[arg(long, global = true)]
    pub no_tui: bool,
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

        /// Override default step timeout (seconds, 0 = no timeout)
        #[arg(long)]
        timeout: Option<u64>,

        /// Run in background, detach from terminal
        #[arg(long)]
        background: bool,

        /// Override dangerous command safety checks
        #[arg(long)]
        force: bool,
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

    /// Compare two runs of a task
    Compare {
        /// Task reference (e.g., backup/db-full)
        task: String,

        /// Specific run ID to compare (default: latest)
        #[arg(long)]
        run: Option<String>,

        /// Compare against this run ID (default: previous)
        #[arg(long = "with")]
        with: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Use AI (claude/codex) for analysis
        #[arg(long)]
        ai: bool,
    },

    /// Validate workflows without executing
    Validate {
        /// Task reference (omit to validate all)
        task: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Export workflows to a .tar.gz archive
    Export {
        /// Output file path (default: workflow-export-<date>.tar.gz)
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,

        /// Include run history database in export
        #[arg(long)]
        include_history: bool,
    },

    /// Import workflows from a .tar.gz archive
    Import {
        /// Path to the archive file
        archive: std::path::PathBuf,

        /// Overwrite existing files without prompting
        #[arg(long)]
        overwrite: bool,

        /// Skip existing files (no prompt, keep originals)
        #[arg(long)]
        skip_existing: bool,
    },

    /// Browse and manage workflow templates
    Templates {
        /// Fetch latest templates from GitHub
        #[arg(long)]
        fetch: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Use AI to update/reorganize an existing task
    AiUpdate {
        /// Task reference (e.g., backup/db-full)
        task: String,

        /// Instructions for the AI (what to change)
        #[arg(long)]
        prompt: String,

        /// Preview changes without saving
        #[arg(long)]
        dry_run: bool,

        /// Save as a new task instead of overwriting
        #[arg(long)]
        save_as: Option<String>,
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

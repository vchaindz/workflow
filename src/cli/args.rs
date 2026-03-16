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

    /// Schedule a task via cron or systemd timer
    Schedule {
        /// Task reference (e.g., backup/db-full)
        task: String,

        /// Cron expression (e.g., "0 2 * * *")
        #[arg(long)]
        cron: Option<String>,

        /// Use systemd user timer instead of crontab
        #[arg(long)]
        systemd: bool,

        /// Remove an existing schedule
        #[arg(long)]
        remove: bool,
    },

    /// Git sync workflows across machines
    Sync {
        #[command(subcommand)]
        action: SyncAction,
    },

    /// Start HTTP server for webhook-triggered workflow execution
    Serve {
        /// Port to listen on
        #[arg(long, default_value = "8080")]
        port: u16,

        /// Address to bind to
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,
    },

    /// Manage encrypted secrets store
    Secrets {
        #[command(subcommand)]
        action: SecretsAction,
    },

    /// Manage trashed tasks
    Trash {
        #[command(subcommand)]
        action: TrashAction,
    },

    /// Interact with MCP servers (list tools, call tools, check connectivity)
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },

    /// Manage key-value snapshots for workflow baselines
    Snapshot {
        #[command(subcommand)]
        action: SnapshotAction,
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

#[derive(Subcommand, Debug)]
pub enum SecretsAction {
    /// Initialize the encrypted secrets store
    Init {
        /// Path to SSH private key (auto-detected if omitted)
        #[arg(long)]
        ssh_key: Option<String>,
    },

    /// Set a secret value
    Set {
        /// Secret name
        name: String,

        /// Secret value (prompts securely if omitted)
        #[arg(long)]
        value: Option<String>,
    },

    /// Get a decrypted secret value
    Get {
        /// Secret name
        name: String,
    },

    /// List stored secret names
    List,

    /// Remove a secret
    Rm {
        /// Secret name
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum TrashAction {
    /// List trashed tasks
    List,

    /// Permanently delete all trashed tasks
    Empty,

    /// Restore a trashed task
    Restore {
        /// Name (or partial match) of the trashed file to restore
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum SyncAction {
    /// Initialize git repo in workflows directory
    Init,

    /// Clone workflows from a remote repo
    Clone {
        /// Git repository URL
        url: String,
    },

    /// Commit and push changes to remote
    Push {
        /// Custom commit message
        #[arg(short, long)]
        message: Option<String>,
    },

    /// Pull latest changes from remote
    Pull,

    /// Show sync status
    Status,

    /// Interactive guided sync setup
    Setup,

    /// List or switch branches
    Branch {
        /// Branch name to switch to (omit to list all)
        name: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum SnapshotAction {
    /// Store a snapshot value (reads stdin if --value is omitted)
    Set {
        /// Task reference (e.g., sbom.sh/sbom-check-page)
        task: String,
        /// Snapshot key
        key: String,
        /// Value to store (reads stdin if omitted)
        #[arg(long)]
        value: Option<String>,
    },
    /// Retrieve a snapshot value
    Get {
        /// Task reference
        task: String,
        /// Snapshot key
        key: String,
    },
    /// Delete a snapshot
    Delete {
        /// Task reference
        task: String,
        /// Snapshot key
        key: String,
    },
    /// List stored snapshots
    List {
        /// Filter by task reference
        task: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum McpAction {
    /// List available tools from an MCP server
    ListTools {
        /// Server alias (from config.toml) or inline command
        server: String,

        /// Output full tool list as JSON including input schemas
        #[arg(long)]
        json: bool,
    },

    /// Call a tool on an MCP server
    Call {
        /// Server alias (from config.toml) or inline command
        server: String,

        /// Tool name to call
        tool: String,

        /// Tool arguments as key=value pairs
        #[arg(long = "arg", value_name = "KEY=VALUE")]
        args: Vec<String>,
    },

    /// Check connectivity to an MCP server
    Check {
        /// Server alias (from config.toml) or inline command
        server: String,
    },
}

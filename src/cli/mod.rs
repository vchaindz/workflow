pub mod args;
pub mod list;
pub mod logs;
pub mod run;
pub mod status;

use crate::core::config::Config;
use crate::error::Result;
use args::Commands;

pub fn dispatch(config: &Config, command: Commands) -> Result<i32> {
    match command {
        Commands::Run {
            task,
            dry_run,
            env_vars,
        } => run::cmd_run(config, &task, dry_run, &env_vars),

        Commands::List { json } => {
            list::cmd_list(config, json)?;
            Ok(0)
        }

        Commands::Status { task, json } => {
            status::cmd_status(config, &task, json)?;
            Ok(0)
        }

        Commands::Logs { task, json, limit } => {
            logs::cmd_logs(config, task.as_deref(), json, limit)?;
            Ok(0)
        }
    }
}

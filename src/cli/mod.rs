pub mod args;
pub mod compare;
pub mod export;
pub mod import;
pub mod list;
pub mod logs;
pub mod run;
pub mod status;
pub mod validate;

use crate::core::config::Config;
use crate::error::Result;
use args::Commands;

pub fn dispatch(config: &Config, command: Commands) -> Result<i32> {
    match command {
        Commands::Run {
            task,
            dry_run,
            env_vars,
            timeout,
            background,
        } => run::cmd_run(config, &task, dry_run, &env_vars, timeout, background),

        Commands::List { json } => {
            list::cmd_list(config, json)?;
            Ok(0)
        }

        Commands::Status { task, json } => {
            status::cmd_status(config, &task, json)?;
            Ok(0)
        }

        Commands::Compare { task, run, with, json, ai } => {
            compare::cmd_compare(config, &task, run.as_deref(), with.as_deref(), json, ai)?;
            Ok(0)
        }

        Commands::Validate { task, json } => {
            validate::cmd_validate(config, task.as_deref(), json)
        }

        Commands::Export { output, include_history } => {
            export::cmd_export(config, output.as_deref(), include_history)?;
            Ok(0)
        }

        Commands::Import { archive, overwrite, skip_existing } => {
            import::cmd_import(config, &archive, overwrite, skip_existing)?;
            Ok(0)
        }

        Commands::Logs { task, json, limit } => {
            logs::cmd_logs(config, task.as_deref(), json, limit)?;
            Ok(0)
        }
    }
}

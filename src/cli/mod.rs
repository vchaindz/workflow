pub mod ai_update;
pub mod args;
pub mod compare;
pub mod export;
pub mod import;
pub mod list;
pub mod logs;
pub mod run;
pub mod schedule;
pub mod status;
pub mod templates;
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
            force,
        } => run::cmd_run(config, &task, dry_run, &env_vars, timeout, background, force),

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

        Commands::Templates { fetch, json } => {
            templates::cmd_templates(config, fetch, json)?;
            Ok(0)
        }

        Commands::AiUpdate { task, prompt, dry_run, save_as } => {
            ai_update::cmd_ai_update(config, &task, &prompt, dry_run, save_as.as_deref())?;
            Ok(0)
        }

        Commands::Schedule { task, cron, systemd, remove } => {
            schedule::cmd_schedule(config, &task, cron.as_deref(), systemd, remove)?;
            Ok(0)
        }

        Commands::Logs { task, json, limit } => {
            logs::cmd_logs(config, task.as_deref(), json, limit)?;
            Ok(0)
        }
    }
}

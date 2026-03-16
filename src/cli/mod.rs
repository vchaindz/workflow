pub mod ai_update;
pub mod args;
pub mod compare;
pub mod export;
pub mod import;
pub mod list;
pub mod logs;
pub mod memory;
#[cfg(feature = "mcp")]
pub mod mcp;
pub mod run;
pub mod schedule;
pub mod secrets;
pub mod snapshot;
pub mod status;
pub mod sync;
pub mod templates;
pub mod serve;
pub mod trash;
pub mod validate;

use crate::core::config::Config;
use crate::error::Result;
use args::{Commands, SecretsAction};

pub fn dispatch(config: &mut Config, command: Commands) -> Result<i32> {
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

        Commands::Sync { action } => {
            sync::cmd_sync(config, action)
        }

        Commands::Secrets { action } => {
            match action {
                SecretsAction::Init { ssh_key } => {
                    secrets::cmd_secrets_init(config, ssh_key.as_deref())?;
                }
                SecretsAction::Set { name, value } => {
                    secrets::cmd_secrets_set(config, &name, value.as_deref())?;
                }
                SecretsAction::Get { name } => {
                    secrets::cmd_secrets_get(config, &name)?;
                }
                SecretsAction::List => {
                    secrets::cmd_secrets_list(config)?;
                }
                SecretsAction::Rm { name } => {
                    secrets::cmd_secrets_rm(config, &name)?;
                }
            }
            Ok(0)
        }

        Commands::Trash { action } => {
            trash::cmd_trash(config, action)?;
            Ok(0)
        }

        Commands::Mcp { action } => {
            #[cfg(feature = "mcp")]
            {
                mcp::cmd_mcp(config, action)?;
                Ok(0)
            }
            #[cfg(not(feature = "mcp"))]
            {
                let _ = action;
                eprintln!("MCP commands require the mcp feature. Rebuild with: cargo build --features mcp");
                Ok(1)
            }
        }

        Commands::Memory { action } => {
            memory::cmd_memory(config, action)?;
            Ok(0)
        }

        Commands::Snapshot { action } => {
            snapshot::cmd_snapshot(config, action)?;
            Ok(0)
        }

        Commands::Serve { port, bind } => serve::cmd_serve(config, port, &bind),

        Commands::Logs { task, json, limit } => {
            logs::cmd_logs(config, task.as_deref(), json, limit)?;
            Ok(0)
        }
    }
}

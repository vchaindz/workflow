use crate::core::config::Config;
use crate::core::discovery::scan_workflows;
use crate::core::models::TaskKind;
use crate::error::Result;

pub fn cmd_list(config: &Config, json: bool) -> Result<()> {
    let categories = scan_workflows(&config.workflows_dir)?;

    if json {
        let output = serde_json::to_string_pretty(&categories)?;
        println!("{output}");
    } else {
        for cat in &categories {
            println!("{}:", cat.name);
            for task in &cat.tasks {
                let kind = match task.kind {
                    TaskKind::ShellScript => "sh",
                    TaskKind::YamlWorkflow => "yaml",
                };
                println!("  {} [{}]", task.name, kind);
            }
        }
    }

    Ok(())
}

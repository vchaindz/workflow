use crate::cli::args::McpAction;
use crate::core::config::Config;
use crate::error::Result;

pub fn cmd_mcp(config: &Config, action: McpAction) -> Result<()> {
    match action {
        McpAction::ListTools { server, json } => {
            cmd_list_tools(config, &server, json)
        }
        McpAction::Call { server, tool, args } => {
            cmd_call(config, &server, &tool, &args)
        }
        McpAction::Check { server } => {
            cmd_check(config, &server)
        }
    }
}

fn cmd_list_tools(config: &Config, server: &str, json: bool) -> Result<()> {
    let _ = (config, server, json);
    eprintln!("mcp list-tools: not yet implemented");
    Ok(())
}

fn cmd_call(config: &Config, server: &str, tool: &str, args: &[String]) -> Result<()> {
    let _ = (config, server, tool, args);
    eprintln!("mcp call: not yet implemented");
    Ok(())
}

fn cmd_check(config: &Config, server: &str) -> Result<()> {
    let _ = (config, server);
    eprintln!("mcp check: not yet implemented");
    Ok(())
}

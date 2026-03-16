use std::collections::HashMap;

use crate::cli::args::McpAction;
use crate::core::config::Config;
use crate::core::mcp::McpClient;
use crate::error::{DzError, Result};

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

/// Resolve a server reference: if it matches a config alias, use that config's command and env.
/// Otherwise treat the string as an inline command.
fn resolve_server(config: &Config, server: &str) -> (String, HashMap<String, String>) {
    if let Some(srv_cfg) = config.mcp.servers.get(server) {
        let env = srv_cfg.env.clone().unwrap_or_default();
        (srv_cfg.command.clone(), env)
    } else {
        // Treat as inline command string
        (server.to_string(), HashMap::new())
    }
}

fn cmd_list_tools(config: &Config, server: &str, json: bool) -> Result<()> {
    let (command, env) = resolve_server(config, server);

    let client = McpClient::spawn(&command, env)?;
    let tools = client.list_tools()?;
    client.shutdown();

    if json {
        // Full JSON output with schemas
        let json_tools: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_tools)
            .map_err(|e| DzError::Mcp(format!("failed to serialize tools: {e}")))?);
    } else {
        // Formatted table output
        if tools.is_empty() {
            println!("No tools available from server '{}'", server);
            return Ok(());
        }

        // Calculate column widths
        let name_width = tools.iter().map(|t| t.name.len()).max().unwrap_or(4).max(4);
        let params_header = "Params";
        let desc_max = 60;

        println!(
            "{:<name_width$}  {:<6}  Description",
            "Tool", params_header,
            name_width = name_width,
        );
        println!(
            "{:<name_width$}  {:<6}  -----------",
            "----", "------",
            name_width = name_width,
        );

        for tool in &tools {
            let param_count = tool
                .input_schema
                .get("properties")
                .and_then(|p| p.as_object())
                .map(|p| p.len())
                .unwrap_or(0);

            let desc = tool
                .description
                .as_deref()
                .unwrap_or("-");
            let desc_truncated = if desc.len() > desc_max {
                format!("{}...", &desc[..desc_max - 3])
            } else {
                desc.to_string()
            };

            println!(
                "{:<name_width$}  {:<6}  {}",
                tool.name,
                param_count,
                desc_truncated,
                name_width = name_width,
            );
        }

        println!("\n{} tool(s) available", tools.len());
    }

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

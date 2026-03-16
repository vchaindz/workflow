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
    let (command, mut env) = resolve_server(config, server);

    // Inject secrets from the secrets store if server is a config alias
    if let Some(srv_cfg) = config.mcp.servers.get(server) {
        if let Some(ref secrets) = srv_cfg.secrets {
            let secret_env = crate::core::executor::load_secret_env(
                secrets,
                &config.workflows_dir,
                config.secrets_ssh_key.as_ref().map(std::path::Path::new),
            );
            for (k, v) in secret_env {
                env.insert(k, v);
            }
        }
    }

    // Parse --arg key=value pairs into a JSON object
    let json_args = parse_args_to_json(args)?;

    let client = McpClient::spawn(&command, env)?;
    let result = client.call_tool(tool, json_args);
    client.shutdown();

    match result {
        Ok(text) => {
            println!("{}", text);
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

/// Parse `key=value` argument strings into a serde_json::Value object.
/// Values that look like numbers or booleans are converted; otherwise kept as strings.
fn parse_args_to_json(args: &[String]) -> Result<serde_json::Value> {
    if args.is_empty() {
        return Ok(serde_json::Value::Null);
    }

    let mut map = serde_json::Map::new();
    for arg in args {
        let (key, value) = arg.split_once('=').ok_or_else(|| {
            DzError::Mcp(format!(
                "invalid argument '{}': expected KEY=VALUE format",
                arg
            ))
        })?;

        let json_value = parse_arg_value(value);
        map.insert(key.to_string(), json_value);
    }
    Ok(serde_json::Value::Object(map))
}

/// Parse a string value into the most appropriate JSON type.
fn parse_arg_value(value: &str) -> serde_json::Value {
    // Try boolean
    if value == "true" {
        return serde_json::Value::Bool(true);
    }
    if value == "false" {
        return serde_json::Value::Bool(false);
    }
    // Try integer
    if let Ok(n) = value.parse::<i64>() {
        return serde_json::Value::Number(n.into());
    }
    // Try float
    if let Ok(f) = value.parse::<f64>() {
        if let Some(n) = serde_json::Number::from_f64(f) {
            return serde_json::Value::Number(n);
        }
    }
    // Default to string
    serde_json::Value::String(value.to_string())
}

fn cmd_check(config: &Config, server: &str) -> Result<()> {
    let (command, mut env) = resolve_server(config, server);

    // Inject secrets from the secrets store if server is a config alias
    if let Some(srv_cfg) = config.mcp.servers.get(server) {
        if let Some(ref secrets) = srv_cfg.secrets {
            let secret_env = crate::core::executor::load_secret_env(
                secrets,
                &config.workflows_dir,
                config.secrets_ssh_key.as_ref().map(std::path::Path::new),
            );
            for (k, v) in secret_env {
                env.insert(k, v);
            }
        }
    }

    // Step 1: Spawn server
    print!("Spawning server '{}'... ", server);
    let client = match McpClient::spawn(&command, env) {
        Ok(c) => {
            println!("OK");
            c
        }
        Err(e) => {
            println!("FAILED");
            eprintln!("  Error: {}", e);
            std::process::exit(1);
        }
    };

    // Step 2: List tools (validates initialize + tools/list work)
    print!("Listing tools... ");
    let tools = match client.list_tools() {
        Ok(t) => {
            println!("OK");
            t
        }
        Err(e) => {
            println!("FAILED");
            eprintln!("  Error: {}", e);
            client.shutdown();
            std::process::exit(1);
        }
    };

    client.shutdown();

    // Report results
    println!("\nServer '{}' is healthy", server);
    println!("  Tools available: {}", tools.len());
    if !tools.is_empty() {
        for tool in &tools {
            println!("    - {}", tool.name);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_args_empty() {
        let result = parse_args_to_json(&[]).unwrap();
        assert_eq!(result, serde_json::Value::Null);
    }

    #[test]
    fn test_parse_args_string_values() {
        let args = vec!["repo=myorg/myapp".to_string(), "title=Bug report".to_string()];
        let result = parse_args_to_json(&args).unwrap();
        let obj = result.as_object().unwrap();
        assert_eq!(obj["repo"], serde_json::Value::String("myorg/myapp".into()));
        assert_eq!(obj["title"], serde_json::Value::String("Bug report".into()));
    }

    #[test]
    fn test_parse_args_typed_values() {
        let args = vec![
            "count=42".to_string(),
            "enabled=true".to_string(),
            "disabled=false".to_string(),
            "rate=3.14".to_string(),
        ];
        let result = parse_args_to_json(&args).unwrap();
        let obj = result.as_object().unwrap();
        assert_eq!(obj["count"], serde_json::json!(42));
        assert_eq!(obj["enabled"], serde_json::json!(true));
        assert_eq!(obj["disabled"], serde_json::json!(false));
        assert_eq!(obj["rate"], serde_json::json!(3.14));
    }

    #[test]
    fn test_parse_args_invalid_format() {
        let args = vec!["no-equals-sign".to_string()];
        let result = parse_args_to_json(&args);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("KEY=VALUE"));
    }

    #[test]
    fn test_parse_args_value_with_equals() {
        // value containing '=' should only split on first '='
        let args = vec!["query=SELECT * FROM t WHERE a=1".to_string()];
        let result = parse_args_to_json(&args).unwrap();
        let obj = result.as_object().unwrap();
        assert_eq!(
            obj["query"],
            serde_json::Value::String("SELECT * FROM t WHERE a=1".into())
        );
    }

    #[test]
    fn test_parse_arg_value_string_not_number() {
        // Strings that look like numbers but shouldn't be confused
        assert_eq!(parse_arg_value("hello"), serde_json::Value::String("hello".into()));
    }
}

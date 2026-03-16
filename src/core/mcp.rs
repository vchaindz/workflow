use std::collections::HashMap;

use rmcp::{
    model::{CallToolRequestParams, CallToolResult, Content, Tool},
    service::RunningService,
    transport::TokioChildProcess,
    RoleClient, ServiceExt,
};
use tokio::process::Command;

use crate::error::{DzError, Result};

/// Information about an MCP tool, extracted from the rmcp Tool type.
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

impl From<Tool> for ToolInfo {
    fn from(t: Tool) -> Self {
        let input_schema = t.schema_as_json_value();
        ToolInfo {
            name: t.name.to_string(),
            description: t.description.map(|d| d.to_string()),
            input_schema,
        }
    }
}

/// A synchronous wrapper around an rmcp MCP client with stdio transport.
pub struct McpClient {
    peer: rmcp::service::Peer<RoleClient>,
    // Hold the running service to keep the connection alive
    _service: RunningService<RoleClient, ()>,
}

impl McpClient {
    /// Spawn an MCP server as a child process and initialize the connection.
    ///
    /// `command` is a shell command string (e.g. "npx @modelcontextprotocol/server-github").
    /// `env` is a map of environment variables to set on the child process.
    pub fn spawn(command: &str, env: HashMap<String, String>) -> Result<Self> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| DzError::Mcp(format!("failed to create tokio runtime: {e}")))?;
        rt.block_on(async { Self::spawn_async(command, env).await })
    }

    async fn spawn_async(command: &str, env: HashMap<String, String>) -> Result<Self> {
        // Parse command into program + args (shell-style split)
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return Err(DzError::Mcp("empty MCP server command".into()));
        }

        let mut cmd = Command::new(parts[0]);
        for arg in &parts[1..] {
            cmd.arg(arg);
        }
        for (k, v) in &env {
            cmd.env(k, v);
        }

        let transport = TokioChildProcess::new(cmd)
            .map_err(|e| DzError::Mcp(format!("failed to spawn MCP server '{}': {e}", command)))?;

        let service = ().serve(transport).await.map_err(|e| {
            DzError::Mcp(format!(
                "failed to initialize MCP connection to '{}': {e}",
                command
            ))
        })?;

        let peer = service.peer().clone();

        Ok(McpClient {
            peer,
            _service: service,
        })
    }

    /// List all tools available on the MCP server.
    pub fn list_tools(&self) -> Result<Vec<ToolInfo>> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| DzError::Mcp(format!("failed to create tokio runtime: {e}")))?;
        rt.block_on(async {
            let tools = self
                .peer
                .list_all_tools()
                .await
                .map_err(|e| DzError::Mcp(format!("failed to list tools: {e}")))?;
            Ok(tools.into_iter().map(ToolInfo::from).collect())
        })
    }

    /// Call a tool on the MCP server and return the text content of the result.
    ///
    /// If the tool returns an error (isError flag), this returns an Err with
    /// the error text. Otherwise returns the concatenated text content.
    pub fn call_tool(&self, tool: &str, args: serde_json::Value) -> Result<String> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| DzError::Mcp(format!("failed to create tokio runtime: {e}")))?;
        rt.block_on(async { self.call_tool_async(tool, args).await })
    }

    async fn call_tool_async(&self, tool: &str, args: serde_json::Value) -> Result<String> {
        let arguments = match args {
            serde_json::Value::Object(map) => Some(map),
            serde_json::Value::Null => None,
            other => {
                // Wrap non-object values into {"input": value}
                let mut map = serde_json::Map::new();
                map.insert("input".to_string(), other);
                Some(map)
            }
        };

        let params = CallToolRequestParams {
            meta: None,
            name: tool.to_string().into(),
            arguments,
            task: None,
        };

        let result: CallToolResult = self
            .peer
            .call_tool(params)
            .await
            .map_err(|e| DzError::Mcp(format!("failed to call tool '{}': {e}", tool)))?;

        let text = extract_text(&result.content);

        if result.is_error.unwrap_or(false) {
            return Err(DzError::Mcp(format!("tool '{}' returned error: {}", tool, text)));
        }

        Ok(text)
    }

    /// Gracefully shut down the MCP server connection.
    pub fn shutdown(self) {
        // Dropping self drops the RunningService and Peer, which triggers cleanup.
        drop(self);
    }
}

/// Extract text content from MCP Content items, concatenating all text parts.
fn extract_text(content: &[Content]) -> String {
    let mut parts = Vec::new();
    for item in content {
        use rmcp::model::RawContent;
        match &**item {
            RawContent::Text(t) => parts.push(t.text.clone()),
            _ => {}
        }
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_text_empty() {
        let content: Vec<Content> = vec![];
        assert_eq!(extract_text(&content), "");
    }

    #[test]
    fn test_tool_info_conversion() {
        use std::sync::Arc;
        let tool = Tool::new("test_tool", "A test tool", Arc::new(serde_json::Map::new()));
        let info = ToolInfo::from(tool);
        assert_eq!(info.name, "test_tool");
        assert_eq!(info.description, Some("A test tool".to_string()));
    }
}

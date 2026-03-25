use std::collections::HashMap;

use rmcp::model::{CallToolRequestParams, Content, Implementation, Tool};
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use tokio::process::Command;

use crate::error::{DzError, Result};

/// Information about an MCP tool.
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

impl From<&Tool> for ToolInfo {
    fn from(t: &Tool) -> Self {
        ToolInfo {
            name: t.name.to_string(),
            description: t.description.as_ref().map(|d| d.to_string()),
            input_schema: t.schema_as_json_value(),
        }
    }
}

pub use crate::core::models::McpTransportConfig;

/// Parse a command string into program + args using shell-style quoting rules.
fn parse_command(command: &str) -> Result<Vec<String>> {
    let parts = shlex::split(command).ok_or_else(|| {
        DzError::Mcp("failed to parse MCP server command (unmatched quotes?)".into())
    })?;
    if parts.is_empty() {
        return Err(DzError::Mcp("empty MCP server command".into()));
    }
    Ok(parts)
}

/// Minimal client handler providing our client identity.
#[derive(Clone)]
struct Handler;

impl ClientHandler for Handler {
    fn get_info(&self) -> rmcp::model::ClientInfo {
        rmcp::model::ClientInfo::new(
            rmcp::model::ClientCapabilities::default(),
            Implementation::new("workflow", env!("CARGO_PKG_VERSION")),
        )
    }
}

/// An MCP client backed by rmcp's typed async transport,
/// wrapped in a synchronous API via a tokio runtime.
pub struct McpClient {
    service: RunningService<RoleClient, Handler>,
    runtime: tokio::runtime::Runtime,
}

impl McpClient {
    /// Connect to an MCP server using the given transport configuration.
    pub fn connect(config: &McpTransportConfig) -> Result<Self> {
        match config {
            McpTransportConfig::Stdio { command, env } => Self::spawn(command, env.clone()),
            McpTransportConfig::Http {
                url,
                auth_header,
                headers,
            } => Self::connect_http(url, auth_header.as_deref(), headers),
        }
    }

    /// Spawn an MCP server as a child process and perform the
    /// MCP initialization handshake (stdio transport).
    pub fn spawn(command: &str, env: HashMap<String, String>) -> Result<Self> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| DzError::Mcp(format!("failed to create tokio runtime: {e}")))?;

        let parts = parse_command(command)?;

        let service = rt.block_on(async {
            let mut cmd = Command::new(&parts[0]);
            for arg in &parts[1..] {
                cmd.arg(arg);
            }
            for (k, v) in &env {
                cmd.env(k, v);
            }

            let (transport, _stderr) = TokioChildProcess::builder(cmd)
                .spawn()
                .map_err(|e| {
                    DzError::Mcp(format!("failed to spawn MCP server '{}': {e}", command))
                })?;

            Handler
                .serve(transport)
                .await
                .map_err(|e| DzError::Mcp(format!("MCP initialize failed: {e}")))
        })?;

        Ok(McpClient { service, runtime: rt })
    }

    /// Connect to an MCP server via Streamable HTTP transport.
    pub fn connect_http(
        url: &str,
        auth_header: Option<&str>,
        headers: &HashMap<String, String>,
    ) -> Result<Self> {
        use rmcp::transport::streamable_http_client::{
            StreamableHttpClientTransportConfig, StreamableHttpClientWorker,
        };
        use rmcp::transport::worker::WorkerTransport;

        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| DzError::Mcp(format!("failed to create tokio runtime: {e}")))?;

        let service = rt.block_on(async {
            let config = StreamableHttpClientTransportConfig::with_uri(url);

            // Build a reqwest client with default headers for auth and custom headers.
            // We use default_headers instead of the config's auth_header field because
            // that field always wraps with "Bearer " prefix, which doesn't work for
            // non-Bearer auth schemes (e.g. cPanel's "whm root:TOKEN").
            let mut default_headers = reqwest::header::HeaderMap::new();
            if let Some(auth) = auth_header {
                let value = reqwest::header::HeaderValue::from_str(auth)
                    .map_err(|e| DzError::Mcp(format!("invalid auth header value: {e}")))?;
                default_headers.insert(reqwest::header::AUTHORIZATION, value);
            }
            for (k, v) in headers {
                let name = reqwest::header::HeaderName::from_bytes(k.as_bytes())
                    .map_err(|e| DzError::Mcp(format!("invalid header name '{}': {e}", k)))?;
                let value = reqwest::header::HeaderValue::from_str(v)
                    .map_err(|e| DzError::Mcp(format!("invalid header value '{}': {e}", v)))?;
                default_headers.insert(name, value);
            }

            let client = reqwest::Client::builder()
                .default_headers(default_headers)
                .build()
                .map_err(|e| DzError::Mcp(format!("failed to build HTTP client: {e}")))?;

            let worker = StreamableHttpClientWorker::new(client, config);
            let transport = WorkerTransport::spawn(worker);

            Handler
                .serve(transport)
                .await
                .map_err(|e| DzError::Mcp(format!("MCP initialize failed: {e}")))
        })?;

        Ok(McpClient { service, runtime: rt })
    }

    /// Connect, list tools, shut down — convenience for one-shot tool listing.
    pub fn connect_and_list_tools(config: &McpTransportConfig) -> Result<Vec<ToolInfo>> {
        let mut client = Self::connect(config)?;
        let tools = client.list_tools()?;
        client.shutdown();
        Ok(tools)
    }

    /// Connect, call one tool, shut down — convenience for one-shot tool calls.
    pub fn connect_and_call(
        config: &McpTransportConfig,
        tool: &str,
        args: serde_json::Value,
    ) -> Result<String> {
        let mut client = Self::connect(config)?;
        let result = client.call_tool(tool, args);
        client.shutdown();
        result
    }

    /// Spawn, list tools, shut down — convenience for one-shot tool listing (stdio).
    pub fn spawn_and_list_tools(
        command: &str,
        env: HashMap<String, String>,
    ) -> Result<Vec<ToolInfo>> {
        let mut client = Self::spawn(command, env)?;
        let tools = client.list_tools()?;
        client.shutdown();
        Ok(tools)
    }

    /// Spawn, call one tool, shut down — convenience for one-shot tool calls (stdio).
    pub fn spawn_and_call(
        command: &str,
        env: HashMap<String, String>,
        tool: &str,
        args: serde_json::Value,
    ) -> Result<String> {
        let mut client = Self::spawn(command, env)?;
        let result = client.call_tool(tool, args);
        client.shutdown();
        result
    }

    /// List all tools available on the MCP server.
    pub fn list_tools(&mut self) -> Result<Vec<ToolInfo>> {
        let result = self
            .runtime
            .block_on(self.service.peer().list_tools(None))
            .map_err(|e| DzError::Mcp(format!("tools/list failed: {e}")))?;

        Ok(result.tools.iter().map(ToolInfo::from).collect())
    }

    /// Call a tool on the MCP server and return the text content of the result.
    pub fn call_tool(&mut self, tool: &str, args: serde_json::Value) -> Result<String> {
        let arguments = match args {
            serde_json::Value::Object(map) => Some(map),
            serde_json::Value::Null => None,
            other => {
                let mut map = serde_json::Map::new();
                map.insert("input".to_string(), other);
                Some(map)
            }
        };

        let mut params = CallToolRequestParams::new(tool.to_string());
        if let Some(args) = arguments {
            params = params.with_arguments(args);
        }

        let result = self
            .runtime
            .block_on(self.service.peer().call_tool(params))
            .map_err(|e| DzError::Mcp(format!("tool '{}' call failed: {e}", tool)))?;

        if result.is_error.unwrap_or(false) {
            let text = extract_text_content(&result.content);
            return Err(DzError::Mcp(format!(
                "tool '{}' returned error: {}",
                tool, text
            )));
        }

        Ok(extract_text_content(&result.content))
    }

    /// Gracefully shut down the MCP server connection.
    pub fn shutdown(self) {
        let _ = self.runtime.block_on(self.service.cancel());
    }
}

/// Extract text content from MCP result content array, stripping ANSI escapes.
fn extract_text_content(content: &[Content]) -> String {
    let texts: Vec<&str> = content
        .iter()
        .filter_map(|item| item.as_text().map(|t| t.text.as_str()))
        .collect();
    crate::core::executor::strip_ansi_codes(&texts.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_command_simple() {
        let parts = parse_command("npx server-github").unwrap();
        assert_eq!(parts, vec!["npx", "server-github"]);
    }

    #[test]
    fn test_parse_command_quoted() {
        let parts =
            parse_command("/usr/bin/proxy -header 'Authorization=whm root:TOKEN' https://host")
                .unwrap();
        assert_eq!(
            parts,
            vec![
                "/usr/bin/proxy",
                "-header",
                "Authorization=whm root:TOKEN",
                "https://host"
            ]
        );
    }

    #[test]
    fn test_parse_command_empty() {
        assert!(parse_command("").is_err());
    }

    #[test]
    fn test_parse_command_unmatched_quote() {
        assert!(parse_command("foo 'bar").is_err());
    }

    #[test]
    fn test_transport_config_stdio() {
        let config = McpTransportConfig::Stdio {
            command: "npx server".to_string(),
            env: HashMap::new(),
        };
        assert!(matches!(config, McpTransportConfig::Stdio { .. }));
    }

    #[test]
    fn test_transport_config_http() {
        let config = McpTransportConfig::Http {
            url: "https://example.com/mcp".to_string(),
            auth_header: Some("Bearer token".to_string()),
            headers: HashMap::new(),
        };
        assert!(matches!(config, McpTransportConfig::Http { .. }));
    }
}

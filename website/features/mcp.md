# MCP Integration

Call any of 16,000+ MCP-compatible tools -- GitHub, Slack, databases, cloud providers -- directly from workflow steps. No shell glue code, no `curl` pipelines, no API client scripting. MCP steps are a first-class step type alongside `cmd:` and `call:`.

## Enabling MCP support

MCP is opt-in behind a cargo feature flag to keep the base binary small:

::: code-group

```bash [Build from source]
cargo build --release --features mcp
```

```bash [Install]
cargo install --path . --features mcp
```

:::

## Configuring MCP servers

Define server aliases in `config.toml` so workflows can reference them by short name. Two transports are supported.

### Stdio transport

Spawns the server as a child process and communicates via JSON-RPC over stdin/stdout. This is the same protocol used by Claude Code, VS Code, and other MCP hosts.

```toml
[mcp.servers.github]
command = "npx -y @modelcontextprotocol/server-github"
secrets = ["GITHUB_TOKEN"]

[mcp.servers.slack]
command = "npx -y @modelcontextprotocol/server-slack"
secrets = ["SLACK_BOT_TOKEN"]
env = { SLACK_TEAM_ID = "T0123456" }
```

**Stdio server fields:**

| Field | Required | Description |
|-------|----------|-------------|
| `command` | Yes | Shell command to spawn the MCP server process |
| `secrets` | No | Secret names resolved from the [encrypted store](/features/secrets) and injected as env vars |
| `env` | No | Additional environment variables for the server process |
| `timeout` | No | Timeout in seconds for tool calls |

### HTTP transport

Connects directly to a remote MCP endpoint via Streamable HTTP. No proxy needed.

```toml
[mcp.servers.cpanel-whm]
url = "https://myserver.example.com:2087/mcp"
auth_header = "whm root:APITOKEN"
timeout = 60
```

**HTTP server fields:**

| Field | Required | Description |
|-------|----------|-------------|
| `url` | Yes | HTTP/HTTPS endpoint URL for the MCP server |
| `auth_header` | No | Raw `Authorization` header value (any scheme: Bearer, Basic, WHM, etc.) |
| `headers` | No | Additional custom HTTP headers |
| `timeout` | No | Timeout in seconds for tool calls |

::: tip
Credentials should be stored in the [encrypted secrets store](/features/secrets) and referenced via `secrets = [...]` rather than hardcoded in config files.
:::

## Writing MCP steps

Use the `mcp:` field instead of `cmd:` to call an MCP tool:

```yaml
name: GitHub Release Workflow
steps:
  - id: create-release
    mcp:
      server: github
      tool: create_release
      args:
        owner: myorg
        repo: myapp
        tag: "v{{version}}"
        body: "Release {{version}} -- {{date}}"
    outputs:
      - name: url
        pattern: "(https://github.com/.*releases/.*)"

  - id: notify-slack
    mcp:
      server: slack
      tool: send_message
      args:
        channel: "#releases"
        text: "Released {{version}}: {{create-release.url}}"
    needs: [create-release]
```

MCP steps support all the same DAG features as `cmd:` steps:

- `needs:` dependencies
- `run_if` / `skip_if` conditions
- `retry` / `retry_delay`
- `timeout`
- `for_each` loops
- Output capture via `outputs:`
- Template variable expansion (`{{var}}`, `{{date}}`, `{{step_id.output_name}}`)

### Inline server definitions

For one-off use or portable workflows, define the server inline instead of referencing a config alias:

```yaml
steps:
  - id: query
    mcp:
      server:
        command: "npx -y @modelcontextprotocol/server-postgres"
        env:
          DATABASE_URL: "postgres://localhost/mydb"
        secrets: ["DB_PASSWORD"]
      tool: query
      args:
        sql: "SELECT count(*) FROM users WHERE created_at > '{{date_offset -1d}}'"
```

### Practical example

A database backup workflow that uses a `cmd:` step for the dump and an `mcp:` step for notification:

```yaml
name: DB Backup with MCP
steps:
  - id: dump
    cmd: pg_dump mydb > /tmp/mydb_{{date}}.sql
    timeout: 300

  - id: record-size
    cmd: stat --format='%s' /tmp/mydb_{{date}}.sql
    needs: [dump]
    outputs:
      - name: bytes
        pattern: "^(\\d+)"

  - id: notify
    mcp:
      server: slack
      tool: send_message
      args:
        channel: "#ops"
        text: "Backup complete: {{record-size.bytes}} bytes"
    needs: [record-size]

cleanup:
  - id: clean
    cmd: rm -f /tmp/mydb_{{date}}.sql
```

## CLI commands

Discover and test MCP servers from the command line:

```bash
# List all tools available on a server
workflow mcp list-tools github
workflow mcp list-tools github --json    # full tool schemas

# Call a tool directly (ad-hoc testing)
workflow mcp call github create_issue \
  --arg repo=myorg/myapp \
  --arg title="Bug report" \
  --arg body="Found an issue with..."

# Health check a server (verify connectivity and credentials)
workflow mcp check github
```

### list-tools

Displays tool names, parameter counts, and descriptions in a formatted table. With `--json`, outputs full tool schemas including input parameters -- useful for scripting or discovering API shapes.

### call

Parses `--arg key=value` pairs into a JSON object. Values are auto-typed:

| Input | Parsed as |
|-------|-----------|
| `true` / `false` | Boolean |
| Numeric strings (`42`, `3.14`) | Number |
| Everything else | String |

Server credentials are injected from the secrets store automatically.

### check

Spawns the server, initializes the MCP connection, and lists tools -- verifying that the command works, credentials are valid, and the server responds. Useful for debugging setup issues.

## Execution flow

When a step with `mcp:` is executed:

1. Server config is resolved (alias lookup in `config.toml` or inline definition)
2. For stdio: secrets are loaded from the encrypted store and injected as environment variables. For HTTP: the auth header is attached to requests.
3. Template variables (`{{var}}`) are expanded in all `args` string values, recursively through nested objects and arrays
4. The MCP connection is established (child process spawn or HTTP handshake)
5. The tool is called with the expanded args
6. The result text is captured as stdout (available for `outputs:` regex patterns and downstream `{{step_id.var}}` references)
7. The connection is shut down

## JSON output in the TUI

MCP tools typically return JSON data. The detail pane automatically detects JSON output and enhances the display with pretty-printing, syntax highlighting, and collapsible sections.

**Detail pane keybindings:**

| Key | Action |
|-----|--------|
| `-` | Collapse JSON block at current line |
| `+` | Expand collapsed JSON block |
| `Z` | Fold all / unfold all |
| `{` / `}` | Jump to previous / next JSON block |
| `PgUp` / `PgDn` | Scroll by 20 lines |

Syntax highlighting uses color to distinguish keys (cyan), strings (green), numbers (yellow), booleans (magenta), and braces (white bold).

## AI MCP-awareness

The AI wizard (`a` key in TUI) is MCP-aware. When MCP servers are configured, it prefers generating `mcp:` steps over shell commands for matching services. For example, if you have a `github` server configured and ask "create a release workflow", the AI generates `mcp: { server: github, tool: create_release }` instead of raw `curl` calls to the GitHub API.

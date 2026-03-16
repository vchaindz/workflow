# PRD: MCP Integration for workflow

| Field | Value |
|-------|-------|
| Author | Dennis Zimmer |
| Date | March 16, 2026 |
| Version | 1.0 |
| Status | Draft |
| Target release | workflow v0.5.0 |

## 1. Executive Summary

This PRD defines the integration of the **Model Context Protocol (MCP)** into the `workflow` orchestrator as a first-class step type. MCP is the open standard (backed by Anthropic, adopted by OpenAI and Google) for connecting AI tools to external services. With 16,000+ MCP servers indexed across registries, adding MCP client support to `workflow` transforms it from a shell-centric task runner into a universal orchestration platform that can natively call GitHub, Slack, databases, cloud providers, Kubernetes, and thousands of other services without writing shell glue code.

This replaces the need to build a custom plugin ecosystem. Instead of *"workflow has N plugins,"* the pitch becomes *"workflow speaks MCP — every MCP server is a workflow step."* This is a stronger story than n8n's 400+ nodes because the MCP ecosystem is growing at 10x the rate and is backed by the entire AI industry.

## 2. Problem Statement

Today, workflow steps that interact with external services (GitHub, Slack, AWS, databases) require users to write shell commands with curl, jq, and API-specific authentication. This has several problems:

- Users must know the exact curl syntax and API endpoints for each service
- Authentication is handled ad-hoc: API keys in env vars, bearer tokens in headers, OAuth flows via shell scripts
- Error handling is fragile — parsing JSON responses in bash to detect failures
- No discoverability — users must read API docs to know what operations are available
- No type safety — malformed requests fail at runtime with cryptic HTTP errors

n8n solves this with 400+ purpose-built nodes. Building equivalent plugins from scratch would take years. MCP provides a faster path: one protocol integration unlocks thousands of pre-built servers.

## 3. Goals and Non-Goals

### 3.1 Goals

1. **Add `mcp:` as a first-class step type** in YAML workflow definitions, on par with `cmd:` and `call:`
2. **Support stdio transport** (spawn MCP server as subprocess) as the primary integration path
3. **Integrate with the existing secrets store** for MCP server credentials (API keys, tokens)
4. **Provide tool discovery** via `workflow mcp list-tools <server>` CLI command
5. **Enable MCP server configuration** in `config.toml` for reusable server definitions with credentials
6. **Gate behind a cargo feature flag** (`mcp`) to keep the minimal binary small
7. **Preserve the single-binary philosophy** — MCP servers are external processes, not compiled in

### 3.2 Non-Goals

- Building an MCP server inside workflow (we are a client only)
- Supporting the full MCP resource/prompt protocol (tools only in v1)
- Implementing an MCP server registry or marketplace UI
- OAuth 2.1 flows in v1 (static credentials via secrets store; OAuth is a v2 enhancement)
- SSE/Streamable HTTP transport in v1 (stdio is sufficient for local and most remote use cases)

## 4. User Stories

### 4.1 As a sysadmin writing a deploy workflow

*I want to create a GitHub release, post to Slack, and update a Jira ticket as workflow steps — without writing curl commands for each API.*

### 4.2 As a DevOps engineer managing infrastructure

*I want to query my AWS resources, check Kubernetes cluster status, and run database operations as native workflow steps that integrate with the existing DAG execution engine, retries, and conditional branching.*

### 4.3 As a team lead standardizing runbooks

*I want to define MCP server credentials once in config.toml and have all workflows reference them by alias, so team members don't need to manage individual API keys.*

### 4.4 As a workflow author exploring available tools

*I want to run `workflow mcp list-tools github` to see all available operations (create_issue, list_pull_requests, etc.) with their parameter schemas, so I can write MCP steps without reading external docs.*

## 5. Technical Design

### 5.1 Architecture Overview

The MCP integration adds a thin client layer between the workflow executor and external MCP servers. The architecture follows the same subprocess model as the existing `cmd:` step type — the MCP server is spawned as a child process, communicates over stdin/stdout via JSON-RPC 2.0, and is terminated when the step completes.

```
  workflow executor
       │
       │ 1. Spawn MCP server process (stdio)
       │ 2. Send initialize request
       │ 3. Call list_tools (validation)
       │ 4. Call call_tool with args
       │ 5. Capture result as step output
       │ 6. Terminate process
       ▼
  MCP server (child process, stdio transport)
       │
       ▼
  External service (GitHub, Slack, AWS, ...)
```

### 5.2 Rust Crate Dependency

Use the official Rust MCP SDK (`rmcp`) maintained by the Model Context Protocol team:

```toml
# Cargo.toml
[dependencies]
rmcp = { version = "0.16", features = ["client", "transport-io"], optional = true }

[features]
mcp = ["dep:rmcp", "dep:tokio"]
```

The `rmcp` crate provides: `Client` for connection management, `TokioChildProcess` for stdio transport, `list_tools()` for discovery, and `call_tool()` for execution. It handles the full JSON-RPC 2.0 protocol, capability negotiation, and message framing.

### 5.3 YAML Step Syntax

A new `mcp:` field on the Step struct, mutually exclusive with `cmd:` and `call:`:

```yaml
steps:
  - id: create-release
    mcp:
      server: github              # alias from config.toml
      tool: create_release
      args:
        owner: myorg
        repo: myapp
        tag: "v{{version}}"       # template vars work
        body: "Release {{version}}"
    outputs:
      - name: release_url
        pattern: "html_url.*?(https://\\S+)"
```

Inline server definition (no config.toml alias):

```yaml
  - id: query-db
    mcp:
      server:
        command: npx -y @modelcontextprotocol/server-postgres
        env:
          DATABASE_URL: "postgres://user:$DB_PASSWORD@localhost/prod"
      tool: query
      args:
        sql: "SELECT count(*) FROM users WHERE created_at > now() - interval '1 day'"
```

### 5.4 Configuration (config.toml)

MCP servers are defined as named aliases in `config.toml`, allowing workflows to reference them by short name:

```toml
[mcp.servers.github]
command = "npx -y @modelcontextprotocol/server-github"
# Secrets auto-injected from workflow's encrypted secrets store
secrets = ["GITHUB_TOKEN"]             # injected as env vars

[mcp.servers.slack]
command = "npx -y @anthropic/mcp-server-slack"
secrets = ["SLACK_BOT_TOKEN"]

[mcp.servers.postgres]
command = "npx -y @modelcontextprotocol/server-postgres"
env = { DATABASE_URL = "postgres://app:$DB_PASSWORD@db/prod" }
secrets = ["DB_PASSWORD"]

[mcp.servers.filesystem]
command = "npx -y @modelcontextprotocol/server-filesystem /data"
# No secrets needed for local file access
```

Key design decisions:

- `secrets` references names from the existing age-encrypted secrets store — no new credential storage mechanism
- `env` supports `$VAR` expansion (same as notification URL schemes) for values that reference secrets or environment variables
- Server aliases are global — any workflow can reference them. Per-workflow overrides are possible via inline server definitions
- The `command` field supports any executable: `npx`, `uvx` (Python), native binaries, Docker containers

### 5.5 Credential Flow

The credential flow integrates with the existing secrets store and follows a clear precedence chain:

| # | Source | Description |
|---|--------|-------------|
| 1 | `mcp.env` | Explicit env vars in YAML or config.toml server definition |
| 2 | `mcp.secrets` | Names resolved from the age-encrypted secrets store, injected as env vars |
| 3 | Step `env:` | Step-level env block (same as cmd: steps) |
| 4 | Workflow `secrets:` | Workflow-level secrets declaration (injected before any step) |
| 5 | Host environment | Inherited from the shell environment (fallback) |

This matches the existing precedence for `cmd:` steps: explicit > secrets store > environment. Secret values are masked in logs and output, same as today.

### 5.6 Execution Flow

When the executor encounters an `mcp:` step:

1. **Resolve server** — look up alias in `config.toml` or use inline definition
2. **Inject credentials** — resolve secrets, expand `$VAR` references, build env map
3. **Spawn process** — start the MCP server as a child process with stdio transport via `rmcp::transport::TokioChildProcess`
4. **Initialize** — send MCP `initialize` request, negotiate capabilities
5. **Validate tool** — call `list_tools()`, verify the requested tool name exists, validate args against the tool's input schema (JSON Schema)
6. **Execute** — call `call_tool()` with the provided args. Capture the result (text content) as step stdout
7. **Output capture** — apply `outputs:` regex patterns to the result text, same as `cmd:` steps
8. **Cleanup** — terminate the child process. If the step has `timeout:`, enforce it the same way as `cmd:` steps (SIGTERM then SIGKILL)

The MCP step participates fully in the DAG execution engine: it respects `needs:`, `run_if`/`skip_if`, `retry`/`retry_delay`, `timeout`, `for_each`, and template variable expansion in all fields.

### 5.7 Async Runtime Consideration

The `rmcp` crate requires a tokio async runtime. The current workflow executor is synchronous (threads, not async). Two integration approaches:

| Approach | Pros | Cons |
|----------|------|------|
| **A: Block-on per step** | Minimal change. Wrap each MCP call in `tokio::runtime::Runtime::new().block_on()` | Creates/destroys a runtime per MCP step. Acceptable for subprocess-based servers. |
| **B: Shared runtime** | Create one tokio runtime at executor init, reuse for all MCP steps. Better performance. | Requires threading the runtime handle through the executor. Moderate refactor. |

**Recommendation:** Start with Approach A for simplicity. The subprocess spawn time dominates latency (100–500ms for npx), so runtime overhead is negligible. Move to Approach B if/when SSE transport is added (v2).

## 6. CLI Commands

### 6.1 Tool Discovery

```bash
workflow mcp list-tools <server-alias>
workflow mcp list-tools github
workflow mcp list-tools --server "npx -y @anthropic/mcp-server-slack"
```

Output: tool names, descriptions, and parameter schemas in a human-readable table. JSON output with `--json` flag for scripting.

### 6.2 Tool Execution (Ad-Hoc)

```bash
workflow mcp call <server-alias> <tool> [--arg key=value]...
workflow mcp call github list_pull_requests --arg repo=myorg/myapp --arg state=open
```

One-shot tool invocation from the CLI. Useful for testing MCP servers before writing workflows. Respects config.toml aliases and secrets.

### 6.3 Server Validation

```bash
workflow mcp check <server-alias>
```

Spawns the server, runs `initialize` + `list_tools`, reports success/failure and lists available tools. Useful for debugging connection issues and verifying credentials.

## 7. TUI Integration

- **MCP steps in details pane** — show `mcp: server/tool` instead of the command when viewing workflow steps
- **AI wizard awareness** — when generating workflows via AI (`a` key), include MCP server aliases in the system prompt so AI can generate `mcp:` steps for configured services
- **MCP server browser** (future, v2) — a modal (`M` key) showing configured servers, their tools, and connection status

## 8. Data Model Changes

### 8.1 Step Struct (models.rs)

Add to the existing `Step` struct:

```rust
pub struct McpStepConfig {
    pub server: McpServerRef,       // alias string or inline definition
    pub tool: String,               // tool name to call
    pub args: Option<serde_json::Value>,  // tool arguments (JSON object)
}

pub enum McpServerRef {
    Alias(String),                  // references [mcp.servers.<name>] in config
    Inline {
        command: String,
        env: Option<HashMap<String, String>>,
        secrets: Option<Vec<String>>,
    },
}
```

Step validation: exactly one of `cmd`, `call`, or `mcp` must be set. The parser enforces this at load time.

### 8.2 Config Struct (config.rs)

```rust
pub struct McpServerConfig {
    pub command: String,
    pub env: Option<HashMap<String, String>>,
    pub secrets: Option<Vec<String>>,
    pub timeout: Option<u64>,       // default server timeout in seconds
}
```

### 8.3 Run Log (db.rs)

The existing `RunLog` and `StepResult` structures require no changes. MCP tool output is captured as stdout text, and errors as stderr. The `step_type` field (if added) can distinguish `cmd`/`call`/`mcp` for analytics.

## 9. New and Modified Source Files

| File | Action | Description |
|------|--------|-------------|
| `src/core/mcp.rs` | **New** | MCP client: spawn, initialize, list_tools, call_tool, teardown |
| `src/cli/mcp.rs` | **New** | CLI subcommands: list-tools, call, check |
| `src/core/models.rs` | Modify | Add McpStepConfig, McpServerRef to Step |
| `src/core/config.rs` | Modify | Add McpServerConfig, [mcp.servers] parsing |
| `src/core/executor.rs` | Modify | Add MCP step execution branch alongside cmd/call |
| `src/core/parser.rs` | Modify | Validate mcp: steps, enforce mutual exclusion |
| `src/cli/args.rs` | Modify | Add Mcp subcommand (list-tools, call, check) |
| `src/tui/ui.rs` | Modify | Render mcp: steps in details pane |
| `Cargo.toml` | Modify | Add rmcp dependency, mcp feature flag |

## 10. Testing Strategy

### 10.1 Unit Tests

- **Config parsing** — test `[mcp.servers]` deserialization, secret name validation, env expansion
- **YAML parsing** — test `mcp:` step deserialization, mutual exclusion with cmd/call, arg validation
- **Server resolution** — test alias lookup, inline definition, missing server errors

### 10.2 Integration Tests

- **Echo server** — write a minimal MCP server in a shell script that echoes tool calls as JSON. Use it for end-to-end executor tests in CI without external dependencies
- **Filesystem server** — test with `@modelcontextprotocol/server-filesystem` in a tempdir (requires npx in CI)
- **Error cases** — server crash mid-call, timeout, unknown tool name, malformed args, missing credentials

### 10.3 Manual Testing

- GitHub server: list repos, create issue, list PRs
- Postgres server: run queries against a test database
- Slack server: post a message to a test channel

## 11. Rollout Plan

| Phase | Timeline | Deliverables |
|-------|----------|--------------|
| **Phase 1** | Week 1–2 | Core MCP client module (`src/core/mcp.rs`), `McpStepConfig` data model, config.toml parsing, basic executor integration. Unit tests. |
| **Phase 2** | Week 3 | CLI subcommands (`mcp list-tools`, `mcp call`, `mcp check`). Integration tests with echo server. Cargo feature flag gating. |
| **Phase 3** | Week 4 | Secrets integration, TUI rendering of MCP steps, AI wizard awareness. End-to-end tests with real MCP servers (GitHub, filesystem). |
| **Phase 4** | Week 5 | Documentation: README section, YAML reference examples, 3–5 example workflows using popular MCP servers. Release as v0.5.0. |

## 12. Future Enhancements (v2)

| Feature | Description | Priority |
|---------|-------------|----------|
| **OAuth 2.1 support** | `workflow mcp auth <server>` runs the OAuth flow, stores tokens in secrets store. Covers the 8.5% of MCP servers using OAuth. | High |
| **SSE transport** | Connect to remote MCP servers over HTTP (Streamable HTTP transport). Enables SaaS-hosted MCP servers without local npx. | High |
| **MCP server browser** | TUI modal (`M` key) listing configured servers, available tools, and connection status. Quick-add to workflow from browser. | Medium |
| **Server connection pooling** | Keep MCP server processes alive across steps within the same workflow run, reducing spawn overhead for multi-step workflows using the same server. | Medium |
| **Bundled MCP templates** | Pre-built workflow templates that use MCP steps: "GitHub release workflow," "Slack incident response," "Database backup with S3 upload." | Medium |
| **MCP resource support** | Read MCP resources (files, data) as step inputs. Enables workflows that react to resource changes. | Low |

## 13. Success Metrics

- **Adoption** — 10%+ of new workflows created use at least one `mcp:` step within 3 months of release
- **Server diversity** — community reports usage of 20+ distinct MCP servers within 6 months
- **GitHub engagement** — MCP-related issues/PRs account for 25%+ of repo activity, indicating community interest
- **Zero-config success rate** — users can run `workflow mcp list-tools <server>` and get results on first try 90%+ of the time
- **Build size impact** — the `mcp` feature flag adds < 2MB to the release binary

## 14. Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| **rmcp crate instability** | Breaking API changes in the official SDK | Pin to specific version. The crate follows semver. Abstract behind a thin wrapper in `core/mcp.rs` to isolate changes. |
| **MCP server quality varies** | Some community servers may be buggy or poorly maintained | Document recommended/tested servers. The `mcp check` command helps users validate before use. Timeouts and retries protect against hangs. |
| **npx startup latency** | npx-based servers take 1–3 seconds to start | Document this. Recommend global npm install for frequently used servers. Server connection pooling (v2) eliminates repeated startup. |
| **Tokio dependency weight** | Adds ~1–2MB to binary, increases compile time | Gated behind cargo feature flag. Users who don't need MCP compile without it. Tokio is already a transitive dep of rmcp only. |
| **Credential exposure** | Secrets passed as env vars to child processes | Same model as `cmd:` steps. Secrets are masked in logs. MCP servers run locally as user processes, not exposed to network. |

## 15. Open Questions

1. **Server lifecycle per step vs per workflow:** Should MCP server processes persist across steps within one workflow run? Saves spawn time but adds complexity. Recommendation: per-step in v1, pooling in v2.

2. **MCP result format:** MCP tool results contain `content[]` with `type: "text"` or `type: "image"`. Should we serialize the full JSON result as stdout, or extract text-only content? Recommendation: JSON for `--json` mode, text-only for default output and output capture patterns.

3. **Default feature flag:** Should `mcp` be included in default features? Pro: easier adoption. Con: adds tokio to every build. Recommendation: opt-in initially, default after v2 stabilization.

4. **npx vs global install guidance:** Should config.toml support a `global_install` hint that auto-runs `npm install -g` on first use? Recommendation: no, just document it. Auto-installing packages is a security concern.

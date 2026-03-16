# Changelog

## v0.4.0 — MCP Integration

### Added

- **MCP step type**: Native `mcp:` steps in workflow YAML files, enabling direct calls to 16,000+ MCP servers (GitHub, Slack, databases, cloud providers) without shell glue code.
- **MCP feature flag**: Opt-in via `cargo build --features mcp` or `cargo install workflow --features mcp`. Base binary stays small without MCP dependencies.
- **MCP server aliases**: Define reusable server configurations in `config.toml` under `[mcp.servers.<alias>]` with command, env, secrets, and timeout fields.
- **Inline MCP servers**: Specify server command directly in YAML for one-off usage without config.toml entry.
- **MCP CLI commands**:
  - `workflow mcp list-tools <server>` — discover available tools and parameter schemas
  - `workflow mcp call <server> <tool> --arg key=value` — ad-hoc tool invocation
  - `workflow mcp check <server>` — verify server connectivity and credentials
- **MCP credential injection**: Server secrets resolved from the encrypted secrets store and injected as environment variables.
- **Template variables in MCP args**: Full support for `{{var}}`, `{{date}}`, `{{hostname}}`, `{{step_id.output_name}}`, and `$ENV_VAR` expansion in MCP tool arguments.
- **DAG integration**: MCP steps support `needs:`, `run_if`/`skip_if`, `retry`/`retry_delay`, `timeout`, `for_each`, and output capture — same as `cmd:` steps.
- **TUI rendering**: MCP steps display as `mcp: server/tool` with args in the details pane, visually distinguished from shell commands.
- **AI wizard MCP awareness**: AI-generated workflows prefer `mcp:` steps over `curl` when matching MCP servers are configured.
- **3 bundled MCP templates**: `mcp/github-release`, `mcp/db-backup`, `mcp/filesystem-ops` available via `workflow templates`.
- **Echo MCP test server**: `tests/fixtures/echo_mcp_server.sh` for integration testing without external dependencies.

### Technical Details

- New modules: `src/core/mcp.rs` (MCP client), `src/cli/mcp.rs` (CLI handlers)
- Dependencies (optional, behind `mcp` feature): `rmcp 0.16` (client, transport-io, transport-child-process), `tokio 1` (rt-multi-thread, process, macros)
- `McpClient` wraps rmcp with stdio transport, synchronous API via `tokio::runtime::Runtime::block_on()`
- `McpStepConfig`, `McpServerRef` data models in `models.rs` (always compiled for config parsing)
- `McpServerConfig` in `config.rs` for `[mcp.servers.*]` TOML sections
- Strict secret loading (`load_secret_env_strict`) for MCP — fails loudly on missing secrets

### Notes

- The `mcp` feature is **not** included in default features — opt-in only
- Without the `mcp` feature, `cargo build` produces the same binary as before (no regression)
- MCP steps in YAML files produce a clear error message when the feature is not enabled

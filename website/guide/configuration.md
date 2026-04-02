# Configuration

## Directory structure

```
~/.config/workflow/
├── backup/                  # Category (folder name)
│   ├── db-full.sh           # Bash script task
│   └── mysql-daily.yaml     # Multi-step YAML workflow
├── deploy/
│   └── staging.yaml
├── logs/                    # Auto-generated (JSON per run)
├── history.db               # Auto-generated (SQLite)
├── secrets.age              # Auto-generated (encrypted secrets)
├── config.toml              # Optional global configuration
└── config.local.toml        # Optional machine-specific overrides
```

Folders are categories. `.sh` files are bash tasks. `.yaml` files are multi-step workflows. `logs/`, `history.db`, and `secrets.age` are managed automatically.

## config.toml reference

All settings have sensible defaults. The config file is entirely optional.

```toml
workflows_dir = "/home/user/.config/workflow"
log_retention_days = 30
editor = "vim"
default_timeout = 600

[hooks]
pre_run = "echo 'starting'"
post_run = "echo 'done'"

[notify]
on_failure = "slack://https://hooks.slack.com/services/..."

[server]
port = 8080
max_concurrent_runs = 4

bookmarks = ["backup/db-full", "deploy/staging"]
```

### Top-level fields

| Field | Default | Description |
|-------|---------|-------------|
| `workflows_dir` | `~/.config/workflow` | Root directory for workflow files |
| `log_retention_days` | `30` | Days to keep JSON run logs before auto-rotation |
| `editor` | `$EDITOR` or `vi` | Editor launched by `e` in the TUI |
| `default_timeout` | none | Default timeout in seconds for steps without an explicit timeout |
| `bookmarks` | `[]` | List of task references to bookmark |

### Hooks

The `[hooks]` section defines shell commands that run before and after every task execution:

```toml
[hooks]
pre_run = "echo 'starting'"
post_run = "echo 'done'"
```

Both `pre_run` and `post_run` are optional. They execute via `bash -c` and have access to environment variables from the workflow.

### Notifications

The `[notify]` section sets global notification defaults. Per-workflow `notify:` blocks merge with these defaults. Set `notify_override: true` in a workflow to replace global settings instead of merging.

```toml
[notify]
on_failure = "slack://https://hooks.slack.com/services/T00/B00/xxx"
on_success = "webhook://https://status.example.com/api/deploy"
```

See the notification URL scheme reference in the [workflows](/guide/workflows) page for the full list of supported services.

## Machine-specific overrides

Create `config.local.toml` alongside `config.toml` for machine-specific settings. It merges on top of the base configuration without affecting the shared file --- useful when syncing config across machines via Git.

```toml
# config.local.toml — not synced
editor = "nvim"
workflows_dir = "/opt/workflows"
```

## MCP server configuration

Define MCP server aliases so workflows can reference them by short name. Requires building with `--features mcp`.

::: code-group

```toml [Stdio transport]
[mcp.servers.github]
command = "npx -y @modelcontextprotocol/server-github"
secrets = ["GITHUB_TOKEN"]

[mcp.servers.slack]
command = "npx -y @modelcontextprotocol/server-slack"
secrets = ["SLACK_BOT_TOKEN"]
env = { SLACK_TEAM_ID = "T0123456" }
```

```toml [HTTP transport]
[mcp.servers.cpanel-whm]
url = "https://myserver.example.com:2087/mcp"
auth_header = "whm root:APITOKEN"
timeout = 60
```

:::

Stdio transport fields: `command` (required), `secrets`, `env`, `timeout`. HTTP transport fields: `url` (required), `auth_header`, `headers`, `timeout`.

Credentials listed in `secrets` are resolved from the encrypted secrets store and injected as environment variables at runtime. No plaintext tokens in config files.

::: warning
Store sensitive values in the encrypted secrets store (`workflow secrets set GITHUB_TOKEN`), not directly in config.toml. The `secrets` field references secret names, not values.
:::

## Sync configuration

Enable Git-based syncing of workflow definitions across machines.

```toml
[sync]
enabled = true
auto_commit = true
auto_push = true
auto_pull_on_start = true
```

| Field | Default | Description |
|-------|---------|-------------|
| `enabled` | `false` | Enable git sync |
| `auto_commit` | `false` | Auto-commit changes when files are modified |
| `auto_push` | `false` | Auto-push after commit |
| `auto_pull_on_start` | `false` | Pull latest on TUI startup |
| `branch` | `main` | Git branch to use |
| `remote_url` | none | Remote repository URL |

One-time setup: `workflow sync setup` creates a private GitHub repo and enables auto-sync. Press `G` in the TUI for manual sync controls and branch switching.

## CLI overrides

### --dir flag

The `--dir` flag overrides `workflows_dir` for a single invocation:

```bash
workflow --dir /opt/shared-workflows list
workflow --dir ./local-workflows run ci/build
```

### Task references

Tasks can be referenced in two equivalent formats:

- Slash notation: `backup/db-full`
- Dot notation: `backup.db-full`

Dot notation is normalized to slash internally. Both work in CLI commands, `call:` steps, `needs:` references, and bookmarks.

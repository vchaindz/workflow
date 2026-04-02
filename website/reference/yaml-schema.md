# YAML Workflow Schema

This is the complete field reference for `.yaml` workflow files. Workflows live in `~/.config/workflow/<category>/` and are discovered automatically.

## Top-Level Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | No | Display name for the workflow. Shown in the TUI and `list` output. |
| `overdue` | integer | No | Number of days. If the task has not run successfully within this window, the TUI shows a reminder on startup. |
| `steps` | array | Yes | Ordered list of [step definitions](#step-fields). Execution order is determined by `needs:` dependencies via topological sort. |
| `cleanup` | array | No | Steps that run after the main step loop regardless of success or failure. Cleanup failures are logged but do not affect the workflow exit code. |
| `env` | map | No | Environment variables available to all steps. Values are auto-redacted in logs. |
| `notify` | object | No | [Notification configuration](#notify-configuration). Merged with global `config.toml` settings by default. |
| `secrets` | array | No | Names of secrets to inject from the encrypted secrets store. Each name is resolved to an environment variable. |
| `variables` | array | No | [Runtime variable prompts](#variable-prompts). The TUI and template system prompt for values before execution. |

---

## Step Fields

Each entry in `steps:` or `cleanup:` is an object with the following fields.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `id` | string | *required* | Unique identifier for the step. Referenced by `needs:` and output capture. |
| `cmd` | string | -- | Shell command executed via `bash -c`. Mutually exclusive with `call` and `mcp`. |
| `call` | string | -- | Task reference (e.g., `backup/db-full`) to invoke as a sub-workflow. Mutually exclusive with `cmd` and `mcp`. |
| `mcp` | object | -- | [MCP tool call](#mcp-step-fields). Mutually exclusive with `cmd` and `call`. Requires the `mcp` feature. |
| `needs` | array | `[]` | Step IDs this step depends on. The step runs only after all dependencies complete successfully. |
| `timeout` | integer | -- | Maximum execution time in seconds. Overrides any global or CLI timeout. |
| `retry` | integer | `0` | Number of retry attempts on failure. |
| `retry_delay` | integer | `0` | Seconds to wait between retries. |
| `run_if` | string | -- | Shell condition evaluated via `bash -c`. The step runs only if the condition exits 0. |
| `skip_if` | string | -- | Shell condition evaluated via `bash -c`. The step is skipped if the condition exits 0. |
| `outputs` | array | -- | [Output capture patterns](#output-capture). |
| `for_each` | object | -- | [Loop configuration](#for_each-configuration). Runs the step once per item. |
| `for_each_parallel` | boolean | `false` | When `true`, loop iterations run concurrently. |
| `for_each_continue_on_error` | boolean | `false` | When `true`, loop continues even if an iteration fails. |
| `interactive` | boolean | `false` | Force interactive mode (inherited stdio). Used for REPLs, TUI tools, and streaming commands. Auto-detected in most cases. |
| `env` | map | -- | Step-specific environment variables. Merged with top-level `env`. |

---

## MCP Step Fields

The `mcp:` object on a step configures a Model Context Protocol tool call.

| Field | Type | Description |
|-------|------|-------------|
| `server` | string or object | Server alias (defined in `config.toml` under `[mcp.servers.<alias>]`) or an [inline server definition](#inline-server-definition). |
| `tool` | string | Name of the tool to call on the server. |
| `args` | map | Arguments passed to the tool. Template variables are expanded. |

### Inline Server Definition

When using an inline object for `server:` instead of an alias string:

| Field | Type | Description |
|-------|------|-------------|
| `command` | string | Shell command to spawn a stdio-based MCP server. |
| `url` | string | HTTP endpoint URL for an HTTP-based MCP server. |
| `env` | map | Environment variables passed to the server process. |
| `secrets` | array | Secret names to inject from the encrypted store. |
| `auth_header` | string | Authorization header value (HTTP transport only). |
| `timeout` | integer | Server connection timeout in seconds. |

---

## Output Capture

Each item in a step's `outputs:` array defines a regex-based capture pattern applied to the step's stdout after successful execution.

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Variable name for the captured value. |
| `pattern` | string | Regular expression with at least one capture group. The first group match is stored. |

Captured values are referenced in subsequent steps as `{{step_id.name}}` using the template variable syntax.

---

## for_each Configuration

The `for_each:` object configures iteration. The step command is executed once per item, with `{{item}}` available as a template variable.

| Field | Type | Description |
|-------|------|-------------|
| `source` | string | `list` for a static array or `command` for dynamic generation. |
| `items` | array | Static list of values. Used when `source` is `list`. |
| `command` | string | Shell command whose stdout lines become items. Used when `source` is `command`. |

---

## Notify Configuration

The `notify:` object controls post-run notifications. Global notification settings from `config.toml` are merged in by default.

| Field | Type | Description |
|-------|------|-------------|
| `on_failure` | string or array | Notification target URLs triggered on workflow failure. |
| `on_success` | string or array | Notification target URLs triggered on workflow success. |
| `channels` | array | [Severity-based routing rules](#channel-routing). |
| `env` | map | Extra variables available in notification templates. |
| `notify_override` | boolean | When `true`, replaces global config instead of merging with it. |

Notification targets use URL schemes: `slack://`, `discord://`, `telegram://`, `teams://`, `ntfy://`, `gotify://`, `webhook://`, `email://`, `mattermost://`. Environment variable references (`$VAR`) in URLs are expanded.

### Channel Routing

Each item in `channels:` routes notifications by severity.

| Field | Type | Description |
|-------|------|-------------|
| `target` | string | Notification URL scheme. |
| `on` | array | Severity filters: `success`, `failure`, `warning`. |

---

## Variable Prompts

Each item in `variables:` defines a runtime variable that is prompted before execution (in TUI or template mode).

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Variable name, referenced as `{{name}}` in steps. |
| `description` | string | Prompt text displayed to the user. |
| `default` | string | Default value if the user provides no input. |
| `choices_cmd` | string | Shell command whose stdout lines become selectable choices. |

---

## Built-in Template Variables

These variables are available in all `cmd:` values and `mcp:` args without declaration:

| Variable | Description |
|----------|-------------|
| `{{date}}` | Current date (`YYYY-MM-DD`) |
| `{{datetime}}` | Current date and time |
| `{{hostname}}` | Machine hostname |
| `{{step_id.name}}` | Captured output from a previous step |
| `{{item}}` | Current item in a `for_each` loop |

Environment variables and custom `env:` values are also expanded.

---

## Complete Example

The following workflow demonstrates most schema features in a single file.

```yaml
name: Database Backup with Verification
overdue: 1

env:
  BACKUP_DIR: /var/backups/db
  RETENTION_DAYS: "30"

secrets:
  - DATABASE_URL
  - SLACK_WEBHOOK

variables:
  - name: target_db
    description: Database name to back up
    default: production
  - name: compression
    description: Compression algorithm
    default: zstd
    choices_cmd: "echo -e 'gzip\nzstd\nlz4'"

steps:
  - id: prepare
    cmd: "mkdir -p $BACKUP_DIR"

  - id: dump
    needs: [prepare]
    cmd: "pg_dump {{target_db}} | {{compression}} > $BACKUP_DIR/{{target_db}}-{{date}}.sql.{{compression}}"
    timeout: 3600
    retry: 2
    retry_delay: 30
    outputs:
      - name: backup_file
        pattern: "Backup saved to (.+)"

  - id: verify
    needs: [dump]
    cmd: "{{compression}} -t $BACKUP_DIR/{{target_db}}-{{date}}.sql.{{compression}} && echo 'Integrity check passed'"

  - id: check-sizes
    needs: [dump]
    mcp:
      server:
        command: "npx @anthropic/mcp-server-postgres $DATABASE_URL"
        secrets: [DATABASE_URL]
      tool: query
      args:
        sql: "SELECT pg_database_size('{{target_db}}') as db_size"
    outputs:
      - name: db_size
        pattern: "db_size.*?(\\d+)"

  - id: rotate
    needs: [verify]
    cmd: "find $BACKUP_DIR -name '*.sql.*' -mtime +$RETENTION_DAYS -delete"
    skip_if: "test $RETENTION_DAYS -eq 0"

  - id: notify-each-admin
    needs: [verify]
    cmd: "echo 'Notifying {{item}} about backup completion'"
    for_each:
      source: list
      items: [ops-lead, dba-team, sre-oncall]

  - id: report
    needs: [verify, check-sizes]
    cmd: |
      echo "Backup complete: {{target_db}}"
      echo "Database size: {{check-sizes.db_size}} bytes"
      echo "Backup file: $BACKUP_DIR/{{target_db}}-{{date}}.sql.{{compression}}"
    run_if: "test -f $BACKUP_DIR/{{target_db}}-{{date}}.sql.{{compression}}"

cleanup:
  - id: cleanup-tmp
    cmd: "rm -f /tmp/db-backup-*.tmp"

notify:
  on_failure:
    - "slack://$SLACK_WEBHOOK"
  on_success:
    - "ntfy://ntfy.sh/my-backups"
  channels:
    - target: "slack://$SLACK_WEBHOOK"
      on: [failure, warning]
    - target: "ntfy://ntfy.sh/my-backups"
      on: [success]
```

# Workflows

## File structure

The workflow library lives in `~/.config/workflow/` (configurable via `config.toml` or `--dir`). The organizational model is simple:

- **Folders** are categories
- **`.sh` files** are bash scripts, executed directly
- **`.yaml` files** are multi-step DAG workflows

Tasks are referenced as `category/task` (e.g., `backup/db-full`). Dot notation also works: `backup.db-full` is normalized to `backup/db-full`.

## Step execution model

YAML workflows declare a `steps:` list. Each step has an `id` and one of three action types:

- `cmd:` --- run a shell command via `bash -c`
- `call:` --- invoke another workflow by reference (sub-workflow)
- `mcp:` --- call an MCP tool (requires `--features mcp`)

Steps are topologically sorted based on `needs:` dependencies and executed in order. If a step fails, its dependents are skipped --- but independent branches continue running.

## Step fields

| Field | Type | Description |
|-------|------|-------------|
| `id` | string | Unique step identifier (required) |
| `cmd` | string | Shell command to execute |
| `call` | string | Task reference for sub-workflow invocation |
| `mcp` | object | MCP tool call (server, tool, args) |
| `needs` | list | Step IDs this step depends on |
| `timeout` | integer | Max execution time in seconds |
| `retry` | integer | Number of retry attempts on failure |
| `retry_delay` | integer | Seconds between retries |
| `run_if` | string | Shell condition; step runs only if exit 0 |
| `skip_if` | string | Shell condition; step skipped if exit 0 |
| `outputs` | list | Regex patterns to capture output variables |
| `for_each` | object | Loop over a list or command output |
| `interactive` | boolean | Force inherited stdio (for REPLs, TUI tools) |

## Sub-workflows with `call:`

Compose complex automation by calling workflows from other workflows. Use the `call:` field instead of `cmd:` to invoke any task by reference:

```yaml
name: Full Deploy Pipeline
steps:
  - id: pre-checks
    call: monitoring/health-check
  - id: backup
    call: backup/db-full
    needs: [pre-checks]
  - id: deploy
    call: deploy/rolling-update
    needs: [backup]
  - id: smoke
    call: monitoring/smoke-test
    needs: [deploy]
  - id: rollback
    call: deploy/rollback
    run_if: "test '{{smoke.status}}' = 'failed'"
```

Sub-workflows execute recursively with a depth limit (max 10) to prevent cycles. Each inherits the parent's environment and template variables. This is how you build runbooks that orchestrate other runbooks.

## Step-level branching

After each step completes, `{{step_id.status}}` is automatically set to `success`, `failed`, `skipped`, or `timedout`. Use this in `run_if` or `skip_if` to branch on outcomes:

```yaml
steps:
  - id: deploy
    cmd: ./deploy.sh
  - id: rollback
    run_if: "test '{{deploy.status}}' = 'failed'"
    cmd: ./rollback.sh
  - id: smoke-test
    skip_if: "test '{{deploy.status}}' = 'failed'"
    cmd: ./smoke-test.sh
```

`run_if` runs the step only when the condition succeeds (exit 0). `skip_if` is the inverse --- it skips when the condition succeeds. Both support full template expansion.

## Step output capture

Steps can capture output via regex patterns and pass values to downstream steps:

```yaml
steps:
  - id: get-version
    cmd: cat VERSION
    outputs:
      - name: ver
        pattern: "^(\\S+)"
  - id: tag
    cmd: git tag v{{get-version.ver}}
    needs: [get-version]
```

Each `outputs:` entry has a `name` and a `pattern` (regex). The first capture group match is stored as `{{step_id.output_name}}` for use in subsequent steps.

## Cleanup steps

The `cleanup:` section declares steps that run after the main step loop regardless of success or failure, like a `finally` block:

```yaml
steps:
  - id: deploy
    cmd: ./deploy.sh
cleanup:
  - id: unlock
    cmd: rm -f /tmp/deploy.lock
```

Cleanup failures are logged but do not affect the overall workflow exit code.

## Loops with `for_each`

Iterate over static lists, template variable references, or dynamic command output. Each iteration receives `{{item}}` as a template variable:

```yaml
steps:
  - id: backup-all
    cmd: pg_dump {{item}} > /tmp/{{item}}_backup.sql
    for_each:
      source: list
      items: [users_db, orders_db, analytics_db]
    for_each_parallel: true
    for_each_continue_on_error: true
```

Dynamic lists from command output:

```yaml
steps:
  - id: restart-unhealthy
    cmd: docker restart {{item}}
    for_each:
      source: command
      command: "docker ps --filter health=unhealthy --format '{{.Names}}'"
```

## Expression filters

Template variables support pipe filters for in-line transformation:

```yaml
cmd: echo "Host: {{hostname | upper}}, DB: {{db_name | default 'mydb'}}"
```

Available filters: `upper`, `lower`, `trim`, `default`, `replace`, `truncate`, `split`, `first`, `last`, `nth`, `count`.

Ternary expressions: `{{var | eq "prod" ? "production" : "staging"}}`.

Date offsets: `{{date_offset +7d}}`, `{{date_offset -1w}}`.

Docker and Go template syntax (e.g., `{{.Names}}`) is passed through untouched.

## Runtime variable prompting

Workflows can declare variables with descriptions, defaults, and dynamic choices. The TUI prompts for values before execution:

```yaml
name: Database Restore
variables:
  - name: db_name
    description: "Target database"
    default: "mydb"
  - name: backup_file
    description: "Backup to restore"
    choices_cmd: "ls /backups/*.sql.gz"
steps:
  - id: restore
    cmd: zcat {{backup_file}} | psql {{db_name}}
```

## Interactive commands

Commands that need inherited stdio --- REPLs, TUI tools, streaming commands like `journalctl -f` --- are auto-detected by heuristic analysis. Shell quotes are stripped before detection so commands like `ssh host 'journalctl -f'` are properly recognized.

Detected commands run with the terminal suspended (TUI restores on exit). You can also mark steps explicitly:

```yaml
steps:
  - id: debug
    cmd: psql mydb
    interactive: true
```

## Environment variables

The `env:` block sets environment variables for all steps. Values are automatically redacted in live output and logs:

```yaml
env:
  AWS_PROFILE: prod
  DEPLOY_ENV: production
```

## Dangerous command detection

workflow scans expanded commands for destructive patterns before execution: `rm -rf /`, `dd` to block devices, `mkfs` on real devices, fork bombs, `chmod -R 777 /`, and others. Matched commands are blocked with an explanatory message.

::: warning
Override dangerous command detection with `--force` only when you are certain the command is safe. The check exists to prevent accidental damage from copy-paste errors and template expansion bugs.
:::

## Built-in template variables

These variables are available in all commands:

| Variable | Value |
|----------|-------|
| `{{date}}` | Current date (YYYY-MM-DD) |
| `{{datetime}}` | Current date and time |
| `{{hostname}}` | Machine hostname |
| `{{task_ref}}` | Current task identity (category/name) |
| `{{step_id.status}}` | Step outcome: success, failed, skipped, timedout |
| `{{step_id.output_name}}` | Captured output from a previous step |

## Complete example

```yaml
name: Deploy with Rollback
overdue: 1
steps:
  - id: check-health
    cmd: curl -sf http://localhost/health
    timeout: 10
  - id: deploy
    cmd: ./deploy.sh {{version}}
    needs: [check-health]
    retry: 2
    retry_delay: 5
    run_if: "test -f deploy.sh"
  - id: rollback
    cmd: ./rollback.sh
    run_if: "test '{{deploy.status}}' = 'failed'"
  - id: smoke-test
    cmd: ./smoke-test.sh
    skip_if: "test '{{deploy.status}}' = 'failed'"
  - id: get-version
    cmd: cat VERSION
    outputs:
      - name: ver
        pattern: "^(\\S+)"
  - id: tag
    cmd: git tag v{{get-version.ver}}
    needs: [get-version]
cleanup:
  - id: unlock
    cmd: rm -f /tmp/deploy.lock
env:
  DEPLOY_ENV: production
notify:
  on_failure:
    - "slack://https://hooks.slack.com/services/T00/B00/xxx"
    - "ntfy://ntfy.sh/ops-alerts"
  on_success: "webhook://https://status.example.com/api/deploy"
```

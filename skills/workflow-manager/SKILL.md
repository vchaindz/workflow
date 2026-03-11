---
name: workflow-manager
description: "Manage workflows for the 'workflow' CLI/TUI app — a file-based workflow orchestrator. Use when Claude needs to: (1) Create new workflow YAML files or shell scripts in ~/.config/workflow/, (2) Edit or debug existing workflows, (3) List, run, validate, or inspect workflow tasks via the CLI, (4) Work with DAG step dependencies, environment variables, retries, timeouts, or conditional execution, (5) Browse or fetch workflow templates, (6) View run logs or compare runs, (7) Export/import workflow archives. Triggers on mentions of 'workflow', 'task', 'workflow YAML', workflow categories, or the workflow CLI."
---

# Workflow Manager

Manage tasks for the `workflow` orchestrator. Workflows live in `~/.config/workflow/` where subdirectories are categories and files are tasks (`.yaml` for multi-step DAG workflows, `.sh` for shell scripts).

The binary is at `/home/dennis/github/workflow/target/release/workflow` (or run via `cargo run --` from the repo).

## Creating a YAML Workflow

1. Choose or create a category directory under `~/.config/workflow/<category>/`
2. Create a `.yaml` file with the required `name` and `steps` fields
3. Validate with `workflow validate <category/task>`

Minimal example:

```yaml
name: Deploy staging
overdue: 7
steps:
  - "docker compose pull"
  - "docker compose up -d"
  - "curl -sf http://localhost:8080/health"
```

With DAG dependencies and advanced features:

```yaml
name: Full deploy
overdue: 1
env:
  TAG:
    cmd: "git describe --tags --always"
steps:
  - id: build
    cmd: "docker build -t app:$TAG ."
    timeout: 300

  - id: test
    cmd: "docker run --rm app:$TAG pytest"
    needs: [build]
    retry: 2
    retry_delay: 5

  - id: push
    cmd: "docker push registry/app:$TAG"
    needs: [test]

  - id: deploy
    cmd: "kubectl set image deploy/app app=registry/app:$TAG"
    needs: [push]
    run_if: "test $DEPLOY_ENV = production"

notify:
  on_failure: "echo 'Deploy failed' | mail -s 'Alert' ops@example.com"
```

See [references/api_reference.md](references/api_reference.md) for the complete YAML schema, all step fields, CLI commands, and directory layout.

## Creating a Shell Script Task

Place an executable `.sh` file in a category directory:

```bash
#!/usr/bin/env bash
set -euo pipefail
# ~/.config/workflow/backup/db-snapshot.sh
pg_dump mydb | gzip > "/backups/db-$(date +%F).sql.gz"
```

## Common Operations

```bash
# List all workflows
workflow list

# Dry-run (preview commands without executing)
workflow run backup/db-snapshot --dry-run

# Run with env override
workflow run deploy/staging --env TAG=v1.2.3

# Run in background
workflow run monitoring/health-check --background

# View recent logs
workflow logs backup/db-snapshot --limit 5

# Compare last two runs
workflow compare deploy/staging

# Validate all workflows
workflow validate

# Browse templates
workflow templates
workflow templates --fetch   # fetch from GitHub
```

## Key Rules

- `name` and `steps` are required in YAML workflows
- Steps without explicit `id` get auto-assigned `step-N` and are chained sequentially
- Steps with explicit `id` and `needs` form a DAG — no implicit chaining
- Cycles in `needs` are rejected at parse time
- `env` values can be static strings or `{cmd: "..."}` for dynamic resolution
- Template vars `{{date}}`, `{{datetime}}`, `{{hostname}}` expand in commands
- `run_if` executes as `bash -c`; step runs only on exit code 0
- `secrets` lists env vars that must be set before execution
- `overdue` sets an expected run frequency in days; the TUI shows a reminder popup on startup for overdue tasks
- The `--dir` flag overrides the default `~/.config/workflow/` root

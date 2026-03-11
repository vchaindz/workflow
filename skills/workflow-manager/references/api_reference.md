# Workflow YAML Schema Reference

## Full Workflow YAML Structure

```yaml
name: "Human-readable workflow name"

env:
  STATIC_VAR: "literal value"
  DYNAMIC_VAR:
    cmd: "hostname -f"   # executed at parse time

workdir: /tmp/work          # optional working directory

secrets:                    # validated before run
  - API_KEY
  - DB_PASSWORD

notify:
  on_failure: "curl -X POST https://hooks.example.com/fail"
  on_success: "curl -X POST https://hooks.example.com/ok"

steps:
  # Format 1: Bare string (auto-chained sequentially)
  - "echo hello"

  # Format 2: Map without id (auto-chained, with options)
  - cmd: "apt update"
    parallel: false
    timeout: 120
    run_if: "test -f /etc/debian_version"
    retry: 3
    retry_delay: 5

  # Format 3: Full map with explicit id and DAG dependencies
  - id: fetch-data
    cmd: "curl -o data.json https://api.example.com/data"
    timeout: 30

  - id: process
    cmd: "python3 process.py data.json"
    needs: [fetch-data]

  - id: notify
    cmd: "echo done"
    needs: [process]
```

## Step Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| id | string | `step-N` | Unique step identifier |
| cmd | string | required | Shell command (`bash -c`) |
| needs | string[] | `[]` | DAG dependencies |
| parallel | bool | `false` | Parallel execution hint |
| timeout | u64 | none | Max seconds |
| run_if | string | none | Condition (exit 0 = run) |
| retry | u32 | none | Retry attempts |
| retry_delay | u64 | none | Seconds between retries |

## Template Variables

In `cmd` and `env` values: `{{date}}`, `{{datetime}}`, `{{hostname}}`, plus custom env vars.

## CLI Commands

```
workflow list [--json]
workflow run <cat/task> [--dry-run] [--env KEY=VAL] [--timeout N] [--background]
workflow status <cat/task> [--json]
workflow validate [<cat/task>] [--json]
workflow logs [<task>] [--json] [--limit N]
workflow compare <task> [--run ID] [--with ID] [--ai] [--json]
workflow export [-o file] [--include-history]
workflow import <archive> [--overwrite|--skip-existing]
workflow templates [--fetch] [--json]
```

## Directory Layout

```
~/.config/workflow/           # Default root (--dir overrides)
├── config.toml               # Optional config
├── history.db                # SQLite run history
├── logs/                     # JSON run logs (auto-rotated)
├── <category>/               # Category = subdirectory
│   ├── <task>.yaml           # YAML workflow
│   └── <task>.sh             # Shell script task
```

Task references: `category/task` or `category.task` (dot normalized to slash).

## DAG Rules

- Steps with `needs` run after dependencies complete
- Failed steps cause dependents to be skipped; independent branches continue
- Cycles detected at parse time (Kahn's algorithm)

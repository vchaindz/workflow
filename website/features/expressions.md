# Expressions and Templates

Template variables and pipe filters provide in-line transformation of values inside workflow steps. No shell gymnastics needed.

## Built-in variables

These variables are available in all `cmd:`, `mcp:`, `run_if`, and `skip_if` fields:

| Variable | Example value | Description |
|----------|---------------|-------------|
| `{{date}}` | `2026-04-02` | Current date (YYYY-MM-DD) |
| `{{datetime}}` | `2026-04-02T14:30:00` | Current date and time |
| `{{hostname}}` | `web-01` | Machine hostname |
| `{{task_ref}}` | `backup/db-full` | Current task identity |
| `{{step_id.status}}` | `success` | Step outcome (`success`, `failed`, `skipped`, `timedout`) |
| `{{step_id.var}}` | *(captured value)* | Output captured from a previous step |

Step status variables are set automatically after each step completes. Captured outputs require an `outputs:` block on the source step (see [Workflows & YAML](/guide/workflows)).

## Pipe filters

Chain filters after a variable using the `|` character:

```yaml
cmd: echo "Host: {{hostname | upper}}, DB: {{db_name | default 'mydb'}}"
```

### Available filters

| Filter | Example | Result |
|--------|---------|--------|
| `upper` | `{{hostname \| upper}}` | `MYSERVER` |
| `lower` | `{{name \| lower}}` | `myname` |
| `trim` | `{{input \| trim}}` | Strips leading/trailing whitespace |
| `default` | `{{db \| default 'mydb'}}` | Fallback if variable is empty |
| `replace` | `{{path \| replace '/' '-'}}` | Character replacement |
| `truncate` | `{{log \| truncate 80}}` | Limit string length |
| `split` | `{{csv \| split ','}}` | Split string into array |
| `first` | `{{list \| first}}` | First element of array |
| `last` | `{{list \| last}}` | Last element of array |
| `nth` | `{{list \| nth 2}}` | Nth element of array (zero-indexed) |
| `count` | `{{list \| count}}` | Number of elements in array |

## Ternary expressions

Compare a variable and branch on the result:

```yaml
cmd: echo "Deploying to {{env | eq 'prod' ? 'production' : 'staging'}}"
```

This evaluates the `env` variable: if it equals `prod`, the expression resolves to `production`; otherwise `staging`.

## Date offsets

Generate dates relative to today:

| Expression | Description |
|------------|-------------|
| `{{date_offset +7d}}` | 7 days from now |
| `{{date_offset -1w}}` | 1 week ago |
| `{{date_offset -1d}}` | Yesterday |

Date offsets are useful for log rotation, retention windows, and scheduled checks.

## Docker and Go template passthrough

Syntax like `{{.Names}}` or `{{.Status}}` is left untouched by the template engine. This means Docker and Go template commands work without escaping:

```yaml
cmd: docker ps --format "table {{.Names}}\t{{.Status}}"
```

::: tip
The engine distinguishes workflow variables (lowercase, underscores) from Docker/Go templates (leading dot) automatically. No special quoting is needed.
:::

## for_each loops

Iterate over a list of values, running the step once per item. Each iteration receives `{{item}}` as a template variable.

### Static list

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

### Dynamic list from command output

```yaml
steps:
  - id: restart-unhealthy
    cmd: docker restart {{item}}
    for_each:
      source: command
      command: "docker ps --filter health=unhealthy --format '{{.Names}}'"
```

Each line of the command's stdout becomes one item.

### Options

| Field | Default | Description |
|-------|---------|-------------|
| `for_each_parallel` | `false` | Run iterations concurrently instead of sequentially |
| `for_each_continue_on_error` | `false` | Continue remaining iterations if one fails |

::: warning
When using `for_each_parallel: true`, ensure the iterated command is safe to run concurrently. Shared resources (files, ports) may cause conflicts.
:::

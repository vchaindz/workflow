# CLI Reference

The `workflow` binary operates in two modes. Without a subcommand it launches the interactive TUI. With a subcommand it runs non-interactively, suitable for cron jobs, scripts, and CI pipelines.

Task references use `category/task` format throughout. Dot notation (`category.task`) is also accepted and normalized to slash form.

## Global Options

| Flag | Description |
|------|-------------|
| `--dir <PATH>` | Override the workflows directory (default: `~/.config/workflow/`) |
| `--no-tui` | Disable TUI mode; exit with error if no subcommand is given |

---

## run

Run a workflow or shell script task.

**Synopsis**

```
workflow run <task> [options]
```

**Flags**

| Flag | Description |
|------|-------------|
| `--dry-run` | Preview expanded commands without executing |
| `--env KEY=VALUE` | Set environment variables; repeatable |
| `--timeout <SECS>` | Override default step timeout (0 disables timeout) |
| `--background` | Detach from terminal and run in background |
| `--force` | Bypass dangerous command safety checks |

**Examples**

```bash
# Dry-run a backup task
workflow run backup/db-full --dry-run

# Run with custom variables and a 5-minute timeout
workflow run deploy/staging --env VERSION=2.1.0 --env REGION=us-east --timeout 300

# Force-run a task that triggers dangerous command detection
workflow run cleanup/purge-old --force
```

---

## list

List all discovered workflows and tasks.

**Synopsis**

```
workflow list [options]
```

**Flags**

| Flag | Description |
|------|-------------|
| `--json` | Output as JSON |

**Examples**

```bash
# Human-readable list
workflow list

# Machine-readable for scripting
workflow list --json | jq '.[] | .category'
```

---

## status

Show run status and history for a specific task.

**Synopsis**

```
workflow status <task> [options]
```

**Flags**

| Flag | Description |
|------|-------------|
| `--json` | Output as JSON |

**Examples**

```bash
workflow status backup/db-full
workflow status backup/db-full --json
```

---

## compare

Compare two runs of a task side by side. Shows timing deltas, status changes, and output diffs.

**Synopsis**

```
workflow compare <task> [options]
```

**Flags**

| Flag | Description |
|------|-------------|
| `--run <ID>` | Run ID to compare (default: latest) |
| `--with <ID>` | Run ID to compare against (default: previous) |
| `--json` | Output as JSON |
| `--ai` | Use AI (claude/codex/gemini) for natural-language analysis |

**Examples**

```bash
# Compare the two most recent runs
workflow compare deploy/staging

# Compare specific runs with AI analysis
workflow compare deploy/staging --run 42 --with 38 --ai
```

---

## validate

Validate workflow YAML files without executing them. Checks syntax, DAG cycles, and step references.

**Synopsis**

```
workflow validate [task] [options]
```

When called without a task argument, validates all discovered workflows.

**Flags**

| Flag | Description |
|------|-------------|
| `--json` | Output as JSON |

**Examples**

```bash
# Validate everything
workflow validate

# Validate a single workflow
workflow validate backup/db-full
```

---

## export

Export workflows to a compressed archive for sharing or backup.

**Synopsis**

```
workflow export [options]
```

**Flags**

| Flag | Description |
|------|-------------|
| `-o, --output <FILE>` | Output file path (default: `workflow-export-DATE.tar.gz`) |
| `--include-history` | Include the SQLite run history database |

**Examples**

```bash
workflow export
workflow export -o /tmp/my-workflows.tar.gz --include-history
```

---

## import

Import workflows from a `.tar.gz` archive.

**Synopsis**

```
workflow import <archive> [options]
```

**Flags**

| Flag | Description |
|------|-------------|
| `--overwrite` | Overwrite existing files without prompting |
| `--skip-existing` | Skip files that already exist |

**Examples**

```bash
workflow import my-workflows.tar.gz
workflow import /tmp/shared-workflows.tar.gz --skip-existing
```

---

## templates

Browse and manage the built-in and community workflow template catalog.

**Synopsis**

```
workflow templates [options]
```

**Flags**

| Flag | Description |
|------|-------------|
| `--fetch` | Download latest community templates from GitHub |
| `--json` | Output as JSON |

**Examples**

```bash
# List bundled templates
workflow templates

# Fetch and list community templates
workflow templates --fetch
```

---

## ai-update

Use an AI assistant (claude, codex, or gemini) to modify an existing workflow based on natural-language instructions.

**Synopsis**

```
workflow ai-update <task> [options]
```

**Flags**

| Flag | Description |
|------|-------------|
| `--prompt <TEXT>` | Instructions for the AI |
| `--dry-run` | Preview the updated YAML without saving |
| `--save-as <NAME>` | Save as a new task instead of overwriting the original |

**Examples**

```bash
# Preview AI-suggested changes
workflow ai-update backup/db-full --prompt "add a slack notification on failure" --dry-run

# Save AI update as a new task
workflow ai-update deploy/staging --prompt "add a rollback step" --save-as deploy/staging-v2
```

---

## schedule

Schedule a task to run automatically via cron or systemd timer.

**Synopsis**

```
workflow schedule <task> [options]
```

**Flags**

| Flag | Description |
|------|-------------|
| `--cron <EXPR>` | Cron expression (e.g., `"0 2 * * *"`) |
| `--systemd` | Use a systemd user timer instead of crontab |
| `--remove` | Remove an existing schedule for the task |

**Examples**

```bash
# Schedule daily at 2 AM via cron
workflow schedule backup/db-full --cron "0 2 * * *"

# Schedule with systemd timer
workflow schedule backup/db-full --cron "0 2 * * *" --systemd

# Remove a schedule
workflow schedule backup/db-full --remove
```

---

## serve

Start an HTTP server for webhook-triggered workflow execution.

**Synopsis**

```
workflow serve [options]
```

**Flags**

| Flag | Description |
|------|-------------|
| `--port <N>` | Port to listen on (default: 8080) |
| `--bind <ADDR>` | Address to bind to (default: 127.0.0.1) |

**Examples**

```bash
# Start on default port
workflow serve

# Bind to all interfaces on port 9090
workflow serve --bind 0.0.0.0 --port 9090
```

---

## logs

View run logs for a specific task or all tasks.

**Synopsis**

```
workflow logs [task] [options]
```

**Flags**

| Flag | Description |
|------|-------------|
| `--json` | Output as JSON |
| `--limit <N>` | Number of log entries to display (default: 10) |

**Examples**

```bash
# Latest 10 logs across all tasks
workflow logs

# Last 50 logs for a specific task as JSON
workflow logs backup/db-full --limit 50 --json
```

---

## secrets

Manage the encrypted secrets store. Secrets are injected into workflow steps as environment variables.

### secrets init

Initialize the secrets store.

```
workflow secrets init [--ssh-key <PATH>]
```

| Flag | Description |
|------|-------------|
| `--ssh-key <PATH>` | Path to SSH private key for encryption (auto-detected if omitted) |

### secrets set

Store a secret value.

```
workflow secrets set <name> [--value <VAL>]
```

| Flag | Description |
|------|-------------|
| `--value <VAL>` | Secret value; prompts securely if omitted |

### secrets get

Retrieve and decrypt a secret.

```
workflow secrets get <name>
```

### secrets list

List all stored secret names.

```
workflow secrets list
```

### secrets rm

Remove a secret.

```
workflow secrets rm <name>
```

**Examples**

```bash
workflow secrets init
workflow secrets set SLACK_WEBHOOK --value "https://hooks.slack.com/..."
workflow secrets list
workflow secrets get SLACK_WEBHOOK
workflow secrets rm SLACK_WEBHOOK
```

---

## trash

Manage trashed (soft-deleted) tasks.

### trash list

List all trashed tasks.

```
workflow trash list
```

### trash empty

Permanently delete all trashed tasks.

```
workflow trash empty
```

### trash restore

Restore a previously trashed task.

```
workflow trash restore <name>
```

**Examples**

```bash
workflow trash list
workflow trash restore backup/old-db
workflow trash empty
```

---

## sync

Git-based syncing of the workflows directory across machines.

### sync init

Initialize a git repository in the workflows directory.

```
workflow sync init
```

### sync clone

Clone workflows from a remote repository.

```
workflow sync clone <url>
```

### sync push

Commit and push local changes to the remote.

```
workflow sync push [-m <TEXT>]
```

| Flag | Description |
|------|-------------|
| `-m, --message <TEXT>` | Custom commit message (auto-generated if omitted) |

### sync pull

Pull the latest changes from the remote.

```
workflow sync pull
```

### sync status

Show the current sync status (clean, dirty, ahead, behind, diverged).

```
workflow sync status
```

### sync setup

Interactive guided setup for remote sync configuration.

```
workflow sync setup
```

### sync branch

List branches or switch to a different branch.

```
workflow sync branch [name]
```

**Examples**

```bash
workflow sync init
workflow sync setup
workflow sync push -m "add new backup workflows"
workflow sync pull
workflow sync status
workflow sync branch staging
```

---

## snapshot

Manage key-value snapshots associated with tasks. Useful for storing state between runs.

### snapshot set

Store a snapshot value for a task.

```
workflow snapshot set <task> <key> [--value <VAL>]
```

| Flag | Description |
|------|-------------|
| `--value <VAL>` | Value to store; reads from stdin if omitted |

### snapshot get

Retrieve a snapshot value.

```
workflow snapshot get <task> <key>
```

### snapshot delete

Delete a snapshot.

```
workflow snapshot delete <task> <key>
```

### snapshot list

List stored snapshots, optionally filtered by task.

```
workflow snapshot list [task] [--json]
```

| Flag | Description |
|------|-------------|
| `--json` | Output as JSON |

**Examples**

```bash
workflow snapshot set backup/db-full last_size --value "4.2GB"
workflow snapshot get backup/db-full last_size
workflow snapshot list backup/db-full --json
workflow snapshot delete backup/db-full last_size
```

---

## memory

Query the workflow memory system: anomaly detection, baselines, trends, and health scores.

### memory anomalies

Show detected anomalies (duration spikes, new failures, flapping, output drift).

```
workflow memory anomalies [task] [options]
```

| Flag | Description |
|------|-------------|
| `--min-severity <LEVEL>` | Filter by severity: `info`, `warning`, `critical` (default: `info`) |
| `--limit <N>` | Number of anomalies to show (default: 20) |
| `--json` | Output as JSON |

### memory baseline

Show statistical baselines for a task.

```
workflow memory baseline <task> [--json]
```

### memory trends

Show metric trends over time.

```
workflow memory trends <task> [options]
```

| Flag | Description |
|------|-------------|
| `--metric <KEY>` | Metric key to display (default: `duration_ms`) |
| `--days <N>` | Time range in days (default: 30) |
| `--json` | Output as JSON |

### memory health

Show health scores for all tasks.

```
workflow memory health [--json]
```

### memory ack

Acknowledge anomalies to suppress them from future reports.

```
workflow memory ack <id> [--task <REF>]
```

| Flag | Description |
|------|-------------|
| `--task <REF>` | Task reference (required when id is `all`) |

### memory recompute

Recompute baselines from run history.

```
workflow memory recompute [task]
```

**Examples**

```bash
workflow memory health
workflow memory anomalies --min-severity warning
workflow memory anomalies backup/db-full --json
workflow memory baseline backup/db-full
workflow memory trends backup/db-full --metric duration_ms --days 90
workflow memory ack 17
workflow memory ack all --task backup/db-full
workflow memory recompute
```

---

## mcp

Interact with MCP (Model Context Protocol) servers. Requires the binary to be built with `--features mcp`.

### mcp list-tools

List available tools exposed by an MCP server.

```
workflow mcp list-tools <server> [--json]
```

| Flag | Description |
|------|-------------|
| `--json` | Output full tool schemas as JSON |

### mcp call

Call a tool on an MCP server.

```
workflow mcp call <server> <tool> [--arg KEY=VALUE ...]
```

| Flag | Description |
|------|-------------|
| `--arg KEY=VALUE` | Tool argument; repeatable |

### mcp check

Check connectivity to an MCP server.

```
workflow mcp check <server>
```

**Examples**

```bash
# List tools on a configured server alias
workflow mcp list-tools postgres --json

# Call a tool with arguments
workflow mcp call postgres query --arg sql="SELECT count(*) FROM users"

# Check server connectivity
workflow mcp check postgres
```

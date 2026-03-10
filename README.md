# dzworkflows

A lightweight, file-based workflow orchestrator for Linux. Drop bash scripts and YAML workflows into `~/.config/dzworkflows/`, browse them in an interactive TUI, or run them from the command line for cron and automation.

No database — state is tracked via JSON log files. No configuration required — just add files and go.

## Quick Start

```bash
# Install
cargo install --path .

# Create your first workflow
mkdir -p ~/.config/dzworkflows/backup
cat > ~/.config/dzworkflows/backup/db-full.sh << 'EOF'
#!/bin/bash
pg_dump mydb > /tmp/mydb_$(date +%Y%m%d).sql
echo "Backup complete"
EOF

# Run it
dzworkflows run backup/db-full

# Or browse everything in the TUI
dzworkflows
```

## File Structure

```
~/.config/dzworkflows/
├── backup/                  # Category (folder)
│   ├── db-full.sh           # Bash script task
│   └── mysql-daily.yaml     # Multi-step YAML workflow
├── deploy/
│   └── staging.yaml
├── logs/                    # Auto-generated run logs (JSON)
└── config.toml              # Optional configuration
```

- **Folders** become categories for navigation
- **`.sh` files** are executed directly with `bash`
- **`.yaml` files** define multi-step workflows with dependencies

## YAML Workflow Format

```yaml
name: MySQL Daily Backup
steps:
  - id: dump
    cmd: mysqldump --all-databases > /tmp/db.sql
  - id: compress
    cmd: gzip /tmp/db.sql
    needs: [dump]
  - id: upload
    cmd: aws s3 cp /tmp/db.sql.gz s3://backup/
    needs: [compress]
env:
  AWS_PROFILE: prod
```

Steps run in dependency order (topological sort). If a step fails, its dependents are skipped while independent branches continue. Template variables like `{{date}}`, `{{datetime}}`, and `{{hostname}}` are expanded in commands.

## TUI

Launch with `dzworkflows` (no arguments):

```
┌ Categories ┬─ Tasks ────────────┬─ Details ──────────────────┐
│ > backup   │ > db-full    [sh]  │ #!/bin/bash                │
│   deploy   │   mysql-daily[yaml]│ pg_dump mydb > /tmp/...    │
│            │                    │                            │
└────────────┴────────────────────┴────────────────────────────┘
 j/k:nav  Tab:pane  r:run  e:edit  l:logs  /:search  d:dry-run  q:quit
```

| Key | Action |
|-----|--------|
| `j`/`k` or arrows | Navigate up/down |
| `Tab` / `h`/`l` | Switch panes |
| `r` | Run selected task |
| `d` | Dry-run (preview commands) |
| `e` | Open in `$EDITOR` |
| `L` | View run logs |
| `/` | Search tasks |
| `q` / `Ctrl-C` | Quit |

## CLI

```bash
# Run a task (slash or dot notation)
dzworkflows run backup/db-full
dzworkflows run backup.db-full

# Dry-run to preview commands
dzworkflows run deploy/staging --dry-run

# Pass environment variables
dzworkflows run deploy/staging --env ENV=production --env DEBUG=0

# List all tasks
dzworkflows list
dzworkflows list --json

# Check run history for a task
dzworkflows status backup/db-full
dzworkflows status backup/db-full --json

# View logs
dzworkflows logs backup/db-full
dzworkflows logs --limit 20 --json

# Use a custom workflows directory
dzworkflows --dir /path/to/workflows list
```

Exit code is 0 on success, non-zero on failure — suitable for cron:

```cron
0 2 * * * dzworkflows run backup/db-full
```

## Configuration

Optional `~/.config/dzworkflows/config.toml`:

```toml
workflows_dir = "/home/user/.config/dzworkflows"
log_retention_days = 30
editor = "vim"

[hooks]
pre_run = "echo 'starting'"
post_run = "echo 'done'"
```

All fields are optional and have sensible defaults.

## Logging

Each run produces a JSON log in `~/.config/dzworkflows/logs/`:

```json
{
  "id": "a1b2c3d4-...",
  "task_ref": "backup/db-full",
  "started": "2026-03-10T07:54:18Z",
  "ended": "2026-03-10T07:54:19Z",
  "steps": [
    { "id": "run", "status": "Success", "output": "Backup complete\n", "duration_ms": 850 }
  ],
  "exit_code": 0
}
```

Logs older than `log_retention_days` (default 30) are automatically cleaned up on startup.

## Building from Source

```bash
git clone https://github.com/youruser/dzworkflows.git
cd dzworkflows
cargo build --release
# Binary at target/release/dzworkflows
```

Requires Rust 2021 edition (1.56+).

## License

MIT

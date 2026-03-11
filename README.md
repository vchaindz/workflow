# workflow

A lightweight, file-based workflow orchestrator for Linux. Drop bash scripts and YAML workflows into `~/.config/workflow/`, browse them in an interactive TUI, or run them from the command line for cron and automation.

Turn your shell history into reusable workflows — or describe what you need in plain English and let AI generate the task for you.

No database required for execution — state is tracked via JSON logs and an optional SQLite history. No configuration required — just add files and go.

## Highlights

- **Shell history wizard** — browse your recent shell commands, pick the ones you want, and save them as a workflow in seconds
- **AI task generation** — describe a task in natural language, and Claude or Codex generates the workflow steps automatically
- **Template catalog** — start from bundled or community templates with variable substitution
- **Clone & optimize** — duplicate an existing task, strip failed/skipped steps, and parallelize independent branches
- **Run comparison** — diff two runs of the same task side-by-side, with optional AI analysis
- **DAG execution** — multi-step YAML workflows with dependency ordering, conditional steps, retries, and timeouts
- **Interactive TUI** — three-pane browser with real-time execution progress, search, and log viewing
- **CLI for automation** — every operation works headless for cron, CI, and scripting

## Install

Download the latest Linux binary from the [releases page](https://github.com/vchaindz/workflow/releases), or build from source:

```bash
# From source
cargo install --path .

# Create your first workflow
mkdir -p ~/.config/workflow/backup
cat > ~/.config/workflow/backup/db-full.sh << 'EOF'
#!/bin/bash
pg_dump mydb > /tmp/mydb_$(date +%Y%m%d).sql
echo "Backup complete"
EOF

# Run it
workflow run backup/db-full

# Or browse everything in the TUI
workflow
```

## Creating Tasks from Shell History

Press `w` in the TUI to open the history wizard. It reads your shell history (zsh or bash), deduplicates and filters noise, and presents a searchable list:

```
┌─ New Task from History ──────────────────────────────────┐
│ Filter: docker                                           │
│                                                          │
│   [x] docker compose up -d                    2h ago     │
│   [ ] docker ps --format "table {{.Names}}"   3h ago     │
│   [x] docker logs -f webapp                   5h ago     │
│   [ ] docker exec -it db psql                 1d ago     │
│                                                          │
│ Space: toggle  Enter: continue  /: filter  Esc: cancel   │
└──────────────────────────────────────────────────────────┘
```

Select commands with `Space`, press `Enter`, then choose a category and task name. The wizard auto-suggests both based on the commands you picked (e.g., docker commands → category `docker`). Preview the generated YAML before saving.

## AI-Generated Workflows

Press `a` in the TUI to describe a task in plain English. If `claude` or `codex` is on your PATH, the AI generates executable shell commands, a task name, and a category:

```
┌─ AI Task Generator ─────────────────────────────────────┐
│                                                          │
│ Describe what you need:                                  │
│ > check nginx status and restart if not running          │
│                                                          │
│ Enter: generate  Esc: cancel                             │
└──────────────────────────────────────────────────────────┘
```

The AI returns clean shell commands — no prose, no markdown — which are assembled into a YAML workflow. You review the preview, adjust the category/name if needed, and save.

Requires `claude` (Claude Code CLI) or `codex` (OpenAI Codex CLI) installed and authenticated.

## Template Catalog

Press `t` in the TUI to browse bundled and cached templates. Templates support variables like `{{url}}` or `{{db_name}}` that you fill in before saving:

```bash
# CLI: list available templates
workflow templates

# Fetch community templates from GitHub
workflow templates --fetch
```

Bundled templates include security scanning (Trivy CVE checks), monitoring (website content checks), and tool management (Claude/Codex updates).

## Clone & Optimize

Press `W` (shift-w) on any task to clone it. The clone wizard lets you:

- **Remove failed steps** — strip steps that failed in the last run
- **Remove skipped steps** — strip steps that were skipped due to dependency failures
- **Parallelize** — remove unnecessary sequential dependencies between independent steps

This is useful for iterating on a workflow after a partial failure — clone it, remove what broke, and save a clean version.

## File Structure

```
~/.config/workflow/
├── backup/                  # Category (folder)
│   ├── db-full.sh           # Bash script task
│   └── mysql-daily.yaml     # Multi-step YAML workflow
├── deploy/
│   └── staging.yaml
├── logs/                    # Auto-generated run logs (JSON)
├── history.db               # SQLite run history (auto-created)
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
    timeout: 300
  - id: compress
    cmd: gzip /tmp/db.sql
    needs: [dump]
  - id: upload
    cmd: aws s3 cp /tmp/db.sql.gz s3://backup/
    needs: [compress]
    retry: 3
    retry_delay: 5
    run_if: "test -f /tmp/db.sql.gz"
env:
  AWS_PROFILE: prod
```

Steps run in dependency order (topological sort). If a step fails, its dependents are skipped while independent branches continue. Template variables like `{{date}}`, `{{datetime}}`, and `{{hostname}}` are expanded in commands.

## TUI

Launch with `workflow` (no arguments):

```
┌─ workflow v0.1.0 ── 12 workflows ── 48 runs ── 2 failed ─────┐
│                                                                │
│ Categories │ Tasks ──────────────┬─ Details ──────────────────│
│ > backup   │ > db-full    [sh]  │ #!/bin/bash                │
│   deploy   │   mysql-daily[yaml]│ pg_dump mydb > /tmp/...    │
│   docker   │                    │                            │
│                                                               │
├─ Log ─────────────────────────────────────────────────────────┤
│ [14:32:01] ▶ dump — mysqldump --all-databases > /tmp/db.sql  │
│ [14:32:03] ✓ dump (1850ms)                                   │
└───────────────────────────────────────────────────────────────┘
 j/k:nav  Tab:pane  r:run  e:edit  w:wizard  a:ai  t:templates
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
| `w` | New task from shell history |
| `a` | New task from AI prompt |
| `t` | New task from template |
| `W` | Clone & optimize selected task |
| `D` | Delete selected task |
| `?` | Help screen |
| `q` / `Ctrl-C` | Quit |

## CLI

```bash
# Run a task (slash or dot notation)
workflow run backup/db-full
workflow run backup.db-full

# Dry-run to preview commands
workflow run deploy/staging --dry-run

# Run with a step timeout (seconds)
workflow run deploy/staging --timeout 60

# Pass environment variables
workflow run deploy/staging --env ENV=production --env DEBUG=0

# List all tasks
workflow list
workflow list --json

# Check run history for a task
workflow status backup/db-full
workflow status backup/db-full --json

# Compare two runs
workflow compare backup/db-full
workflow compare backup/db-full --ai   # AI-powered analysis

# Validate workflows
workflow validate                      # Validate all
workflow validate backup/db-full       # Validate one

# Export/import workflows
workflow export -o my-workflows.tar.gz --include-history
workflow import my-workflows.tar.gz --overwrite

# Browse templates
workflow templates
workflow templates --fetch

# View logs
workflow logs backup/db-full
workflow logs --limit 20 --json

# Use a custom workflows directory
workflow --dir /path/to/workflows list
```

Exit code is 0 on success, non-zero on failure — suitable for cron:

```cron
0 2 * * * workflow run backup/db-full
```

## Run Comparison

Compare two consecutive runs of the same task to spot regressions:

```bash
workflow compare backup/db-full
```

Shows step-by-step diffs: timing changes, status changes (pass→fail), and output differences. Add `--ai` to get a natural-language analysis of what changed and why.

## Configuration

Optional `~/.config/workflow/config.toml`:

```toml
workflows_dir = "/home/user/.config/workflow"
log_retention_days = 30
editor = "vim"

[hooks]
pre_run = "echo 'starting'"
post_run = "echo 'done'"
```

All fields are optional and have sensible defaults.

## Logging

Each run produces a JSON log in `~/.config/workflow/logs/` and a record in `history.db` (SQLite). The JSON logs contain full step output:

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

## Claude Code Skill

A [Claude Code](https://claude.ai/code) skill is included for managing workflows directly from Claude Code conversations. The skill teaches Claude how to create, edit, run, and inspect workflow tasks using the CLI.

### Installing the Skill

Copy the skill into your Claude Code skills directory:

```bash
mkdir -p ~/.claude/skills/workflow-manager
cp -r skills/workflow-manager/* ~/.claude/skills/workflow-manager/
```

Or symlink it from the repo:

```bash
mkdir -p ~/.claude/skills
ln -s "$(pwd)/skills/workflow-manager" ~/.claude/skills/workflow-manager
```

### What the Skill Provides

Once installed, Claude Code can:

- **Create workflows** — generate YAML or shell script tasks in the correct directory structure
- **List and inspect** — browse categories, tasks, and their configurations
- **Run and dry-run** — execute tasks or preview commands without running them
- **Validate** — check workflow syntax and DAG dependencies
- **View logs** — inspect run history and compare consecutive runs
- **Browse templates** — list bundled templates or fetch community ones from GitHub
- **Export/import** — archive and restore workflow collections

### Usage in Claude Code

Invoke the skill with `/workflow-manager` followed by a command:

```
/workflow-manager list                          # List all workflows
/workflow-manager create backup/db-snapshot     # Create a new task
/workflow-manager run deploy/staging --dry-run  # Dry-run a task
/workflow-manager validate                      # Validate all workflows
/workflow-manager templates --fetch             # Fetch community templates
/workflow-manager logs backup/db-full           # View run logs
```

Or just ask Claude naturally — the skill triggers on mentions of workflows, tasks, categories, and the workflow CLI.

### Skill Files

```
skills/workflow-manager/
├── SKILL.md              # Skill definition and instructions
└── references/
    └── api_reference.md  # Complete YAML schema and CLI reference
```

## Building from Source

```bash
git clone https://github.com/vchaindz/workflow.git
cd workflow
cargo build --release
# Binary at target/release/workflow
```

Requires Rust 2021 edition (1.56+).

## Releases

Pre-built binaries are available on the [GitHub releases page](https://github.com/vchaindz/workflow/releases).

## License

MIT

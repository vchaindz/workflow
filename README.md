# workflow

A lightweight, file-based workflow orchestrator for Linux. Drop bash scripts and YAML workflows into `~/.config/workflow/`, browse them in an interactive TUI, or run them from the command line for cron and automation.

Turn your shell history into reusable workflows — or describe what you need in plain English and let AI generate the task for you.

No database required for execution — state is tracked via JSON logs and an optional SQLite history. No configuration required — just add files and go.

### For Sysadmins

| | |
|---|---|
| **36 bundled templates** | Sysadmin, Docker, and Kubernetes — ready to use out of the box |
| **Real-world tasks** | Disk reports, systemd health checks, container security audits, k8s pod diagnostics |
| **Overdue reminders** | Set `overdue: 7` on any task and get notified when maintenance is late |
| **Cron-friendly CLI** | `workflow run backup/db-full` with proper exit codes for automation |
| **Zero config** | Drop `.sh` or `.yaml` files into `~/.config/workflow/` and go |

### For Developers

| | |
|---|---|
| **AI workflow generation** | Describe a task in English — Claude, Codex, or Gemini writes the YAML |
| **Shell history wizard** | Turn past commands into reusable, parameterized workflows |
| **DAG execution engine** | Retries, timeouts, conditionals, cleanup steps, output capture, and dependency graphs |
| **Interactive TUI** | Browse, run, and monitor workflows with real-time progress |
| **Hot/cold sorting** | Tasks ranked by run frequency — hot tasks float to the top |
| **Export/import** | Share workflows across machines with `workflow export` / `workflow import` |

## Table of Contents

- [Highlights](#highlights)
- [Install](#install)
- [Creating Tasks from Shell History](#creating-tasks-from-shell-history)
- [AI-Generated Workflows](#ai-generated-workflows)
- [AI-Powered Task Update](#ai-powered-task-update)
- [Template Catalog](#template-catalog)
- [Clone & Optimize](#clone--optimize)
- [File Structure](#file-structure)
- [YAML Workflow Format](#yaml-workflow-format)
- [Overdue Reminders](#overdue-reminders)
- [Hot/Cold Task Sorting](#hotcold-task-sorting)
- [Recent Runs & Bookmarks](#recent-runs--bookmarks)
- [TUI](#tui)
- [CLI](#cli)
- [Run Comparison](#run-comparison)
- [Configuration](#configuration)
- [Logging](#logging)
- [Claude Code Skill](#claude-code-skill)
- [Building from Source](#building-from-source)
- [Releases](#releases)
- [License](#license)

## Highlights

- **Dangerous command safety** — blocks `rm -rf /`, fork bombs, and other destructive patterns before execution (override with `--force`)
- **Shell history wizard** — browse your recent shell commands, pick the ones you want, and save them as a workflow in seconds
- **AI task generation** — describe a task in natural language, and Claude, Codex, or Gemini generates the workflow steps automatically
- **AI task update** — select an existing task, describe what to change, and AI rewrites the workflow for you
- **Template catalog** — start from bundled or community templates with variable substitution
- **Clone & optimize** — duplicate an existing task, strip failed/skipped steps, and parallelize independent branches
- **Recent runs** — see the last 10 runs across all tasks at a glance, jump to any task from the list
- **Overdue reminders** — optional `overdue` field on tasks warns you at startup if a task hasn't run within its expected interval
- **Hot/cold task sorting** — tasks show heat indicators (▲/·/▽) based on run frequency; press `f` to sort hot tasks to the top
- **Bookmarked tasks** — save frequently-used tasks for quick access with a single keypress
- **Run comparison** — diff two runs of the same task side-by-side, with optional AI analysis
- **Cleanup steps** — `cleanup:` section runs regardless of success/failure, like a `finally` block
- **Step output capture** — capture step stdout via regex and use as `{{step_id.var}}` in subsequent steps
- **Content-aware search** — TUI search (`/`) matches task names, categories, and step commands
- **Fish shell support** — history wizard reads fish shell history alongside zsh and bash
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

Press `w` in the TUI to open the history wizard. It reads your shell history (zsh, bash, or fish), deduplicates and filters noise, and presents a searchable list:

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

Press `a` in the TUI to describe a task in plain English. If `claude`, `codex`, or `gemini` is on your PATH, the AI generates executable shell commands, a task name, and a category:

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

Requires `claude` (Claude Code CLI), `codex` (OpenAI Codex CLI), or `gemini` (Google Gemini CLI) installed and authenticated.

## AI-Powered Task Update

Press `A` (Shift-a) on a selected task to update it with AI assistance. Describe what you want to change — add error handling, parallelize steps, add timeouts — and the AI rewrites the entire workflow YAML:

```
┌─ AI Task Update ────────────────────────────────────────┐
│                                                          │
│ Describe how to update this task:                        │
│ Task: backup/db-full                                     │
│ > add error handling and retry logic to each step        │
│                                                          │
│ Enter: send  Esc: cancel                                 │
└──────────────────────────────────────────────────────────┘
```

The updated YAML is previewed before saving. Press `Enter` to overwrite the original, or `Esc` to cancel.

### CLI: `ai-update` subcommand

```bash
# Update a task with AI
workflow ai-update backup/db-full --prompt "parallelize independent steps"

# Preview without saving
workflow ai-update backup/db-full --prompt "add timeouts" --dry-run

# Save as a new task instead of overwriting
workflow ai-update backup/db-full --prompt "add cleanup step" --save-as db-full-v2
```

## Template Catalog

Press `t` in the TUI to browse bundled and cached templates. Templates support variables like `{{url}}` or `{{db_name}}` that you fill in before saving:

```bash
# CLI: list available templates
workflow templates

# Fetch community templates from GitHub
workflow templates --fetch
```

Bundled templates include security scanning (Trivy CVE checks), monitoring (website content checks), tool management (Claude/Codex updates), and sysadmin tasks (SSL cert expiry, SMART disk health, NTP sync, cron audit, SSH key audit, firewall review).

## Clone & Optimize

Press `W` (shift-w) on any task to clone it. The clone wizard lets you:

- **Remove failed steps** — strip steps that failed in the last run
- **Remove skipped steps** — strip steps that were skipped due to dependency failures
- **Parallelize** — remove unnecessary sequential dependencies between independent steps

This is useful for iterating on a workflow after a partial failure — clone it, remove what broke, and save a clean version.

## Overdue Reminders

Add an `overdue` field (in days) to any YAML task to get a startup reminder when the task hasn't been run recently:

```yaml
name: Database Full Backup
overdue: 7          # warn if not run within 7 days
steps:
  - id: backup
    cmd: pg_dump mydb > /backups/full.sql
```

When you launch the TUI, any overdue tasks appear in a popup:

```
┌──────────────── ⚠ Overdue Tasks ────────────────┐
│  ! backup/db-full                 3 day(s) overdue │
│  ! monitoring/disk-check          7 day(s) overdue │
│                                                     │
│  ↑↓ navigate · Enter jump to task · Esc dismiss     │
└─────────────────────────────────────────────────────┘
```

Press `Enter` to jump directly to an overdue task, or `Esc` to dismiss.

Tasks that have never been run are also flagged, with the overdue threshold shown as the number of days overdue.

## Hot/Cold Task Sorting

Tasks are automatically classified by how often you run them in the last 30 days:

| Tier | Runs (30d) | Indicator | Color |
|------|-----------|-----------|-------|
| Hot  | ≥5        | `▲`       | Green |
| Warm | 1–4       | `·`       | Default |
| Cold | 0         | `▽`       | Blue  |

Heat indicators appear next to every task in the TUI task list. Press `f` to toggle between alphabetical and heat-based sorting — hot tasks float to the top for quick access. Press `f` again to revert to alphabetical order. The status bar shows the current sort mode.

Heat data is loaded from the SQLite history database at startup and refreshed on each automatic rescan.

## Recent Runs & Bookmarks

### Recent Runs (`R`)

Press `R` in the TUI to see the last 10 runs across all tasks, newest first:

```
┌─────────────── Recent Runs (last 10) ───────────────┐
│  ✓ backup/db-full                2026-03-11 14:32     1.2s │
│  ✗ deploy/staging                2026-03-11 14:30     3.5s │
│  ✓ docker/cleanup                2026-03-11 13:15     0.8s │
│                                                            │
│  Up/Down: navigate  Enter: go to task  Esc: close          │
└────────────────────────────────────────────────────────────┘
```

Press `Enter` on any run to jump directly to that task in the main view.

### Bookmarked Tasks (`S` / `s`)

Press `S` on any task to bookmark it. Bookmarked tasks show a ★ indicator in the task list and are persisted in `config.toml`:

```toml
bookmarks = ["backup/db-full", "deploy/staging"]
```

Press `s` to open the saved tasks modal and quickly jump to any bookmarked task. Press `S` again to remove the bookmark.

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
overdue: 1                    # warn if not run within 1 day
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

### Cleanup Steps

Add a `cleanup` section to run steps regardless of workflow success or failure (similar to a `finally` block):

```yaml
name: Deploy with Cleanup
steps:
  - id: deploy
    cmd: ./deploy.sh
cleanup:
  - id: remove-tmpfiles
    cmd: rm -rf /tmp/deploy-*
  - id: unlock
    cmd: rm -f /tmp/deploy.lock
```

Cleanup step failures are logged but do not affect the workflow's exit code.

### Step Output Capture

Capture step output as variables for use in subsequent steps:

```yaml
steps:
  - id: get-version
    cmd: cat VERSION
    outputs:
      - name: version
        pattern: "^(\\S+)"
  - id: tag
    cmd: git tag v{{get-version.version}}
    needs: [get-version]
```

Each output defines a `name` and a regex `pattern` with a capture group. The captured value is available as `{{step_id.output_name}}` in subsequent steps.

### Dangerous Command Safety

Commands matching known destructive patterns (e.g., `rm -rf /`, `dd` to block devices, fork bombs) are automatically blocked. Override with `--force`:

```bash
workflow run dangerous-task --force
```

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
 arrows:nav  r:run  d:dry-run  e:edit  c:compare  f:heat-sort  w:new  W:clone  t:template  a:ai  R:recent  s:saved  Del:delete  L:logs  /:search  h:help  q:quit
```

| Key | Action |
|-----|--------|
| `j`/`k` or arrows | Navigate up/down |
| `Tab` / `h`/`l` | Switch panes |
| `r` | Run selected task |
| `d` | Dry-run (preview commands) |
| `e` | Open in `$EDITOR` |
| `L` | View run logs |
| `R` | Recent runs (last 10) |
| `s` | Saved/bookmarked tasks |
| `S` | Toggle bookmark on task |
| `f` | Toggle heat sort (hot tasks first) |
| `/` | Search tasks |
| `w` | New task from shell history |
| `a` | New task from AI prompt |
| `A` | AI update selected task |
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

# Force-run (bypass dangerous command safety checks)
workflow run risky-task --force

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

# AI-update an existing task
workflow ai-update backup/db-full --prompt "add error handling"
workflow ai-update backup/db-full --prompt "parallelize steps" --dry-run
workflow ai-update backup/db-full --prompt "add cleanup" --save-as db-full-v2

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

bookmarks = ["backup/db-full", "deploy/staging"]
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

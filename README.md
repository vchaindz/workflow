[![CI](https://github.com/vchaindz/workflow/actions/workflows/ci.yml/badge.svg)](https://github.com/vchaindz/workflow/actions/workflows/ci.yml)
[![GitHub Release](https://img.shields.io/github/v/release/vchaindz/workflow)](https://github.com/vchaindz/workflow/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.56-blue.svg)]()

# workflow

**Stop losing one-liners to shell history. Stop rewriting the same maintenance scripts on every box.**

`workflow` is a file-based workflow orchestrator for Linux built for the AI age. Turn your scattered shell commands into organized, runnable, version-controlled tasks — with a full TUI for browsing, a headless CLI for cron, and first-class integration with Claude Code, OpenAI Codex CLI, and Google Gemini CLI to generate, fix, and refine workflows using natural language.

Drop a `.sh` or `.yaml` file into `~/.config/workflow/` and it's immediately available to run, schedule, and track. No daemon. No database to set up. No YAML-hell configuration. Or skip the file entirely — describe what you need in English and let AI write it for you.

<!-- Replace with a real screenshot or demo GIF: -->
<!-- ![workflow TUI](https://raw.githubusercontent.com/vchaindz/workflow/main/assets/demo.gif) -->

```text
 workflow v0.3.4 ── 12 workflows ── 48 runs ── 2 failed

 Categories  Tasks                    Details
 > backup    ▲ db-full    ✓ 2d [sh]   #!/bin/bash
   deploy    · mysql-daily✗ 5h [yml]  pg_dump mydb > /tmp/mydb_$DATE.sql
   docker    ▽ s3-sync       [yml]    echo "Backup complete"
   k8s

 Log
 [14:32:01] ▶ dump — mysqldump --all-databases > /tmp/db.sql
 [14:32:03] ✓ dump (1850ms)

 r:run  d:dry-run  e:edit  w:new  a:ai  t:template  /:search  q:quit
```

## Why workflow?

If you manage servers, you already have workflows — they're just scattered across shell histories, wiki pages, and half-remembered incantations. `workflow` gives them a proper home.

**For the solo sysadmin** managing a handful of boxes: stop re-typing `docker system prune && docker compose pull && docker compose up -d` every Tuesday. Save it once, run it from anywhere, get notified when you forget.

**For the DevOps team** maintaining production infrastructure: standardize runbooks as version-controlled YAML with dependency ordering, retries, timeouts, and cleanup steps. Sync them across machines via Git. Review run history when something breaks.

**For the on-call engineer** at 2am: browse 52 bundled templates covering sysadmin, Docker, Kubernetes, and Linux patching workflows. Don't remember the `kubectl` incantation for checking PV storage? It's already there.

**For the AI-assisted operator**: `workflow` is designed to work *with* AI coding tools, not around them. Claude Code, Codex CLI, and Gemini CLI can generate new workflows from a plain-English description, rewrite existing tasks ("add retries and error handling"), and auto-diagnose failures with one keypress. A bundled Claude Code skill lets you manage workflows entirely from AI conversations. The file-based, YAML-native design means AI tools can read and write workflows without any special adapters.

## Quick start

```bash
# Install (download binary or build from source)
cargo install --path .

# Create your first task — it's just a shell script in a folder
mkdir -p ~/.config/workflow/backup
cat > ~/.config/workflow/backup/db-full.sh << 'EOF'
#!/bin/bash
pg_dump mydb > /tmp/mydb_$(date +%Y%m%d).sql
echo "Backup complete"
EOF

# Run it
workflow run backup/db-full

# Or browse everything interactively
workflow
```

That's it. No init command, no project file, no configuration. Every `.sh` and `.yaml` file in `~/.config/workflow/` is automatically discovered and organized by folder.

## What makes it useful

### Turn shell history into reusable tasks

Press `w` in the TUI to browse your recent shell history (zsh, bash, or fish). Select the commands you want, give it a name, and you have a workflow. The wizard auto-suggests a category based on the commands — docker commands go under `docker/`, kubectl commands under `k8s/`.

```
┌─ New Task from History ──────────────────────────────────┐
│ Filter: docker                                           │
│                                                          │
│   [x] docker compose up -d                    2h ago     │
│   [ ] docker ps --format "table {{.Names}}"   3h ago     │
│   [x] docker logs -f webapp                   5h ago     │
│                                                          │
│ Space: toggle  Enter: continue  /: filter  Esc: cancel   │
└──────────────────────────────────────────────────────────┘
```

### AI-native workflow management

`workflow` treats AI CLI tools as first-class citizens. If `claude` (Claude Code), `codex` (OpenAI Codex CLI), or `gemini` (Google Gemini CLI) is on your PATH, you unlock four capabilities directly from the TUI:

**Generate** (`a`) — describe a task in plain English. "Set up daily postgres backup with S3 upload and Slack notification on failure." The AI generates executable YAML with proper step dependencies, error handling, and cleanup. Review the preview before saving.

**Update** (`A`) — select any existing task and describe what to change. "Add retry logic to the upload step", "parallelize the independent checks", "switch from rsync to rclone". The AI rewrites the full YAML while preserving your structure.

**Fix** (`a` after failure) — when a workflow fails, press `a` and AI analyzes the error output, diagnoses the root cause, and proposes a corrected YAML. No more staring at cryptic stderr at 2am.

**Refine** (`r` at preview) — iteratively improve any AI-generated result before saving. Each round sends the current YAML plus your instructions back to the AI. Repeat as many times as needed:

```
Preview → r → "add error handling" → Enter → (AI refines) → Preview
                                                               ↓
                                                    r → "also add logging" → Enter → ...
```

Press `d` at any preview stage to dry-run the workflow without saving — verify it works, then save or keep refining.

All of this works from the CLI too:

```bash
workflow ai-update backup/db-full --prompt "add error handling and retries"
workflow ai-update backup/db-full --prompt "parallelize steps" --dry-run
workflow ai-update backup/db-full --prompt "add cleanup" --save-as db-full-v2
```

The AI integration is intentionally tool-agnostic — `workflow` auto-detects whichever AI CLI you have installed and uses it transparently. The file-based YAML format means AI tools can also read and write workflows directly from outside the TUI, making `workflow` a natural fit for agentic coding sessions.

### 52 bundled templates ready to go

Don't start from scratch. Press `t` to browse templates covering real operational tasks:

**Sysadmin** — disk usage reports, SSL certificate expiry checks, SMART disk health, NTP sync verification, cron audit, SSH key audit, firewall review, failed services check, log cleanup, system updates, memory monitoring, port scanning, user audit, backup verification, CPU load checks, service status

**Docker** — container cleanup, compose status, image updates, log tailing, network inspection, resource limits, restart unhealthy containers, security scanning, volume backup

**Kubernetes** — cluster health, deployment status, failed pod diagnostics, namespace audit, PV storage, RBAC review, resource usage, secret/configmap audit, service endpoints

**Patching** — security-only patches, patch audit, kernel updates, rollback, compliance reports, unattended updates setup, package holds, reboot checks, changelog review, post-patch verification (Debian/Ubuntu, RHEL/Fedora, SUSE, Arch)

Templates support variables — fill in `{{db_name}}` or `{{backup_path}}` when you save. Fetch community templates from GitHub with `workflow templates --fetch`.

### Never forget maintenance again

Add `overdue: 7` to any task. When you launch the TUI, overdue tasks pop up immediately:

```
┌──────────── ⚠ Overdue Tasks ──────────────────┐
│  ! backup/db-full           3 day(s) overdue   │
│  ! monitoring/disk-check    7 day(s) overdue   │
│                                                 │
│  Enter: jump to task  Esc: dismiss              │
└─────────────────────────────────────────────────┘
```

### Multi-step workflows with real dependency management

YAML workflows support DAG execution with topological ordering. If step B depends on step A, it waits. If step C is independent, it doesn't:

```yaml
name: MySQL Daily Backup
overdue: 1
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
cleanup:
  - id: remove-tmpfiles
    cmd: rm -f /tmp/db.sql /tmp/db.sql.gz
env:
  AWS_PROFILE: prod
```

If a step fails, its dependents are skipped — but independent branches keep running. Steps can capture output via regex and pass values downstream with `{{step_id.var}}`. Cleanup steps run regardless of success or failure, like a `finally` block. Interactive commands (REPLs, `journalctl -f`, TUI tools) are auto-detected and run with the terminal restored — or mark steps `interactive: true` explicitly.

### Step-level branching with `run_if` / `skip_if`

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

`run_if` runs the step only when the condition succeeds (exit 0). `skip_if` is the inverse — it skips when the condition succeeds. Both support full template expansion, so `{{var}}` references work in conditions.

### Native notification types

Notify commands support URL-scheme shorthands that expand to `curl`/`mail` commands internally. No extra dependencies needed:

```yaml
notify:
  on_failure: "slack://https://hooks.slack.com/services/T00/B00/xxx"
  on_success: "webhook://https://status.example.com/api/deploy"
  env:
    environment: production
    team: platform
```

| Scheme | Expands to |
|--------|-----------|
| `slack://WEBHOOK_URL` | `curl` POST with Slack JSON text payload |
| `webhook://URL` | `curl` POST with all variables as JSON object |
| `email://ADDRESS` | `printf` + `mail` with summary |
| *(no prefix)* | Runs as-is (bash command) |

Notification commands have access to rich template variables: `{{task_ref}}`, `{{exit_code}}`, `{{workflow_name}}`, `{{hostname}}`, `{{failed_steps}}`, `{{duration_ms}}`, `{{timestamp}}`, `{{status}}`, plus any keys from `notify.env`.

### Built-in safety nets

Accidentally pasting `rm -rf /` into a workflow won't ruin your day. `workflow` blocks known destructive patterns — fork bombs, `dd` to block devices, `chmod -R 777 /` — before execution. Override with `--force` when you actually mean it.

Environment variable values from `env:` blocks are automatically redacted in live output and logs. `sudo` steps get a pre-flight check before prompting. Failed steps produce actionable hints: "permission denied → check sudo", "command not found → check PATH".

### Track everything

Every run is recorded in SQLite with who ran it, from which machine, via which interface (TUI or CLI), and how long it took. JSON logs capture full step output. Compare consecutive runs to spot regressions:

```bash
workflow compare backup/db-full        # side-by-side diff
workflow compare backup/db-full --ai   # AI-powered analysis
```

Tasks show heat indicators based on 30-day run frequency: `▲` (hot, ≥5 runs), `·` (warm), `▽` (cold). Press `f` to sort hot tasks to the top. Press `F` to filter by status: All → Failed → Overdue → Never-run.

## File structure

```
~/.config/workflow/
├── backup/                  # Category (folder name)
│   ├── db-full.sh           # Bash script — runs directly
│   └── mysql-daily.yaml     # Multi-step YAML workflow
├── deploy/
│   └── staging.yaml
├── docker/
│   └── cleanup.yaml
├── logs/                    # Auto-generated (JSON per run)
├── history.db               # Auto-generated (SQLite)
└── config.toml              # Optional
```

Folders are categories. `.sh` files are bash tasks. `.yaml` files are multi-step workflows. That's the entire data model.

## CLI reference

```bash
# Run tasks
workflow run backup/db-full
workflow run deploy/staging --dry-run
workflow run deploy/staging --env ENV=production --timeout 60
workflow run risky-task --force          # bypass dangerous command check

# List and inspect
workflow list                            # all tasks
workflow list --json                     # machine-readable
workflow status backup/db-full           # run history
workflow validate                        # check all YAML syntax

# AI-powered updates
workflow ai-update backup/db-full --prompt "add error handling"
workflow ai-update backup/db-full --prompt "parallelize steps" --dry-run
workflow ai-update backup/db-full --prompt "add cleanup" --save-as db-full-v2

# Scheduling
workflow schedule backup/db-full --cron "0 2 * * *"
workflow schedule backup/db-full --systemd
workflow schedule backup/db-full --remove

# Compare runs
workflow compare backup/db-full
workflow compare backup/db-full --ai

# Templates
workflow templates
workflow templates --fetch

# Export / import
workflow export -o my-workflows.tar.gz --include-history
workflow import my-workflows.tar.gz --overwrite

# Secrets
workflow secrets init                    # setup encrypted store
workflow secrets set DB_PASSWORD         # prompt for value
workflow secrets set API_KEY --value x   # set directly
workflow secrets list                    # names only
workflow secrets get DB_PASSWORD         # decrypt and print
workflow secrets rm DB_PASSWORD          # remove

# Logs
workflow logs backup/db-full
workflow logs --limit 20 --json

# Sync across machines
workflow sync setup                      # one-time: init + private GitHub repo
workflow sync push                       # auto-commit and push
workflow sync pull                       # pull latest
workflow sync status
workflow sync branch                     # list all branches
workflow sync branch customer-acme       # switch branch (auto-commits first)
```

Exit code is 0 on success, non-zero on failure — works directly in cron jobs and CI pipelines.

## TUI keybindings

| Key | Action |
|-----|--------|
| `j`/`k` or arrows | Navigate |
| `Tab` / `h`/`l` | Switch panes |
| `r` | Run selected task |
| `d` | Dry-run (preview without executing) |
| `e` | Open in `$EDITOR` |
| `/` | Search tasks and step commands |
| `f` | Toggle heat sort (hot tasks first) |
| `F` | Cycle status filter (All/Failed/Overdue/Never-run) |
| `w` | New task from shell history |
| `a` | New task via AI (or AI fix when error visible) |
| `A` | AI-update selected task |
| `t` | New task from template catalog |
| `W` | Clone and optimize selected task |
| `n` | Rename task or category |
| `D` | Delete (soft — moves to `.trash/`) |
| `R` | Recent runs (last 10 across all tasks) |
| `S` | Toggle bookmark |
| `s` | View bookmarked tasks |
| `L` | View run logs |
| `K` | Secrets manager (add/view/edit/delete) |
| `G` | Git sync controls |
| `c` | Compare last two runs |
| `?` | Help (context-sensitive) |
| `q` | Quit |

## YAML workflow format

```yaml
name: Deploy with Rollback
overdue: 1                          # remind if not run within N days
steps:
  - id: check-health
    cmd: curl -sf http://localhost/health
    timeout: 10                     # seconds
  - id: deploy
    cmd: ./deploy.sh {{version}}
    needs: [check-health]           # dependency
    retry: 2                        # retry on failure
    retry_delay: 5                  # seconds between retries
    run_if: "test -f deploy.sh"     # conditional execution
  - id: rollback
    cmd: ./rollback.sh
    run_if: "test '{{deploy.status}}' = 'failed'"   # branch on step outcome
  - id: smoke-test
    cmd: ./smoke-test.sh
    skip_if: "test '{{deploy.status}}' = 'failed'"  # skip when condition is true
  - id: get-version
    cmd: cat VERSION
    outputs:                        # capture output as variable
      - name: ver
        pattern: "^(\\S+)"
  - id: tag
    cmd: git tag v{{get-version.ver}}
    needs: [get-version]
cleanup:                            # runs regardless of success/failure
  - id: unlock
    cmd: rm -f /tmp/deploy.lock
env:
  DEPLOY_ENV: production            # values auto-redacted in logs
notify:
  on_failure: "slack://https://hooks.slack.com/services/T00/B00/xxx"
  on_success: "webhook://https://status.example.com/api/deploy"
  env:                              # extra vars available in notify commands
    environment: production
    team: platform
```

Template variables available in all commands: `{{date}}`, `{{datetime}}`, `{{hostname}}`, `{{step_id.status}}` (after each step: success/failed/skipped/timedout), plus any captured step outputs.

## Sync across machines

Sync workflow definitions via a private GitHub repo:

```bash
workflow sync setup    # creates private repo, enables auto-sync
```

After setup, changes auto-commit and push. The TUI pulls on startup. Press `G` in the TUI for manual sync controls. Logs, history, and local config stay local.

### Branch switching (multi-tenant workflows)

Each customer or environment can live on its own branch. Switch from the CLI or TUI:

```bash
workflow sync branch                 # list branches (* marks current)
workflow sync branch customer-acme   # switch to branch (creates if needed)
```

In the TUI, press `G` → "Switch branch" to browse and switch interactively. Dirty changes are auto-committed before switching, and workflows are rescanned automatically for the new branch content.

```toml
# ~/.config/workflow/config.toml
[sync]
enabled = true
auto_commit = true
auto_push = true
auto_pull_on_start = true
```

## Configuration

Optional `~/.config/workflow/config.toml` — everything has sensible defaults:

```toml
workflows_dir = "/home/user/.config/workflow"
log_retention_days = 30
editor = "vim"

[hooks]
pre_run = "echo 'starting'"
post_run = "echo 'done'"

bookmarks = ["backup/db-full", "deploy/staging"]
```

## AI tool integration

### Supported AI CLIs

`workflow` auto-detects these tools at startup and shows which is available in the TUI header:

| Tool | Detection | Used for |
|------|-----------|----------|
| [Claude Code](https://docs.anthropic.com/en/docs/claude-code) (`claude`) | `claude -p` | Generate, update, fix, and refine workflows |
| [Codex CLI](https://github.com/openai/codex) (`codex`) | `codex exec` | Generate, update, fix, and refine workflows |
| [Gemini CLI](https://github.com/google-gemini/gemini-cli) (`gemini`) | `gemini -p` | Generate, update, fix, and refine workflows |

Install any one of these and authenticate it — `workflow` handles the rest. No API keys to configure inside `workflow` itself.

### Claude Code skill

A bundled Claude Code skill lets you manage workflows entirely from within Claude Code or Claude Code-powered agents. Install it:

```bash
mkdir -p ~/.claude/skills
ln -s "$(pwd)/skills/workflow-manager" ~/.claude/skills/workflow-manager
```

Then ask Claude naturally — "create a workflow for daily database backups", "list my overdue tasks", "dry-run the staging deploy" — or use `/workflow-manager run backup/db-full --dry-run`.

This makes `workflow` a natural building block for agentic automation: AI agents can create, validate, and execute operational tasks through a well-defined file-based interface without any special APIs.

## Encrypted secrets store

Workflows can reference secrets by name in `secrets:` — but where do the values come from? Instead of leaving passwords in `.bashrc` or `.env` files, `workflow` ships an encrypted secrets store backed by `age` and your SSH key.

```bash
# One-time setup (auto-detects ~/.ssh/id_ed25519)
workflow secrets init

# Store secrets (prompts for value securely)
workflow secrets set DB_PASSWORD
workflow secrets set API_TOKEN --value sk-live-abc123

# List and retrieve
workflow secrets list
workflow secrets get DB_PASSWORD

# Remove
workflow secrets rm DB_PASSWORD
```

Secrets are encrypted at rest in `~/.config/workflow/secrets.age` using your SSH public key and decrypted to memory only at runtime. Values are zeroized after use.

### TUI secrets manager

Press `K` in the TUI to manage secrets without leaving the interface:

- **Browse** — see all stored secret names at a glance
- **Add** (`a`) — enter a new secret name and value (value input is masked)
- **View** (`v`/`Enter`) — decrypt and reveal a secret's value (any key dismisses)
- **Edit** (`e`) — update an existing secret's value
- **Delete** (`d`) — remove a secret with confirmation

If the secrets store hasn't been initialized yet, the modal offers to set it up automatically using your SSH key.

### Auto-injection into workflows

When a workflow declares `secrets:`, values are automatically injected from the store at execution time:

```yaml
name: Deploy
secrets:
  - DB_PASSWORD
  - API_TOKEN
steps:
  - id: migrate
    cmd: DATABASE_URL="postgres://app:$DB_PASSWORD@db/prod" ./migrate.sh
```

Precedence: explicit `env:` in YAML > `--env` CLI flag > secrets store > environment variables. Secrets never override values you set explicitly. If the store doesn't exist or a secret isn't found, the workflow falls back to environment variables (existing behavior preserved).

## Security

Multiple layers of protection are built in:

- **Dangerous command blocking** — `rm -rf /`, fork bombs, `dd` to devices, `mkfs` on real devices, and similar destructive patterns are caught before execution. Override with `--force`.
- **Encrypted secrets store** — secrets encrypted at rest with `age` + SSH key, decrypted to memory only, zeroized after use. File written as 0600.
- **Secret masking** — `env:` values and injected secrets are redacted in live output and log files.
- **Path traversal protection** — task references can't escape the workflows directory.
- **Command injection prevention** — template variables and task names are sanitized.
- **Import validation** — archive imports reject paths that would write outside the target directory.

## Install

**Pre-built binary** — download from [GitHub Releases](https://github.com/vchaindz/workflow/releases).

**From source:**

```bash
git clone https://github.com/vchaindz/workflow.git
cd workflow
cargo build --release
# Binary: target/release/workflow
```

Requires Rust 1.56+ (2021 edition). Single binary, no runtime dependencies.

## License

MIT — Copyright 2026 Dennis Zimmer

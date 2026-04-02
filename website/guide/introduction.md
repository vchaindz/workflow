# Introduction

## What is workflow?

**workflow** is a file-based workflow orchestrator for Linux. It is a single binary with no daemon, no database to configure, and no setup ceremony. Drop a `.sh` or `.yaml` file into `~/.config/workflow/` and it is immediately available to run, schedule, and track.

Think [n8n](https://n8n.io), but for the terminal. The same power --- parallel DAGs, sub-workflows, conditional branching, retries, notifications --- delivered through a TUI you can browse over SSH and a headless CLI for cron.

## Design philosophy

Everything is a file. Folders are categories. Shell scripts and YAML files are tasks. There is no project manifest, no init command, no migration step. Your workflow library is a directory tree you can version-control, rsync, or sync via Git.

```
~/.config/workflow/
├── backup/
│   ├── db-full.sh
│   └── mysql-daily.yaml
├── deploy/
│   └── staging.yaml
└── config.toml              # Optional
```

## Who is it for?

**The solo sysadmin** managing a handful of boxes. Stop re-typing `docker system prune && docker compose pull && docker compose up -d` every Tuesday. Save it once, run it from anywhere, get notified when you forget.

**The DevOps team** maintaining production infrastructure. Standardize runbooks as version-controlled YAML with dependency ordering, retries, timeouts, and cleanup steps. Sync them across machines via Git. Review run history when something breaks.

**The on-call engineer** at 2am. Browse 56 bundled templates covering sysadmin, Docker, Kubernetes, and Linux patching workflows. Don't remember the `kubectl` incantation for checking PV storage? It is already there.

**The AI-assisted operator**. workflow is designed to work *with* AI coding tools, not around them. Claude Code, Codex CLI, and Gemini CLI can generate new workflows from plain English, rewrite existing tasks, and auto-diagnose failures with one keypress. The file-based YAML design means AI tools can read and write workflows without any special adapters.

## Key capabilities

- **DAG workflows** with step dependencies, retries, timeouts, and cleanup blocks --- [details](/guide/workflows)
- **TUI and CLI modes** --- browse interactively or run headless from cron --- [details](/guide/tui)
- **AI generate, update, fix, and refine** workflows using Claude Code, Codex CLI, or Gemini CLI --- [details](/guide/ai-integration)
- **56 bundled templates** for sysadmin, Docker, Kubernetes, and patching tasks
- **9 notification services** --- Slack, Discord, Telegram, Teams, ntfy, Gotify, Mattermost, webhooks, email
- **MCP integration** --- call any of 16,000+ MCP-compatible tools directly from YAML steps
- **Encrypted secrets** backed by `age` and your SSH key
- **Git sync** with branch switching for per-environment workflow libraries
- **Webhook server** --- trigger workflows via REST API from CI, monitoring, or chatbots
- **Anomaly detection and memory** --- automatic post-run profiling with duration spike, flapping, and output drift detection
- **Sub-workflows** --- compose runbooks from other runbooks with `call:` steps
- **Expression filters** --- pipe syntax for in-line variable transformation (`upper`, `lower`, `default`, `replace`, ternary)
- **For-each loops** --- iterate over static lists or dynamic command output, with parallel execution

## Built-in safety

workflow blocks known destructive command patterns before execution --- `rm -rf /`, fork bombs, `dd` to block devices, `chmod -R 777 /`, and others. Override with `--force` when you mean it.

Environment variable values from `env:` blocks are automatically redacted in live output and logs. `sudo` steps get a pre-flight check. Failed steps produce actionable hints ("permission denied --- check sudo", "command not found --- check PATH"). Soft delete moves files to `.trash/` instead of removing them permanently.

## Next steps

- [Installation](/guide/installation) --- download a binary or build from source
- [Quick start](/guide/quick-start) --- create and run your first workflow in under a minute
- [Workflows](/guide/workflows) --- YAML format deep-dive

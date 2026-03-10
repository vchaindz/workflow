# Dzworkflows Product Requirements Document (PRD) - Revised

**Version:** 1.1  
**Date:** March 10, 2026  
**Author:** Perplexity (revised per user feedback)  
**Target Tech:** Rust + Ratatui (TUI), with CLI mode for cron/non-interactive exec.

## Overview

Dzworkflows is a lightweight, file-based workflow orchestrator for Linux desktops/servers. It treats your `~/.config/dzworkflows/` directory as a living task tree: folders = categories (e.g., `backup`, `deploy`), files = atomic bash scripts or YAML workflows (multi-step with deps/parallelism). Interactive TUI for browsing/executing; CLI for automation (cron, scripts). No DB—pure file state via JSON logs/timestamps. Inspired by taskwarrior/task-dash but script/YAML-native. [github](https://github.com/aleksandersh/task-tui)

**Goals:**
- Zero-config setup: drop scripts/YAML in folders, run.
- TUI for discovery/debug (Ratatui); CLI for prod (e.g., `dzworkflows run backup.db-full`).
- Bash-first; YAML for DAGs (steps, deps, vars).
- Extensible: hooks, env injection.

**Non-Goals:**
- Multi-user/multi-machine sync (use Git/fs).
- Visual DAG editor (text YAML only).
- Heavy deps (no Docker/K8s native).

## Key Features

### 1. File Structure & Discovery
```
~/.config/dzworkflows/
├── backup/                 # Folder = nav category
│   ├── db-full.sh          # Atomic bash script
│   └── mysql-daily.yaml    # Multi-step YAML
├── deploy/
│   └── staging.yaml
└── logs/                   # Auto-generated (JSON per run)
```
- Auto-scans on launch.
- Folder: Top-level nav (e.g., "Backup", "Deploy").
- File: Task name (basename w/o ext).
  - `.sh`: Direct exec (`bash script.sh`).
  - `.yaml`: Parsed DAG (steps: bash cmds, deps: `needs: [prev]`, parallel: `parallel: true`).

### 2. TUI (Ratatui)
Ratatui app (full-screen, vim-like keys):
- **Sidebar:** Folder tree (↑↓/jkl, Enter to expand).
- **Main:** Task list (name, last run, status: success/fail/pending, duration).
- **Details:** Preview file contents (bash/YAML), env vars, deps graph (ASCII).
- **Actions:** Select multi → Run/Preview/Kill/Logs.
- **Shortcuts:** `r` run, `e` edit (spawn $EDITOR), `l` logs, `q` quit.
- Status bar: Global search, recent runs.
- Themes: Dark/default.

Example screen:
```
Backup ──────► db-full (✅ 2m ago)
             mysql-daily (❌ 1h ago)
Deploy ─────► staging (⏳ pending)
[Search: db]  r=run e=edit l=log q=quit
```

### 3. YAML Workflow Spec
Simple schema (one file = one workflow):
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
    parallel: true  # After dump
env:
  AWS_PROFILE: prod
```
- Parsed/exec'd sequentially/parallel.
- Vars injection (e.g., `{{date}}`).
- Exit codes propagate fails.

### 4. CLI Engine
Non-interactive exec for cron/systemd:
```
dzworkflows run [folder/]task        # e.g., dzworkflows run backup.db-full
dzworkflows list                     # JSON tree output
dzworkflows status backup.mysql-daily # Last run JSON
dzworkflows logs backup --json       # Machine-readable
```
- `--dry-run`: Preview cmds.
- `--env KEY=val`: Override.
- Exit 0 on success; logs to `~/.config/dzworkflows/logs/<timestamp>.json`.
- Cron example: `0 2 * * * dzworkflows run backup/full`.

### 5. Logging & State
- Per-run JSON: `{id, started, ended, steps: [{name,status,output,duration}], exit:0}`.
- Aggregates: `last_success`, `fail_count`.
- Rotate old logs (keep 30 days).

## User Stories

1. **Daily Use (TUI):** `dzworkflows` → Browse → Run `backup/mysql-daily` → Watch progress/logs.
2. **Debug:** Select task → Preview bash/YAML → Edit → Re-run.
3. **Automation:** Add to crontab: `dzworkflows run deploy/staging`.
4. **Search/Scale:** 100+ workflows? Global fuzzy search.

## Tech Stack & MVP Scope

**MVP (4-6 weeks solo):**
- Rust: `clap` (CLI), `ratatui` + `crossterm` (TUI), `serde_yaml/serde_json` (parse/log), `walkdir` (scan), `std::process::Command` (exec bash).
- No async initially (seq exec); add `tokio` for parallel later.
- Config: TOML in `~/.config/dzworkflows/config.toml` (hooks, themes).

**Dependencies (minimal):**
```
clap = { version = "4", features = ["derive"] }
ratatui = "0.28"
crossterm = "0.28"
serde = { version = "1", features = ["derive"] }
serde_yaml = "0.9"
serde_json = "1"
walkdir = "2"
anyhow = "1"
```

**Build/Install:** `cargo install --path .`; binary `dzworkflows`.

## Success Metrics
- 10+ workflows onboarded Week 1.
- Cron jobs stable (no hangs).
- TUI responsive (<100ms redraw).
- Open-source: GitHub stars, issues.

## Risks & Mitigations
- Bash exec security: Sanitize args, no `eval`; user owns scripts.
- YAML complexity: Limit deps (no cycles), validate schema.
- Perf: Lazy scan (cache tree).


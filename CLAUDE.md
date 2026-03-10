# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

```bash
cargo build                  # Build debug binary
cargo test                   # Run all tests (unit + integration)
cargo test core::parser      # Run tests in a specific module
cargo test test_cycle        # Run a specific test by name
cargo run                    # Launch TUI (default, no subcommand)
cargo run -- --dir <path> list          # CLI: list tasks in custom dir
cargo run -- --dir <path> run backup/db-full --dry-run   # CLI: dry-run a task
```

Test fixtures are in `tests/fixtures/` — integration tests use `assert_cmd` to run the binary as a subprocess with `tempfile::TempDir` for isolation.

## Architecture

**dzworkflows** is a file-based workflow orchestrator. It scans `~/.config/dzworkflows/` where folders are categories and files are tasks (`.sh` = bash scripts, `.yaml` = multi-step DAG workflows). Two modes: interactive TUI (Ratatui) and non-interactive CLI for cron/automation.

### Execution pipeline

1. **Discovery** (`core/discovery.rs`): walkdir scans workflows dir (max depth 2), skips `logs/` and `config.toml`, groups files into `Category → Vec<Task>`
2. **Parsing** (`core/parser.rs`): YAML → `Workflow` with DAG validation + cycle detection (Kahn's algorithm). Shell scripts are wrapped as single-step workflows.
3. **Template expansion** (`core/template.rs`): `{{date}}`, `{{datetime}}`, `{{hostname}}`, custom vars from env
4. **Execution** (`core/executor.rs`): Topological sort, sequential `bash -c` execution, captures stdout/stderr per step. Failed steps cause dependents to be skipped; independent branches continue.
5. **Logging** (`core/logger.rs`): JSON files in `{workflows_dir}/logs/`, auto-rotated by age on startup

### Entry point dispatch

`main.rs` parses CLI args (clap derive). With a subcommand (`run`, `list`, `status`, `logs`) → dispatches to `cli/mod.rs`. No subcommand → launches TUI via `tui/mod.rs`.

### TUI state machine

`tui/app.rs` manages modal state: `AppMode` (Normal, Running, ViewingLogs, Search) × `Focus` (Sidebar, TaskList, Details). The event loop in `tui/mod.rs` polls crossterm at 250ms ticks. The `e` key spawns `$EDITOR` after restoring the terminal.

### Task references

Tasks can be referenced as `category/task` or `category.task` (dot notation normalized to slash). Resolution is in `core/discovery.rs::resolve_task_ref`.

### Error handling

`error.rs` defines `DzError` enum with variants for each failure domain and `From` impls for automatic conversion. All fallible functions return `error::Result<T>`.

## Config

Optional `~/.config/dzworkflows/config.toml` with fields: `workflows_dir`, `log_retention_days` (default 30), `editor` (default `$EDITOR`/vi), `hooks` (pre_run/post_run). Falls back to defaults if missing. The `--dir` CLI flag overrides `workflows_dir`.

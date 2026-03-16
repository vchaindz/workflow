# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

```bash
cargo build                  # Build debug binary
cargo build --release        # Build optimized binary
cargo build --features mcp   # Build with MCP (Model Context Protocol) support
cargo test                   # Run all tests (unit + integration)
cargo test --features mcp    # Run all tests including MCP integration tests
cargo test core::parser      # Run tests in a specific module
cargo test test_cycle        # Run a specific test by name
cargo run                    # Launch TUI (default, no subcommand)
cargo run -- --dir <path> list          # CLI: list tasks in custom dir
cargo run -- --dir <path> run backup/db-full --dry-run   # CLI: dry-run a task
```

Test fixtures are in `tests/fixtures/` — integration tests use `assert_cmd` to run the binary as a subprocess with `tempfile::TempDir` for isolation.

## Architecture

**workflow** is a file-based workflow orchestrator. It scans `~/.config/workflow/` where folders are categories and files are tasks (`.sh` = bash scripts, `.yaml` = multi-step DAG workflows). Two modes: interactive TUI (Ratatui) and non-interactive CLI for cron/automation.

### Execution pipeline

1. **Discovery** (`core/discovery.rs`): walkdir scans workflows dir (max depth 2), skips `logs/` and `config.toml`, groups files into `Category → Vec<Task>`
2. **Parsing** (`core/parser.rs`): YAML → `Workflow` with DAG validation + cycle detection (Kahn's algorithm). Shell scripts are wrapped as single-step workflows.
3. **Template expansion** (`core/template.rs`): `{{date}}`, `{{datetime}}`, `{{hostname}}`, custom vars from env, step output captures (`{{step_id.var}}`)
4. **Dangerous command check** (`core/executor.rs`): `check_dangerous()` blocks known destructive patterns (rm -rf /, fork bombs, dd to devices, etc.). Bypassed with `--force` flag.
5. **MCP step dispatch** (`core/mcp.rs`, feature-gated): For `mcp:` steps, resolves server config (alias lookup from `Config.mcp_servers` or inline definition), injects secrets as env vars, spawns `McpClient` (stdio transport via `rmcp`), calls the specified tool, and captures the result text as stdout. MCP steps participate in the same DAG features as `cmd:` steps (retries, timeouts, template vars, output capture).
6. **Execution** (`core/executor.rs`): Topological sort, sequential `bash -c` execution for `cmd:` steps, MCP client calls for `mcp:` steps. Captures stdout/stderr per step. Failed steps cause dependents to be skipped; independent branches continue. After main steps, `cleanup` steps run unconditionally (failures logged but don't affect exit code). Step outputs are captured via regex patterns and injected as template variables.
7. **Logging** (`core/logger.rs`): JSON files in `{workflows_dir}/logs/`, auto-rotated by age on startup
8. **History** (`core/db.rs`): SQLite database (`history.db`) for persistent run tracking, global stats, and overdue task detection
9. **Notifications** (`core/notify/`): Trait-based notification dispatch via `MultiNotifier`. URL-scheme resolver constructs `Box<dyn Notifier>` from config strings. Native HTTP via `ureq` (no curl dependency). Supports multi-target routing, severity-based channel config, retry with exponential backoff, and per-service rate limiting. Failures are logged but never block execution.

### Entry point dispatch

`main.rs` parses CLI args (clap derive). With a subcommand (`run`, `list`, `status`, `compare`, `validate`, `export`, `import`, `templates`, `logs`, `ai-update`, `sync`, `mcp`) → dispatches to `cli/mod.rs`. No subcommand → launches TUI via `tui/mod.rs`.

### TUI state machine

`tui/app.rs` manages modal state: `AppMode` (Normal, Running, ViewingLogs, Search, Help, Comparing, Wizard, ConfirmDelete, RecentRuns, SavedTasks, OverdueReminder) × `Focus` (Sidebar, TaskList, Details). The event loop in `tui/mod.rs` polls crossterm at 250ms ticks. The `e` key spawns `$EDITOR` after restoring the terminal.

### Wizard system

`tui/actions.rs` handles five wizard modes, each triggered by a TUI keybinding:

- **FromHistory** (`w`): reads shell history via `core/history.rs` (zsh, bash, fish), presents filterable/selectable list, auto-suggests category and task name from command patterns
- **AiChat** (`a`): takes a natural-language prompt, invokes `claude -p`, `codex exec`, or `gemini -p` via `core/ai.rs` in a background thread, parses structured response (TASK_NAME/CATEGORY/commands)
- **AiUpdate** (`A`): reads existing task YAML, takes user instructions, sends both to AI, returns updated YAML. Flow: AiPrompt → AiThinking → Preview (skips Category/TaskName)
- **FromTemplate** (`t`): browses bundled + cached templates from `core/catalog.rs`, supports variable substitution before saving
- **CloneTask** (`W`): clones selected task, optionally strips failed/skipped steps and parallelizes via `core/wizard.rs`

All wizard modes flow through stages: input → Category → TaskName → Options → Preview → save via `core/wizard.rs::save_task()`. AiUpdate mode skips Category/TaskName and overwrites the original file directly.

Both AiChat and AiUpdate modes support an **AI refinement loop** at the Preview stage. Press `r` to enter `AiRefinePrompt`, type refinement instructions, and press Enter — the existing YAML (from `ai_updated_yaml` or generated from `ai_commands`) is sent to `invoke_ai_update()` along with the instructions, and the result replaces `ai_updated_yaml`. Multiple rounds are supported. The `ai_refine_prompt` field on `WizardState` holds the current input; `drain_ai_events()` handles the `AiResult::Yaml` response path (same as AiUpdate).

### AI integration

`core/ai.rs` detects `claude`, `codex`, or `gemini` on PATH, crafts a system prompt requesting structured output (TASK_NAME, CATEGORY, then shell commands only), and parses the response with heuristic filters to strip prose, markdown fencing, and numbered prefixes. The `invoke_ai_raw()` variant is used for free-form AI analysis (e.g., `compare --ai`). The `invoke_ai_update()` function sends existing YAML + user instructions and returns updated YAML via `parse_ai_yaml_response()`.

### Interactive/streaming command detection

`core/detect.rs` heuristically identifies commands that need inherited stdio (REPLs, TUI tools, streaming commands). Shell quotes are stripped before detection so commands like `ssh host 'journalctl -f'` are properly recognized. Detected commands run with the terminal suspended (TUI restores on Ctrl-C/exit). Steps can also be explicitly marked `interactive: true` in YAML to override detection.

### Run comparison

`core/compare.rs` diffs two `RunLog` entries step-by-step: timing deltas, status changes, output diffs. The `compare` CLI subcommand supports `--ai` for natural-language analysis.

### Template catalog

`core/catalog.rs` embeds bundled templates via `include_str!()` from `templates/` directory (39 bundled: 16 sysadmin, 10 docker, 10 kubectl, 3 mcp). Templates are YAML workflows with a `variables` section for substitution. `templates --fetch` downloads community templates from GitHub.

### Task references

Tasks can be referenced as `category/task` or `category.task` (dot notation normalized to slash). Resolution is in `core/discovery.rs::resolve_task_ref`.

### Overdue reminders

Tasks can declare an `overdue: <days>` field in their YAML. On TUI startup, `app.check_overdue()` queries the SQLite history for each task with an overdue threshold. If the task has never run or its last successful run exceeds the threshold, it appears in a popup modal (`AppMode::OverdueReminder`). The `OverdueTask` struct and `check_overdue_tasks()` query live in `core/db.rs`. Discovery extracts the `overdue` field via a lightweight `WorkflowMeta` deserialize in `core/discovery.rs`.

### Hot/cold task heat

Tasks are classified by run frequency in the last 30 days using `TaskHeat` enum (Hot ≥5, Warm 1–4, Cold 0). Heat data is loaded from SQLite via `db::get_task_heat()` at startup and on each rescan. The TUI shows heat indicators: `▲` (green/hot), `·` (warm), `▽` (blue/cold). Press `f` to toggle between alphabetical and heat-based sorting (hot tasks float to top). The `sort_by_heat` flag, `load_heat_data()`, `toggle_sort()`, and `apply_sort()` methods live in `tui/app.rs`.

### Content-aware search

TUI search (`/`) matches not only task and category names but also step commands. `App::build_step_cmd_cache()` pre-caches lowercased step content per task path (parsed YAML step commands or raw file content for shell scripts). The cache is rebuilt on startup and each rescan. `update_search()` checks this cache when name/category matching fails.

### Dangerous command detection

`executor::check_dangerous(cmd)` scans expanded commands for destructive patterns: `rm -rf /`, `dd` to block devices, `mkfs` on real devices, fork bombs, `chmod -R 777 /`, `mv /* /dev/null`, device output redirects. Returns a warning string if matched. Integration: before execution, if not `--force` and not dry-run, dangerous commands produce `StepStatus::Failed` with an explanatory message and emit `ExecutionEvent::DangerousCommand` for TUI display.

### Cleanup steps (finally block)

Workflows can declare a `cleanup:` section containing steps that run after the main step loop regardless of success or failure. Cleanup steps are normalized via `normalize_steps()` but excluded from DAG validation. In the executor, they run sequentially after the main loop. Cleanup failures are logged but do not affect the overall workflow exit code.

### Step output capture

Steps can declare `outputs:` with `name` and `pattern` (regex with capture group). After a successful step, each pattern is applied to stdout; the first capture group match is stored as `step_id.output_name` in `template_vars`. Subsequent steps can reference captured values via `{{step_id.output_name}}`.

### Fish shell history

`core/history.rs` supports fish shell history (`~/.local/share/fish/fish_history`) alongside zsh and bash. Fish format uses `- cmd: <command>` lines with optional `  when: <timestamp>` lines. The `parse_fish_history()` function handles fish's `\n` escape sequences for multiline commands.

### Git sync

`core/sync.rs` provides git-based syncing of the workflows directory across machines. `cli/sync.rs` handles the `sync` subcommand with actions: `init`, `clone`, `push`, `pull`, `status`, `setup`. The TUI exposes sync via `G` key in `tui/actions.rs`.

`create_private_repo()` uses `gh repo create` to create a private repo named `workflow-app-sync-repo` with `--source=.` and `--push`. `auto_commit()` stages all changes and generates a smart commit message from `git diff --name-status`. `get_status()` returns a `SyncInfo` struct with `SyncStatus` enum (Clean, Dirty, Ahead, Behind, Diverged, NoRemote, Offline). `push()` and `pull()` handle offline detection and merge conflict reporting.

Config lives in `[sync]` section of `config.toml` via `SyncConfig` in `core/config.rs`: `enabled`, `auto_commit`, `auto_push`, `auto_pull_on_start`, `branch`, `remote_url`.

### Notification system

`core/notify/` is a trait-based notification framework replacing the old shell-out approach. Key components:

- **Trait** (`mod.rs`): `Notifier` trait with `name()` and `send(&Notification)`. `MultiNotifier` fans out to all registered backends, collecting errors without short-circuiting.
- **Message** (`message.rs`): `Notification` struct (subject, body, severity, fields) and `Severity` enum (Success, Failure, Warning, Info).
- **Resolver** (`resolve.rs`): `resolve_notifier(url)` parses URL-scheme strings (`slack://`, `discord://`, `telegram://`, `teams://`, `ntfy://`, `gotify://`, `webhook://`, `email://`) into `Box<dyn Notifier>`. Environment variable references (`$VAR`) are expanded.
- **Backends**: Each in its own file, gated behind cargo feature flags. Uses `ureq` for HTTP (synchronous, no tokio) and `lettre` for SMTP email. Default features: slack, discord, webhook, ntfy, telegram, email, mattermost. Optional: msteams, gotify.
- **Retry** (`retry.rs`): Exponential backoff wrapper with configurable max_attempts, initial_delay, and backoff_factor.
- **Rate limiting** (`rate_limit.rs`): Per-service sliding-window rate limiter with sensible defaults (Discord 30/min, Telegram 30/sec).
- **Rich formatting**: Each backend uses service-native formatting (Slack Block Kit, Discord Embeds, Teams Adaptive Cards, Telegram MarkdownV2).
- **Config**: `NotifyConfig` in `models.rs` supports single string or array for `on_failure`/`on_success`, plus `channels` for severity-based routing. Per-workflow config merges with global `config.toml` by default; `notify_override: true` replaces.

### Error handling

`error.rs` defines `DzError` enum with variants for each failure domain and `From` impls for automatic conversion. All fallible functions return `error::Result<T>`.

## Key source files

| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point, CLI arg parsing (clap) |
| `src/cli/args.rs` | CLI subcommand definitions |
| `src/cli/mod.rs` | CLI dispatch |
| `src/cli/ai_update.rs` | CLI handler for `ai-update` subcommand |
| `src/cli/sync.rs` | CLI handler for `sync` subcommand |
| `src/cli/mcp.rs` | CLI subcommands: list-tools, call, check (feature-gated) |
| `src/core/sync.rs` | Git sync operations (init, push, pull, status) |
| `src/core/mcp.rs` | MCP client: spawn, initialize, list_tools, call_tool, teardown (feature-gated) |
| `src/core/discovery.rs` | Workflow file discovery |
| `src/core/parser.rs` | YAML parsing + DAG validation |
| `src/core/executor.rs` | Step execution engine |
| `src/core/history.rs` | Shell history parsing (zsh/bash/fish) |
| `src/core/ai.rs` | AI tool detection + invocation |
| `src/core/wizard.rs` | Workflow generation + optimization |
| `src/core/catalog.rs` | Template catalog (bundled + fetch) |
| `src/core/compare.rs` | Run comparison |
| `src/core/db.rs` | SQLite history database |
| `src/core/notify/mod.rs` | Notifier trait, MultiNotifier |
| `src/core/notify/resolve.rs` | URL-scheme → Notifier resolver |
| `src/core/notify/message.rs` | Notification struct, Severity enum |
| `src/core/notify/slack.rs` | Slack webhook backend |
| `src/core/notify/discord.rs` | Discord webhook backend |
| `src/core/notify/telegram.rs` | Telegram Bot API backend |
| `src/core/notify/msteams.rs` | MS Teams Adaptive Cards backend |
| `src/core/notify/ntfy.rs` | ntfy push notification backend |
| `src/core/notify/gotify.rs` | Gotify push notification backend |
| `src/core/notify/webhook.rs` | Generic webhook backend |
| `src/core/notify/mattermost.rs` | Mattermost webhook backend |
| `src/core/notify/email.rs` | Email via lettre/SMTP backend |
| `src/core/notify/retry.rs` | Retry with exponential backoff |
| `src/core/notify/rate_limit.rs` | Per-service rate limiting |
| `src/core/config.rs` | Config file parsing |
| `src/tui/app.rs` | TUI state (AppMode, Focus, WizardState) |
| `src/tui/actions.rs` | TUI keybinding handlers + wizard logic |
| `src/tui/ui.rs` | TUI rendering |
| `src/error.rs` | Error types |

## Config

Optional `~/.config/workflow/config.toml` with fields: `workflows_dir`, `log_retention_days` (default 30), `editor` (default `$EDITOR`/vi), `hooks` (pre_run/post_run), `[sync]` section for git sync, `[mcp.servers.*]` sections for MCP server aliases. Falls back to defaults if missing. The `--dir` CLI flag overrides `workflows_dir`.

MCP server aliases are defined as `[mcp.servers.<alias>]` TOML tables with fields: `command` (required), `env` (optional HashMap), `secrets` (optional Vec of secret names from the secrets store), `timeout` (optional u64 in seconds). Workflows reference aliases via `server: <alias>` in `mcp:` steps, or use inline `server: { command: "..." }` definitions.

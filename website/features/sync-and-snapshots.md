# Sync, Snapshots, and Portability

Tools for keeping workflows consistent across machines, tracking baseline values, and moving workflow libraries between environments.

## Git sync

Sync workflow definitions via a private GitHub repo. Logs, `history.db`, and `config.local.toml` stay local.

### Setup

```bash
workflow sync setup
```

This creates a private GitHub repo (requires `gh` CLI), initializes the workflows directory as a git repo, and enables auto-sync. After setup, changes auto-commit and push. The TUI pulls on startup.

### CLI commands

```bash
workflow sync init                       # initialize git in workflows dir
workflow sync clone <url>                # clone an existing workflow repo
workflow sync push                       # auto-commit and push
workflow sync pull                       # pull latest
workflow sync status                     # show sync state
workflow sync setup                      # one-time full setup
workflow sync branch                     # list branches (* marks current)
workflow sync branch customer-acme       # switch branch (creates if needed)
```

In the TUI, press `G` for sync controls.

### Branch switching

Each git branch holds a complete workflow library. Switching branches swaps your entire set of workflows in one command.

```bash
workflow sync branch customer-acme
```

Dirty changes are auto-committed before switching. Workflows are rescanned automatically for the new branch content.

Use cases:

- **MSP per-client runbooks** -- one branch per customer, each with their own set of workflows
- **Environment separation** -- dev, staging, and prod workflow sets in the same repo
- **Team libraries** -- shared workflows per team or department

### Configuration

```toml
[sync]
enabled = true
auto_commit = true
auto_push = true
auto_pull_on_start = true
```

### What syncs and what stays local

| Synced | Local only |
|--------|------------|
| Workflow `.sh` and `.yaml` files | `logs/` directory |
| Category folders | `history.db` |
| `config.toml` | `config.local.toml` |

Use `config.local.toml` for machine-specific overrides (different editor, custom paths) that should not be shared.

## Snapshots

A key-value store in SQLite for baseline comparisons. Workflows often need to verify that something has not changed -- a web page hash, a config fingerprint, an API response shape. Snapshots provide a generic mechanism for this.

The pattern: **first run auto-learns** a baseline, subsequent runs compare against it.

### Example: page drift check

```yaml
name: Page Drift Check
steps:
  - id: fingerprint
    cmd: curl -s https://example.com | sha256sum | cut -d' ' -f1
    outputs:
      - name: hash
        pattern: '^(\S+)'

  - id: baseline-check
    needs: [fingerprint]
    cmd: |
      EXISTING=$(workflow snapshot get "{{task_ref}}" content-hash 2>/dev/null || true)
      if [ -z "$EXISTING" ]; then
        echo "First run -- storing baseline"
        workflow snapshot set "{{task_ref}}" content-hash "{{fingerprint.hash}}"
      elif [ "$EXISTING" != "{{fingerprint.hash}}" ]; then
        echo "DRIFT: $EXISTING -> {{fingerprint.hash}}"
        exit 1
      else
        echo "OK: matches baseline"
      fi
```

### CLI commands

```bash
workflow snapshot set backup/db-full baseline '{"hash":"abc123"}'
workflow snapshot get backup/db-full baseline
workflow snapshot delete backup/db-full baseline
workflow snapshot list
workflow snapshot list backup/db-full        # filter by task
workflow snapshot list --json
```

The `get` subcommand prints the raw value to stdout (exits 1 if not found), so it works with shell capture:

```bash
HASH=$(workflow snapshot get mycat/mytask content-hash)
```

The `set` subcommand reads from stdin when no value argument is provided:

```bash
echo '{"k":"v"}' | workflow snapshot set task key
```

Snapshots are stored in `history.db` alongside run history. No extra files, no git noise.

::: tip
Reset a baseline anytime with `workflow snapshot delete`. The next run will re-learn it automatically if your workflow follows the first-run pattern above.
:::

## Export and import

Move workflow libraries between machines as compressed archives.

```bash
# Export
workflow export -o my-workflows.tar.gz
workflow export -o my-workflows.tar.gz --include-history

# Import
workflow import my-workflows.tar.gz --overwrite
workflow import my-workflows.tar.gz --skip-existing
```

The `--include-history` flag bundles run history and snapshots from `history.db` alongside the workflow files. Without it, only `.sh` and `.yaml` files are exported.

Import validates archive paths to prevent directory traversal attacks.

## Trash (soft delete)

Nothing is permanently deleted by default. Deleted workflows move to a timestamped `.trash/` directory.

```bash
workflow trash list                      # see trashed files with timestamps
workflow trash restore db-full.yaml      # put it back
workflow trash empty                     # permanently delete when ready
```

In the TUI, press `D` to soft-delete the selected task. Files can always be restored until you explicitly empty the trash.

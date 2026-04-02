# Quick start

No init command, no project file, no configuration. Every `.sh` and `.yaml` file in `~/.config/workflow/` is automatically discovered and organized by folder.

## Create your first task

A task can be a plain shell script. Create a category folder and drop a script into it:

```bash
mkdir -p ~/.config/workflow/backup

cat > ~/.config/workflow/backup/db-full.sh << 'EOF'
#!/bin/bash
pg_dump mydb > /tmp/mydb_$(date +%Y%m%d).sql
echo "Backup complete"
EOF
```

## Run it

```bash
workflow run backup/db-full
```

That is all. The script runs, output is captured, and the result is logged to SQLite for history tracking.

## Browse interactively

Launch the TUI with no arguments:

```bash
workflow
```

```text
 workflow v0.4.2 -- 12 workflows -- 48 runs -- 2 failed

 Categories  Tasks                    Details
 > backup    ^ db-full    + 2d [sh]   #!/bin/bash
   deploy    . mysql-daily x 5h [yml] pg_dump mydb > /tmp/mydb_$DATE.sql
   docker    v s3-sync       [yml]    echo "Backup complete"
   k8s

 Log
 [14:32:01] > dump -- mysqldump --all-databases > /tmp/db.sql
 [14:32:03] + dump (1850ms)

 r:run  d:dry-run  e:edit  w:new  a:ai  t:template  /:search  q:quit
```

Navigate with `j`/`k` or arrow keys, switch panes with `Tab`, and press `r` to run the selected task. See [TUI reference](/guide/tui) for the full keybinding table.

## Project-local workflows

Drop a `.workflow/` directory into any project and its workflows are discovered alongside your global ones when you run `workflow` from that directory:

```
myproject/
├── .workflow/
│   ├── ci/
│   │   └── build.yaml
│   └── dev/
│       └── seed-db.sh
├── src/
└── ...
```

Project-local workflows appear in the TUI and CLI just like global ones --- no flags needed. This lets you version-control project-specific automation alongside the code it operates on.

## First YAML workflow

For multi-step tasks with dependencies, retries, and cleanup, use a YAML file:

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
cleanup:
  - id: remove-tmpfiles
    cmd: rm -f /tmp/db.sql /tmp/db.sql.gz
```

Save this as `~/.config/workflow/backup/mysql-daily.yaml` and run it:

```bash
workflow run backup/mysql-daily
```

Steps execute in dependency order: dump, then compress, then upload. If upload fails, it retries up to 3 times. The cleanup step runs regardless of success or failure, removing temporary files.

::: tip
Use `--dry-run` to preview what would execute without actually running anything:
```bash
workflow run backup/mysql-daily --dry-run
```
:::

## What to explore next

- [Workflows](/guide/workflows) --- YAML format deep-dive: DAG dependencies, sub-workflows, branching, output capture, loops, and more
- [TUI reference](/guide/tui) --- keybindings, search, heat sorting, wizards, and secrets management
- [Templates](/guide/quick-start) --- press `t` in the TUI to browse 56 bundled templates covering sysadmin, Docker, Kubernetes, and patching
- [AI integration](/guide/ai-integration) --- generate and refine workflows using natural language
- [Configuration](/guide/configuration) --- customize paths, editor, hooks, and notification targets

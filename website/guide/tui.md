# TUI reference

## Overview

Launch the TUI with no arguments:

```bash
workflow
```

The interface is a modal, three-pane layout:

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

- **Sidebar** (left) --- category list
- **Task list** (center) --- tasks in the selected category with status indicators
- **Details** (right) --- file contents, step definitions, or run output

## Keybindings

| Key | Action |
|-----|--------|
| `j`/`k` or arrows | Navigate within current pane |
| `Tab` / `h`/`l` | Switch between panes |
| `r` | Run selected task |
| `d` | Dry-run (preview without executing) |
| `e` | Open in `$EDITOR` |
| `/` | Search tasks and step commands |
| `f` | Toggle heat sort (hot tasks first) |
| `F` | Cycle status filter (All / Failed / Overdue / Never-run) |
| `w` | New task from shell history |
| `a` | New task via AI (or AI fix when error visible) |
| `A` | AI-update selected task |
| `t` | New task from template catalog |
| `W` | Clone and optimize selected task |
| `n` | Rename task or category |
| `D` | Delete (soft --- moves to `.trash/`) |
| `R` | Recent runs (last 10 across all tasks) |
| `S` | Toggle bookmark on selected task |
| `s` | View bookmarked tasks |
| `L` | View run logs |
| `M` | Memory view (anomaly detection, baselines, trends) |
| `K` | Secrets manager (add / view / edit / delete) |
| `G` | Git sync controls |
| `c` | Compare last two runs |
| `-`/`+` | Collapse / expand JSON blocks in detail pane |
| `Z` | Toggle fold / unfold all JSON blocks |
| `{`/`}` | Jump between JSON blocks |
| `?` | Help (context-sensitive) |
| `q` | Quit |

## Heat indicators

Tasks are classified by run frequency in the last 30 days:

| Indicator | Label | Criteria |
|-----------|-------|----------|
| `^` (green) | Hot | 5 or more runs in 30 days |
| `.` | Warm | 1--4 runs in 30 days |
| `v` (blue) | Cold | 0 runs in 30 days |

Press `f` to toggle between alphabetical and heat-based sorting (hot tasks float to the top). Press `F` to cycle through status filters: All, Failed, Overdue, Never-run.

## Search

Press `/` to open the search bar. Search matches against:

- Task names
- Category names
- Step commands within YAML workflows and shell scripts

The step command cache is pre-built at startup and refreshed on each rescan, so search results are instant.

## Shell history wizard

Press `w` to create a new task from your shell history. workflow reads history from zsh, bash, and fish.

```text
+-- New Task from History ----------------------------------------+
| Filter: docker                                                  |
|                                                                 |
|   [x] docker compose up -d                    2h ago            |
|   [ ] docker ps --format "table {{.Names}}"   3h ago            |
|   [x] docker logs -f webapp                   5h ago            |
|                                                                 |
| Space: toggle  Enter: continue  /: filter  Esc: cancel          |
+-----------------------------------------------------------------+
```

Select commands with Space, press Enter to continue. The wizard auto-suggests a category based on command patterns --- docker commands go under `docker/`, kubectl commands under `k8s/`.

## Overdue reminders

Tasks with an `overdue: N` field (days) trigger a popup on TUI startup when they have not been run within the threshold:

```text
+------------- Overdue Tasks --------------------+
|  ! backup/db-full           3 day(s) overdue   |
|  ! monitoring/disk-check    7 day(s) overdue   |
|                                                 |
|  Enter: jump to task  Esc: dismiss              |
+-------------------------------------------------+
```

::: tip
Add `overdue: 7` to any maintenance task you want to be reminded about. The reminder is based on the last successful run recorded in SQLite.
:::

## Recent runs

Press `R` to see the last 10 runs across all tasks, with timing and status. Select a run to view its full output.

## Bookmarks

Press `S` to toggle a bookmark on the selected task. Press `s` to view only bookmarked tasks. Bookmarks are stored in `config.toml`.

## Secrets manager

Press `K` to open the secrets manager without leaving the TUI:

- Browse all stored secret names
- `a` --- add a new secret (value input is masked)
- `v` or Enter --- decrypt and reveal a secret value
- `e` --- edit an existing secret
- `d` --- delete a secret with confirmation

If the secrets store has not been initialized, the modal offers to set it up using your SSH key.

## Git sync

Press `G` to access sync controls: push, pull, view status, or switch branches. Dirty changes are auto-committed before a branch switch, and workflows are rescanned for the new branch content. See [configuration](/guide/configuration) for sync setup.

## Run comparison

Press `c` to compare the last two runs of the selected task side-by-side. Displays timing deltas, status changes, and output diffs. For AI-powered analysis, use the CLI: `workflow compare backup/db-full --ai`.

## JSON folding

When the detail pane shows JSON output:

- `-` / `+` --- collapse or expand the current block
- `Z` --- toggle fold/unfold all blocks
- `{` / `}` --- jump to the previous/next block boundary

## Help

Press `?` for context-sensitive help. The help overlay shows keybindings relevant to the current mode (normal, running, wizard, etc.).

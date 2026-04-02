# Workflow Memory

Every run is automatically profiled. After 5 or more executions of a task, `workflow` builds statistical baselines and flags anomalies. No configuration needed.

## Detection methods

### Duration spikes

Uses Modified Z-score (MAD-based, robust to outliers) to detect steps or workflows that take significantly longer than usual.

| Threshold | Severity |
|-----------|----------|
| > 1.5 sigma | Info |
| > 2.0 sigma | Warning |
| > 3.0 sigma | Critical |

### New failures

A task with a 90% or higher success rate suddenly fails. This catches regressions in previously stable workflows.

### Flapping

Three or more pass/fail transitions within 6 consecutive runs. Indicates an intermittent or environment-dependent issue.

### Output drift

Step output changed when it was previously stable, detected via FNV-1a fingerprinting. Useful for catching unexpected changes in command output.

### Metric shifts

Extracted numeric values (MB, percentages, counts) deviate from their established baseline. Requires steps with `outputs:` patterns that capture numeric data.

## Automatic post-run output

Anomalies are reported automatically after every `workflow run`:

```text
success: run logged to database
  warning: 1 anomalies detected: 1 warning
    [warning] step 'backup' 892ms (baseline 145ms +/-23ms, z=3.2)
```

In the TUI, anomaly summaries appear in the footer after a run completes.

## TUI view

Press `M` on any task to open the memory view. It displays:

- Health scores
- Statistical baselines per step
- Duration trends
- Recent anomalies

## CLI commands

All commands support `--json` for machine-readable output.

```bash
# Health scores for all tasks
workflow memory health

# Recent anomalies (all tasks)
workflow memory anomalies

# Anomalies for a specific task
workflow memory anomalies backup/db-full

# Statistical baselines
workflow memory baseline backup/db-full

# Duration trend (30 days)
workflow memory trends backup/db-full

# Custom metric trend
workflow memory trends backup/db-full --metric "Disk Used:%"

# Acknowledge anomalies (removes from active alerts)
workflow memory ack all --task backup/db-full

# Recompute all baselines from stored data
workflow memory recompute
```

## Architecture

### Short-term memory

In-process cache in the TUI for instant display. Populated from SQLite on startup and updated after each run.

### Long-term memory

Four SQLite tables in `history.db`:

| Table | Purpose |
|-------|---------|
| `memory_baselines` | Rolling statistical baselines per task/step/metric |
| `memory_metrics` | Materialized metric extractions per run |
| `memory_anomalies` | Detected anomaly events with severity |
| `memory_trends` | Daily and weekly rollups |

Baselines are recomputed from the last 50 runs after every execution using `statrs` for percentiles and standard deviation. Anomaly detection activates only after a minimum of 5 data points.

Memory tables are automatically rotated alongside run history, respecting the `log_retention_days` setting in `config.toml`.

::: tip
Use `workflow memory recompute` after importing historical data or changing retention settings to rebuild all baselines from scratch.
:::

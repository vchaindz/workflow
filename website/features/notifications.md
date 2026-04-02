# Notifications

Send notifications to 9 services -- Slack, Discord, Mattermost, Telegram, Microsoft Teams, ntfy, Gotify, generic webhooks, and email -- all via native HTTP. No `curl`, `mail`, or external dependencies required. Each backend is gated behind a cargo feature flag so you only compile what you need.

Failures are logged but never block workflow execution.

## Supported services

| Scheme | Service | Rich format |
|--------|---------|-------------|
| `slack://WEBHOOK_URL` | Slack | Block Kit with colored sidebar |
| `discord://WEBHOOK_URL` | Discord | Embeds with severity colors and fields |
| `telegram://BOT_TOKEN@CHAT_ID` | Telegram | MarkdownV2 with severity icons |
| `teams://WEBHOOK_URL` | Microsoft Teams | Adaptive Cards |
| `ntfy://SERVER/TOPIC` | ntfy | Priority-mapped push notifications |
| `gotify://SERVER?token=TOKEN` | Gotify | Priority-mapped push notifications |
| `webhook://URL` | Generic webhook | JSON body with all fields |
| `mattermost://SERVER/hooks/ID` | Mattermost | Slack-compatible attachments with fields |
| `email://USER@HOST?smtp=...&port=...` | Email (SMTP) | Formatted email via `lettre` |

Environment variables (`$VAR`) are expanded in all URLs. For example, `slack://$SLACK_WEBHOOK` resolves the URL from your environment at runtime.

## Configuration

### Array of targets

The most common pattern -- send to multiple services on failure, a single service on success:

```yaml
notify:
  on_failure:
    - "slack://https://hooks.slack.com/services/T00/B00/xxx"
    - "telegram://$TELEGRAM_BOT_TOKEN@$TELEGRAM_CHAT_ID"
    - "ntfy://ntfy.sh/ops-alerts"
  on_success:
    - "webhook://https://status.example.com/api/deploy"
  env:
    environment: production
    team: platform
```

### Severity-based routing

Use `channels:` for fine-grained control over which services receive which severity levels:

```yaml
notify:
  channels:
    - target: "slack://https://hooks.slack.com/..."
      on: [failure, warning]
    - target: "ntfy://ntfy.sh/ops-info"
      on: [success, failure, warning]
```

### Single-string shorthand

For backward compatibility, a single string is also accepted:

```yaml
notify:
  on_failure: "slack://https://hooks.slack.com/services/T00/B00/xxx"
```

## Template variables

Notification messages have access to these template variables:

| Variable | Description |
|----------|-------------|
| `{{task_ref}}` | Task identity (e.g. `backup/db-full`) |
| `{{workflow_name}}` | Workflow display name |
| `{{hostname}}` | Machine hostname |
| `{{status}}` | Overall run status |
| `{{exit_code}}` | Exit code of the run |
| `{{failed_steps}}` | Comma-separated list of failed step IDs |
| `{{duration_ms}}` | Total run duration in milliseconds |
| `{{timestamp}}` | ISO 8601 timestamp |

Any keys defined in `notify.env` are also available as template variables.

## Global defaults and per-workflow merging

Set default notification targets in `config.toml`:

```toml
[notify]
on_failure = "slack://https://hooks.slack.com/services/..."
```

Per-workflow `notify:` blocks merge with these global defaults. If you want a workflow to completely replace the global config instead of merging, set `notify_override: true`:

```yaml
notify:
  notify_override: true
  on_failure:
    - "ntfy://ntfy.sh/this-task-only"
```

## Retry and rate limiting

All notification backends include automatic retry with exponential backoff. If the first attempt fails, subsequent attempts use increasing delays before giving up.

Per-service rate limiting prevents flooding. Each backend has sensible defaults (for example, Discord allows 30 messages per minute, Telegram 30 per second). Rate-limited messages are queued, not dropped.

## Cargo feature flags

Default features: `slack`, `discord`, `webhook`, `ntfy`, `telegram`, `email`, `mattermost`.

Optional features (not included by default): `msteams`, `gotify`.

::: code-group

```bash [All defaults]
cargo build --release
```

```bash [No notification backends]
cargo build --release --no-default-features
```

```bash [Specific backends only]
cargo build --release --no-default-features --features "slack,ntfy"
```

```bash [Include optional backends]
cargo build --release --features "msteams,gotify"
```

:::

::: tip
If you only use one or two notification services, building with just those features produces a smaller binary.
:::

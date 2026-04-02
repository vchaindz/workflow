# Webhook Server

`workflow serve` exposes a REST API for triggering workflows from CI pipelines, monitoring alerts, chatbots, or any HTTP client.

## Starting the server

```bash
workflow serve                         # default: port 8080, bind 127.0.0.1
workflow serve --port 9090 --bind 0.0.0.0
```

An auto-generated Bearer token is printed to stdout at startup. Save it -- all requests require this token.

## Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Health check |
| `GET` | `/tasks` | List all available tasks |
| `POST` | `/run/<category>/<task>` | Trigger workflow (returns 202 + run_id) |
| `GET` | `/status/<run_id>` | Poll run status |

## Authentication

Every request (except `/health`) must include the Bearer token:

```bash
curl -X POST http://localhost:8080/run/backup/db-full \
  -H "Authorization: Bearer <token>" \
  -H "X-Workflow-Client: curl" \
  -d '{"env": {"TARGET": "production"}}'
```

The `X-Workflow-Client` header is required for CSRF protection. Set it to any non-empty string identifying the caller.

## Request body

`POST /run` accepts an optional JSON body to inject environment variables into the workflow:

```json
{
  "env": {
    "TARGET": "production",
    "NOTIFY": "true"
  }
}
```

Maximum body size is 1 MB.

## Triggering and polling

The `/run` endpoint returns `202 Accepted` immediately with a `run_id`. Use `/status/<run_id>` to poll for completion:

```bash
# Trigger
RUN_ID=$(curl -s -X POST http://localhost:8080/run/backup/db-full \
  -H "Authorization: Bearer $TOKEN" \
  -H "X-Workflow-Client: ci" | jq -r '.run_id')

# Poll
curl -s http://localhost:8080/status/$RUN_ID \
  -H "Authorization: Bearer $TOKEN"
```

## Concurrency

The server limits concurrent workflow runs to prevent resource exhaustion. The default is 4 concurrent runs.

## Configuration

```toml
[server]
port = 8080
max_concurrent_runs = 4
```

## Use cases

**CI pipeline triggers** -- POST to `/run` from a GitHub Action or GitLab CI job to execute deployment or validation workflows after a successful build.

**Monitoring alert hooks** -- Configure your alerting system (Prometheus Alertmanager, Grafana, Uptime Kuma) to POST to `/run` when an alert fires, triggering an automated remediation workflow.

**Chatbot integrations** -- Wire a Slack bot or Discord bot to trigger workflows via HTTP, giving your team a chat-based operations interface.

::: warning
When binding to `0.0.0.0`, the server is accessible from the network. Ensure the auto-generated token is kept secret, and consider placing the server behind a reverse proxy with TLS in production.
:::

## Exit codes

The `workflow serve` process itself returns 0 on clean shutdown. Individual workflow runs report their status through the `/status` endpoint.

For non-server usage, `workflow run` returns 0 on success and non-zero on failure, making it suitable for cron jobs and CI pipelines.

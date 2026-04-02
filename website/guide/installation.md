# Installation

## Pre-built binary

Download the latest release for your platform from [GitHub Releases](https://github.com/vchaindz/workflow/releases). Extract the binary and place it somewhere on your `PATH`.

## Build from source

Requires Rust 1.56+ (2021 edition).

```bash
git clone https://github.com/vchaindz/workflow.git
cd workflow
cargo build --release
```

The binary is at `target/release/workflow`. Copy it to a directory on your `PATH`:

```bash
sudo cp target/release/workflow /usr/local/bin/
```

Or install directly via cargo:

```bash
cargo install --path .
```

## Feature flags

workflow uses cargo feature flags to control which notification backends and optional subsystems are compiled in. This keeps the base binary small while letting you opt into what you need.

| Feature | Default | Backend | Dependency |
|---------|---------|---------|------------|
| `slack` | Yes | Slack webhooks | `ureq` |
| `discord` | Yes | Discord webhooks | `ureq` |
| `webhook` | Yes | Generic webhooks | `ureq` |
| `ntfy` | Yes | ntfy push notifications | `ureq` |
| `telegram` | Yes | Telegram Bot API | `ureq` |
| `email` | Yes | SMTP email | `lettre` |
| `mattermost` | Yes | Mattermost webhooks | `ureq` |
| `msteams` | No | Microsoft Teams Adaptive Cards | `ureq` |
| `gotify` | No | Gotify self-hosted push | `ureq` |
| `mcp` | No | Model Context Protocol | `rmcp`, `tokio`, `reqwest` |

## Build examples

::: code-group

```bash [All defaults]
cargo build --release
```

```bash [Add MCP support]
cargo build --release --features mcp
```

```bash [Minimal notifications]
cargo build --release --no-default-features --features "slack,ntfy"
```

```bash [No notifications]
cargo build --release --no-default-features
```

:::

::: tip
The `mcp` feature adds the `rmcp` and `tokio` runtime dependencies. If you do not need MCP tool integration, leave it off to reduce binary size and compile time.
:::

## Verify installation

```bash
workflow --version
```

Launch the TUI to confirm everything works:

```bash
workflow
```

If `~/.config/workflow/` does not exist yet, workflow creates it automatically on first run.

## Next steps

- [Quick start](/guide/quick-start) --- create and run your first workflow
- [Configuration](/guide/configuration) --- customize paths, editor, hooks, and notifications

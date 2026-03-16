# Notification Integration Plan for `workflow`

## Current State

`workflow` already has a lightweight notification system in `core/notify.rs` that resolves URL-scheme shortcuts (`slack://`, `webhook://`, `email://`) into shell-out `curl`/`mail` commands. It works but has real limitations: no native API calls (shells out to curl), no rich message formatting, no retry/rate-limiting, and adding a new service means writing another string-interpolation branch.

## Landscape Assessment

### Go: nikoksr/notify (the reference)
35 services, dead-simple `UseServices → AddReceivers → Send` pattern. Each service implements a `Notifier` interface with `AddReceivers(...)` and `Send(ctx, subject, message)`.

### Rust: what exists today

| Crate | Services | Status |
|-------|----------|--------|
| **pling** (v0.6.0, Jan 2026) | Telegram, Slack, Matrix, Webhook | Active, small scope |
| **messenger-rs** | Discord, Slack, Telegram | Low adoption, unclear maintenance |
| **rnotify** | Discord, Telegram, Mail | CLI-first, library secondary |

**Verdict:** Nothing in Rust comes close to nikoksr/notify. The existing crates cover 3-4 services each and none have the breadth or maturity to build on.

---

## Architectural Decision: Separate Crate vs. Integrated

**Recommendation: Start integrated, extract later.**

### Why NOT a separate crate from day one

1. **Premature abstraction risk** — You don't yet know the exact trait shape that works best for workflow's fire-and-forget, template-variable-driven notifications. Designing a "general purpose" API up front will either over-engineer (trying to match nikoksr/notify's 35 services) or under-serve workflow's specific needs (RunLog context, step failure details, YAML-driven config).

2. **Iteration speed** — Keeping it in-tree means you can refactor the trait, config format, and executor integration in a single commit. Once extracted, every change requires coordinating across repos + version bumps.

3. **Your notify system is opinionated** — It's tied to `HashMap<String, String>` template vars, workflow-specific status/exit_code/failed_steps semantics, and a config model that lives in `config.toml`. A generic crate wouldn't have these opinions.

### When to extract

Extract into `workflow-notify` (or a more generic `notify-rs`) once:
- The trait has stabilized across 5+ services
- Someone else wants to use it outside workflow
- The notification code exceeds ~2000 LOC and deserves its own test/CI matrix

### Crate structure when you do extract

```
workflow-notify/
├── Cargo.toml          # feature flags per service
├── src/
│   ├── lib.rs          # Notifier trait + MultiNotifier
│   ├── message.rs      # Message struct (subject, body, severity, fields)
│   ├── slack.rs        # feature = "slack"
│   ├── discord.rs      # feature = "discord"
│   ├── telegram.rs     # feature = "telegram"
│   ├── msteams.rs      # feature = "msteams"
│   ├── email.rs        # feature = "email"
│   ├── webhook.rs      # feature = "webhook"
│   ├── ntfy.rs         # feature = "ntfy"
│   └── gotify.rs       # feature = "gotify"
```

---

## Development Plan

### Phase 1: Trait Foundation + Refactor (1-2 weeks)

Replace the current string-based shell-out approach with a proper trait-based system using native HTTP.

#### 1.1 Define the core trait

```rust
// core/notify/mod.rs

use std::collections::HashMap;

/// Severity level for notification routing/formatting
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Severity {
    Success,
    Failure,
    Warning,
    Info,
}

/// A structured notification message
#[derive(Debug, Clone)]
pub struct Notification {
    pub subject: String,
    pub body: String,
    pub severity: Severity,
    /// Workflow-specific context (task_ref, exit_code, failed_steps, duration_ms, etc.)
    pub fields: HashMap<String, String>,
}

/// The core trait every backend implements
pub trait Notifier: Send + Sync {
    /// Human-readable name for logging ("slack", "discord", etc.)
    fn name(&self) -> &str;

    /// Send a notification. Implementations should handle their own retries.
    fn send(&self, notification: &Notification) -> Result<(), NotifyError>;
}

/// Sends to all registered notifiers, collecting errors
pub struct MultiNotifier {
    notifiers: Vec<Box<dyn Notifier>>,
}

impl MultiNotifier {
    pub fn new() -> Self { Self { notifiers: vec![] } }

    pub fn add(&mut self, n: Box<dyn Notifier>) { self.notifiers.push(n); }

    pub fn send_all(&self, notification: &Notification) -> Vec<NotifyError> {
        self.notifiers.iter()
            .filter_map(|n| n.send(notification).err())
            .collect()
    }
}
```

#### 1.2 Add `ureq` dependency (blocking HTTP, no async runtime needed)

```toml
# Cargo.toml — keep it simple, no tokio needed
ureq = { version = "3", features = ["json"] }
```

This is important: workflow's executor is synchronous (spawns bash processes via `std::process::Command`). Introducing tokio/async just for notifications would be a massive architectural change for no benefit. `ureq` is the right choice — it's what `pling` uses too.

#### 1.3 Refactor executor integration

Replace the current `run_notify()` shell-out in `executor.rs` with:

```rust
// Build Notification from RunLog + template vars (replaces build_notify_vars)
fn build_notification(run_log: &RunLog, workflow: &Workflow, vars: &HashMap<String, String>) -> Notification;

// In execute_workflow(), after completion:
let notifiers = build_notifiers_from_config(&config.notify, &workflow.notify);
let notification = build_notification(&run_log, &workflow, &template_vars);
let errors = notifiers.send_all(&notification);
for err in errors {
    eprintln!("[notify] {}: {}", err.service, err.message);
}
```

#### 1.4 Migrate existing backends

Rewrite the current three backends as trait implementations instead of string interpolation:

- `SlackWebhook` — native HTTP POST (replaces curl shell-out)
- `GenericWebhook` — native HTTP POST with JSON body
- `EmailNotifier` — keeps `mail` command for now (or switch to `lettre` crate)

**Backward-compatible config:** The `slack://`, `webhook://`, `email://` URL schemes in config.toml and workflow YAML continue to work — the resolver just constructs trait objects instead of bash strings.

---

### Phase 2: Discord + Telegram (1-2 weeks)

The two most-requested chat integrations beyond Slack.

#### 2.1 Discord via Webhooks

Discord webhooks are almost identical to Slack's — POST JSON to a URL. The payload format differs:

```rust
// core/notify/discord.rs
pub struct DiscordWebhook {
    webhook_url: String,
}

impl Notifier for DiscordWebhook {
    fn name(&self) -> &str { "discord" }

    fn send(&self, n: &Notification) -> Result<(), NotifyError> {
        let color = match n.severity {
            Severity::Success => 0x2ECC71,  // green
            Severity::Failure => 0xE74C3C,  // red
            Severity::Warning => 0xF39C12,  // orange
            Severity::Info    => 0x3498DB,  // blue
        };

        let embed = serde_json::json!({
            "embeds": [{
                "title": &n.subject,
                "description": &n.body,
                "color": color,
                "fields": n.fields.iter().map(|(k, v)| {
                    serde_json::json!({"name": k, "value": v, "inline": true})
                }).collect::<Vec<_>>(),
            }]
        });

        ureq::post(&self.webhook_url)
            .send_json(&embed)
            .map_err(|e| NotifyError::new("discord", e))?;
        Ok(())
    }
}
```

**Config:**
```toml
# config.toml
[notify]
on_failure = "discord://https://discord.com/api/webhooks/ID/TOKEN"
```

```yaml
# workflow YAML
notify:
  on_failure: "discord://https://discord.com/api/webhooks/ID/TOKEN"
```

#### 2.2 Telegram via Bot API

Requires a bot token + chat ID. Slightly different config pattern since it needs two values.

```rust
// core/notify/telegram.rs
pub struct TelegramBot {
    bot_token: String,
    chat_id: String,
}

impl Notifier for TelegramBot {
    fn name(&self) -> &str { "telegram" }

    fn send(&self, n: &Notification) -> Result<(), NotifyError> {
        let icon = match n.severity {
            Severity::Success => "✅",
            Severity::Failure => "❌",
            Severity::Warning => "⚠️",
            Severity::Info    => "ℹ️",
        };

        let text = format!("{icon} *{subject}*\n{body}",
            subject = escape_markdown(&n.subject),
            body = escape_markdown(&n.body),
        );

        let url = format!(
            "https://api.telegram.org/bot{}/sendMessage", self.bot_token
        );

        ureq::post(&url)
            .send_json(&serde_json::json!({
                "chat_id": &self.chat_id,
                "text": &text,
                "parse_mode": "MarkdownV2",
            }))
            .map_err(|e| NotifyError::new("telegram", e))?;
        Ok(())
    }
}
```

**Config:**
```toml
[notify]
on_failure = "telegram://BOT_TOKEN@CHAT_ID"
```

Or environment-variable based:
```toml
[notify]
on_failure = "telegram://$TELEGRAM_BOT_TOKEN@$TELEGRAM_CHAT_ID"
```

---

### Phase 3: Microsoft Teams + ntfy + Gotify (1-2 weeks)

#### 3.1 Microsoft Teams (Incoming Webhook / Workflow connector)

Teams deprecated the old Office 365 connector in late 2024 in favor of "Workflows" (Power Automate). The new approach uses an Adaptive Card JSON payload posted to a webhook URL.

```rust
// core/notify/msteams.rs
pub struct TeamsWebhook {
    webhook_url: String,
}
// Adaptive Card JSON format with severity-colored header
```

**Config:** `teams://https://TENANT.webhook.office.com/webhookb2/...`

#### 3.2 ntfy (self-hosted push notifications)

ntfy.sh is increasingly popular for homelab/sysadmin use — perfect for workflow's target audience. Dead simple: POST to a topic URL.

```rust
// core/notify/ntfy.rs
pub struct Ntfy {
    server_url: String,  // e.g., "https://ntfy.sh" or self-hosted
    topic: String,
    token: Option<String>,
}
// POST body as message, headers for title/priority/tags
```

**Config:** `ntfy://ntfy.sh/my-workflows` or `ntfy://my-server.local/topic?token=tk_xxx`

#### 3.3 Gotify (self-hosted)

Similar to ntfy, popular in self-hosted circles.

```rust
// core/notify/gotify.rs
pub struct Gotify {
    server_url: String,
    app_token: String,
}
// POST /message with title, message, priority
```

**Config:** `gotify://my-server.local?token=APP_TOKEN`

---

### Phase 4: Enhanced Config + Multi-Target (1 week)

#### 4.1 Multiple notification targets

Currently `on_failure` and `on_success` are single strings. Extend to support arrays:

```yaml
notify:
  on_failure:
    - "slack://https://hooks.slack.com/services/T/B/x"
    - "telegram://BOT@CHAT"
    - "ntfy://ntfy.sh/ops-alerts"
  on_success:
    - "ntfy://ntfy.sh/ops-info"
```

Backward-compatible: single string still works (deserialized as one-element vec).

#### 4.2 Severity-based routing

```yaml
notify:
  channels:
    - target: "slack://..."
      on: [failure, warning]
    - target: "telegram://..."
      on: [failure]
    - target: "ntfy://..."
      on: [success, failure, warning]
```

#### 4.3 Global defaults + per-workflow overrides (already works)

Keep the existing pattern where workflow-level `notify:` overrides global `config.toml` notify. Add merge semantics so a workflow can *add* targets without replacing the global list.

---

### Phase 5: Polish + Optional Extras (ongoing)

#### 5.1 Retry with backoff

```rust
pub struct RetryConfig {
    pub max_attempts: u32,      // default: 3
    pub initial_delay_ms: u64,  // default: 1000
    pub backoff_factor: f64,    // default: 2.0
}
```

Wrap each `Notifier::send()` call in a retry loop. Important for flaky networks / rate limits.

#### 5.2 Rate limiting

Per-service rate limiter to avoid hitting API limits (Discord: 30/min per webhook, Telegram: 30/sec to same chat).

#### 5.3 Rich formatting per service

Each backend can implement an optional `send_rich()` method that uses service-specific features:
- Slack: Block Kit with sections, buttons, action links
- Discord: Embeds with fields, colors, thumbnails
- Telegram: MarkdownV2 with inline code blocks for step output
- Teams: Adaptive Cards

#### 5.4 Additional services (community-driven)

Lower priority, add as needed:
- PagerDuty (incident creation for critical failures)
- Pushover (mobile push)
- Matrix (self-hosted chat)
- Google Chat (workspace users)
- Email via SMTP/lettre (replace `mail` shell-out)

---

## Priority-Ordered Service List

Based on sysadmin/devops usage patterns (workflow's core audience):

| Priority | Service | Effort | Why |
|----------|---------|--------|-----|
| 1 | **Slack** (refactor) | Low | Already exists, just migrate to native HTTP |
| 2 | **Discord** | Low | Nearly identical to Slack webhooks |
| 3 | **Telegram** | Low | Simple Bot API, very popular |
| 4 | **ntfy** | Low | Self-hosted crowd loves it, trivial API |
| 5 | **Webhook** (refactor) | Low | Already exists, migrate |
| 6 | **MS Teams** | Medium | Adaptive Cards are verbose but well-documented |
| 7 | **Gotify** | Low | Simple REST API |
| 8 | **Email/SMTP** | Medium | Replace shell-out with `lettre` crate |
| 9 | **PagerDuty** | Medium | Events API v2, incident management |
| 10 | **Matrix** | Medium | Requires auth flow, room management |

---

## File Layout (integrated, Phase 1-3)

```
src/core/notify/
├── mod.rs          # Notifier trait, MultiNotifier, Notification, build_notifiers_from_config()
├── message.rs      # Notification struct, Severity enum
├── error.rs        # NotifyError type
├── resolve.rs      # URL-scheme parser (slack://, discord://, telegram://, etc.)
├── slack.rs        # SlackWebhook impl
├── discord.rs      # DiscordWebhook impl
├── telegram.rs     # TelegramBot impl
├── msteams.rs      # TeamsWebhook impl
├── ntfy.rs         # Ntfy impl
├── gotify.rs       # Gotify impl
├── webhook.rs      # GenericWebhook impl
└── email.rs        # EmailNotifier impl (mail cmd or lettre)
```

The existing `core/notify.rs` gets replaced by `core/notify/mod.rs` which re-exports everything.

---

## Key Design Decisions

1. **Synchronous (ureq), not async** — workflow's executor is sync. No tokio dependency.
2. **Feature flags** — each service behind a cargo feature so users don't pull deps they don't need. Default features: slack, discord, webhook, ntfy.
3. **URL-scheme config** — keep the `service://` pattern. It's terse, works in both TOML and YAML, and is already familiar to users.
4. **Fire-and-forget with logging** — notification failures never block workflow execution. Log errors, move on.
5. **Template variable injection** — all services receive the same `HashMap<String, String>` context. Each service formats it appropriately.
6. **No async runtime tax** — this is critical. Adding tokio to a TUI app that uses crossterm event polling would be a significant architectural burden for marginal benefit.

---

## Estimated Timeline

| Phase | Scope | Duration |
|-------|-------|----------|
| 1 | Trait + refactor existing backends | 1-2 weeks |
| 2 | Discord + Telegram | 1-2 weeks |
| 3 | Teams + ntfy + Gotify | 1-2 weeks |
| 4 | Multi-target config + routing | 1 week |
| 5 | Retry, rate-limit, rich formatting | Ongoing |
| **Total MVP (Phases 1-3)** | | **3-6 weeks** |

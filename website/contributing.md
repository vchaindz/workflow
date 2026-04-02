# Contributing

Thanks for your interest in contributing to workflow.

## Getting Started

1. Fork and clone the repo
2. Build and verify:
   ```bash
   cargo build
   cargo test
   cargo clippy -- -D warnings
   ```

3. To build with MCP support:
   ```bash
   cargo build --features mcp
   cargo test --features mcp
   cargo clippy --features mcp -- -D warnings
   ```

## Architecture

The codebase is organized into three main areas:

```
src/
├── main.rs              # Entry point, CLI arg parsing (clap)
├── cli/                 # CLI subcommand dispatch and handlers
│   ├── args.rs          # All subcommand definitions
│   ├── mod.rs           # CLI dispatch logic
│   ├── ai_update.rs     # ai-update command handler
│   ├── sync.rs          # sync command handler
│   ├── memory.rs        # memory command handler
│   └── mcp.rs           # mcp command handler (feature-gated)
├── core/                # Engine: execution, parsing, all business logic
│   ├── discovery.rs     # Workflow file discovery (walkdir)
│   ├── parser.rs        # YAML parsing + DAG validation (Kahn's algorithm)
│   ├── executor.rs      # Step execution engine + dangerous command detection
│   ├── template.rs      # Template variable expansion + pipe filters
│   ├── ai.rs            # AI tool detection + invocation
│   ├── wizard.rs        # Workflow generation + optimization
│   ├── catalog.rs       # Template catalog (bundled + fetch)
│   ├── compare.rs       # Run comparison + metric extraction
│   ├── memory.rs        # Anomaly detection, baselines, trends
│   ├── db.rs            # SQLite history database
│   ├── config.rs        # Config file parsing
│   ├── sync.rs          # Git sync operations
│   ├── history.rs       # Shell history parsing (zsh/bash/fish)
│   ├── detect.rs        # Interactive command detection
│   ├── mcp.rs           # MCP client (feature-gated)
│   └── notify/          # Notification system
│       ├── mod.rs       # Notifier trait, MultiNotifier
│       ├── resolve.rs   # URL-scheme resolver
│       ├── message.rs   # Notification struct, Severity enum
│       ├── retry.rs     # Exponential backoff
│       ├── rate_limit.rs # Per-service rate limiting
│       └── *.rs         # Per-backend implementations
├── tui/                 # Terminal UI (ratatui)
│   ├── app.rs           # TUI state machine (AppMode, Focus, WizardState)
│   ├── actions.rs       # Keybinding handlers + wizard logic
│   └── ui.rs            # TUI rendering
└── error.rs             # Error types (DzError enum)
```

### Key design patterns

- **Entry point dispatch**: with a subcommand, `main.rs` dispatches to `cli/mod.rs`. Without a subcommand, it launches the TUI.
- **Error handling**: `error.rs` defines `DzError` with `From` impls for automatic conversion. All fallible functions return `error::Result<T>`.
- **Feature gating**: MCP support and individual notification backends are behind cargo feature flags.
- **Trait-based notifications**: the `Notifier` trait in `core/notify/mod.rs` allows adding new backends without modifying existing code.

## How to Add Things

### New CLI command

1. Add the command variant to `Commands` enum in `src/cli/args.rs`
2. Add the dispatch arm in `src/cli/mod.rs`
3. Implement the handler (in `src/cli/` or `src/core/`)
4. Add tests in `tests/cli_tests.rs`

### New notification backend

1. Create `src/core/notify/myservice.rs` implementing the `Notifier` trait
2. Gate it behind a feature flag in `Cargo.toml`
3. Add the URL scheme to `resolve.rs`
4. Add tests

### New template

1. Create a `.yaml` file under `templates/<category>/`
2. Include `name:`, `description:`, and a `variables:` section if needed
3. The template catalog is auto-generated at docs build time

## Submitting Changes

- Open an issue first for large changes so the approach can be discussed
- Keep PRs focused: one feature or fix per PR
- Add tests for new functionality (see `tests/` for examples)
- Run `cargo test && cargo clippy` before submitting

## Reporting Bugs

File a GitHub issue with:
- What you expected to happen
- What actually happened
- Steps to reproduce
- Your OS and workflow version (`workflow --version`)

## Code of Conduct

Be kind. Be constructive. We're all here to build useful tools.

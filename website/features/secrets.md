# Encrypted Secrets

An encrypted secrets store backed by `age` and your SSH key. No `.env` files, no plaintext tokens in config.

Secrets are encrypted at rest in `~/.config/workflow/secrets.age` with 0600 permissions, decrypted to memory only at runtime, and zeroized after use.

## CLI commands

```bash
# One-time setup (auto-detects ~/.ssh/id_ed25519)
workflow secrets init

# Store secrets
workflow secrets set DB_PASSWORD           # prompts for value securely
workflow secrets set API_TOKEN --value sk-live-abc123

# List and retrieve
workflow secrets list                      # names only
workflow secrets get DB_PASSWORD            # decrypt and print

# Remove
workflow secrets rm DB_PASSWORD
```

::: tip
`secrets init` auto-detects your SSH key at `~/.ssh/id_ed25519`. If you use a different key path, the init command will prompt you.
:::

## TUI secrets manager

Press `K` in the TUI to open the secrets manager without leaving the interface.

| Key | Action |
|-----|--------|
| `a` | Add a new secret (name + masked value input) |
| `v` / `Enter` | Decrypt and reveal a secret's value |
| `e` | Edit an existing secret's value |
| `d` | Delete a secret (with confirmation) |

If the secrets store has not been initialized yet, the modal offers to run `secrets init` automatically using your SSH key.

## Auto-injection into workflows

Declare required secrets in the workflow YAML. Values are injected from the store as environment variables at execution time:

```yaml
name: Deploy
secrets:
  - DB_PASSWORD
  - API_TOKEN
steps:
  - id: migrate
    cmd: DATABASE_URL="postgres://app:$DB_PASSWORD@db/prod" ./migrate.sh
```

::: warning
Secrets are injected as environment variables into the step's shell process. They are automatically redacted in live output and log files.
:::

## Precedence

When the same variable name is defined in multiple places, this precedence applies (highest to lowest):

1. Explicit `env:` block in the workflow YAML
2. `--env` CLI flag
3. Encrypted secrets store
4. Host environment variables

Secrets never override values you set explicitly. If the store does not exist or a secret is not found, the workflow falls back to environment variables silently.

## MCP server secret injection

MCP server aliases in `config.toml` can reference secrets by name. They are loaded from the encrypted store and injected as environment variables when the server process spawns:

```toml
[mcp.servers.github]
command = "npx -y @modelcontextprotocol/server-github"
secrets = ["GITHUB_TOKEN"]
```

See [MCP Integration](/features/mcp) for details on configuring MCP servers.

# AI integration

workflow treats AI CLI tools as first-class citizens. If any supported tool is on your `PATH`, you unlock generation, updating, fixing, and refinement capabilities directly from the TUI and CLI.

## Supported AI CLIs

| Tool | Detection | Used for |
|------|-----------|----------|
| [Claude Code](https://docs.anthropic.com/en/docs/claude-code) (`claude`) | `claude -p` | Generate, update, fix, refine |
| [Codex CLI](https://github.com/openai/codex) (`codex`) | `codex exec` | Generate, update, fix, refine |
| [Gemini CLI](https://github.com/google-gemini/gemini-cli) (`gemini`) | `gemini -p` | Generate, update, fix, refine |

Install any one of these and authenticate it. workflow auto-detects which is available at startup and shows it in the TUI header. No API keys need to be configured inside workflow itself.

## TUI capabilities

### Generate (`a`)

Describe a task in plain English:

> "Set up daily postgres backup with S3 upload and Slack notification on failure."

The AI generates executable YAML with proper step dependencies, error handling, and cleanup. Review the preview before saving.

### Update (`A`)

Select any existing task and describe what to change:

> "Add retry logic to the upload step"
> "Parallelize the independent checks"
> "Switch from rsync to rclone"

The AI rewrites the full YAML while preserving your structure.

### Fix (`a` after failure)

When a workflow fails, press `a` and the AI analyzes the error output, diagnoses the root cause, and proposes corrected YAML.

### Refine (`r` at preview)

Iteratively improve any AI-generated result before saving. Each round sends the current YAML plus your instructions back to the AI. Repeat as many times as needed:

```text
Preview --> r --> "add error handling" --> Enter --> (AI refines) --> Preview
                                                                       |
                                                            r --> "also add logging" --> Enter --> ...
```

Press `d` at any preview stage to dry-run the workflow without saving --- verify it works, then save or keep refining.

::: tip
The refine loop is the fastest way to iteratively build a production-quality workflow. Start with a rough description, then refine in multiple passes: error handling, retries, notifications, cleanup.
:::

## CLI usage

All AI capabilities are available from the command line:

```bash
# Rewrite a task with instructions
workflow ai-update backup/db-full --prompt "add error handling and retries"

# Preview changes without saving
workflow ai-update backup/db-full --prompt "parallelize steps" --dry-run

# Save as a new task instead of overwriting
workflow ai-update backup/db-full --prompt "add cleanup" --save-as db-full-v2
```

## AI-powered comparison

Compare consecutive runs with natural-language analysis:

```bash
workflow compare backup/db-full --ai
```

The AI receives the timing deltas, status changes, and output diffs, then provides a human-readable summary of what changed and why.

## Claude Code skill

A bundled Claude Code skill lets you manage workflows entirely from within Claude Code or Claude Code-powered agents. Install it:

```bash
mkdir -p ~/.claude/skills
ln -s "$(pwd)/skills/workflow-manager" ~/.claude/skills/workflow-manager
```

Then ask Claude naturally:

- "Create a workflow for daily database backups"
- "List my overdue tasks"
- "Dry-run the staging deploy"

Or use the explicit command: `/workflow-manager run backup/db-full --dry-run`.

This makes workflow a natural building block for agentic automation: AI agents can create, validate, and execute operational tasks through a well-defined file-based interface without any special APIs.

## MCP-awareness

When MCP servers are configured in `config.toml`, AI-generated workflows prefer `mcp:` steps over shell `curl`/API calls where appropriate. For example, instead of generating a `curl` command to create a GitHub issue, the AI will generate an `mcp:` step that calls the `create_issue` tool on the `github` server.

## Tool-agnostic design

The file-based YAML format means any AI tool --- not just the three supported CLIs --- can read and write workflow files directly. This makes workflow compatible with:

- AI coding agents that manipulate files
- LLM-powered automation pipelines
- Custom scripts that generate YAML programmatically

No special adapters or APIs are required. The filesystem is the interface.

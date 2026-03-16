---
name: research
description: Research a task and create or enrich a GitHub issue with comprehensive technical context. Use when asked to improve/enrich/research an issue, add details, analyze what's needed, or break down a task. Do NOT implement — research only. Usage - /research <issue description or #number>
---

# Dev Research Command

You are a research assistant for the development team. Your job is to research a task and create or enrich a GitHub issue with comprehensive technical context.

## Hard Rules

- **This command is RESEARCH ONLY.** Do NOT write code, create branches, or modify source files.
- The ONLY outputs are: GitHub issues, GitHub comments, and a summary to the user.
- If the user wants implementation after research — stop and suggest `/implement`.

## When This Command Applies

Use `/research` when the user asks to:
- "improve/enrich/flesh out this issue/task"
- "what needs to be done for #XX"
- "add more detail to the task"
- "break this down into subtasks"
- "research how to implement #XX"

Do NOT use this command when the user says:
- "implement", "fix", "build", "code" — use `/implement`
- "review" — use `/review`

## Input

$ARGUMENTS

## Determine Mode

Parse the input to determine what to do:

- **If input contains an issue reference** (e.g., `#42`, `enrich #42`, URL with `/issues/`): **Enrich mode** — read the existing issue and add research findings as a comment
- **If input is a short description** (e.g., `add heat sorting to templates`): **New issue mode** — research and create a detailed issue
- **If input contains `repo:owner/name`**: use that repo explicitly via `gh ... -R owner/name`
- **If repo is ambiguous**, infer from the current directory's `gh repo view` or ask the user

## Research Phase

Scale research depth to task complexity. Use all available tools.

### 1. Codebase Research (local)

The codebase is local — use local tools for code research:
- **Glob** — find relevant files by pattern (e.g., `**/executor*.rs`, `**/parser*.rs`)
- **Grep** — search for keywords, function names, types, error messages
- **Read** — read the most relevant files (max 3-5 files)
- Identify patterns, conventions, and tech stack used in the relevant area

**Key areas of this project:**
- Core pipeline: `src/core/` (discovery, parser, executor, models, template, logger, db)
- TUI: `src/tui/` (app state, actions/keybindings, UI rendering)
- CLI: `src/cli/` (args, dispatch, ai-update)
- Templates: `templates/` (embedded YAML workflows)
- Tests: `tests/` (integration tests with `assert_cmd` + `tempfile`)
- Error handling: `src/error.rs` (`DzError` enum)

### 2. Issue Research (via gh CLI)

Search for similar or duplicate issues — check both open and closed:

```bash
gh issue list --search "keyword" --state all --limit 10
gh issue view <number> --comments
```

Flag potential duplicates before creating new ones.

### 3. External Research (when relevant)

- Use **WebSearch** for best practices, Rust crate docs, etc.
- Skip for purely internal refactoring or simple tasks

## Issue Template

Structure the output using this template (adapt as needed — skip empty sections):

```markdown
## Summary
[1-2 sentences: what needs to be done and why]

## Context
[Background from research — why this matters, what prompted it]

## Current State
[What exists today — relevant code paths, current behavior, patterns found]

## Proposed Implementation
[Concrete approach based on research]

### Changes Required
- [ ] [Specific file/component change]
- [ ] [...]

### Key Files
- `src/core/file.rs` — [why relevant]
- `src/tui/app.rs` — [why relevant]

## Acceptance Criteria
- [ ] [Testable criterion]
- [ ] [...]

## Related Issues
- #XX — [relationship]

## Notes
[Edge cases, security considerations, performance implications, links]
```

## Create or Update

### New Issue Mode

**Step 1 — Draft the issue** based on research findings using the template above.

**Step 2 — Present the draft to the user for approval** before creating.

**Step 3 — Create the issue:**

```bash
gh issue create --title "<title>" --body-file /tmp/issue-body.md --label "<labels>"
```

If the body is long, write it to a temp file and use `--body-file`.

**Step 4 — For large tasks, break into sub-issues:**

Create each sub-issue first, then link it to the parent:

```bash
SUB_URL=$(gh issue create --title "<sub-title>" --body "<sub-body>" --label "<labels>" | tail -1)

PARENT_ID=$(gh issue view <parent-number> --json id --jq '.id')
gh api graphql -f query='
  mutation($parentId: ID!, $subIssueUrl: String!) {
    addSubIssue(input: { issueId: $parentId, subIssueUrl: $subIssueUrl }) {
      subIssue { url }
    }
  }
' -f parentId="$PARENT_ID" -f subIssueUrl="$SUB_URL"
```

### Enrich Mode

**IMPORTANT: In enrich mode your ONLY action on GitHub is adding a comment. You do NOT:**
- Edit the issue body
- Create branches or write code
- Start implementation

Steps:

1. Read the current issue:
   ```bash
   gh issue view <number> --comments
   ```
2. Research the codebase and external sources (same as Research Phase above)
3. Add research as a comment:
   ```bash
   gh issue comment <number> --body-file /tmp/research-comment.md
   ```
4. If the task should be broken down — create sub-issues and link them to the parent

## Report Back

After completing, print a concise summary:
- Link to the created/updated issue
- 2-3 key findings from research
- Any concerns or open questions
- Suggested next steps

## Guidelines

- Write issue content in English
- Write issues as if the implementer has no prior context
- Include specific file paths and code references from local codebase research
- Acceptance criteria must be testable
- Match the project's terminology style (look at recent issues via `gh issue list`)
- If similar issues exist — flag them, don't create duplicates
- Before creating an issue, present the draft to the user for approval
